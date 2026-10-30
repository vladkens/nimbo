// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What an app needs on Linux besides its windows: GTK's loop with no window and windows on demand
//! ([shell]), a StatusNotifierItem with its menu ([tray]), hotkeys through X11's grab ([grab]) or
//! the GlobalShortcuts portal on Wayland ([hotkey]), a window's context menus ([gtk_menu]), an XDG
//! autostart entry ([autostart]) and launcher entry ([desktop]), links and files opened ([open]),
//! shortcuts posted to the app in focus ([post]) and where the user types on X11 ([focus]), and the
//! clipboard for a paste ([paste]), both through the RemoteDesktop portal on Wayland ([remote]);
//! the tray, the portals and the file manager speak D-Bus through GIO ([super::dbus]); what an
//! uninstall removes ([uninstall]); no hardware read yet ([hardware]).

pub mod activity;
pub mod autostart;
pub mod desktop;
pub mod focus;
mod grab;
pub(super) mod gtk_menu;
pub mod hardware;
pub mod hotkey;
mod menu;
pub mod open;
pub mod paste;
pub mod post;
mod remote;
pub mod shell;
pub mod tray;
pub mod uninstall;

use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use super::dl::{Lib, fns};
use super::{Fp, P};

// Xlib, for posted keys, the hotkey's grab and the active window on X11.
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
  fn XSync(d: P, discard: c_int) -> c_int;
  fn XSetErrorHandler(handler: Fp) -> Fp;
  fn XGrabKey(d: P, code: c_int, mods: c_uint, w: c_ulong, owner: c_int, pointer: c_int,
    keyboard: c_int) -> c_int;
  fn XUngrabKey(d: P, code: c_int, mods: c_uint, w: c_ulong) -> c_int;
  fn XPending(d: P) -> c_int;
  fn XNextEvent(d: P, event: *mut [c_long; 24]) -> c_int;
  fn XConnectionNumber(d: P) -> c_int;
  fn XkbSetDetectableAutoRepeat(d: P, on: c_int, supported: *mut c_int) -> c_int;
});

/// `libX11`, loaded at the first use.
fn x11() -> Option<&'static X11> {
  static X11: OnceLock<Option<X11>> = OnceLock::new();
  X11.get_or_init(|| X11::load(&Lib::open(&[c"libX11.so.6"])?)).as_ref()
}

/// The desktop session is Wayland's (`XDG_SESSION_TYPE`, which the login manager sets): the
/// hotkey and a paste go through the portals.
fn wayland() -> bool {
  std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
}

/// Runs `f`, which makes requests on display `d`, with `d`'s X errors caught, not Xlib's default
/// handler's (which exits on a window gone): what `f` returns and the code of the first error,
/// 0 for none. The handler is the process's: one call at a time, put back after (another
/// display's errors meanwhile are ignored).
fn trap<R>(x: &X11, d: P, f: impl FnOnce() -> R) -> (R, u8) {
  static ONE: Mutex<()> = Mutex::new(());
  static OURS: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
  static CODE: AtomicU8 = AtomicU8::new(0);

  /// `XErrorHandler`; the event an `XErrorEvent`, its code at byte 32 (LP64).
  unsafe extern "C" fn on_error(d: P, e: *const u8) -> c_int {
    if d == OURS.load(Ordering::Acquire) {
      let _ = CODE.compare_exchange(0, unsafe { *e.add(32) }, Ordering::AcqRel, Ordering::Acquire);
    }

    0
  }

  let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
  OURS.store(d, Ordering::Release);
  CODE.store(0, Ordering::Release);
  let before = unsafe { (x.XSetErrorHandler)(on_error as Fp) };
  let out = f();
  unsafe {
    (x.XSync)(d, 0);
    (x.XSetErrorHandler)(before);
  }

  (out, CODE.load(Ordering::Acquire))
}
