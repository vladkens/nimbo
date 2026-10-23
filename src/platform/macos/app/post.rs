// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Shortcuts posted as key events on macOS: a key down and up from a private event source, the
//! modifiers as the events' flags (no modifier key goes down, so keys the user still holds stay
//! theirs), at the HID level, where the app in focus gets them as typed. Needs Accessibility.

use keyboard_types::Modifiers;
use objc2_core_graphics::{
  CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
};

use super::super::keyboard;
use crate::keys::{Key, Shortcut, lower};

pub fn post(s: Shortcut) -> Result<(), String> {
  if !super::permissions::accessibility(false) {
    return Err("no Accessibility access: keys can't be posted".into());
  }

  let cmd = s.mods.contains(Modifiers::META);
  let (key, shift) = match s.key {
    Key::Char(c) => keyboard::key_for(lower(c), cmd).ok_or(format!("no key types {c:?}"))?,
    Key::Named(_) => (s.key.code().and_then(keyboard::kvk).ok_or("no such key")?, false),
  };

  let source = CGEventSource::new(CGEventSourceStateID::Private).ok_or("no event source")?;
  // Keys the user types right after are not held back.
  CGEventSource::set_local_events_suppression_interval(Some(&source), 0.0);
  let mut flags = CGEventFlags::empty();
  for (m, f) in [
    (Modifiers::SHIFT, CGEventFlags::MaskShift),
    (Modifiers::CONTROL, CGEventFlags::MaskControl),
    (Modifiers::ALT, CGEventFlags::MaskAlternate),
    (Modifiers::META, CGEventFlags::MaskCommand),
  ] {
    if s.mods.contains(m) || (m == Modifiers::SHIFT && shift) {
      flags |= f;
    }
  }

  let events = [true, false].map(|down| CGEvent::new_keyboard_event(Some(&source), key, down));
  let [Some(down), Some(up)] = events else { return Err("no key events".into()) };
  for e in [&down, &up] {
    CGEvent::set_flags(Some(e), flags);
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(e));
  }

  Ok(())
}
