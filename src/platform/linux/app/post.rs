// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Shortcuts posted as key events on Linux: XTest's fake key events on X11 (`libX11` and
//! `libXtst`, loaded at run time), the modifier keys set as the shortcut wants them, the key, the
//! modifiers back as the user holds them ([crate::keys::strokes]). The key is the one with the
//! character's keysym in the group in use, else in another (so Ctrl-V works on a Russian
//! layout), Shift added at its second level; a key with no symbols is the mask tapped around a
//! held Alt or Super lifted and put back, so neither reads as tapped alone. Wayland has no
//! protocol for one app to type into another: there the RemoteDesktop portal's keyboard presses
//! the keys ([super::remote]), the desktop asking the user the first time; it fails without the
//! portal, as on X11 without the libraries.

use std::ffi::{c_int, c_uint, c_ulong};
use std::ptr::null;
use std::sync::OnceLock;

use keyboard_types::{Modifiers, NamedKey};

use super::super::dl::{Lib, fns};
use super::super::{P, keys, lib};
use super::{X11, x11};
use crate::keys::{Key, Shortcut, f_key, strokes};

fns!(Xtst {
  fn XTestFakeKeyEvent(d: P, code: c_uint, press: c_int, delay: c_ulong) -> c_int;
});

fns!(Gdk {
  fn gdk_unicode_to_keyval(c: u32) -> u32;
});

/// `XkbUseCoreKbd`.
const CORE_KBD: c_uint = 0x100;

fn xtst() -> Option<&'static Xtst> {
  static XTST: OnceLock<Option<Xtst>> = OnceLock::new();
  XTST.get_or_init(|| Xtst::load(&Lib::open(&[c"libXtst.so.6"])?)).as_ref()
}

fn gdk() -> Option<&'static Gdk> {
  static GDK: OnceLock<Option<Gdk>> = OnceLock::new();
  GDK.get_or_init(|| Gdk::load(&lib()?)).as_ref()
}

pub fn ready() -> bool {
  match super::wayland() {
    true => super::remote::granted(),
    false => x11().zip(xtst()).is_some(),
  }
}

pub fn post(s: Shortcut) -> Result<(), String> {
  if super::wayland() {
    return super::remote::post(s);
  }

  let (x, t) = x11().zip(xtst()).ok_or("no libX11 or libXtst")?;
  let sym = keysym(s.key).ok_or("no keysym for the key")?;
  let d = unsafe { (x.XOpenDisplay)(null()) };
  if d.is_null() {
    return Err("no X display".into());
  }

  let posted = send(x, t, d, s.mods, sym);
  unsafe {
    (x.XFlush)(d);
    (x.XCloseDisplay)(d);
  }
  posted.ok_or("the keymap has no such key".into())
}

/// Sends `sym` with `mods` on display `d`: none sent when the keymap lacks a key it needs.
fn send(x: &X11, t: &Xtst, d: P, mods: Modifiers, sym: c_ulong) -> Option<()> {
  let (key, shift) = find(x, d, sym)?;
  let mods = if shift { mods | Modifiers::SHIFT } else { mods };
  let mut held = [0u8; 32];
  unsafe { (x.XQueryKeymap)(d, held.as_mut_ptr()) };
  let of = |code| keys::modifier_sym(code).map(|s| unsafe { (x.XKeysymToKeycode)(d, s.into()) });
  let down = |k: u8| held[k as usize / 8] & (1 << (k % 8)) != 0;
  for (k, press) in strokes(mods, key, |c| of(c).filter(|&k| k != 0), down, spare(x, d))? {
    unsafe { (t.XTestFakeKeyEvent)(d, k.into(), press.into(), 0) };
  }

  Some(())
}

/// A keycode of the server's with no symbols in any group: pressed, it does nothing.
fn spare(x: &X11, d: P) -> Option<u8> {
  let (mut min, mut max) = (0, 0);
  unsafe { (x.XDisplayKeycodes)(d, &mut min, &mut max) };
  let codes = (min.max(8) as u8)..=(max.min(255) as u8);
  let none = |k| (0..4).all(|g| (0..4).all(|l| unsafe { (x.XkbKeycodeToKeysym)(d, k, g, l) } == 0));
  codes.rev().find(|&k| none(k))
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

/// The X keysym of `key`: a character's as GDK has it (Cyrillic, Greek and others have their own
/// keysyms, `ж` is `Cyrillic_zhe`, the rest `0x1000000` plus the code point); named keys' from
/// `keysymdef.h`.
pub(super) fn keysym(key: Key) -> Option<c_ulong> {
  let named = match key {
    Key::Char(c) => return gdk().map(|g| unsafe { (g.gdk_unicode_to_keyval)(c as u32) }.into()),
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn keysyms_of_keys() {
    let keys = [Key::Char('v'), Key::Char('+'), Key::Char('ж'), Key::Named(NamedKey::Enter)];
    assert_eq!(keys.map(keysym), [Some(0x76), Some(0x2B), Some(0x6D6), Some(0xFF0D)]);
    assert_eq!(keysym(Key::Char('\u{1F600}')), Some(0x0101_F600));
    assert_eq!(keysym(Key::Named(NamedKey::F12)), Some(0xFFC9));
  }
}
