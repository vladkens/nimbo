// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Shortcuts posted as key events on macOS: a key down and up from a private event source, the
//! modifiers as the events' flags (no modifier key goes down, so keys the user still holds stay
//! theirs), at the HID level, where the app in focus gets them as typed, marked [POSTED] so the
//! app's own hotkey tap lets them pass unseen. Needs Accessibility.

use keyboard_types::Modifiers;
use objc2_core_graphics::{
  CGEvent, CGEventField, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
};

use super::super::keyboard;
use super::hotkey::tap_flags;
use crate::keys::{Key, Shortcut};

/// The posted events' source user data: the hotkey tap passes them on unjudged.
pub(super) const POSTED: i64 = u32::from_be_bytes(*b"nmbo") as i64;

pub fn post(s: Shortcut) -> Result<(), String> {
  // The layout's lookup (TIS) aborts the process when another thread calls it at once.
  if objc2::MainThreadMarker::new().is_none() {
    return Err("keys are posted on the main thread".into());
  }

  if !super::permissions::accessibility(false) {
    return Err("no Accessibility access: keys can't be posted".into());
  }

  let cmd = s.mods.contains(Modifiers::META);
  let (key, shift) = match s.key {
    Key::Char(c) => keyboard::key_for(c, cmd).ok_or(format!("no key types {c:?}"))?,
    Key::Named(_) => (s.key.code().and_then(keyboard::kvk).ok_or("no such key")?, false),
  };

  let source = CGEventSource::new(CGEventSourceStateID::Private).ok_or("no event source")?;
  // Keys the user types right after are not held back.
  CGEventSource::set_local_events_suppression_interval(Some(&source), 0.0);
  let mods = if shift { s.mods | Modifiers::SHIFT } else { s.mods };
  let flags = CGEventFlags(tap_flags(mods));

  let events = [true, false].map(|down| CGEvent::new_keyboard_event(Some(&source), key, down));
  let [Some(down), Some(up)] = events else { return Err("no key events".into()) };
  for e in [&down, &up] {
    CGEvent::set_flags(Some(e), flags);
    CGEvent::set_integer_value_field(Some(e), CGEventField::EventSourceUserData, POSTED);
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(e));
  }

  Ok(())
}
