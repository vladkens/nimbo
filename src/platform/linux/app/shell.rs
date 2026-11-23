// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app on GTK: its main loop with no window, windows by id opened and closed on demand (the
//! window layer is the parent module), and work posted from other threads as GLib sources on
//! the default main context, which GLib makes safe to add from any thread and wakes for.
//!
//! Linux has no Dock: [Presence] changes nothing, the panel's task list shows the windows. A
//! second launch is a second process (the reopen hook runs for its "open" to the instance
//! [Main::serve] keeps), and the session's end (SIGTERM) is not caught: the quit hook runs after
//! [quit] only.

use std::any::TypeId;
use std::ffi::{c_char, c_int};
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::thread::{self, ThreadId};
use std::time::Duration;

use super::super::dbus::cstr;
use super::super::{FALSE, Fp, P, as_window, lib, load, slot};
use super::running;
use crate::app::{App, Event};
use crate::commands::Bar;
use crate::shell::{AppInfo, Main, OnChosen, Presence};
use crate::{Config, shell};

/// The thread GTK runs on.
static MAIN: OnceLock<ThreadId> = OnceLock::new();

/// Opens the display, runs `start`, then GTK's loop until [quit]; the quit hook, then the
/// windows close.
pub fn run(info: &AppInfo, start: impl FnOnce(&mut Main)) {
  let _ = MAIN.set(thread::current().id());
  if let Err(e) = super::super::init(info.id) {
    eprintln!("nimbo: {e}");
    std::process::exit(1);
  }

  super::INFO.set(Some(*info));
  start(&mut Main::here());
  super::super::started();
  super::super::main_loop();
  shell::finish(|id| closed(id, false));

  super::super::reap();
  super::INFO.set(None);
}

/// Opens a window running `app` (an app of type `ty`), in front unless `cfg.hidden`.
pub fn open(cfg: &Config, app: Box<dyn App>, ty: TypeId) -> u64 {
  let id = super::super::open(cfg, app).unwrap_or_else(|e| panic!("nimbo: the window title: {e}"));
  shell::opened(id, ty);
  id
}

pub fn front(id: u64) {
  super::super::front(id);
}

pub fn hide(id: u64) {
  super::super::hide(id);
}

pub fn close(id: u64) {
  closed(id, true);
}

/// Window `id` goes ([super::super::close]), then its close callback runs.
fn closed(id: u64, defer: bool) {
  let on_close = shell::closed(id);
  super::super::close(id, defer);
  if let Some(f) = on_close {
    f(&mut Main::here());
  }
}

/// Sends `ev` to window `id`'s runtime; `None` when it is busy up the stack or gone.
pub fn send(id: u64, ev: Event) -> Option<bool> {
  let win = slot(id, |s| s.win.clone())?;
  as_window(id, || {
    let mut w = win.try_borrow_mut().ok()?;
    let used = w.send(ev);
    w.sync();
    Some(used)
  })
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

/// No menu bar: the windows bind the commands' keys ([crate::App::keymap]).
pub fn set_commands(_: &dyn Fn(&str) -> Bar, _: OnChosen) {}

pub fn terminating() -> bool {
  false
}

/// A GTK message dialog run modally (its constructors are variadic: looked up apart from
/// [super::super::Gtk]); stderr while no app runs or GTK isn't there.
pub fn alert(title: &str, message: &str) {
  type New = unsafe extern "C" fn(P, c_int, c_int, c_int, *const c_char, ...) -> P;
  type Secondary = unsafe extern "C" fn(P, *const c_char, ...);
  let found = lib().filter(|_| running()).zip(load()).and_then(|(lib, g)| unsafe {
    let new = lib.sym::<New>("gtk_message_dialog_new\0")?;
    Some((new, lib.sym::<Secondary>("gtk_message_dialog_format_secondary_text\0")?, g))
  });
  let Some((new, secondary, g)) = found else { return eprintln!("{title}: {message}") };
  let (title, message) = (cstr(title), cstr(message));
  // `GTK_DIALOG_MODAL`, `GTK_MESSAGE_WARNING`, `GTK_BUTTONS_OK`; the texts through "%s".
  unsafe {
    let dialog = new(null_mut(), 1, 1, 1, c"%s".as_ptr(), title.as_ptr());
    secondary(dialog, c"%s".as_ptr(), message.as_ptr());
    (g.gtk_dialog_run)(dialog);
    (g.gtk_widget_destroy)(dialog);
  }
}

pub fn overlay_takes_focus() -> bool {
  super::super::overlay::takes_focus()
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
