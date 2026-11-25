// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Keys by their W3C UI Events names: [Code] a key's place, [NamedKey] a key that types nothing,
//! [Modifiers] the modifier keys held. nimbo's own, with the keys its platforms report: a new
//! key is a new variant, not a breaking change.

use std::fmt;
use std::ops::{BitAnd, BitOr, BitOrAssign, Sub, SubAssign};

/// The physical key, by its place on a US keyboard (W3C UI Events `code`): [Code::KeyA] is where
/// A is there, whatever the layout types. [Code::Unidentified] for a key nimbo doesn't map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
#[rustfmt::skip]
pub enum Code {
  Unidentified,
  KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM,
  KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
  Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
  Minus, Equal, BracketLeft, BracketRight, Backslash, Semicolon, Quote, Comma, Period, Slash,
  Backquote, Space, IntlBackslash, IntlRo, IntlYen,
  Enter, Tab, Backspace, Escape, CapsLock, ContextMenu, Lang1, Lang2,
  ShiftLeft, ShiftRight, ControlLeft, ControlRight, AltLeft, AltRight, MetaLeft, MetaRight, Fn,
  Insert, Delete, Home, End, PageUp, PageDown, ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
  PrintScreen, ScrollLock, Pause, NumLock,
  Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9,
  NumpadAdd, NumpadSubtract, NumpadMultiply, NumpadDivide, NumpadDecimal, NumpadComma,
  NumpadEqual, NumpadEnter,
  F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16, F17, F18, F19, F20,
  AudioVolumeMute, AudioVolumeDown, AudioVolumeUp,
}

/// A key that types nothing (W3C UI Events `key` values): Enter, the arrows, F5;
/// [NamedKey::Unidentified] for one nimbo doesn't name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
#[rustfmt::skip]
pub enum NamedKey {
  Unidentified,
  Enter, Tab, Backspace, Delete, Escape, Insert, ContextMenu,
  ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Home, End, PageUp, PageDown,
  F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16, F17, F18, F19, F20,
}

/// W3C's name: `KeyA`, `ArrowLeft`.
impl fmt::Display for Code {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Debug::fmt(self, f)
  }
}

/// W3C's name: `Enter`, `F5`.
impl fmt::Display for NamedKey {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Debug::fmt(self, f)
  }
}

/// The modifier keys held, a set: `Modifiers::CONTROL | Modifiers::SHIFT`. Shift, Control, Alt
/// (Option) and Meta (⌘, the Windows or Super key) only: locks and AltGr are not tracked.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Modifiers(u8);

impl Modifiers {
  pub const SHIFT: Self = Self(1);
  pub const CONTROL: Self = Self(2);
  /// Alt, Option on a Mac.
  pub const ALT: Self = Self(4);
  /// ⌘ on a Mac, the Windows or Super key elsewhere.
  pub const META: Self = Self(8);

  const NAMES: [(Self, &str); 4] =
    [(Self::SHIFT, "SHIFT"), (Self::CONTROL, "CONTROL"), (Self::ALT, "ALT"), (Self::META, "META")];

  /// None held.
  pub const fn empty() -> Self {
    Self(0)
  }

  pub const fn is_empty(self) -> bool {
    self.0 == 0
  }

  /// Whether every modifier of `other` is held.
  pub const fn contains(self, other: Self) -> bool {
    self.0 & other.0 == other.0
  }

  /// Whether any modifier of `other` is held.
  pub const fn intersects(self, other: Self) -> bool {
    self.0 & other.0 != 0
  }

  /// Both sets: `|` in a `const`.
  pub const fn union(self, other: Self) -> Self {
    Self(self.0 | other.0)
  }

  /// Adds `other` when `on`, removes it otherwise.
  pub fn set(&mut self, other: Self, on: bool) {
    *self = if on { *self | other } else { *self - other };
  }

  pub const fn shift(self) -> bool {
    self.contains(Self::SHIFT)
  }

  pub const fn ctrl(self) -> bool {
    self.contains(Self::CONTROL)
  }

  pub const fn alt(self) -> bool {
    self.contains(Self::ALT)
  }

  pub const fn meta(self) -> bool {
    self.contains(Self::META)
  }
}

/// `Modifiers(CONTROL | SHIFT)`.
impl fmt::Debug for Modifiers {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let held = Self::NAMES.iter().filter(|n| self.contains(n.0)).map(|n| n.1);
    write!(f, "Modifiers({})", held.collect::<Vec<_>>().join(" | "))
  }
}

impl BitOr for Modifiers {
  type Output = Self;

  fn bitor(self, other: Self) -> Self {
    self.union(other)
  }
}

impl BitOrAssign for Modifiers {
  fn bitor_assign(&mut self, other: Self) {
    *self = *self | other;
  }
}

/// The modifiers held in both.
impl BitAnd for Modifiers {
  type Output = Self;

  fn bitand(self, other: Self) -> Self {
    Self(self.0 & other.0)
  }
}

/// Without `other`'s modifiers.
impl Sub for Modifiers {
  type Output = Self;

  fn sub(self, other: Self) -> Self {
    Self(self.0 & !other.0)
  }
}

impl SubAssign for Modifiers {
  fn sub_assign(&mut self, other: Self) {
    *self = *self - other;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn modifiers() {
    let m = Modifiers::CONTROL | Modifiers::SHIFT;
    assert!(m.contains(Modifiers::SHIFT) && m.shift() && m.ctrl() && !m.alt() && !m.meta());
    assert!(!m.contains(Modifiers::SHIFT | Modifiers::ALT));
    assert!(m.intersects(Modifiers::SHIFT | Modifiers::ALT));
    assert_eq!(m - Modifiers::SHIFT, Modifiers::CONTROL);
    assert_eq!(m & (Modifiers::SHIFT | Modifiers::META), Modifiers::SHIFT);
    assert!(Modifiers::default().is_empty() && (m - m).is_empty());
    assert_eq!(format!("{m:?}"), "Modifiers(SHIFT | CONTROL)");
    let mut n = Modifiers::empty();
    n.set(Modifiers::ALT, true);
    n.set(Modifiers::META, false);
    assert_eq!(n, Modifiers::ALT);
  }

  #[test]
  fn names() {
    assert_eq!((Code::KeyA.to_string(), NamedKey::F5.to_string()), ("KeyA".into(), "F5".into()));
  }
}
