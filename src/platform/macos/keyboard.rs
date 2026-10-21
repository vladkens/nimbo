// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What a key types on the ASCII-capable keyboard layout, the one macOS matches Cmd shortcuts
//! against while a non-Latin layout is active: on a Russian layout Cmd-C reports `'с'`
//! (Cyrillic) in `charactersIgnoringModifiers`, and should still be `"cmd-c"`.
//!
//! Text Input Sources (HIToolbox) and `UCKeyTranslate` (CarbonCore), declared here: the objc2
//! crates don't cover them.

use std::ffi::c_void;

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
/// Carbon's `shiftKey` (1 << 9) as `UCKeyTranslate` takes modifiers: shifted right by 8.
const SHIFT: u32 = 0x02;

/// The character key `code` types on the ASCII-capable layout, Shift applied or not; a US
/// keyboard's when the layout can't be read. Main thread only (Text Input Sources).
pub fn ascii_char(code: u16, shift: bool) -> Option<char> {
  let c = translate(code, shift).filter(|c| c.is_ascii() && !c.is_ascii_control());
  c.or_else(|| keys::us_char(code, shift))
}

fn translate(code: u16, shift: bool) -> Option<char> {
  // SAFETY: a Copy function returns a retained source or null, released below; the property is
  // a CFData owned by the source (Get rule), read while the source lives.
  unsafe {
    let src = TISCopyCurrentASCIICapableKeyboardLayoutInputSource();
    if src.is_null() {
      return None;
    }

    let data = TISGetInputSourceProperty(src, kTISPropertyUnicodeKeyLayoutData);
    let mut out = None;
    if !data.is_null() {
      let layout = CFDataGetBytePtr(data) as *const c_void;
      let (mut dead, mut len, mut buf) = (0u32, 0usize, [0u16; 4]);
      let mods = if shift { SHIFT } else { 0 };
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

    CFRelease(src);
    out
  }
}
