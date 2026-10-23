// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Shortcuts posted as key events on Linux: XTest's fake key events on X11 (`libX11` and
//! `libXtst`, loaded at run time), the modifier keys set as the shortcut wants them, the key, the
//! modifiers back as the user holds them ([crate::keys::strokes]). The key is the one with the
//! character's keysym in the group in use, else in another (so Ctrl-V works on a Russian
//! layout), Shift added at its second level. Wayland has no
//! protocol for one app to type into another (only the RemoteDesktop portal, which asks the user
//! each session): there it fails, as on X11 without the libraries.

use std::ffi::{c_char, c_int, c_uint, c_ulong};
use std::ptr::null;
use std::sync::OnceLock;

use keyboard_types::{Modifiers, NamedKey};

use super::super::P;
use super::super::dl::{Lib, fns};
use crate::keys::{Key, Shortcut, f_key, lower, strokes};

fns!(X11 {
  fn XOpenDisplay(name: *const c_char) -> P;
  fn XCloseDisplay(d: P) -> c_int;
  fn XDefaultRootWindow(d: P) -> c_ulong;
  fn XQueryPointer(d: P, w: c_ulong, root: *mut c_ulong, child: *mut c_ulong, rx: *mut c_int,
    ry: *mut c_int, x: *mut c_int, y: *mut c_int, mask: *mut c_uint) -> c_int;
  fn XKeysymToKeycode(d: P, sym: c_ulong) -> u8;
  fn XkbKeycodeToKeysym(d: P, code: u8, group: c_int, level: c_int) -> c_ulong;
  fn XkbGetState(d: P, device: c_uint, state: *mut u8) -> c_int;
  fn XFlush(d: P) -> c_int;
});

fns!(Xtst {
  fn XTestFakeKeyEvent(d: P, code: c_uint, press: c_int, delay: c_ulong) -> c_int;
});

fns!(Gdk {
  fn gdk_unicode_to_keyval(c: u32) -> u32;
});

/// `XkbUseCoreKbd`.
const CORE_KBD: c_uint = 0x100;

fn libs() -> Option<&'static (X11, Xtst)> {
  static LIBS: OnceLock<Option<(X11, Xtst)>> = OnceLock::new();
  let load = || {
    let x = X11::load(&Lib::open(&[c"libX11.so.6"])?)?;
    Some((x, Xtst::load(&Lib::open(&[c"libXtst.so.6"])?)?))
  };
  LIBS.get_or_init(load).as_ref()
}

fn gdk() -> Option<&'static Gdk> {
  static GDK: OnceLock<Option<Gdk>> = OnceLock::new();
  GDK.get_or_init(|| Gdk::load(&Lib::open(&[c"libgdk-3.so.0"])?)).as_ref()
}

pub fn post(s: Shortcut) -> Result<(), String> {
  if std::env::var_os("WAYLAND_DISPLAY").is_some() {
    return Err("Wayland lets no app send keys to another".into());
  }

  let (x, t) = libs().ok_or("no libX11 or libXtst")?;
  let sym = keysym(s.key).ok_or("no keysym for the key")?;
  let d = unsafe { (x.XOpenDisplay)(null()) };
  if d.is_null() {
    return Err("no X display".into());
  }

  let posted = (|| {
    let (key, shift) = find(x, d, sym)?;
    let mods = if shift { s.mods | Modifiers::SHIFT } else { s.mods };
    let modkey = |m| unsafe { (x.XKeysymToKeycode)(d, modifier_keysym(m)) };
    let presses = strokes(mods, held(x, d), key, modkey);
    if presses.iter().any(|&(k, _)| k == 0) {
      return None;
    }

    for (k, down) in presses {
      unsafe { (t.XTestFakeKeyEvent)(d, k.into(), down.into(), 0) };
    }

    Some(())
  })();
  unsafe {
    (x.XFlush)(d);
    (x.XCloseDisplay)(d);
  }
  posted.ok_or("the keymap has no such key".into())
}

/// The keycode with `sym` and whether it takes Shift: in the group in use first (with US and
/// Dvorak, Dvorak's V when Dvorak is on), then the others (a Latin key while Russian is on).
fn find(x: &X11, d: P, sym: c_ulong) -> Option<(u8, bool)> {
  // XkbStateRec, its first byte the effective group.
  let mut state = [0u8; 32];
  let group = match unsafe { (x.XkbGetState)(d, CORE_KBD, state.as_mut_ptr()) } {
    0 => c_int::from(state[0]),
    _ => 0,
  };

  let groups = [group].into_iter().chain((0..4).filter(|&g| g != group));
  let at =
    |g, level| (8..=255u8).find(|&k| unsafe { (x.XkbKeycodeToKeysym)(d, k, g, level) } == sym);
  groups.flat_map(|g| [(g, 0), (g, 1)]).find_map(|(g, level)| Some((at(g, level)?, level == 1)))
}

/// The modifiers held now (the pointer's state: the keyboard's modifiers).
fn held(x: &X11, d: P) -> Modifiers {
  let (mut w, mut i, mut mask) = (0, 0, 0);
  let root = unsafe { (x.XDefaultRootWindow)(d) };
  let ok = unsafe {
    (x.XQueryPointer)(d, root, &mut w, &mut w, &mut i, &mut i, &mut i, &mut i, &mut mask)
  };
  // X's Shift, Control, Mod1 and Mod4 masks are GDK's.
  if ok != 0 { super::super::keys::mods(mask) } else { Modifiers::empty() }
}

/// The X keysym of `key`: a character's as GDK has it (Cyrillic, Greek and others have their own
/// keysyms, `ж` is `Cyrillic_zhe`), else [unicode_keysym]; named keys' from `keysymdef.h`.
fn keysym(key: Key) -> Option<c_ulong> {
  let named = match key {
    Key::Char(c) => {
      let c = lower(c);
      let gdk = gdk().map(|g| unsafe { (g.gdk_unicode_to_keyval)(c as u32) } as c_ulong);
      return Some(gdk.unwrap_or_else(|| unicode_keysym(c)));
    }
    Key::Named(k) => k,
  };

  let f = (1..=20).find(|&n| f_key(n).map(|f| f.1) == Some(named));
  if let Some(n) = f {
    return Some(0xFFBE + n as c_ulong - 1);
  }

  Some(match named {
    NamedKey::Enter => 0xFF0D,
    NamedKey::Tab => 0xFF09,
    NamedKey::Backspace => 0xFF08,
    NamedKey::Delete => 0xFFFF,
    NamedKey::Escape => 0xFF1B,
    NamedKey::Home => 0xFF50,
    NamedKey::ArrowLeft => 0xFF51,
    NamedKey::ArrowUp => 0xFF52,
    NamedKey::ArrowRight => 0xFF53,
    NamedKey::ArrowDown => 0xFF54,
    NamedKey::PageUp => 0xFF55,
    NamedKey::PageDown => 0xFF56,
    NamedKey::End => 0xFF57,
    NamedKey::Insert => 0xFF63,
    _ => return None,
  })
}

/// A character's keysym without GDK: Latin-1's is its code point, another's `0x1000000` plus it.
fn unicode_keysym(c: char) -> c_ulong {
  let c = c as c_ulong;
  if (0x20..0x7F).contains(&c) || (0xA0..0x100).contains(&c) { c } else { 0x0100_0000 | c }
}

/// The left key of modifier `m`: Control_L, Alt_L, Shift_L, Super_L.
fn modifier_keysym(m: Modifiers) -> c_ulong {
  match m {
    Modifiers::CONTROL => 0xFFE3,
    Modifiers::ALT => 0xFFE9,
    Modifiers::SHIFT => 0xFFE1,
    _ => 0xFFEB,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn keysyms_of_keys() {
    let keys = [Key::Char('V'), Key::Char('+'), Key::Named(NamedKey::Enter)];
    assert_eq!(keys.map(keysym), [Some(0x76), Some(0x2B), Some(0xFF0D)]);
    assert_eq!(unicode_keysym('ж'), 0x0100_0436);
    assert_eq!(keysym(Key::Named(NamedKey::F12)), Some(0xFFC9));
  }
}
