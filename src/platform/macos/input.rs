// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! NSEvent and text-input values converted for the view ([super::view]): ranges, modifier
//! keys, scroll phases, key events, cursors and the buttons held.

use std::ops::Range;

use keyboard_types::{Modifiers, NamedKey};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSCursor, NSEvent, NSEventModifierFlags, NSEventPhase};
use objc2_foundation::{NSAttributedString, NSNotFound, NSRange, NSRangePointer, NSString};

use super::keyboard;
use crate::app::{MouseButton, Phase};
use crate::keys::{self, Key, KeyEvent};
use crate::scene::{Point, Size};
use crate::ui::Cursor;

pub const NOT_FOUND: NSRange = NSRange::new(NSNotFound as usize, 0);

pub fn to_range(r: NSRange) -> Option<Range<usize>> {
  (r.location != NSNotFound as usize).then(|| r.location..r.location + r.length)
}

pub fn ns_range(r: Range<usize>) -> NSRange {
  NSRange::new(r.start, r.end - r.start)
}

/// Writes `r` where AppKit asked for the range it got, unless it passed null.
pub fn write_range(to: NSRangePointer, r: Range<usize>) {
  if !to.is_null() {
    // SAFETY: AppKit passes a valid pointer or null.
    unsafe { to.write(ns_range(r)) };
  }
}

/// The string of an NSString or NSAttributedString, as text input passes either.
pub fn ns_text(text: &AnyObject) -> Option<String> {
  match (text.downcast_ref::<NSAttributedString>(), text.downcast_ref::<NSString>()) {
    (Some(a), _) => Some(a.string().to_string()),
    (None, Some(s)) => Some(s.to_string()),
    (None, None) => None,
  }
}

pub fn mods(event: &NSEvent) -> Modifiers {
  flags_mods(event.modifierFlags())
}

pub fn flags_mods(f: NSEventModifierFlags) -> Modifiers {
  let flags = [
    (NSEventModifierFlags::Shift, Modifiers::SHIFT),
    (NSEventModifierFlags::Control, Modifiers::CONTROL),
    (NSEventModifierFlags::Option, Modifiers::ALT),
    (NSEventModifierFlags::Command, Modifiers::META),
  ];
  flags.into_iter().filter(|(ns, _)| f.contains(*ns)).fold(Modifiers::empty(), |m, (_, k)| m | k)
}

pub fn phase(p: NSEventPhase) -> Phase {
  match p {
    NSEventPhase::MayBegin => Phase::MayBegin,
    NSEventPhase::Began => Phase::Began,
    NSEventPhase::Changed | NSEventPhase::Stationary => Phase::Changed,
    NSEventPhase::Ended => Phase::Ended,
    NSEventPhase::Cancelled => Phase::Cancelled,
    _ => Phase::None,
  }
}

/// A key event: named keys by place, the rest by the character they type. With Cmd
/// or Ctrl a non-Latin character is the ASCII-capable layout's, so shortcuts work on a Russian
/// layout as AppKit's menus do; with Cmd on a layout that maps keys anew under it ("Dvorak –
/// QWERTY ⌘"), that mapping's.
pub fn key_event(event: &NSEvent) -> KeyEvent {
  let (kvk, mods) = (event.keyCode(), mods(event));
  let (code, shortcut) =
    (keyboard::code(kvk), mods.intersects(Modifiers::META | Modifiers::CONTROL));
  let first = |s: Option<Retained<NSString>>| s.and_then(|s| s.to_string().chars().next());
  let cmd = mods.meta().then(|| keyboard::cmd_char(kvk, mods.shift())).flatten();
  let key = Key::named(code).unwrap_or_else(|| match first(event.charactersIgnoringModifiers()) {
    Some(_) if let Some(c) = cmd => Key::Char(c),
    Some(c) if shortcut && !c.is_ascii() => {
      keyboard::ascii_char(kvk, mods.shift()).map_or(Key::Char(c), Key::Char)
    }
    Some(c) if !c.is_control() => Key::Char(c),
    _ => Key::Named(NamedKey::Unidentified),
  });
  let text = match key {
    Key::Char(_) if !shortcut => {
      let s = event.characters().map(|s| s.to_string()).unwrap_or_default();
      let mut cs = s.chars();
      match (cs.next(), cs.next()) {
        (Some(c), None) if keys::printable(&s) => Some(c),
        _ => None,
      }
    }
    _ => None,
  };

  KeyEvent { key, text, code, mods, repeat: event.isARepeat() }
}

/// The cursor to set, going from the one shown in the view to the one wanted there (none: the
/// pointer is outside).
pub fn cursor_change(shown: Option<Cursor>, want: Option<Cursor>) -> Option<Cursor> {
  want.filter(|_| want != shown)
}

/// The system cursor for ours.
pub fn ns_cursor(c: Cursor) -> Retained<NSCursor> {
  match c {
    Cursor::Inherit | Cursor::Arrow => NSCursor::arrowCursor(),
    Cursor::Text => NSCursor::IBeamCursor(),
    Cursor::Pointer => NSCursor::pointingHandCursor(),
    // `columnResizeCursor` and `rowResizeCursor` replace these from macOS 15 only.
    #[allow(deprecated)]
    Cursor::ResizeX => NSCursor::resizeLeftRightCursor(),
    #[allow(deprecated)]
    Cursor::ResizeY => NSCursor::resizeUpDownCursor(),
    Cursor::Crosshair => NSCursor::crosshairCursor(),
    Cursor::Grab => NSCursor::openHandCursor(),
    Cursor::Grabbing => NSCursor::closedHandCursor(),
    Cursor::NotAllowed => NSCursor::operationNotAllowedCursor(),
  }
}

/// AppKit's button `number` (`buttonNumber`, a bit of `pressedMouseButtons`).
pub fn button(number: isize) -> MouseButton {
  match number {
    0 => MouseButton::Left,
    1 => MouseButton::Right,
    2 => MouseButton::Middle,
    n => MouseButton::Other(n as u8),
  }
}

/// The buttons `held` after button `number` went `down` or up; a number past the mask's bits
/// counts as none.
pub fn buttons(held: u64, number: isize, down: bool) -> u64 {
  let bit = u32::try_from(number).ok().and_then(|n| 1u64.checked_shl(n)).unwrap_or(0);
  if down { held | bit } else { held & !bit }
}

/// Whether a release at `pos` ends a drag outside the view of `size`: no button `held` any
/// more, and the pointer off it.
pub fn released_outside(pos: Point, size: Size, held: u64) -> bool {
  let inside = (0.0..size.w).contains(&pos.x) && (0.0..size.h).contains(&pos.y);
  held == 0 && !inside
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The last button let go outside the view takes the pointer out; one let go inside, or with
  /// another still held, doesn't.
  #[test]
  fn a_drag_released_outside_exits() {
    let size = Size::new(100.0, 80.0);
    assert!(released_outside(Point::new(120.0, 40.0), size, 0));
    assert!(released_outside(Point::new(50.0, -1.0), size, 0));
    assert!(!released_outside(Point::new(50.0, 40.0), size, 0));
    assert!(!released_outside(Point::new(120.0, 40.0), size, 0b10));
  }

  /// The buttons held are the view's own presses: the left one let go is the last even while
  /// the right one is down in another window, the right one pressed here holds the exit back,
  /// and a button number past the mask is ignored.
  #[test]
  fn held_buttons_are_the_views_presses() {
    let left = buttons(0, 0, true);
    assert_eq!(buttons(left, 0, false), 0);
    let both = buttons(left, 1, true);
    assert_eq!(buttons(both, 0, false), 0b10);
    assert_eq!(buttons(both, 70, true), both);
    assert_eq!(buttons(both, -1, false), both);
  }

  /// A crosshair asked for while the pointer is outside: set as it comes in, not again while it
  /// stays, and again after it left and came back.
  #[test]
  fn a_cursor_asked_for_outside_shows_as_the_pointer_enters() {
    let cross = Some(Cursor::Crosshair);
    assert_eq!(cursor_change(None, None), None);
    assert_eq!(cursor_change(None, cross), cross);
    assert_eq!(cursor_change(cross, cross), None);
    assert_eq!(cursor_change(cross, None), None);
    assert_eq!(cursor_change(cross, Some(Cursor::Arrow)), Some(Cursor::Arrow));
  }
}
