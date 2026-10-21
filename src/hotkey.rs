// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! How hotkeys are written ([Hotkey]): a chord or a modifier key alone.
//!
//! Hotkeys as settings spell them: a chord of modifiers and one key joined by `+`, such as
//! `option+space` or `ctrl+option+cmd+F12`, or one modifier key alone, left or right, such as
//! `right_option` or `fn` ([Hotkey]; macOS only). Case does not matter; `control` / `alt` /
//! `opt` / `command` and the like are accepted for the modifiers. The grammar and the key names
//! are the same on every platform: keys are places ([Code], named after a US layout), modifiers
//! the keys as they are named on the keyboard ([Modifiers]: `cmd` is [Modifiers::META], ⌘ on
//! macOS and the Windows or Super key elsewhere).

use std::fmt;

use keyboard_types::{Code, Modifiers};

use crate::keys::{self, Key};

/// The modifiers held, as the platform shows them before a key: `⌃⌥` on macOS, `Ctrl+Alt+`
/// elsewhere.
pub fn modifiers_label(m: Modifiers) -> String {
  if cfg!(target_os = "macos") { mod_symbols(m) } else { mod_names(m) }
}

fn mod_symbols(m: Modifiers) -> String {
  let (ctrl, alt, shift, meta) =
    (Modifiers::CONTROL, Modifiers::ALT, Modifiers::SHIFT, Modifiers::META);
  let symbols = [(ctrl, "⌃"), (alt, "⌥"), (shift, "⇧"), (meta, "⌘")];
  symbols.into_iter().filter(|(k, _)| m.contains(*k)).map(|(_, s)| s).collect()
}

fn mod_names(m: Modifiers) -> String {
  let meta = if cfg!(windows) { "Win+" } else { "Super+" };
  let (ctrl, alt, shift) = (Modifiers::CONTROL, Modifiers::ALT, Modifiers::SHIFT);
  let names = [(ctrl, "Ctrl+"), (alt, "Alt+"), (shift, "Shift+"), (Modifiers::META, meta)];
  names.into_iter().filter(|(k, _)| m.contains(*k)).map(|(_, n)| n).collect()
}

/// Names of the keys besides letters, digits, punctuation and F-keys; the first one of a key is
/// how it is written back.
#[rustfmt::skip]
const NAMES: &[(&str, Code)] = &[
  ("space", Code::Space), ("escape", Code::Escape), ("esc", Code::Escape), ("tab", Code::Tab),
  ("return", Code::Enter), ("enter", Code::Enter), ("delete", Code::Backspace),
  ("backspace", Code::Backspace), ("forwarddelete", Code::Delete), ("up", Code::ArrowUp),
  ("down", Code::ArrowDown), ("left", Code::ArrowLeft), ("right", Code::ArrowRight),
  ("home", Code::Home), ("end", Code::End), ("pageup", Code::PageUp), ("pagedown", Code::PageDown),
  ("minus", Code::Minus), ("equal", Code::Equal), ("bracketleft", Code::BracketLeft),
  ("bracketright", Code::BracketRight), ("backslash", Code::Backslash),
  ("semicolon", Code::Semicolon), ("quote", Code::Quote), ("comma", Code::Comma),
  ("period", Code::Period), ("slash", Code::Slash), ("grave", Code::Backquote),
];

/// A key and the modifiers held with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
  pub modifiers: Modifiers,
  pub key: Code,
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

    let mut modifiers = Modifiers::empty();
    let mut key: Option<(Code, String)> = None;
    // `+` itself is no key name, so splitting on it is unambiguous.
    for part in text.split('+').map(str::trim) {
      let name = part.to_lowercase();
      modifiers |= match name.as_str() {
        "ctrl" | "control" | "ctl" => Modifiers::CONTROL,
        "option" | "opt" | "alt" => Modifiers::ALT,
        "shift" => Modifiers::SHIFT,
        "cmd" | "command" | "super" | "meta" => Modifiers::META,
        "fn" | "function" => return Err(ChordError::Fn),
        _ => {
          let parsed = parse_key(part).ok_or_else(|| ChordError::Unknown(part.to_owned()))?;
          if let Some((_, first)) = &key {
            return Err(ChordError::TwoKeys(first.clone(), part.to_owned()));
          }

          key = Some((parsed, part.to_owned()));
          continue;
        }
      };
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
      Code::Delete => "Delete".into(),
      Code::PageUp => "Page Up".into(),
      Code::PageDown => "Page Down".into(),
      code => key_label(code, true),
    };

    mod_names(self.modifiers) + &key
  }

  /// As macOS menus show it: `⌃⌥⌘F12`, `⌥Space`.
  pub fn symbols(&self) -> String {
    mod_symbols(self.modifiers) + &key_label(self.key, false)
  }
}

/// The key at `code` as menus show it ([Key::label]), W3C's name for a key we don't name.
fn key_label(code: Code, windows: bool) -> String {
  let key = Key::named(code).or_else(|| keys::us_char(code, false).map(Key::Char));
  key.map_or_else(|| code.to_string(), |k| k.label(windows))
}

/// A hotkey: a chord, or a modifier key alone that works by itself (macOS: the event tap sees it go
/// down and up, and a key pressed while it is held makes it a chord of the user's instead).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hotkey {
  Chord(Chord),
  /// A modifier key by its place: [Code::AltRight], [Code::Fn].
  Modifier(Code),
}

/// The spellings of a modifier key alone; the first one of a key is how it is written back.
const MODIFIER_KEYS: &[(&str, Code)] = &[
  ("left_ctrl", Code::ControlLeft),
  ("right_ctrl", Code::ControlRight),
  ("left_option", Code::AltLeft),
  ("right_option", Code::AltRight),
  ("left_shift", Code::ShiftLeft),
  ("right_shift", Code::ShiftRight),
  ("left_cmd", Code::MetaLeft),
  ("right_cmd", Code::MetaRight),
  ("fn", Code::Fn),
  ("globe", Code::Fn),
];

/// Whether the key at `code` is a modifier a hotkey can be alone: Ctrl, Alt, Shift or Meta by
/// its side, or Fn.
pub fn is_modifier(code: Code) -> bool {
  MODIFIER_KEYS.iter().any(|&(_, k)| k == code)
}

/// The modifier key `text` names alone: `right_option`, `Right Option`, `right-alt`, `rcmd`, `fn`.
fn modifier_key(text: &str) -> Option<Code> {
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
    "ctrl" | "control" | "ctl" => side(Code::ControlLeft, Code::ControlRight),
    "option" | "opt" | "alt" => side(Code::AltLeft, Code::AltRight),
    "shift" => side(Code::ShiftLeft, Code::ShiftRight),
    "cmd" | "command" | "super" | "meta" | "win" => side(Code::MetaLeft, Code::MetaRight),
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
pub fn modifier_label(key: Code) -> String {
  let mac = cfg!(target_os = "macos");
  let meta = if cfg!(windows) { "Win" } else { "Super" };
  let (ctrl, alt, shift, meta) = match mac {
    true => ("⌃", "⌥", "⇧", "⌘"),
    false => ("Ctrl", "Alt", "Shift", meta),
  };

  let (side, name) = match key {
    Code::Fn => return "fn".into(),
    Code::ControlLeft => ("Left", ctrl),
    Code::ControlRight => ("Right", ctrl),
    Code::AltLeft => ("Left", alt),
    Code::AltRight => ("Right", alt),
    Code::ShiftLeft => ("Left", shift),
    Code::ShiftRight => ("Right", shift),
    Code::MetaLeft => ("Left", meta),
    Code::MetaRight => ("Right", meta),
    _ => return key.to_string(),
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

/// The key `part` names: a letter, digit or punctuation character of a US layout, `f1` to `f20`,
/// a name in [NAMES], or W3C's name of another key that is no modifier (`Numpad1`).
fn parse_key(part: &str) -> Option<Code> {
  let name = part.to_lowercase();
  let mut chars = name.chars();
  if let (Some(c), None) = (chars.next(), chars.next()) {
    return keys::us_code(c).filter(|k| !k.1).map(|k| k.0);
  }

  if let Some(n) = name.strip_prefix('f').filter(|n| !n.starts_with('0'))
    && let Some((code, _)) = n.parse().ok().and_then(keys::f_key)
  {
    return Some(code);
  }

  match NAMES.iter().find(|(n, _)| *n == name) {
    Some(&(_, code)) => Some(code),
    None => part.parse().ok().filter(|&c| !is_modifier(c) && c != Code::Unidentified),
  }
}

/// The canonical spelling: `ctrl+option+shift+cmd+key`, lowercase but for W3C's names of keys
/// that have none of ours (`Numpad1`).
impl fmt::Display for Chord {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let m = self.modifiers;
    let (ctrl, alt, shift) = (Modifiers::CONTROL, Modifiers::ALT, Modifiers::SHIFT);
    let names = [(ctrl, "ctrl+"), (alt, "option+"), (shift, "shift+"), (Modifiers::META, "cmd+")];
    for (k, name) in names {
      if m.contains(k) {
        f.write_str(name)?;
      }
    }

    match NAMES.iter().find(|(_, c)| *c == self.key) {
      Some((name, _)) => f.write_str(name),
      None if let Some(c) = keys::us_char(self.key, false) => write!(f, "{c}"),
      None if let Some(n) = (1..=20).find(|&n| keys::f_key(n).is_some_and(|k| k.0 == self.key)) => {
        write!(f, "f{n}")
      }
      None => write!(f, "{}", self.key),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn chord(ctrl: bool, option: bool, shift: bool, cmd: bool, key: Code) -> Chord {
    let mut modifiers = Modifiers::empty();
    modifiers.set(Modifiers::CONTROL, ctrl);
    modifiers.set(Modifiers::ALT, option);
    modifiers.set(Modifiers::SHIFT, shift);
    modifiers.set(Modifiers::META, cmd);
    Chord { modifiers, key }
  }

  #[test]
  fn parses_settings_and_other_spellings() {
    assert_eq!(Chord::parse("option+space"), Ok(chord(false, true, false, false, Code::Space)));
    assert_eq!(Chord::parse("ctrl+option+cmd+F12"), Ok(chord(true, true, false, true, Code::F12)));
    assert_eq!(
      Chord::parse(" Control + Alt + Command + f12 "),
      Chord::parse("ctrl+option+cmd+f12")
    );
    assert_eq!(Chord::parse("f13"), Ok(chord(false, false, false, false, Code::F13)));
    assert_eq!(
      Chord::parse("option+shift+space"),
      Ok(chord(false, true, true, false, Code::Space))
    );
    assert_eq!(Chord::parse("cmd+K"), Ok(chord(false, false, false, true, Code::KeyK)));
    assert_eq!(Chord::parse("ctrl+/"), Ok(chord(true, false, false, false, Code::Slash)));
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
    let held = if cfg!(target_os = "macos") { "⌃⌥" } else { "Ctrl+Alt+" };
    assert_eq!(modifiers_label(Modifiers::CONTROL | Modifiers::ALT), held);
  }

  /// Every key the grammar had before `keyboard-types`, written and shown as it was then:
  /// canonical, macOS's symbols and the other platforms' names, with Ctrl-Shift.
  #[test]
  fn every_key_as_before() {
    let mut want: Vec<String> = ('a'..='z')
      .chain('0'..='9')
      .map(|c| {
        let up = c.to_ascii_uppercase();
        format!("ctrl+shift+{c} ⌃⇧{up} Ctrl+Shift+{up}")
      })
      .collect();
    want.extend((1..=20).map(|n| format!("ctrl+shift+f{n} ⌃⇧F{n} Ctrl+Shift+F{n}")));
    want.extend(
      "space Space Space|escape ⎋ Esc|tab ⇥ Tab|return ↩ Enter|delete ⌫ Backspace|\
       forwarddelete ⌦ Delete|up ↑ Up|down ↓ Down|left ← Left|right → Right|home ↖ Home|\
       end ↘ End|pageup ⇞ Page Up|pagedown ⇟ Page Down|minus - -|equal = =|bracketleft [ [|\
       bracketright ] ]|backslash \\ \\|semicolon ; ;|quote ' '|comma , ,|period . .|\
       slash / /|grave ` `"
        .split('|')
        .map(|row| {
          let (name, rest) = row.trim().split_once(' ').unwrap();
          let (symbol, names) = rest.split_once(' ').unwrap();
          format!("ctrl+shift+{name} ⌃⇧{symbol} Ctrl+Shift+{names}")
        }),
    );
    assert_eq!(want.len(), 26 + 10 + 20 + 25);
    for row in want {
      let name = row.split(' ').next().unwrap();
      let c = Chord::parse(name).unwrap();
      assert_eq!(Chord::parse(&c.to_string()), Ok(c), "{row}");
      assert_eq!(format!("{c} {} {}", c.symbols(), c.names()), row);
    }

    // The other spellings of keys.
    for (alias, name) in [
      ("esc", "escape"),
      ("enter", "return"),
      ("backspace", "delete"),
      ("-", "minus"),
      ("=", "equal"),
      ("[", "bracketleft"),
      ("]", "bracketright"),
      ("\\", "backslash"),
      (";", "semicolon"),
      ("'", "quote"),
      (",", "comma"),
      (".", "period"),
      ("/", "slash"),
      ("`", "grave"),
      ("F7", "f7"),
      ("Q", "q"),
    ] {
      assert_eq!(Chord::parse(alias), Chord::parse(name), "{alias}");
    }

    // Keys with none of our names go by W3C's, and none is a modifier.
    let c = Chord::parse("shift+Numpad1").unwrap();
    assert_eq!((c.key, c.to_string()), (Code::Numpad1, "shift+Numpad1".into()));
    assert_eq!(Chord::parse("numpad1"), Err(ChordError::Unknown("numpad1".into())));
    assert!(Chord::parse("ctrl+AltRight").is_err() && Chord::parse("!").is_err());
  }

  #[test]
  fn a_modifier_alone_is_a_hotkey_of_its_own() {
    for (text, key, canonical, label) in [
      ("right_option", Code::AltRight, "right_option", "Right ⌥"),
      ("Right Option", Code::AltRight, "right_option", "Right ⌥"),
      ("right-alt", Code::AltRight, "right_option", "Right ⌥"),
      ("ropt", Code::AltRight, "right_option", "Right ⌥"),
      ("left_option", Code::AltLeft, "left_option", "Left ⌥"),
      ("rcmd", Code::MetaRight, "right_cmd", "Right ⌘"),
      ("left_ctrl", Code::ControlLeft, "left_ctrl", "Left ⌃"),
      ("RightShift", Code::ShiftRight, "right_shift", "Right ⇧"),
      ("fn", Code::Fn, "fn", "fn"),
      ("Globe", Code::Fn, "fn", "fn"),
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
    for &(_, key) in MODIFIER_KEYS {
      let hotkey = Hotkey::Modifier(key);
      assert_eq!(Hotkey::parse(&hotkey.to_string()), Ok(hotkey));
      assert!(is_modifier(key));
    }

    let names = MODIFIER_KEYS[..9].iter().map(|&(_, k)| Hotkey::Modifier(k).to_string());
    let want = "left_ctrl right_ctrl left_option right_option left_shift right_shift left_cmd \
                right_cmd fn";
    assert_eq!(names.collect::<Vec<_>>().join(" "), want);
    assert!(!is_modifier(Code::CapsLock) && !is_modifier(Code::KeyA));
    // Chords stay chords; a modifier without a side is no key of its own.
    assert!(matches!(Hotkey::parse("ctrl+option+space"), Ok(Hotkey::Chord(_))));
    assert_eq!(Hotkey::parse("option"), Err(ChordError::NoKey));
    assert_eq!(Hotkey::parse("fn+f1"), Err(ChordError::Fn));
    assert!(Hotkey::parse("right").is_ok_and(|h| h != Hotkey::Modifier(Code::AltRight)));
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
