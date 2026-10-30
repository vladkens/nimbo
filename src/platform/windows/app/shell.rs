// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app on Win32: the message loop with no window ([run]), the windows by id over the
//! platform's windows by handle, and a hidden top-level window of the shell's own (never shown,
//! out of the taskbar) that runs posted work ([post]: a message carrying the boxed closure, or a
//! timer for a delay), takes the tray's callbacks and the broadcasts only top-level windows get:
//! the session ending (`WM_ENDSESSION`, which runs the quit hook and exits), `TaskbarCreated`
//! (Explorer restarted: the tray icons are added again) and colour changes (the icons retinted).
//! Modal loops (a menu, a live resize) dispatch its messages too, so posts run under them.
//!
//! Windows has no Dock: the taskbar shows the windows only, so [set_presence] changes nothing; a
//! second launch starts a second process (the reopen hook never runs yet).

use std::any::TypeId;
use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::time::Duration;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Power::POWERBROADCAST_SETTING;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::HiDpi::{
  DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::super::{Timer, create, run_loop, settings, show_overlay, wide, with_window};
use super::{hardware, paste, tray};
use crate::Config;
use crate::app::{App, Event};
use crate::shell::{AppInfo, Main, Presence};

type Job = Box<dyn FnOnce() + Send>;

/// Work from any thread: lParam is a `Box<Job>`.
const WM_POST: u32 = WM_APP + 2;
/// The tray icons' callback message ([tray]).
pub(super) const WM_TRAY: u32 = WM_APP + 3;

#[derive(Default)]
struct Shell {
  /// The windows by id.
  windows: Vec<(u64, HWND)>,
  next: u64,
  /// Work waiting for its delay, by timer id.
  later: Vec<(usize, Job)>,
  next_timer: usize,
  quitting: bool,
  /// The message loop runs: [quit] posts WM_QUIT to end it.
  looping: bool,
  terminating: bool,
}

thread_local! {
  static SHELL: RefCell<Shell> = RefCell::default();
}

fn shell<R>(f: impl FnOnce(&mut Shell) -> R) -> R {
  SHELL.with_borrow_mut(f)
}

/// The thread `run` runs on.
static MAIN: AtomicU32 = AtomicU32::new(0);
/// The shell's window, as a number for other threads; 0 when there is none. An atomic, not a
/// lock: [post] runs in callbacks that must never wait (Windows' audio notifications). A post
/// that read the handle just before the loop ended sends to the window being destroyed: the
/// message fails and the work is dropped, or goes with the window and its closure leaks, once,
/// as the app ends; work posted after the loop is dropped anyway.
static HOST: AtomicIsize = AtomicIsize::new(0);

/// The shell's window: null while no app runs.
pub(in super::super) fn host() -> HWND {
  HOST.load(Ordering::Acquire) as HWND
}

pub fn is_main() -> bool {
  MAIN.load(Ordering::Relaxed) == unsafe { GetCurrentThreadId() }
}

/// Sets the app up, runs `start`, then the message loop until [quit].
pub fn run(info: &AppInfo, start: impl FnOnce(&mut Main)) {
  MAIN.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
  unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
  shell(|s| *s = Shell::default());
  let hwnd = make_host(info.name);
  HOST.store(hwnd as isize, Ordering::Release);

  start(&mut Main::here());
  if !shell(|s| s.quitting) {
    shell(|s| s.looping = true);
    run_loop(&Timer::new());
  }

  crate::shell::finish(close);
  tray::removed();
  // Work posted after the loop is dropped unrun: the closures are let go here.
  HOST.store(0, Ordering::Release);

  let mut msg: MSG = unsafe { std::mem::zeroed() };
  while unsafe { PeekMessageW(&mut msg, hwnd, WM_POST, WM_POST, PM_REMOVE) } != 0 {
    drop(unsafe { Box::from_raw(msg.lParam as *mut Job) });
  }

  unsafe { DestroyWindow(hwnd) };
  SHELL.take();
}

/// The shell's hidden window: top-level, so broadcasts reach it, as a tool window out of the
/// taskbar and Alt-Tab.
fn make_host(name: &str) -> HWND {
  let hinstance = unsafe { GetModuleHandleW(null()) };
  let class = wide("nimbo-app");
  let wc = WNDCLASSW {
    lpfnWndProc: Some(host_proc),
    hInstance: hinstance,
    lpszClassName: class.as_ptr(),
    ..unsafe { std::mem::zeroed() }
  };
  unsafe { RegisterClassW(&wc) };
  let title = wide(name);
  let (ex, style) = (WS_EX_TOOLWINDOW, WS_POPUP);
  let hwnd = unsafe {
    CreateWindowExW(
      ex,
      class.as_ptr(),
      title.as_ptr(),
      style,
      0,
      0,
      0,
      0,
      null_mut(),
      null_mut(),
      hinstance,
      null(),
    )
  };
  assert!(!hwnd.is_null(), "nimbo: CreateWindowExW failed for the app's window");
  hwnd
}

/// Ends the loop after the message in progress; the windows close after it.
pub fn overlay_takes_focus() -> bool {
  false
}

pub fn quit() {
  let looping = shell(|s| !std::mem::replace(&mut s.quitting, true) && s.looping);
  if looping {
    unsafe { PostQuitMessage(0) };
  }
}

/// Windows has no Dock: the taskbar follows the windows (see the module docs).
pub fn set_presence(_: Presence) {}

pub fn terminating() -> bool {
  shell(|s| s.terminating)
}

/// A message box of its own, topmost, with the system's warning sound and icon.
pub fn alert(title: &str, message: &str) {
  let (title, message) = (wide(title), wide(message));
  let style = MB_OK | MB_ICONWARNING | MB_TOPMOST | MB_SETFOREGROUND;
  unsafe { MessageBoxW(null_mut(), message.as_ptr(), title.as_ptr(), style) };
}

/// Runs `f` on the main thread after `delay`: a message to the shell's window, which schedules a
/// timer for a delay. Dropped when no app runs.
pub fn post(delay: Duration, f: Job) {
  let job: Job = match delay.is_zero() {
    true => f,
    false => Box::new(move || later(delay, f)),
  };

  let raw = Box::into_raw(Box::new(job));
  let h = host();
  let sent = !h.is_null() && unsafe { PostMessageW(h, WM_POST, 0, raw as LPARAM) } != 0;
  if !sent {
    drop(unsafe { Box::from_raw(raw) });
  }
}

/// Runs `f` once `delay` passed: a timer of the shell's window. On the main thread.
fn later(delay: Duration, f: Job) {
  let id = shell(|s| {
    s.next_timer += 1;
    s.later.push((s.next_timer, f));
    s.next_timer
  });
  let ms = delay.as_millis().clamp(1, u32::MAX as u128) as u32;
  unsafe { SetTimer(host(), id, ms, None) };
}

/// The shell's window: posted work, delays, the tray, the session's end, colour changes.
unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
  match msg {
    WM_POST => {
      // SAFETY: `post` boxed it for this message, which is taken once.
      let job = unsafe { Box::from_raw(lp as *mut Job) };
      job();
      0
    }
    WM_TIMER => {
      unsafe { KillTimer(hwnd, wp) };
      let job = shell(|s| {
        let i = s.later.iter().position(|(id, _)| *id == wp)?;
        Some(s.later.remove(i).1)
      });
      if let Some(job) = job {
        job();
      }

      0
    }
    WM_TRAY => {
      tray::notify(wp, lp);
      0
    }
    WM_POWERBROADCAST if wp == PBT_POWERSETTINGCHANGE as WPARAM => {
      // SAFETY: the message's setting, a header and its data.
      hardware::power(unsafe { &*(lp as *const POWERBROADCAST_SETTING) });
      1
    }
    WM_RENDERFORMAT | WM_RENDERALLFORMATS => {
      paste::render(hwnd, msg == WM_RENDERALLFORMATS);
      0
    }
    WM_DESTROYCLIPBOARD => {
      paste::destroyed();
      0
    }
    WM_QUERYENDSESSION => 1,
    WM_ENDSESSION if wp != 0 => {
      // The session ends: the process may be ended at any moment after this returns.
      shell(|s| s.terminating = true);
      crate::shell::finish(close);
      std::process::exit(0);
    }
    m if m == tray::taskbar_created() => {
      tray::added_again();
      0
    }
    m if settings::changed(m, lp) => {
      tray::retint();
      unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }
    _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
  }
}

// MARK: Windows

/// Opens a window running `app` (an app of type `ty`), in front unless `cfg.hidden`.
pub fn open(cfg: &Config, app: Box<dyn App>, ty: TypeId) -> u64 {
  let hwnd = create(cfg, app);
  let id = shell(|s| {
    s.next += 1;
    s.windows.push((s.next, hwnd));
    s.next
  });
  crate::shell::opened(id, ty);
  if !cfg.hidden {
    front(id);
  }

  id
}

fn hwnd(id: u64) -> Option<HWND> {
  shell(|s| s.windows.iter().find(|w| w.0 == id).map(|w| w.1))
}

/// Shows the window, restored if minimized, and asks for the foreground (which Windows grants
/// when the app has it or the user just used it: a tray menu, not a hotkey from the background).
pub fn front(id: u64) {
  let Some(hwnd) = hwnd(id) else { return };
  if let Some(anchor) = with_window(hwnd, |w| w.overlay).flatten() {
    return show_overlay(hwnd, anchor);
  }

  let show = if unsafe { IsIconic(hwnd) } != 0 { SW_RESTORE } else { SW_SHOW };
  super::super::shown(hwnd);
  unsafe {
    ShowWindow(hwnd, show);
    SetForegroundWindow(hwnd);
  }
}

pub fn hide(id: u64) {
  if let Some(hwnd) = hwnd(id) {
    unsafe { ShowWindow(hwnd, SW_HIDE) };
    with_window(hwnd, |w| w.hidden = true);
  }
}

/// Closes window `id`: WM_DESTROY drops its runtime and runs its close callback ([destroyed]).
pub fn close(id: u64) {
  if let Some(hwnd) = hwnd(id) {
    unsafe { DestroyWindow(hwnd) };
  }
}

/// Window `hwnd` was destroyed: its entry goes, then its close callback runs. Nothing for a
/// window the shell didn't open.
pub(in super::super) fn destroyed(hwnd: HWND) {
  let id = shell(|s| {
    let i = s.windows.iter().position(|w| w.1 == hwnd)?;
    Some(s.windows.remove(i).0)
  });
  if let Some(f) = id.and_then(crate::shell::closed) {
    f(&mut Main::here());
  }
}

/// Runs `f` on window `id`'s app if it is not busy, then redraws the window.
/// Sends `ev` to window `id`'s runtime, its modal timer following; `None` when it is busy up
/// the stack or gone.
pub fn send(id: u64, ev: Event) -> Option<bool> {
  with_window(hwnd(id)?, |w| {
    let used = w.send(ev);
    w.sync_modal();
    used
  })
}

pub fn with_app(id: u64, f: &mut dyn FnMut(&mut dyn App)) -> bool {
  let Some(hwnd) = hwnd(id) else { return false };
  let done = with_window(hwnd, |w| {
    f(&mut *w.rt.app);
    w.rt.redraw();
    // In a live resize's or a menu's loop, frames come from its timer: started for the redraw.
    w.sync_modal();
  });
  done.is_some()
}
