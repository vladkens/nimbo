// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Keys on macOS: virtual key codes (`kVK_*`) as W3C [Code]s ([code]), and what a key types on
//! the ASCII-capable keyboard layout, the one macOS matches Cmd shortcuts against while a
//! non-Latin layout is active: on a Russian layout Cmd-C reports `'с'`
//! (Cyrillic) in `charactersIgnoringModifiers`, and should still be `"cmd+c"`. A layout that
//! maps keys anew under Cmd ("Dvorak – QWERTY ⌘" types QWERTY's letters then) gives that
//! mapping, which `charactersIgnoringModifiers` drops.
//!
//! Text Input Sources (HIToolbox) and `UCKeyTranslate` (CarbonCore), declared here: the objc2
//! crates don't cover them.

use std::ffi::c_void;

use keyboard_types::Code;

use crate::keys;

type CFTypeRef = *const c_void;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
  static kTISPropertyUnicodeKeyLayoutData: CFTypeRef;
  fn TISCopyCurrentASCIICapableKeyboardLayoutInputSource() -> CFTypeRef;
  fn TISGetInputSourceProperty(src: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
  fn LMGetKbdType() -> u8;
  fn UCKeyTranslate(
    layout: *const c_void,
    code: u16,
    action: u16,
    mods: u32,
    kbd_type: u32,
    opts: u32,
    dead: *mut u32,
    max: usize,
    len: *mut usize,
    chars: *mut u16,
  ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
  fn CFDataGetBytePtr(data: CFTypeRef) -> *const u8;
  fn CFRelease(cf: CFTypeRef);
}

const KEY_ACTION_DOWN: u16 = 0;
const NO_DEAD_KEYS: u32 = 1;
/// Carbon's `cmdKey` (1 << 8) and `shiftKey` (1 << 9) as `UCKeyTranslate` takes modifiers:
/// shifted right by 8.
const CMD: u32 = 0x01;
const SHIFT: u32 = 0x02;

/// The place of the key at virtual key code `kvk` (Events.h), as Chromium and Firefox map them
/// (`dom_code_data.inc`): Help is Insert, Keypad Clear is Num Lock.
pub fn code(kvk: u16) -> Code {
  use Code::*;
  #[rustfmt::skip]
  const KVK: [Code; 128] = [
    KeyA, KeyS, KeyD, KeyF, KeyH, KeyG, KeyZ, KeyX, // 0x00
    KeyC, KeyV, IntlBackslash, KeyB, KeyQ, KeyW, KeyE, KeyR,
    KeyY, KeyT, Digit1, Digit2, Digit3, Digit4, Digit6, Digit5, // 0x10
    Equal, Digit9, Digit7, Minus, Digit8, Digit0, BracketRight, KeyO,
    KeyU, BracketLeft, KeyI, KeyP, Enter, KeyL, KeyJ, Quote, // 0x20
    KeyK, Semicolon, Backslash, Comma, Slash, KeyN, KeyM, Period,
    Tab, Space, Backquote, Backspace, Unidentified, Escape, MetaRight, MetaLeft, // 0x30
    ShiftLeft, CapsLock, AltLeft, ControlLeft, ShiftRight, AltRight, ControlRight, Fn,
    F17, NumpadDecimal, Unidentified, NumpadMultiply, // 0x40
    Unidentified, NumpadAdd, Unidentified, NumLock,
    AudioVolumeUp, AudioVolumeDown, AudioVolumeMute, NumpadDivide,
    NumpadEnter, Unidentified, NumpadSubtract, F18,
    F19, NumpadEqual, Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, // 0x50
    Numpad6, Numpad7, F20, Numpad8, Numpad9, IntlYen, IntlRo, NumpadComma,
    F5, F6, F7, F3, F8, F9, Lang2, F11, // 0x60
    Lang1, F13, F16, F14, Unidentified, F10, ContextMenu, F12,
    Unidentified, F15, Insert, Home, PageUp, Delete, F4, End, // 0x70
    F2, PageDown, F1, ArrowLeft, ArrowRight, ArrowDown, ArrowUp, Unidentified,
  ];
  KVK.get(kvk as usize).copied().unwrap_or(Unidentified)
}

/// The bit of the event flags (`NSEvent`'s and `CGEvent`'s alike) set while modifier key `code`
/// is down: the device-dependent ones (`NX_DEVICE*KEYMASK`, IOLLEvent.h) tell left from right;
/// Fn has only its own. `None` for other keys.
pub fn device_flag(code: Code) -> Option<u64> {
  Some(match code {
    Code::ControlLeft => 0x0000_0001,
    Code::ShiftLeft => 0x0000_0002,
    Code::ShiftRight => 0x0000_0004,
    Code::MetaLeft => 0x0000_0008,
    Code::MetaRight => 0x0000_0010,
    Code::AltLeft => 0x0000_0020,
    Code::AltRight => 0x0000_0040,
    Code::ControlRight => 0x0000_2000,
    Code::Fn => 0x0080_0000,
    _ => return None,
  })
}

/// The character key `kvk` types on the ASCII-capable layout, Shift applied or not; a US
/// keyboard's when the layout can't be read. Main thread only (Text Input Sources).
pub fn ascii_char(kvk: u16, shift: bool) -> Option<char> {
  let c = with_layout(|src| translate(src, kvk, shift, false)).flatten();
  ascii(c).or_else(|| keys::us_char(code(kvk), shift))
}

/// The character key `code` types with Cmd held, Shift applied or not, on an ASCII-capable
/// layout that maps it anew under Cmd ("Dvorak – QWERTY ⌘": J's key is C then); none on the
/// others, whose Cmd mapping is their own (US gives `=` for Cmd-Shift-=, not the `+` it types).
/// Main thread only.
pub fn cmd_char(code: u16, shift: bool) -> Option<char> {
  with_layout(|src| cmd_char_in(src, code, shift)).flatten()
}

/// [cmd_char] on the layout of input source `src`, a live `TISInputSourceRef`.
fn cmd_char_in(src: CFTypeRef, code: u16, shift: bool) -> Option<char> {
  let plain = translate(src, code, false, false);
  let cmd = translate(src, code, false, true);
  ascii(translate(src, code, shift, true)).filter(|_| cmd != plain)
}

fn ascii(c: Option<char>) -> Option<char> {
  c.filter(|c| c.is_ascii() && !c.is_ascii_control())
}

/// `f` on the current ASCII-capable layout's input source; none when there is none.
fn with_layout<R>(f: impl FnOnce(CFTypeRef) -> R) -> Option<R> {
  // SAFETY: a Copy function returns a retained source or null, released after `f`.
  unsafe {
    let src = TISCopyCurrentASCIICapableKeyboardLayoutInputSource();
    if src.is_null() {
      return None;
    }

    let r = f(src);
    CFRelease(src);
    Some(r)
  }
}

/// What `code` types on the layout of input source `src`, a live `TISInputSourceRef`.
fn translate(src: CFTypeRef, code: u16, shift: bool, cmd: bool) -> Option<char> {
  // SAFETY: the property is a CFData owned by the source (Get rule), read while it lives.
  unsafe {
    let data = TISGetInputSourceProperty(src, kTISPropertyUnicodeKeyLayoutData);
    let mut out = None;
    if !data.is_null() {
      let layout = CFDataGetBytePtr(data) as *const c_void;
      let (mut dead, mut len, mut buf) = (0u32, 0usize, [0u16; 4]);
      let mods = if shift { SHIFT } else { 0 } | if cmd { CMD } else { 0 };
      let kbd = LMGetKbdType() as u32;
      let status = UCKeyTranslate(
        layout,
        code,
        KEY_ACTION_DOWN,
        mods,
        kbd,
        NO_DEAD_KEYS,
        &mut dead,
        buf.len(),
        &mut len,
        buf.as_mut_ptr(),
      );
      if status == 0 && len > 0 {
        let units = buf[..len.min(buf.len())].iter().copied();
        out = char::decode_utf16(units).next().and_then(|c| c.ok());
      }
    }

    out
  }
}

#[cfg(test)]
mod tests {
  use objc2_core_foundation::{CFDictionary, CFRetained, CFString};

  use super::*;

  #[link(name = "Carbon", kind = "framework")]
  unsafe extern "C" {
    static kTISPropertyInputSourceID: CFTypeRef;
    fn TISCreateInputSourceList(filter: CFTypeRef, all_installed: u8) -> CFTypeRef;
  }

  #[link(name = "CoreFoundation", kind = "framework")]
  unsafe extern "C" {
    fn CFArrayGetCount(a: CFTypeRef) -> isize;
    fn CFArrayGetValueAtIndex(a: CFTypeRef, i: isize) -> CFTypeRef;
  }

  /// `f` on the installed layout `id` (enabled or not).
  fn on_layout(id: &str, f: impl FnOnce(CFTypeRef)) {
    // SAFETY: the key is a CFString constant; the list is retained (Create rule) and released
    // after its source is used.
    unsafe {
      let key = &*(kTISPropertyInputSourceID as *const CFString);
      let id = CFString::from_str(id);
      let filter: CFRetained<CFDictionary<CFString, CFString>> =
        CFDictionary::from_slices(&[key], &[&*id]);
      let list = TISCreateInputSourceList((&raw const *filter).cast(), 1);
      assert!(!list.is_null() && CFArrayGetCount(list) == 1, "{id} is installed");
      f(CFArrayGetValueAtIndex(list, 0));
      CFRelease(list);
    }
  }

  /// Codes as the old tables had them: US ANSI places, the keypad, F-keys, the modifiers.
  #[test]
  fn virtual_key_codes() {
    let kvks = [0x00, 0x0C, 0x12, 0x31, 0x6F, 0x35, 0x4C, 0x52, 0x72, 0x3D, 0x3F, 0x7F, 0x200];
    let want = [
      Code::KeyA,
      Code::KeyQ,
      Code::Digit1,
      Code::Space,
      Code::F12,
      Code::Escape,
      Code::NumpadEnter,
      Code::Numpad0,
      Code::Insert,
      Code::AltRight,
      Code::Fn,
      Code::Unidentified,
      Code::Unidentified,
    ];
    assert_eq!(kvks.map(code), want);
    // Each US character key at its own code.
    let typing = (0..128).filter(|&k| keys::us_char(code(k), false).is_some()).count();
    assert_eq!(typing, 48);
    assert_eq!((device_flag(Code::AltRight), device_flag(Code::KeyA)), (Some(0x40), None));
  }

  /// "Dvorak – QWERTY ⌘" types J at the QWERTY C key (code 8) and C with Cmd held, `]` at `=`
  /// (24) and `=` or `+` with Cmd; US maps nothing anew under Cmd, so its keys are what they type.
  #[test]
  fn cmd_takes_a_layouts_own_cmd_mapping() {
    on_layout("com.apple.keylayout.DVORAK-QWERTYCMD", |src| {
      assert_eq!(translate(src, 8, false, false), Some('j'));
      assert_eq!(cmd_char_in(src, 8, false), Some('c'));
      assert_eq!(cmd_char_in(src, 24, false), Some('='));
      assert_eq!(cmd_char_in(src, 24, true), Some('+'));
    });
    on_layout("com.apple.keylayout.US", |src| {
      assert_eq!(cmd_char_in(src, 8, false), None);
      assert_eq!(cmd_char_in(src, 24, true), None);
    });
  }
}
