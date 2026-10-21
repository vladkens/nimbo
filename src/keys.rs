// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Keys: what a key press reports ([KeyEvent]), keystrokes and bindings ([Keystroke],
//! [Binding]), and the commands keys become ([Command]), named after AppKit's key binding
//! selectors (`moveWordLeft:`, `deleteBackward:`, `insertNewline:`).
//!
//! A key press travels in this order (the runtime routes it, [crate::Runtime::event]):
//!
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
//! The harness has no AppKit: it builds key events on a US layout and interprets them with
//! [standard], the part of AppKit's standard bindings we model.

use crate::app::Mods;

/// A key: named, or the character it types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
  /// A key that types a character: the character on the current layout with Shift applied
  /// but no other modifier (`charactersIgnoringModifiers`): `'a'`, `'A'` with Shift, `'!'` for
  /// Shift-1, `'a'` for Option-A or Cmd-A, `' '` for the space bar. With Cmd or Ctrl a
  /// non-ASCII character is replaced by the ASCII-capable layout's (Cmd-C on a Russian layout
  /// is `'c'`, not `'с'`), as AppKit matches menu shortcuts.
  Char(char),
  Enter,
  Tab,
  Backspace,
  /// Forward delete.
  Delete,
  Escape,
  Left,
  Right,
  Up,
  Down,
  Home,
  End,
  PageUp,
  PageDown,
  F(u8),
  /// Anything else, by its virtual key code.
  Other(u16),
}

/// Named keys by virtual key code (Carbon's `kVK_*`); F-keys are in [F_KEYS].
const NAMED: [(u16, Key); 14] = [
  (36, Key::Enter),
  (76, Key::Enter), // keypad
  (48, Key::Tab),
  (51, Key::Backspace),
  (117, Key::Delete),
  (53, Key::Escape),
  (123, Key::Left),
  (124, Key::Right),
  (125, Key::Down),
  (126, Key::Up),
  (115, Key::Home),
  (119, Key::End),
  (116, Key::PageUp),
  (121, Key::PageDown),
];

/// Virtual key codes of F1 to F12.
const F_KEYS: [u16; 12] = [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111];

impl Key {
  /// The named key at virtual key code `code`, if it is one.
  pub fn named(code: u16) -> Option<Self> {
    if let Some(&(_, k)) = NAMED.iter().find(|(c, _)| *c == code) {
      return Some(k);
    }

    F_KEYS.iter().position(|&c| c == code).map(|i| Self::F(i as u8 + 1))
  }

  /// The virtual key code of a named key (the main Enter); `None` for characters.
  pub fn code(self) -> Option<u16> {
    match self {
      Self::Char(_) => None,
      Self::F(n) => F_KEYS.get((n as usize).wrapping_sub(1)).copied(),
      Self::Other(c) => Some(c),
      k => NAMED.iter().find(|(_, n)| *n == k).map(|(c, _)| *c),
    }
  }
}

/// What the keys of a US ANSI keyboard type, by virtual key code, without and with Shift; `\0`
/// where a code is not a character key.
pub(crate) const US: &str = "asdfhgzxcv\0bqweryt123465=97-80]ou[ip\0lj'k;\\,/nm.\0 `";
pub(crate) const US_SHIFT: &str = "ASDFHGZXCV\0BQWERYT!@#$^%+(&_*)}OU{IP\0LJ\"K:|<?NM>\0 ~";

/// The character key `code` types on a US keyboard, with Shift or without.
pub fn us_char(code: u16, shift: bool) -> Option<char> {
  let table = if shift { US_SHIFT } else { US };
  table.chars().nth(code as usize).filter(|&c| c != '\0')
}

/// A key press or release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
  pub key: Key,
  /// What the key types with every modifier applied (`characters`): `'å'` for Option-A on a US
  /// layout. `None` for named keys, with Cmd or Ctrl held, and for dead keys. Typed text
  /// reaches a text field as [crate::Event::Text], not from here.
  pub text: Option<char>,
  /// The physical key, as a virtual key code (Carbon's `kVK_*`, named after US ANSI
  /// positions: 0 is A, 18 is 1), the same on every layout.
  pub code: u16,
  pub mods: Mods,
  /// An auto-repeat of a held key.
  pub repeat: bool,
}

impl KeyEvent {
  /// Whether this is the keystroke `s` ("cmd-shift-z", "escape"); see [Keystroke].
  pub fn is(&self, s: &str) -> bool {
    Keystroke::parse(s).is_some_and(|k| k.matches(self))
  }
}

// MARK: Keystrokes and bindings

/// A key with modifiers, written as in Zed and Sublime: modifiers first, joined by `-`:
/// `"cmd-z"`, `"cmd-shift-z"`, `"alt-left"`, `"ctrl-a"`, `"escape"`, `"f5"`, `"cmd--"`,
/// `"space"`. Modifiers: `cmd`, `ctrl`, `alt` (or `opt`), `shift`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keystroke {
  pub key: Key,
  pub mods: Mods,
}

impl Keystroke {
  pub fn parse(s: &str) -> Option<Self> {
    let mut mods = Mods::NONE;
    let mut rest = s;
    loop {
      let (name, tail) = match rest.split_once('-') {
        Some((name, tail)) if !tail.is_empty() => (name, tail),
        _ => break,
      };

      match name {
        "cmd" => mods.cmd = true,
        "ctrl" => mods.ctrl = true,
        "alt" | "opt" => mods.alt = true,
        "shift" => mods.shift = true,
        _ => return None,
      }

      rest = tail;
    }

    let key = match rest {
      "enter" | "return" => Key::Enter,
      "tab" => Key::Tab,
      "space" => Key::Char(' '),
      "backspace" => Key::Backspace,
      "delete" => Key::Delete,
      "escape" | "esc" => Key::Escape,
      "left" => Key::Left,
      "right" => Key::Right,
      "up" => Key::Up,
      "down" => Key::Down,
      "home" => Key::Home,
      "end" => Key::End,
      "pageup" => Key::PageUp,
      "pagedown" => Key::PageDown,
      _ => match rest.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        Some(n) if (1..=20).contains(&n) => Key::F(n),
        _ => {
          let mut cs = rest.chars();
          let (Some(c), None) = (cs.next(), cs.next()) else { return None };
          Key::Char(c)
        }
      },
    };

    Some(Self { key, mods })
  }

  /// Letters match in either case, Shift given by the modifiers: `"cmd-shift-z"` is Cmd-Shift-Z.
  /// Any other character matches itself, Shift or not, since the layout may need Shift to type
  /// it: `"cmd-+"` is Cmd-Shift-= on a US layout. Named keys need the exact modifiers.
  pub fn matches(&self, k: &KeyEvent) -> bool {
    match (self.key, k.key) {
      (Key::Char(a), Key::Char(b)) if !a.is_alphabetic() => {
        a == b && Mods { shift: false, ..self.mods } == Mods { shift: false, ..k.mods }
      }
      (Key::Char(a), Key::Char(b)) => lower(a) == lower(b) && self.mods == k.mods,
      (a, b) => a == b && self.mods == k.mods,
    }
  }
}

/// As macOS menus show it: `⌃⌥⇧⌘` in that order, then the key: `⇧⌘Z`, `⌘=`, `⎋`, `F5`. On
/// Windows as its menus do, Cmd being Ctrl there: `Ctrl+Shift+Z`, `Ctrl+=`, `Esc`, `F5`.
impl std::fmt::Display for Keystroke {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    if cfg!(target_os = "windows") {
      return self.fmt_windows(f);
    }

    let m = self.mods;
    for (on, glyph) in [(m.ctrl, '⌃'), (m.alt, '⌥'), (m.shift, '⇧'), (m.cmd, '⌘')] {
      if on {
        write!(f, "{glyph}")?;
      }
    }

    let name = match self.key {
      Key::Char(' ') => "Space",
      Key::Char(c) => return write!(f, "{}", c.to_uppercase()),
      Key::F(n) => return write!(f, "F{n}"),
      Key::Enter => "↩",
      Key::Tab => "⇥",
      Key::Backspace => "⌫",
      Key::Delete => "⌦",
      Key::Escape => "⎋",
      Key::Left => "←",
      Key::Right => "→",
      Key::Up => "↑",
      Key::Down => "↓",
      Key::Home => "↖",
      Key::End => "↘",
      Key::PageUp => "⇞",
      Key::PageDown => "⇟",
      Key::Other(_) => "?",
    };

    write!(f, "{name}")
  }
}

impl Keystroke {
  fn fmt_windows(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    let m = self.mods;
    for (on, name) in [(m.ctrl, "Win"), (m.cmd, "Ctrl"), (m.alt, "Alt"), (m.shift, "Shift")] {
      if on {
        write!(f, "{name}+")?;
      }
    }

    let name = match self.key {
      Key::Char(' ') => "Space",
      Key::Char(c) => return write!(f, "{}", c.to_uppercase()),
      Key::F(n) => return write!(f, "F{n}"),
      Key::Enter => "Enter",
      Key::Tab => "Tab",
      Key::Backspace => "Backspace",
      Key::Delete => "Del",
      Key::Escape => "Esc",
      Key::Left => "Left",
      Key::Right => "Right",
      Key::Up => "Up",
      Key::Down => "Down",
      Key::Home => "Home",
      Key::End => "End",
      Key::PageUp => "PgUp",
      Key::PageDown => "PgDn",
      Key::Other(_) => "?",
    };

    write!(f, "{name}")
  }
}

fn lower(c: char) -> char {
  c.to_lowercase().next().unwrap_or(c)
}

/// A keystroke ([Keystroke] syntax) and the command it runs. `const`, so a keymap is a
/// constant: `const KEYS: &[Binding] = &[bind("cmd-z", Command::Undo)]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
  pub keys: &'static str,
  pub command: Command,
}

pub const fn bind(keys: &'static str, command: Command) -> Binding {
  Binding { keys, command }
}

/// The keystroke of the first binding in `map` that runs `cmd`: to show it beside the command,
/// in a menu item or a tooltip.
pub fn shortcut(map: &[Binding], cmd: Command) -> Option<Keystroke> {
  map.iter().find(|b| b.command == cmd).and_then(|b| Keystroke::parse(b.keys))
}

/// The command of the first binding in `map` that matches `k`.
pub fn lookup(map: &[Binding], k: &KeyEvent) -> Option<Command> {
  map.iter().find(|b| k.is(b.keys)).map(|b| b.command)
}

/// The edit menu's keys, which a text field binds on itself (AppKit gets them from the
/// menu's key equivalents, which we don't have).
pub const EDIT_KEYS: &[Binding] = &[
  bind("cmd-a", Command::SelectAll),
  bind("cmd-c", Command::Copy),
  bind("cmd-x", Command::Cut),
  bind("cmd-v", Command::Paste),
  bind("cmd-z", Command::Undo),
  bind("cmd-shift-z", Command::Redo),
];

/// The keys of selectable text: select all and copy.
pub const SELECT_KEYS: &[Binding] =
  &[bind("cmd-a", Command::SelectAll), bind("cmd-c", Command::Copy)];

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
  /// The app's own command, by name: `bind("cmd-n", Command::Action("new"))`.
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
  let Mods { shift, ctrl, alt, cmd } = k.mods;
  let to = |m| Some(Command::Move(m, shift));
  let del = |d| Some(Command::Delete(d));
  // Ctrl with an arrow is Mission Control's; with Option or Cmd, nothing.
  match (k.key, ctrl, alt, cmd) {
    (Key::Left, false, false, false) => to(Left),
    (Key::Left, false, true, false) => to(WordLeft),
    (Key::Left, false, false, true) => to(LineStart),
    (Key::Right, false, false, false) => to(Right),
    (Key::Right, false, true, false) => to(WordRight),
    (Key::Right, false, false, true) => to(LineEnd),
    (Key::Up, false, false, false) => to(Up),
    (Key::Up, false, true, false) => to(ParagraphStart),
    (Key::Up, false, false, true) => to(DocStart),
    (Key::Down, false, false, false) => to(Down),
    (Key::Down, false, true, false) => to(ParagraphEnd),
    (Key::Down, false, false, true) => to(DocEnd),
    (Key::Home, false, false, false) if shift => to(DocStart),
    (Key::Home, false, false, false) => Some(Command::ScrollToTop),
    (Key::End, false, false, false) if shift => to(DocEnd),
    (Key::End, false, false, false) => Some(Command::ScrollToBottom),
    (Key::PageUp, false, false, false) if shift => to(PageUp),
    (Key::PageUp, false, false, false) => Some(Command::ScrollPageUp),
    (Key::PageDown, false, false, false) if shift => to(PageDown),
    (Key::PageDown, false, false, false) => Some(Command::ScrollPageDown),
    (Key::Backspace, _, false, false) => del(Delete::Backward),
    (Key::Backspace, false, true, false) => del(Delete::WordBackward),
    (Key::Backspace, false, false, true) => del(Delete::ToLineStart),
    (Key::Delete, _, false, false) => del(Delete::Forward),
    (Key::Delete, false, true, false) => del(Delete::WordForward),
    (Key::Enter, _, _, false) => Some(Command::InsertNewline),
    (Key::Tab, false, false, false) if shift => Some(Command::InsertBacktab),
    (Key::Tab, false, false, false) => Some(Command::InsertTab),
    (Key::Escape, false, false, false) => Some(Command::Cancel),
    // Emacs keys.
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
  }
}

/// The command a key that types nothing runs on Linux and Windows, as their text fields do
/// (GTK, Win32 edit controls): the window gets it from here where AppKit's bindings would be.
/// Ctrl is the command modifier there and arrives as `cmd`: Ctrl-Left a word, Home and End the
/// line, Ctrl-Home the document, Ctrl-Backspace a word.
pub fn pc(k: &KeyEvent) -> Option<Command> {
  use Move::*;
  let Mods { shift, ctrl, alt, cmd } = k.mods;
  let to = |m| Some(Command::Move(m, shift));
  let del = |d| Some(Command::Delete(d));
  if alt || ctrl {
    return None;
  }

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
    (Key::Delete, false) => del(Delete::Forward),
    (Key::Delete, true) => del(Delete::WordForward),
    (Key::Enter, _) => Some(Command::InsertNewline),
    (Key::Tab, false) if shift => Some(Command::InsertBacktab),
    (Key::Tab, false) => Some(Command::InsertTab),
    (Key::Escape, false) => Some(Command::Cancel),
    _ => None,
  }
}

/// Whether text from `insertText:` may be inserted: not empty, and no control characters (Ctrl
/// keys AppKit didn't bind) or private-use function key characters (`NSUpArrowFunctionKey`).
pub fn printable(s: &str) -> bool {
  !s.is_empty() && s.chars().all(|c| !c.is_control() && !('\u{f700}'..='\u{f8ff}').contains(&c))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ev(key: Key, mods: Mods) -> KeyEvent {
    KeyEvent { key, text: None, code: 0, mods, repeat: false }
  }

  const CMD_SHIFT: Mods = Mods { cmd: true, shift: true, ..Mods::NONE };

  #[test]
  fn parses_keystrokes() {
    let k = |key, mods| Some(Keystroke { key, mods });
    assert_eq!(Keystroke::parse("cmd-z"), k(Key::Char('z'), Mods::CMD));
    assert_eq!(Keystroke::parse("cmd-shift-z"), k(Key::Char('z'), CMD_SHIFT));
    assert_eq!(Keystroke::parse("cmd--"), k(Key::Char('-'), Mods::CMD));
    assert_eq!(Keystroke::parse("-"), k(Key::Char('-'), Mods::NONE));
    assert_eq!(Keystroke::parse("alt-left"), k(Key::Left, Mods { alt: true, ..Mods::NONE }));
    assert_eq!(Keystroke::parse("escape"), k(Key::Escape, Mods::NONE));
    assert_eq!(Keystroke::parse("f12"), k(Key::F(12), Mods::NONE));
    assert_eq!(Keystroke::parse("space"), k(Key::Char(' '), Mods::NONE));
    for bad in ["", "cmd-", "hyper-a", "ab", "cmd-ab"] {
      assert_eq!(Keystroke::parse(bad), None, "{bad}");
    }
  }

  #[test]
  fn matches_letters_in_either_case_and_symbols_with_shift() {
    // Cmd-Shift-Z reports 'Z'.
    assert!(ev(Key::Char('Z'), CMD_SHIFT).is("cmd-shift-z"));
    assert!(!ev(Key::Char('Z'), CMD_SHIFT).is("cmd-z"));
    assert!(ev(Key::Char('z'), Mods::CMD).is("cmd-z"));
    // Cmd-Shift-= types '+' on a US layout.
    assert!(ev(Key::Char('+'), CMD_SHIFT).is("cmd-+"));
    assert!(!ev(Key::Char('+'), CMD_SHIFT).is("cmd-="));
    assert!(ev(Key::Char('='), Mods::CMD).is("cmd-="));
    assert!(!ev(Key::Left, Mods::SHIFT).is("left"));
    let map = [bind("cmd-z", Command::Undo), bind("cmd-shift-z", Command::Redo)];
    assert_eq!(lookup(&map, &ev(Key::Char('Z'), CMD_SHIFT)), Some(Command::Redo));
    assert_eq!(lookup(&map, &ev(Key::Char('y'), Mods::CMD)), None);
  }

  #[test]
  #[cfg(not(target_os = "windows"))]
  fn shortcuts_show_as_in_macos_menus() {
    let show = |s: &str| Keystroke::parse(s).unwrap().to_string();
    assert_eq!(show("cmd-shift-z"), "⇧⌘Z");
    assert_eq!(show("ctrl-alt-left"), "⌃⌥←");
    assert_eq!(show("cmd-="), "⌘=");
    assert_eq!(
      (show("escape"), show("f5"), show("cmd-space")),
      ("⎋".into(), "F5".into(), "⌘Space".into())
    );
    let map = [bind("cmd-z", Command::Undo), bind("cmd-shift-z", Command::Redo)];
    assert_eq!(shortcut(&map, Command::Redo).map(|k| k.to_string()).as_deref(), Some("⇧⌘Z"));
    assert_eq!(shortcut(&map, Command::Copy), None);
  }

  #[test]
  #[cfg(target_os = "windows")]
  fn shortcuts_show_as_in_windows_menus() {
    let show = |s: &str| Keystroke::parse(s).unwrap().to_string();
    assert_eq!(show("cmd-shift-z"), "Ctrl+Shift+Z");
    assert_eq!(show("cmd-="), "Ctrl+=");
    assert_eq!(
      (show("escape"), show("f5"), show("alt-left")),
      ("Esc".into(), "F5".into(), "Alt+Left".into())
    );
  }

  #[test]
  fn key_codes() {
    assert_eq!(Key::named(123), Some(Key::Left));
    assert_eq!(Key::named(76), Some(Key::Enter));
    assert_eq!(Key::named(111), Some(Key::F(12)));
    assert_eq!(Key::named(0), None);
    for k in [Key::Enter, Key::Tab, Key::Escape, Key::PageDown, Key::F(1), Key::F(12)] {
      assert_eq!(k.code().and_then(Key::named), Some(k));
    }

    assert_eq!(Key::F(13).code(), None);
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
    let alt = Mods { alt: true, ..Mods::NONE };
    let ctrl = Mods { ctrl: true, ..Mods::NONE };
    let alt_shift = Mods { alt: true, shift: true, ..Mods::NONE };
    // StandardKeyBinding.dict: key → selector.
    for (key, mods, selector) in [
      (Key::Left, Mods::NONE, "moveLeft:"),
      (Key::Left, alt_shift, "moveWordLeftAndModifySelection:"),
      (Key::Right, Mods::CMD, "moveToRightEndOfLine:"),
      (Key::Up, Mods::CMD, "moveToBeginningOfDocument:"),
      (Key::Backspace, alt, "deleteWordBackward:"),
      (Key::Backspace, Mods::CMD, "deleteToBeginningOfLine:"),
      (Key::Char('a'), ctrl, "moveToBeginningOfParagraph:"),
      (Key::Char('k'), ctrl, "deleteToEndOfParagraph:"),
      (Key::Tab, Mods::SHIFT, "insertBacktab:"),
      (Key::Escape, Mods::NONE, "cancelOperation:"),
      (Key::Home, Mods::NONE, "scrollToBeginningOfDocument:"),
    ] {
      assert_eq!(standard(&ev(key, mods)), Command::from_selector(selector), "{selector}");
    }
    assert_eq!(standard(&ev(Key::Char('c'), Mods::CMD)), None);
    assert_eq!(standard(&ev(Key::Left, ctrl)), None);
  }

  #[test]
  fn pc_bindings_use_ctrl_as_cmd() {
    let cmd_shift = Mods { cmd: true, shift: true, ..Mods::NONE };
    let alt = Mods { alt: true, ..Mods::NONE };
    for (key, mods, want) in [
      (Key::Left, Mods::CMD, Command::Move(Move::WordLeft, false)),
      (Key::Home, Mods::NONE, Command::Move(Move::LineStart, false)),
      (Key::End, cmd_shift, Command::Move(Move::DocEnd, true)),
      (Key::Backspace, Mods::CMD, Command::Delete(Delete::WordBackward)),
      (Key::Delete, Mods::NONE, Command::Delete(Delete::Forward)),
      (Key::Tab, Mods::SHIFT, Command::InsertBacktab),
    ] {
      assert_eq!(pc(&ev(key, mods)), Some(want), "{key:?} {mods:?}");
    }
    assert_eq!(pc(&ev(Key::Left, alt)), None);
    assert_eq!(pc(&ev(Key::Char('a'), Mods::CMD)), None);
  }

  #[test]
  fn printable_text() {
    assert!(printable("a") && printable("é") && printable("👍🏽"));
    assert!(!printable("") && !printable("\u{1}") && !printable("\u{f700}") && !printable("\t"));
  }
}
