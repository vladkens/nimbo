// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The hotkeys' fallback: Carbon's `RegisterEventHotKey` (HIToolbox, declared by hand), which
//! works without Accessibility access and under Secure Input, chords only (one registration per
//! slot, id `1 + which`, each refused alone), and takes Escape alone while that is on: a key
//! registered here never reaches the app in front. No keys while held: they need the tap. Not ⇧⎋:
//! the tap passes it while a Shift hotkey is held within its chord window, which Carbon can't tell.
//! Its handler runs on the main thread and sends the edge, timed when the key moved, to the hotkey
//! thread.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender as Sender;
use std::time::{Duration, Instant};

use super::{DEDUPE, Hit, KEY_ESCAPE, Raw, Shared, Source};
use crate::hotkey::MAX;

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
/// Slot `which`'s id is `1 + which`, up to this; Escape's is apart.
const SLOTS: u32 = MAX as u32;
const ESCAPE_ID: u32 = 0x100;

/// `RegisterEventHotKey` (code, Carbon's modifiers, id) and `UnregisterEventHotKey`: Carbon's
/// own, or a test's fakes.
type Calls = (fn(u16, u32, u32) -> Result<EventHotKeyRef, String>, fn(EventHotKeyRef));

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
  /// Each slot's registration, and Escape's.
  pub(super) chords: [Option<EventHotKeyRef>; MAX],
  pub(super) escape: Option<EventHotKeyRef>,
  state: &'static State,
  calls: Calls,
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
    let (chords, calls) = ([None; MAX], (register as _, release as _));
    Self { installed: status == 0, chords, escape: None, state, calls }
  }

  /// Registers virtual key `code` with Carbon's `modifiers` as slot `which`, unregistered before.
  pub fn register(&mut self, which: usize, code: u16, modifiers: u32) -> Result<(), String> {
    self.chords[which] = Some(self.hot_key(code, modifiers, 1 + which as u32)?);
    Ok(())
  }

  fn hot_key(&self, code: u16, modifiers: u32, id: u32) -> Result<EventHotKeyRef, String> {
    if !self.installed {
      return Err("no Carbon event handler".into());
    }

    (self.calls.0)(code, modifiers, id)
  }

  pub fn unregister(&mut self, which: usize) {
    self.chords[which].take().into_iter().for_each(self.calls.1);
  }

  /// Escape alone registered while `on`; the error is why it is not.
  pub fn set_escape(&mut self, on: bool) -> Result<(), String> {
    if !on {
      self.escape.take().into_iter().for_each(self.calls.1);
    } else if self.escape.is_none() {
      // A release missed while it was off is not waited for.
      self.state.escape_down.store(false, Ordering::Relaxed);
      self.escape = Some(self.hot_key(KEY_ESCAPE, 0, ESCAPE_ID)?);
    }

    Ok(())
  }
}

/// Carbon's `RegisterEventHotKey` for the application target.
fn register(code: u16, modifiers: u32, id: u32) -> Result<EventHotKeyRef, String> {
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

fn release(hot_key: EventHotKeyRef) {
  // SAFETY: a ref RegisterEventHotKey returned and that was not unregistered yet.
  unsafe { UnregisterEventHotKey(hot_key) };
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
  let Some(edge) = hit(id.id, kind, &state.escape_down) else { return 0 };
  // When the key moved, not when the main thread got to the event: it may have been busy, and the
  // dedupe compares the two sources' times.
  let ago = Duration::try_from_secs_f64(now - happened).unwrap_or_default().min(DEDUPE * 20);
  let at = Instant::now().checked_sub(ago).unwrap_or_else(Instant::now);
  // A press carries its slot's hotkey: the hotkey thread drops it if the slot changes before it.
  let chord = match edge {
    Hit::Down { which, .. } => state.shared.chords[which].load(Ordering::SeqCst),
    _ => 0,
  };

  state.shared.send(&state.events, Raw { source: Source::Carbon, edge, at, chord });
  0
}

/// The edge of Carbon's event `kind` of hotkey `id`: slot `id - 1`'s press or release, or
/// Escape's press, told once while it is held (Carbon repeats it).
fn hit(id: u32, kind: u32, escape: &AtomicBool) -> Option<Hit> {
  let which = (id as usize).wrapping_sub(1);
  match (id, kind) {
    (1..=SLOTS, K_EVENT_HOT_KEY_PRESSED) => Some(Hit::Down { which, repeat: false }),
    (1..=SLOTS, K_EVENT_HOT_KEY_RELEASED) => Some(Hit::Up { which }),
    (ESCAPE_ID, K_EVENT_HOT_KEY_PRESSED) => {
      (!escape.swap(true, Ordering::Relaxed)).then_some(Hit::Escape)
    }
    (ESCAPE_ID, _) => {
      escape.store(false, Ordering::Relaxed);
      None
    }
    _ => None,
  }
}

#[cfg(test)]
pub(super) mod tests {
  use super::*;

  #[test]
  fn ids_map_to_slots() {
    let escape = AtomicBool::new(false);
    let (press, release) = (K_EVENT_HOT_KEY_PRESSED, K_EVENT_HOT_KEY_RELEASED);
    assert_eq!(hit(1, press, &escape), Some(Hit::Down { which: 0, repeat: false }));
    assert_eq!(hit(8, release, &escape), Some(Hit::Up { which: 7 }));
    assert_eq!((hit(0, press, &escape), hit(9, press, &escape)), (None, None), "no such slot");
    // Escape: told once while held, as Carbon repeats it.
    assert_eq!(hit(ESCAPE_ID, press, &escape), Some(Hit::Escape));
    assert_eq!(hit(ESCAPE_ID, press, &escape), None);
    assert_eq!(hit(ESCAPE_ID, release, &escape), None);
    assert_eq!(hit(ESCAPE_ID, press, &escape), Some(Hit::Escape));
  }

  thread_local! {
    /// The fake registrations alive, by id.
    static LIVE: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
  }

  fn fake_register(_: u16, _: u32, id: u32) -> Result<EventHotKeyRef, String> {
    if id == 3 {
      return Err("another app has registered this hotkey".into());
    }

    LIVE.with_borrow_mut(|l| l.push(id));
    Ok(id as usize as EventHotKeyRef)
  }

  fn fake_release(hot_key: EventHotKeyRef) {
    LIVE.with_borrow_mut(|l| l.retain(|&id| id as usize != hot_key as usize));
  }

  /// A Carbon with fake calls: no handler installed, nothing registered with the system.
  pub(in super::super) fn fake(shared: Arc<Shared>, events: Sender<Raw>) -> Carbon {
    let state = Box::leak(Box::new(State { shared, events, escape_down: AtomicBool::new(false) }));
    let calls: Calls = (fake_register, fake_release);
    Carbon { installed: true, chords: [None; MAX], escape: None, state, calls }
  }

  pub(in super::super) fn live() -> Vec<u32> {
    LIVE.with_borrow(Clone::clone)
  }
}
