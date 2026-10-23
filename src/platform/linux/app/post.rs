// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Shortcuts posted as key events on Linux: XTest's fake key events on X11 (`libX11` and
//! `libXtst`, loaded at run time), the modifier keys down, the key, the modifiers up
//! ([crate::keys::strokes]). The key is the one the keymap has for the character's keysym in any
//! group (so Ctrl-V works on a Russian layout), Shift added at its second level. Wayland has no
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
  fn XKeysymToKeycode(d: P, sym: c_ulong) -> u8;
  fn XkbKeycodeToKeysym(d: P, code: u8, group: c_int, level: c_int) -> c_ulong;
  fn XFlush(d: P) -> c_int;
});

fns!(Xtst {
  fn XTestFakeKeyEvent(d: P, code: c_uint, press: c_int, delay: c_ulong) -> c_int;
});

fn libs() -> Option<&'static (X11, Xtst)> {
  static LIBS: OnceLock<Option<(X11, Xtst)>> = OnceLock::new();
  let load = || {
    let x = X11::load(&Lib::open(&[c"libX11.so.6"])?)?;
    Some((x, Xtst::load(&Lib::open(&[c"libXtst.so.6"])?)?))
  };
  LIBS.get_or_init(load).as_ref()
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

  let code = |sym| unsafe { (x.XKeysymToKeycode)(d, sym) };
  let key = code(sym);
  let shift = key != 0
    && unsafe { (x.XkbKeycodeToKeysym)(d, key, 0, 0) } != sym
    && unsafe { (x.XkbKeycodeToKeysym)(d, key, 0, 1) } == sym;
  let mods = if shift { s.mods | Modifiers::SHIFT } else { s.mods };
  let modkey = |m| code(modifier_keysym(m));
  let presses = strokes(mods, key, modkey);
  let posted = key != 0 && presses.iter().all(|&(k, _)| k != 0);
  if posted {
    for (k, down) in presses {
      unsafe { (t.XTestFakeKeyEvent)(d, k.into(), down.into(), 0) };
    }
  }

  unsafe {
    (x.XFlush)(d);
    (x.XCloseDisplay)(d);
  }
  posted.then_some(()).ok_or("the keymap has no such key".into())
}

/// The X keysym of `key`: a Latin-1 character's is its code point, another's `0x1000000` plus it;
/// named keys' from `keysymdef.h`.
fn keysym(key: Key) -> Option<c_ulong> {
  let named = match key {
    Key::Char(c) => {
      let c = lower(c) as c_ulong;
      return Some(if (0x20..0x7F).contains(&c) || (0xA0..0x100).contains(&c) {
        c
      } else {
        0x0100_0000 | c
      });
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
    let keys = [Key::Char('V'), Key::Char('+'), Key::Char('ж'), Key::Named(NamedKey::Enter)];
    assert_eq!(keys.map(keysym), [Some(0x76), Some(0x2B), Some(0x0100_0436), Some(0xFF0D)]);
    assert_eq!(keysym(Key::Named(NamedKey::F12)), Some(0xFFC9));
  }
}
