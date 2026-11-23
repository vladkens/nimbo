// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The displays GDK opened on Linux: whether the default one is Wayland's ([wayland]), and
//! XWayland's X display ([xdisplay]), which an overlay and a paste on Wayland use.

use std::ffi::{CString, c_int};
use std::os::unix::ffi::OsStringExt;
use std::sync::OnceLock;

use super::dl::fns;
use super::{P, S, lib};

fns!(Gdk {
  fn g_type_check_instance_is_a(instance: P, ty: usize) -> c_int;
  fn gdk_display_get_default() -> P;
  fn gdk_display_open(name: S) -> P;
});

fn gdk() -> &'static Gdk {
  static GDK: OnceLock<Option<Gdk>> = OnceLock::new();
  GDK.get_or_init(|| Gdk::load(&lib()?)).as_ref().expect("GTK is loaded before a window opens")
}

/// The display is Wayland's: GDK's Wayland backend is built in and opened it.
pub(super) fn wayland() -> bool {
  is_wayland(unsafe { (gdk().gdk_display_get_default)() })
}

/// Display `display` is a Wayland one.
pub(super) fn is_wayland(display: P) -> bool {
  let Some(lib) = lib() else { return false };
  // SAFETY: GObject's type getter of GDK's Wayland display, a `GType` (a `gsize`).
  let get = unsafe { lib.sym::<unsafe extern "C" fn() -> usize>("gdk_wayland_display_get_type\0") };
  get.is_some_and(|f| unsafe { (gdk().g_type_check_instance_is_a)(display, f()) } != 0)
}

/// XWayland's display (`DISPLAY`), opened at the first call: an overlay's on Wayland without the
/// layer shell, and the clipboard's ([super::app::paste]).
pub(super) fn xdisplay() -> Option<P> {
  static X: OnceLock<usize> = OnceLock::new();
  let at = X.get_or_init(|| {
    let name = std::env::var_os("DISPLAY").and_then(|d| CString::new(d.into_vec()).ok());
    // SAFETY: GDK's call on the main thread; GDK's Wayland backend refuses an X display's name.
    let d = name.map_or(std::ptr::null_mut(), |n| unsafe { (gdk().gdk_display_open)(n.as_ptr()) });
    if d.is_null() || is_wayland(d) { 0 } else { d as usize }
  });
  (*at != 0).then_some(*at as P)
}
