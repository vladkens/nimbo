// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Keys as GDK gives them, keyvals (X keysyms, the layout and its options applied) and hardware
//! codes (evdev's plus 8), into [KeyEvent]s with W3C [Code]s ([code]), so bindings and
//! [Key::named] work unchanged. Ctrl is the command modifier on Linux ([crate::keys::CMD]); with
//! Ctrl or Super, a non-Latin layout's character is a Latin one's, so shortcuts work on a Russian
//! layout.

use keyboard_types::{Code, Modifiers, NamedKey};

use crate::hotkey;
use crate::keys::{self, Key, KeyEvent};

/// GdkModifierType: Shift, Control, Mod1 (Alt), Mod4 and the virtual Super (the Super keys).
const SHIFT: u32 = 1;
const CONTROL: u32 = 1 << 2;
const MOD1: u32 = 1 << 3;
const MOD4: u32 = 1 << 6;
const SUPER: u32 = 1 << 26;

/// The modifiers of an event's state, as they were before it.
pub fn mods(state: u32) -> Modifiers {
  let mut m = Modifiers::empty();
  m.set(Modifiers::SHIFT, state & SHIFT != 0);
  m.set(Modifiers::CONTROL, state & CONTROL != 0);
  m.set(Modifiers::ALT, state & MOD1 != 0);
  m.set(Modifiers::META, state & (MOD4 | SUPER) != 0);
  m
}

/// Whether a key with `mods` may type: not with Ctrl, Super or Alt, as GTK (whose input context
/// lets Super type).
pub fn types(mods: Modifiers) -> bool {
  (mods - Modifiers::SHIFT).is_empty()
}

/// The modifiers after modifier key `sym` went down or up, from those before (`mods`) and the
/// keyvals of the keys still `held` (with it while it is down): a Shift let go while the other
/// is down keeps Shift, as does a Control while Caps Lock is a Control too (`ctrl:nocaps`).
pub fn after(mods: Modifiers, sym: u32, down: bool, held: &[u32]) -> Modifiers {
  // Shift, Control, Meta and Alt, Super and Hyper.
  let group = |s: u32| match s {
    0xffe1 | 0xffe2 => Some(0),
    0xffe3 | 0xffe4 => Some(1),
    0xffe7..=0xffea => Some(2),
    0xffeb..=0xffee => Some(3),
    _ => None,
  };
  let Some(g) = group(sym) else { return mods };
  let on = down || held.iter().any(|&h| group(h) == Some(g));
  let mut m = mods;
  m.set([Modifiers::SHIFT, Modifiers::CONTROL, Modifiers::ALT, Modifiers::META][g], on);
  m
}

/// The key event for keyval `sym`, the character `ch` it types (`gdk_keyval_to_unicode`), at
/// place `code`. Named keys go by their keyval, wherever the key is (the keypad's
/// arrows with Num Lock off, Caps Lock as Escape with `caps:escape`), by their place only
/// without one; with Ctrl or Super a non-ASCII character is the one `latin` finds (the key's on
/// the first layout with an ASCII one), else the US layout's.
pub fn key_event(
  sym: u32,
  ch: Option<char>,
  code: Code,
  mods: Modifiers,
  repeat: bool,
  latin: impl FnOnce() -> Option<char>,
) -> KeyEvent {
  let shortcut = mods.intersects(Modifiers::CONTROL | Modifiers::META);
  let named = match sym {
    0 => Key::named(code),
    sym => named(sym),
  };

  let key = named.unwrap_or_else(|| match ch {
    Some(c) if shortcut && !c.is_ascii() => {
      latin().or_else(|| keys::us_char(code, mods.shift())).map_or(Key::Char(c), Key::Char)
    }
    Some(c) if !c.is_control() => Key::Char(c),
    _ => Key::Named(NamedKey::Unidentified),
  });
  let text = match key {
    Key::Char(_) if !shortcut => ch.filter(|c| keys::printable(c.encode_utf8(&mut [0; 4]))),
    _ => None,
  };

  KeyEvent { key, text, code, mods, repeat }
}

/// The named key a keyval is (an X keysym, <gdk/gdkkeysyms.h>): what the layout and its options
/// make of the key, wherever it is (Caps Lock as Escape with `caps:escape`), and the keypad's
/// keys with Num Lock off (`GDK_KEY_KP_*`).
fn named(sym: u32) -> Option<Key> {
  use NamedKey::*;
  Some(Key::Named(match sym {
    0xff0d | 0xff8d => Enter,
    // Tab, and ISO_Left_Tab: Shift-Tab.
    0xff09 | 0xfe20 => Tab,
    0xff08 => Backspace,
    0xffff | 0xff9f => Delete,
    0xff1b => Escape,
    0xff51 | 0xff96 => ArrowLeft,
    0xff52 | 0xff97 => ArrowUp,
    0xff53 | 0xff98 => ArrowRight,
    0xff54 | 0xff99 => ArrowDown,
    0xff50 | 0xff95 => Home,
    0xff57 | 0xff9c => End,
    0xff55 | 0xff9a => PageUp,
    0xff56 | 0xff9b => PageDown,
    0xff63 | 0xff9e => Insert,
    // F1 to F20.
    0xffbe..=0xffd1 => keys::f_key((sym - 0xffbe) as u8 + 1)?.1,
    _ => return None,
  }))
}

/// Whether a keysym is a modifier's (Shift, Control, Caps and Shift Lock, Meta, Alt, Super,
/// Hyper; the ISO level, latch, lock and group keys), and the modifier key it is: by its side,
/// else by the key's place `code`: AltGr's `ISO_Level3_Shift` at the right Alt is that key.
pub fn modifier(sym: u32, code: Code) -> Option<Option<Code>> {
  let at = || Some(code).filter(|&c| hotkey::is_modifier(c));
  side(sym).map(|side| side.or_else(at))
}

fn side(sym: u32) -> Option<Option<Code>> {
  Some(match sym {
    0xffe1 => Some(Code::ShiftLeft),
    0xffe2 => Some(Code::ShiftRight),
    0xffe3 => Some(Code::ControlLeft),
    0xffe4 => Some(Code::ControlRight),
    0xffe9 => Some(Code::AltLeft),
    0xffea => Some(Code::AltRight),
    0xffeb | 0xffed => Some(Code::MetaLeft),
    0xffec | 0xffee => Some(Code::MetaRight),
    0xffe5..=0xffe8 | 0xfe01..=0xfe0f => None,
    _ => return None,
  })
}

/// The place of evdev key `key` (<linux/input-event-codes.h>): set 1's scan code up to F12, then
/// the keys Linux numbers itself.
pub fn code(key: u32) -> Code {
  match key {
    96 => Code::NumpadEnter,
    97 => Code::ControlRight,
    98 => Code::NumpadDivide,
    100 => Code::AltRight,
    102 => Code::Home,
    103 => Code::ArrowUp,
    104 => Code::PageUp,
    105 => Code::ArrowLeft,
    106 => Code::ArrowRight,
    107 => Code::End,
    108 => Code::ArrowDown,
    109 => Code::PageDown,
    110 => Code::Insert,
    111 => Code::Delete,
    117 => Code::NumpadEqual,
    125 => Code::MetaLeft,
    126 => Code::MetaRight,
    127 => Code::ContextMenu,
    183..=190 => keys::f_key((key - 170) as u8).map_or(Code::Unidentified, |f| f.0),
    _ => crate::platform::scan_code(key.try_into().unwrap_or(0)),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::keys::{Command, Delete, Move};

  const A: Code = Code::KeyA;
  const C: Code = Code::KeyC;
  const CTRL: Modifiers = Modifiers::CONTROL;
  const LEFT: Key = Key::Named(NamedKey::ArrowLeft);

  fn key(sym: u32, code: Code, mods: Modifiers) -> KeyEvent {
    let ch = char::from_u32(sym).filter(|_| sym < 0x100);
    key_event(sym, ch, code, mods, false, || None)
  }

  #[test]
  fn keys_type_and_name_like_appkit() {
    let k = key('a' as u32, A, Modifiers::empty());
    assert_eq!((k.key, k.text, k.code), (Key::Char('a'), Some('a'), A));
    let k = key('A' as u32, A, Modifiers::SHIFT);
    assert_eq!((k.key, k.text, k.mods), (Key::Char('A'), Some('A'), Modifiers::SHIFT));
    let k = key(0xff51, Code::ArrowLeft, Modifiers::SHIFT);
    assert_eq!((k.key, k.text), (LEFT, None));
    assert_eq!(keys::pc(&k), Some(Command::Move(Move::Left, true)));

    // Ctrl is the command modifier: Ctrl-C is "cmd-c", and types nothing.
    let k = key('c' as u32, C, CTRL);
    assert!(k.is("cmd-c") && k.text.is_none(), "{k:?}");
    let k = key(0xff08, Code::Backspace, CTRL);
    assert_eq!(keys::pc(&k), Some(Command::Delete(Delete::WordBackward)));
    // Super is `ctrl`: no command, no text.
    let k = key('c' as u32, C, Modifiers::META);
    assert!(k.is("ctrl-c") && !k.is("cmd-c") && k.text.is_none(), "{k:?}");
  }

  #[test]
  fn shortcuts_on_a_cyrillic_layout_use_the_latin_key() {
    // Cyrillic_es, on the C key.
    let es = |mods, latin: Option<char>| key_event(0x6d3, Some('с'), C, mods, false, || latin);
    let k = es(Modifiers::empty(), Some('c'));
    assert_eq!((k.key, k.text), (Key::Char('с'), Some('с')));
    assert!(es(CTRL, Some('c')).is("cmd-c"));
    // No Latin layout: the US one's key at that place.
    assert!(es(CTRL, None).is("cmd-c"));
  }

  #[test]
  fn keypad_keys_follow_num_lock() {
    // Num Lock off: KP_Left is Left, KP_Delete deletes; the code stays the key's.
    let k = key(0xff96, Code::Numpad4, Modifiers::empty());
    assert_eq!((k.key, k.text, k.code), (LEFT, None, Code::Numpad4));
    let k = key(0xff9f, Code::NumpadDecimal, Modifiers::empty());
    assert_eq!(k.key, Key::Named(NamedKey::Delete));
    // Num Lock on: KP_4 types 4.
    let k = key_event(0xffb4, Some('4'), Code::Numpad4, Modifiers::empty(), false, || None);
    assert_eq!((k.key, k.text), (Key::Char('4'), Some('4')));
  }

  /// Caps Lock remapped by the layout's options: Escape at Caps Lock's place, Control (swapped
  /// with Caps Lock) a modifier there.
  #[test]
  fn remapped_keys_go_by_their_keyval() {
    const CAPS: Code = Code::CapsLock;
    let k = key(0xff1b, CAPS, Modifiers::empty());
    assert_eq!((k.key, k.code), (Key::Named(NamedKey::Escape), CAPS));
    assert_eq!(modifier(0xff1b, CAPS), None);
    assert_eq!(modifier(0xffe1, Code::ShiftLeft), Some(Some(Code::ShiftLeft)));
    assert_eq!(modifier(0xffe3, CAPS), Some(Some(Code::ControlLeft)));
    assert_eq!(modifier(0xffe5, CAPS), Some(None), "Caps Lock: a modifier with no side");
    // AltGr: ISO_Level3_Shift at the right Alt.
    assert_eq!(modifier(0xfe03, Code::AltRight), Some(Some(Code::AltRight)));
  }

  #[test]
  fn modifier_keys_change_the_modifiers() {
    assert_eq!(mods(CONTROL | SHIFT | 0x100), CTRL | Modifiers::SHIFT);
    assert_eq!(
      (mods(MOD4), mods(SUPER), mods(MOD1)),
      (Modifiers::META, Modifiers::META, Modifiers::ALT)
    );
    // Shift_L, then Shift_R, goes down; Shift_L up keeps Shift while Shift_R is held.
    let shift = after(Modifiers::empty(), 0xffe1, true, &[0xffe1]);
    assert_eq!(shift, Modifiers::SHIFT);
    assert_eq!(after(shift, 0xffe1, false, &[0xffe2]), Modifiers::SHIFT);
    assert_eq!(after(shift, 0xffe1, false, &[]), Modifiers::empty());
    // Caps Lock as a second Control_L: letting go of one keeps Control.
    assert_eq!(after(CTRL, 0xffe3, false, &[0xffe3]), CTRL);
    assert_eq!(after(Modifiers::empty(), 0xffeb, true, &[]), Modifiers::META);
    assert_eq!(after(Modifiers::empty(), 0xffe9, true, &[]), Modifiers::ALT);
    assert_eq!(after(shift, 0xffe5, true, &[]), shift, "Caps Lock");
  }

  #[test]
  fn shortcuts_type_nothing() {
    assert!(types(Modifiers::empty()) && types(Modifiers::SHIFT));
    assert!(!types(CTRL) && !types(Modifiers::META) && !types(Modifiers::ALT));
  }

  #[test]
  fn evdev_codes_by_place() {
    let evdev = [30, 105, 28, 96, 88, 183, 190, 110, 125, 126, 69, 86, 99];
    let want = [
      Code::KeyA,
      Code::ArrowLeft,
      Code::Enter,
      Code::NumpadEnter,
      Code::F12,
      Code::F13,
      Code::F20,
      Code::Insert,
      Code::MetaLeft,
      Code::MetaRight,
      Code::NumLock,
      Code::IntlBackslash,
      Code::Unidentified,
    ];
    assert_eq!(evdev.map(code), want);
    assert_eq!(
      (code(105), Key::named(code(183))),
      (Code::ArrowLeft, Some(Key::Named(NamedKey::F13)))
    );
    assert_eq!(named(0xffd1), Some(Key::Named(NamedKey::F20)));
  }
}
