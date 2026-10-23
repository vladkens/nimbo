// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The hotkeys' fallback: Carbon's `RegisterEventHotKey` (HIToolbox, declared by hand), which
//! works without Accessibility access and under Secure Input, chords only, and takes Escape
//! alone while that is on: a key registered here never reaches the app in front. Not ⇧⎋: the
//! tap passes it while a Shift hotkey is held within its chord window, which Carbon can't tell. Its
//! handler runs on the main thread and sends the edge, timed when the key moved, to the hotkey
//! thread.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender as Sender;
use std::time::{Duration, Instant};

use keyboard_types::Modifiers;

use super::{DEDUPE, Hit, KEY_ESCAPE, Raw, Shared, Source};

type OSStatus = i32;
type EventRef = *mut c_void;
type EventTargetRef = *mut c_void;
type EventHandlerRef = *mut c_void;
type EventHandlerCallRef = *mut c_void;
type EventHotKeyRef = *mut c_void;
type EventHandlerUPP =
  extern "C" fn(call: EventHandlerCallRef, event: EventRef, user_data: *mut c_void) -> OSStatus;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct EventHotKeyID {
  signature: u32,
  id: u32,
}

#[repr(C)]
struct EventTypeSpec {
  event_class: u32,
  event_kind: u32,
}

const K_EVENT_CLASS_KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
const K_EVENT_HOT_KEY_PRESSED: u32 = 5;
const K_EVENT_HOT_KEY_RELEASED: u32 = 6;
const K_EVENT_PARAM_DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
const TYPE_EVENT_HOT_KEY_ID: u32 = u32::from_be_bytes(*b"hkid");
const EVENT_NOT_HANDLED_ERR: OSStatus = -9874;
/// `eventHotKeyExistsErr`.
const HOT_KEY_EXISTS: OSStatus = -9878;
const CMD_KEY: u32 = 1 << 8;
const SHIFT_KEY: u32 = 1 << 9;
const OPTION_KEY: u32 = 1 << 11;
const CONTROL_KEY: u32 = 1 << 12;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
  fn GetEventDispatcherTarget() -> EventTargetRef;
  fn InstallEventHandler(
    target: EventTargetRef,
    handler: EventHandlerUPP,
    num_types: usize,
    list: *const EventTypeSpec,
    user_data: *mut c_void,
    out_ref: *mut EventHandlerRef,
  ) -> OSStatus;
  fn RegisterEventHotKey(
    key_code: u32,
    modifiers: u32,
    id: EventHotKeyID,
    target: EventTargetRef,
    options: u32,
    out_ref: *mut EventHotKeyRef,
  ) -> OSStatus;
  fn UnregisterEventHotKey(hot_key: EventHotKeyRef) -> OSStatus;
  fn GetEventParameter(
    event: EventRef,
    name: u32,
    desired_type: u32,
    actual_type: *mut u32,
    buffer_size: usize,
    actual_size: *mut usize,
    data: *mut c_void,
  ) -> OSStatus;
  fn GetEventKind(event: EventRef) -> u32;
  /// Seconds since startup, when the event happened.
  fn GetEventTime(event: EventRef) -> f64;
  fn GetCurrentEventTime() -> f64;
}

const SIGNATURE: u32 = u32::from_be_bytes(*b"nmbo");
const CHORD_ID: u32 = 1;
const ESCAPE_ID: u32 = 2;

/// What the handler works with; leaked: the handler stays installed for the process.
struct State {
  shared: Arc<Shared>,
  events: Sender<Raw>,
  /// Escape is held: Carbon repeats its press, told once.
  escape_down: AtomicBool,
}

/// This app's Carbon hotkeys; main thread only.
pub struct Carbon {
  installed: bool,
  chord: Option<EventHotKeyRef>,
  escape: Option<EventHotKeyRef>,
  state: &'static State,
}

impl Carbon {
  pub fn install(shared: Arc<Shared>, events: Sender<Raw>) -> Self {
    let state: &'static State =
      Box::leak(Box::new(State { shared, events, escape_down: AtomicBool::new(false) }));
    let kinds = [K_EVENT_HOT_KEY_PRESSED, K_EVENT_HOT_KEY_RELEASED];
    let specs =
      kinds.map(|event_kind| EventTypeSpec { event_class: K_EVENT_CLASS_KEYBOARD, event_kind });
    let mut handler = null_mut();
    // SAFETY: the handler matches `EventHandlerUPP`; `state` lives for good.
    let status = unsafe {
      InstallEventHandler(
        GetEventDispatcherTarget(),
        handler_fn,
        specs.len(),
        specs.as_ptr(),
        state as *const State as *mut c_void,
        &mut handler,
      )
    };
    Self { installed: status == 0, chord: None, escape: None, state }
  }

  /// Registers virtual key `code` with Carbon's `modifiers`, in place of the one before.
  pub fn register(&mut self, code: u16, modifiers: u32) -> Result<(), String> {
    self.unregister();
    self.chord = Some(self.hot_key(code, modifiers, CHORD_ID)?);
    Ok(())
  }

  fn hot_key(&self, code: u16, modifiers: u32, id: u32) -> Result<EventHotKeyRef, String> {
    if !self.installed {
      return Err("no Carbon event handler".into());
    }

    let mut hot_key = null_mut();
    let id = EventHotKeyID { signature: SIGNATURE, id };
    // SAFETY: plain registration with the application target; `hot_key` receives the ref.
    let status = unsafe {
      RegisterEventHotKey(code.into(), modifiers, id, GetEventDispatcherTarget(), 0, &mut hot_key)
    };
    match status {
      0 => Ok(hot_key),
      HOT_KEY_EXISTS => Err("another app has registered this hotkey".into()),
      status => Err(format!("RegisterEventHotKey failed (OSStatus {status})")),
    }
  }

  pub fn unregister(&mut self) {
    release(self.chord.take());
  }

  pub fn registered(&self) -> bool {
    self.chord.is_some()
  }

  /// Escape alone registered while `on`; the error is why it is not.
  pub fn set_escape(&mut self, on: bool) -> Result<(), String> {
    if !on {
      release(self.escape.take());
    } else if self.escape.is_none() {
      // A release missed while it was off is not waited for.
      self.state.escape_down.store(false, Ordering::Relaxed);
      self.escape = Some(self.hot_key(KEY_ESCAPE, 0, ESCAPE_ID)?);
    }

    Ok(())
  }

  pub fn escape_registered(&self) -> bool {
    self.escape.is_some()
  }
}

fn release(hot_key: Option<EventHotKeyRef>) {
  if let Some(hot_key) = hot_key {
    // SAFETY: a ref RegisterEventHotKey returned and that was not unregistered yet.
    unsafe { UnregisterEventHotKey(hot_key) };
  }
}

extern "C" fn handler_fn(
  _call: EventHandlerCallRef,
  event: EventRef,
  data: *mut c_void,
) -> OSStatus {
  // SAFETY: `data` is the leaked `State` given to `InstallEventHandler`.
  let state = unsafe { &*(data as *const State) };
  let mut id = EventHotKeyID::default();
  // SAFETY: reads the hot key ID parameter into a buffer of its size.
  let status = unsafe {
    GetEventParameter(
      event,
      K_EVENT_PARAM_DIRECT_OBJECT,
      TYPE_EVENT_HOT_KEY_ID,
      null_mut(),
      size_of::<EventHotKeyID>(),
      null_mut(),
      (&mut id as *mut EventHotKeyID).cast(),
    )
  };
  if status != 0 || id.signature != SIGNATURE {
    return EVENT_NOT_HANDLED_ERR;
  }

  // SAFETY: a valid event for the duration of the handler.
  let (kind, happened, now) =
    unsafe { (GetEventKind(event), GetEventTime(event), GetCurrentEventTime()) };
  let escape = &state.escape_down;
  let edge = match (id.id, kind) {
    (CHORD_ID, K_EVENT_HOT_KEY_PRESSED) => Hit::Down { repeat: false },
    (CHORD_ID, K_EVENT_HOT_KEY_RELEASED) => Hit::Up,
    // Carbon repeats the press while Escape is held: told once.
    (ESCAPE_ID, K_EVENT_HOT_KEY_PRESSED) if !escape.swap(true, Ordering::Relaxed) => Hit::Escape,
    (ESCAPE_ID, K_EVENT_HOT_KEY_RELEASED) => {
      escape.store(false, Ordering::Relaxed);
      return 0;
    }
    _ => return 0,
  };
  // When the key moved, not when the main thread got to the event: it may have been busy, and the
  // dedupe compares the two sources' times.
  let ago = Duration::try_from_secs_f64(now - happened).unwrap_or_default().min(DEDUPE * 20);
  let at = Instant::now().checked_sub(ago).unwrap_or_else(Instant::now);
  state.shared.send(&state.events, Raw { source: Source::Carbon, edge, at });
  0
}

/// Carbon's modifier bits for a chord's modifiers.
pub fn modifiers(mods: Modifiers) -> u32 {
  let bits = [
    (Modifiers::CONTROL, CONTROL_KEY),
    (Modifiers::ALT, OPTION_KEY),
    (Modifiers::SHIFT, SHIFT_KEY),
    (Modifiers::META, CMD_KEY),
  ];
  bits.iter().filter(|(m, _)| mods.contains(*m)).fold(0, |all, (_, b)| all | b)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn modifier_bits() {
    let all = Modifiers::CONTROL | Modifiers::ALT | Modifiers::META;
    assert_eq!(modifiers(all), CONTROL_KEY | OPTION_KEY | CMD_KEY);
    assert_eq!(modifiers(Modifiers::SHIFT), SHIFT_KEY);
  }
}
