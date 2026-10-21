// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Keys as GDK gives them, keyvals (X keysyms, the layout and its options applied) and hardware
//! codes (evdev's plus 8), into [KeyEvent]s with macOS virtual key codes ([kvk]), so bindings
//! and [Key::named] work unchanged. Ctrl is the command modifier on Linux (`cmd`), Super is
//! `ctrl`; with either, a non-Latin layout's character is a Latin one's, so shortcuts work on a
//! Russian layout.

use crate::app::Mods;
use crate::hotkey::ModKey;
use crate::keys::{self, Key, KeyEvent};

/// GdkModifierType: Shift, Control, Mod1 (Alt), Mod4 and the virtual Super (the Super keys).
const SHIFT: u32 = 1;
const CONTROL: u32 = 1 << 2;
const MOD1: u32 = 1 << 3;
const MOD4: u32 = 1 << 6;
const SUPER: u32 = 1 << 26;

/// The modifiers of an event's state, as they were before it.
pub fn mods(state: u32) -> Mods {
  let on = |m: u32| state & m != 0;
  Mods { shift: on(SHIFT), ctrl: on(MOD4 | SUPER), alt: on(MOD1), cmd: on(CONTROL) }
}

/// Whether a key with `mods` may type: not with Ctrl, Super or Alt, as GTK (whose input context
/// lets Super type).
pub fn types(mods: Mods) -> bool {
  !(mods.cmd || mods.ctrl || mods.alt)
}

/// The modifiers after modifier key `sym` went down or up, from those before (`mods`) and the
/// keys still `held`: a Shift let go while the other is down keeps Shift.
pub fn after(mods: Mods, sym: u32, down: bool, held: &[u32]) -> Mods {
  // Shift, Control, Meta and Alt, Super and Hyper.
  let group = |s: u32| match s {
    0xffe1 | 0xffe2 => Some(0),
    0xffe3 | 0xffe4 => Some(1),
    0xffe7..=0xffea => Some(2),
    0xffeb..=0xffee => Some(3),
    _ => None,
  };
  let Some(g) = group(sym) else { return mods };
  let on = down || held.iter().any(|&h| h != sym && group(h) == Some(g));
  let mut m = mods;
  match g {
    0 => m.shift = on,
    1 => m.cmd = on,
    2 => m.alt = on,
    _ => m.ctrl = on,
  }

  m
}

/// The key event for keyval `sym`, the character `ch` it types (`gdk_keyval_to_unicode`), at
/// virtual key code `code`. Named keys go by their keyval, wherever the key is (the keypad's
/// arrows with Num Lock off, Caps Lock as Escape with `caps:escape`), by their place only
/// without one; with Ctrl or Super a non-ASCII character is the one `latin` finds (the key's on
/// the first layout with an ASCII one), else the US layout's.
pub fn key_event(
  sym: u32,
  ch: Option<char>,
  code: u16,
  mods: Mods,
  repeat: bool,
  latin: impl FnOnce() -> Option<char>,
) -> KeyEvent {
  let shortcut = mods.cmd || mods.ctrl;
  let named = match sym {
    0 => Key::named(code),
    sym => named(sym),
  };

  let key = named.unwrap_or_else(|| match ch {
    Some(c) if shortcut && !c.is_ascii() => {
      latin().or_else(|| keys::us_char(code, mods.shift)).map_or(Key::Char(c), Key::Char)
    }
    Some(c) if !c.is_control() => Key::Char(c),
    _ => Key::Other(code),
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
  Some(match sym {
    0xff0d | 0xff8d => Key::Enter,
    // Tab, and ISO_Left_Tab: Shift-Tab.
    0xff09 | 0xfe20 => Key::Tab,
    0xff08 => Key::Backspace,
    0xffff | 0xff9f => Key::Delete,
    0xff1b => Key::Escape,
    0xff51 | 0xff96 => Key::Left,
    0xff52 | 0xff97 => Key::Up,
    0xff53 | 0xff98 => Key::Right,
    0xff54 | 0xff99 => Key::Down,
    0xff50 | 0xff95 => Key::Home,
    0xff57 | 0xff9c => Key::End,
    0xff55 | 0xff9a => Key::PageUp,
    0xff56 | 0xff9b => Key::PageDown,
    // F1 to F20.
    0xffbe..=0xffd1 => Key::F((sym - 0xffbe) as u8 + 1),
    _ => return None,
  })
}

/// Whether a keysym is a modifier's (Shift, Control, Caps and Shift Lock, Meta, Alt, Super,
/// Hyper; the ISO level, latch, lock and group keys), and the [ModKey] it names by its side.
pub fn modifier(sym: u32) -> Option<Option<ModKey>> {
  Some(match sym {
    0xffe1 => Some(ModKey::LeftShift),
    0xffe2 => Some(ModKey::RightShift),
    0xffe3 => Some(ModKey::LeftCtrl),
    0xffe4 => Some(ModKey::RightCtrl),
    0xffe9 => Some(ModKey::LeftAlt),
    0xffea => Some(ModKey::RightAlt),
    0xffeb | 0xffed => Some(ModKey::LeftMeta),
    0xffec | 0xffee => Some(ModKey::RightMeta),
    0xffe5..=0xffe8 | 0xfe01..=0xfe0f => None,
    _ => return None,
  })
}

/// Evdev key codes (<linux/input-event-codes.h>) to macOS virtual key codes (`kVK_*`): US ANSI
/// positions, as [crate::keys] names them.
const KVK: &[(u16, u16)] = &[
  (1, 53),    // Esc
  (2, 18),    // 1
  (3, 19),    // 2
  (4, 20),    // 3
  (5, 21),    // 4
  (6, 23),    // 5
  (7, 22),    // 6
  (8, 26),    // 7
  (9, 28),    // 8
  (10, 25),   // 9
  (11, 29),   // 0
  (12, 27),   // -
  (13, 24),   // =
  (14, 51),   // Backspace
  (15, 48),   // Tab
  (16, 12),   // Q
  (17, 13),   // W
  (18, 14),   // E
  (19, 15),   // R
  (20, 17),   // T
  (21, 16),   // Y
  (22, 32),   // U
  (23, 34),   // I
  (24, 31),   // O
  (25, 35),   // P
  (26, 33),   // [
  (27, 30),   // ]
  (28, 36),   // Enter
  (29, 59),   // Left Ctrl
  (30, 0),    // A
  (31, 1),    // S
  (32, 2),    // D
  (33, 3),    // F
  (34, 5),    // G
  (35, 4),    // H
  (36, 38),   // J
  (37, 40),   // K
  (38, 37),   // L
  (39, 41),   // ;
  (40, 39),   // '
  (41, 50),   // `
  (42, 56),   // Left Shift
  (43, 42),   // backslash
  (44, 6),    // Z
  (45, 7),    // X
  (46, 8),    // C
  (47, 9),    // V
  (48, 11),   // B
  (49, 45),   // N
  (50, 46),   // M
  (51, 43),   // ,
  (52, 47),   // .
  (53, 44),   // /
  (54, 60),   // Right Shift
  (55, 67),   // Keypad *
  (56, 58),   // Left Alt
  (57, 49),   // Space
  (58, 57),   // Caps Lock
  (59, 122),  // F1
  (60, 120),  // F2
  (61, 99),   // F3
  (62, 118),  // F4
  (63, 96),   // F5
  (64, 97),   // F6
  (65, 98),   // F7
  (66, 100),  // F8
  (67, 101),  // F9
  (68, 109),  // F10
  (69, 71),   // Num Lock: Keypad Clear
  (71, 89),   // Keypad 7
  (72, 91),   // Keypad 8
  (73, 92),   // Keypad 9
  (74, 78),   // Keypad -
  (75, 86),   // Keypad 4
  (76, 87),   // Keypad 5
  (77, 88),   // Keypad 6
  (78, 69),   // Keypad +
  (79, 83),   // Keypad 1
  (80, 84),   // Keypad 2
  (81, 85),   // Keypad 3
  (82, 82),   // Keypad 0
  (83, 65),   // Keypad .
  (86, 10),   // 102nd key: ISO section
  (87, 103),  // F11
  (88, 111),  // F12
  (183, 105), // F13
  (184, 107), // F14
  (185, 113), // F15
  (186, 106), // F16
  (187, 64),  // F17
  (188, 79),  // F18
  (189, 80),  // F19
  (190, 90),  // F20
  (96, 76),   // Keypad Enter
  (97, 62),   // Right Ctrl
  (98, 75),   // Keypad /
  (100, 61),  // Right Alt
  (102, 115), // Home
  (103, 126), // Up
  (104, 116), // Page Up
  (105, 123), // Left
  (106, 124), // Right
  (107, 119), // End
  (108, 125), // Down
  (109, 121), // Page Down
  (110, 114), // Insert: Help
  (111, 117), // Delete
  (117, 81),  // Keypad =
  (125, 55),  // Left Super: Command
  (126, 54),  // Right Super
];

/// The macOS virtual key code of evdev key `key`.
pub fn kvk(key: u32) -> Option<u16> {
  KVK.iter().find(|(e, _)| *e as u32 == key).map(|(_, k)| *k)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::keys::{Command, Delete, Move};

  /// Virtual key codes.
  const A: u16 = 0;
  const C: u16 = 8;
  const LEFT: u16 = 123;
  const BACKSPACE: u16 = 51;
  const CTRL: Mods = Mods { cmd: true, ..Mods::NONE };

  fn key(sym: u32, code: u16, mods: Mods) -> KeyEvent {
    let ch = char::from_u32(sym).filter(|_| sym < 0x100);
    key_event(sym, ch, code, mods, false, || None)
  }

  #[test]
  fn keys_type_and_name_like_appkit() {
    let k = key('a' as u32, A, Mods::NONE);
    assert_eq!((k.key, k.text, k.code), (Key::Char('a'), Some('a'), 0));
    let k = key('A' as u32, A, Mods::SHIFT);
    assert_eq!((k.key, k.text, k.mods), (Key::Char('A'), Some('A'), Mods::SHIFT));
    let k = key(0xff51, LEFT, Mods::SHIFT);
    assert_eq!((k.key, k.text), (Key::Left, None));
    assert_eq!(keys::pc(&k), Some(Command::Move(Move::Left, true)));

    // Ctrl is the command modifier: Ctrl-C is "cmd-c", and types nothing.
    let k = key('c' as u32, C, CTRL);
    assert!(k.is("cmd-c") && k.text.is_none(), "{k:?}");
    let k = key(0xff08, BACKSPACE, CTRL);
    assert_eq!(keys::pc(&k), Some(Command::Delete(Delete::WordBackward)));
  }

  #[test]
  fn shortcuts_on_a_cyrillic_layout_use_the_latin_key() {
    // Cyrillic_es, on the C key.
    let es = |mods, latin: Option<char>| key_event(0x6d3, Some('с'), C, mods, false, || latin);
    let k = es(Mods::NONE, Some('c'));
    assert_eq!((k.key, k.text), (Key::Char('с'), Some('с')));
    assert!(es(CTRL, Some('c')).is("cmd-c"));
    // No Latin layout: the US one's key at that place.
    assert!(es(CTRL, None).is("cmd-c"));
  }

  #[test]
  fn keypad_keys_follow_num_lock() {
    // Num Lock off: KP_Left is Left, KP_Delete deletes; the code stays the key's.
    let k = key(0xff96, 86, Mods::NONE);
    assert_eq!((k.key, k.text, k.code), (Key::Left, None, 86));
    assert_eq!(key(0xff9f, 65, Mods::NONE).key, Key::Delete);
    // Num Lock on: KP_4 types 4.
    let k = key_event(0xffb4, Some('4'), 86, Mods::NONE, false, || None);
    assert_eq!((k.key, k.text), (Key::Char('4'), Some('4')));
  }

  /// Caps Lock remapped by the layout's options: Escape at Caps Lock's place, Control (swapped
  /// with Caps Lock) a modifier there.
  #[test]
  fn remapped_keys_go_by_their_keyval() {
    const CAPS: u16 = 57;
    let k = key(0xff1b, CAPS, Mods::NONE);
    assert_eq!((k.key, k.code), (Key::Escape, CAPS));
    assert_eq!(modifier(0xff1b), None);
    assert_eq!(modifier(0xffe1), Some(Some(ModKey::LeftShift)));
    assert_eq!(modifier(0xffe3), Some(Some(ModKey::LeftCtrl)));
    assert_eq!(modifier(0xffe5), Some(None), "Caps Lock: a modifier with no side");
  }

  #[test]
  fn modifier_keys_change_the_modifiers() {
    assert_eq!(mods(CONTROL | SHIFT | 0x100), Mods { shift: true, cmd: true, ..Mods::NONE });
    assert!(mods(MOD4).ctrl && mods(SUPER).ctrl && mods(MOD1).alt);
    // Shift_L, then Shift_R, goes down; Shift_L up keeps Shift while Shift_R is held.
    let shift = after(Mods::NONE, 0xffe1, true, &[0xffe1]);
    assert_eq!(shift, Mods::SHIFT);
    assert_eq!(after(shift, 0xffe1, false, &[0xffe2]), Mods::SHIFT);
    assert_eq!(after(shift, 0xffe1, false, &[]), Mods::NONE);
    assert_eq!(after(Mods::NONE, 0xffeb, true, &[]), Mods { ctrl: true, ..Mods::NONE });
    assert_eq!(after(shift, 0xffe5, true, &[]), shift, "Caps Lock");
  }

  #[test]
  fn shortcuts_type_nothing() {
    assert!(types(Mods::NONE) && types(Mods::SHIFT));
    assert!(!types(CTRL) && !types(Mods { ctrl: true, ..Mods::NONE }));
    assert!(!types(Mods { alt: true, ..Mods::NONE }));
  }

  #[test]
  fn key_codes_map_to_named_keys() {
    assert_eq!(kvk(105).and_then(Key::named), Some(Key::Left));
    assert_eq!(kvk(28).and_then(Key::named), Some(Key::Enter));
    assert_eq!(kvk(88).and_then(Key::named), Some(Key::F(12)));
    assert_eq!(kvk(183).and_then(Key::named), Some(Key::F(13)));
    assert_eq!(named(0xffd1), Some(Key::F(20)));
    assert_eq!(kvk(30), Some(A));
  }
}
