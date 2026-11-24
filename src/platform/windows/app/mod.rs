// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app platform on Windows: the loop with no window and the windows on demand ([shell], over
//! a hidden window of its own for posts, session end and the tray's messages), the notification
//! area icon ([tray]) and the popup menus ([menu]), global hotkeys on a thread of their own
//! ([hotkey]), launch at login through the registry ([autostart]), links and files opened ([open]),
//! shortcuts posted to the app in front ([post]), where the user types ([focus]) and the clipboard
//! for a paste ([paste]); the Downloads folder ([downloads]), the console of the command line
//! ([attach_console]), the uninstall's retries ([uninstall]) and a microphone's transport, the
//! lid and their changes ([hardware]).

pub mod activity;
#[cfg(feature = "autostart")]
pub mod autostart;
#[cfg(feature = "autostart")]
pub mod desktop;
pub mod focus;
#[cfg(feature = "system")]
pub mod hardware;
#[cfg(feature = "hotkey")]
pub mod hotkey;
#[cfg(feature = "system")]
pub mod input_source;
pub mod menu;
pub mod open;
pub mod paste;
#[cfg(feature = "paste")]
pub mod post;
pub mod shell;
#[cfg(feature = "tray")]
pub mod tray;
#[cfg(feature = "autostart")]
pub mod uninstall;

use std::cell::Cell;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows_sys::Win32::System::Ole::OleInitialize;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
  VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
};
use windows_sys::Win32::UI::Shell::{FOLDERID_Downloads, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

use crate::Code;

/// The shell's window ([shell::run]'s), as a number for other threads; 0 when there is none. An
/// atomic, not a lock: [shell::post] runs in callbacks that must never wait (Windows' audio
/// notifications). A post that read the handle just before the loop ended sends to the window
/// being destroyed: the message fails and the work is dropped, or goes with the window and its
/// closure leaks, once, as the app ends; work posted after the loop is dropped anyway.
static HOST: AtomicIsize = AtomicIsize::new(0);

/// The shell's window: null while no app runs.
pub(super) fn host() -> HWND {
  HOST.load(Ordering::Acquire) as HWND
}

/// Starts OLE on this thread, once (it lasts as long as the app): the apartment drops need
/// ([super::files]), so it comes before any other COM start here ([hardware]).
pub(super) fn ole() {
  thread_local!(static OLE: Cell<bool> = const { Cell::new(false) });
  if !OLE.replace(true) {
    unsafe { OleInitialize(null()) };
  }
}

/// The user's Downloads, the Known Folder (wherever the user moved it): [crate::paths::downloads].
pub fn downloads() -> Option<PathBuf> {
  let mut s = null_mut::<u16>();
  // SAFETY: a known folder's id and a pointer for the string the shell allocates.
  let ok = unsafe {
    SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT as u32, null_mut(), &mut s)
  } >= 0;
  // SAFETY: a NUL-terminated string the shell allocated (also on failure), freed after the copy.
  let path = (ok && !s.is_null()).then(|| {
    let n = (0..).take_while(|&i| unsafe { *s.add(i) } != 0).count();
    PathBuf::from(OsString::from_wide(unsafe { std::slice::from_raw_parts(s, n) }))
  });
  unsafe { CoTaskMemFree(s.cast()) };
  path
}

/// The parent's console, if it has one: [crate::attach_console].
pub fn attach_console() {
  // SAFETY: no preconditions; it fails harmlessly without a parent console.
  unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

/// The posted events' `dwExtraInfo`: the hotkey hook leaves them to the app in front.
const POSTED: usize = u32::from_be_bytes(*b"nmbo") as usize;

/// The left or right modifier key at `code`.
fn lone_vk(code: Code) -> Option<u32> {
  let vk = match code {
    Code::ControlLeft => VK_LCONTROL,
    Code::ControlRight => VK_RCONTROL,
    Code::ShiftLeft => VK_LSHIFT,
    Code::ShiftRight => VK_RSHIFT,
    Code::AltLeft => VK_LMENU,
    Code::AltRight => VK_RMENU,
    Code::MetaLeft => VK_LWIN,
    Code::MetaRight => VK_RWIN,
    _ => return None,
  };

  Some(vk as u32)
}
