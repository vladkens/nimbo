// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Global hotkeys: chords or modifier keys alone ([Hotkey], as settings spell them) that the app
//! hears while any app has the keyboard ([Hotkeys]: each one's press, autorepeats and release as
//! [Edge]s, on a thread of its own), and keys taken while one is held.
//!
//! An app picks, say, a dictation model from the hotkey in either of two ways, or both at once:
//! - **several hotkeys**: each model has its own (F1 and F2, ⌥Space and ⌃⌥Space), and each edge
//!   says which ([Edge::Down]'s `which`, its index in [Hotkeys::set]);
//! - **keys while held**: while any hotkey is held (fn alone, ⌥Space), a configured key (Z, ⇧Z, 1,
//!   F5) comes as [Edge::Key] and is taken from every app, its press, repeats and release, so a
//!   fast fn-Z never types a z in the app in front ([keys_while_held] says where).
//!
//! | | chords | a modifier alone | release | keys while held | needs |
//! |---|---|---|---|---|---|
//! | macOS | event tap, Carbon `RegisterEventHotKey` as the fallback | event tap | both | event tap | the tap: Accessibility access |
//! | Windows | `RegisterHotKey` | low-level keyboard hook | key state polled | not yet | nothing |
//! | Linux X11 | `XGrabKey` on the root window | no | `KeyRelease` | not yet | nothing |
//! | Linux Wayland | the `GlobalShortcuts` portal (KDE, GNOME 48+, Hyprland) | no | `Deactivated` | not yet | the user's approval |
//!
//! Windows and Linux arm only the first hotkey for now. On X11 another app's grab of the chord is
//! that slot's error. On Wayland the portal binds the shortcut with the chord only as a
//! preference: the desktop asks the user, who may pick another. Without the portal (older GNOME)
//! X11's grab through XWayland stands in, which hears the chord only while an X11 app has the
//! keyboard ([Status::listener_error] says so). [modifier_alone] and [start_asks] tell an app what
//! this platform can do before it asks.
//!
//! [Hotkeys::escape] takes Escape from every app for a while (to cancel what the hotkey started)
//! and reports it as [Edge::Escape]: on macOS the event tap swallows it, and Carbon's hotkey takes
//! it without the tap (no Accessibility access, Secure Input); on Windows the keyboard hook
//! swallows it; on X11 a grab of Escape takes it. The portal can't, so on Wayland Escape stays
//! the focused app's ([Status::escape_error] says so).

use std::time::Instant;

use crate::Modifiers;
use crate::keys::{Hotkey, Shortcut};
use crate::platform::hotkey as imp;

/// The most hotkeys, and the most keys, [Hotkeys::set] takes.
pub const MAX: usize = 8;

/// A modifier key alone can be the hotkey ([Hotkey::Modifier]): on macOS and Windows; Linux's
/// grab and portal take only chords, so the shortcut recorder ([crate::Ui::shortcut]) records none
/// there.
pub fn modifier_alone() -> bool {
  imp::modifier_alone()
}

/// [Hotkeys::start] shows the desktop's dialog: on Linux in a Wayland session, where the
/// GlobalShortcuts portal asks the user to approve the shortcut (a desktop without the portal
/// falls back to X11's grab and asks nothing). An app then starts the listener when the user asks
/// for it, or at a launch after the user approved it once, never unasked at every launch.
pub fn start_asks() -> bool {
  imp::start_asks()
}

/// Keys set with [Hotkeys::set] are taken while a hotkey is held ([Edge::Key]): on macOS (with
/// the event tap); not yet on Windows and Linux, where [Status::keys_error] says so.
pub fn keys_while_held() -> bool {
  imp::keys_while_held()
}

/// What a hotkey or a key did, on the hotkeys' thread:
/// - [Edge::Down]: hotkey `which` (its index in [Hotkeys::set]) pressed, or held (`repeat`, the
///   key's autorepeat), `at` when the key moved; [Edge::Up] its release.
/// - [Edge::Key]: key `index` of [Hotkeys::set]'s `keys` pressed while a hotkey is held, taken with
///   its repeats and release, which come as no edge. Every press comes: an app that picks one model
///   per dictation keeps the first after [Edge::Down] and ignores the rest until [Edge::Up] or
///   [Edge::Aborted].
/// - [Edge::Escape]: Escape pressed while [Hotkeys::escape] is on; the app in front never sees it.
/// - [Edge::Aborted]: hotkey `which`, a modifier key alone, was pressed, then another key within a
///   moment: the user's own chord (⌥E), so what the press started is dropped. Its release comes as
///   no edge. Also a hotkey changed, removed or paused while held (macOS).
///
/// Exhaustive on purpose: an app's hotkey state machine should fail to build on a new edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
  Down { which: usize, at: Instant, repeat: bool },
  Up { which: usize, at: Instant },
  Key { index: usize, at: Instant },
  Escape,
  Aborted { which: usize },
}

/// The system-wide key source [Hotkeys::start] starts: macOS's event tap, Windows' low-level
/// keyboard hook, Linux's X connection or portal session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Listener {
  /// Not started.
  #[default]
  Off,
  /// [Hotkeys::start] started it; it is active or failed a moment later.
  Starting,
  Active,
  /// It could not be made ([Status::listener_error]): [Hotkeys::start] again.
  Failed,
}

/// One hotkey of [Hotkeys::set], at its index.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Slot {
  pub hotkey: Hotkey,
  /// Why it is not armed: refused by `set`, or by the OS (another app has it).
  pub error: Option<String>,
  /// The source that works without the listener holds it (macOS: Carbon's hotkey, which also
  /// works under Secure Input; Windows: `RegisterHotKey`).
  pub fallback: bool,
  pub fallback_error: Option<String>,
}

/// What [Hotkeys] does now, for a status line or diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Status {
  /// The hotkeys set, by index (armed, or waiting for the pause to end, unless in error).
  pub slots: Vec<Slot>,
  /// The keys armed, taken while a hotkey is held unless [Status::keys_error] says why not.
  pub keys: Vec<Hotkey>,
  /// A key `set` left out, or why the keys are not taken now (no listener, Secure Input, this
  /// platform), or a warning (taken while a lone ⌥, ⌘ or ⇧ is held, they may clash with typing).
  pub keys_error: Option<String>,
  pub listener: Listener,
  /// Why the listener last failed.
  pub listener_error: Option<String>,
  /// Every armed slot has a working fallback ([Slot::fallback]); none armed, none.
  pub fallback: bool,
  pub paused: bool,
  /// Escape is taken now: [Hotkeys::escape] is on and a source here has it.
  pub escape: bool,
  /// Why Escape is on but not taken: the platform can't (Linux on Wayland) or no source is up.
  pub escape_error: Option<String>,
  /// Some app keeps key presses from the listener (macOS Secure Input: a password field, a
  /// terminal's secure keyboard entry); the fallback still hears chords.
  pub secure_input: bool,
  pub counts: Counts,
}

/// Events seen since the start.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Counts {
  pub listener: u64,
  pub fallback: u64,
  /// Presses the fallback repeated while the key was held.
  pub fallback_repeats: u64,
  /// Edges both sources reported, the second dropped.
  pub duplicates: u64,
  /// Times the OS disabled the listener (a slow callback) and it was enabled again.
  pub reenabled: u64,
  /// Edges lost to a full queue.
  pub overflow: u64,
  /// Keys told ([Edge::Key]).
  pub keys: u64,
}

pub(crate) type OnEdge = Box<dyn Fn(Edge) + Send>;
type Slots = [Option<Hotkey>; MAX];

/// The app's global hotkeys: one set per app, made and used on the main thread while
/// [crate::app] runs. `on` hears their edges on a thread of its own, never the main one, so it
/// can hand them to a worker at once (or to the main thread through a [crate::Proxy]). Dropped,
/// the hotkeys stop; the threads stay for the process.
pub struct Hotkeys {
  imp: imp::Hotkeys,
  book: Book,
  escape: bool,
}

/// What [Hotkeys] keeps of `set` and `pause`, and what the platform holds: apart from the
/// platform ([Arm]), so tests drive it with a fake.
#[derive(Default)]
struct Book {
  hotkeys: Vec<Hotkey>,
  /// Why `set` refused each hotkey; the keys it took (none where refused), and why it refused
  /// the first key it did.
  invalid: Vec<Option<String>>,
  keys: Vec<Option<Hotkey>>,
  left_out: Option<String>,
  /// What the platform holds per slot (none where it refused, so it is tried again), and why it
  /// refused.
  armed: Slots,
  refused: [Option<String>; MAX],
  armed_keys: Vec<Option<Hotkey>>,
  paused: bool,
}

/// The platform's side of [Book::arm].
trait Arm {
  fn set_slot(&mut self, which: usize, hotkey: Option<Hotkey>) -> Result<(), String>;
  fn set_keys(&mut self, keys: &[Option<Hotkey>]);
}

impl Arm for imp::Hotkeys {
  fn set_slot(&mut self, which: usize, hotkey: Option<Hotkey>) -> Result<(), String> {
    imp::Hotkeys::set_slot(self, which, hotkey)
  }

  fn set_keys(&mut self, keys: &[Option<Hotkey>]) {
    imp::Hotkeys::set_keys(self, keys);
  }
}

impl Hotkeys {
  /// No hotkey until [Hotkeys::set]; the listener after [Hotkeys::start].
  pub fn new(on: impl Fn(Edge) + Send + 'static) -> Self {
    Self { imp: imp::Hotkeys::new(Box::new(on)), book: Book::default(), escape: false }
  }

  /// Uses `hotkeys` (each told by its index) and `keys` (taken while one is held, [Edge::Key]),
  /// as [Hotkey::parse] reads them; `set(&[], &[])` clears. Refused each alone, the rest taken:
  /// more than [MAX] of either, one given twice, a key that is a modifier alone or a chord
  /// hotkey's own key; a hotkey the OS can't take (another app has it, a key this platform can't
  /// see alone) is its slot's error ([Slot::error]). The `Err` names the first problem.
  ///
  /// Cheap to call on every settings change: only what changed reaches the OS, so a held
  /// hotkey stays held through the same lists again; a hotkey the OS refused is tried again.
  /// While paused they are taken once the pause ends.
  pub fn set(&mut self, hotkeys: &[Hotkey], keys: &[Hotkey]) -> Result<(), String> {
    self.book.set(hotkeys, keys, &mut self.imp)
  }

  /// While a shortcut recorder records a new hotkey, the hotkeys and keys are not matched: their
  /// keys reach the window.
  pub fn pause(&mut self, paused: bool) {
    self.book.paused = paused;
    self.book.arm(&mut self.imp);
  }

  /// While on, Escape (alone or with Shift, with Shift not through macOS's fallback; a modifier
  /// hotkey held past its chord window aside) is taken from every app and comes as [Edge::Escape],
  /// once per press; its repeats and release are swallowed too. A press that began before stays
  /// the app's. Whether it is taken is in [Status::escape]: macOS needs the listener or Carbon's
  /// hotkey, Windows the listener, Linux X11's grab (with a hotkey set); Wayland can't.
  pub fn escape(&mut self, on: bool) {
    if self.escape != on {
      self.escape = on;
      self.imp.escape(on);
    }
  }

  /// Starts the listener, unless it runs or is starting; again after it failed. Returns at once:
  /// whether it came up is in [Hotkeys::status] a moment later; the error here is only one
  /// starting it. macOS: the event tap needs Accessibility access (System Settings → Privacy &
  /// Security), chords work without it through the fallback; Linux: on Wayland the portal asks
  /// the user to approve the shortcut, on X11 the chord is grabbed already.
  pub fn start(&mut self) -> Result<(), String> {
    self.imp.start()
  }

  pub fn status(&self) -> Status {
    let mut s = self.book.status();
    self.imp.status(&mut s);
    finish(&mut s, self.escape, self.book.left_out.clone());
    s
  }
}

impl Book {
  fn set(&mut self, hotkeys: &[Hotkey], keys: &[Hotkey], imp: &mut impl Arm) -> Result<(), String> {
    // A refusal is the hotkey's it was for: another one in its slot is tried afresh.
    for i in (0..MAX).filter(|&i| self.hotkeys.get(i) != hotkeys.get(i)) {
      self.refused[i] = None;
    }

    let (invalid, bad) = check(hotkeys, keys);
    self.keys = keys.iter().zip(&bad).take(MAX).map(|(&k, e)| e.is_none().then_some(k)).collect();
    (self.hotkeys, self.invalid, self.left_out) =
      (hotkeys.to_vec(), invalid, bad.into_iter().flatten().next());

    self.arm(imp);

    let first = self.status().slots.into_iter().find_map(|s| s.error);
    first.or(self.left_out.clone()).map_or(Ok(()), Err)
  }

  /// Hands the platform what changed: each slot alone, the keys together; none while paused.
  fn arm(&mut self, imp: &mut impl Arm) {
    let ok = |i: usize| !self.paused && self.invalid[i].is_none();
    let want = std::array::from_fn(|i| self.hotkeys.get(i).copied().filter(|_| ok(i)));
    for (i, hotkey) in diff(&self.armed, &want) {
      self.refused[i] = imp.set_slot(i, hotkey).err();
      self.armed[i] = hotkey.filter(|_| self.refused[i].is_none());
    }

    let keys = if self.paused { Vec::new() } else { self.keys.clone() };
    if keys != self.armed_keys {
      imp.set_keys(&keys);
      self.armed_keys = keys;
    }
  }

  /// The slots and keys as `set` and the platform left them.
  fn status(&self) -> Status {
    let error = |i: usize| self.invalid[i].clone().or_else(|| self.refused[i].clone());
    let slot =
      |(i, &hotkey)| Slot { hotkey, error: error(i), fallback: false, fallback_error: None };
    let slots = self.hotkeys.iter().enumerate().map(slot).collect();
    let keys = self.armed_keys.iter().flatten().copied().collect();
    Status { slots, keys, paused: self.paused, ..Status::default() }
  }
}

/// Why `set` refuses each of `hotkeys` and of `keys`; `None` for the ones it takes.
fn check(hotkeys: &[Hotkey], keys: &[Hotkey]) -> (Vec<Option<String>>, Vec<Option<String>>) {
  let own =
    |k: Shortcut| hotkeys.iter().any(|h| matches!(h, Hotkey::Shortcut(h) if h.code() == k.code()));
  let why = |list: &[Hotkey], i: usize, what: &str| match list[i] {
    _ if i >= MAX => Some(format!("more than {MAX} {what}s")),
    k if list[..i].contains(&k) => Some(format!("{k} is set twice")),
    k @ Hotkey::Modifier(_) if what == "key" => Some(format!("{k} is a modifier alone, no key")),
    Hotkey::Shortcut(k) if what == "key" && k.mods.contains(Modifiers::FN) => {
      Some(format!("{k}: fn is in no key"))
    }
    Hotkey::Shortcut(k) if what == "key" && own(k) => Some(format!("{k} is on a hotkey's own key")),
    _ => None,
  };
  let hot = (0..hotkeys.len()).map(|i| why(hotkeys, i, "hotkey")).collect();
  (hot, (0..keys.len()).map(|i| why(keys, i, "key")).collect())
}

/// The platform's calls from `armed` to `want`, slot by slot: an unchanged slot is left alone
/// (one refused is held as none, so it is tried again); changed and removed ones are cleared
/// first, so a hotkey moving to another slot is never held twice, then armed.
fn diff(armed: &Slots, want: &Slots) -> Vec<(usize, Option<Hotkey>)> {
  let changed = |&i: &usize| armed[i] != want[i];
  let clear = (0..MAX).filter(changed).filter(|&i| armed[i].is_some()).map(|i| (i, None));
  clear.chain((0..MAX).filter(changed).filter_map(|i| Some((i, Some(want[i]?))))).collect()
}

/// The status's parts over the platform's: [Status::fallback] over the armed slots, Escape taken,
/// and a key `set` left out (`left_out`) first among the keys' problems.
fn finish(s: &mut Status, escape: bool, left_out: Option<String>) {
  // The least of the armed slots' (false before true), none without one.
  let armed = s.slots.iter().filter(|x| x.error.is_none() && !s.paused);
  s.fallback = armed.map(|x| x.fallback).min() == Some(true);
  s.escape = escape && s.escape_error.is_none();
  s.keys_error = left_out.or(s.keys_error.take());
}

/// What a source does with a key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(target_os = "linux", allow(dead_code, reason = "X11's keys take it later"))]
pub(crate) enum Take {
  Pass,
  /// An autorepeat or the release of a key claimed: swallowed.
  Swallow,
  /// A fresh press claimed: swallowed, and told.
  Claim,
}

/// The one rule for a key a source takes from every app: a fresh press that is `ours` is claimed
/// and the key `owed` until its release; its autorepeats and the release are swallowed, whatever
/// the modifiers are by then. An autorepeat of a press the app saw stays the app's; any fresh
/// press settles who owns the key, so a lost release is not owed for long. `down`: a press or an
/// autorepeat (`repeat`), else the release. The key's new owed state, and what to do.
#[cfg_attr(target_os = "linux", allow(dead_code, reason = "X11's keys take it later"))]
pub(crate) fn owe(owed: bool, down: bool, repeat: bool, ours: bool) -> (bool, Take) {
  let swallow = if owed { Take::Swallow } else { Take::Pass };
  match (down, repeat) {
    (false, _) => (false, swallow),
    (true, true) => (owed, swallow),
    (true, false) => (ours, if ours { Take::Claim } else { Take::Pass }),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn hk(text: &str) -> Hotkey {
    Hotkey::parse(text).unwrap()
  }

  #[test]
  fn set_refuses_each_alone() {
    let hot = [hk("f1"), hk("option+space"), hk("f1"), hk("fn")];
    let keys = [hk("z"), hk("right_option"), hk("space"), hk("z"), hk("shift+z"), hk("a")];
    let (h, k) = check(&hot, &keys);
    let bad = |v: &[Option<String>]| v.iter().map(Option::is_some).collect::<Vec<_>>();
    assert_eq!(bad(&h), [false, false, true, false], "{h:?}");
    assert!(h[2].as_deref().is_some_and(|e| e.contains("twice")));
    assert_eq!(bad(&k), [false, true, true, true, false, false], "{k:?}");
    assert!(k[1].as_deref().is_some_and(|e| e.contains("modifier")));
    assert!(k[2].as_deref().is_some_and(|e| e.contains("own key")), "Space is ⌥Space's");
    let many: Vec<Hotkey> = (1..=10).map(|n| hk(&format!("f{n}"))).collect();
    let letters: Vec<Hotkey> = "abcdefghij".chars().map(|c| hk(&c.to_string())).collect();
    let (h, k) = check(&many, &letters);
    assert_eq!(bad(&h)[7..], [false, true, true]);
    assert_eq!(bad(&k)[7..], [false, true, true]);
    assert!(h[8].as_deref().is_some_and(|e| e.contains("more than 8")));
    let (_, k) = check(&[], &[hk("fn+z")]);
    assert!(k[0].as_deref().is_some_and(|e| e.contains("fn is in no key")));
    // A key on a lone hotkey's modifier is no chord's own key: F-keys as keys with fn alone.
    let (_, k) = check(&[hk("fn")], &[hk("f5"), hk("a")]);
    assert_eq!(bad(&k), [false, false]);
  }

  fn slots(list: &[&str]) -> Slots {
    std::array::from_fn(|i| list.get(i).filter(|t| !t.is_empty()).map(|t| hk(t)))
  }

  #[test]
  fn the_diff_touches_only_what_changed() {
    let had = slots(&["fn", "f1", "f2"]);
    assert_eq!(diff(&had, &had), [], "the same lists: nothing");
    // F1 changed alone, F2 removed alone; fn untouched.
    let f3 = Some(hk("f3"));
    assert_eq!(diff(&had, &slots(&["fn", "f3"])), [(1, None), (2, None), (1, f3)]);
    // A refused slot, held as none, is tried again with the same hotkey; gone, nothing to clear.
    let refused = slots(&["fn", "", "f2"]);
    assert_eq!(diff(&refused, &had), [(1, Some(hk("f1")))]);
    assert_eq!(diff(&refused, &slots(&["fn", "", "f2"])), []);
    // A hotkey moving to another slot is cleared from the first before it is armed again.
    let moved = diff(&slots(&["f1", "f2"]), &slots(&["f2"]));
    assert_eq!(moved, [(0, None), (1, None), (0, Some(hk("f2")))]);
    assert_eq!(diff(&slots(&[]), &slots(&["f1"])), [(0, Some(hk("f1")))]);
  }

  /// The platform as [Book::arm] sees it: the calls it gets, and F2 refused (another app has it).
  #[derive(Default)]
  struct Fake(Vec<String>);

  impl Arm for Fake {
    fn set_slot(&mut self, which: usize, hotkey: Option<Hotkey>) -> Result<(), String> {
      self.0.push(format!("{which}={}", hotkey.map_or("-".into(), |h| h.to_string())));
      if hotkey == Some(hk("f2")) { Err("taken".into()) } else { Ok(()) }
    }

    fn set_keys(&mut self, keys: &[Option<Hotkey>]) {
      let keys: Vec<String> =
        keys.iter().map(|k| k.map_or("-".into(), |k| k.to_string())).collect();
      self.0.push(format!("keys {}", keys.join(" ")));
    }
  }

  #[test]
  fn set_and_pause_hand_over_only_what_changed() {
    let (mut b, mut f) = (Book::default(), Fake::default());
    let (hot, keys) = ([hk("fn"), hk("f2")], [hk("z"), hk("right_option"), hk("x")]);
    assert_eq!(b.set(&hot, &keys, &mut f), Err("taken".into()), "F2's refusal, then the key's");
    assert_eq!(f.0, ["0=fn", "1=f2", "keys z - x"]);
    let s = b.status();
    assert_eq!(
      s.slots.iter().map(|s| s.error.clone()).collect::<Vec<_>>(),
      [None, Some("taken".into())]
    );
    assert_eq!(s.keys, [hk("z"), hk("x")]);
    // The same lists: only the refused slot again.
    f.0.clear();
    _ = b.set(&hot, &keys, &mut f);
    assert_eq!(f.0, ["1=f2"]);
    // Paused: every slot cleared and no keys; resumed: armed again, F2 tried again.
    f.0.clear();
    b.paused = true;
    b.arm(&mut f);
    assert_eq!(f.0, ["0=-", "keys "]);
    assert!(b.status().keys.is_empty());
    // Known, cosmetic: the refusal from before the pause is still shown while paused.
    assert_eq!(b.status().slots[1].error.as_deref(), Some("taken"));
    f.0.clear();
    b.paused = false;
    b.arm(&mut f);
    assert_eq!(f.0, ["0=fn", "1=f2", "keys z - x"]);
    // F2 gone, the keys the same: slot 1 alone, nothing to clear as it was refused.
    f.0.clear();
    assert_eq!(b.set(&hot[..1], &keys, &mut f), Err(b.left_out.clone().unwrap()));
    assert!(f.0.is_empty(), "{:?}", f.0);

    // The refused F2 replaced while paused: its refusal goes with it.
    let mut b = Book::default();
    _ = b.set(&hot, &[], &mut f);
    b.paused = true;
    b.arm(&mut f);
    assert_eq!(b.set(&[hk("fn"), hk("f3")], &[], &mut f), Ok(()));
  }

  #[test]
  fn owed_keys() {
    use Take::*;
    // A fresh press of ours: claimed, its repeats and release swallowed.
    assert_eq!(owe(false, true, false, true), (true, Claim));
    assert_eq!(owe(true, true, true, false), (true, Swallow), "ours whatever it is now");
    assert_eq!(owe(true, false, false, false), (false, Swallow));
    // The app's press: its repeats stay the app's, even when they match now.
    assert_eq!(owe(false, true, false, false), (false, Pass));
    assert_eq!(owe(false, true, true, true), (false, Pass));
    assert_eq!(owe(false, false, false, true), (false, Pass));
    // A lost release: the next fresh press settles it.
    assert_eq!(owe(true, true, false, false), (false, Pass));
  }

  #[test]
  fn the_status_over_the_platforms() {
    let slot = |fallback, error: Option<&str>| Slot {
      hotkey: hk("f1"),
      error: error.map(Into::into),
      fallback,
      fallback_error: None,
    };
    let mut s =
      Status { slots: vec![slot(true, None), slot(false, Some("taken"))], ..Status::default() };
    s.keys_error = Some("no event tap".into());
    finish(&mut s, true, None);
    assert!(s.fallback && s.escape, "a slot in error is not armed");
    assert_eq!(s.keys_error.as_deref(), Some("no event tap"));
    s.slots.push(slot(false, None));
    finish(&mut s, false, Some("z is set twice".into()));
    assert!(!s.fallback && !s.escape);
    assert_eq!(s.keys_error.as_deref(), Some("z is set twice"));
    s.slots.clear();
    s.escape_error = Some("no".into());
    finish(&mut s, true, None);
    assert!(!s.fallback && !s.escape, "none armed: no fallback");
  }
}
