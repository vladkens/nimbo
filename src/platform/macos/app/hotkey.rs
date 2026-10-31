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
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender as Sender, sync_channel};
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

use super::permissions::secure_input;

/// What the tap measures time from: set before the tap exists, read without a lock.
static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// While a slot is held, the hotkey thread looks this often for Secure Input and settles ([look]).
const LOOK: Duration = Duration::from_millis(250);
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
  /// A bit per slot: the tap owes a chord's release, or a modifier alone is held; and those whose
  /// release may have been missed (the tap was disabled, Secure Input on), checked before keys.
  down: AtomicU8,
  suspect: AtomicU8,
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
  /// A bit per slot whose press Carbon reported, released not yet; cleared as the slot changes.
  carbon_down: AtomicU8,
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
  /// Why Carbon did not take each slot's chord.
  carbon_errors: [Option<String>; MAX],
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
    Self { shared, events, carbon, carbon_errors: Default::default(), escape_error: None }
  }

  /// Slot `which` matches `hotkey` from now on, nothing for `None`; `Err` when it has no key code
  /// here. Carbon refusing the chord (another app has it) is no error while the tap may still see
  /// it: the slot's fallback error.
  pub fn set_slot(&mut self, which: usize, hotkey: Option<Hotkey>) -> Result<(), String> {
    let value = hotkey.map(slot_value).transpose();
    self.shared.store_slot(which, value.clone().ok().flatten().unwrap_or(0));
    // A press Carbon reported of the old one is no repeat of the new one.
    self.shared.carbon_down.fetch_and(!(1 << which), Ordering::AcqRel);
    self.carbon.unregister(which);
    self.carbon_errors[which] = None;
    if let (Ok(_), Some(Hotkey::Shortcut(s))) = (&value, hotkey)
      && !s.mods.contains(Modifiers::FN)
    {
      let (code, (_, mods)) = (s.code().and_then(kvk).unwrap_or_default(), mod_bits(s.mods));
      self.carbon_errors[which] = self.carbon.register(which, code, mods).err();
    }

    value.map(drop)
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
    for (i, slot) in out.slots.iter_mut().enumerate().take(MAX) {
      (slot.fallback, slot.fallback_error) =
        (self.carbon.chords[i].is_some(), self.carbon_errors[i].clone());
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
    if s.escape.load(Ordering::Relaxed) && !tap && self.carbon.escape.is_none() {
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
      SET | u64::from(s.code().and_then(kvk).ok_or_else(none)?) | mod_bits(s.mods).0 << 16
    }
    Hotkey::Modifier(key) => SET | LONE | u64::from(kvk(key).ok_or_else(none)?),
  })
}

/// The tap's value for a key, [NO_KEY] for none or one without a key code here.
fn key_value(key: Option<Hotkey>) -> u64 {
  let Some(Hotkey::Shortcut(s)) = key else { return NO_KEY };
  // fn is in no key.
  s.code()
    .and_then(kvk)
    .map_or(NO_KEY, |code| u64::from(code) | (mod_bits(s.mods).0 & MODIFIERS) << 16)
}

/// The "hotkey" thread: dedupes and hands the edges to the app.
fn route(rx: &Receiver<Raw>, on: &OnEdge, shared: &Shared) {
  let mut dedupe = Dedupe::default();
  // When Carbon reported each slot's press, in ns of [EPOCH].
  let (carbon_down, mut pressed) = (&shared.carbon_down, [0; MAX]);
  // The slots whose press the app heard: a release or an abort comes once per press, whichever
  // source (the tap, Carbon, a settle) tells it first; and when to look next ([look]).
  let (mut told, mut next_look) = (0u8, Instant::now() + LOOK);
  loop {
    // The look is due also while events keep coming: it never waits behind them.
    let held = shared.down.load(Ordering::Relaxed) != 0;
    let raws = if held && Instant::now() >= next_look {
      next_look = Instant::now() + LOOK;
      look(shared, secure_input(), &tap::key_down)
    } else {
      let next = match held {
        false => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        true => rx.recv_timeout(next_look.saturating_duration_since(Instant::now())),
      };

      match next {
        Ok(raw) => std::array::from_fn(|i| (i == 0).then_some(raw)),
        Err(RecvTimeoutError::Timeout) => continue,
        Err(RecvTimeoutError::Disconnected) => return,
      }
    };

    // Idle until this event: the first look is a full wait after the press.
    if !held {
      next_look = Instant::now() + LOOK;
    }

    for mut raw in raws.into_iter().flatten() {
      let counter = match raw.source {
        Source::Tap => &shared.tap_events,
        Source::Carbon => &shared.carbon_events,
      };

      counter.fetch_add(1, Ordering::Relaxed);
      if raw.source == Source::Carbon {
        match raw.edge {
          // Carbon repeats the press while the key is held: an autorepeat.
          Hit::Down { which, .. } if carbon_down.load(Ordering::Acquire) & 1 << which != 0 => {
            shared.carbon_repeats.fetch_add(1, Ordering::Relaxed);
            raw.edge = Hit::Down { which, repeat: true };
          }
          Hit::Down { which, .. } => {
            carbon_down.fetch_or(1 << which, Ordering::AcqRel);
            pressed[which] = ns(raw.at);
          }
          // The tap's press of it is over too, unless it saw a newer one (a release it missed
          // while it was disabled would keep the key owed). Carbon's release may come before the
          // tap's: the tap then lets the key's release through, and Carbon's is the edge.
          Hit::Up { which } => {
            carbon_down.fetch_and(!(1 << which), Ordering::AcqRel);
            if shared.since[which].load(Ordering::Relaxed)
              <= pressed[which] + DEDUPE.as_nanos() as u64
            {
              shared.down.fetch_and(!(1 << which), Ordering::AcqRel);
            }
          }
          _ => {}
        }
      }

      let untold = |w: usize| told & 1 << w == 0;
      let again = matches!(raw.edge, Hit::Up { which } | Hit::Abort { which } if untold(which));
      if again || !dedupe.accept(raw.source, raw.edge, raw.at) {
        shared.duplicates.fetch_add(1, Ordering::Relaxed);
        continue;
      }

      match raw.edge {
        Hit::Down { which, .. } => told |= 1 << which,
        Hit::Up { which } | Hit::Abort { which } => told &= !(1 << which),
        _ => {}
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
}

/// While a slot is held, Secure Input (which keeps its release from the tap) makes it suspect,
/// and a suspect chord whose key is up by now is released ([tap::settle]): its `Up`, as the
/// tap's.
fn look(shared: &Shared, secure: bool, key_down: &dyn Fn(u64) -> bool) -> [Option<Raw>; MAX] {
  if secure {
    shared.suspect.fetch_or(shared.down.load(Ordering::Relaxed), Ordering::Relaxed);
  }

  let up = tap::settle(
    shared,
    &std::array::from_fn(|i| shared.chords[i].load(Ordering::Acquire)),
    key_down,
  );
  let raw = |which| Raw { source: Source::Tap, edge: Hit::Up { which }, at: Instant::now() };
  std::array::from_fn(|i| (up & 1 << i != 0).then(|| raw(i)))
}

/// `at` in ns of [EPOCH], as the tap tells time.
fn ns(at: Instant) -> u64 {
  EPOCH.get().map_or(0, |e| at.saturating_duration_since(*e).as_nanos() as u64)
}

/// Each modifier: the event tap's flag, and Carbon's bit (`controlKey`, `optionKey`,
/// `shiftKey`, `cmdKey`; fn has none: a chord with it is the tap's only).
const MOD_BITS: [(Modifiers, u64, u32); 5] = [
  (Modifiers::CONTROL, CGEventFlags::MaskControl.0, 1 << 12),
  (Modifiers::ALT, CGEventFlags::MaskAlternate.0, 1 << 11),
  (Modifiers::SHIFT, CGEventFlags::MaskShift.0, 1 << 9),
  (Modifiers::META, CGEventFlags::MaskCommand.0, 1 << 8),
  (Modifiers::FN, CGEventFlags::MaskSecondaryFn.0, 0),
];

/// The event tap's modifier flags and Carbon's modifier bits for a chord's modifiers.
pub(super) fn mod_bits(mods: Modifiers) -> (u64, u32) {
  MOD_BITS.iter().filter(|m| mods.contains(m.0)).fold((0, 0), |(f, c), m| (f | m.1, c | m.2))
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

  /// Hotkeys over a fake Carbon: nothing registered with the system, no tap.
  fn fake() -> Hotkeys {
    let shared = Arc::new(Shared::default());
    let (events, _) = sync_channel(QUEUE);
    let carbon = carbon::tests::fake(shared.clone(), events.clone());
    Hotkeys { shared, events, carbon, carbon_errors: Default::default(), escape_error: None }
  }

  #[test]
  fn each_chord_is_registered_once_and_never_leaked() {
    let hk = |t: &str| Hotkey::parse(t).ok();
    let mut h = fake();
    for (i, t) in ["f1", "fn", "f3", "option+space"].into_iter().enumerate() {
      assert_eq!(h.set_slot(i, hk(t)), Ok(()), "{t}");
    }

    // Ids 1 + which; fn alone has none; slot 2 refused by "another app", alone.
    assert_eq!(carbon::tests::live(), [1, 4]);
    let slots = ["f1", "fn", "f3", "option+space"]
      .map(|t| Slot { hotkey: hk(t).unwrap(), error: None, fallback: false, fallback_error: None })
      .to_vec();
    let mut out = Status { slots, ..Status::default() };
    h.status(&mut out);
    let fallback: Vec<_> =
      out.slots.iter().map(|s| (s.fallback, s.fallback_error.is_some())).collect();
    assert_eq!(fallback, [(true, false), (false, false), (false, true), (true, false)]);
    // Changed: the old one goes first, and a press Carbon reported of it is forgotten; removed:
    // gone; Escape on and off.
    h.shared.carbon_down.store(0b1, Ordering::Relaxed);
    h.set_slot(0, hk("f2")).unwrap();
    assert_eq!(h.shared.carbon_down.load(Ordering::Relaxed), 0);
    h.set_slot(3, None).unwrap();
    h.escape(true);
    assert_eq!(carbon::tests::live(), [1, 0x100]);
    h.escape(false);
    // Paused (every slot cleared, as `crate::hotkey` does), then dropped: nothing left.
    (0..MAX).for_each(|i| _ = h.set_slot(i, None));
    assert_eq!(carbon::tests::live(), []);
    // A chord with fn is the tap's only: nothing registered, no fallback error.
    h.set_slot(1, hk("fn+1")).unwrap();
    assert_eq!((carbon::tests::live(), &h.carbon_errors[1]), (vec![], &None));
    h.set_slot(0, hk("f1")).unwrap();
    h.escape(true);
    drop(h);
    assert_eq!(carbon::tests::live(), []);
  }

  /// While a slot is held the hotkey thread looks for Secure Input: it makes the held chords
  /// suspect, and one whose key is up by now is released, its `Up` as the tap's.
  #[test]
  fn secure_input_settles_a_chord_held_through_it() {
    let shared = Shared::default();
    shared.store_slot(0, slot_value(Hotkey::parse("fn+1").unwrap()).unwrap());
    shared.store_slot(1, slot_value(Hotkey::parse("f2").unwrap()).unwrap());
    shared.down.store(0b11, Ordering::Relaxed);
    assert!(look(&shared, false, &|_| false).iter().all(Option::is_none), "nothing asked");
    let ups = look(&shared, true, &|code| code == 0x78);
    let ups: Vec<_> = ups.iter().flatten().map(|r| (r.source, r.edge)).collect();
    assert_eq!(ups, [(Source::Tap, Hit::Up { which: 0 })]);
    assert_eq!(shared.down.load(Ordering::Relaxed), 0b10, "F2 still down");
    // A modifier alone held through it, its flag gone by now: released too.
    shared.store_slot(2, slot_value(Hotkey::parse("right_option").unwrap()).unwrap());
    shared.down.store(0b100, Ordering::Relaxed);
    let ups = look(&shared, true, &|_| false);
    assert_eq!(ups.iter().flatten().map(|r| r.edge).collect::<Vec<_>>(), [Hit::Up { which: 2 }]);
  }

  /// One release per press, whichever source tells it first: a settle's `Up`, then Carbon's
  /// later than the dedupe's window, is one `Up`; so is an abort then a release.
  #[test]
  fn one_release_per_press() {
    let t0 = Instant::now();
    let ms = |n| t0 + Duration::from_millis(n);
    let (shared, (tx, rx)) = (Shared::default(), sync_channel(QUEUE));
    let raw = |source, edge, at| Raw { source, edge, at };
    for r in [
      raw(Source::Tap, Hit::Down { which: 0, repeat: false }, ms(0)),
      raw(Source::Carbon, Hit::Down { which: 0, repeat: false }, ms(2)),
      raw(Source::Tap, Hit::Up { which: 0 }, ms(300)),
      raw(Source::Carbon, Hit::Up { which: 0 }, ms(400)),
      raw(Source::Tap, Hit::Down { which: 1, repeat: false }, ms(500)),
      raw(Source::Tap, Hit::Abort { which: 1 }, ms(510)),
      raw(Source::Tap, Hit::Up { which: 1 }, ms(700)),
    ] {
      tx.send(r).unwrap();
    }

    drop(tx);
    let (got_tx, got) = std::sync::mpsc::channel();
    route(&rx, &(Box::new(move |e| got_tx.send(e).unwrap()) as OnEdge), &shared);
    let ends = got.try_iter().filter(|e| matches!(e, Edge::Up { .. } | Edge::Aborted { .. }));
    assert_eq!(ends.count(), 2);
  }

  /// Carbon's release clears the tap's held bit of a press it missed the release of, not one of
  /// a newer press.
  #[test]
  fn carbons_release_clears_a_stale_press() {
    let t0 = *EPOCH.get_or_init(Instant::now);
    let ms = |n| t0 + Duration::from_millis(n);
    let shared = Shared::default();
    let (tx, rx) = sync_channel(QUEUE);
    // Slot 0's press seen by both, its release by Carbon only; slot 1's press by Carbon at 0,
    // and a newer one by the tap at 1 s, before Carbon's release of the first came through.
    shared.down.store(0b11, Ordering::Relaxed);
    shared.since[0].store(ns(ms(2)), Ordering::Relaxed);
    shared.since[1].store(ns(ms(1_000)), Ordering::Relaxed);
    let raw = |edge, at| Raw { source: Source::Carbon, edge, at };
    for r in [
      raw(Hit::Down { which: 0, repeat: false }, ms(0)),
      raw(Hit::Down { which: 1, repeat: false }, ms(0)),
      raw(Hit::Up { which: 0 }, ms(400)),
      raw(Hit::Up { which: 1 }, ms(900)),
    ] {
      tx.send(r).unwrap();
    }

    drop(tx);
    route(&rx, &(Box::new(|_| {}) as OnEdge), &shared);
    assert_eq!(shared.down.load(Ordering::Relaxed), 0b10);
  }

  #[test]
  fn hotkeys_map_to_key_codes_and_flags() {
    let Ok(Hotkey::Shortcut(s)) = Hotkey::parse("ctrl+option+cmd+F12") else { panic!() };
    assert_eq!(s.code().and_then(kvk), Some(0x6F));
    let want = CGEventFlags::MaskControl.0 | CGEventFlags::MaskAlternate.0;
    assert_eq!(mod_bits(s.mods), (want | CGEventFlags::MaskCommand.0, 1 << 12 | 1 << 11 | 1 << 8));
    assert_eq!(mod_bits(Modifiers::SHIFT), (CGEventFlags::MaskShift.0, 1 << 9));
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
