// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What an app needs on Linux besides its windows: GTK's loop with no window and windows on
//! demand ([shell]), a StatusNotifierItem with its menu ([tray]), hotkeys through the
//! GlobalShortcuts portal ([hotkey]), an XDG autostart entry ([autostart]), links and files opened
//! ([open]), shortcuts posted to the app in focus and where the user types on X11 ([post],
//! [focus]) and the clipboard for a paste ([paste]); the tray, the portal and the file manager
//! speak D-Bus through GIO ([super::dbus]).

pub mod autostart;
pub mod focus;
pub mod hotkey;
mod menu;
pub mod open;
pub mod paste;
pub mod post;
pub mod shell;
pub mod tray;

use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong};
use std::sync::OnceLock;

use super::P;
use super::dl::{Lib, fns};

// Xlib, for posted keys and the active window on X11.
fns!(X11 {
  fn XOpenDisplay(name: *const c_char) -> P;
  fn XCloseDisplay(d: P) -> c_int;
  fn XQueryKeymap(d: P, keys: *mut u8) -> c_int;
  fn XKeysymToKeycode(d: P, sym: c_ulong) -> u8;
  fn XkbKeycodeToKeysym(d: P, code: u8, group: c_int, level: c_int) -> c_ulong;
  fn XkbGetState(d: P, device: c_uint, state: *mut u8) -> c_int;
  fn XFlush(d: P) -> c_int;
  fn XDisplayKeycodes(d: P, min: *mut c_int, max: *mut c_int) -> c_int;
  fn XDefaultRootWindow(d: P) -> c_ulong;
  fn XInternAtom(d: P, name: *const c_char, only_if_exists: c_int) -> c_ulong;
  fn XGetWindowProperty(
    d: P,
    w: c_ulong,
    property: c_ulong,
    offset: c_long,
    length: c_long,
    delete: c_int,
    kind: c_ulong,
    actual_kind: *mut c_ulong,
    actual_format: *mut c_int,
    items: *mut c_ulong,
    after: *mut c_ulong,
    data: *mut *mut u8,
  ) -> c_int;
  fn XFree(p: P) -> c_int;
});

/// `libX11`, loaded at the first use.
fn x11() -> Option<&'static X11> {
  static X11: OnceLock<Option<X11>> = OnceLock::new();
  X11.get_or_init(|| X11::load(&Lib::open(&[c"libX11.so.6"])?)).as_ref()
}
