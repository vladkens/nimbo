// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Global hotkeys on macOS: an active `CGEventTap` on its own thread (with its own run loop,
//! [tap]) matches up to [MAX] hotkeys, swallows their key events, takes the keys set while one is
//! held and, only while [Hotkeys::escape] is on, Escape. A Carbon `RegisterEventHotKey` on each
//! chord is the fallback for Secure Input, which leaves the tap deaf, and for no Accessibility
//! access ([carbon]); while Escape is on it is registered there too (alone, [carbon]), which also
//! keeps it from the app in front. Keys while held are the tap's only. Both sources feed the
//! "hotkey" thread, which drops an edge the other source reported ≤ [DEDUPE] earlier and passes the
//! rest to the app's callback.
//!
//! A hotkey that is a modifier key alone (`right_option`, `fn`) is the tap's only: Carbon has no
//! such hotkeys. The tap sees the key's own press and release (`flagsChanged`, left and right
//! told apart by the device-dependent flags) and lets them through; another key going down while
//! it is held makes it the user's chord (⌥E types é): [Hit::Abort].
//!
//! The tap sees every key press in the system, so its callback only compares the event with the
//! slots and keys (atomics the main thread stores) and puts edges into a preallocated channel: no
//! allocation, no lock, no I/O.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender as Sender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use keyboard_types::{Code, Modifiers};
use objc2_core_graphics::CGEventFlags;

use super::super::keyboard::kvk;
use crate::hotkey::{Edge, Listener, MAX, OnEdge, Slot, Status};
use crate::keys::Hotkey;

mod carbon;
mod tap;

use carbon::Carbon;

/// What the tap measures time from: set before the tap exists, read without a lock.
static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// An edge the other source reported this long before is the same key press.
const DEDUPE: Duration = Duration::from_millis(50);
/// Room for a burst of events between two runs of the hotkey thread; a full channel drops events
/// (the app must cope with a lost release).
const QUEUE: usize = 64;
const KEY_ESCAPE: u16 = 0x35;

/// A slot as the tap compares it: key code in the low 16 bits, the event's modifier flags
/// above, [SET] when there is one.
const SET: u64 = 1 << 63;
/// With [SET]: the key code is a modifier key's, which is the hotkey alone.
const LONE: u64 = 1 << 62;
/// A key as the tap compares it: key code and modifier flags as a slot's, without [SET] (the key
/// count says which are set: `kVK_ANSI_A` is 0); a key `set` refused matches no event.
const NO_KEY: u64 = 0xffff;
const MODIFIERS: u64 = CGEventFlags::MaskShift.0
  | CGEventFlags::MaskControl.0
  | CGEventFlags::MaskAlternate.0
  | CGEventFlags::MaskCommand.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
  Tap = 0,
  Carbon = 1,
}

/// An edge as a source reports it: a slot's press (`repeat`: its autorepeat), its release, a
/// modifier slot alone dropped as another key went down while it was held (`Abort`), key `index`
/// taken while a slot was held (`Key`), Escape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
  Down { which: usize, repeat: bool },
  Up { which: usize },
  Abort { which: usize },
  Key(usize),
  Escape,
}

impl Hit {
  /// What the dedupe tells apart: the kind of edge, and whose.
  fn kind(self) -> (usize, usize) {
    match self {
      Hit::Down { which, .. } => (0, which),
      Hit::Up { which } => (1, which),
      Hit::Abort { which } => (2, which),
      Hit::Key(index) => (3, index),
      Hit::Escape => (4, 0),
    }
  }
}

#[derive(Debug, Clone, Copy)]
struct Raw {
  source: Source,
  edge: Hit,
  at: Instant,
}

/// Drops an edge when the other source reported the same kind of edge of the same slot ≤
/// [DEDUPE] earlier: whichever source is first wins, so neither waits for the other.
#[derive(Debug, Default)]
struct Dedupe {
  last: [[[Option<Instant>; MAX]; 5]; 2],
}

impl Dedupe {
  fn accept(&mut self, source: Source, edge: Hit, at: Instant) -> bool {
    let (kind, which) = edge.kind();
    let other = self.last[1 - source as usize][kind][which];
    self.last[source as usize][kind][which] = Some(at);
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
  /// Each slot's hotkey ([SET], [LONE]), 0 for none.
  chords: [AtomicU64; MAX],
  /// The keys ([NO_KEY]), how many, and a bit per key whose press the tap took (its repeats and
  /// release owed).
  keys: [AtomicU64; MAX],
  keys_count: AtomicU8,
  keys_owed: AtomicU8,
  /// A bit per slot: the tap owes a chord's release, or a modifier alone is held.
  down: AtomicU8,
  /// When each slot last went down, in ns of [EPOCH]; [tap::CLOSED] once a key was taken while a
  /// modifier alone was held.
  since: [AtomicU64; MAX],
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
  key_events: AtomicU64,
}

impl Shared {
  fn send(&self, events: &Sender<Raw>, raw: Raw) {
    if events.try_send(raw).is_err() {
      self.overflow.fetch_add(1, Ordering::Relaxed);
    }
  }

  /// Slot `i` holds `value` from now on: 0 first, its held bit dropped, so the tap never sees the
  /// new hotkey with the old one's press. Main thread only.
  fn store_slot(&self, i: usize, value: u64) {
    self.chords[i].store(0, Ordering::Release);
    self.down.fetch_and(!(1 << i), Ordering::AcqRel);
    self.chords[i].store(value, Ordering::Release);
  }

  /// The keys from now on; a key that changed owes nothing. Main thread only.
  fn store_keys(&self, keys: &[u64]) {
    for (j, key) in self.keys.iter().enumerate() {
      let new = keys.get(j).copied().unwrap_or(NO_KEY);
      if key.swap(new, Ordering::AcqRel) != new {
        self.keys_owed.fetch_and(!(1 << j), Ordering::AcqRel);
      }
    }

    self.keys_count.store(keys.len().min(MAX) as u8, Ordering::Release);
  }
}

/// The event tap sees a modifier key alone go down and up.
pub fn modifier_alone() -> bool {
  true
}

/// The tap's Accessibility access is asked for apart ([crate::permissions]), never by the start.
pub fn start_asks() -> bool {
  false
}

/// The tap takes them; without it (no Accessibility access, Secure Input) they are not.
pub fn keys_while_held() -> bool {
  true
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
  /// Starts the hotkey thread feeding `on` and installs the Carbon handler; the slots come with
  /// [set_slot](Self::set_slot), the tap with [start](Self::start).
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

  /// Slot `which` matches `hotkey` from now on, nothing for `None`; `Err` when it has no key code
  /// here. Carbon refusing the chord (another app has it) is no error while the tap may still see
  /// it: the slot's fallback error.
  pub fn set_slot(&mut self, which: usize, hotkey: Option<Hotkey>) -> Result<(), String> {
    self.shared.store_slot(which, 0);
    if which == 0 {
      self.carbon.unregister();
      self.carbon_error = None;
    }

    let Some(hotkey) = hotkey else { return Ok(()) };
    self.shared.store_slot(which, slot_value(hotkey)?);
    if let (0, Hotkey::Shortcut(s)) = (which, hotkey) {
      let code = s.code().and_then(kvk).unwrap_or_default();
      self.carbon_error = self.carbon.register(code, carbon::modifiers(s.mods)).err();
    }

    Ok(())
  }

  /// The keys taken while a slot is held, by index; `None` where refused.
  pub fn set_keys(&mut self, keys: &[Option<Hotkey>]) {
    let values: Vec<u64> = keys.iter().map(|&k| key_value(k)).collect();
    self.shared.store_keys(&values);
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
      thread::Builder::new().name("hotkey-tap".into()).spawn(move || tap::run(shared, events));
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
    if let Some(slot) = out.slots.first_mut() {
      slot.fallback = self.carbon.registered();
      slot.fallback_error = self.carbon_error.clone();
    }

    out.secure_input = super::permissions::secure_input();
    let c = &mut out.counts;
    (c.listener, c.fallback, c.fallback_repeats) =
      (n(&s.tap_events), n(&s.carbon_events), n(&s.carbon_repeats));
    (c.duplicates, c.reenabled, c.overflow) =
      (n(&s.duplicates), n(&s.tap_reenabled), n(&s.overflow));
    c.keys = n(&s.key_events);
    // Secure Input leaves the tap deaf, never Carbon.
    let tap = out.listener == Listener::Active && !out.secure_input;
    if s.escape.load(Ordering::Relaxed) && !tap && !self.carbon.escape_registered() {
      let why = self.escape_error.as_deref().unwrap_or("not registered");
      out.escape_error = Some(format!("no event tap, and Carbon's hotkey for Escape: {why}"));
    }

    out.keys_error = keys_error(out, tap);
  }
}

impl Drop for Hotkeys {
  fn drop(&mut self) {
    (0..MAX).for_each(|i| _ = self.set_slot(i, None));
    self.escape(false);
  }
}

/// Why the keys are not taken now, or the warning a modifier hotkey alone brings; none without
/// keys.
fn keys_error(out: &Status, tap: bool) -> Option<String> {
  let lone =
    |s: &&Slot| s.error.is_none() && matches!(s.hotkey, Hotkey::Modifier(k) if k != Code::Fn);
  let lone = out.slots.iter().filter(lone).map(|s| s.hotkey.label());
  match () {
    _ if out.keys.is_empty() => None,
    _ if out.secure_input => Some("Secure Input is on: the event tap sees no keys".into()),
    _ if !tap => Some("keys while held need the event tap: Accessibility access".into()),
    _ => lone
      .map(|k| format!("while {k} is held the keys are taken: they may clash with typing"))
      .next(),
  }
}

/// The tap's value for `hotkey` in a slot.
fn slot_value(hotkey: Hotkey) -> Result<u64, String> {
  let none = || format!("{hotkey} has no key here");
  Ok(match hotkey {
    Hotkey::Shortcut(s) => {
      SET | u64::from(s.code().and_then(kvk).ok_or_else(none)?) | tap_flags(s.mods) << 16
    }
    Hotkey::Modifier(key) => SET | LONE | u64::from(kvk(key).ok_or_else(none)?),
  })
}

/// The tap's value for a key, [NO_KEY] for none or one without a key code here.
fn key_value(key: Option<Hotkey>) -> u64 {
  let Some(Hotkey::Shortcut(s)) = key else { return NO_KEY };
  s.code().and_then(kvk).map_or(NO_KEY, |code| u64::from(code) | tap_flags(s.mods) << 16)
}

/// The "hotkey" thread: dedupes and hands the edges to the app.
fn route(rx: &Receiver<Raw>, on: &OnEdge, shared: &Shared) {
  let mut dedupe = Dedupe::default();
  // A bit per slot whose press Carbon reported, released not yet.
  let mut carbon_down = 0u8;
  for mut raw in rx {
    let counter = match raw.source {
      Source::Tap => &shared.tap_events,
      Source::Carbon => &shared.carbon_events,
    };

    counter.fetch_add(1, Ordering::Relaxed);
    if raw.source == Source::Carbon {
      match raw.edge {
        // Carbon repeats the press while the key is held: an autorepeat.
        Hit::Down { which, .. } if carbon_down & 1 << which != 0 => {
          shared.carbon_repeats.fetch_add(1, Ordering::Relaxed);
          raw.edge = Hit::Down { which, repeat: true };
        }
        Hit::Down { which, .. } => carbon_down |= 1 << which,
        Hit::Up { which } => carbon_down &= !(1 << which),
        _ => {}
      }
    }

    if !dedupe.accept(raw.source, raw.edge, raw.at) {
      shared.duplicates.fetch_add(1, Ordering::Relaxed);
      continue;
    }

    let at = raw.at;
    on(match raw.edge {
      Hit::Down { which, repeat } => Edge::Down { which, at, repeat },
      Hit::Up { which } => Edge::Up { which, at },
      Hit::Abort { which } => Edge::Aborted { which },
      Hit::Key(index) => {
        shared.key_events.fetch_add(1, Ordering::Relaxed);
        Edge::Key { index, at }
      }
      Hit::Escape => Edge::Escape,
    });
  }
}

/// The event tap's modifier flags for a chord's modifiers.
pub(super) fn tap_flags(mods: Modifiers) -> u64 {
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

  #[test]
  fn the_first_source_wins_and_the_other_is_dropped_within_the_window() {
    let mut d = Dedupe::default();
    let t0 = Instant::now();
    let ms = |n| t0 + Duration::from_millis(n);
    let down = Hit::Down { which: 0, repeat: false };
    let up = Hit::Up { which: 0 };
    assert!(d.accept(Source::Tap, down, ms(0)));
    assert!(!d.accept(Source::Carbon, down, ms(3)), "Carbon right after the tap");
    assert!(d.accept(Source::Tap, up, ms(400)));
    assert!(!d.accept(Source::Carbon, up, ms(450)), "the window is inclusive");
    // Carbon first (the tap was late), then the tap's copy.
    assert!(d.accept(Source::Carbon, down, ms(1_000)));
    assert!(!d.accept(Source::Tap, down, ms(1_010)));
    // Out of the window both count; one source never dedupes itself (autorepeat, quick taps).
    assert!(d.accept(Source::Carbon, up, ms(1_200)));
    assert!(d.accept(Source::Tap, up, ms(1_251)));
    assert!(d.accept(Source::Tap, down, ms(1_260)));
    assert!(d.accept(Source::Tap, Hit::Down { which: 0, repeat: true }, ms(1_270)));
    // Kinds do not mix; Escape from both is one.
    assert!(d.accept(Source::Carbon, Hit::Abort { which: 0 }, ms(1_275)));
    assert!(d.accept(Source::Tap, Hit::Escape, ms(1_280)));
    assert!(!d.accept(Source::Carbon, Hit::Escape, ms(1_281)));
    // Nor do slots: another slot's press at once is its own.
    assert!(d.accept(Source::Carbon, Hit::Down { which: 1, repeat: false }, ms(2_000)));
    assert!(d.accept(Source::Tap, down, ms(2_001)));
    assert!(!d.accept(Source::Tap, Hit::Down { which: 1, repeat: false }, ms(2_002)));
  }

  /// The hotkey thread: the tap's press, Carbon's copy of it (dropped), Carbon repeating it (an
  /// autorepeat), the release and Escape from both, another slot's press and a key, as the app
  /// hears them.
  #[test]
  fn the_thread_dedupes_and_hands_edges_over() {
    let shared = Shared::default();
    let (tx, rx) = sync_channel(QUEUE);
    let t0 = Instant::now();
    let ms = |n| t0 + Duration::from_millis(n);
    let raw = |source, edge, at| Raw { source, edge, at };
    let down = |which| Hit::Down { which, repeat: false };
    for r in [
      raw(Source::Tap, down(0), ms(0)),
      raw(Source::Carbon, down(0), ms(5)),
      raw(Source::Carbon, down(1), ms(10)),
      raw(Source::Carbon, down(0), ms(400)),
      raw(Source::Carbon, down(1), ms(420)),
      raw(Source::Tap, Hit::Key(2), ms(450)),
      raw(Source::Tap, Hit::Up { which: 0 }, ms(600)),
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
      Edge::Down { which: 0, at: ms(0), repeat: false },
      Edge::Down { which: 1, at: ms(10), repeat: false },
      Edge::Down { which: 0, at: ms(400), repeat: true },
      Edge::Down { which: 1, at: ms(420), repeat: true },
      Edge::Key { index: 2, at: ms(450) },
      Edge::Up { which: 0, at: ms(600) },
      Edge::Escape,
    ];
    assert_eq!(got, want);
    let n = |a: &AtomicU64| a.load(Ordering::Relaxed);
    assert_eq!((n(&shared.duplicates), n(&shared.carbon_repeats)), (2, 2));
    assert_eq!((n(&shared.tap_events), n(&shared.carbon_events)), (4, 5));
    assert_eq!(n(&shared.key_events), 1);
  }

  #[test]
  fn hotkeys_map_to_key_codes_and_flags() {
    let Ok(Hotkey::Shortcut(s)) = Hotkey::parse("ctrl+option+cmd+F12") else { panic!() };
    assert_eq!(s.code().and_then(kvk), Some(0x6F));
    let want = CGEventFlags::MaskControl.0 | CGEventFlags::MaskAlternate.0;
    assert_eq!(tap_flags(s.mods), want | CGEventFlags::MaskCommand.0);
    for (text, code) in
      [("a", 0x00), ("z", 0x06), ("0", 0x1D), ("9", 0x19), ("f1", 0x7A), ("f20", 0x5A)]
    {
      let Ok(Hotkey::Shortcut(s)) = Hotkey::parse(text) else { panic!("{text}") };
      assert_eq!(s.code().and_then(kvk), Some(code), "{text}");
    }

    let lone = [("right_option", 0x3D), ("left_cmd", 0x37), ("fn", 0x3F), ("rshift", 0x3C)];
    for (text, code) in lone {
      let Ok(key @ Hotkey::Modifier(_)) = Hotkey::parse(text) else { panic!("{text}") };
      assert_eq!(slot_value(key), Ok(SET | LONE | code), "{text}");
    }

    // A key at kVK 0 is A, told from none by the count, not by its value.
    assert_eq!(key_value(Hotkey::parse("a").ok()), 0);
    assert_eq!(key_value(Hotkey::parse("shift+a").ok()), CGEventFlags::MaskShift.0 << 16);
    assert_eq!(key_value(None), NO_KEY);
  }

  /// A slot stored again drops only its own held bit; keys stored again owe what they owed,
  /// unless they changed.
  #[test]
  fn storing_keeps_what_did_not_change() {
    let s = Shared::default();
    let fn_alone = slot_value(Hotkey::parse("fn").unwrap()).unwrap();
    s.store_slot(0, fn_alone);
    s.store_slot(2, slot_value(Hotkey::parse("f2").unwrap()).unwrap());
    let (z, x) = (key_value(Hotkey::parse("z").ok()), key_value(Hotkey::parse("x").ok()));
    s.store_keys(&[z, x]);
    s.down.store(0b101, Ordering::Relaxed);
    s.keys_owed.store(0b11, Ordering::Relaxed);
    s.store_slot(2, 0);
    assert_eq!(s.down.load(Ordering::Relaxed), 0b001, "fn still held");
    s.store_keys(&[z, x]);
    assert_eq!(s.keys_owed.load(Ordering::Relaxed), 0b11, "the same keys owe the same");
    s.store_keys(&[z]);
    assert_eq!(s.keys_owed.load(Ordering::Relaxed), 0b01, "X gone, Z still owed");
    assert_eq!(s.keys_count.load(Ordering::Relaxed), 1);
  }

  #[test]
  fn the_keys_status() {
    let mut out = Status::default();
    assert_eq!(keys_error(&out, false), None, "no keys, nothing to say");
    out.keys = vec![Hotkey::parse("z").unwrap()];
    assert!(keys_error(&out, false).is_some_and(|e| e.contains("Accessibility")));
    let slot = |t| Slot {
      hotkey: Hotkey::parse(t).unwrap(),
      error: None,
      fallback: false,
      fallback_error: None,
    };
    out.slots = vec![slot("fn"), slot("right_option")];
    let warning = keys_error(&out, true).unwrap();
    assert!(warning.contains("Right ⌥") && !warning.contains("fn"), "{warning}");
    out.secure_input = true;
    assert!(keys_error(&out, true).is_some_and(|e| e.contains("Secure Input")));
  }
}
