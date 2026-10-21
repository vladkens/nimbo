// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Keys on Windows: virtual keys and scan codes to [KeyEvent]s with W3C [Code]s, the layout's
//! characters through ToUnicodeEx, and the commands Windows' text controls give the keys that
//! type nothing ([standard]).
//!
//! Ctrl is the command key ([crate::keys::CMD]): bindings written `cmd+c` are Ctrl-C here, and
//! `ctrl` is the Windows key. Alt is Alt.

use keyboard_types::{Code, Modifiers, NamedKey};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
  GetAsyncKeyState, GetKeyState, GetKeyboardLayout, GetKeyboardState, ToUnicodeEx, VK_APPS,
  VK_BACK, VK_CONTROL, VK_DELETE, VK_DIVIDE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F20, VK_HOME,
  VK_INSERT, VK_LEFT, VK_LWIN, VK_MENU, VK_NEXT, VK_NUMLOCK, VK_PAUSE, VK_PRIOR, VK_RETURN,
  VK_RIGHT, VK_RWIN, VK_SHIFT, VK_SNAPSHOT, VK_SPACE, VK_TAB, VK_UP,
};

use crate::keys::{self, Command, Key, KeyEvent};
use crate::platform::scan_code;

/// The keypad's scan codes that send navigation virtual keys with Num Lock off (7 8 9, 4 5 6,
/// 1 2 3, 0 and .), never with the extended bit the dedicated keys have.
fn keypad_scan(scan: u16, extended: bool) -> bool {
  !extended && matches!(scan, 0x47..=0x49 | 0x4B..=0x4D | 0x4F..=0x53)
}

/// The place of a key: the keypad by its scan code whatever Num Lock makes it send, other named
/// keys by virtual key (so a posted message with no scan code maps too), the rest by scan code.
pub fn code(vk: u16, scan: u16, extended: bool) -> Code {
  let named = navigation(vk).or_else(|| named(vk)).and_then(|k| Key::Named(k).code());
  match vk {
    _ if keypad_scan(scan, extended) => scan_code(scan),
    VK_RETURN if extended => Code::NumpadEnter,
    VK_DIVIDE => Code::NumpadDivide, // extended: not by scan code
    VK_NUMLOCK => Code::NumLock,     // extended; Pause has its scan code
    VK_PAUSE => Code::Pause,
    VK_APPS => Code::ContextMenu,
    VK_SNAPSHOT => Code::PrintScreen,
    VK_SPACE => Code::Space,
    _ if let Some(code) = named => code,
    _ if extended => Code::Unidentified,
    _ => scan_code(scan),
  }
}

/// The named keys that are not on the keypad, by virtual key.
fn named(vk: u16) -> Option<NamedKey> {
  Some(match vk {
    VK_RETURN => NamedKey::Enter,
    VK_TAB => NamedKey::Tab,
    VK_BACK => NamedKey::Backspace,
    VK_ESCAPE => NamedKey::Escape,
    VK_F1..=VK_F20 => keys::f_key((vk - VK_F1) as u8 + 1)?.1,
    _ => return None,
  })
}

fn down(vk: u16) -> bool {
  unsafe { GetKeyState(vk as i32) as u16 & 0x8000 != 0 }
}

/// The modifiers held now (as of the message being handled), the Windows key as META.
pub fn mods() -> Modifiers {
  mods_by(down)
}

/// The modifiers held on the keyboard at this moment, whatever messages wait: on gaining the
/// focus, after keys went up or down in another window.
pub fn mods_held() -> Modifiers {
  mods_by(|vk| unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 })
}

fn mods_by(down: impl Fn(u16) -> bool) -> Modifiers {
  let mut m = Modifiers::empty();
  m.set(Modifiers::SHIFT, down(VK_SHIFT));
  m.set(Modifiers::CONTROL, down(VK_CONTROL));
  m.set(Modifiers::ALT, down(VK_MENU));
  m.set(Modifiers::META, down(VK_LWIN) || down(VK_RWIN));
  m
}

/// Whether `vk` is a modifier key: it changes [Modifiers], it isn't a key press.
pub fn is_modifier(vk: u16) -> bool {
  matches!(vk, VK_SHIFT | VK_CONTROL | VK_MENU | VK_LWIN | VK_RWIN | 0xA0..=0xA5)
}

/// The modifier key a key message is about, by its side: the left and right keys share a
/// virtual key, the scan code (Shift) or the extended bit (Ctrl, Alt) tell them apart.
pub fn mod_key(vk: u16, scan: u16, extended: bool) -> Option<Code> {
  let side = |left, right| Some(if extended { right } else { left });
  match vk {
    VK_SHIFT => Some(if scan == 0x36 { Code::ShiftRight } else { Code::ShiftLeft }),
    VK_CONTROL => side(Code::ControlLeft, Code::ControlRight),
    VK_MENU => side(Code::AltLeft, Code::AltRight),
    VK_LWIN => Some(Code::MetaLeft),
    VK_RWIN => Some(Code::MetaRight),
    0xA0 => Some(Code::ShiftLeft),
    0xA1 => Some(Code::ShiftRight),
    0xA2 => Some(Code::ControlLeft),
    0xA3 => Some(Code::ControlRight),
    0xA4 => Some(Code::AltLeft),
    0xA5 => Some(Code::AltRight),
    _ => None,
  }
}

/// What a key types on the current layout with the key state `state`, without touching the
/// layout's dead-key state, and whether it is a dead key; `None` for nothing or more than one
/// character.
fn typed(vk: u16, scan: u16, state: &[u8; 256]) -> Option<(char, bool)> {
  let mut buf = [0u16; 8];
  // Flag 4: keep the keyboard state (dead keys) as it is (Windows 10 1607).
  let layout = unsafe { GetKeyboardLayout(0) };
  let n =
    unsafe { ToUnicodeEx(vk as u32, scan as u32, state.as_ptr(), buf.as_mut_ptr(), 8, 4, layout) };
  decoded(n, &buf)
}

/// ToUnicodeEx's result: `n` units of `buf`, a negative `n` for a dead key (its spacing form).
fn decoded(n: i32, buf: &[u16]) -> Option<(char, bool)> {
  let units = &buf[..(n.unsigned_abs() as usize).min(buf.len())];
  let mut cs = char::decode_utf16(units.iter().copied());
  match (cs.next(), cs.next()) {
    (Some(Ok(c)), None) => Some((c, n < 0)),
    _ => None,
  }
}

/// The key a navigation virtual key names wherever it is: the keypad's with Num Lock off too.
fn navigation(vk: u16) -> Option<NamedKey> {
  Some(match vk {
    VK_LEFT => NamedKey::ArrowLeft,
    VK_RIGHT => NamedKey::ArrowRight,
    VK_UP => NamedKey::ArrowUp,
    VK_DOWN => NamedKey::ArrowDown,
    VK_HOME => NamedKey::Home,
    VK_END => NamedKey::End,
    VK_PRIOR => NamedKey::PageUp,
    VK_NEXT => NamedKey::PageDown,
    VK_DELETE => NamedKey::Delete,
    VK_INSERT => NamedKey::Insert,
    _ => return None,
  })
}

/// A key press: named keys by place (the keypad's navigation keys by virtual key), the rest by
/// the character the key types with Shift alone (with Ctrl or Windows, a non-ASCII one is the US
/// layout's, so shortcuts work on any layout); `text` what it types with every modifier, unless
/// Ctrl or Windows is held or it is a dead key.
pub fn key_event(vk: u16, scan: u16, extended: bool, repeat: bool) -> KeyEvent {
  let (code, mods) = (code(vk, scan, extended), mods());
  let shortcut = mods.intersects(Modifiers::CONTROL | Modifiers::META);
  let mut state = [0u8; 256];
  if mods.shift() {
    state[VK_SHIFT as usize] = 0x80;
  }

  let named = Key::named(code).or_else(|| navigation(vk).map(Key::Named));
  let key = named.unwrap_or_else(|| match typed(vk, scan, &state).map(|t| t.0) {
    Some(c) if shortcut && !c.is_ascii() => {
      keys::us_char(code, mods.shift()).map_or(Key::Char(c), Key::Char)
    }
    Some(c) if !c.is_control() => Key::Char(c),
    _ => Key::Named(NamedKey::Unidentified),
  });
  let text = match key {
    Key::Char(_) if !shortcut => {
      unsafe { GetKeyboardState(state.as_mut_ptr()) };
      let typed = typed(vk, scan, &state).filter(|t| !t.1).map(|t| t.0);
      typed.filter(|c| keys::printable(c.encode_utf8(&mut [0; 4])))
    }
    _ => None,
  };

  KeyEvent { key, text, code, mods, repeat }
}

/// The command a Windows text control gives a key that types nothing (Ctrl is `cmd`): Ctrl
/// with arrows moves by words (and paragraphs), Home and End to the line's ends and with Ctrl
/// the document's, Page Up and Down move, Ctrl-Backspace and Ctrl-Delete delete words, Ctrl-Y
/// redoes, and the old Shift-Delete, Ctrl-Insert and Shift-Insert cut, copy and paste; Ctrl-Enter
/// does nothing.
pub fn standard(k: &KeyEvent) -> Option<Command> {
  let (shift, ctrl, alt, cmd) = keys::flags(k.mods);
  match (k.key, cmd, shift) {
    _ if ctrl || alt => None,
    (Key::Named(NamedKey::Delete), false, true) => Some(Command::Cut),
    (Key::Named(NamedKey::Insert), true, _) => Some(Command::Copy),
    (Key::Named(NamedKey::Insert), false, true) => Some(Command::Paste),
    (Key::Char('y' | 'Y'), true, false) => Some(Command::Redo),
    (Key::Named(NamedKey::Enter), true, _) => None,
    // The rest as GTK's: [keys::pc].
    _ => keys::pc(k),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::keys::{Delete, Move};

  #[test]
  fn codes_by_place() {
    let cases = [
      (0x41, 0x1E, false, Code::KeyA),
      (0x5A, 0x2C, false, Code::KeyZ),
      (0x31, 0x02, false, Code::Digit1),
      (VK_RETURN, 0x1C, false, Code::Enter),
      (VK_RETURN, 0x1C, true, Code::NumpadEnter),
      (VK_LEFT, 0x4B, true, Code::ArrowLeft),
      (VK_LEFT, 0, false, Code::ArrowLeft), // posted, no scan code
      (VK_LEFT, 0x4B, false, Code::Numpad4), // keypad 4, Num Lock off
      (0x64, 0x4B, false, Code::Numpad4),   // keypad 4, Num Lock on
      (VK_DELETE, 0x53, true, Code::Delete),
      (VK_DELETE, 0x53, false, Code::NumpadDecimal),
      (VK_DIVIDE, 0x35, true, Code::NumpadDivide),
      (0xBF, 0x35, false, Code::Slash),
      (VK_F1 + 11, 0x58, false, Code::F12),
      (VK_F1 + 12, 0x64, false, Code::F13),
      (VK_F20, 0x6B, false, Code::F20),
      (VK_SPACE, 0x39, false, Code::Space),
      (VK_PAUSE, 0x45, false, Code::Pause),
      (VK_NUMLOCK, 0x45, true, Code::NumLock),
      (VK_APPS, 0x5D, true, Code::ContextMenu),
      (VK_SNAPSHOT, 0x37, true, Code::PrintScreen),
      (0xAD, 0x20, true, Code::Unidentified), // volume mute
    ];
    for (vk, scan, ext, want) in cases {
      assert_eq!(code(vk, scan, ext), want, "vk {vk:#x}");
    }
  }

  #[test]
  fn keypad_keys_keep_their_place() {
    let k = key_event(VK_LEFT, 0x4B, false, false);
    let left = Key::Named(NamedKey::ArrowLeft);
    assert_eq!((k.key, k.code, k.text), (left, Code::Numpad4, None), "keypad 4, Num Lock off");
    let k = key_event(VK_LEFT, 0x4B, true, false);
    assert_eq!((k.key, k.code), (left, Code::ArrowLeft));
    let k = key_event(VK_APPS, 0x5D, true, false);
    assert_eq!((k.key, k.code), (Key::Named(NamedKey::ContextMenu), Code::ContextMenu));
    let k = key_event(VK_INSERT, 0x52, false, false);
    let insert = Key::Named(NamedKey::Insert);
    assert_eq!((k.key, k.code), (insert, Code::Numpad0), "keypad 0 is Insert, at its place");
    let ctrl = KeyEvent { mods: Modifiers::CONTROL, ..k };
    assert_eq!(standard(&ctrl), Some(Command::Copy));
  }

  #[test]
  fn dead_keys_type_nothing() {
    // `^` as ToUnicodeEx gives a dead key (-1) and a typed one (1).
    assert_eq!(decoded(-1, &[0x5E, 0]), Some(('^', true)));
    assert_eq!(decoded(1, &[0x5E, 0]), Some(('^', false)));
    assert_eq!(decoded(0, &[0x5E, 0]), None);
    assert_eq!(decoded(2, &[0x61, 0x62]), None, "two characters");
  }

  #[test]
  fn windows_editing_keys() {
    use NamedKey as N;
    let ev =
      |key, mods| KeyEvent { key, text: None, code: Code::Unidentified, mods, repeat: false };
    let ctrl = Modifiers::CONTROL;
    for (key, mods, want) in [
      (Key::Named(N::ArrowLeft), ctrl, Command::Move(Move::WordLeft, false)),
      (Key::Named(N::ArrowRight), ctrl | Modifiers::SHIFT, Command::Move(Move::WordRight, true)),
      (Key::Named(N::Home), Modifiers::empty(), Command::Move(Move::LineStart, false)),
      (Key::Named(N::End), ctrl, Command::Move(Move::DocEnd, false)),
      (Key::Named(N::Backspace), ctrl, Command::Delete(Delete::WordBackward)),
      (Key::Named(N::Delete), Modifiers::empty(), Command::Delete(Delete::Forward)),
      (Key::Char('y'), ctrl, Command::Redo),
      (Key::Named(N::Enter), Modifiers::empty(), Command::InsertNewline),
      (Key::Named(N::Tab), Modifiers::SHIFT, Command::InsertBacktab),
    ] {
      assert_eq!(standard(&ev(key, mods)), Some(want), "{key:?} {mods:?}");
    }
    assert_eq!(standard(&ev(Key::Char('c'), ctrl)), None);
    assert_eq!(standard(&ev(Key::Named(N::Enter), ctrl)), None, "Ctrl-Enter");
    assert_eq!(standard(&ev(Key::Named(N::Delete), Modifiers::SHIFT)), Some(Command::Cut));
    assert_eq!(standard(&ev(Key::Char('y'), ctrl | Modifiers::SHIFT)), None);
    assert_eq!(standard(&ev(Key::Named(N::ArrowLeft), Modifiers::META)), None, "the Windows key");
  }
}
