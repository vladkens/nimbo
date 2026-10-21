// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Keys by their place on the keyboard, named as on a US layout, and the platforms' key codes of
//! those places: what a recording shortcut reports and what a global hotkey grabs.

/// A key that is not a modifier, by its place: `Char('a')` is the key A is on with a US layout,
/// whatever the current layout prints there. Letters and digits are their characters, `a`–`z` and
/// `0`–`9`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
  Char(char),
  /// F1–F20.
  F(u8),
  Space,
  Escape,
  Tab,
  Return,
  /// Backspace.
  Delete,
  ForwardDelete,
  Up,
  Down,
  Left,
  Right,
  Home,
  End,
  PageUp,
  PageDown,
  Minus,
  Equal,
  BracketLeft,
  BracketRight,
  Backslash,
  Semicolon,
  Quote,
  Comma,
  Period,
  Slash,
  Grave,
}

/// A modifier key by its place: a shortcut can be one alone, which tells left from right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModKey {
  LeftCtrl,
  RightCtrl,
  /// ⌥ Option on macOS.
  LeftAlt,
  RightAlt,
  LeftShift,
  RightShift,
  /// ⌘ Command on macOS, Super or Windows elsewhere.
  LeftMeta,
  RightMeta,
  /// The Fn key; Globe on newer Mac keyboards.
  Fn,
}

impl ModKey {
  pub const ALL: [ModKey; 9] = [
    ModKey::LeftCtrl,
    ModKey::RightCtrl,
    ModKey::LeftAlt,
    ModKey::RightAlt,
    ModKey::LeftShift,
    ModKey::RightShift,
    ModKey::LeftMeta,
    ModKey::RightMeta,
    ModKey::Fn,
  ];

  /// macOS's virtual key code of the key (`kVK_*`, Events.h), as a `flagsChanged` event has it.
  pub fn mac(self) -> u16 {
    match self {
      ModKey::LeftCtrl => 0x3B,
      ModKey::RightCtrl => 0x3E,
      ModKey::LeftAlt => 0x3A,
      ModKey::RightAlt => 0x3D,
      ModKey::LeftShift => 0x38,
      ModKey::RightShift => 0x3C,
      ModKey::LeftMeta => 0x37,
      ModKey::RightMeta => 0x36,
      ModKey::Fn => 0x3F,
    }
  }

  /// The bit of macOS's event flags (`NSEvent`'s and `CGEvent`'s alike) that is set while this
  /// key is down: the device-dependent ones (`NX_DEVICE*KEYMASK`, IOLLEvent.h) tell left from
  /// right; Fn has only its own.
  pub fn mac_flag(self) -> u64 {
    match self {
      ModKey::LeftCtrl => 0x0000_0001,
      ModKey::RightCtrl => 0x0000_2000,
      ModKey::LeftShift => 0x0000_0002,
      ModKey::RightShift => 0x0000_0004,
      ModKey::LeftMeta => 0x0000_0008,
      ModKey::RightMeta => 0x0000_0010,
      ModKey::LeftAlt => 0x0000_0020,
      ModKey::RightAlt => 0x0000_0040,
      ModKey::Fn => 0x0080_0000,
    }
  }

  /// The key at macOS's virtual key code `code`, if it is a modifier.
  pub fn of_mac(code: u16) -> Option<ModKey> {
    ModKey::ALL.into_iter().find(|k| k.mac() == code)
  }
}

const NAMED: [Key; 25] = [
  Key::Space,
  Key::Escape,
  Key::Tab,
  Key::Return,
  Key::Delete,
  Key::ForwardDelete,
  Key::Up,
  Key::Down,
  Key::Left,
  Key::Right,
  Key::Home,
  Key::End,
  Key::PageUp,
  Key::PageDown,
  Key::Minus,
  Key::Equal,
  Key::BracketLeft,
  Key::BracketRight,
  Key::Backslash,
  Key::Semicolon,
  Key::Quote,
  Key::Comma,
  Key::Period,
  Key::Slash,
  Key::Grave,
];

impl Key {
  /// Every key: letters, digits, F1–F20, then the named ones.
  pub fn all() -> impl Iterator<Item = Key> {
    let chars = ('a'..='z').chain('0'..='9').map(Key::Char);
    chars.chain((1..=20).map(Key::F)).chain(NAMED)
  }

  /// This platform's key code of the key's place: macOS's virtual key code, X11's hardware key
  /// code (evdev's plus 8, as GTK and XKB report it), Windows' virtual-key code.
  pub fn code(self) -> Option<u16> {
    #[cfg(target_os = "macos")]
    return self.mac();
    #[cfg(windows)]
    return self.vk();
    #[cfg(not(any(target_os = "macos", windows)))]
    return self.xkb();
  }

  /// The key at this platform's key code `code`, if it is one of these.
  pub fn of_code(code: u16) -> Option<Key> {
    Key::all().find(|k| k.code() == Some(code))
  }

  /// macOS's virtual key code (`kVK_*`, Events.h): letters, digits and punctuation are the
  /// places of the ANSI layout.
  pub fn mac(self) -> Option<u16> {
    mac(self)
  }

  /// X11's hardware key code (XKB's), as X11, Wayland and GTK report it: the evdev code
  /// (linux/input-event-codes.h) plus 8.
  pub fn xkb(self) -> Option<u16> {
    xkb(self)
  }

  /// Windows' virtual-key code; the punctuation keys' (`VK_OEM_*`) are the US layout's.
  pub fn vk(self) -> Option<u16> {
    vk(self)
  }
}

fn mac(key: Key) -> Option<u16> {
  const LETTERS: [u16; 26] = [
    0x00, 0x0B, 0x08, 0x02, 0x0E, 0x03, 0x05, 0x04, 0x22, 0x26, 0x28, 0x25, 0x2E, 0x2D, 0x1F, 0x23,
    0x0C, 0x0F, 0x01, 0x11, 0x20, 0x09, 0x0D, 0x07, 0x10, 0x06,
  ];
  const DIGITS: [u16; 10] = [0x1D, 0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1A, 0x1C, 0x19];
  const F_KEYS: [u16; 20] = [
    0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F, 0x69, 0x6B, 0x71, 0x6A,
    0x40, 0x4F, 0x50, 0x5A,
  ];
  Some(match key {
    Key::Char(c @ 'a'..='z') => LETTERS[(c as u8 - b'a') as usize],
    Key::Char(c @ '0'..='9') => DIGITS[(c as u8 - b'0') as usize],
    Key::Char(_) => return None,
    Key::F(n @ 1..=20) => F_KEYS[n as usize - 1],
    Key::F(_) => return None,
    Key::Space => 0x31,
    Key::Escape => 0x35,
    Key::Tab => 0x30,
    Key::Return => 0x24,
    Key::Delete => 0x33,
    Key::ForwardDelete => 0x75,
    Key::Up => 0x7E,
    Key::Down => 0x7D,
    Key::Left => 0x7B,
    Key::Right => 0x7C,
    Key::Home => 0x73,
    Key::End => 0x77,
    Key::PageUp => 0x74,
    Key::PageDown => 0x79,
    Key::Minus => 0x1B,
    Key::Equal => 0x18,
    Key::BracketLeft => 0x21,
    Key::BracketRight => 0x1E,
    Key::Backslash => 0x2A,
    Key::Semicolon => 0x29,
    Key::Quote => 0x27,
    Key::Comma => 0x2B,
    Key::Period => 0x2F,
    Key::Slash => 0x2C,
    Key::Grave => 0x32,
  })
}

fn xkb(key: Key) -> Option<u16> {
  const LETTERS: [u16; 26] = [
    30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45,
    21, 44,
  ];
  const DIGITS: [u16; 10] = [11, 2, 3, 4, 5, 6, 7, 8, 9, 10];
  const F_KEYS: [u16; 20] =
    [59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 87, 88, 183, 184, 185, 186, 187, 188, 189, 190];
  let evdev = match key {
    Key::Char(c @ 'a'..='z') => LETTERS[(c as u8 - b'a') as usize],
    Key::Char(c @ '0'..='9') => DIGITS[(c as u8 - b'0') as usize],
    Key::Char(_) => return None,
    Key::F(n @ 1..=20) => F_KEYS[n as usize - 1],
    Key::F(_) => return None,
    Key::Space => 57,
    Key::Escape => 1,
    Key::Tab => 15,
    Key::Return => 28,
    Key::Delete => 14,
    Key::ForwardDelete => 111,
    Key::Up => 103,
    Key::Down => 108,
    Key::Left => 105,
    Key::Right => 106,
    Key::Home => 102,
    Key::End => 107,
    Key::PageUp => 104,
    Key::PageDown => 109,
    Key::Minus => 12,
    Key::Equal => 13,
    Key::BracketLeft => 26,
    Key::BracketRight => 27,
    Key::Backslash => 43,
    Key::Semicolon => 39,
    Key::Quote => 40,
    Key::Comma => 51,
    Key::Period => 52,
    Key::Slash => 53,
    Key::Grave => 41,
  };

  Some(evdev + 8)
}

fn vk(key: Key) -> Option<u16> {
  Some(match key {
    Key::Char(c @ ('a'..='z' | '0'..='9')) => c.to_ascii_uppercase() as u16,
    Key::Char(_) => return None,
    Key::F(n @ 1..=20) => 0x70 + n as u16 - 1,
    Key::F(_) => return None,
    Key::Space => 0x20,
    Key::Escape => 0x1b,
    Key::Tab => 0x09,
    Key::Return => 0x0d,
    Key::Delete => 0x08,
    Key::ForwardDelete => 0x2e,
    Key::Up => 0x26,
    Key::Down => 0x28,
    Key::Left => 0x25,
    Key::Right => 0x27,
    Key::Home => 0x24,
    Key::End => 0x23,
    Key::PageUp => 0x21,
    Key::PageDown => 0x22,
    Key::Minus => 0xbd,
    Key::Equal => 0xbb,
    Key::BracketLeft => 0xdb,
    Key::BracketRight => 0xdd,
    Key::Backslash => 0xdc,
    Key::Semicolon => 0xba,
    Key::Quote => 0xde,
    Key::Comma => 0xbc,
    Key::Period => 0xbe,
    Key::Slash => 0xbf,
    Key::Grave => 0xc0,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn every_key_once_with_its_own_code_on_each_platform() {
    let keys: Vec<Key> = Key::all().collect();
    assert_eq!(keys.len(), 26 + 10 + 20 + 25);
    for table in [mac, xkb, vk] {
      let mut codes: Vec<u16> = keys.iter().map(|&k| table(k).unwrap()).collect();
      codes.sort_unstable();
      codes.dedup();
      assert_eq!(codes.len(), keys.len());
    }

    assert_eq!((Key::Char('!').code(), Key::F(21).code()), (None, None));
    assert!(keys.iter().all(|&k| Key::of_code(k.code().unwrap()) == Some(k)));
  }

  #[test]
  fn modifier_keys_have_their_own_mac_codes() {
    let mut codes: Vec<u16> = ModKey::ALL.iter().map(|k| k.mac()).collect();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), ModKey::ALL.len());
    assert!(ModKey::ALL.iter().all(|&k| ModKey::of_mac(k.mac()) == Some(k)));
    assert_eq!((ModKey::of_mac(0x3D), ModKey::of_mac(0x31)), (Some(ModKey::RightAlt), None));
  }

  #[test]
  fn codes_as_the_systems_report_them() {
    let keys = [Key::Char('a'), Key::Char('q'), Key::Char('1'), Key::Space, Key::F(12)];
    // xev on a US layout.
    assert_eq!(keys.map(|k| xkb(k).unwrap()), [38, 24, 10, 65, 96]);
    assert_eq!(keys.map(|k| mac(k).unwrap()), [0x00, 0x0C, 0x12, 0x31, 0x6F]);
    assert_eq!(keys.map(|k| vk(k).unwrap()), [0x41, 0x51, 0x31, 0x20, 0x7B]);
    assert_eq!(
      (xkb(Key::Escape), mac(Key::Escape), vk(Key::Escape)),
      (Some(9), Some(0x35), Some(0x1b))
    );
  }
}
