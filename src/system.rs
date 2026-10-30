// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Changes in the system an app may show, on one channel: the kinds an app watches
//! ([Main::watch]), told by the OS from any thread, coalesced per kind and delivered on the main
//! thread to the app's hook, then to every open window as [Event::System].
//!
//! A change says what to read again (the audio devices through the app's audio library,
//! [crate::hardware::transport], [crate::hardware::lid_closed]), not the new value; it can come
//! when nothing the app shows changed (an output came, a monitor source), so it is read again,
//! never trusted to differ. A burst of one kind (a headset connecting) comes as one change, 250 ms
//! after its last notice or 1 s after its first; kinds due together come one by one, in
//! [Change]'s order. The OS's light or dark look is not here: it is a value the runtime keeps
//! itself ([Event::Appearance]).
//!
//! An app with a tray and windows: the hook updates what lives outside the windows (the tray's
//! menu, a model the windows read), then each window's [App::event] reads its own data again on
//! the kinds it shows and returns true to redraw. A window busy up the stack (a nested loop: a
//! dialog, a paste waiting) gets the event once it is free; nothing is dropped.
//!
//! ```no_run
//! use nimbo::system::Change;
//! use nimbo::{App, AppInfo, Cx, Event, Presence};
//!
//! struct Recorder {
//!   lid: Option<bool>,
//! }
//!
//! impl App for Recorder {
//!   fn event(&mut self, _cx: &mut Cx, ev: &Event) -> bool {
//!     match ev {
//!       Event::System(Change::Lid) => {
//!         self.lid = nimbo::hardware::lid_closed();
//!         true
//!       }
//!       // The devices read again (cpal…).
//!       Event::System(Change::AudioInputs | Change::DefaultInput) => true,
//!       _ => false,
//!     }
//!   }
//! }
//!
//! const INFO: AppInfo =
//!   AppInfo { id: "com.example.rec", name: "Rec", presence: Presence::Accessory };
//! nimbo::app(INFO, |main| {
//!   let kinds = [Change::AudioInputs, Change::DefaultInput, Change::Lid];
//!   // The tray's menu built again here; the windows hear it after.
//!   if let Err(e) = main.watch(&kinds, |_main, change| eprintln!("changed: {change:?}")) {
//!     eprintln!("system: {e}");
//!   }
//! });
//! ```
//!
//! | | audio inputs, default input | lid |
//! |---|---|---|
//! | macOS | CoreAudio's property listeners on the system object | IOPMrootDomain's clamshell message |
//! | Windows | `IMMNotificationClient` | `GUID_LIDSWITCH_STATE_CHANGE` |
//! | Linux | PulseAudio's subscription (PipeWire through `pipewire-pulse`) | UPower's `LidIsClosed` |

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::platform::hardware as imp;
use crate::shell::Proxy;
#[cfg(doc)]
use crate::{App, Event, Main};

/// A kind of change in the system: what to read again. More kinds come (displays, power), so a
/// `match` on it keeps a `_` arm.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Change {
  /// An audio input came or went (a microphone plugged, a Bluetooth headset connected); outputs
  /// may tell too.
  AudioInputs,
  /// The system's default audio input changed.
  DefaultInput,
  /// The laptop's lid opened or closed ([crate::hardware::lid_closed]).
  Lid,
}

const KINDS: usize = 3;

impl Change {
  const ALL: [Change; KINDS] = [Change::AudioInputs, Change::DefaultInput, Change::Lid];

  fn bit(self) -> u32 {
    1 << self as u32
  }
}

/// A burst ends when its kind is quiet this long…
const SETTLE: Duration = Duration::from_millis(250);
/// …or this long after it began.
const CAP: Duration = Duration::from_secs(1);
/// A window busy up the stack is tried again after this.
pub(crate) const RETRY: Duration = Duration::from_millis(100);

/// Whether a burst that began at `first` and was last told at `last` is delivered at `now`
/// (nanoseconds), else how long it waits.
fn due(first: u64, last: u64, now: u64) -> Result<(), Duration> {
  let at = (last + SETTLE.as_nanos() as u64).min(first + CAP.as_nanos() as u64);
  match now >= at {
    true => Ok(()),
    false => Err(Duration::from_nanos(at - now)),
  }
}

/// The kinds watched and those told, with each kind's burst, all atomics: the OS tells from its
/// own threads, which must never wait (Windows' audio callbacks). One instance runs the app
/// ([WATCHER]); tests make their own.
pub(crate) struct Watcher {
  watched: AtomicU32,
  pending: AtomicU32,
  /// A job to deliver them is posted.
  scheduled: AtomicBool,
  first: [AtomicU64; KINDS],
  last: [AtomicU64; KINDS],
}

impl Watcher {
  pub(crate) const fn new() -> Self {
    Self {
      watched: AtomicU32::new(0),
      pending: AtomicU32::new(0),
      scheduled: AtomicBool::new(false),
      first: [const { AtomicU64::new(0) }; KINDS],
      last: [const { AtomicU64::new(0) }; KINDS],
    }
  }

  /// `kind` was told at `now`: true when the caller posts a job to deliver it. An unwatched
  /// kind is ignored. A burst's first notice stores its start after its bit is set, so a job
  /// between the two reads the last burst's start and delivers early: never late, never lost.
  pub(crate) fn notice(&self, kind: Change, now: u64) -> bool {
    let (bit, i) = (kind.bit(), kind as usize);
    if self.watched.load(Ordering::Acquire) & bit == 0 {
      return false;
    }

    self.last[i].store(now, Ordering::Release);
    if self.pending.fetch_or(bit, Ordering::AcqRel) & bit != 0 {
      return false;
    }

    self.first[i].store(now, Ordering::Release);
    !self.scheduled.swap(true, Ordering::AcqRel)
  }

  /// The job at `now`: the kinds due, in order, their bits cleared first (a notice during their
  /// delivery starts a new burst), and how long until the next job when the caller posts one.
  pub(crate) fn take(&self, now: u64) -> (Vec<Change>, Option<Duration>) {
    self.scheduled.store(false, Ordering::Release);
    let pending = self.pending.load(Ordering::Acquire);
    let (mut ready, mut wait) = (Vec::new(), None::<Duration>);
    for kind in Change::ALL.into_iter().filter(|k| pending & k.bit() != 0) {
      let i = kind as usize;
      let (first, last) =
        (self.first[i].load(Ordering::Acquire), self.last[i].load(Ordering::Acquire));
      match due(first, last, now) {
        Ok(()) => {
          self.pending.fetch_and(!kind.bit(), Ordering::AcqRel);
          ready.push(kind);
        }
        Err(d) => wait = Some(wait.map_or(d, |w| w.min(d))),
      }
    }

    // A notice since may have posted one already.
    let wait = wait.filter(|_| !self.scheduled.swap(true, Ordering::AcqRel));
    (ready, wait)
  }

  /// Starts telling `kind`, nothing pending of it from before.
  fn watch(&self, kind: Change) {
    self.pending.fetch_and(!kind.bit(), Ordering::AcqRel);
    self.watched.fetch_or(kind.bit(), Ordering::AcqRel);
  }

  fn unwatch(&self, kind: Change) {
    self.watched.fetch_and(!kind.bit(), Ordering::AcqRel);
    self.pending.fetch_and(!kind.bit(), Ordering::AcqRel);
  }

  fn watching(&self, kind: Change) -> bool {
    self.watched.load(Ordering::Acquire) & kind.bit() != 0
  }
}

static WATCHER: Watcher = Watcher::new();
static EPOCH: OnceLock<Instant> = OnceLock::new();
/// Each notice to stderr (`NIMBO_TRACE`), and whether it came on the main thread: for checks,
/// not for apps (it takes stderr's lock, which the OS's callbacks shouldn't).
static TRACE: AtomicBool = AtomicBool::new(false);

/// Nanoseconds since the first watch.
fn now() -> u64 {
  EPOCH.get().map_or(0, |e| e.elapsed().as_nanos() as u64)
}

/// The OS told `kind`, from any thread: atomics and, once a burst, a post to the main thread.
pub(crate) fn notice(kind: Change) {
  if TRACE.load(Ordering::Relaxed) && WATCHER.watching(kind) {
    let main = crate::platform::shell::is_main();
    eprintln!("nimbo: system raw {kind:?}{}", if main { " (main thread)" } else { "" });
  }

  if WATCHER.notice(kind, now()) {
    Proxy(()).after(SETTLE, job);
  }
}

/// Delivers the kinds due, then posts itself again for the rest.
fn job(main: &mut crate::Main) {
  let (ready, wait) = WATCHER.take(now());
  if let Some(wait) = wait {
    Proxy(()).after(wait, job);
  }

  for kind in ready {
    crate::shell::changed(main, kind);
  }
}

/// Watches each of `kinds` not watched yet, on the main thread; `Err` names those the OS
/// refused, the rest watched.
pub(crate) fn watch(kinds: &[Change]) -> Result<(), String> {
  EPOCH.get_or_init(Instant::now);
  TRACE.store(std::env::var_os("NIMBO_TRACE").is_some(), Ordering::Relaxed);
  let mut errors = Vec::new();
  for &kind in kinds.iter().filter(|k| !WATCHER.watching(**k)) {
    // Watched first: the OS may tell as it registers.
    WATCHER.watch(kind);
    let at = Instant::now();
    if let Err(e) = imp::watch(kind) {
      WATCHER.unwatch(kind);
      errors.push(format!("{kind:?}: {e}"));
    }

    if TRACE.load(Ordering::Relaxed) {
      eprintln!("nimbo: system watch {kind:?} in {:.1} ms", at.elapsed().as_secs_f64() * 1e3);
    }
  }

  match errors.is_empty() {
    true => Ok(()),
    false => Err(errors.join("; ")),
  }
}

/// Stops watching everything: the loop ends.
pub(crate) fn unwatch() {
  if Change::ALL.iter().any(|k| WATCHER.watching(*k)) {
    imp::unwatch();
  }

  for kind in Change::ALL {
    WATCHER.unwatch(kind);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const MS: u64 = 1_000_000;

  fn watcher(kinds: &[Change]) -> Watcher {
    let w = Watcher::new();
    for &k in kinds {
      w.watch(k);
    }

    w
  }

  #[test]
  fn a_burst_ends_quiet_or_capped() {
    assert_eq!(due(0, 0, 250 * MS), Ok(()));
    assert_eq!(due(0, 0, 100 * MS), Err(Duration::from_millis(150)));
    assert_eq!(due(0, 900 * MS, 1000 * MS), Ok(()), "1 s after its first");
    assert_eq!(due(0, 700 * MS, 900 * MS), Err(Duration::from_millis(50)));
  }

  /// Ten notices within 250 ms post one job and give one change after the last is quiet.
  #[test]
  fn a_burst_gives_one_change() {
    let w = watcher(&[Change::Lid]);
    let posts = (0..10).filter(|i| w.notice(Change::Lid, i * 20 * MS)).count();
    assert_eq!(posts, 1);
    assert_eq!(w.take(250 * MS), (vec![], Some(Duration::from_millis(180))));
    assert_eq!(w.take(430 * MS), (vec![Change::Lid], None));
    assert_eq!(w.take(500 * MS), (vec![], None), "delivered once");
  }

  /// Two kinds in one burst come one by one, in the enum's order.
  #[test]
  fn kinds_come_apart_in_order() {
    let w = watcher(&[Change::AudioInputs, Change::DefaultInput]);
    assert!(w.notice(Change::DefaultInput, 0));
    assert!(!w.notice(Change::AudioInputs, 10 * MS), "one job for both");
    assert_eq!(w.take(300 * MS).0, [Change::AudioInputs, Change::DefaultInput]);
  }

  /// Notices every 200 ms never go quiet: the burst is delivered 1 s after it began.
  #[test]
  fn a_long_burst_is_capped() {
    let w = watcher(&[Change::AudioInputs]);
    for i in 0..=4 {
      w.notice(Change::AudioInputs, i * 200 * MS);
    }

    assert_eq!(w.take(999 * MS).0, []);
    assert_eq!(w.take(1000 * MS).0, [Change::AudioInputs]);
  }

  /// An unwatched kind is ignored; watching again drops what was pending from before.
  #[test]
  fn only_watched_kinds_are_told() {
    let w = watcher(&[Change::Lid]);
    assert!(!w.notice(Change::AudioInputs, 0));
    assert!(w.notice(Change::Lid, 0));
    w.unwatch(Change::Lid);
    w.watch(Change::Lid);
    assert_eq!(w.take(SETTLE.as_nanos() as u64).0, []);
  }

  /// A notice while its kind is delivered (its bit already cleared) starts a new burst and a
  /// new job.
  #[test]
  fn a_notice_during_delivery_starts_a_burst() {
    let w = watcher(&[Change::Lid]);
    w.notice(Change::Lid, 0);
    assert_eq!(w.take(250 * MS).0, [Change::Lid]);
    assert!(w.notice(Change::Lid, 260 * MS));
    assert_eq!(w.take(510 * MS).0, [Change::Lid]);
  }
}
