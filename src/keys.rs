// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Keys: what a key press reports ([KeyEvent]: our [Key], and the W3C UI Events [Code] and
//! [Modifiers] of the `keyboard-types` crate), shortcuts as keymaps and settings write them
//! ([Shortcut], global [Hotkey]s) and their labels, bindings ([Binding]), and the commands keys
//! become ([Command]), named after AppKit's key binding selectors (`moveWordLeft:`,
//! `deleteBackward:`, `insertNewline:`).
//!
//! A key press travels in this order (the runtime routes it, [crate::Runtime::event]):
//!
//! 0. A focused element that grabs the keys ([crate::Ui::grab_keys]: a shortcut recorder recording,
//!    a modal dialog) gets the key itself first.
//! 1. Bindings of the focused element, then of its ancestors ([crate::Ui::keymap]): a match becomes
//!    a [Command] for that element, which reads it with [crate::Ui::command].
//! 2. The app's bindings ([crate::App::keymap]): a match goes to `App::event` as
//!    [crate::Event::Command].
//! 3. The key itself: the focused element ([crate::Ui::key_down]), then `App::event` as
//!    [crate::Event::KeyDown].
//! 4. Nobody used it: AppKit's `interpretKeyEvents:` turns it into typed text (`insertText:`,
//!    [crate::Event::Text]) or a command (`doCommandBySelector:`, [crate::Event::Command]), with
//!    the system's bindings: arrows, Option and Cmd moves, Emacs-style Ctrl keys. Text and commands
//!    go to the focused element, then the app; an unused Tab or Shift-Tab moves focus. A key with
//!    Cmd that nobody used goes on to the menu (Cmd-Q), then to step 4.
//!
//! Windows and Linux have no `interpretKeyEvents:`: their windows get step 4's commands from
//! [Os::command], the keys their own text fields bind. The harness has no AppKit either: it
//! builds key events on a US layout ([us_char]) and interprets them with [Os::command] of the
//! platform it plays ([crate::Harness::set_os]), for macOS [standard], the part of AppKit's
//! standard bindings we model.

use keyboard_types::{Code, Modifiers, NamedKey};

/// The command key, `cmd` in a shortcut in the app: ⌘ ([Modifiers::META]) on macOS, Ctrl
/// ([Modifiers::CONTROL]) on Windows and Linux, as their shortcuts use them.
pub const CMD: Modifiers = Os::CURRENT.cmd();

/// `ctrl` in a shortcut in the app: Control on macOS; the Windows or Super key elsewhere, Ctrl
/// being [CMD].
pub const CTRL: Modifiers = Os::CURRENT.ctrl();

/// Whose keyboard conventions keys follow: what `cmd` and `ctrl` in a binding are, and the
/// command a key nobody used runs in a text field ([Os::command]). The runtime keeps the
/// platform's ([Os::CURRENT]); the harness may take another's
/// ([crate::Harness::set_os]), so tests check every platform's keys on any host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
  Mac,
  Windows,
  Linux,
}

impl Os {
  /// The platform built for.
  pub const CURRENT: Os = match () {
    _ if cfg!(target_os = "macos") => Os::Mac,
    _ if cfg!(windows) => Os::Windows,
    _ => Os::Linux,
  };

  /// `cmd` in a binding: ⌘ on macOS, Ctrl elsewhere.
  pub const fn cmd(self) -> Modifiers {
    if let Os::Mac = self { Modifiers::META } else { Modifiers::CONTROL }
  }

  /// `ctrl` in a binding: Control on macOS, the Windows or Super key elsewhere.
  pub const fn ctrl(self) -> Modifiers {
    if let Os::Mac = self { Modifiers::CONTROL } else { Modifiers::META }
  }

  /// The command of the first binding in `map` that matches `k`, `cmd` and `ctrl` as this
  /// platform's.
  pub fn lookup(self, map: &[Binding], k: &KeyEvent) -> Option<Command> {
    let hit = |b: &&Binding| parse(b.keys, Some(self)).is_ok_and(|s| s.matches(k));
    map.iter().find(hit).map(|b| b.command)
  }

  /// The command a key that types nothing runs in a text field when nobody used it, as the
  /// platform's own text fields bind it: AppKit's ([standard]), or Windows' and GTK's ([pc]).
  pub fn command(self, k: &KeyEvent) -> Option<Command> {
    if let Os::Mac = self { standard(k) } else { pc(k, self) }
  }
}

/// A key: the character it types, or its name. A [Copy] twin of `keyboard_types::Key`, whose
/// characters are a `String`: a key press types one character here, and keymaps match keys in
/// `const` patterns, without an allocation per key and binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
  /// A key that types a character: the character on the current layout with Shift applied
  /// but no other modifier (`charactersIgnoringModifiers`): `'a'`, `'A'` with Shift, `'!'` for
  /// Shift-1, `'a'` for Option-A or Cmd-A, `' '` for the space bar. With Cmd or Ctrl a
  /// non-ASCII character is replaced by the ASCII-capable layout's (Cmd-C on a Russian layout
  /// is `'c'`, not `'с'`), as AppKit matches menu shortcuts.
  Char(char),
  /// A key that types nothing: Enter, the arrows, F5; [NamedKey::Unidentified] when unknown.
  Named(NamedKey),
}

impl From<char> for Key {
  fn from(c: char) -> Self {
    Self::Char(c)
  }
}

impl From<NamedKey> for Key {
  fn from(k: NamedKey) -> Self {
    Self::Named(k)
  }
}

/// The named keys by place; a key's first place is its own ([Key::code]).
#[rustfmt::skip]
const NAMED: [(Code, NamedKey); 16] = [
  (Code::Enter, NamedKey::Enter), (Code::NumpadEnter, NamedKey::Enter),
  (Code::Tab, NamedKey::Tab), (Code::Backspace, NamedKey::Backspace),
  (Code::Delete, NamedKey::Delete), (Code::Escape, NamedKey::Escape),
  (Code::ArrowLeft, NamedKey::ArrowLeft), (Code::ArrowRight, NamedKey::ArrowRight),
  (Code::ArrowUp, NamedKey::ArrowUp), (Code::ArrowDown, NamedKey::ArrowDown),
  (Code::Home, NamedKey::Home), (Code::End, NamedKey::End),
  (Code::PageUp, NamedKey::PageUp), (Code::PageDown, NamedKey::PageDown),
  (Code::Insert, NamedKey::Insert), (Code::ContextMenu, NamedKey::ContextMenu),
];

/// F1 to F20, by place and name.
#[rustfmt::skip]
const F_KEYS: [(Code, NamedKey); 20] = [
  (Code::F1, NamedKey::F1), (Code::F2, NamedKey::F2), (Code::F3, NamedKey::F3),
  (Code::F4, NamedKey::F4), (Code::F5, NamedKey::F5), (Code::F6, NamedKey::F6),
  (Code::F7, NamedKey::F7), (Code::F8, NamedKey::F8), (Code::F9, NamedKey::F9),
  (Code::F10, NamedKey::F10), (Code::F11, NamedKey::F11), (Code::F12, NamedKey::F12),
  (Code::F13, NamedKey::F13), (Code::F14, NamedKey::F14), (Code::F15, NamedKey::F15),
  (Code::F16, NamedKey::F16), (Code::F17, NamedKey::F17), (Code::F18, NamedKey::F18),
  (Code::F19, NamedKey::F19), (Code::F20, NamedKey::F20),
];

/// The function key F`n`, F1 to F20, by place and name.
pub fn f_key(n: u8) -> Option<(Code, NamedKey)> {
  F_KEYS.get((n as usize).wrapping_sub(1)).copied()
}

impl Key {
  /// The named key at `code`, wherever the layout puts it: the keypad's Enter is Enter.
  pub fn named(code: Code) -> Option<Self> {
    let found = NAMED.iter().chain(&F_KEYS).find(|(c, _)| *c == code);
    found.map(|&(_, k)| Self::Named(k))
  }

  /// The place of a named key (the main Enter); `None` for characters.
  pub fn code(self) -> Option<Code> {
    let Self::Named(k) = self else { return None };
    NAMED.iter().chain(&F_KEYS).find(|(_, n)| *n == k).map(|&(c, _)| c)
  }
}

/// The character keys of a US keyboard by place, and what they type without and with Shift.
#[rustfmt::skip]
const US: [Code; 48] = [
  Code::KeyA, Code::KeyB, Code::KeyC, Code::KeyD, Code::KeyE, Code::KeyF, Code::KeyG, Code::KeyH,
  Code::KeyI, Code::KeyJ, Code::KeyK, Code::KeyL, Code::KeyM, Code::KeyN, Code::KeyO, Code::KeyP,
  Code::KeyQ, Code::KeyR, Code::KeyS, Code::KeyT, Code::KeyU, Code::KeyV, Code::KeyW, Code::KeyX,
  Code::KeyY, Code::KeyZ, Code::Digit0, Code::Digit1, Code::Digit2, Code::Digit3, Code::Digit4,
  Code::Digit5, Code::Digit6, Code::Digit7, Code::Digit8, Code::Digit9, Code::Minus, Code::Equal,
  Code::BracketLeft, Code::BracketRight, Code::Backslash, Code::Semicolon, Code::Quote,
  Code::Comma, Code::Period, Code::Slash, Code::Backquote, Code::Space,
];
const US_PLAIN: &str = "abcdefghijklmnopqrstuvwxyz0123456789-=[]\\;',./` ";
const US_SHIFT: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ)!@#$%^&*(_+{}|:\"<>?~ ";

/// What the key at `code` types on a US keyboard, with Shift or without: a shortcut's Latin
/// character where the layout has none.
pub fn us_char(code: Code, shift: bool) -> Option<char> {
  let i = US.iter().position(|&c| c == code)?;
  (if shift { US_SHIFT } else { US_PLAIN }).chars().nth(i)
}

/// The key typing `c` on a US keyboard, and whether it takes Shift.
pub fn us_code(c: char) -> Option<(Code, bool)> {
  let at = |s: &str| s.chars().position(|k| k == c).map(|i| US[i]);
  at(US_PLAIN).map(|k| (k, false)).or_else(|| at(US_SHIFT).map(|k| (k, true)))
}

/// A key press or release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
  pub key: Key,
  /// What the key types with every modifier applied (`characters`): `'å'` for Option-A on a US
  /// layout. `None` for named keys, with Cmd or Ctrl held, and for dead keys. Typed text
  /// reaches a text field as [crate::Event::Text], not from here.
  pub text: Option<char>,
  /// The physical key, by its place on a US keyboard ([Code::KeyA] is where A is there), the
  /// same on every layout.
  pub code: Code,
  /// The modifier keys held, as they are named on the keyboard: Shift, Ctrl
  /// ([Modifiers::CONTROL]), Alt or Option, and [Modifiers::META] (⌘, or the Windows or Super
  /// key); [CMD] is the command key. No others: locks and AltGr are not set.
  pub mods: Modifiers,
  /// An auto-repeat of a held key.
  pub repeat: bool,
}

impl KeyEvent {
  /// Whether this is the shortcut `s` in the app (`cmd+shift+z`, `escape`); see [Shortcut].
  pub fn is(&self, s: &str) -> bool {
    Shortcut::parse(s).is_ok_and(|k| k.matches(self))
  }
}

// MARK: Shortcuts

/// A key with modifiers, as keymaps and settings write it: modifiers and one key joined by `+`
/// (or `-`), in any order and case, spaces allowed around the parts: `cmd+shift+z`,
/// `option+space`, `ctrl+option+cmd+F12`, `cmd++`, `escape`. Modifiers: `ctrl` (`control`),
/// `option` (`opt`, `alt`), `shift`, `cmd` (`command`, `super`, `meta`). Keys: a character,
/// `f1` to `f20`, or a name: `return` (`enter`), `tab`, `delete` (`backspace`, ⌫) and
/// `forwarddelete` (⌦) as Mac keyboards name them, `escape` (`esc`), `left`, `right`, `up`, `down`,
/// `home`, `end`, `pageup`, `pagedown`, and `space`, `minus`, `equal`, `bracketleft`,
/// `bracketright`, `backslash`, `semicolon`, `quote`, `comma`, `period`, `slash`, `grave` for those
/// characters.
///
/// `mods` are the modifier keys held, as a [KeyEvent] has them; where it works decides what `cmd`
/// and `ctrl` name and how the key matches:
/// - in the app ([Shortcut::parse], [Binding]s): `cmd` is the command key [CMD] (Ctrl on Windows
///   and Linux), `ctrl` is [CTRL]; the key matches the character a key types ([Shortcut::matches]),
///   so `cmd+?` works on any layout;
/// - system-wide ([Hotkey::parse], global hotkeys): `cmd` is [Modifiers::META] (⌘, the Windows or
///   Super key), `ctrl` is Control; the key is a place on the keyboard ([Shortcut::code]).
///
/// Written back ([std::fmt::Display]) as a global hotkey spells its keys:
/// `ctrl+option+shift+cmd+` then the key's name, lowercase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
  pub key: Key,
  pub mods: Modifiers,
}

/// Named keys: how a shortcut writes them, and how macOS menus (glyphs) and the PC show them.
#[rustfmt::skip]
const NAMES: [(NamedKey, &str, &str, &str); 13] = [
  (NamedKey::Enter, "return", "↩", "Enter"),
  (NamedKey::Tab, "tab", "⇥", "Tab"),
  (NamedKey::Backspace, "delete", "⌫", "Backspace"),
  (NamedKey::Delete, "forwarddelete", "⌦", "Delete"),
  (NamedKey::Escape, "escape", "⎋", "Esc"),
  (NamedKey::ArrowUp, "up", "↑", "Up"),
  (NamedKey::ArrowDown, "down", "↓", "Down"),
  (NamedKey::ArrowLeft, "left", "←", "Left"),
  (NamedKey::ArrowRight, "right", "→", "Right"),
  (NamedKey::Home, "home", "↖", "Home"),
  (NamedKey::End, "end", "↘", "End"),
  (NamedKey::PageUp, "pageup", "⇞", "Page Up"),
  (NamedKey::PageDown, "pagedown", "⇟", "Page Down"),
];

/// Characters a shortcut writes by name.
#[rustfmt::skip]
const CHAR_NAMES: [(char, &str); 12] = [
  (' ', "space"), ('-', "minus"), ('=', "equal"), ('[', "bracketleft"), (']', "bracketright"),
  ('\\', "backslash"), (';', "semicolon"), ('\'', "quote"), (',', "comma"), ('.', "period"),
  ('/', "slash"), ('`', "grave"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutError {
  Empty,
  Unknown(String),
  TwoKeys(String, String),
  NoKey,
  Fn,
  ModifierAlone,
}

impl std::fmt::Display for ShortcutError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

impl std::error::Error for ShortcutError {}

impl Shortcut {
  /// A shortcut in the app: `cmd` is the command key [CMD]; see [Shortcut].
  pub fn parse(text: &str) -> Result<Self, ShortcutError> {
    parse(text, Some(Os::CURRENT))
  }

  /// Letters match in either case, Shift given by the modifiers: `cmd+shift+z` is Cmd-Shift-Z.
  /// Any other character matches itself, Shift or not, since the layout may need Shift to type
  /// it: `cmd++` is Cmd-Shift-= on a US layout. Named keys need the exact modifiers.
  pub fn matches(&self, k: &KeyEvent) -> bool {
    let shift = Modifiers::SHIFT;
    match (self.key, k.key) {
      (Key::Char(a), Key::Char(b)) if !a.is_alphabetic() => {
        a == b && self.mods - shift == k.mods - shift
      }
      (Key::Char(a), Key::Char(b)) => lower(a) == lower(b) && self.mods == k.mods,
      (a, b) => a == b && self.mods == k.mods,
    }
  }

  /// The key's place, as a global hotkey takes it: a character's on a US keyboard without Shift;
  /// `None` for a character that needs Shift there (`!`) or none.
  pub fn code(&self) -> Option<Code> {
    place(self.key)
  }

  /// As the platform shows it: [Shortcut::symbols] on macOS, [Shortcut::names] elsewhere.
  pub fn label(&self) -> String {
    if cfg!(target_os = "macos") { self.symbols() } else { self.names() }
  }

  /// As macOS menus show it: `⌃⌥⇧⌘` in that order, then the key: `⇧⌘Z`, `⌃⌥⌘F12`, `⌥Space`, `⎋`.
  pub fn symbols(&self) -> String {
    mods_label(self.mods, true) + &key_label(self.key, true)
  }

  /// As Windows, GNOME and KDE show it: `Ctrl+Shift+Z`, `Ctrl+Alt+Super+F12` (Windows: `Win`),
  /// `Alt+Space`, `Esc`.
  pub fn names(&self) -> String {
    mods_label(self.mods, false) + &key_label(self.key, false)
  }
}

/// The shortcut `text` writes, in the app of platform `app` or, with `None`, global (where its
/// key needs a place). Its parts are split at `+` and `-`; a separator that would leave a part
/// empty is the key itself: `cmd++`, `cmd+-`, `-`. No allocation unless it fails: bindings are
/// parsed on every key press.
fn parse(text: &str, app: Option<Os>) -> Result<Shortcut, ShortcutError> {
  let global = app.is_none();
  use ShortcutError::*;
  let text = text.trim();
  if text.is_empty() {
    return Err(Empty);
  }

  if modifier_key(text).is_some() {
    return Err(ModifierAlone);
  }

  let mut mods = Modifiers::empty();
  let mut key: Option<(Key, &str)> = None;
  let mut take = |from: usize, to: usize| {
    let part = text[from..to].trim();
    let mut buf = [0; 16];
    let name = ascii_lower(part, &mut buf);
    if name == "fn" || name == "function" {
      return Err(Fn);
    }

    match (modifier(name), app) {
      // In the app, the platform's command key and its other one.
      (Some(m), Some(os)) if m == Modifiers::META => mods |= os.cmd(),
      (Some(m), Some(os)) if m == Modifiers::CONTROL => mods |= os.ctrl(),
      (Some(m), _) => mods |= m,
      (None, _) => {
        let k = parse_key(part, name).filter(|&k| !global || place(k).is_some());
        let k = k.ok_or_else(|| Unknown(part.to_owned()))?;
        if let Some((_, first)) = key {
          return Err(TwoKeys(first.to_owned(), part.to_owned()));
        }

        key = Some((k, part));
      }
    }

    Ok(())
  };

  let mut start = 0;
  for (i, c) in text.char_indices() {
    if matches!(c, '+' | '-') && !text[start..i].trim().is_empty() {
      take(start, i)?;
      start = i + 1;
    }
  }

  take(start, text.len())?;
  let (key, _) = key.ok_or(NoKey)?;
  Ok(Shortcut { key, mods })
}

/// [Shortcut::code] of `key`.
fn place(key: Key) -> Option<Code> {
  match key {
    Key::Char(c) => us_code(lower(c)).filter(|k| !k.1).map(|k| k.0),
    k => k.code(),
  }
}

/// `s` in ASCII lowercase, in `buf`; empty when it is longer or not ASCII, as no name is.
fn ascii_lower<'a>(s: &str, buf: &'a mut [u8; 16]) -> &'a str {
  let Some(out) = buf.get_mut(..s.len()).filter(|_| s.is_ascii()) else { return "" };
  out.copy_from_slice(s.as_bytes());
  out.make_ascii_lowercase();
  std::str::from_utf8(out).unwrap_or("")
}

/// A modifier: as a shortcut writes it, as macOS and the PC show it, and its left and right keys.
type Mod = (Modifiers, &'static str, &'static str, &'static str, Code, Code);

/// The modifiers, in the order shortcuts write and show them.
#[rustfmt::skip]
const MODS: [Mod; 4] = [
  (Modifiers::CONTROL, "ctrl", "⌃", "Ctrl", Code::ControlLeft, Code::ControlRight),
  (Modifiers::ALT, "option", "⌥", "Alt", Code::AltLeft, Code::AltRight),
  (Modifiers::SHIFT, "shift", "⇧", "Shift", Code::ShiftLeft, Code::ShiftRight),
  (Modifiers::META, "cmd", "⌘", if cfg!(windows) { "Win" } else { "Super" }, Code::MetaLeft,
    Code::MetaRight),
];

/// The modifier a lowercase name is, as macOS names its keys: `cmd` is [Modifiers::META].
fn modifier(name: &str) -> Option<Modifiers> {
  Some(match name {
    "ctrl" | "control" | "ctl" => Modifiers::CONTROL,
    "option" | "opt" | "alt" => Modifiers::ALT,
    "shift" => Modifiers::SHIFT,
    "cmd" | "command" | "super" | "meta" => Modifiers::META,
    _ => return None,
  })
}

/// The key `part` names (`name` in lowercase): a character, `f1` to `f20`, or a name.
fn parse_key(part: &str, name: &str) -> Option<Key> {
  let mut cs = part.chars();
  if let (Some(c), None) = (cs.next(), cs.next()) {
    return Some(Key::Char(c.to_ascii_lowercase()));
  }

  let f = name.strip_prefix('f').filter(|n| !n.starts_with('0'));
  if let Some((_, k)) = f.and_then(|n| n.parse().ok()).and_then(f_key) {
    return Some(Key::Named(k));
  }

  let name = match name {
    "enter" => "return",
    "backspace" => "delete",
    "esc" => "escape",
    name => name,
  };

  let named = NAMES.iter().find(|n| n.1 == name).map(|n| Key::Named(n.0));
  named.or_else(|| CHAR_NAMES.iter().find(|n| n.1 == name).map(|n| Key::Char(n.0)))
}

/// The canonical spelling of a global hotkey: `ctrl+option+shift+cmd+key`, lowercase.
impl std::fmt::Display for Shortcut {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    for m in MODS.iter().filter(|m| self.mods.contains(m.0)) {
      write!(f, "{}+", m.1)?;
    }

    match self.key {
      Key::Char(c) => match CHAR_NAMES.iter().find(|n| n.0 == c) {
        Some(n) => f.write_str(n.1),
        None => write!(f, "{c}"),
      },
      Key::Named(k) => match NAMES.iter().find(|n| n.0 == k) {
        Some(n) => f.write_str(n.1),
        None => f.write_str(&k.to_string().to_lowercase()),
      },
    }
  }
}

/// As menus show a key: macOS's glyphs (`↩`, `⎋`, `←`) or the PC's names (`Enter`, `Esc`,
/// `Left`); a character in capitals, `Space`, `F5`, W3C's name for the rest.
fn key_label(key: Key, mac: bool) -> String {
  match key {
    Key::Char(' ') => "Space".into(),
    Key::Char(c) => c.to_uppercase().collect(),
    Key::Named(k) => match NAMES.iter().find(|n| n.0 == k) {
      Some(&(_, _, glyph, name)) => (if mac { glyph } else { name }).into(),
      None => k.to_string(),
    },
  }
}

/// The modifiers held, as the platform shows them before a key: `⌃⌥` on macOS, `Ctrl+Alt+`
/// elsewhere.
pub fn modifiers_label(m: Modifiers) -> String {
  mods_label(m, cfg!(target_os = "macos"))
}

fn mods_label(m: Modifiers, mac: bool) -> String {
  let held = MODS.iter().filter(|r| m.contains(r.0));
  held.map(|r| if mac { r.2.to_string() } else { format!("{}+", r.3) }).collect()
}

fn lower(c: char) -> char {
  c.to_lowercase().next().unwrap_or(c)
}

// MARK: Global hotkeys

/// A global hotkey as settings spell it: a [Shortcut] system-wide (`option+space`,
/// `ctrl+option+cmd+F12`), or one modifier key alone, left or right, such as `right_option` or
/// `fn` (macOS only: the event tap sees it go down and up, and a key pressed while it is held
/// makes it a chord of the user's instead).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hotkey {
  Shortcut(Shortcut),
  /// A modifier key by its place: [Code::AltRight], [Code::Fn].
  Modifier(Code),
}

/// The modifier key `key` is, and whether it is the right one; none for Fn and other keys.
fn side(key: Code) -> Option<(&'static Mod, bool)> {
  MODS.iter().find(|r| r.4 == key || r.5 == key).map(|r| (r, r.5 == key))
}

/// Whether the key at `code` is a modifier a hotkey can be alone: Ctrl, Alt, Shift or Meta by
/// its side, or Fn.
pub fn is_modifier(code: Code) -> bool {
  code == Code::Fn || side(code).is_some()
}

/// The modifier key `text` names alone: `right_option`, `Right Option`, `right-alt`, `rcmd`, `fn`,
/// `globe`.
fn modifier_key(text: &str) -> Option<Code> {
  if text.contains('+') {
    return None;
  }

  // Its letters and digits, lowercase: no allocation, as bindings are parsed on each key press.
  let (mut buf, mut n) = ([0u8; 16], 0);
  for c in text.bytes().filter(u8::is_ascii_alphanumeric) {
    *buf.get_mut(n)? = c.to_ascii_lowercase();
    n += 1;
  }

  let name = std::str::from_utf8(&buf[..n]).ok()?;
  if name == "fn" || name == "globe" {
    return Some(Code::Fn);
  }

  let (right, rest) = match () {
    _ if name.starts_with("right") => (true, &name[5..]),
    _ if name.starts_with("left") => (false, &name[4..]),
    _ if name.starts_with('r') => (true, &name[1..]),
    _ if name.starts_with('l') => (false, &name[1..]),
    _ => return None,
  };

  let m = modifier(if rest == "win" { "cmd" } else { rest })?;
  let row = MODS.iter().find(|r| r.0 == m)?;
  Some(if right { row.5 } else { row.4 })
}

impl Hotkey {
  /// A global hotkey: a modifier key alone, or a [Shortcut] whose `cmd` is ⌘, the Windows or the
  /// Super key and whose key has a place ([Shortcut::code]).
  pub fn parse(text: &str) -> Result<Self, ShortcutError> {
    if let Some(key) = modifier_key(text.trim()) {
      return Ok(Hotkey::Modifier(key));
    }

    parse(text, None).map(Hotkey::Shortcut)
  }

  /// As the platform shows it: `⌃⌥Space`, `Right ⌥`, `fn`; `Ctrl+Alt+Space`, `Right Alt`.
  pub fn label(&self) -> String {
    match self {
      Hotkey::Shortcut(s) => s.label(),
      Hotkey::Modifier(key) => modifier_label(*key),
    }
  }
}

/// A modifier key alone, by its side and the platform's mark or name: `Right ⌥`, `Left ⌘`
/// on macOS (as its menus write modifiers), `Right Alt`, `Left Win` (Windows) or `Left Super`
/// elsewhere; `fn` is `fn` (🌐 on newer Mac keyboards).
pub fn modifier_label(key: Code) -> String {
  let mac = cfg!(target_os = "macos");
  match side(key) {
    Some((r, right)) => {
      format!("{} {}", if right { "Right" } else { "Left" }, if mac { r.2 } else { r.3 })
    }
    None if key == Code::Fn => "fn".into(),
    None => key.to_string(),
  }
}

/// The canonical spelling: the [Shortcut]'s, or the key's alone (`right_option`).
impl std::fmt::Display for Hotkey {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Hotkey::Shortcut(s) => s.fmt(f),
      Hotkey::Modifier(Code::Fn) => f.write_str("fn"),
      Hotkey::Modifier(key) => match side(*key) {
        Some((r, right)) => write!(f, "{}_{}", if right { "right" } else { "left" }, r.1),
        None => f.write_str("?"),
      },
    }
  }
}

// MARK: Bindings

/// A shortcut in the app ([Shortcut] syntax) and the command it runs. `const`, so a keymap is a
/// constant: `const KEYS: &[Binding] = &[bind("cmd+z", Command::Undo)]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
  pub keys: &'static str,
  pub command: Command,
}

pub const fn bind(keys: &'static str, command: Command) -> Binding {
  Binding { keys, command }
}

/// The shortcut of the first binding in `map` that runs `cmd`: to show it beside the command,
/// in a menu item or a tooltip ([Shortcut::label]).
pub fn shortcut(map: &[Binding], cmd: Command) -> Option<Shortcut> {
  map.iter().find(|b| b.command == cmd).and_then(|b| Shortcut::parse(b.keys).ok())
}

/// The command of the first binding in `map` that matches `k`, on this platform ([Os::lookup]).
pub fn lookup(map: &[Binding], k: &KeyEvent) -> Option<Command> {
  Os::CURRENT.lookup(map, k)
}

/// The edit menu's keys, which a text field binds on itself (AppKit gets them from the
/// menu's key equivalents, which we don't have).
pub const EDIT_KEYS: &[Binding] = &[
  bind("cmd+a", Command::SelectAll),
  bind("cmd+c", Command::Copy),
  bind("cmd+x", Command::Cut),
  bind("cmd+v", Command::Paste),
  bind("cmd+z", Command::Undo),
  bind("cmd+shift+z", Command::Redo),
];

/// The keys of selectable text: select all and copy.
pub const SELECT_KEYS: &[Binding] =
  &[bind("cmd+a", Command::SelectAll), bind("cmd+c", Command::Copy)];

// MARK: Commands

/// Where a caret move goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
  Left,
  Right,
  WordLeft,
  WordRight,
  /// To the start of the visual line (Cmd-Left).
  LineStart,
  LineEnd,
  /// To the start of the line between line breaks (Ctrl-A).
  ParagraphStart,
  ParagraphEnd,
  Up,
  Down,
  PageUp,
  PageDown,
  DocStart,
  DocEnd,
}

/// What a delete removes when nothing is selected (a selection is removed as a whole).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delete {
  Backward,
  Forward,
  WordBackward,
  WordForward,
  /// To the start of the visual line (Cmd-Backspace).
  ToLineStart,
  ToLineEnd,
  ToParagraphStart,
  /// To the end of the line between line breaks (Ctrl-K).
  ToParagraphEnd,
}

/// A command from a key: AppKit's text commands (`doCommandBySelector:`), the edit menu's
/// (from bindings) and the app's own ([Command::Action]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
  /// Moves the caret; `true` extends the selection (the `…AndModifySelection:` selectors).
  Move(Move, bool),
  Delete(Delete),
  InsertNewline,
  InsertTab,
  InsertBacktab,
  /// Escape (`cancelOperation:`).
  Cancel,
  SelectAll,
  Copy,
  Cut,
  Paste,
  Undo,
  Redo,
  /// Scrolls without moving the caret (Page Up / Down, Home / End on a Mac).
  ScrollPageUp,
  ScrollPageDown,
  ScrollToTop,
  ScrollToBottom,
  /// The app's own command, by name: `bind("cmd+n", Command::Action("new"))`.
  Action(&'static str),
}

impl Command {
  /// The command for an AppKit selector name as `doCommandBySelector:` gets it
  /// (`"moveWordLeftAndModifySelection:"`); `None` for those we don't model (`noop:`,
  /// `transpose:`, `complete:`).
  pub fn from_selector(name: &str) -> Option<Self> {
    use Move::*;
    let (base, select) = match name.strip_suffix("AndModifySelection:") {
      Some(base) => (base, true),
      None => (name.strip_suffix(':')?, false),
    };

    let to = |m| Some(Self::Move(m, select));
    match base {
      "moveLeft" | "moveBackward" => return to(Left),
      "moveRight" | "moveForward" => return to(Right),
      "moveUp" => return to(Up),
      "moveDown" => return to(Down),
      "moveWordLeft" | "moveWordBackward" => return to(WordLeft),
      "moveWordRight" | "moveWordForward" => return to(WordRight),
      "moveToLeftEndOfLine" | "moveToBeginningOfLine" => return to(LineStart),
      "moveToRightEndOfLine" | "moveToEndOfLine" => return to(LineEnd),
      "moveToBeginningOfParagraph" | "moveParagraphBackward" => return to(ParagraphStart),
      "moveToEndOfParagraph" | "moveParagraphForward" => return to(ParagraphEnd),
      "moveToBeginningOfDocument" => return to(DocStart),
      "moveToEndOfDocument" => return to(DocEnd),
      "pageUp" => return to(PageUp),
      "pageDown" => return to(PageDown),
      _ if select => return None,
      _ => {}
    }

    let del = |d| Some(Self::Delete(d));
    match base {
      "deleteBackward" | "deleteBackwardByDecomposingPreviousCharacter" => del(Delete::Backward),
      "deleteForward" => del(Delete::Forward),
      "deleteWordBackward" => del(Delete::WordBackward),
      "deleteWordForward" => del(Delete::WordForward),
      "deleteToBeginningOfLine" => del(Delete::ToLineStart),
      "deleteToEndOfLine" => del(Delete::ToLineEnd),
      "deleteToBeginningOfParagraph" => del(Delete::ToParagraphStart),
      "deleteToEndOfParagraph" => del(Delete::ToParagraphEnd),
      "insertNewline"
      | "insertLineBreak"
      | "insertParagraphSeparator"
      | "insertNewlineIgnoringFieldEditor" => Some(Self::InsertNewline),
      "insertTab" | "insertTabIgnoringFieldEditor" => Some(Self::InsertTab),
      "insertBacktab" => Some(Self::InsertBacktab),
      "cancelOperation" | "cancel" => Some(Self::Cancel),
      "selectAll" => Some(Self::SelectAll),
      "copy" => Some(Self::Copy),
      "cut" => Some(Self::Cut),
      "paste" => Some(Self::Paste),
      "undo" => Some(Self::Undo),
      "redo" => Some(Self::Redo),
      "scrollPageUp" => Some(Self::ScrollPageUp),
      "scrollPageDown" => Some(Self::ScrollPageDown),
      "scrollToBeginningOfDocument" => Some(Self::ScrollToTop),
      "scrollToEndOfDocument" => Some(Self::ScrollToBottom),
      _ => None,
    }
  }
}

/// The command AppKit's standard key bindings (`StandardKeyBinding.dict`) give a key that types
/// nothing, for the keys we model: what `interpretKeyEvents:` sends a view in place of text.
/// The window gets them from AppKit; the harness from here.
pub fn standard(k: &KeyEvent) -> Option<Command> {
  use Move::*;
  use NamedKey as N;
  let (shift, ctrl, alt, cmd) = flags(k.mods, Os::Mac);
  let to = |m| Some(Command::Move(m, shift));
  let del = |d| Some(Command::Delete(d));
  let Key::Named(key) = k.key else {
    // Emacs keys.
    return match (k.key, ctrl, alt, cmd) {
      (Key::Char(c), true, false, false) => match lower(c) {
        'a' => to(ParagraphStart),
        'e' => to(ParagraphEnd),
        'b' => to(Left),
        'f' => to(Right),
        'p' => to(Up),
        'n' => to(Down),
        'v' => to(PageDown),
        'd' => del(Delete::Forward),
        'h' => del(Delete::Backward),
        'k' => del(Delete::ToParagraphEnd),
        _ => None,
      },
      _ => None,
    };
  };

  // Ctrl with an arrow is Mission Control's; with Backspace or Delete, the key alone.
  if let Some([alone, word, line]) = by_unit(key, shift) {
    let deletes = matches!(key, N::Backspace | N::Delete);
    return match (ctrl, alt, cmd) {
      (false, false, false) => alone,
      (true, false, false) if deletes => alone,
      (false, true, false) => word,
      (false, false, true) => line,
      _ => None,
    };
  }

  match (key, ctrl, alt, cmd) {
    (N::Home, false, false, false) if shift => to(DocStart),
    (N::Home, false, false, false) => Some(Command::ScrollToTop),
    (N::End, false, false, false) if shift => to(DocEnd),
    (N::End, false, false, false) => Some(Command::ScrollToBottom),
    (N::PageUp, false, false, false) if shift => to(PageUp),
    (N::PageUp, false, false, false) => Some(Command::ScrollPageUp),
    (N::PageDown, false, false, false) if shift => to(PageDown),
    (N::PageDown, false, false, false) => Some(Command::ScrollPageDown),
    (N::Enter, _, _, false) => Some(Command::InsertNewline),
    (N::Tab, false, false, false) if shift => Some(Command::InsertBacktab),
    (N::Tab, false, false, false) => Some(Command::InsertTab),
    (N::Escape, false, false, false) => Some(Command::Cancel),
    _ => None,
  }
}

/// What an arrow, Backspace or Delete does alone, with the word modifier (macOS Option, the
/// PC's Ctrl) and with macOS's Cmd: both move and delete by the same units.
fn by_unit(key: NamedKey, shift: bool) -> Option<[Option<Command>; 3]> {
  use Move::*;
  use NamedKey as N;
  let to = |m| Some(Command::Move(m, shift));
  let del = |d| Some(Command::Delete(d));
  Some(match key {
    N::ArrowLeft => [to(Left), to(WordLeft), to(LineStart)],
    N::ArrowRight => [to(Right), to(WordRight), to(LineEnd)],
    N::ArrowUp => [to(Up), to(ParagraphStart), to(DocStart)],
    N::ArrowDown => [to(Down), to(ParagraphEnd), to(DocEnd)],
    N::Backspace => [del(Delete::Backward), del(Delete::WordBackward), del(Delete::ToLineStart)],
    N::Delete => [del(Delete::Forward), del(Delete::WordForward), None],
    _ => return None,
  })
}

/// The modifiers as shortcuts in the app of `os` name them: Shift, `ctrl`, Alt, `cmd`.
fn flags(m: Modifiers, os: Os) -> (bool, bool, bool, bool) {
  (m.shift(), m.contains(os.ctrl()), m.alt(), m.contains(os.cmd()))
}

/// The command a key nobody used runs on Windows and Linux (`os`), as their text fields do
/// (Win32 edit controls, GTK): the window gets it from here where AppKit's bindings would be.
/// Ctrl is the command modifier there and arrives as `cmd`: Ctrl-Left a word, Home and End the
/// line, Ctrl-Home the document, Ctrl-Backspace a word; Ctrl-Y redoes, and the old Shift-Delete,
/// Ctrl-Insert and Shift-Insert cut, copy and paste. Ctrl-Enter does nothing on Windows.
fn pc(k: &KeyEvent, os: Os) -> Option<Command> {
  use Move::*;
  use NamedKey as N;
  let (shift, ctrl, alt, cmd) = flags(k.mods, os);
  let to = |m| Some(Command::Move(m, shift));
  if alt || ctrl {
    return None;
  }

  let key = match k.key {
    Key::Char('y' | 'Y') if cmd && !shift => return Some(Command::Redo),
    Key::Char(_) => return None,
    Key::Named(key) => key,
  };

  if let Some([alone, word, _]) = by_unit(key, shift) {
    return match (key, cmd, shift) {
      (N::Delete, false, true) => Some(Command::Cut),
      _ if cmd => word,
      _ => alone,
    };
  }

  match (key, cmd) {
    (N::Insert, true) => Some(Command::Copy),
    (N::Insert, false) if shift => Some(Command::Paste),
    (N::Home, false) => to(LineStart),
    (N::Home, true) => to(DocStart),
    (N::End, false) => to(LineEnd),
    (N::End, true) => to(DocEnd),
    (N::PageUp, false) => to(PageUp),
    (N::PageDown, false) => to(PageDown),
    (N::Enter, true) if os == Os::Windows => None,
    (N::Enter, _) => Some(Command::InsertNewline),
    (N::Tab, false) if shift => Some(Command::InsertBacktab),
    (N::Tab, false) => Some(Command::InsertTab),
    (N::Escape, false) => Some(Command::Cancel),
    _ => None,
  }
}

/// Whether text from `insertText:` may be inserted: not empty, and no control characters (Ctrl
/// keys AppKit didn't bind) or private-use function key characters (`NSUpArrowFunctionKey`).
pub fn printable(s: &str) -> bool {
  !s.is_empty() && s.chars().all(|c| !c.is_control() && !('\u{f700}'..='\u{f8ff}').contains(&c))
}

// MARK: Posting keys

/// Whether [post] works now without asking the user: macOS has Accessibility access, Linux on
/// Wayland has the RemoteDesktop portal's session or its saved approval (which the user may have
/// revoked since: only the next session tells), X11 has XTest; Windows always. An app offers its
/// own "Allow Pasting…" only until it does (wavo-app).
pub fn ready() -> bool {
  crate::platform::post::ready()
}

/// Sends shortcut `s` to the app in focus as key presses, as if typed: `cmd+v` pastes there. A
/// character is the key that types it on the layout in use (on macOS with ⌘ held, as apps match
/// ⌘ shortcuts: ⌘V on a Russian layout, or in Dvorak), with Shift added where it needs it
/// (`cmd++`); a named key is its key. On the main thread.
///
/// | | how | needs |
/// |---|---|---|
/// | macOS | `CGEvent`s posted at the HID level, the modifiers as their flags | Accessibility ([crate::permissions::accessibility]); Secure Input may swallow them |
/// | Windows | `SendInput`: the modifier keys down, the key, the modifiers up | an app in focus that does not run elevated above this one |
/// | Linux | XTest (`libXtst`, loaded at run time) on X11; on Wayland the RemoteDesktop portal's keyboard | Wayland: the user's approval, asked once (the desktop's dialog) |
pub fn post(s: Shortcut) -> Result<(), String> {
  let key = match s.key {
    Key::Char(c) => Key::Char(lower(c)),
    k => k,
  };

  crate::platform::post::post(Shortcut { key, ..s })
}

/// The presses that send `key` with modifiers `want` where modifier keys are pressed (Windows,
/// X11), `of` giving the key at a modifier's place and `down` whether a key is held now: each
/// held key of a modifier not wanted up, the left key of each wanted modifier not held down, the
/// key down and up, then each modifier key back as it was. The keys the user still holds (a
/// hotkey's) neither join the shortcut nor end up let go, and no other key stays down. A held Alt
/// or Win/Super lifted and put back would read as tapped alone (the menu bar, the Start menu,
/// GNOME's overview), at the lift and at the user's own release after: `mask`, a key that does
/// nothing, is tapped before the lift and after the keys are back. `None` when a wanted modifier
/// has no key.
#[cfg(any(test, not(target_os = "macos")))]
pub(crate) fn strokes<K: Copy>(
  want: Modifiers,
  key: K,
  of: impl Fn(Code) -> Option<K>,
  down: impl Fn(K) -> bool,
  mask: Option<K>,
) -> Option<Vec<(K, bool)>> {
  let (mut lift, mut press, mut lone) = (Vec::new(), Vec::new(), false);
  for &(m, .., left, right) in &MODS {
    let held: Vec<K> = [left, right].into_iter().filter_map(&of).filter(|&k| down(k)).collect();
    if !want.contains(m) {
      lone |= !held.is_empty() && m.intersects(Modifiers::ALT | Modifiers::META);
      lift.extend(held);
    } else if held.is_empty() {
      press.push(of(left)?);
    }
  }

  let (up, dn) = (|&k: &K| (k, false), |&k: &K| (k, true));
  let tap: Vec<_> =
    mask.filter(|_| lone).into_iter().flat_map(|m| [(m, true), (m, false)]).collect();
  let around = tap.iter().copied().chain(lift.iter().map(up)).chain(press.iter().map(dn));
  let back = press.iter().rev().map(up).chain(lift.iter().rev().map(dn)).chain(tap.iter().copied());
  Some(around.chain([(key, true), (key, false)]).chain(back).collect())
}

#[cfg(test)]
mod tests {
  use NamedKey as N;

  use super::*;

  fn ev(key: Key, mods: Modifiers) -> KeyEvent {
    KeyEvent { key, text: None, code: Code::Unidentified, mods, repeat: false }
  }

  const CMD_SHIFT: Modifiers = CMD.union(Modifiers::SHIFT);

  /// A global hotkey's shortcut.
  fn hot(text: &str) -> Result<Shortcut, ShortcutError> {
    match Hotkey::parse(text)? {
      Hotkey::Shortcut(s) => Ok(s),
      Hotkey::Modifier(_) => Err(ShortcutError::ModifierAlone),
    }
  }

  /// Ctrl-V as Windows and X11 get it: the left Ctrl around the key; a held right Shift lifted
  /// for it and put back after, itself and not the left one; a held right Ctrl left as it is; and
  /// none without a key for a modifier wanted. A held right Alt or Win lifted gets the mask key
  /// tapped before it goes up and after it is back, so neither its lift nor the user's release
  /// reads as a lone tap; Shift needs none.
  #[test]
  fn posted_keys_hold_the_modifiers_around_the_key() {
    use Code::{
      AltRight as RAlt, ControlLeft as LCtrl, ControlRight as RCtrl, F24 as Mask, KeyA, KeyV,
      MetaRight as RWin, ShiftRight as RShift,
    };
    let s = |want, held: &[Code], key| strokes(want, key, Some, |c| held.contains(&c), Some(Mask));
    let ctrl = Modifiers::CONTROL;
    let want = [(RShift, false), (LCtrl, true), (KeyV, true), (KeyV, false), (LCtrl, false)];
    assert_eq!(s(ctrl, &[RShift], KeyV), Some([&want[..], &[(RShift, true)]].concat()));
    assert_eq!(s(ctrl, &[RCtrl], KeyV), Some(vec![(KeyV, true), (KeyV, false)]));
    assert_eq!(s(Modifiers::empty(), &[], KeyA), Some(vec![(KeyA, true), (KeyA, false)]));
    let no_left = |c| Some(c).filter(|&c| c != LCtrl);
    assert_eq!(strokes(ctrl, KeyV, no_left, |_| false, None), None);

    let tap = [(Mask, true), (Mask, false)];
    for held in [RAlt, RWin] {
      let lifted = [(held, false), (LCtrl, true), (KeyV, true), (KeyV, false), (LCtrl, false)];
      let want = [&tap[..], &lifted, &[(held, true)], &tap].concat();
      assert_eq!(s(ctrl, &[held], KeyV), Some(want), "{held}");
    }

    let alt = Modifiers::ALT;
    assert_eq!(s(alt, &[RAlt], KeyV), Some(vec![(KeyV, true), (KeyV, false)]), "Alt kept");
  }

  #[test]
  fn parses_shortcuts_in_the_app() {
    let k = |key, mods| Ok(Shortcut { key, mods });
    assert_eq!(Shortcut::parse("cmd+z"), k(Key::Char('z'), CMD));
    assert_eq!(Shortcut::parse("cmd+shift+z"), k(Key::Char('z'), CMD_SHIFT));
    assert_eq!(Shortcut::parse("cmd-shift-Z"), k(Key::Char('z'), CMD_SHIFT), "dashes");
    assert_eq!(Shortcut::parse("cmd+-"), k(Key::Char('-'), CMD));
    assert_eq!(Shortcut::parse("cmd--"), k(Key::Char('-'), CMD));
    assert_eq!(Shortcut::parse("cmd++"), k(Key::Char('+'), CMD));
    assert_eq!(Shortcut::parse("ctrl + -"), k(Key::Char('-'), CTRL));
    assert_eq!(Shortcut::parse("-"), k(Key::Char('-'), Modifiers::empty()));
    assert_eq!(Shortcut::parse("alt+left"), k(Key::Named(N::ArrowLeft), Modifiers::ALT));
    assert_eq!(Shortcut::parse("escape"), k(Key::Named(N::Escape), Modifiers::empty()));
    assert_eq!(Shortcut::parse("f12"), k(Key::Named(N::F12), Modifiers::empty()));
    assert_eq!(Shortcut::parse("space"), k(Key::Char(' '), Modifiers::empty()));
    assert_eq!(Shortcut::parse("cmd+?"), k(Key::Char('?'), CMD));
    assert_eq!(Shortcut::parse("shift+İ"), k(Key::Char('İ'), Modifiers::SHIFT));
    // `delete` is ⌫ in the app too, as in settings; ⌦ is `forwarddelete`.
    let backspace = Key::Named(N::Backspace);
    assert_eq!(Shortcut::parse("cmd+delete"), k(backspace, CMD));
    assert_eq!(Shortcut::parse("backspace"), k(backspace, Modifiers::empty()));
    assert_eq!(Shortcut::parse("forwarddelete"), k(Key::Named(N::Delete), Modifiers::empty()));
    assert_eq!(hot("delete").map(|s| s.key), Ok(backspace));
    assert_eq!(Shortcut::parse("a+!"), Err(ShortcutError::TwoKeys("a".into(), "!".into())));
    for bad in ["", "cmd+", "hyper+a", "ab", "cmd+ab", "f21"] {
      assert!(Shortcut::parse(bad).is_err(), "{bad}");
    }

    // In the app `cmd` is the command key; system-wide, ⌘ or the Windows or Super key.
    let all = Shortcut::parse("ctrl+cmd+alt+shift+a").unwrap().mods;
    assert_eq!(all, Modifiers::CONTROL | Modifiers::META | Modifiers::ALT | Modifiers::SHIFT);
    assert_eq!(Shortcut::parse("cmd+a").unwrap().mods, CMD);
    assert_eq!(hot("cmd+a").unwrap().mods, Modifiers::META);
    assert_eq!(hot("ctrl+a").unwrap().mods, Modifiers::CONTROL);
  }

  #[test]
  fn matches_letters_in_either_case_and_symbols_with_shift() {
    // Cmd-Shift-Z reports 'Z'.
    assert!(ev(Key::Char('Z'), CMD_SHIFT).is("cmd+shift+z"));
    assert!(!ev(Key::Char('Z'), CMD_SHIFT).is("cmd+z"));
    assert!(ev(Key::Char('z'), CMD).is("cmd+z"));
    // Cmd-Shift-= types '+' on a US layout.
    assert!(ev(Key::Char('+'), CMD_SHIFT).is("cmd++"));
    assert!(!ev(Key::Char('+'), CMD_SHIFT).is("cmd+="));
    assert!(ev(Key::Char('='), CMD).is("cmd+="));
    assert!(!ev(Key::Named(N::ArrowLeft), Modifiers::SHIFT).is("left"));
    let map = [bind("cmd+z", Command::Undo), bind("cmd+shift+z", Command::Redo)];
    assert_eq!(lookup(&map, &ev(Key::Char('Z'), CMD_SHIFT)), Some(Command::Redo));
    assert_eq!(lookup(&map, &ev(Key::Char('y'), CMD)), None);
  }

  #[test]
  fn shortcuts_show_as_in_menus() {
    let show = |s: &str| {
      let k = hot(s).unwrap();
      (k.symbols(), k.names())
    };
    let pc = |s: &str| if cfg!(windows) { s.replace("Super", "Win") } else { s.to_string() };
    for (text, symbols, names) in [
      ("cmd+shift+z", "⇧⌘Z", "Shift+Super+Z"),
      ("ctrl+alt+left", "⌃⌥←", "Ctrl+Alt+Left"),
      ("cmd+=", "⌘=", "Super+="),
      ("escape", "⎋", "Esc"),
      ("f5", "F5", "F5"),
      ("cmd+space", "⌘Space", "Super+Space"),
    ] {
      assert_eq!(show(text), (symbols.to_string(), pc(names)), "{text}");
    }

    // In the app, the command key: ⌘ on macOS, Ctrl elsewhere.
    let map = [bind("cmd+z", Command::Undo), bind("cmd+shift+z", Command::Redo)];
    let redo = shortcut(&map, Command::Redo).map(|k| k.label());
    let want = if cfg!(target_os = "macos") { "⇧⌘Z" } else { "Ctrl+Shift+Z" };
    assert_eq!(redo.as_deref(), Some(want));
    assert_eq!(shortcut(&map, Command::Copy), None);
  }

  #[test]
  fn named_keys_by_place() {
    assert_eq!(Key::named(Code::ArrowLeft), Some(Key::Named(N::ArrowLeft)));
    assert_eq!(Key::named(Code::NumpadEnter), Some(Key::Named(N::Enter)));
    assert_eq!(Key::named(Code::F12), Some(Key::Named(N::F12)));
    assert_eq!(Key::named(Code::KeyA), None);
    let some = [N::Enter, N::Tab, N::Escape, N::PageDown, N::Insert, N::ContextMenu, N::F1, N::F20];
    for n in some {
      assert_eq!(Key::Named(n).code().and_then(Key::named), Some(Key::Named(n)));
    }

    assert_eq!(Key::Named(N::Enter).code(), Some(Code::Enter));
    assert_eq!((f_key(13), f_key(0), f_key(21)), (Some((Code::F13, N::F13)), None, None));
    assert_eq!(Key::Char('a').code(), None);
  }

  #[test]
  fn us_keyboard() {
    assert_eq!((us_char(Code::KeyC, false), us_char(Code::KeyC, true)), (Some('c'), Some('C')));
    assert_eq!((us_char(Code::Digit1, true), us_char(Code::Quote, true)), (Some('!'), Some('"')));
    assert_eq!((us_char(Code::Space, false), us_char(Code::Enter, false)), (Some(' '), None));
    assert_eq!((us_code('a'), us_code('A')), (Some((Code::KeyA, false)), Some((Code::KeyA, true))));
    assert_eq!(
      (us_code('~'), us_code(' '), us_code('é')),
      (Some((Code::Backquote, true)), Some((Code::Space, false)), None)
    );
    for &code in &US {
      for shift in [false, true] {
        let c = us_char(code, shift).unwrap();
        assert_eq!(us_code(c), Some((code, shift && c != ' ')), "{code}");
      }
    }
  }

  /// The labels of every named key, on both platforms' menus.
  #[test]
  fn key_labels() {
    let labels = |mac| NAMES.map(|(k, ..)| key_label(Key::Named(k), mac)).join(" ");
    assert_eq!(labels(true), "↩ ⇥ ⌫ ⌦ ⎋ ↑ ↓ ← → ↖ ↘ ⇞ ⇟");
    let pc = "Enter Tab Backspace Delete Esc Up Down Left Right Home End Page Up Page Down";
    assert_eq!(labels(false), pc);
    let other = [Key::Char(' '), Key::Char('ß'), Key::Named(N::F5), Key::Named(N::Insert)];
    assert_eq!(other.map(|k| key_label(k, false)), ["Space", "SS", "F5", "Insert"]);
  }

  /// Every global hotkey key with Ctrl-Shift: its canonical spelling parses back to it, and it
  /// shows in macOS's symbols and the PC's names.
  #[test]
  fn every_hotkey_key() {
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
      let c = hot(name).unwrap();
      assert_eq!(hot(&c.to_string()), Ok(c), "{row}");
      assert_eq!(format!("{c} {} {}", c.symbols(), c.names()), row);
      assert!(c.code().is_some(), "{row}");
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
      assert_eq!(hot(alias), hot(name), "{alias}");
    }

    let places = ["ctrl+/", "a", "1", "f12", "space", "return", "delete", "forwarddelete"];
    let codes = places.map(|s| hot(s).unwrap().code().unwrap());
    let want = [Code::Slash, Code::KeyA, Code::Digit1, Code::F12, Code::Space, Code::Enter];
    assert_eq!(codes[..6], want);
    assert_eq!(codes[6..], [Code::Backspace, Code::Delete]);
  }

  #[test]
  fn parses_settings_and_other_spellings() {
    let k = |key, mods| Ok(Shortcut { key, mods });
    let (ctrl, alt, meta) = (Modifiers::CONTROL, Modifiers::ALT, Modifiers::META);
    assert_eq!(hot("option+space"), k(Key::Char(' '), alt));
    assert_eq!(hot("ctrl+option+cmd+F12"), k(Key::Named(N::F12), ctrl | alt | meta));
    assert_eq!(hot(" Control + Alt + Command + f12 "), hot("ctrl+option+cmd+f12"));
    assert_eq!(hot("option+shift+space"), k(Key::Char(' '), alt | Modifiers::SHIFT));
    assert_eq!(hot("cmd+K"), k(Key::Char('k'), meta));
    assert_eq!(hot("ctrl+/"), k(Key::Char('/'), ctrl));
    assert_eq!(hot("escape"), hot("esc"));
    for (text, canonical, symbols, names) in [
      ("ctrl+option+cmd+F12", "ctrl+option+cmd+f12", "⌃⌥⌘F12", "Ctrl+Alt+Super+F12"),
      ("Alt+Space", "option+space", "⌥Space", "Alt+Space"),
      ("cmd+shift+a", "shift+cmd+a", "⇧⌘A", "Shift+Super+A"),
      ("ctrl+/", "ctrl+slash", "⌃/", "Ctrl+/"),
      ("ctrl+esc", "ctrl+escape", "⌃⎋", "Ctrl+Esc"),
      ("shift+pagedown", "shift+pagedown", "⇧⇟", "Shift+Page Down"),
    ] {
      let c = hot(text).unwrap();
      assert_eq!(c.to_string(), canonical);
      assert_eq!(hot(canonical), Ok(c), "round trip of {text}");
      // The command key is Windows' Win key there.
      let names = if cfg!(windows) { names.replace("Super", "Win") } else { names.to_string() };
      assert_eq!((c.symbols(), c.names()), (symbols.to_string(), names));
    }

    let held = if cfg!(target_os = "macos") { "⌃⌥" } else { "Ctrl+Alt+" };
    assert_eq!(modifiers_label(Modifiers::CONTROL | Modifiers::ALT), held);
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

      assert_eq!(Shortcut::parse(text), Err(ShortcutError::ModifierAlone), "{text}");
    }

    let keys = MODS.iter().flat_map(|r| [r.4, r.5]).chain([Code::Fn]);
    for key in keys.clone() {
      let hotkey = Hotkey::Modifier(key);
      assert_eq!(Hotkey::parse(&hotkey.to_string()), Ok(hotkey));
      assert!(is_modifier(key));
    }

    let names = keys.map(|k| Hotkey::Modifier(k).to_string());
    let want = "left_ctrl right_ctrl left_option right_option left_shift right_shift left_cmd \
                right_cmd fn";
    assert_eq!(names.collect::<Vec<_>>().join(" "), want);
    assert!(!is_modifier(Code::CapsLock) && !is_modifier(Code::KeyA));
    // Shortcuts stay shortcuts; a modifier without a side is no key of its own.
    assert!(matches!(Hotkey::parse("ctrl+option+space"), Ok(Hotkey::Shortcut(_))));
    assert_eq!(Hotkey::parse("option"), Err(ShortcutError::NoKey));
    assert_eq!(Hotkey::parse("fn+f1"), Err(ShortcutError::Fn));
    assert!(Hotkey::parse("right").is_ok_and(|h| h != Hotkey::Modifier(Code::AltRight)));
    assert!(Hotkey::parse("rx").is_err());
    assert!(matches!(Hotkey::parse("l+shift"), Ok(Hotkey::Shortcut(_))), "the L key with Shift");
  }

  #[test]
  fn rejects_what_is_not_a_hotkey() {
    use ShortcutError::*;
    assert_eq!(Hotkey::parse("  "), Err(Empty));
    assert_eq!(Hotkey::parse("ctrl+option"), Err(NoKey));
    assert_eq!(Hotkey::parse("fn+f1"), Err(Fn));
    assert_eq!(Hotkey::parse("ctrl+hyper"), Err(Unknown("hyper".into())));
    assert_eq!(Hotkey::parse("a+b"), Err(TwoKeys("a".into(), "b".into())));
    // Keys with no place without Shift on a US keyboard.
    assert_eq!(Hotkey::parse("ctrl+!"), Err(Unknown("!".into())));
    assert_eq!(Hotkey::parse("a+!"), Err(Unknown("!".into())));
    for bad in ["f0", "f21", "f012", "ctrl++", "ctrl+", "é"] {
      assert!(Hotkey::parse(bad).is_err(), "{bad}");
    }
  }

  #[test]
  fn selectors() {
    let sel = Command::from_selector;
    assert_eq!(sel("moveWordLeft:"), Some(Command::Move(Move::WordLeft, false)));
    assert_eq!(sel("moveWordLeftAndModifySelection:"), Some(Command::Move(Move::WordLeft, true)));
    assert_eq!(
      sel("moveToBeginningOfParagraph:"),
      Some(Command::Move(Move::ParagraphStart, false))
    );
    assert_eq!(sel("deleteBackward:"), Some(Command::Delete(Delete::Backward)));
    assert_eq!(sel("insertNewline:"), Some(Command::InsertNewline));
    assert_eq!(sel("selectAll:"), Some(Command::SelectAll));
    assert_eq!(sel("deleteBackwardAndModifySelection:"), None);
    assert_eq!(sel("noop:"), None);
    assert_eq!(sel("moveLeft"), None);
  }

  #[test]
  fn standard_bindings_agree_with_selectors() {
    let (alt, ctrl, cmd) = (Modifiers::ALT, Modifiers::CONTROL, Modifiers::META);
    let alt_shift = Modifiers::ALT | Modifiers::SHIFT;
    // StandardKeyBinding.dict: key → selector.
    for (key, mods, selector) in [
      (Key::Named(N::ArrowLeft), Modifiers::empty(), "moveLeft:"),
      (Key::Named(N::ArrowLeft), alt_shift, "moveWordLeftAndModifySelection:"),
      (Key::Named(N::ArrowRight), cmd, "moveToRightEndOfLine:"),
      (Key::Named(N::ArrowUp), cmd, "moveToBeginningOfDocument:"),
      (Key::Named(N::Backspace), alt, "deleteWordBackward:"),
      (Key::Named(N::Backspace), cmd, "deleteToBeginningOfLine:"),
      (Key::Named(N::Backspace), ctrl, "deleteBackward:"),
      (Key::Named(N::Delete), alt, "deleteWordForward:"),
      (Key::Char('a'), ctrl, "moveToBeginningOfParagraph:"),
      (Key::Char('k'), ctrl, "deleteToEndOfParagraph:"),
      (Key::Named(N::Tab), Modifiers::SHIFT, "insertBacktab:"),
      (Key::Named(N::Escape), Modifiers::empty(), "cancelOperation:"),
      (Key::Named(N::Home), Modifiers::empty(), "scrollToBeginningOfDocument:"),
    ] {
      let want = Command::from_selector(selector);
      assert_eq!(Os::Mac.command(&ev(key, mods)), want, "{selector}");
    }
    assert_eq!(standard(&ev(Key::Char('c'), cmd)), None);
    assert_eq!(standard(&ev(Key::Named(N::ArrowLeft), ctrl)), None);
    assert_eq!(standard(&ev(Key::Named(N::Delete), cmd)), None);
  }

  /// Windows' and GTK's text keys, Ctrl the command key there whatever the host.
  #[test]
  fn pc_bindings_use_ctrl_as_cmd() {
    let (ctrl, shift) = (Modifiers::CONTROL, Modifiers::SHIFT);
    for os in [Os::Windows, Os::Linux] {
      for (key, mods, want) in [
        (Key::Named(N::ArrowLeft), ctrl, Command::Move(Move::WordLeft, false)),
        (Key::Named(N::ArrowRight), ctrl | shift, Command::Move(Move::WordRight, true)),
        (Key::Named(N::Home), Modifiers::empty(), Command::Move(Move::LineStart, false)),
        (Key::Named(N::End), ctrl | shift, Command::Move(Move::DocEnd, true)),
        (Key::Named(N::Backspace), ctrl, Command::Delete(Delete::WordBackward)),
        (Key::Named(N::Delete), ctrl, Command::Delete(Delete::WordForward)),
        (Key::Named(N::Delete), Modifiers::empty(), Command::Delete(Delete::Forward)),
        (Key::Named(N::ArrowUp), ctrl | shift, Command::Move(Move::ParagraphStart, true)),
        (Key::Named(N::Tab), shift, Command::InsertBacktab),
        (Key::Named(N::Enter), Modifiers::empty(), Command::InsertNewline),
        (Key::Char('y'), ctrl, Command::Redo),
        (Key::Named(N::Delete), shift, Command::Cut),
        (Key::Named(N::Insert), ctrl, Command::Copy),
        (Key::Named(N::Insert), shift, Command::Paste),
      ] {
        assert_eq!(os.command(&ev(key, mods)), Some(want), "{os:?} {key:?} {mods:?}");
      }

      for (key, mods) in [
        (Key::Named(N::ArrowLeft), Modifiers::ALT),
        (Key::Named(N::ArrowLeft), Modifiers::META),
        (Key::Char('a'), ctrl),
        (Key::Char('y'), ctrl | shift),
      ] {
        assert_eq!(os.command(&ev(key, mods)), None, "{os:?} {key:?} {mods:?}");
      }
    }

    let ctrl_enter = ev(Key::Named(N::Enter), ctrl);
    assert_eq!(Os::Windows.command(&ctrl_enter), None);
    assert_eq!(Os::Linux.command(&ctrl_enter), Some(Command::InsertNewline));
  }

  /// `cmd` in a binding is each platform's command key, whatever the host.
  #[test]
  fn bindings_follow_the_platform() {
    let map = [bind("cmd+v", Command::Paste), bind("ctrl+a", Command::SelectAll)];
    let (ctrl, meta) = (Modifiers::CONTROL, Modifiers::META);
    for (os, cmd, other) in
      [(Os::Mac, meta, ctrl), (Os::Windows, ctrl, meta), (Os::Linux, ctrl, meta)]
    {
      assert_eq!(os.lookup(&map, &ev(Key::Char('v'), cmd)), Some(Command::Paste), "{os:?}");
      assert_eq!(os.lookup(&map, &ev(Key::Char('v'), other)), None, "{os:?}");
      assert_eq!(os.lookup(&map, &ev(Key::Char('a'), other)), Some(Command::SelectAll), "{os:?}");
    }

    assert_eq!((CMD, CTRL), (Os::CURRENT.cmd(), Os::CURRENT.ctrl()));
  }

  #[test]
  fn printable_text() {
    assert!(printable("a") && printable("é") && printable("👍🏽"));
    assert!(!printable("") && !printable("\u{1}") && !printable("\u{f700}") && !printable("\t"));
  }
}
