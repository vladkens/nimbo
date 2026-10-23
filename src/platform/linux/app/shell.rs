// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app on GTK: its main loop with no window, windows by id opened and closed on demand (the
//! window layer is the parent module), and work posted from other threads as GLib sources on
//! the default main context, which GLib makes safe to add from any thread and wakes for.
//!
//! Linux has no Dock: [Presence] changes nothing, the panel's task list shows the windows. A
//! second launch is a second process (the reopen hook never runs), and the session's
//! end (SIGTERM) is not caught: the quit hook runs after [quit] only.

use std::cell::Cell;
use std::ffi::c_int;
use std::sync::OnceLock;
use std::thread::{self, ThreadId};
use std::time::Duration;

use super::super::{FALSE, Fp, P, as_window, load, slot};
use crate::Config;
use crate::app::App;
use crate::shell::{AppInfo, Main, Presence};

thread_local! {
  /// The app running.
  static INFO: Cell<Option<AppInfo>> = const { Cell::new(None) };
}

/// The thread GTK runs on.
static MAIN: OnceLock<ThreadId> = OnceLock::new();

/// The app running ([crate::app]'s info); outside it, one named after the executable.
pub(super) fn info() -> AppInfo {
  INFO.get().unwrap_or(AppInfo { id: "nimbo", name: "nimbo", presence: Presence::Regular })
}

/// Opens the display, runs `start`, then GTK's loop until [quit]; the quit hook, then the
/// windows close.
pub fn run(info: &AppInfo, start: impl FnOnce(&mut Main)) {
  let _ = MAIN.set(thread::current().id());
  if let Err(e) = super::super::init(info.id) {
    eprintln!("nimbo: {e}");
    std::process::exit(1);
  }

  INFO.set(Some(*info));
  start(&mut Main::here());
  super::super::main_loop();
  crate::shell::quitting();
  for id in crate::shell::windows() {
    closed(id, false);
  }

  super::super::reap();
  INFO.set(None);
}

/// Opens a window running `app`, in front unless `cfg.hidden`.
pub fn open(cfg: &Config, app: Box<dyn App>) -> u64 {
  super::super::open(cfg, app).unwrap_or_else(|e| panic!("nimbo: the window title: {e}"))
}

pub fn front(id: u64) {
  super::super::front(id);
}

pub fn close(id: u64) {
  closed(id, true);
}

/// Window `id` goes ([super::super::close]), then its close callback runs.
fn closed(id: u64, defer: bool) {
  let on_close = crate::shell::closed(id);
  super::super::close(id, defer);
  if let Some(f) = on_close {
    f(&mut Main::here());
  }
}

/// Runs `f` on window `id`'s app if it is not busy, then redraws the window.
pub fn with_app(id: u64, f: &mut dyn FnMut(&mut dyn App)) -> bool {
  let Some(win) = slot(id, |s| s.win.clone()) else { return false };
  as_window(id, || {
    let Ok(mut w) = win.try_borrow_mut() else { return false };
    f(&mut *w.rt.app);
    w.rt.redraw();
    w.sync();
    true
  })
}

pub fn set_presence(_: Presence) {}

/// A second launch is a second process: never called.
pub fn on_reopen(_: Box<dyn FnMut(&mut Main)>) {}

pub fn terminating() -> bool {
  false
}

pub fn quit() {
  super::super::quit();
}

pub fn is_main() -> bool {
  MAIN.get() == Some(&thread::current().id())
}

type Posted = Box<dyn FnOnce() + Send>;

/// Runs `f` on the main thread after `delay` (at most `u32::MAX` ms, GLib's timeout: about 49
/// days): an idle or timeout source, from any thread.
pub fn post(delay: Duration, f: Posted) {
  let Some(g) = load() else { return };
  let data = Box::into_raw(Box::new(f)) as P;
  // SAFETY: GLib's source functions are thread-safe; `on_post` takes the box back once.
  unsafe {
    match delay.is_zero() {
      true => (g.g_idle_add)(on_post as Fp, data),
      false => {
        let ms = delay.as_micros().div_ceil(1000).min(u32::MAX as u128) as u32;
        (g.g_timeout_add)(ms, on_post as Fp, data)
      }
    };
  }
}

unsafe extern "C" fn on_post(data: P) -> c_int {
  // SAFETY: the box `post` leaked, run once (FALSE removes the source).
  let f = unsafe { Box::from_raw(data as *mut Posted) };
  f();
  FALSE
}
