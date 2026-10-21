// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! How hotkeys are written ([Hotkey]): a chord or a modifier key alone.
//!
//! Hotkeys as settings spell them: a chord of modifiers and one key joined by `+`, such as
//! `option+space` or `ctrl+option+cmd+F12`, or one modifier key alone, left or right, such as
//! `right_option` or `fn` ([Hotkey]; macOS only). Case does not matter; `control` / `alt` /
//! `opt` / `command` and the like are accepted for the modifiers. The grammar and the key names
//! are the same on every platform; each maps [Key] to its own key codes ([Key::code]).

mod keys;

use std::fmt;

pub use keys::{Key, ModKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
  pub ctrl: bool,
  pub option: bool,
  pub shift: bool,
  pub cmd: bool,
}

impl Modifiers {
  /// The modifiers held, as the platform shows them before a key: `⌃⌥` on macOS, `Ctrl+Alt+`
  /// elsewhere.
  pub fn label(&self) -> String {
    if cfg!(target_os = "macos") {
      Chord { modifiers: *self, key: Key::Space }.symbols().replace("Space", "")
    } else {
      self.names()
    }
  }

  fn names(&self) -> String {
    let m = self;
    let cmd = if cfg!(windows) { "Win+" } else { "Super+" };
    let names = [(m.ctrl, "Ctrl+"), (m.option, "Alt+"), (m.shift, "Shift+"), (m.cmd, cmd)];
    names.into_iter().filter(|(on, _)| *on).map(|(_, n)| n).collect()
  }
}

/// Names of the keys besides letters, digits and F-keys; the first one of a key is how it is
/// written back.
const NAMES: &[(&str, Key)] = &[
  ("space", Key::Space),
  ("escape", Key::Escape),
  ("esc", Key::Escape),
  ("tab", Key::Tab),
  ("return", Key::Return),
  ("enter", Key::Return),
  ("delete", Key::Delete),
  ("backspace", Key::Delete),
  ("forwarddelete", Key::ForwardDelete),
  ("up", Key::Up),
  ("down", Key::Down),
  ("left", Key::Left),
  ("right", Key::Right),
  ("home", Key::Home),
  ("end", Key::End),
  ("pageup", Key::PageUp),
  ("pagedown", Key::PageDown),
  ("minus", Key::Minus),
  ("-", Key::Minus),
  ("equal", Key::Equal),
  ("=", Key::Equal),
  ("bracketleft", Key::BracketLeft),
  ("[", Key::BracketLeft),
  ("bracketright", Key::BracketRight),
  ("]", Key::BracketRight),
  ("backslash", Key::Backslash),
  ("\\", Key::Backslash),
  ("semicolon", Key::Semicolon),
  (";", Key::Semicolon),
  ("quote", Key::Quote),
  ("'", Key::Quote),
  ("comma", Key::Comma),
  (",", Key::Comma),
  ("period", Key::Period),
  (".", Key::Period),
  ("slash", Key::Slash),
  ("/", Key::Slash),
  ("grave", Key::Grave),
  ("`", Key::Grave),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
  pub modifiers: Modifiers,
  pub key: Key,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChordError {
  Empty,
  Unknown(String),
  TwoKeys(String, String),
  NoKey,
  Fn,
  ModifierAlone,
}

impl fmt::Display for ChordError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Empty => f.write_str("the hotkey is empty"),
      Self::Unknown(k) => write!(f, "unknown key {k:?} in the hotkey"),
      Self::TwoKeys(a, b) => write!(f, "the hotkey has two keys: {a:?} and {b:?}"),
      Self::NoKey => {
        f.write_str("the hotkey has only modifiers; it needs a key, such as ctrl+option+space")
      }
      Self::Fn => f.write_str("the fn key cannot be part of a hotkey"),
      Self::ModifierAlone => f.write_str("a modifier key alone works as the hotkey only on macOS"),
    }
  }
}

impl std::error::Error for ChordError {}

impl Chord {
  pub fn parse(text: &str) -> Result<Self, ChordError> {
    let text = text.trim();
    if text.is_empty() {
      return Err(ChordError::Empty);
    }

    if modifier_key(text).is_some() {
      return Err(ChordError::ModifierAlone);
    }

    let mut modifiers = Modifiers::default();
    let mut key: Option<(Key, String)> = None;
    // `+` itself is no key name, so splitting on it is unambiguous.
    for part in text.split('+').map(str::trim) {
      let name = part.to_lowercase();
      let flag = match name.as_str() {
        "ctrl" | "control" | "ctl" => &mut modifiers.ctrl,
        "option" | "opt" | "alt" => &mut modifiers.option,
        "shift" => &mut modifiers.shift,
        "cmd" | "command" | "super" | "meta" => &mut modifiers.cmd,
        "fn" | "function" => return Err(ChordError::Fn),
        _ => {
          let parsed = parse_key(&name).ok_or_else(|| ChordError::Unknown(part.to_owned()))?;
          if let Some((_, first)) = &key {
            return Err(ChordError::TwoKeys(first.clone(), part.to_owned()));
          }

          key = Some((parsed, part.to_owned()));
          continue;
        }
      };

      *flag = true;
    }

    let (key, _) = key.ok_or(ChordError::NoKey)?;
    Ok(Self { modifiers, key })
  }

  /// As the platform shows it: [Chord::symbols] on macOS, [Chord::names] elsewhere.
  pub fn label(&self) -> String {
    if cfg!(target_os = "macos") { self.symbols() } else { self.names() }
  }

  /// As GNOME, KDE and Windows write it: `Ctrl+Alt+Super+F12` (Windows: `Win`), `Alt+Space`.
  pub fn names(&self) -> String {
    let key = match self.key {
      Key::Char(c) => c.to_ascii_uppercase().to_string(),
      Key::F(n) => format!("F{n}"),
      Key::Escape => "Esc".into(),
      Key::Return => "Enter".into(),
      Key::Delete => "Backspace".into(),
      Key::ForwardDelete => "Delete".into(),
      Key::PageUp => "Page Up".into(),
      Key::PageDown => "Page Down".into(),
      key @ (Key::Space
      | Key::Tab
      | Key::Up
      | Key::Down
      | Key::Left
      | Key::Right
      | Key::Home
      | Key::End) => {
        let name = NAMES.iter().find(|(_, k)| *k == key).map_or("?", |(n, _)| n);
        name[..1].to_uppercase() + &name[1..]
      }
      // The punctuation keys: their second name is the character.
      key => NAMES.iter().filter(|(_, k)| *k == key).nth(1).map_or("?", |(n, _)| n).to_owned(),
    };

    self.modifiers.names() + &key
  }

  /// As macOS menus show it: `⌃⌥⌘F12`, `⌥Space`.
  pub fn symbols(&self) -> String {
    let m = self.modifiers;
    let mut s = String::new();
    for (on, symbol) in [(m.ctrl, '⌃'), (m.option, '⌥'), (m.shift, '⇧'), (m.cmd, '⌘')] {
      if on {
        s.push(symbol);
      }
    }

    let key = match self.key {
      Key::Char(c) => c.to_ascii_uppercase().to_string(),
      Key::F(n) => format!("F{n}"),
      Key::Space => "Space".into(),
      Key::Escape => "⎋".into(),
      Key::Tab => "⇥".into(),
      Key::Return => "↩".into(),
      Key::Delete => "⌫".into(),
      Key::ForwardDelete => "⌦".into(),
      Key::Up => "↑".into(),
      Key::Down => "↓".into(),
      Key::Left => "←".into(),
      Key::Right => "→".into(),
      Key::Home => "↖".into(),
      Key::End => "↘".into(),
      Key::PageUp => "⇞".into(),
      Key::PageDown => "⇟".into(),
      // The punctuation keys: their second name is the character.
      key => NAMES.iter().filter(|(_, k)| *k == key).nth(1).map_or("?", |(n, _)| n).to_owned(),
    };

    s + &key
  }
}

/// A hotkey: a chord, or a modifier key alone that works by itself (macOS: the event tap sees it go
/// down and up, and a key pressed while it is held makes it a chord of the user's instead).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hotkey {
  Chord(Chord),
  Modifier(ModKey),
}

/// The spellings of a modifier key alone; the first one of a key is how it is written back.
const MODIFIER_KEYS: &[(&str, ModKey)] = &[
  ("left_ctrl", ModKey::LeftCtrl),
  ("right_ctrl", ModKey::RightCtrl),
  ("left_option", ModKey::LeftAlt),
  ("right_option", ModKey::RightAlt),
  ("left_shift", ModKey::LeftShift),
  ("right_shift", ModKey::RightShift),
  ("left_cmd", ModKey::LeftMeta),
  ("right_cmd", ModKey::RightMeta),
  ("fn", ModKey::Fn),
  ("globe", ModKey::Fn),
];

/// The modifier key `text` names alone: `right_option`, `Right Option`, `right-alt`, `rcmd`, `fn`.
fn modifier_key(text: &str) -> Option<ModKey> {
  if text.contains('+') {
    return None;
  }

  let name: String = text.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
  if let Some((_, key)) = MODIFIER_KEYS.iter().find(|(n, _)| n.replace('_', "") == name) {
    return Some(*key);
  }

  let (right, rest) = match () {
    _ if name.starts_with("right") => (true, &name[5..]),
    _ if name.starts_with("left") => (false, &name[4..]),
    _ if name.starts_with('r') => (true, &name[1..]),
    _ if name.starts_with('l') => (false, &name[1..]),
    _ => return None,
  };

  let side = |left, right_key| Some(if right { right_key } else { left });
  match rest {
    "ctrl" | "control" | "ctl" => side(ModKey::LeftCtrl, ModKey::RightCtrl),
    "option" | "opt" | "alt" => side(ModKey::LeftAlt, ModKey::RightAlt),
    "shift" => side(ModKey::LeftShift, ModKey::RightShift),
    "cmd" | "command" | "super" | "meta" | "win" => side(ModKey::LeftMeta, ModKey::RightMeta),
    _ => None,
  }
}

impl Hotkey {
  pub fn parse(text: &str) -> Result<Self, ChordError> {
    match modifier_key(text.trim()) {
      Some(key) => Ok(Hotkey::Modifier(key)),
      None => Chord::parse(text).map(Hotkey::Chord),
    }
  }

  /// As the platform shows it: `⌃⌥Space`, `Right ⌥`, `fn`; `Ctrl+Alt+Space`, `Right Alt`.
  pub fn label(&self) -> String {
    match self {
      Hotkey::Chord(chord) => chord.label(),
      Hotkey::Modifier(key) => modifier_label(*key),
    }
  }

  /// As macOS menus show it: [Chord::symbols], or the key's name.
  pub fn symbols(&self) -> String {
    match self {
      Hotkey::Chord(chord) => chord.symbols(),
      Hotkey::Modifier(key) => modifier_label(*key),
    }
  }
}

/// A modifier key alone, by its side and the platform's mark or name: `Right ⌥`, `Left ⌘`
/// on macOS (as its menus write modifiers), `Right Alt`, `Left Win` (Windows) or `Left Super`
/// elsewhere; `fn` is `fn` (🌐 on newer Mac keyboards).
pub fn modifier_label(key: ModKey) -> String {
  let mac = cfg!(target_os = "macos");
  let meta = if cfg!(windows) { "Win" } else { "Super" };
  let (ctrl, alt, shift, meta) = match mac {
    true => ("⌃", "⌥", "⇧", "⌘"),
    false => ("Ctrl", "Alt", "Shift", meta),
  };

  let (side, name) = match key {
    ModKey::Fn => return "fn".into(),
    ModKey::LeftCtrl => ("Left", ctrl),
    ModKey::RightCtrl => ("Right", ctrl),
    ModKey::LeftAlt => ("Left", alt),
    ModKey::RightAlt => ("Right", alt),
    ModKey::LeftShift => ("Left", shift),
    ModKey::RightShift => ("Right", shift),
    ModKey::LeftMeta => ("Left", meta),
    ModKey::RightMeta => ("Right", meta),
  };

  format!("{side} {name}")
}

/// The canonical spelling: [Chord]'s, or the key's alone (`right_option`).
impl fmt::Display for Hotkey {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Hotkey::Chord(chord) => chord.fmt(f),
      Hotkey::Modifier(key) => {
        let name = MODIFIER_KEYS.iter().find(|(_, k)| k == key).map_or("?", |(n, _)| n);
        f.write_str(name)
      }
    }
  }
}

fn parse_key(name: &str) -> Option<Key> {
  let mut chars = name.chars();
  if let (Some(c), None) = (chars.next(), chars.clone().next())
    && (c.is_ascii_lowercase() || c.is_ascii_digit())
  {
    return Some(Key::Char(c));
  }
  if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u8>().ok())
    && (1..=20).contains(&n)
    && !name.starts_with("f0")
  {
    return Some(Key::F(n));
  }
  NAMES.iter().find(|(n, _)| *n == name).map(|&(_, key)| key)
}

fn key_name(key: Key) -> &'static str {
  NAMES.iter().find(|(_, k)| *k == key).map_or("?", |(n, _)| n)
}

/// The canonical spelling: `ctrl+option+shift+cmd+key`, lowercase.
impl fmt::Display for Chord {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let m = self.modifiers;
    for (on, name) in [(m.ctrl, "ctrl"), (m.option, "option"), (m.shift, "shift"), (m.cmd, "cmd")] {
      if on {
        write!(f, "{name}+")?;
      }
    }

    match self.key {
      Key::Char(c) => write!(f, "{c}"),
      Key::F(n) => write!(f, "f{n}"),
      key => f.write_str(key_name(key)),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn chord(ctrl: bool, option: bool, shift: bool, cmd: bool, key: Key) -> Chord {
    Chord { modifiers: Modifiers { ctrl, option, shift, cmd }, key }
  }

  #[test]
  fn parses_settings_and_other_spellings() {
    assert_eq!(Chord::parse("option+space"), Ok(chord(false, true, false, false, Key::Space)));
    assert_eq!(Chord::parse("ctrl+option+cmd+F12"), Ok(chord(true, true, false, true, Key::F(12))));
    assert_eq!(
      Chord::parse(" Control + Alt + Command + f12 "),
      Chord::parse("ctrl+option+cmd+f12")
    );
    assert_eq!(Chord::parse("f13"), Ok(chord(false, false, false, false, Key::F(13))));
    assert_eq!(Chord::parse("option+shift+space"), Ok(chord(false, true, true, false, Key::Space)));
    assert_eq!(Chord::parse("cmd+K"), Ok(chord(false, false, false, true, Key::Char('k'))));
    assert_eq!(Chord::parse("ctrl+/"), Ok(chord(true, false, false, false, Key::Slash)));
    assert_eq!(Chord::parse("escape"), Chord::parse("esc"));
  }

  #[test]
  fn writes_the_canonical_spelling_back() {
    for (text, canonical, symbols, names) in [
      ("ctrl+option+cmd+F12", "ctrl+option+cmd+f12", "⌃⌥⌘F12", "Ctrl+Alt+Super+F12"),
      ("Alt+Space", "option+space", "⌥Space", "Alt+Space"),
      ("cmd+shift+a", "shift+cmd+a", "⇧⌘A", "Shift+Super+A"),
      ("ctrl+/", "ctrl+slash", "⌃/", "Ctrl+/"),
      ("ctrl+esc", "ctrl+escape", "⌃⎋", "Ctrl+Esc"),
      ("shift+pagedown", "shift+pagedown", "⇧⇟", "Shift+Page Down"),
    ] {
      let c = Chord::parse(text).unwrap();
      assert_eq!(c.to_string(), canonical);
      assert_eq!(Chord::parse(canonical), Ok(c), "round trip of {text}");
      // The command key is Windows' Win key there.
      let names = if cfg!(windows) { names.replace("Super", "Win") } else { names.to_string() };
      assert_eq!((c.symbols(), c.names()), (symbols.to_string(), names));
    }
    let ctrl_alt = Modifiers { ctrl: true, option: true, ..Modifiers::default() };
    let held = if cfg!(target_os = "macos") { "⌃⌥" } else { "Ctrl+Alt+" };
    assert_eq!(ctrl_alt.label(), held);
  }

  #[test]
  fn every_key_once() {
    let keys: Vec<Key> = Key::all().collect();
    assert_eq!(keys.len(), 26 + 10 + 20 + 25);
    for (i, k) in keys.iter().enumerate() {
      assert!(!keys[..i].contains(k), "{k:?} twice");
      let chord = Chord { modifiers: Modifiers { ctrl: true, ..Modifiers::default() }, key: *k };
      assert_eq!(Chord::parse(&chord.to_string()), Ok(chord), "{chord}");
    }
  }

  #[test]
  fn a_modifier_alone_is_a_hotkey_of_its_own() {
    for (text, key, canonical, label) in [
      ("right_option", ModKey::RightAlt, "right_option", "Right ⌥"),
      ("Right Option", ModKey::RightAlt, "right_option", "Right ⌥"),
      ("right-alt", ModKey::RightAlt, "right_option", "Right ⌥"),
      ("ropt", ModKey::RightAlt, "right_option", "Right ⌥"),
      ("left_option", ModKey::LeftAlt, "left_option", "Left ⌥"),
      ("rcmd", ModKey::RightMeta, "right_cmd", "Right ⌘"),
      ("left_ctrl", ModKey::LeftCtrl, "left_ctrl", "Left ⌃"),
      ("RightShift", ModKey::RightShift, "right_shift", "Right ⇧"),
      ("fn", ModKey::Fn, "fn", "fn"),
      ("Globe", ModKey::Fn, "fn", "fn"),
    ] {
      let hotkey = Hotkey::parse(text).unwrap();
      assert_eq!(hotkey, Hotkey::Modifier(key), "{text}");
      assert_eq!(hotkey.to_string(), canonical);
      assert_eq!(Hotkey::parse(canonical), Ok(hotkey), "round trip of {text}");
      if cfg!(target_os = "macos") {
        assert_eq!(hotkey.label(), label);
      }

      assert_eq!(Chord::parse(text), Err(ChordError::ModifierAlone), "{text}");
    }
    for key in ModKey::ALL {
      let hotkey = Hotkey::Modifier(key);
      assert_eq!(Hotkey::parse(&hotkey.to_string()), Ok(hotkey));
    }
    // Chords stay chords; a modifier without a side is no key of its own.
    assert!(matches!(Hotkey::parse("ctrl+option+space"), Ok(Hotkey::Chord(_))));
    assert_eq!(Hotkey::parse("option"), Err(ChordError::NoKey));
    assert_eq!(Hotkey::parse("fn+f1"), Err(ChordError::Fn));
    assert!(Hotkey::parse("right").is_ok_and(|h| h != Hotkey::Modifier(ModKey::RightAlt)));
    assert!(Hotkey::parse("rx").is_err());
    assert!(matches!(Hotkey::parse("l+shift"), Ok(Hotkey::Chord(_))), "the L key with Shift");
  }

  #[test]
  fn rejects_what_is_not_a_chord() {
    assert_eq!(Chord::parse("  "), Err(ChordError::Empty));
    assert_eq!(Chord::parse("ctrl+option"), Err(ChordError::NoKey));
    assert_eq!(Chord::parse("fn+f1"), Err(ChordError::Fn));
    assert_eq!(Chord::parse("ctrl+hyper"), Err(ChordError::Unknown("hyper".into())));
    assert_eq!(Chord::parse("a+b"), Err(ChordError::TwoKeys("a".into(), "b".into())));
    for bad in ["f0", "f21", "f012", "ctrl++", "ctrl+"] {
      assert!(Chord::parse(bad).is_err(), "{bad}");
    }
  }
}
