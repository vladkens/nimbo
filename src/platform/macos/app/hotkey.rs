// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Global hotkeys on macOS: an active `CGEventTap` on its own thread (with its own run loop)
//! matches the chord and swallows its key events and, only while [Hotkeys::escape] is on, Escape.
//! A Carbon `RegisterEventHotKey` on the same chord is the fallback for Secure Input, which leaves
//! the tap deaf, and for no Accessibility access ([carbon]); while Escape is on it is registered
//! there too, which also keeps it from the app in front. Both sources feed the "hotkey" thread,
//! which drops an edge the other source reported ≤ [DEDUPE] earlier and passes the rest to the
//! app's callback.
//!
//! A hotkey that is a modifier key alone (`right_option`, `fn`) is the tap's only: Carbon has no
//! such hotkeys. The tap sees the key's own press and release (`flagsChanged`, left and right
//! told apart by the device-dependent flags) and lets them through; another key going down while
//! it is held makes it the user's chord (⌥E types é): [Hit::Abort].
//!
//! The tap sees every key press in the system, so its callback only compares the event with the
//! chord and puts it into a preallocated channel: no allocation, no lock, no I/O.

use std::ffi::c_void;
use std::ptr::{NonNull, null_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender as Sender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use keyboard_types::{Code, Modifiers};
use objc2_core_foundation::{CFMachPort, CFRetained, CFRunLoop, kCFRunLoopCommonModes};
use objc2_core_graphics::{
  CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
  CGEventTapProxy, CGEventType,
};

use super::super::keyboard::{code as key_at, device_flag};
use crate::hotkey::{Edge, Listener, OnEdge, Status};
use crate::keys::{Hotkey, is_modifier};

mod carbon;

use carbon::Carbon;

/// A key pressed this soon after a modifier hotkey went down makes it a chord of the user's (⌥E,
/// fn+F): the press is dropped. Later, keys are typing: they never cancel.
const CHORD_WINDOW: Duration = Duration::from_millis(300);

/// What the tap callback measures time from: set before the tap exists, read without a lock.
static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// An edge the other source reported this long before is the same key press.
const DEDUPE: Duration = Duration::from_millis(50);
/// Room for a burst of events between two runs of the hotkey thread; a full channel drops events
/// (the app must cope with a lost release).
const QUEUE: usize = 64;
const KEY_ESCAPE: u16 = 0x35;

/// The chord as the tap compares it: key code in the low 16 bits, the event's modifier flags
/// above, [SET] when there is one.
const SET: u64 = 1 << 63;
/// With [SET]: the key code is a modifier key's, which is the hotkey alone.
const LONE: u64 = 1 << 62;
const MODIFIERS: u64 = CGEventFlags::MaskShift.0
  | CGEventFlags::MaskControl.0
  | CGEventFlags::MaskAlternate.0
  | CGEventFlags::MaskCommand.0;
/// The modifier keys a hotkey can be alone.
const MOD_KEYS: [Code; 9] = [
  Code::ControlLeft,
  Code::ControlRight,
  Code::AltLeft,
  Code::AltRight,
  Code::ShiftLeft,
  Code::ShiftRight,
  Code::MetaLeft,
  Code::MetaRight,
  Code::Fn,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
  Tap = 0,
  Carbon = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
  Down {
    repeat: bool,
  },
  Up,
  Escape,
  /// Another key went down while the modifier key alone was held.
  Abort,
}

impl Hit {
  fn kind(self) -> usize {
    match self {
      Hit::Down { .. } => 0,
      Hit::Up => 1,
      Hit::Escape => 2,
      Hit::Abort => 3,
    }
  }
}

#[derive(Debug, Clone, Copy)]
struct Raw {
  source: Source,
  edge: Hit,
  at: Instant,
}

/// Drops an edge when the other source reported the same kind of edge ≤ [DEDUPE] earlier:
/// whichever source is first wins, so neither waits for the other.
#[derive(Debug, Default)]
struct Dedupe {
  last: [[Option<Instant>; 4]; 2],
}

impl Dedupe {
  fn accept(&mut self, source: Source, edge: Hit, at: Instant) -> bool {
    let kind = edge.kind();
    let other = self.last[1 - source as usize][kind];
    self.last[source as usize][kind] = Some(at);
    !other.is_some_and(|t| at.saturating_duration_since(t) <= DEDUPE)
  }
}

// `Shared::tap`: 0, its default, is off (not started).
const TAP_ACTIVE: u8 = 1;
const TAP_FAILED: u8 = 2;
const TAP_STARTING: u8 = 3;

/// State shared by the tap callback, the Carbon handler, the hotkey thread and the main thread.
#[derive(Default)]
struct Shared {
  chord: AtomicU64,
  /// The tap swallowed a press of the chord and owes its release.
  chord_down: AtomicBool,
  /// When a modifier alone went down, in ns of [EPOCH].
  chord_since: AtomicU64,
  /// Escape is swallowed and told only while this is set ([Hotkeys::escape]).
  escape: AtomicBool,
  /// The tap swallowed a press of Escape and owes its release.
  escape_down: AtomicBool,
  tap: AtomicU8,
  /// Why the tap failed, set before [TAP_FAILED].
  tap_error: Mutex<Option<String>>,
  tap_events: AtomicU64,
  carbon_events: AtomicU64,
  /// Presses Carbon repeated while the key was held (no release in between).
  carbon_repeats: AtomicU64,
  duplicates: AtomicU64,
  tap_reenabled: AtomicU64,
  /// Events lost to a full channel.
  overflow: AtomicU64,
}

impl Shared {
  fn send(&self, events: &Sender<Raw>, raw: Raw) {
    if events.try_send(raw).is_err() {
      self.overflow.fetch_add(1, Ordering::Relaxed);
    }
  }
}

/// The hotkey sources. Lives on the main thread (Carbon is registered there).
pub struct Hotkeys {
  shared: Arc<Shared>,
  events: Sender<Raw>,
  carbon: Carbon,
  carbon_error: Option<String>,
  /// Why Carbon did not take Escape (another app has it).
  escape_error: Option<String>,
}

impl Hotkeys {
  /// Starts the hotkey thread feeding `on` and installs the Carbon handler; the chord comes
  /// with [set](Self::set), the tap with [start](Self::start).
  pub fn new(on: OnEdge) -> Self {
    EPOCH.get_or_init(Instant::now);
    let shared = Arc::new(Shared::default());
    let (events, rx) = sync_channel(QUEUE);
    let routing = shared.clone();
    thread::Builder::new()
      .name("hotkey".into())
      .spawn(move || route(&rx, &on, &routing))
      .expect("spawn the hotkey thread");
    let carbon = Carbon::install(shared.clone(), events.clone());
    Self { shared, events, carbon, carbon_error: None, escape_error: None }
  }

  /// Matches `hotkey` from now on, none for `None`; `Err` when it has no key code here. Carbon
  /// refusing the chord (another app has it) is no error while the tap may still see it: the
  /// status's fallback error.
  pub fn set(&mut self, hotkey: Option<Hotkey>) -> Result<(), String> {
    self.shared.chord.store(0, Ordering::Release);
    self.shared.chord_down.store(false, Ordering::Release);
    self.carbon.unregister();
    self.carbon_error = None;
    let Some(hotkey) = hotkey else { return Ok(()) };
    match hotkey {
      Hotkey::Shortcut(s) => {
        let code = s.code().and_then(mac_code).ok_or_else(|| format!("{s} has no key here"))?;
        let flags = tap_flags(s.mods);
        self.shared.chord.store(SET | u64::from(code) | flags << 16, Ordering::Release);
        self.carbon_error = self.carbon.register(code, carbon::modifiers(s.mods)).err();
      }
      Hotkey::Modifier(key) => {
        let code = mac_code(key).ok_or_else(|| format!("{hotkey} has no key here"))?;
        self.shared.chord.store(SET | LONE | u64::from(code), Ordering::Release);
      }
    }

    Ok(())
  }

  /// Starts the event tap thread, unless the tap runs or is starting; again after it failed (it
  /// needs Accessibility access). Returns at once: the tap's state comes in [status](Self::status).
  pub fn start(&mut self) -> Result<(), String> {
    let tap = &self.shared.tap;
    if matches!(tap.load(Ordering::Acquire), TAP_ACTIVE | TAP_STARTING) {
      return Ok(());
    }

    tap.store(TAP_STARTING, Ordering::Release);
    let (shared, events) = (self.shared.clone(), self.events.clone());
    let spawned =
      thread::Builder::new().name("hotkey-tap".into()).spawn(move || run_tap(shared, events));
    if let Err(e) = spawned {
      let e = format!("spawn the event tap thread: {e}");
      *self.shared.tap_error.lock().unwrap() = Some(e.clone());
      tap.store(TAP_FAILED, Ordering::Release);
      return Err(e);
    }

    Ok(())
  }

  /// While on, Escape is taken: the tap swallows it, and Carbon has it registered.
  pub fn escape(&mut self, on: bool) {
    self.shared.escape.store(on, Ordering::Release);
    self.escape_error = self.carbon.set_escape(on).err();
  }

  pub fn status(&self, out: &mut Status) {
    let s = &self.shared;
    let n = |a: &AtomicU64| a.load(Ordering::Relaxed);
    out.listener = match s.tap.load(Ordering::Acquire) {
      TAP_ACTIVE => Listener::Active,
      TAP_FAILED => Listener::Failed,
      TAP_STARTING => Listener::Starting,
      _ => Listener::Off,
    };
    out.listener_error = s.tap_error.lock().unwrap().clone();
    out.fallback = self.carbon.registered();
    out.fallback_error = self.carbon_error.clone();
    out.secure_input = carbon::secure_input();
    out.counts.listener = n(&s.tap_events);
    out.counts.fallback = n(&s.carbon_events);
    out.counts.fallback_repeats = n(&s.carbon_repeats);
    out.counts.duplicates = n(&s.duplicates);
    out.counts.reenabled = n(&s.tap_reenabled);
    out.counts.overflow = n(&s.overflow);
    // Secure Input leaves the tap deaf, never Carbon.
    let tap = out.listener == Listener::Active && !out.secure_input;
    if s.escape.load(Ordering::Relaxed) && !tap && !self.carbon.escape_registered() {
      let why = self.escape_error.as_deref().unwrap_or("not registered");
      out.escape_error = Some(format!("no event tap, and Carbon's hotkey for Escape: {why}"));
    }
  }
}

impl Drop for Hotkeys {
  fn drop(&mut self) {
    _ = self.set(None);
    self.escape(false);
  }
}

/// The virtual key code (`kVK_*`) of the key at `code`.
fn mac_code(code: Code) -> Option<u16> {
  (0..128).find(|&k| key_at(k) == code && code != Code::Unidentified)
}

/// The modifier key alone at virtual key code `kvk`.
fn mod_key(kvk: u16) -> Option<Code> {
  Some(key_at(kvk)).filter(|&c| is_modifier(c))
}

/// The "hotkey" thread: dedupes and hands the edges to the app.
fn route(rx: &Receiver<Raw>, on: &OnEdge, shared: &Shared) {
  let mut dedupe = Dedupe::default();
  let mut carbon_down = false;
  for mut raw in rx {
    let counter = match raw.source {
      Source::Tap => &shared.tap_events,
      Source::Carbon => &shared.carbon_events,
    };

    counter.fetch_add(1, Ordering::Relaxed);
    if raw.source == Source::Carbon {
      match raw.edge {
        // Carbon repeats the press while the key is held: an autorepeat.
        Hit::Down { .. } if carbon_down => {
          shared.carbon_repeats.fetch_add(1, Ordering::Relaxed);
          raw.edge = Hit::Down { repeat: true };
        }
        Hit::Down { .. } => carbon_down = true,
        Hit::Up => carbon_down = false,
        Hit::Escape | Hit::Abort => {}
      }
    }

    if !dedupe.accept(raw.source, raw.edge, raw.at) {
      shared.duplicates.fetch_add(1, Ordering::Relaxed);
      continue;
    }

    on(match raw.edge {
      Hit::Down { repeat } => Edge::Down { at: raw.at, repeat },
      Hit::Up => Edge::Up { at: raw.at },
      Hit::Escape => Edge::Escape,
      Hit::Abort => Edge::Aborted,
    });
  }
}

// MARK: Event tap

/// What the tap callback works with; leaked once the tap exists: the tap thread lives as long as
/// the process.
struct TapState {
  shared: Arc<Shared>,
  events: Sender<Raw>,
  port: AtomicPtr<CFMachPort>,
}

fn run_tap(shared: Arc<Shared>, events: Sender<Raw>) {
  let state = Box::into_raw(Box::new(TapState {
    shared: shared.clone(),
    events,
    port: AtomicPtr::new(null_mut()),
  }));
  let failed = |why: &str| {
    *shared.tap_error.lock().unwrap() = Some(why.into());
    shared.tap.store(TAP_FAILED, Ordering::Release);
  };
  let mask = (1u64 << CGEventType::KeyDown.0)
    | (1u64 << CGEventType::KeyUp.0)
    | (1u64 << CGEventType::FlagsChanged.0);
  // SAFETY: the callback matches `CGEventTapCallBack`; `state` outlives the tap (below).
  let port = unsafe {
    CGEvent::tap_create(
      CGEventTapLocation::SessionEventTap,
      CGEventTapPlacement::HeadInsertEventTap,
      CGEventTapOptions::Default,
      mask,
      Some(tap_callback),
      state.cast(),
    )
  };
  let Some(port) = port else {
    // SAFETY: no tap was created, so nothing else has the pointer.
    drop(unsafe { Box::from_raw(state) });
    return failed("the event tap was not created: no Accessibility access");
  };
  let Some(source) = CFMachPort::new_run_loop_source(None, Some(&port), 0) else {
    drop(port);
    // SAFETY: the tap is released and was never in a run loop: its callback never ran.
    drop(unsafe { Box::from_raw(state) });
    return failed("no run loop source for the event tap");
  };
  // SAFETY: from here on the tap may call back with `state` for the rest of the process.
  let state: &'static TapState = unsafe { &*state };
  state.port.store(CFRetained::as_ptr(&port).as_ptr(), Ordering::Release);
  let Some(run_loop) = CFRunLoop::current() else { return failed("no run loop") };
  // SAFETY: a static of the CoreFoundation framework.
  run_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
  CGEvent::tap_enable(&port, true);
  *shared.tap_error.lock().unwrap() = None;
  shared.tap.store(TAP_ACTIVE, Ordering::Release);
  // The port and the source stay alive for good: this thread never leaves the run loop.
  CFRunLoop::run();
  drop((source, port));
}

/// Compares, and maybe sends; nothing else (see the module docs).
unsafe extern "C-unwind" fn tap_callback(
  _proxy: CGEventTapProxy,
  ty: CGEventType,
  event: NonNull<CGEvent>,
  info: *mut c_void,
) -> *mut CGEvent {
  // SAFETY: `info` is the leaked `TapState` given to `tap_create`.
  let state = unsafe { &*(info as *const TapState) };
  let pass = event.as_ptr();
  if ty == CGEventType::TapDisabledByTimeout || ty == CGEventType::TapDisabledByUserInput {
    let port = state.port.load(Ordering::Acquire);
    if !port.is_null() {
      // SAFETY: the port is alive as long as the tap thread.
      CGEvent::tap_enable(unsafe { &*port }, true);
    }

    state.shared.tap_reenabled.fetch_add(1, Ordering::Relaxed);
    return pass;
  }

  let kind = match ty {
    CGEventType::KeyDown => KeyKind::Down,
    CGEventType::KeyUp => KeyKind::Up,
    CGEventType::FlagsChanged => KeyKind::Flags,
    _ => return pass,
  };
  // SAFETY: the event is valid for the duration of the callback.
  let event = Some(unsafe { event.as_ref() });
  let code = CGEvent::integer_value_field(event, CGEventField::KeyboardEventKeycode) as u64;
  let flags = CGEvent::flags(event).0;
  let repeat = || CGEvent::integer_value_field(event, CGEventField::KeyboardEventAutorepeat) != 0;
  let send = |edge| {
    let raw = Raw { source: Source::Tap, edge, at: Instant::now() };
    state.shared.send(&state.events, raw);
  };
  let at = EPOCH.get().map_or(0, |e| e.elapsed().as_nanos() as u64);
  match judge(&state.shared, kind, code, flags, at, repeat) {
    Verdict::Pass => pass,
    Verdict::Tell(edge) => {
      send(edge);
      pass
    }
    Verdict::Swallow(edge) => {
      edge.map(send);
      null_mut()
    }
  }
}

/// What kind of key event the tap has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyKind {
  Down,
  Up,
  /// A modifier key went down or up.
  Flags,
}

/// What the tap does with a key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
  Pass,
  /// Let through, and the edge goes to the app.
  Tell(Hit),
  /// Swallowed; the edge, if any, goes to the app.
  Swallow(Option<Hit>),
}

/// The tap's decision for a key event (`code`, the event's `flags`; `repeat` reads its autorepeat
/// field, only when needed): a modifier key alone's ([lone]), the chord's, or Escape's while
/// [Shared::escape] is on. The chord is a press with exactly its modifiers. Escape is one with no
/// ⌃, ⌥ or ⌘ held (Shift is fine, and so is the modifier hotkey's own key, held past
/// [CHORD_WINDOW]); its release is swallowed also once Escape went off. Both: [owned].
fn judge(
  shared: &Shared,
  kind: KeyKind,
  code: u64,
  flags: u64,
  at: u64,
  repeat: impl Fn() -> bool,
) -> Verdict {
  let chord = shared.chord.load(Ordering::Acquire);
  let alone = chord & (SET | LONE) == SET | LONE;
  if alone && let Some(verdict) = lone(shared, chord, kind, code, flags, at, &repeat) {
    return verdict;
  }

  if kind == KeyKind::Flags {
    return Verdict::Pass;
  }

  let ours = || flags & MODIFIERS == (chord >> 16) & MODIFIERS;
  let edges = [Hit::Down { repeat: false }, Hit::Down { repeat: true }, Hit::Up].map(Some);
  if !alone
    && chord & SET != 0
    && code == chord & 0xffff
    && let Some(verdict) = owned(&shared.chord_down, kind, &repeat, ours, edges)
  {
    return verdict;
  }

  if code != u64::from(KEY_ESCAPE) {
    return Verdict::Pass;
  }

  let ours = || {
    let chords = MODIFIERS & !CGEventFlags::MaskShift.0;
    let held = alone && shared.chord_down.load(Ordering::Relaxed);
    let own = mod_key((chord & 0xffff) as u16).filter(|_| held).map_or(0, general_flag);
    flags & chords & !own == 0 && shared.escape.load(Ordering::Acquire)
  };
  let edges = [Some(Hit::Escape), None, None];
  owned(&shared.escape_down, kind, &repeat, ours, edges).unwrap_or(Verdict::Pass)
}

/// A key whose fresh press `ours` claims is ours until its release, whatever the modifiers are by
/// then: the press, its autorepeats and the release are swallowed, telling `edges`. `held`: a
/// press of it is ours now. An autorepeat of a press the app saw stays the app's; any fresh press
/// settles who owns the key, so a lost release is not owed for long. `None`: the app's.
fn owned(
  held: &AtomicBool,
  kind: KeyKind,
  repeat: &impl Fn() -> bool,
  ours: impl FnOnce() -> bool,
  [press, autorepeat, release]: [Option<Hit>; 3],
) -> Option<Verdict> {
  if kind == KeyKind::Up {
    return held.swap(false, Ordering::Relaxed).then_some(Verdict::Swallow(release));
  }

  if repeat() {
    return held.load(Ordering::Relaxed).then_some(Verdict::Swallow(autorepeat));
  }

  let ours = ours();
  held.store(ours, Ordering::Relaxed);
  ours.then_some(Verdict::Swallow(press))
}

/// The hotkey is modifier key `chord`'s code alone: its press (with no other modifier held) and
/// its release are the edges, and the events go through. Another key going down within
/// [CHORD_WINDOW] of the press, while it is held, aborts the press (⌥E, fn+F: the user's chord);
/// later keys are typing and change nothing. An event without the key's flag while it is held
/// is its release, also when the release itself was not seen (the Globe key's may come with
/// another code); Escape as that event drops the press instead. `at`: the event's time, in ns.
/// `None`: the event is none of its business.
fn lone(
  shared: &Shared,
  chord: u64,
  kind: KeyKind,
  code: u64,
  flags: u64,
  at: u64,
  repeat: &impl Fn() -> bool,
) -> Option<Verdict> {
  let key = mod_key((chord & 0xffff) as u16)?;
  let own = device_flag(key)?;
  let held = &shared.chord_down;
  let down = flags & own != 0;
  if code == chord & 0xffff && kind == KeyKind::Flags {
    // Another modifier held: of another kind, or the other side of this one.
    let device = MOD_KEYS.iter().filter_map(|&k| device_flag(k)).fold(0, |all, f| all | f);
    let others = flags & (MODIFIERS | CGEventFlags::MaskSecondaryFn.0) & !general_flag(key)
      | flags & device & !own;
    return Some(match (down, held.load(Ordering::Relaxed)) {
      (true, false) if others == 0 => {
        held.store(true, Ordering::Relaxed);
        shared.chord_since.store(at, Ordering::Relaxed);
        Verdict::Tell(Hit::Down { repeat: false })
      }
      (false, true) => {
        held.store(false, Ordering::Relaxed);
        Verdict::Tell(Hit::Up)
      }
      _ => Verdict::Pass,
    });
  }

  if !held.load(Ordering::Relaxed) {
    return None;
  }

  if !down {
    held.store(false, Ordering::Relaxed);
    let escape = code == u64::from(KEY_ESCAPE) && kind == KeyKind::Down;
    return Some(Verdict::Tell(if escape { Hit::Abort } else { Hit::Up }));
  }

  let since = shared.chord_since.load(Ordering::Relaxed);
  let chord_started = at.saturating_sub(since) < CHORD_WINDOW.as_nanos() as u64;
  let pressed = match kind {
    KeyKind::Down => !repeat(),
    KeyKind::Flags => true,
    KeyKind::Up => false,
  };

  if pressed && chord_started {
    held.store(false, Ordering::Relaxed);
    return Some(Verdict::Tell(Hit::Abort));
  }

  None
}

/// The device-independent flag of modifier key `key`, either side's.
fn general_flag(key: Code) -> u64 {
  match key {
    Code::ControlLeft | Code::ControlRight => CGEventFlags::MaskControl.0,
    Code::AltLeft | Code::AltRight => CGEventFlags::MaskAlternate.0,
    Code::ShiftLeft | Code::ShiftRight => CGEventFlags::MaskShift.0,
    Code::MetaLeft | Code::MetaRight => CGEventFlags::MaskCommand.0,
    _ => CGEventFlags::MaskSecondaryFn.0,
  }
}

/// The event tap's modifier flags for a chord's modifiers.
fn tap_flags(mods: Modifiers) -> u64 {
  let flags = [
    (Modifiers::CONTROL, CGEventFlags::MaskControl),
    (Modifiers::ALT, CGEventFlags::MaskAlternate),
    (Modifiers::SHIFT, CGEventFlags::MaskShift),
    (Modifiers::META, CGEventFlags::MaskCommand),
  ];
  flags.iter().filter(|(m, _)| mods.contains(*m)).fold(0, |all, (_, f)| all | f.0)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::keys::Shortcut;

  #[test]
  fn the_first_source_wins_and_the_other_is_dropped_within_the_window() {
    let mut d = Dedupe::default();
    let t0 = Instant::now();
    let ms = |n| t0 + Duration::from_millis(n);
    let down = Hit::Down { repeat: false };
    assert!(d.accept(Source::Tap, down, ms(0)));
    assert!(!d.accept(Source::Carbon, down, ms(3)), "Carbon right after the tap");
    assert!(d.accept(Source::Tap, Hit::Up, ms(400)));
    assert!(!d.accept(Source::Carbon, Hit::Up, ms(450)), "the window is inclusive");
    // Carbon first (the tap was late), then the tap's copy.
    assert!(d.accept(Source::Carbon, down, ms(1_000)));
    assert!(!d.accept(Source::Tap, down, ms(1_010)));
    // Out of the window both count; one source never dedupes itself (autorepeat, quick taps).
    assert!(d.accept(Source::Carbon, Hit::Up, ms(1_200)));
    assert!(d.accept(Source::Tap, Hit::Up, ms(1_251)));
    assert!(d.accept(Source::Tap, down, ms(1_260)));
    assert!(d.accept(Source::Tap, Hit::Down { repeat: true }, ms(1_270)));
    // Kinds do not mix; Escape from both is one.
    assert!(d.accept(Source::Carbon, Hit::Abort, ms(1_275)));
    assert!(d.accept(Source::Tap, Hit::Escape, ms(1_280)));
    assert!(!d.accept(Source::Carbon, Hit::Escape, ms(1_281)));
  }

  /// The hotkey thread: the tap's press, Carbon's copy of it (dropped), Carbon repeating it (an
  /// autorepeat), the release and Escape from both, as the app hears them.
  #[test]
  fn the_thread_dedupes_and_hands_edges_over() {
    let shared = Shared::default();
    let (tx, rx) = sync_channel(QUEUE);
    let t0 = Instant::now();
    let ms = |n| t0 + Duration::from_millis(n);
    let raw = |source, edge, at| Raw { source, edge, at };
    let down = Hit::Down { repeat: false };
    for r in [
      raw(Source::Tap, down, ms(0)),
      raw(Source::Carbon, down, ms(5)),
      raw(Source::Carbon, down, ms(400)),
      raw(Source::Tap, Hit::Up, ms(600)),
      raw(Source::Tap, Hit::Escape, ms(700)),
      raw(Source::Carbon, Hit::Escape, ms(710)),
    ] {
      tx.send(r).unwrap();
    }

    drop(tx);
    let (got_tx, got) = std::sync::mpsc::channel();
    let on: OnEdge = Box::new(move |e| got_tx.send(e).unwrap());
    route(&rx, &on, &shared);
    let got: Vec<Edge> = got.try_iter().collect();
    let want = [
      Edge::Down { at: ms(0), repeat: false },
      Edge::Down { at: ms(400), repeat: true },
      Edge::Up { at: ms(600) },
      Edge::Escape,
    ];
    assert_eq!(got, want);
    let n = |a: &AtomicU64| a.load(Ordering::Relaxed);
    assert_eq!((n(&shared.duplicates), n(&shared.carbon_repeats)), (2, 1));
    assert_eq!((n(&shared.tap_events), n(&shared.carbon_events)), (3, 3));
  }

  const SPACE: u64 = 0x31;
  const ESC: u64 = KEY_ESCAPE as u64;
  const E: u64 = 0x0E;
  const NONE: CGEventFlags = CGEventFlags(0);
  const OPT: CGEventFlags = CGEventFlags::MaskAlternate;
  const RIGHT_ALT: u64 = CGEventFlags::MaskAlternate.0 | 0x40;
  const LEFT_ALT: u64 = CGEventFlags::MaskAlternate.0 | 0x20;

  /// `option+space` set, as `Hotkeys::set` stores it.
  fn shared() -> Shared {
    let s = Shared::default();
    let chord = Shortcut { key: crate::Key::Char(' '), mods: Modifiers::ALT };
    s.chord.store(SET | SPACE | tap_flags(chord.mods) << 16, Ordering::Release);
    s
  }

  fn key(s: &Shared, down: bool, code: u64, flags: CGEventFlags, repeat: bool) -> Verdict {
    key_at(s, 0, down, code, flags, repeat)
  }

  /// A key event `ms` after the start.
  fn key_at(
    s: &Shared,
    ms: u64,
    down: bool,
    code: u64,
    flags: CGEventFlags,
    repeat: bool,
  ) -> Verdict {
    let kind = if down { KeyKind::Down } else { KeyKind::Up };
    judge(s, kind, code, flags.0, ms * 1_000_000, || repeat)
  }

  /// A modifier key's `flagsChanged`: `flags` as they are after it.
  fn flags(s: &Shared, key: Code, flags: u64) -> Verdict {
    flags_at(s, 0, key, flags)
  }

  fn flags_at(s: &Shared, ms: u64, key: Code, flags: u64) -> Verdict {
    let code = mac_code(key).unwrap();
    judge(s, KeyKind::Flags, u64::from(code), flags, ms * 1_000_000, || false)
  }

  /// `key` alone set, as `Hotkeys::set` stores a modifier alone.
  fn lone_shared(key: Code) -> Shared {
    let s = Shared::default();
    s.chord.store(SET | LONE | u64::from(mac_code(key).unwrap()), Ordering::Release);
    s
  }

  #[test]
  fn a_modifier_alone_fires_on_its_press_and_release_and_lets_them_through() {
    let s = lone_shared(Code::AltRight);
    let tell = Verdict::Tell;
    assert_eq!(flags(&s, Code::AltRight, RIGHT_ALT), tell(Hit::Down { repeat: false }));
    assert_eq!(flags(&s, Code::AltRight, 0), tell(Hit::Up));
    // The left one is another key; with ⌘ held it is a chord of the user's.
    assert_eq!(flags(&s, Code::AltLeft, LEFT_ALT), Verdict::Pass);
    assert_eq!(flags(&s, Code::AltLeft, 0), Verdict::Pass);
    let cmd = CGEventFlags::MaskCommand.0 | 0x08;
    assert_eq!(flags(&s, Code::AltRight, RIGHT_ALT | cmd), Verdict::Pass);
    assert_eq!(flags(&s, Code::AltRight, cmd), Verdict::Pass, "no release owed");
    // Left Option held, then Right Option: a chord of the user's too.
    assert_eq!(flags(&s, Code::AltRight, RIGHT_ALT | LEFT_ALT), Verdict::Pass);
    assert_eq!(flags(&s, Code::AltRight, LEFT_ALT), Verdict::Pass, "no release owed");
    // Keys and their releases are the apps' while it is not held.
    assert_eq!(key(&s, true, E, NONE, false), Verdict::Pass);
    assert_eq!(key(&s, false, E, NONE, false), Verdict::Pass);
  }

  #[test]
  fn another_key_while_the_modifier_is_held_aborts_it() {
    let s = lone_shared(Code::AltRight);
    let opt = CGEventFlags(RIGHT_ALT);
    assert!(matches!(flags(&s, Code::AltRight, RIGHT_ALT), Verdict::Tell(Hit::Down { .. })));
    // ⌥E: the press is aborted, E types é in the app; the release is no edge.
    assert_eq!(key(&s, true, E, opt, false), Verdict::Tell(Hit::Abort));
    assert_eq!(key(&s, false, E, opt, false), Verdict::Pass);
    assert_eq!(flags(&s, Code::AltRight, 0), Verdict::Pass);
    // Another modifier going down aborts it too.
    flags(&s, Code::AltRight, RIGHT_ALT);
    let shift = CGEventFlags::MaskShift.0 | 0x02;
    assert_eq!(flags(&s, Code::ShiftLeft, RIGHT_ALT | shift), Verdict::Tell(Hit::Abort));
  }

  #[test]
  fn fn_alone_is_a_hotkey_too() {
    let s = lone_shared(Code::Fn);
    let fn_flag = CGEventFlags::MaskSecondaryFn.0;
    assert_eq!(flags(&s, Code::Fn, fn_flag), Verdict::Tell(Hit::Down { repeat: false }));
    // An arrow key with fn held (its events carry the fn flag) right away: the user's.
    assert_eq!(key(&s, true, 0x7E, CGEventFlags(fn_flag), false), Verdict::Tell(Hit::Abort));
    assert_eq!(flags(&s, Code::Fn, 0), Verdict::Pass);
    assert_eq!(flags(&s, Code::Fn, fn_flag), Verdict::Tell(Hit::Down { repeat: false }));
    assert_eq!(flags(&s, Code::Fn, 0), Verdict::Tell(Hit::Up));
  }

  #[test]
  fn keys_after_the_chord_window_are_typing_and_never_cancel() {
    let s = lone_shared(Code::Fn);
    let fn_flag = CGEventFlags::MaskSecondaryFn.0;
    let with_fn = CGEventFlags(fn_flag);
    // A long hold while typing: nothing aborts, the release stops.
    assert_eq!(flags_at(&s, 0, Code::Fn, fn_flag), Verdict::Tell(Hit::Down { repeat: false }));
    assert_eq!(key_at(&s, 400, true, E, with_fn, false), Verdict::Pass);
    assert_eq!(key_at(&s, 450, false, E, with_fn, false), Verdict::Pass);
    let shift = CGEventFlags::MaskShift.0 | 0x02;
    assert_eq!(flags_at(&s, 500, Code::ShiftLeft, fn_flag | shift), Verdict::Pass);
    assert_eq!(flags_at(&s, 3_000, Code::Fn, 0), Verdict::Tell(Hit::Up));
    // A tap: typing after it is the app's.
    flags_at(&s, 5_000, Code::Fn, fn_flag);
    assert_eq!(flags_at(&s, 5_100, Code::Fn, 0), Verdict::Tell(Hit::Up));
    for (i, code) in [E, 0x7E, SPACE].into_iter().enumerate() {
      assert_eq!(key_at(&s, 5_150 + i as u64, true, code, NONE, false), Verdict::Pass);
    }
  }

  #[test]
  fn a_release_not_seen_is_seen_in_the_next_event() {
    let s = lone_shared(Code::Fn);
    let fn_flag = CGEventFlags::MaskSecondaryFn.0;
    flags_at(&s, 0, Code::Fn, fn_flag);
    // The Globe key's release came with another code: the next key has no fn flag.
    assert_eq!(key_at(&s, 900, true, E, NONE, false), Verdict::Tell(Hit::Up));
    assert_eq!(key_at(&s, 950, true, E, NONE, false), Verdict::Pass, "typing, no abort");
    // Escape as the next event drops the press instead.
    flags_at(&s, 2_000, Code::Fn, fn_flag);
    assert_eq!(key_at(&s, 2_900, true, ESC, NONE, false), Verdict::Tell(Hit::Abort));
    assert_eq!(key_at(&s, 3_000, true, E, NONE, false), Verdict::Pass, "released by then");
  }

  #[test]
  fn the_chord_is_swallowed_with_its_repeats_and_release() {
    let s = shared();
    let swallow = |hit| Verdict::Swallow(Some(hit));
    assert_eq!(key(&s, true, SPACE, OPT, false), swallow(Hit::Down { repeat: false }));
    assert_eq!(key(&s, true, SPACE, OPT, true), swallow(Hit::Down { repeat: true }));
    // Option let go first: the repeats and the release are still the chord's.
    assert_eq!(key(&s, true, SPACE, NONE, true), swallow(Hit::Down { repeat: true }));
    assert_eq!(key(&s, false, SPACE, NONE, false), swallow(Hit::Up));
    // Then Space is the app's again.
    assert_eq!(key(&s, true, SPACE, NONE, false), Verdict::Pass);
    assert_eq!(key(&s, false, SPACE, NONE, false), Verdict::Pass);
    // Other modifiers, other keys: not the chord.
    assert_eq!(key(&s, true, SPACE, OPT | CGEventFlags::MaskShift, false), Verdict::Pass);
    assert_eq!(key(&s, true, 0x00, OPT, false), Verdict::Pass);
  }

  #[test]
  fn a_key_the_app_saw_go_down_stays_the_apps() {
    let s = shared();
    // Space held, then Option: the autorepeats match the chord but are not a press.
    assert_eq!(key(&s, true, SPACE, NONE, false), Verdict::Pass);
    assert_eq!(key(&s, true, SPACE, OPT, true), Verdict::Pass);
    assert_eq!(key(&s, false, SPACE, OPT, false), Verdict::Pass, "its release reaches the app");
    // A release the tap never saw (tap disabled, Secure Input) is not owed past the next press.
    assert!(matches!(key(&s, true, SPACE, OPT, false), Verdict::Swallow(_)));
    assert_eq!(key(&s, true, SPACE, NONE, false), Verdict::Pass, "a plain press");
    assert_eq!(key(&s, false, SPACE, NONE, false), Verdict::Pass, "and its release");
  }

  #[test]
  fn escape_is_taken_only_while_on() {
    let s = shared();
    let taken = Verdict::Swallow(Some(Hit::Escape));
    assert_eq!(key(&s, true, ESC, NONE, false), Verdict::Pass);
    assert_eq!(key(&s, false, ESC, NONE, false), Verdict::Pass);
    s.escape.store(true, Ordering::Release);
    assert_eq!(key(&s, true, ESC, NONE, false), taken);
    assert_eq!(key(&s, true, ESC, NONE, true), Verdict::Swallow(None), "its repeats");
    s.escape.store(false, Ordering::Release); // turned off meanwhile
    assert_eq!(key(&s, false, ESC, NONE, false), Verdict::Swallow(None), "its release");
    // Held since before it was on: the app's.
    assert_eq!(key(&s, true, ESC, NONE, false), Verdict::Pass);
    s.escape.store(true, Ordering::Release);
    assert_eq!(key(&s, true, ESC, NONE, true), Verdict::Pass);
    assert_eq!(key(&s, false, ESC, NONE, false), Verdict::Pass);
    // ⌘⎋, ⌃⎋, ⌥⎋ are not Escape; ⇧⎋ is.
    for flags in [CGEventFlags::MaskCommand, CGEventFlags::MaskControl, OPT] {
      assert_eq!(key(&s, true, ESC, flags, false), Verdict::Pass, "{flags:?}");
    }

    assert_eq!(key(&s, true, ESC, CGEventFlags::MaskShift, false), taken);
    // The chord still works meanwhile.
    assert!(matches!(key(&s, true, SPACE, OPT, false), Verdict::Swallow(Some(Hit::Down { .. }))));
  }

  #[test]
  fn escape_with_a_modifier_hotkey_held() {
    let s = lone_shared(Code::AltRight);
    s.escape.store(true, Ordering::Release);
    let opt = CGEventFlags(RIGHT_ALT);
    // Right after the press: the user's ⌥⎋, which aborts the press and is the app's.
    flags_at(&s, 0, Code::AltRight, RIGHT_ALT);
    assert_eq!(key_at(&s, 100, true, ESC, opt, false), Verdict::Tell(Hit::Abort));
    assert_eq!(key_at(&s, 150, false, ESC, opt, false), Verdict::Pass);
    flags_at(&s, 200, Code::AltRight, 0);
    // Held past the chord window: Escape is taken, the hotkey's own ⌥ notwithstanding.
    flags_at(&s, 1_000, Code::AltRight, RIGHT_ALT);
    let taken = Verdict::Swallow(Some(Hit::Escape));
    assert_eq!(key_at(&s, 2_000, true, ESC, opt, false), taken);
    assert_eq!(key_at(&s, 2_050, false, ESC, opt, false), Verdict::Swallow(None));
    // ⌥⎋ with another hotkey, or with this one not held, is still the app's.
    let s = lone_shared(Code::MetaRight);
    s.escape.store(true, Ordering::Release);
    assert_eq!(key(&s, true, ESC, opt, false), Verdict::Pass);
    let s = lone_shared(Code::AltRight);
    s.escape.store(true, Ordering::Release);
    assert_eq!(key(&s, true, ESC, CGEventFlags(LEFT_ALT), false), Verdict::Pass);
    assert_eq!(key(&s, true, ESC, NONE, false), taken, "and plain Escape is taken");
  }

  #[test]
  fn hotkeys_map_to_key_codes_and_flags() {
    let Ok(Hotkey::Shortcut(s)) = Hotkey::parse("ctrl+option+cmd+F12") else { panic!() };
    assert_eq!(s.code().and_then(mac_code), Some(0x6F));
    let want = CGEventFlags::MaskControl.0 | CGEventFlags::MaskAlternate.0;
    assert_eq!(tap_flags(s.mods), want | CGEventFlags::MaskCommand.0);
    for (text, code) in
      [("a", 0x00), ("z", 0x06), ("0", 0x1D), ("9", 0x19), ("f1", 0x7A), ("f20", 0x5A)]
    {
      let Ok(Hotkey::Shortcut(s)) = Hotkey::parse(text) else { panic!("{text}") };
      assert_eq!(s.code().and_then(mac_code), Some(code), "{text}");
    }

    let lone = [("right_option", 0x3D), ("left_cmd", 0x37), ("fn", 0x3F), ("rshift", 0x3C)];
    for (text, code) in lone {
      let Ok(Hotkey::Modifier(key)) = Hotkey::parse(text) else { panic!("{text}") };
      assert_eq!(mac_code(key), Some(code), "{text}");
      assert_eq!(mod_key(code), Some(key));
    }

    assert_eq!(mod_key(0x31), None, "space is no modifier");
  }
}
