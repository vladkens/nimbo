// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Keys on Windows: virtual keys and scan codes to [KeyEvent]s with macOS virtual key codes
//! (`kVK_*`, which name the US ANSI positions), the layout's characters through ToUnicodeEx, and
//! the commands Windows' text controls give the keys that type nothing ([standard]).
//!
//! Ctrl is the command key: it sets [Mods::cmd], so bindings written `cmd-c` are Ctrl-C here,
//! and the Windows key sets [Mods::ctrl]. Alt is Alt.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
  GetKeyState, GetKeyboardLayout, GetKeyboardState, ToUnicodeEx, VK_BACK, VK_CONTROL, VK_DELETE,
  VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F12, VK_HOME, VK_INSERT, VK_LEFT, VK_LWIN, VK_MENU,
  VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_RWIN, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};

use crate::app::Mods;
use crate::hotkey::ModKey;
use crate::keys::{self, Command, Delete, Key, KeyEvent, Move};

/// Set-1 scan codes of the character keys (and the keypad) to macOS virtual key codes.
const SCAN: [(u16, u16); 63] = [
  (0x02, 18), // 1
  (0x03, 19),
  (0x04, 20),
  (0x05, 21),
  (0x06, 23), // 5
  (0x07, 22), // 6
  (0x08, 26), // 7
  (0x09, 28), // 8
  (0x0A, 25), // 9
  (0x0B, 29), // 0
  (0x0C, 27), // -
  (0x0D, 24), // =
  (0x10, 12), // Q
  (0x11, 13),
  (0x12, 14),
  (0x13, 15),
  (0x14, 17), // T
  (0x15, 16), // Y
  (0x16, 32), // U
  (0x17, 34), // I
  (0x18, 31), // O
  (0x19, 35), // P
  (0x1A, 33), // [
  (0x1B, 30), // ]
  (0x1E, 0),  // A
  (0x1F, 1),
  (0x20, 2),
  (0x21, 3),
  (0x22, 5),  // G
  (0x23, 4),  // H
  (0x24, 38), // J
  (0x25, 40), // K
  (0x26, 37), // L
  (0x27, 41), // ;
  (0x28, 39), // '
  (0x29, 50), // `
  (0x2B, 42), // \
  (0x2C, 6),  // Z
  (0x2D, 7),
  (0x2E, 8),
  (0x2F, 9),
  (0x30, 11), // B
  (0x31, 45), // N
  (0x32, 46), // M
  (0x33, 43), // ,
  (0x34, 47), // .
  (0x35, 44), // /
  (0x39, 49), // space
  (0x56, 10), // ISO section, left of Z
  (0x37, 67), // keypad *
  (0x4A, 78), // keypad -
  (0x4E, 69), // keypad +
  (0x53, 65), // keypad .
  (0x52, 82), // keypad 0
  (0x4F, 83),
  (0x50, 84),
  (0x51, 85),
  (0x4B, 86),
  (0x4C, 87),
  (0x4D, 88),
  (0x47, 89), // keypad 7
  (0x48, 91),
  (0x49, 92),
];

/// The macOS virtual key code of a key: named keys by virtual key (so a posted message with no
/// extended bit maps too), the rest by scan code; unknown keys `0x100 + vk`.
pub fn code(vk: u16, scan: u16, extended: bool) -> u16 {
  let named = match vk {
    VK_RETURN if extended => Some(76), // keypad
    VK_RETURN => Key::Enter.code(),
    VK_TAB => Key::Tab.code(),
    VK_BACK => Key::Backspace.code(),
    VK_DELETE => Key::Delete.code(),
    VK_ESCAPE => Key::Escape.code(),
    VK_LEFT => Key::Left.code(),
    VK_RIGHT => Key::Right.code(),
    VK_UP => Key::Up.code(),
    VK_DOWN => Key::Down.code(),
    VK_HOME => Key::Home.code(),
    VK_END => Key::End.code(),
    VK_PRIOR => Key::PageUp.code(),
    VK_NEXT => Key::PageDown.code(),
    VK_INSERT => Some(114), // kVK_Help, where Mac keyboards had Insert
    VK_SPACE => Some(49),
    VK_F1..=VK_F12 => Key::F((vk - VK_F1) as u8 + 1).code(),
    _ => None,
  };

  let scanned = || (!extended).then(|| SCAN.iter().find(|(s, _)| *s == scan)).flatten();
  named.or_else(|| scanned().map(|&(_, c)| c)).unwrap_or(0x100 + vk)
}

fn down(vk: u16) -> bool {
  unsafe { GetKeyState(vk as i32) as u16 & 0x8000 != 0 }
}

/// The modifiers held now (as of the message being handled): Ctrl as `cmd`, Windows as `ctrl`.
pub fn mods() -> Mods {
  Mods {
    shift: down(VK_SHIFT),
    ctrl: down(VK_LWIN) || down(VK_RWIN),
    alt: down(VK_MENU),
    cmd: down(VK_CONTROL),
  }
}

/// Whether `vk` is a modifier key: it changes [Mods], it isn't a key press.
pub fn is_modifier(vk: u16) -> bool {
  matches!(vk, VK_SHIFT | VK_CONTROL | VK_MENU | VK_LWIN | VK_RWIN | 0xA0..=0xA5)
}

/// The modifier key a key message is about, by its side: the left and right keys share a
/// virtual key, the scan code (Shift) or the extended bit (Ctrl, Alt) tell them apart.
pub fn mod_key(vk: u16, scan: u16, extended: bool) -> Option<ModKey> {
  let side = |left, right| Some(if extended { right } else { left });
  match vk {
    VK_SHIFT => Some(if scan == 0x36 { ModKey::RightShift } else { ModKey::LeftShift }),
    VK_CONTROL => side(ModKey::LeftCtrl, ModKey::RightCtrl),
    VK_MENU => side(ModKey::LeftAlt, ModKey::RightAlt),
    VK_LWIN => Some(ModKey::LeftMeta),
    VK_RWIN => Some(ModKey::RightMeta),
    0xA0 => Some(ModKey::LeftShift),
    0xA1 => Some(ModKey::RightShift),
    0xA2 => Some(ModKey::LeftCtrl),
    0xA3 => Some(ModKey::RightCtrl),
    0xA4 => Some(ModKey::LeftAlt),
    0xA5 => Some(ModKey::RightAlt),
    _ => None,
  }
}

/// What a key types on the current layout with the key state `state`, without touching the
/// layout's dead-key state; `None` for nothing or more than one character.
fn typed(vk: u16, scan: u16, state: &[u8; 256]) -> Option<char> {
  let mut buf = [0u16; 8];
  // Flag 4: keep the keyboard state (dead keys) as it is (Windows 10 1607).
  let layout = unsafe { GetKeyboardLayout(0) };
  let n =
    unsafe { ToUnicodeEx(vk as u32, scan as u32, state.as_ptr(), buf.as_mut_ptr(), 8, 4, layout) };
  let units = &buf[..n.unsigned_abs().min(8) as usize];
  let mut cs = char::decode_utf16(units.iter().copied());
  match (cs.next(), cs.next()) {
    (Some(Ok(c)), None) => Some(c),
    _ => None,
  }
}

/// A key press: named keys by code, the rest by the character the key types with Shift alone
/// (with Ctrl or Windows, a non-ASCII one is the US layout's, so shortcuts work on any layout);
/// `text` what it types with every modifier, unless Ctrl or Windows is held.
pub fn key_event(vk: u16, scan: u16, extended: bool, repeat: bool) -> KeyEvent {
  let (code, mods) = (code(vk, scan, extended), mods());
  let mut state = [0u8; 256];
  if mods.shift {
    state[VK_SHIFT as usize] = 0x80;
  }

  let key = Key::named(code).unwrap_or_else(|| match typed(vk, scan, &state) {
    Some(c) if (mods.cmd || mods.ctrl) && !c.is_ascii() => {
      keys::us_char(code, mods.shift).map_or(Key::Char(c), Key::Char)
    }
    Some(c) if !c.is_control() => Key::Char(c),
    _ => Key::Other(code),
  });
  let text = match key {
    Key::Char(_) if !(mods.cmd || mods.ctrl) => {
      unsafe { GetKeyboardState(state.as_mut_ptr()) };
      typed(vk, scan, &state).filter(|c| keys::printable(c.encode_utf8(&mut [0; 4])))
    }
    _ => None,
  };

  KeyEvent { key, text, code, mods, repeat }
}

/// The command a Windows text control gives a key that types nothing (Ctrl is `cmd`): Ctrl
/// with arrows moves by words (and paragraphs), Home and End to the line's ends and with Ctrl
/// the document's, Page Up and Down move, Ctrl-Backspace and Ctrl-Delete delete words, Ctrl-Y
/// redoes, and the old Shift-Delete, Ctrl-Insert and Shift-Insert cut, copy and paste.
pub fn standard(k: &KeyEvent) -> Option<Command> {
  use Move::*;
  let Mods { shift, ctrl, alt, cmd } = k.mods;
  if ctrl || alt {
    return None;
  }

  let to = |m| Some(Command::Move(m, shift));
  let del = |d| Some(Command::Delete(d));
  match (k.key, cmd) {
    (Key::Left, false) => to(Left),
    (Key::Left, true) => to(WordLeft),
    (Key::Right, false) => to(Right),
    (Key::Right, true) => to(WordRight),
    (Key::Up, false) => to(Up),
    (Key::Up, true) => to(ParagraphStart),
    (Key::Down, false) => to(Down),
    (Key::Down, true) => to(ParagraphEnd),
    (Key::Home, false) => to(LineStart),
    (Key::Home, true) => to(DocStart),
    (Key::End, false) => to(LineEnd),
    (Key::End, true) => to(DocEnd),
    (Key::PageUp, false) => to(PageUp),
    (Key::PageDown, false) => to(PageDown),
    (Key::Backspace, false) => del(Delete::Backward),
    (Key::Backspace, true) => del(Delete::WordBackward),
    (Key::Delete, false) if shift => Some(Command::Cut),
    (Key::Delete, false) => del(Delete::Forward),
    (Key::Delete, true) => del(Delete::WordForward),
    (Key::Other(114), true) => Some(Command::Copy),
    (Key::Other(114), false) if shift => Some(Command::Paste),
    (Key::Enter, false) => Some(Command::InsertNewline),
    (Key::Tab, false) if shift => Some(Command::InsertBacktab),
    (Key::Tab, false) => Some(Command::InsertTab),
    (Key::Escape, false) => Some(Command::Cancel),
    (Key::Char(c), true) if c.eq_ignore_ascii_case(&'y') && !shift => Some(Command::Redo),
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn codes_are_macos_positions() {
    let cases = [
      (0x41, 0x1E, false, 0), // A
      (0x5A, 0x2C, false, 6), // Z
      (0x31, 0x02, false, 18),
      (VK_RETURN, 0x1C, false, 36),
      (VK_RETURN, 0x1C, true, 76),
      (VK_LEFT, 0x4B, true, 123),
      (VK_LEFT, 0x4B, false, 123),
      (VK_F1 + 11, 0x58, false, 111),
      (VK_SPACE, 0x39, false, 49),
    ];
    for (vk, scan, ext, want) in cases {
      assert_eq!(code(vk, scan, ext), want, "vk {vk:#x}");
    }
  }

  #[test]
  fn windows_editing_keys() {
    let ev = |key, mods| KeyEvent { key, text: None, code: 0, mods, repeat: false };
    let ctrl_shift = Mods { cmd: true, shift: true, ..Mods::NONE };
    for (key, mods, want) in [
      (Key::Left, Mods::CMD, Command::Move(Move::WordLeft, false)),
      (Key::Right, ctrl_shift, Command::Move(Move::WordRight, true)),
      (Key::Home, Mods::NONE, Command::Move(Move::LineStart, false)),
      (Key::End, Mods::CMD, Command::Move(Move::DocEnd, false)),
      (Key::Backspace, Mods::CMD, Command::Delete(Delete::WordBackward)),
      (Key::Delete, Mods::NONE, Command::Delete(Delete::Forward)),
      (Key::Char('y'), Mods::CMD, Command::Redo),
      (Key::Enter, Mods::NONE, Command::InsertNewline),
      (Key::Tab, Mods::SHIFT, Command::InsertBacktab),
    ] {
      assert_eq!(standard(&ev(key, mods)), Some(want), "{key:?} {mods:?}");
    }
    assert_eq!(standard(&ev(Key::Char('c'), Mods::CMD)), None);
  }
}
