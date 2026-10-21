// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Keys: what a key press reports ([KeyEvent]: our [Key], and the W3C UI Events [Code] and
//! [Modifiers] of the `keyboard-types` crate), keystrokes and bindings ([Keystroke], [Binding]),
//! and the commands keys become ([Command]), named after AppKit's key binding selectors
//! (`moveWordLeft:`, `deleteBackward:`, `insertNewline:`).
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
//! The harness has no AppKit: it builds key events on a US layout ([us_char]) and interprets them
//! with [standard], the part of AppKit's standard bindings we model.

use keyboard_types::{Code, Modifiers, NamedKey};

/// The command key, `cmd` in a keystroke: ⌘ ([Modifiers::META]) on macOS, Ctrl
/// ([Modifiers::CONTROL]) on Windows and Linux, as their shortcuts use them.
pub const CMD: Modifiers =
  if cfg!(target_os = "macos") { Modifiers::META } else { Modifiers::CONTROL };

/// `ctrl` in a keystroke: Control on macOS; the Windows or Super key elsewhere, Ctrl being [CMD].
pub const CTRL: Modifiers =
  if cfg!(target_os = "macos") { Modifiers::CONTROL } else { Modifiers::META };

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

/// Named keys as a keystroke writes them, and as macOS menus (glyphs) and Windows menus show
/// them.
#[rustfmt::skip]
const NAMES: [(NamedKey, &str, &str, &str); 13] = [
  (NamedKey::Enter, "enter", "↩", "Enter"),
  (NamedKey::Tab, "tab", "⇥", "Tab"),
  (NamedKey::Backspace, "backspace", "⌫", "Backspace"),
  (NamedKey::Delete, "delete", "⌦", "Del"),
  (NamedKey::Escape, "escape", "⎋", "Esc"),
  (NamedKey::ArrowLeft, "left", "←", "Left"),
  (NamedKey::ArrowRight, "right", "→", "Right"),
  (NamedKey::ArrowUp, "up", "↑", "Up"),
  (NamedKey::ArrowDown, "down", "↓", "Down"),
  (NamedKey::Home, "home", "↖", "Home"),
  (NamedKey::End, "end", "↘", "End"),
  (NamedKey::PageUp, "pageup", "⇞", "PgUp"),
  (NamedKey::PageDown, "pagedown", "⇟", "PgDn"),
];

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

  /// As menus show the key: macOS's glyphs (`↩`, `⎋`, `←`), or with `windows` its names
  /// (`Enter`, `Esc`, `Left`); a character in capitals, `Space`, `F5`, W3C's name for the rest.
  pub fn label(self, windows: bool) -> String {
    match self {
      Key::Char(' ') => "Space".into(),
      Key::Char(c) => c.to_uppercase().collect(),
      Key::Named(k) => match NAMES.iter().find(|n| n.0 == k) {
        Some(&(_, _, mac, win)) => (if windows { win } else { mac }).into(),
        None => k.to_string(),
      },
    }
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
  /// Whether this is the keystroke `s` ("cmd-shift-z", "escape"); see [Keystroke].
  pub fn is(&self, s: &str) -> bool {
    Keystroke::parse(s).is_some_and(|k| k.matches(self))
  }
}

// MARK: Keystrokes and bindings

/// A key with modifiers, written as in Zed and Sublime: modifiers first, joined by `-`:
/// `"cmd-z"`, `"cmd-shift-z"`, `"alt-left"`, `"ctrl-a"`, `"escape"`, `"f5"`, `"cmd--"`,
/// `"space"`. Modifiers: `cmd` ([CMD]), `ctrl` ([CTRL]), `alt` (or `opt`), `shift`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keystroke {
  pub key: Key,
  pub mods: Modifiers,
}

impl Keystroke {
  pub fn parse(s: &str) -> Option<Self> {
    let mut mods = Modifiers::empty();
    let mut rest = s;
    loop {
      let (name, tail) = match rest.split_once('-') {
        Some((name, tail)) if !tail.is_empty() => (name, tail),
        _ => break,
      };

      mods |= match name {
        "cmd" => CMD,
        "ctrl" => CTRL,
        "alt" | "opt" => Modifiers::ALT,
        "shift" => Modifiers::SHIFT,
        _ => return None,
      };

      rest = tail;
    }

    let f = rest.strip_prefix('f').and_then(|n| n.parse().ok()).and_then(f_key);
    let key = match rest {
      "return" => Key::Named(NamedKey::Enter),
      "esc" => Key::Named(NamedKey::Escape),
      "space" => Key::Char(' '),
      _ if let Some(n) = NAMES.iter().find(|n| n.1 == rest) => Key::Named(n.0),
      _ if let Some((_, k)) = f => Key::Named(k),
      _ => {
        let mut cs = rest.chars();
        let (Some(c), None) = (cs.next(), cs.next()) else { return None };
        Key::Char(c)
      }
    };

    Some(Self { key, mods })
  }

  /// Letters match in either case, Shift given by the modifiers: `"cmd-shift-z"` is Cmd-Shift-Z.
  /// Any other character matches itself, Shift or not, since the layout may need Shift to type
  /// it: `"cmd-+"` is Cmd-Shift-= on a US layout. Named keys need the exact modifiers.
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
}

/// As macOS menus show it: `⌃⌥⇧⌘` in that order, then the key: `⇧⌘Z`, `⌘=`, `⎋`, `F5`. On
/// Windows as its menus do, Cmd being Ctrl there: `Ctrl+Shift+Z`, `Ctrl+=`, `Esc`, `F5`.
impl std::fmt::Display for Keystroke {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    let windows = cfg!(target_os = "windows");
    let (alt, shift) = (Modifiers::ALT, Modifiers::SHIFT);
    let mods = match windows {
      true => [(CTRL, "Win+"), (CMD, "Ctrl+"), (alt, "Alt+"), (shift, "Shift+")],
      false => [(CTRL, "⌃"), (alt, "⌥"), (shift, "⇧"), (CMD, "⌘")],
    };

    for (m, name) in mods {
      if self.mods.contains(m) {
        f.write_str(name)?;
      }
    }

    f.write_str(&self.key.label(windows))
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
  use NamedKey as N;
  let (shift, ctrl, alt, cmd) = flags(k.mods);
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

  // Ctrl with an arrow is Mission Control's; with Option or Cmd, nothing.
  match (key, ctrl, alt, cmd) {
    (N::ArrowLeft, false, false, false) => to(Left),
    (N::ArrowLeft, false, true, false) => to(WordLeft),
    (N::ArrowLeft, false, false, true) => to(LineStart),
    (N::ArrowRight, false, false, false) => to(Right),
    (N::ArrowRight, false, true, false) => to(WordRight),
    (N::ArrowRight, false, false, true) => to(LineEnd),
    (N::ArrowUp, false, false, false) => to(Up),
    (N::ArrowUp, false, true, false) => to(ParagraphStart),
    (N::ArrowUp, false, false, true) => to(DocStart),
    (N::ArrowDown, false, false, false) => to(Down),
    (N::ArrowDown, false, true, false) => to(ParagraphEnd),
    (N::ArrowDown, false, false, true) => to(DocEnd),
    (N::Home, false, false, false) if shift => to(DocStart),
    (N::Home, false, false, false) => Some(Command::ScrollToTop),
    (N::End, false, false, false) if shift => to(DocEnd),
    (N::End, false, false, false) => Some(Command::ScrollToBottom),
    (N::PageUp, false, false, false) if shift => to(PageUp),
    (N::PageUp, false, false, false) => Some(Command::ScrollPageUp),
    (N::PageDown, false, false, false) if shift => to(PageDown),
    (N::PageDown, false, false, false) => Some(Command::ScrollPageDown),
    (N::Backspace, _, false, false) => del(Delete::Backward),
    (N::Backspace, false, true, false) => del(Delete::WordBackward),
    (N::Backspace, false, false, true) => del(Delete::ToLineStart),
    (N::Delete, _, false, false) => del(Delete::Forward),
    (N::Delete, false, true, false) => del(Delete::WordForward),
    (N::Enter, _, _, false) => Some(Command::InsertNewline),
    (N::Tab, false, false, false) if shift => Some(Command::InsertBacktab),
    (N::Tab, false, false, false) => Some(Command::InsertTab),
    (N::Escape, false, false, false) => Some(Command::Cancel),
    _ => None,
  }
}

/// The modifiers as keystrokes name them: Shift, [CTRL], Alt, [CMD].
pub(crate) fn flags(m: Modifiers) -> (bool, bool, bool, bool) {
  (m.shift(), m.contains(CTRL), m.alt(), m.contains(CMD))
}

/// The command a key that types nothing runs on Linux and Windows, as their text fields do
/// (GTK, Win32 edit controls): the window gets it from here where AppKit's bindings would be.
/// Ctrl is the command modifier there and arrives as `cmd`: Ctrl-Left a word, Home and End the
/// line, Ctrl-Home the document, Ctrl-Backspace a word.
pub fn pc(k: &KeyEvent) -> Option<Command> {
  use Move::*;
  use NamedKey as N;
  let (shift, ctrl, alt, cmd) = flags(k.mods);
  let to = |m| Some(Command::Move(m, shift));
  let del = |d| Some(Command::Delete(d));
  let Key::Named(key) = k.key else { return None };
  if alt || ctrl {
    return None;
  }

  match (key, cmd) {
    (N::ArrowLeft, false) => to(Left),
    (N::ArrowLeft, true) => to(WordLeft),
    (N::ArrowRight, false) => to(Right),
    (N::ArrowRight, true) => to(WordRight),
    (N::ArrowUp, false) => to(Up),
    (N::ArrowUp, true) => to(ParagraphStart),
    (N::ArrowDown, false) => to(Down),
    (N::ArrowDown, true) => to(ParagraphEnd),
    (N::Home, false) => to(LineStart),
    (N::Home, true) => to(DocStart),
    (N::End, false) => to(LineEnd),
    (N::End, true) => to(DocEnd),
    (N::PageUp, false) => to(PageUp),
    (N::PageDown, false) => to(PageDown),
    (N::Backspace, false) => del(Delete::Backward),
    (N::Backspace, true) => del(Delete::WordBackward),
    (N::Delete, false) => del(Delete::Forward),
    (N::Delete, true) => del(Delete::WordForward),
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

#[cfg(test)]
mod tests {
  use NamedKey as N;

  use super::*;

  fn ev(key: Key, mods: Modifiers) -> KeyEvent {
    KeyEvent { key, text: None, code: Code::Unidentified, mods, repeat: false }
  }

  const CMD_SHIFT: Modifiers = CMD.union(Modifiers::SHIFT);

  #[test]
  fn parses_keystrokes() {
    let k = |key, mods| Some(Keystroke { key, mods });
    assert_eq!(Keystroke::parse("cmd-z"), k(Key::Char('z'), CMD));
    assert_eq!(Keystroke::parse("cmd-shift-z"), k(Key::Char('z'), CMD_SHIFT));
    assert_eq!(Keystroke::parse("cmd--"), k(Key::Char('-'), CMD));
    assert_eq!(Keystroke::parse("-"), k(Key::Char('-'), Modifiers::empty()));
    assert_eq!(Keystroke::parse("alt-left"), k(Key::Named(N::ArrowLeft), Modifiers::ALT));
    assert_eq!(Keystroke::parse("escape"), k(Key::Named(N::Escape), Modifiers::empty()));
    assert_eq!(Keystroke::parse("f12"), k(Key::Named(N::F12), Modifiers::empty()));
    assert_eq!(Keystroke::parse("space"), k(Key::Char(' '), Modifiers::empty()));
    for bad in ["", "cmd-", "hyper-a", "ab", "cmd-ab"] {
      assert_eq!(Keystroke::parse(bad), None, "{bad}");
    }
  }

  #[test]
  fn matches_letters_in_either_case_and_symbols_with_shift() {
    // Cmd-Shift-Z reports 'Z'.
    assert!(ev(Key::Char('Z'), CMD_SHIFT).is("cmd-shift-z"));
    assert!(!ev(Key::Char('Z'), CMD_SHIFT).is("cmd-z"));
    assert!(ev(Key::Char('z'), CMD).is("cmd-z"));
    // Cmd-Shift-= types '+' on a US layout.
    assert!(ev(Key::Char('+'), CMD_SHIFT).is("cmd-+"));
    assert!(!ev(Key::Char('+'), CMD_SHIFT).is("cmd-="));
    assert!(ev(Key::Char('='), CMD).is("cmd-="));
    assert!(!ev(Key::Named(N::ArrowLeft), Modifiers::SHIFT).is("left"));
    let map = [bind("cmd-z", Command::Undo), bind("cmd-shift-z", Command::Redo)];
    assert_eq!(lookup(&map, &ev(Key::Char('Z'), CMD_SHIFT)), Some(Command::Redo));
    assert_eq!(lookup(&map, &ev(Key::Char('y'), CMD)), None);
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
    let labels = |windows| NAMES.map(|(k, ..)| Key::Named(k).label(windows)).join(" ");
    assert_eq!(labels(false), "↩ ⇥ ⌫ ⌦ ⎋ ← → ↑ ↓ ↖ ↘ ⇞ ⇟");
    assert_eq!(labels(true), "Enter Tab Backspace Del Esc Left Right Up Down Home End PgUp PgDn");
    let other = [Key::Char(' '), Key::Char('ß'), Key::Named(N::F5), Key::Named(N::Insert)];
    assert_eq!(other.map(|k| k.label(true)), ["Space", "SS", "F5", "Insert"]);
  }

  /// Keystrokes parse to the same keys as before `keyboard-types`, and print the same.
  #[test]
  fn keystroke_names() {
    for (s, key) in [
      ("enter", N::Enter),
      ("return", N::Enter),
      ("tab", N::Tab),
      ("backspace", N::Backspace),
      ("delete", N::Delete),
      ("escape", N::Escape),
      ("esc", N::Escape),
      ("left", N::ArrowLeft),
      ("right", N::ArrowRight),
      ("up", N::ArrowUp),
      ("down", N::ArrowDown),
      ("home", N::Home),
      ("end", N::End),
      ("pageup", N::PageUp),
      ("pagedown", N::PageDown),
      ("f1", N::F1),
      ("f20", N::F20),
    ] {
      assert_eq!(Keystroke::parse(s).map(|k| k.key), Some(Key::Named(key)), "{s}");
    }

    assert_eq!(Keystroke::parse("f21").map(|k| k.key), None);
    assert_eq!(Keystroke::parse("f").map(|k| k.key), Some(Key::Char('f')));
    let ctrl = Keystroke::parse("ctrl-cmd-alt-shift-a").unwrap().mods;
    assert_eq!(ctrl, Modifiers::CONTROL | Modifiers::META | Modifiers::ALT | Modifiers::SHIFT);
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
    let alt = Modifiers::ALT;
    let ctrl = CTRL;
    let alt_shift = Modifiers::ALT | Modifiers::SHIFT;
    // StandardKeyBinding.dict: key → selector.
    for (key, mods, selector) in [
      (Key::Named(N::ArrowLeft), Modifiers::empty(), "moveLeft:"),
      (Key::Named(N::ArrowLeft), alt_shift, "moveWordLeftAndModifySelection:"),
      (Key::Named(N::ArrowRight), CMD, "moveToRightEndOfLine:"),
      (Key::Named(N::ArrowUp), CMD, "moveToBeginningOfDocument:"),
      (Key::Named(N::Backspace), alt, "deleteWordBackward:"),
      (Key::Named(N::Backspace), CMD, "deleteToBeginningOfLine:"),
      (Key::Char('a'), ctrl, "moveToBeginningOfParagraph:"),
      (Key::Char('k'), ctrl, "deleteToEndOfParagraph:"),
      (Key::Named(N::Tab), Modifiers::SHIFT, "insertBacktab:"),
      (Key::Named(N::Escape), Modifiers::empty(), "cancelOperation:"),
      (Key::Named(N::Home), Modifiers::empty(), "scrollToBeginningOfDocument:"),
    ] {
      assert_eq!(standard(&ev(key, mods)), Command::from_selector(selector), "{selector}");
    }
    assert_eq!(standard(&ev(Key::Char('c'), CMD)), None);
    assert_eq!(standard(&ev(Key::Named(N::ArrowLeft), ctrl)), None);
  }

  #[test]
  fn pc_bindings_use_ctrl_as_cmd() {
    let cmd_shift = CMD | Modifiers::SHIFT;
    let alt = Modifiers::ALT;
    for (key, mods, want) in [
      (Key::Named(N::ArrowLeft), CMD, Command::Move(Move::WordLeft, false)),
      (Key::Named(N::Home), Modifiers::empty(), Command::Move(Move::LineStart, false)),
      (Key::Named(N::End), cmd_shift, Command::Move(Move::DocEnd, true)),
      (Key::Named(N::Backspace), CMD, Command::Delete(Delete::WordBackward)),
      (Key::Named(N::Delete), Modifiers::empty(), Command::Delete(Delete::Forward)),
      (Key::Named(N::Tab), Modifiers::SHIFT, Command::InsertBacktab),
    ] {
      assert_eq!(pc(&ev(key, mods)), Some(want), "{key:?} {mods:?}");
    }
    assert_eq!(pc(&ev(Key::Named(N::ArrowLeft), alt)), None);
    assert_eq!(pc(&ev(Key::Char('a'), CMD)), None);
  }

  #[test]
  fn printable_text() {
    assert!(printable("a") && printable("é") && printable("👍🏽"));
    assert!(!printable("") && !printable("\u{1}") && !printable("\u{f700}") && !printable("\t"));
  }
}
