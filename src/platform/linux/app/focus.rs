// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Where the user types on Linux ([crate::focus]): on X11 the window manager's
//! `_NET_ACTIVE_WINDOW` and its `_NET_WM_PID` (EWMH, `libX11` loaded at run time), the name from
//! `/proc`; Wayland shows no app another's windows, nor does an X server under it (XWayland sees
//! only X clients): there is none.

use std::ffi::{CStr, c_ulong};
use std::ptr::{null, null_mut};

use super::super::P;
use super::{X11, trap, x11};
use crate::focus::{self, Focused};

/// An X window's id.
#[derive(Clone, Debug, PartialEq)]
pub struct Window(c_ulong);

pub fn current() -> Option<Focused> {
  if std::env::var_os("WAYLAND_DISPLAY").is_some() {
    return None;
  }

  let x = x11()?;
  let d = unsafe { (x.XOpenDisplay)(null()) };
  if d.is_null() {
    return None;
  }

  // A window gone meanwhile is a BadWindow error, on which Xlib's default handler exits.
  let ((active, pid), _) = trap(x, d, || {
    let root = unsafe { (x.XDefaultRootWindow)(d) };
    let active = property(x, d, root, c"_NET_ACTIVE_WINDOW");
    (active, active.filter(|&w| w != 0).and_then(|w| property(x, d, w, c"_NET_WM_PID")))
  });
  unsafe { (x.XCloseDisplay)(d) };

  let (active, pid) = (active?, pid.unwrap_or(0) as u32);
  let name = match pid {
    0 => String::new(),
    _ => std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default(),
  };

  Some(Focused {
    pid,
    name: name.trim_end().to_owned(),
    window: Ok((active != 0).then_some(focus::Window(Window(active)))),
    secure: Ok(false),
  })
}

/// The first 32-bit value (a window, a cardinal) of `w`'s property `name`, which Xlib hands out
/// as a C long; `None` when the window has none (a window manager without EWMH).
fn property(x: &X11, d: P, w: c_ulong, name: &CStr) -> Option<c_ulong> {
  let atom = unsafe { (x.XInternAtom)(d, name.as_ptr(), 1) };
  if atom == 0 {
    return None;
  }

  let (mut kind, mut format, mut items, mut after, mut data) = (0, 0, 0, 0, null_mut());
  // `AnyPropertyType` (0), one long from the start, not deleted.
  let done = unsafe {
    let (k, f, i, a) = (&mut kind, &mut format, &mut items, &mut after);
    (x.XGetWindowProperty)(d, w, atom, 0, 1, 0, 0, k, f, i, a, &mut data)
  };
  // SAFETY: a format-32 property of at least one item holds a C long.
  let value = (done == 0 && format == 32 && items >= 1 && !data.is_null())
    .then(|| unsafe { *data.cast::<c_ulong>() });
  if !data.is_null() {
    unsafe { (x.XFree)(data.cast()) };
  }

  value
}
