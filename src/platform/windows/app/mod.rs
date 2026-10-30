// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app platform on Windows: the loop with no window and the windows on demand ([shell], over
//! a hidden window of its own for posts, session end and the tray's messages), the notification
//! area icon ([tray]) and the popup menus ([menu]), global hotkeys on a thread of their own
//! ([hotkey]), launch at login through the registry ([autostart]), links and files opened ([open]),
//! shortcuts posted to the app in front ([post]), where the user types ([focus]) and the clipboard
//! for a paste ([paste]), files fetched over HTTP ([http]) and an update's setup run ([update]);
//! the Downloads folder ([downloads]), the console of the command line ([attach_console]), the
//! uninstall's retries ([uninstall]) and a microphone's transport, the lid and their changes
//! ([hardware]).

pub mod activity;
pub mod autostart;
pub mod desktop;
pub mod focus;
pub mod hardware;
pub mod hotkey;
pub mod http;
pub mod menu;
pub mod open;
pub mod paste;
pub mod post;
pub mod shell;
pub mod tray;
pub mod uninstall;
pub mod update;

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::null_mut;

use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows_sys::Win32::UI::Shell::{FOLDERID_Downloads, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

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
