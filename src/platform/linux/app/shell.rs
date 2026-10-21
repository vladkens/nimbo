// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app on GTK: its main loop with no window, windows by id opened and closed on demand (the
//! window layer is the parent module), and work posted from other threads as GLib sources on
//! the default main context, which GLib makes safe to add from any thread and wakes for.
//!
//! Linux has no Dock: [Presence] changes nothing, the panel's task list shows the windows. A
//! second launch is a second process ([on_reopen] is kept, never called), and the session's
//! end (SIGTERM) is not caught: the quit hook runs after [quit] only.

use std::any::TypeId;
use std::cell::RefCell;
use std::ffi::c_int;
use std::sync::OnceLock;
use std::thread::{self, ThreadId};
use std::time::Duration;

use super::super::{FALSE, Fp, P, as_window, load, slot};
use crate::Config;
use crate::app::App;
use crate::shell::{AppInfo, Main, Presence};

type OnMain = Box<dyn FnOnce(&mut Main)>;
type OnReopen = Box<dyn FnMut(&mut Main)>;

/// An open window's type, for [type_of], and its close callback.
struct Open {
  id: u64,
  ty: TypeId,
  on_close: Option<OnMain>,
}

#[derive(Default)]
struct Shell {
  info: Option<AppInfo>,
  windows: Vec<Open>,
  reopen: Option<OnReopen>,
  on_quit: Option<OnMain>,
}

thread_local! {
  static SHELL: RefCell<Shell> = RefCell::default();
}

/// The thread GTK runs on.
static MAIN: OnceLock<ThreadId> = OnceLock::new();

fn shell<R>(f: impl FnOnce(&mut Shell) -> R) -> R {
  SHELL.with_borrow_mut(f)
}

/// The app running ([crate::app]'s info); outside it, one named after the executable.
pub(super) fn info() -> AppInfo {
  let info = shell(|s| s.info);
  info.unwrap_or(AppInfo { id: "nimbo", name: "nimbo", presence: Presence::Regular })
}

/// Opens the display, runs `start`, then GTK's loop until [quit]; the quit hook, then the
/// windows close.
pub fn run(info: &AppInfo, start: impl FnOnce(&mut Main)) {
  let _ = MAIN.set(thread::current().id());
  if let Err(e) = super::super::init(info.id) {
    eprintln!("nimbo: {e}");
    std::process::exit(1);
  }

  shell(|s| *s = Shell { info: Some(*info), ..Shell::default() });
  start(&mut Main::here());
  super::super::main_loop();
  if let Some(f) = shell(|s| s.on_quit.take()) {
    f(&mut Main::here());
  }

  for id in shell(|s| s.windows.iter().map(|w| w.id).collect::<Vec<_>>()) {
    closed(id, false);
  }

  shell(|s| s.info = None);
}

/// Opens a window running `app` (of type `ty`), in front unless `cfg.hidden`.
pub fn open(cfg: &Config, app: Box<dyn App>, ty: TypeId) -> u64 {
  let id = super::super::open(cfg, app).unwrap_or_else(|e| panic!("nimbo: the window title: {e}"));
  shell(|s| s.windows.push(Open { id, ty, on_close: None }));
  id
}

pub fn type_of(id: u64) -> Option<TypeId> {
  shell(|s| s.windows.iter().find(|w| w.id == id).map(|w| w.ty))
}

pub fn front(id: u64) {
  super::super::front(id);
}

pub fn close(id: u64) {
  closed(id, true);
}

/// Window `id` goes ([super::super::close]), then its close callback runs.
fn closed(id: u64, defer: bool) {
  let open = shell(|s| Some(s.windows.remove(s.windows.iter().position(|w| w.id == id)?)));
  super::super::close(id, defer);
  if let Some(f) = open.and_then(|o| o.on_close) {
    f(&mut Main::here());
  }
}

pub fn on_close(id: u64, f: OnMain) {
  shell(|s| {
    if let Some(w) = s.windows.iter_mut().find(|w| w.id == id) {
      w.on_close = Some(f);
    }
  });
}

/// Runs `f` on window `id`'s app if it is of type `ty` and not busy, then redraws the window.
pub fn with_app(id: u64, ty: TypeId, f: &mut dyn FnMut(&mut dyn App)) -> bool {
  if type_of(id) != Some(ty) {
    return false;
  }

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

pub fn on_reopen(f: OnReopen) {
  shell(|s| s.reopen = Some(f));
}

pub fn on_quit(f: OnMain) {
  shell(|s| s.on_quit = Some(f));
}

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

/// Runs `f` on the main thread after `delay`: an idle or timeout source, from any thread.
pub fn post(delay: Duration, f: Posted) {
  let Some(g) = load() else { return };
  let data = Box::into_raw(Box::new(f)) as P;
  // SAFETY: GLib's source functions are thread-safe; `on_post` takes the box back once.
  unsafe {
    match delay.is_zero() {
      true => (g.g_idle_add)(on_post as Fp, data),
      false => (g.g_timeout_add)(delay.as_micros().div_ceil(1000) as u32, on_post as Fp, data),
    };
  }
}

unsafe extern "C" fn on_post(data: P) -> c_int {
  // SAFETY: the box `post` leaked, run once (FALSE removes the source).
  let f = unsafe { Box::from_raw(data as *mut Posted) };
  f();
  FALSE
}
