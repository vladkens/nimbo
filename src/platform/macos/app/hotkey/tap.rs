// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The hotkeys' event tap: a `CGEventTap` on a thread of its own that judges every key event in
//! the session against the slots and keys ([judge]) and swallows or passes it, sending its edges
//! to the hotkey thread. Its callback compares atomics and sends into a preallocated channel: no
//! allocation, no lock, no I/O.

use std::ffi::c_void;
use std::ptr::{NonNull, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};
use std::sync::mpsc::SyncSender as Sender;
use std::time::{Duration, Instant};

use keyboard_types::Code;
use objc2_core_foundation::{CFMachPort, CFRetained, CFRunLoop, kCFRunLoopCommonModes};
use objc2_core_graphics::{
  CGEvent, CGEventField, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
  CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy, CGEventType,
};

use super::super::super::keyboard::{code as key_at, device_flag};
use super::{
  Hit, KEY_ESCAPE, LONE, MODIFIERS, Raw, SET, Shared, Source, TAP_ACTIVE, TAP_FAILED, ns,
};
use crate::hotkey::{MAX, Take, owe};

/// A key pressed this soon after a modifier hotkey went down makes it a chord of the user's (⌥E,
/// fn+F): the press is dropped. Later, keys are typing: they never cancel.
const CHORD_WINDOW: Duration = Duration::from_millis(300);
/// [Shared::since] of a modifier slot whose chord window a taken key closed.
pub(super) const CLOSED: u64 = u64::MAX;
const ESCAPE: u64 = KEY_ESCAPE as u64;
const FN: u64 = CGEventFlags::MaskSecondaryFn.0;
/// The device-dependent flags of all modifier keys (which side is held): both ⌃, ⇧, ⌘ and ⌥,
/// and fn ([device_flag]).
const DEVICES: u64 = 0x0080_207F;

/// What the tap callback works with; leaked once the tap exists: the tap thread lives as long as
/// the process.
struct TapState {
  shared: Arc<Shared>,
  events: Sender<Raw>,
  port: AtomicPtr<CFMachPort>,
}

pub(super) fn run(shared: Arc<Shared>, events: Sender<Raw>) {
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
      Some(callback),
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
unsafe extern "C-unwind" fn callback(
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

    // Releases may have been missed while it was off: a chord whose key is up is released.
    let (s, at) = (&state.shared, Instant::now());
    s.suspect.fetch_or(s.down.load(Ordering::Relaxed), Ordering::Relaxed);
    let up = settle(s, &slots(s), &key_down);
    for which in (0..MAX).filter(|i| up & 1 << i != 0) {
      s.send(&state.events, Raw { source: Source::Tap, edge: Hit::Up { which }, at, chord: 0 });
    }

    s.tap_reenabled.fetch_add(1, Ordering::Relaxed);
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
  // The app's own posted keys ([super::super::post]) are no key of the user's.
  if CGEvent::integer_value_field(event, CGEventField::EventSourceUserData)
    == super::super::post::POSTED
  {
    return pass;
  }

  let code = CGEvent::integer_value_field(event, CGEventField::KeyboardEventKeycode) as u64;
  let flags = CGEvent::flags(event).0;
  let repeat = || CGEvent::integer_value_field(event, CGEventField::KeyboardEventAutorepeat) != 0;
  let at = ns(Instant::now());
  let chords = slots(&state.shared);
  let (hits, out) = match judge(&state.shared, chords, kind, code, flags, at, repeat, &key_down) {
    Verdict::Pass => return pass,
    Verdict::Tell(hits) => (hits, pass),
    Verdict::Swallow(hits) => (hits, null_mut()),
  };

  for edge in hits.into_iter().flatten() {
    // A press carries the hotkey it was judged against, for the hotkey thread to check.
    let chord = match edge {
      Hit::Down { which, repeat: false } => chords[which],
      _ => 0,
    };

    let raw = Raw { source: Source::Tap, edge, at: Instant::now(), chord };
    state.shared.send(&state.events, raw);
  }

  out
}

/// The key at `code` is down now, as the HID system has it.
pub(super) fn key_down(code: u64) -> bool {
  let hid = CGEventSourceStateID::HIDSystemState;
  match device_flag(key_at(code as u16)) {
    // A modifier key, fn too: by its flag, as the HID system has them.
    Some(flag) => CGEventSource::flags_state(hid).0 & flag != 0,
    None => CGEventSource::key_state(hid, code as u16),
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

/// One event's edges, at most `2 + MAX`: the modifier slot alone it releases or drops (one is held
/// at a time, its press needs no other modifier held), the chords a missed release settles
/// ([settle]), and the event's own.
type Hits = [Option<Hit>; 2 + MAX];

/// The slots as they are now.
pub(super) fn slots(s: &Shared) -> [u64; MAX] {
  std::array::from_fn(|i| s.chords[i].load(Ordering::SeqCst))
}

/// What the tap does with a key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
  Pass,
  /// Let through, and the edges go to the app.
  Tell(Hits),
  /// Swallowed; the edges, if any, go to the app.
  Swallow(Hits),
}

/// A key event as [judge] goes through it: `code`, the event's `flags`, `at` in ns of
/// [super::EPOCH], `down` (a key down) and `rep` (its autorepeat), the slots as they are, the edges
/// so far.
struct Event<'a> {
  s: &'a Shared,
  chords: [u64; MAX],
  kind: KeyKind,
  code: u64,
  flags: u64,
  at: u64,
  down: bool,
  rep: bool,
  hits: Hits,
  key_down: &'a dyn Fn(u64) -> bool,
}

impl Event<'_> {
  fn push(&mut self, hit: Hit) {
    if let Some(free) = self.hits.iter_mut().find(|h| h.is_none()) {
      *free = Some(hit);
    }
  }

  fn tell(&self) -> Verdict {
    if self.hits[0].is_some() { Verdict::Tell(self.hits) } else { Verdict::Pass }
  }

  /// Slot `i` is a modifier alone, and held.
  fn held(&self, i: usize) -> bool {
    alone(self.chords[i]) && self.s.down.load(Ordering::Relaxed) & 1 << i != 0
  }
}

/// The tap's decision for a key event against the slots `chords` read before it (`repeat` reads
/// its autorepeat field, only for a key down), in this order:
/// 1. a modifier slot's own key going down (with no other modifier held) or up ([lone]);
/// 2. a modifier slot held whose flag the event lacks is released (`Up`; Escape's press drops it,
///    `Abort`, and is the app's), before anything reads the held bits: a release missed never takes
///    a key;
/// 3. the keys ([keys]): while any slot is held a fresh press of one is taken, `Key`;
/// 4. the chords ([chords]): a press with exactly their modifiers is taken, `Down`;
/// 5. another fresh press within [CHORD_WINDOW] of a modifier slot's press drops it (⌥E, the user's
///    chord: `Abort`, the key is the app's); a taken key (3) closed the window before;
/// 6. Escape, while [Shared::escape] is on: a press with no ⌃, ⌥ or ⌘ held (Shift is fine, and so
///    is a modifier slot's own key, held past its window) is taken and told.
///
/// What is taken is owed until its release ([owe]): its repeats and release are swallowed too.
#[allow(clippy::too_many_arguments, reason = "the event's fields, as the callback reads them")]
fn judge(
  s: &Shared,
  chords: [u64; MAX],
  kind: KeyKind,
  code: u64,
  flags: u64,
  at: u64,
  repeat: impl Fn() -> bool,
  key_down: &dyn Fn(u64) -> bool,
) -> Verdict {
  let down = kind == KeyKind::Down;
  let rep = down && repeat();
  let mut e =
    Event { s, chords, kind, code, flags, at, down, rep, hits: [None; 2 + MAX], key_down };
  let own =
    (0..MAX).find(|&i| alone(chords[i]) && kind == KeyKind::Flags && code == chords[i] & 0xffff);
  for i in (0..MAX).filter(|&i| alone(chords[i])) {
    if let Some(hit) = lone(&e, i, own == Some(i)) {
      e.push(hit);
    }
  }

  if e.hits.iter().any(|h| matches!(h, Some(Hit::Abort { .. }))) {
    return e.tell();
  }

  if kind != KeyKind::Flags && (keys(&mut e) || chords_of(&mut e)) {
    return Verdict::Swallow(e.hits);
  }

  if kind == KeyKind::Flags || down && !rep {
    for i in (0..MAX).filter(|&i| own != Some(i) && e.held(i)) {
      let since = s.since[i].load(Ordering::Relaxed);
      if since != CLOSED && at.saturating_sub(since) < CHORD_WINDOW.as_nanos() as u64 {
        s.down.fetch_and(!(1 << i), Ordering::Relaxed);
        e.push(Hit::Abort { which: i });
        return e.tell();
      }
    }
  }

  if kind == KeyKind::Flags || code != ESCAPE {
    return e.tell();
  }

  let (owed, take) = owe(s.escape_down.load(Ordering::Relaxed), down, rep, escape(&e));
  s.escape_down.store(owed, Ordering::Relaxed);
  match take {
    Take::Pass => return e.tell(),
    Take::Claim => e.push(Hit::Escape),
    Take::Swallow => {}
  }

  Verdict::Swallow(e.hits)
}

/// The slot is a modifier key alone.
fn alone(chord: u64) -> bool {
  chord & (SET | LONE) == SET | LONE
}

/// The modifier flags that slot `chord` holds down: a chord's, or a modifier alone's own.
fn mods_of(chord: u64) -> u64 {
  match alone(chord) {
    true => general_flag(key_at((chord & 0xffff) as u16)),
    false => chord >> 16 & (MODIFIERS | FN),
  }
}

/// Modifier slot `i`, on its own key's `flagsChanged` (`own`): its press with no other modifier
/// held, or its release. Any other event without its flag while it is held is its release, also
/// when the release itself was not seen (the Globe key's may come with another code); Escape as
/// that event drops the press instead.
fn lone(e: &Event, i: usize, own: bool) -> Option<Hit> {
  let key = key_at((e.chords[i] & 0xffff) as u16);
  let (flag, bit, down) = (device_flag(key)?, 1u8 << i, &e.s.down);
  let (held, pressed) = (e.held(i), e.flags & device_flag(key)? != 0);
  if own {
    // Another modifier held: of another kind, or the other side of this one.
    let others = e.flags & (MODIFIERS | FN) & !general_flag(key) | e.flags & DEVICES & !flag;
    return match (pressed, held) {
      (true, false) if others == 0 && hold(e.s, i, e.chords[i]) => {
        e.s.since[i].store(e.at, Ordering::Relaxed);
        Some(Hit::Down { which: i, repeat: false })
      }
      (false, true) => {
        down.fetch_and(!bit, Ordering::Relaxed);
        Some(Hit::Up { which: i })
      }
      _ => None,
    };
  }

  if !held || pressed {
    return None;
  }

  down.fetch_and(!bit, Ordering::Relaxed);
  let escape = e.code == ESCAPE && e.kind == KeyKind::Down;
  Some(if escape { Hit::Abort { which: i } } else { Hit::Up { which: i } })
}

/// The keys (3): a fresh press while any slot is held, by place, with the event's modifiers or
/// those without the held slots' own (⌥Space held: ⌥Z is `z`; fn is in no key), is taken: the
/// first match in the list, told. Taken, it closes every modifier slot's chord window, so a
/// later key while the modifier is still held is typing, not the user's chord. Any other fresh
/// press is the app's and settles the key. Whether the event is swallowed.
fn keys(e: &mut Event) -> bool {
  let (s, code, n) = (e.s, e.code, usize::from(e.s.keys_count.load(Ordering::Acquire)).min(MAX));
  if e.down && !e.rep && (0..n).any(|j| s.keys[j].load(Ordering::Acquire) & 0xffff == code) {
    let up = settle(s, &e.chords, e.key_down);
    (0..MAX).filter(|i| up & 1 << i != 0).for_each(|which| e.push(Hit::Up { which }));
  }

  let held = s.down.load(Ordering::Relaxed);
  let theirs = (0..MAX).filter(|i| held & 1 << i != 0).fold(0, |m, i| m | mods_of(e.chords[i]));
  let event = e.flags & MODIFIERS;
  let (mut found, mut swallow) = (held == 0, false);
  // A key matches by its place as set now, or owed by the place it was taken at (changed since);
  // only an event at that place settles what is owed, or a claim of another key there.
  let owed = s.keys_owed.load(Ordering::Relaxed);
  let now = |j: usize| j < n && s.keys[j].load(Ordering::Acquire) & 0xffff == code;
  let owes = |j: usize| owed & 1 << j != 0 && s.owed_at[j].load(Ordering::Relaxed) == code;
  for j in (0..MAX).filter(|&j| now(j) || owes(j)) {
    let mods = s.keys[j].load(Ordering::Acquire) >> 16;
    let ours = !found && now(j) && (event == mods || event & !theirs == mods);
    let bit = 1u8 << j;
    let (on, take) = owe(owes(j), e.down, e.rep, ours);
    if owes(j) || take == Take::Claim {
      mark(&s.keys_owed, bit, on);
    }

    (found, swallow) = (found || ours, swallow || take != Take::Pass);
    if take == Take::Claim {
      s.owed_at[j].store(code, Ordering::Relaxed);
      e.push(Hit::Key(j));
      (0..MAX)
        .filter(|&i| alone(e.chords[i]))
        .for_each(|i| s.since[i].store(CLOSED, Ordering::Relaxed));
    }
  }

  swallow
}

/// The chords (4): a fresh press with exactly a slot's modifiers is its press, told, and drops a
/// modifier slot alone held that is one of them (⌥Space while Right ⌥ alone is held: two
/// edges); its repeats and release are told as such. Whether the event is swallowed.
fn chords_of(e: &mut Event) -> bool {
  let (s, chords, code, mut swallow) = (e.s, e.chords, e.code, false);
  for i in (0..MAX).filter(|&i| chords[i] & (SET | LONE) == SET && chords[i] & 0xffff == code) {
    let (bit, down) = (1u8 << i, &s.down);
    // fn counts only for a chord with it: F1's events carry fn anyway.
    let mask = MODIFIERS | chords[i] >> 16 & FN;
    let ours = e.flags & mask == chords[i] >> 16 & mask;
    let (on, take) = owe(down.load(Ordering::Relaxed) & bit != 0, e.down, e.rep, ours);
    if take == Take::Claim {
      // A fresh press, held by `hold` alone (the main thread may clear it meanwhile): its release
      // is the tap's to hear.
      if !hold(s, i, chords[i]) {
        continue;
      }

      s.since[i].store(e.at, Ordering::Relaxed);
      s.suspect.fetch_and(!bit, Ordering::Relaxed);
    } else {
      mark(down, bit, on);
    }

    swallow |= take != Take::Pass;
    match take {
      Take::Pass => {}
      Take::Swallow if e.down => e.push(Hit::Down { which: i, repeat: true }),
      Take::Swallow => e.push(Hit::Up { which: i }),
      Take::Claim => {
        let under = |l: usize| alone(chords[l]) && down.load(Ordering::Relaxed) & 1 << l != 0;
        for l in (0..MAX).filter(|&l| under(l) && mods_of(chords[l]) & mods_of(chords[i]) != 0) {
          down.fetch_and(!(1 << l), Ordering::Relaxed);
          e.push(Hit::Abort { which: l });
        }

        e.push(Hit::Down { which: i, repeat: false });
      }
    }
  }

  swallow
}

/// Holds slot `i` for a fresh press of `chord`, the slot as [judge] read it: its bit set, unless
/// the main thread stored another hotkey there meanwhile ([Shared::store_slot] stores 0, then
/// clears the bit: a bit set after that clear sees the change here), when the press is no
/// longer the slot's and passes. Whether it is held.
fn hold(s: &Shared, i: usize, chord: u64) -> bool {
  s.down.fetch_or(1 << i, Ordering::SeqCst);
  let held = s.chords[i].load(Ordering::SeqCst) == chord;
  if !held {
    s.down.fetch_and(!(1 << i), Ordering::SeqCst);
  }

  held
}

/// Sets `bit` of `bits` when `on`, else clears it.
fn mark(bits: &AtomicU8, bit: u8, on: bool) {
  bits.fetch_and(!bit, Ordering::Relaxed);
  bits.fetch_or(if on { bit } else { 0 }, Ordering::Relaxed);
}

/// A slot held whose release may have been missed ([Shared::suspect]: the tap was disabled, or
/// Secure Input was on) whose key is up (`key_down`, the HID system's state; a modifier alone's
/// by its flag) is released: the bits cleared, for their `Up`s; one whose key is down is no
/// longer suspect. No other press asks the HID system: a chord truly held takes keys.
pub(super) fn settle(s: &Shared, chords: &[u64; MAX], key_down: &dyn Fn(u64) -> bool) -> u8 {
  let down = s.down.load(Ordering::Relaxed);
  let suspect = s.suspect.swap(0, Ordering::Relaxed) & down;
  let gone = |i: usize| suspect & 1 << i != 0 && !key_down(chords[i] & 0xffff);
  let up = (0..MAX).filter(|&i| gone(i)).fold(0, |m, i| m | 1 << i);
  s.down.fetch_and(!up, Ordering::Relaxed);
  up
}

/// A press of Escape is ours (6): taking it is on, and no ⌃, ⌥ or ⌘ is held, but for a modifier
/// slot's own key held with no other modifier key beside Shift.
fn escape(e: &Event) -> bool {
  let (mut own, mut theirs) = (0, 0);
  for i in (0..MAX).filter(|&i| e.held(i)) {
    let key = key_at((e.chords[i] & 0xffff) as u16);
    (own, theirs) = (own | device_flag(key).unwrap_or(0), theirs | general_flag(key));
  }

  let shift = [Code::ShiftLeft, Code::ShiftRight].map(|k| device_flag(k).unwrap_or(0));
  let only = e.flags & DEVICES & !own & !shift[0] & !shift[1] == 0;
  let chord = MODIFIERS & !CGEventFlags::MaskShift.0 & !if only { theirs } else { 0 };
  e.flags & chord == 0 && e.s.escape.load(Ordering::Acquire)
}

/// The device-independent flag of modifier key `key`, either side's.
fn general_flag(key: Code) -> u64 {
  match key {
    Code::ControlLeft | Code::ControlRight => CGEventFlags::MaskControl.0,
    Code::AltLeft | Code::AltRight => CGEventFlags::MaskAlternate.0,
    Code::ShiftLeft | Code::ShiftRight => CGEventFlags::MaskShift.0,
    Code::MetaLeft | Code::MetaRight => CGEventFlags::MaskCommand.0,
    _ => FN,
  }
}

#[cfg(test)]
mod tests {
  use super::super::super::super::keyboard::kvk;
  use super::super::{key_value, slot_value};
  use super::*;
  use crate::keys::Hotkey;

  const SPACE: u64 = 0x31;
  const ESC: u64 = ESCAPE;
  const E: u64 = 0x0E;
  const Z: u64 = 0x06;
  const A: u64 = 0x00;
  const ONE: u64 = 0x12;
  const F5: u64 = 0x60;
  const NONE: u64 = 0;
  const OPT: u64 = CGEventFlags::MaskAlternate.0;
  const SHIFT: u64 = CGEventFlags::MaskShift.0 | 0x02;
  const CMD: u64 = CGEventFlags::MaskCommand.0 | 0x08;
  const RIGHT_ALT: u64 = OPT | 0x40;
  const LEFT_ALT: u64 = OPT | 0x20;

  /// `slots` and `keys` set, as `Hotkeys::set_slot` and `set_keys` store them.
  fn with(slots: &[&str], keys: &[&str]) -> Shared {
    let s = Shared::default();
    for (i, text) in slots.iter().enumerate() {
      s.store_slot(i, slot_value(Hotkey::parse(text).unwrap()).unwrap());
    }

    let keys: Vec<u64> = keys.iter().map(|t| key_value(Hotkey::parse(t).ok())).collect();
    s.store_keys(&keys);
    s
  }

  /// The edges `hits`, in order, as a verdict holds them.
  fn hits(hits: &[Hit]) -> Hits {
    std::array::from_fn(|i| hits.get(i).copied())
  }

  fn tell(hit: Hit) -> Verdict {
    Verdict::Tell(hits(&[hit]))
  }

  fn swallow(hit: Hit) -> Verdict {
    Verdict::Swallow(hits(&[hit]))
  }

  const SWALLOWED: Verdict = Verdict::Swallow([None; 2 + MAX]);

  fn down(which: usize) -> Hit {
    Hit::Down { which, repeat: false }
  }

  fn up(which: usize) -> Hit {
    Hit::Up { which }
  }

  fn key(s: &Shared, down: bool, code: u64, flags: u64, repeat: bool) -> Verdict {
    key_at(s, 0, down, code, flags, repeat)
  }

  /// A key event `ms` after the start.
  fn key_at(s: &Shared, ms: u64, down: bool, code: u64, flags: u64, repeat: bool) -> Verdict {
    let kind = if down { KeyKind::Down } else { KeyKind::Up };
    judge(s, slots(s), kind, code, flags, ms * 1_000_000, || repeat, &|_| true)
  }

  /// A modifier key's `flagsChanged`: `flags` as they are after it.
  fn flags(s: &Shared, key: Code, flags: u64) -> Verdict {
    flags_at(s, 0, key, flags)
  }

  fn flags_at(s: &Shared, ms: u64, key: Code, flags: u64) -> Verdict {
    let code = kvk(key).unwrap();
    let at = ms * 1_000_000;
    judge(s, slots(s), KeyKind::Flags, u64::from(code), flags, at, || false, &|_| true)
  }

  #[test]
  fn the_device_flags_are_every_modifier_keys() {
    let keys = [Code::ControlLeft, Code::ControlRight, Code::AltLeft, Code::AltRight, Code::Fn];
    let keys =
      keys.into_iter().chain([Code::ShiftLeft, Code::ShiftRight, Code::MetaLeft, Code::MetaRight]);
    assert_eq!(keys.filter_map(device_flag).fold(0, |all, f| all | f), DEVICES);
  }

  #[test]
  fn a_modifier_alone_fires_on_its_press_and_release_and_lets_them_through() {
    let s = with(&["right_option"], &[]);
    assert_eq!(flags(&s, Code::AltRight, RIGHT_ALT), tell(down(0)));
    assert_eq!(flags(&s, Code::AltRight, 0), tell(up(0)));
    // The left one is another key; with ⌘ held it is a chord of the user's.
    assert_eq!(flags(&s, Code::AltLeft, LEFT_ALT), Verdict::Pass);
    assert_eq!(flags(&s, Code::AltLeft, 0), Verdict::Pass);
    assert_eq!(flags(&s, Code::AltRight, RIGHT_ALT | CMD), Verdict::Pass);
    assert_eq!(flags(&s, Code::AltRight, CMD), Verdict::Pass, "no release owed");
    // Left Option held, then Right Option: a chord of the user's too.
    assert_eq!(flags(&s, Code::AltRight, RIGHT_ALT | LEFT_ALT), Verdict::Pass);
    assert_eq!(flags(&s, Code::AltRight, LEFT_ALT), Verdict::Pass, "no release owed");
    // Keys and their releases are the apps' while it is not held.
    assert_eq!(key(&s, true, E, NONE, false), Verdict::Pass);
    assert_eq!(key(&s, false, E, NONE, false), Verdict::Pass);
  }

  #[test]
  fn another_key_while_the_modifier_is_held_aborts_it() {
    let s = with(&["right_option"], &[]);
    assert_eq!(flags(&s, Code::AltRight, RIGHT_ALT), tell(down(0)));
    // ⌥E: the press is aborted, E types é in the app; the release is no edge.
    assert_eq!(key(&s, true, E, RIGHT_ALT, false), tell(Hit::Abort { which: 0 }));
    assert_eq!(key(&s, false, E, RIGHT_ALT, false), Verdict::Pass);
    assert_eq!(flags(&s, Code::AltRight, 0), Verdict::Pass);
    // Another modifier going down aborts it too.
    flags(&s, Code::AltRight, RIGHT_ALT);
    assert_eq!(flags(&s, Code::ShiftLeft, RIGHT_ALT | SHIFT), tell(Hit::Abort { which: 0 }));
  }

  #[test]
  fn fn_alone_is_a_hotkey_too() {
    let s = with(&["fn"], &[]);
    assert_eq!(flags(&s, Code::Fn, FN), tell(down(0)));
    // An arrow key with fn held (its events carry the fn flag) right away: the user's.
    assert_eq!(key(&s, true, 0x7E, FN, false), tell(Hit::Abort { which: 0 }));
    assert_eq!(flags(&s, Code::Fn, 0), Verdict::Pass);
    assert_eq!(flags(&s, Code::Fn, FN), tell(down(0)));
    assert_eq!(flags(&s, Code::Fn, 0), tell(up(0)));
  }

  #[test]
  fn keys_after_the_chord_window_are_typing_and_never_cancel() {
    let s = with(&["fn"], &[]);
    // A long hold while typing: nothing aborts, the release stops.
    assert_eq!(flags_at(&s, 0, Code::Fn, FN), tell(down(0)));
    assert_eq!(key_at(&s, 400, true, E, FN, false), Verdict::Pass);
    assert_eq!(key_at(&s, 450, false, E, FN, false), Verdict::Pass);
    assert_eq!(flags_at(&s, 500, Code::ShiftLeft, FN | SHIFT), Verdict::Pass);
    assert_eq!(flags_at(&s, 3_000, Code::Fn, 0), tell(up(0)));
    // A tap: typing after it is the app's.
    flags_at(&s, 5_000, Code::Fn, FN);
    assert_eq!(flags_at(&s, 5_100, Code::Fn, 0), tell(up(0)));
    for (i, code) in [E, 0x7E, SPACE].into_iter().enumerate() {
      assert_eq!(key_at(&s, 5_150 + i as u64, true, code, NONE, false), Verdict::Pass);
    }
  }

  #[test]
  fn a_release_not_seen_is_seen_in_the_next_event() {
    let s = with(&["fn"], &[]);
    flags_at(&s, 0, Code::Fn, FN);
    // The Globe key's release came with another code: the next key has no fn flag.
    assert_eq!(key_at(&s, 900, true, E, NONE, false), tell(up(0)));
    assert_eq!(key_at(&s, 950, true, E, NONE, false), Verdict::Pass, "typing, no abort");
    // Escape as the next event drops the press instead.
    flags_at(&s, 2_000, Code::Fn, FN);
    assert_eq!(key_at(&s, 2_900, true, ESC, NONE, false), tell(Hit::Abort { which: 0 }));
    assert_eq!(key_at(&s, 3_000, true, E, NONE, false), Verdict::Pass, "released by then");
  }

  #[test]
  fn the_chord_is_swallowed_with_its_repeats_and_release() {
    let s = with(&["option+space"], &[]);
    let repeat = Hit::Down { which: 0, repeat: true };
    assert_eq!(key(&s, true, SPACE, OPT, false), swallow(down(0)));
    assert_eq!(key(&s, true, SPACE, OPT, true), swallow(repeat));
    // Option let go first: the repeats and the release are still the chord's.
    assert_eq!(key(&s, true, SPACE, NONE, true), swallow(repeat));
    assert_eq!(key(&s, false, SPACE, NONE, false), swallow(up(0)));
    // Then Space is the app's again.
    assert_eq!(key(&s, true, SPACE, NONE, false), Verdict::Pass);
    assert_eq!(key(&s, false, SPACE, NONE, false), Verdict::Pass);
    // Other modifiers, other keys: not the chord.
    assert_eq!(key(&s, true, SPACE, OPT | SHIFT, false), Verdict::Pass);
    assert_eq!(key(&s, true, 0x00, OPT, false), Verdict::Pass);
  }

  #[test]
  fn a_key_the_app_saw_go_down_stays_the_apps() {
    let s = with(&["option+space"], &[]);
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
    let s = with(&["option+space"], &[]);
    let taken = swallow(Hit::Escape);
    assert_eq!(key(&s, true, ESC, NONE, false), Verdict::Pass);
    assert_eq!(key(&s, false, ESC, NONE, false), Verdict::Pass);
    s.escape.store(true, Ordering::Release);
    assert_eq!(key(&s, true, ESC, NONE, false), taken);
    assert_eq!(key(&s, true, ESC, NONE, true), SWALLOWED, "its repeats");
    s.escape.store(false, Ordering::Release); // turned off meanwhile
    assert_eq!(key(&s, false, ESC, NONE, false), SWALLOWED, "its release");
    // Held since before it was on: the app's.
    assert_eq!(key(&s, true, ESC, NONE, false), Verdict::Pass);
    s.escape.store(true, Ordering::Release);
    assert_eq!(key(&s, true, ESC, NONE, true), Verdict::Pass);
    assert_eq!(key(&s, false, ESC, NONE, false), Verdict::Pass);
    // ⌘⎋, ⌃⎋, ⌥⎋ are not Escape; ⇧⎋ is.
    for flags in [CMD, CGEventFlags::MaskControl.0, OPT] {
      assert_eq!(key(&s, true, ESC, flags, false), Verdict::Pass, "{flags:x}");
    }

    assert_eq!(key(&s, true, ESC, SHIFT, false), taken);
    // The chord still works meanwhile.
    assert_eq!(key(&s, true, SPACE, OPT, false), swallow(down(0)));
  }

  #[test]
  fn escape_with_a_modifier_hotkey_held() {
    let s = with(&["right_option"], &[]);
    s.escape.store(true, Ordering::Release);
    // Right after the press: the user's ⌥⎋, which aborts the press and is the app's.
    flags_at(&s, 0, Code::AltRight, RIGHT_ALT);
    assert_eq!(key_at(&s, 100, true, ESC, RIGHT_ALT, false), tell(Hit::Abort { which: 0 }));
    assert_eq!(key_at(&s, 150, false, ESC, RIGHT_ALT, false), Verdict::Pass);
    flags_at(&s, 200, Code::AltRight, 0);
    // Held past the chord window: Escape is taken, the hotkey's own ⌥ notwithstanding.
    flags_at(&s, 1_000, Code::AltRight, RIGHT_ALT);
    let taken = swallow(Hit::Escape);
    assert_eq!(key_at(&s, 2_000, true, ESC, RIGHT_ALT, false), taken);
    assert_eq!(key_at(&s, 2_050, false, ESC, RIGHT_ALT, false), SWALLOWED);
    // Shift too: still taken. With the left ⌥ held too: the user's ⌥⎋.
    let shift = RIGHT_ALT | SHIFT;
    assert_eq!(key_at(&s, 2_080, true, ESC, shift, false), taken);
    assert_eq!(key_at(&s, 2_090, false, ESC, shift, false), SWALLOWED);
    let both = RIGHT_ALT | LEFT_ALT;
    assert_eq!(key_at(&s, 2_100, true, ESC, both, false), Verdict::Pass);
    // ⌥⎋ with another hotkey, or with this one not held, is still the app's.
    let s = with(&["right_cmd"], &[]);
    s.escape.store(true, Ordering::Release);
    assert_eq!(key(&s, true, ESC, RIGHT_ALT, false), Verdict::Pass);
    let s = with(&["right_option"], &[]);
    s.escape.store(true, Ordering::Release);
    assert_eq!(key(&s, true, ESC, LEFT_ALT, false), Verdict::Pass);
    assert_eq!(key(&s, true, ESC, NONE, false), taken, "and plain Escape is taken");
  }

  #[test]
  fn each_slot_is_told_by_which() {
    let s = with(&["f1", "option+space", "right_option", "fn"], &[]);
    assert_eq!(key(&s, true, 0x7A, NONE, false), swallow(down(0)));
    assert_eq!(key(&s, true, SPACE, OPT, false), swallow(down(1)));
    assert_eq!(key(&s, false, 0x7A, NONE, false), swallow(up(0)));
    assert_eq!(key(&s, false, SPACE, OPT, false), swallow(up(1)));
    assert_eq!(flags(&s, Code::AltRight, RIGHT_ALT), tell(down(2)));
    assert_eq!(flags(&s, Code::AltRight, 0), tell(up(2)));
    assert_eq!(flags(&s, Code::Fn, FN), tell(down(3)));
    // Right ⌥ with fn held: fn's chord of the user's, no press of Right ⌥ alone.
    assert_eq!(flags_at(&s, 100, Code::AltRight, FN | RIGHT_ALT), tell(Hit::Abort { which: 3 }));
    assert_eq!(flags_at(&s, 150, Code::AltRight, FN), Verdict::Pass);
    assert_eq!(flags_at(&s, 200, Code::Fn, 0), Verdict::Pass, "aborted: no release");
  }

  #[test]
  fn a_key_while_held_is_taken_and_closes_the_chord_window() {
    let s = with(&["fn"], &["z", "x"]);
    flags_at(&s, 0, Code::Fn, FN);
    // fn-Z within the window: the key, no abort; its repeats and release are swallowed.
    assert_eq!(key_at(&s, 50, true, Z, FN, false), swallow(Hit::Key(0)));
    assert_eq!(key_at(&s, 90, true, Z, FN, true), SWALLOWED);
    // E after it, still within the window: typing, the app's.
    assert_eq!(key_at(&s, 100, true, E, FN, false), Verdict::Pass);
    assert_eq!(key_at(&s, 110, false, E, FN, false), Verdict::Pass);
    // fn up while Z is held: Z's repeats and release are still owed.
    assert_eq!(flags_at(&s, 200, Code::Fn, 0), tell(up(0)));
    assert_eq!(key_at(&s, 250, true, Z, NONE, true), SWALLOWED);
    assert_eq!(key_at(&s, 300, false, Z, NONE, false), SWALLOWED);
    // Every press while held comes: X, then Z again.
    flags_at(&s, 1_000, Code::Fn, FN);
    assert_eq!(key_at(&s, 1_400, true, 0x07, FN, false), swallow(Hit::Key(1)));
    assert_eq!(key_at(&s, 1_450, true, Z, FN, false), swallow(Hit::Key(0)));
    assert_eq!(s.keys_owed.load(Ordering::Relaxed), 0b11);
  }

  #[test]
  fn keys_are_the_apps_unless_a_slot_is_held() {
    let s = with(&["fn"], &["z"]);
    // Z with nothing held passes.
    assert_eq!(key_at(&s, 0, true, Z, NONE, false), Verdict::Pass);
    assert_eq!(key_at(&s, 10, false, Z, NONE, false), Verdict::Pass);
    // Z down before fn stays the app's, its repeats and release too.
    assert_eq!(key_at(&s, 100, true, Z, NONE, false), Verdict::Pass);
    flags_at(&s, 150, Code::Fn, FN);
    assert_eq!(key_at(&s, 600, true, Z, FN, true), Verdict::Pass);
    assert_eq!(key_at(&s, 650, false, Z, FN, false), Verdict::Pass);
    flags_at(&s, 700, Code::Fn, 0);
    // An unconfigured E within the window aborts as before.
    flags_at(&s, 1_000, Code::Fn, FN);
    assert_eq!(key_at(&s, 1_050, true, E, FN, false), tell(Hit::Abort { which: 0 }));
    // A missed fn release, then Z: fn is released and Z is the app's.
    flags_at(&s, 2_000, Code::Fn, 0);
    flags_at(&s, 3_000, Code::Fn, FN);
    assert_eq!(key_at(&s, 3_900, true, Z, NONE, false), tell(up(0)));
    assert_eq!(key_at(&s, 3_950, false, Z, NONE, false), Verdict::Pass);
    // Paused (no keys, no slots): nothing is taken.
    let s = with(&[], &[]);
    assert_eq!(key_at(&s, 0, true, Z, FN, false), Verdict::Pass);
  }

  /// Carbon's release routed before the tap sees the key's: the tap owes nothing then and lets
  /// the release through, and the app hears one release, Carbon's.
  #[test]
  fn carbons_release_first() {
    use super::super::{EPOCH, OnEdge, QUEUE, Raw, Source, route};

    let t0 = *EPOCH.get_or_init(Instant::now);
    let s = with(&["f1"], &[]);
    assert_eq!(key_at(&s, 0, true, 0x7A, NONE, false), swallow(down(0)));
    let (tx, rx) = std::sync::mpsc::sync_channel(QUEUE);
    let ms = |n| t0 + Duration::from_millis(n);
    tx.send(Raw { source: Source::Carbon, edge: down(0), at: ms(1), chord: 0 }).unwrap();
    tx.send(Raw { source: Source::Carbon, edge: up(0), at: ms(300), chord: 0 }).unwrap();
    drop(tx);
    let (got_tx, got) = std::sync::mpsc::channel();
    route(&rx, &(Box::new(move |e| got_tx.send(e).unwrap()) as OnEdge), &s);
    let ups = got.try_iter().filter(|e| matches!(e, crate::hotkey::Edge::Up { .. })).count();
    assert_eq!((ups, s.down.load(Ordering::Relaxed)), (1, 0));
    assert_eq!(key_at(&s, 310, false, 0x7A, NONE, false), Verdict::Pass);
  }

  /// A chord whose release the tap missed (it was disabled) takes no keys once its key is up.
  #[test]
  fn a_stale_chord_takes_no_keys() {
    let s = with(&["f1"], &["z"]);
    s.down.store(0b1, Ordering::Relaxed);
    // No evidence of a missed release (the tap disabled, Secure Input): the HID state is not
    // asked, the chord is held.
    let never = &|_| -> bool { panic!("asked the HID state") };
    let held = judge(&s, slots(&s), KeyKind::Down, Z, NONE, 0, || false, never);
    assert_eq!(held, swallow(Hit::Key(0)));
    key(&s, false, Z, NONE, false);
    // The tap was disabled meanwhile: its key is up, so it is released, its Up told, Z the app's.
    s.suspect.store(0b1, Ordering::Relaxed);
    let up = judge(&s, slots(&s), KeyKind::Down, Z, NONE, 0, || false, &|_| false);
    assert_eq!(up, tell(Hit::Up { which: 0 }));
    assert_eq!(s.down.load(Ordering::Relaxed), 0, "settled");
    // Truly held, it does.
    s.down.store(0b1, Ordering::Relaxed);
    s.suspect.store(0b1, Ordering::Relaxed);
    let held = judge(&s, slots(&s), KeyKind::Down, Z, NONE, 0, || false, &|code| code == 0x7A);
    assert_eq!(held, swallow(Hit::Key(0)));
  }

  /// One event with more than two edges: fn alone released (its flag gone), a chord whose release
  /// was missed settled, and the key taken while another chord is truly held; none dropped.
  #[test]
  fn every_edge_of_an_event_is_kept() {
    let s = with(&["fn", "f1", "f2"], &["z"]);
    flags(&s, Code::Fn, FN);
    s.down.fetch_or(0b110, Ordering::Relaxed);
    s.suspect.store(0b110, Ordering::Relaxed);
    let f2 = |code| code == 0x78;
    let v = judge(&s, slots(&s), KeyKind::Down, Z, NONE, 0, || false, &f2);
    assert_eq!(v, Verdict::Swallow(hits(&[up(0), up(1), Hit::Key(0)])));
    assert_eq!((s.down.load(Ordering::Relaxed), s.suspect.load(Ordering::Relaxed)), (0b100, 0));
    // A fresh press of a chord is no longer suspect.
    key(&s, false, 0x78, NONE, false);
    s.suspect.store(0b100, Ordering::Relaxed);
    assert_eq!(key(&s, true, 0x78, NONE, false), swallow(down(2)));
    assert_eq!(s.suspect.load(Ordering::Relaxed), 0);
  }

  /// A slot switched between a modifier alone and a chord while held: the old press is dropped
  /// with its bit, the new hotkey works, the old key's events are the app's.
  #[test]
  fn a_slot_switching_kind_while_held() {
    let s = with(&["fn"], &[]);
    assert_eq!(flags(&s, Code::Fn, FN), tell(down(0)));
    s.store_slot(0, slot_value(Hotkey::parse("f1").unwrap()).unwrap());
    assert_eq!(flags(&s, Code::Fn, 0), Verdict::Pass, "no release of fn alone owed");
    assert_eq!(key(&s, true, 0x7A, NONE, false), swallow(down(0)));
    s.store_slot(0, slot_value(Hotkey::parse("fn").unwrap()).unwrap());
    assert_eq!(key(&s, false, 0x7A, NONE, false), Verdict::Pass, "F1's release, now the app's");
    assert_eq!(flags(&s, Code::Fn, FN), tell(down(0)));
  }

  #[test]
  fn a_lost_key_release_is_not_owed_past_the_next_press() {
    let s = with(&["fn"], &["z"]);
    flags_at(&s, 0, Code::Fn, FN);
    assert_eq!(key_at(&s, 400, true, Z, FN, false), swallow(Hit::Key(0)));
    flags_at(&s, 500, Code::Fn, 0);
    // Z's release was lost; a fresh Z with nothing held is the app's and owes nothing.
    assert_eq!(key_at(&s, 900, true, Z, NONE, false), Verdict::Pass);
    assert_eq!(s.keys_owed.load(Ordering::Relaxed), 0);
    assert_eq!(key_at(&s, 950, false, Z, NONE, false), Verdict::Pass);
  }

  #[test]
  fn keys_match_with_or_without_the_held_slots_modifiers() {
    // ⌥Space held: ⌥Z is `z`.
    let s = with(&["option+space"], &["z"]);
    key(&s, true, SPACE, OPT, false);
    assert_eq!(key(&s, true, Z, OPT, false), swallow(Hit::Key(0)));
    // fn held: fn-⇧Z is `shift+z`, not `z`; the first match in the list wins.
    let s = with(&["fn"], &["z", "shift+z", "shift+z"]);
    flags(&s, Code::Fn, FN);
    assert_eq!(key_at(&s, 400, true, Z, FN | SHIFT, false), swallow(Hit::Key(1)));
    // Right ⌥ alone held: ⌥Z is `z`; F5 (its events carry fn) is a key too.
    let s = with(&["right_option"], &["z", "f5"]);
    flags(&s, Code::AltRight, RIGHT_ALT);
    assert_eq!(key(&s, true, Z, RIGHT_ALT, false), swallow(Hit::Key(0)));
    assert_eq!(key(&s, true, F5, RIGHT_ALT | FN, false), swallow(Hit::Key(1)));
    // ⌘Z with Right ⌥ held matches neither: the app's.
    assert_eq!(key(&s, true, Z, RIGHT_ALT | CMD, false), Verdict::Pass);
    // A key at kVK 0: A.
    let s = with(&["fn"], &["1", "a"]);
    flags(&s, Code::Fn, FN);
    assert_eq!(key_at(&s, 400, true, A, FN, false), swallow(Hit::Key(1)));
    assert_eq!(key_at(&s, 450, true, ONE, FN, false), swallow(Hit::Key(0)));
  }

  #[test]
  fn a_chord_on_a_held_modifier_drops_it_first() {
    let s = with(&["right_option", "option+space"], &[]);
    assert_eq!(flags(&s, Code::AltRight, RIGHT_ALT), tell(down(0)));
    let both = Verdict::Swallow(hits(&[Hit::Abort { which: 0 }, down(1)]));
    assert_eq!(key_at(&s, 500, true, SPACE, RIGHT_ALT, false), both);
    assert_eq!(key_at(&s, 600, false, SPACE, RIGHT_ALT, false), swallow(up(1)));
    assert_eq!(flags(&s, Code::AltRight, 0), Verdict::Pass, "aborted: no release");
    // A chord without its modifier leaves it held.
    let s = with(&["fn", "f2"], &[]);
    flags(&s, Code::Fn, FN);
    assert_eq!(key_at(&s, 500, true, 0x78, FN, false), swallow(down(1)));
    assert_eq!(s.down.load(Ordering::Relaxed), 0b11);
  }

  #[test]
  fn fn_as_a_chords_modifier() {
    let s = with(&["fn", "fn+1", "f1"], &[]);
    // fn+1 only with the fn flag; F1 with it (the F-row's events carry it) or without.
    assert_eq!(key(&s, true, ONE, NONE, false), Verdict::Pass);
    assert_eq!(key(&s, true, ONE, OPT | FN, false), Verdict::Pass);
    assert_eq!(key(&s, true, 0x7A, FN, false), swallow(down(2)));
    assert_eq!(key(&s, false, 0x7A, FN, false), swallow(up(2)));
    assert_eq!(key(&s, true, 0x7A, NONE, false), swallow(down(2)));
    assert_eq!(key(&s, false, 0x7A, NONE, false), swallow(up(2)));
    // fn alone held, then fn+1: fn alone is dropped and fn+1 pressed, two edges.
    assert_eq!(flags(&s, Code::Fn, FN), tell(down(0)));
    let both = Verdict::Swallow(hits(&[Hit::Abort { which: 0 }, down(1)]));
    assert_eq!(key_at(&s, 50, true, ONE, FN, false), both);
    assert_eq!(key_at(&s, 90, false, ONE, FN, false), swallow(up(1)));
    assert_eq!(flags_at(&s, 120, Code::Fn, 0), Verdict::Pass, "aborted: no release");
  }

  #[test]
  fn escape_with_keys_set() {
    let s = with(&["fn"], &["z"]);
    s.escape.store(true, Ordering::Release);
    flags_at(&s, 0, Code::Fn, FN);
    assert_eq!(key_at(&s, 400, true, Z, FN, false), swallow(Hit::Key(0)));
    assert_eq!(key_at(&s, 500, true, ESC, FN, false), swallow(Hit::Escape));
    assert_eq!(key_at(&s, 550, false, ESC, FN, false), SWALLOWED);
    assert_eq!(key_at(&s, 600, false, Z, FN, false), SWALLOWED);
  }

  /// `set` with the same lists touches nothing ([crate::hotkey]'s diff); another slot stored
  /// meanwhile leaves fn held and Z owed.
  #[test]
  fn storing_another_slot_keeps_the_press_going() {
    let s = with(&["fn"], &["z"]);
    flags_at(&s, 0, Code::Fn, FN);
    assert_eq!(key_at(&s, 400, true, Z, FN, false), swallow(Hit::Key(0)));
    s.store_slot(1, slot_value(Hotkey::parse("f2").unwrap()).unwrap());
    s.store_keys(&[key_value(Hotkey::parse("z").ok())]);
    assert_eq!(key_at(&s, 450, true, Z, FN, true), SWALLOWED);
    assert_eq!(flags_at(&s, 500, Code::Fn, 0), tell(up(0)));
  }

  /// A key taken, then changed or gone (`set`, a pause) before its release: its repeats and
  /// release are still swallowed, by the place it was taken at; the next fresh press is the app's.
  #[test]
  fn a_key_changed_while_owed_still_owes_its_release() {
    let s = with(&["fn"], &["z"]);
    flags(&s, Code::Fn, FN);
    assert_eq!(key(&s, true, Z, FN, false), swallow(Hit::Key(0)));
    s.store_keys(&[key_value(Hotkey::parse("x").ok())]);
    assert_eq!(key(&s, true, Z, FN, true), SWALLOWED);
    // X, set in Z's place, pressed before and released now: the app's, Z still owed.
    assert_eq!(key(&s, false, 0x07, FN, false), Verdict::Pass);
    s.store_keys(&[]);
    assert_eq!(key(&s, false, Z, FN, false), SWALLOWED);
    assert_eq!(key(&s, true, Z, FN, false), Verdict::Pass);
  }

  /// A press judged against a slot the main thread replaced meanwhile holds nothing: its release
  /// would match no slot.
  #[test]
  fn a_press_of_a_slot_replaced_meanwhile_holds_nothing() {
    let s = with(&["f2", "fn"], &[]);
    let (f2, fn_alone) = (s.chords[0].load(Ordering::Relaxed), s.chords[1].load(Ordering::Relaxed));
    s.store_slot(0, slot_value(Hotkey::parse("f3").unwrap()).unwrap());
    s.store_slot(1, 0);
    assert!(!hold(&s, 0, f2) && !hold(&s, 1, fn_alone));
    assert_eq!(s.down.load(Ordering::Relaxed), 0);
    assert!(hold(&s, 0, s.chords[0].load(Ordering::Relaxed)));
    assert_eq!(s.down.load(Ordering::Relaxed), 0b1);
  }
}
