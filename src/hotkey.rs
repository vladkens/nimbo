// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Global hotkeys: chords or modifier keys alone ([Hotkey], as settings spell them) that the app
//! hears while any app has the keyboard ([Hotkeys]: up to [MAX] at once, each one's press,
//! autorepeats and release as [Edge]s saying which, on a thread of its own).
//!
//! | | chords | a modifier alone | release | needs |
//! |---|---|---|---|---|
//! | macOS | event tap, Carbon `RegisterEventHotKey` as the fallback | event tap | both | the tap: Accessibility access |
//! | Windows | `RegisterHotKey` | low-level keyboard hook | key state polled | nothing |
//! | Linux X11 | `XGrabKey` on the root window | no | `KeyRelease` | nothing |
//! | Linux Wayland | the `GlobalShortcuts` portal (KDE, GNOME 48+, Hyprland) | no | `Deactivated` | the user's approval |
//!
//! On X11 another app's grab of a chord is its place's error ([Slot::error]). On Wayland the portal
//! binds the shortcuts with the chords only as a preference: the desktop asks the user, who may
//! pick others, and asks again when they change. Without the portal (older GNOME) X11's grab
//! through XWayland stands in, which hears the chord only while an X11 app has the keyboard
//! ([Status::listener_error] says so). [modifier_alone] and [start_asks] tell an app what this
//! platform can do before it asks.
//!
//! [Hotkeys::escape] takes Escape from every app for a while (to cancel what the hotkey started)
//! and reports it as [Edge::Escape]: on macOS the event tap swallows it, and Carbon's hotkey takes
//! it without the tap (no Accessibility access, Secure Input); on Windows the keyboard hook
//! swallows it; on X11 a grab of Escape takes it. The portal can't, so on Wayland Escape stays
//! the focused app's ([Status::escape_error] says so).

use std::time::Instant;

use crate::keys::Hotkey;
use crate::platform::hotkey as imp;

/// The most hotkeys [Hotkeys::set] takes.
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

/// What a hotkey did, on the hotkeys' thread; `which` is its place in [Hotkeys::set].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
  /// Pressed, or held (`repeat`, the key's autorepeat), `at` when the key moved.
  Down {
    which: usize,
    at: Instant,
    repeat: bool,
  },
  Up {
    which: usize,
    at: Instant,
  },
  /// Escape pressed while [Hotkeys::escape] is on; the app in front never sees it.
  Escape,
  /// A modifier key alone was pressed, then another key within a moment: the user's own chord
  /// (⌥E), so what the press started is dropped. Its release comes as no edge.
  Aborted {
    which: usize,
  },
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

/// A place of [Hotkeys::set] as it is now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Slot {
  /// The hotkey armed (or waiting for the pause to end); `None` for an empty place or a refused
  /// one.
  pub hotkey: Option<Hotkey>,
  /// Why the place's hotkey is refused: past [MAX], an earlier place's, or the OS's refusal
  /// (another app has it, a key this platform can't see alone).
  pub error: Option<String>,
  /// The source that works without the listener holds the chord (macOS: Carbon's hotkey, which
  /// also works under Secure Input; Windows: `RegisterHotKey`).
  pub fallback: bool,
  pub fallback_error: Option<String>,
}

/// What [Hotkeys] does now, for a status line or diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
  /// Each place of [Hotkeys::set], in its order.
  pub slots: Vec<Slot>,
  pub listener: Listener,
  /// Why the listener last failed.
  pub listener_error: Option<String>,
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
}

pub(crate) type OnEdge = Box<dyn Fn(Edge) + Send>;

/// A place's hotkey as set, and why it is refused.
#[derive(Debug, Clone, Default)]
struct Place {
  hotkey: Option<Hotkey>,
  error: Option<String>,
}

/// The app's global hotkeys: one [Hotkeys] per app, made and used on the main thread while
/// [crate::app] runs. `on` hears their edges on a thread of its own, never the main one, so it
/// can hand them to a worker at once (or to the main thread through a [crate::Proxy]). Dropped,
/// the hotkeys stop; the threads stay for the process.
pub struct Hotkeys {
  imp: imp::Hotkeys,
  places: Vec<Place>,
  paused: bool,
  escape: bool,
}

impl Hotkeys {
  /// No hotkey until [Hotkeys::set]; the listener after [Hotkeys::start].
  pub fn new(on: impl Fn(Edge) + Send + 'static) -> Self {
    let imp = imp::Hotkeys::new(Box::new(on));
    Self { imp, places: Vec::new(), paused: false, escape: false }
  }

  /// Uses `hotkeys`, each place on its own (`None`: none there), its edges saying its place. A
  /// place past [MAX], one with an earlier place's hotkey and one the OS can't take (another app
  /// has it, a key this platform can't see alone) is refused alone ([Slot::error]); the `Err` is
  /// the first refusal. A place as it was is left alone, so a hotkey held stays held; a refused
  /// one is tried again. While paused they are taken once the pause ends.
  pub fn set(&mut self, hotkeys: &[Option<Hotkey>]) -> Result<(), String> {
    let changed = changed(&self.places, hotkeys);
    self.places.resize_with(hotkeys.len().max(self.places.len()), Place::default);
    for which in changed {
      let hotkey = hotkeys.get(which).copied().flatten();
      self.places[which] = Place { hotkey, error: refused(hotkeys, which) };
      self.arm(which);
    }

    self.places.truncate(hotkeys.len());
    self.places.iter().find_map(|p| p.error.clone()).map_or(Ok(()), Err)
  }

  /// While a shortcut recorder records a new hotkey the current ones are not matched: their keys
  /// reach the window.
  pub fn pause(&mut self, paused: bool) {
    if self.paused != paused {
      self.paused = paused;
      (0..self.places.len()).for_each(|which| self.arm(which));
    }
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

  /// Hands place `which`'s hotkey to the platform, none while paused or refused here; a refusal
  /// here stays. A place past [MAX] never reaches it.
  fn arm(&mut self, which: usize) {
    let place = &mut self.places[which];
    let want = place.hotkey.filter(|_| !self.paused && place.error.is_none());
    if which >= MAX {
      return;
    }

    let taken = self.imp.set(which, want);
    if want.is_some() {
      place.error = taken.err();
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
    let slot = |p: &Place| Slot {
      hotkey: p.hotkey.filter(|_| p.error.is_none()),
      error: p.error.clone(),
      ..Slot::default()
    };
    let slots = self.places.iter().map(slot).collect();
    let mut s = Status { slots, paused: self.paused, ..Status::default() };
    self.imp.status(&mut s);
    if self.escape {
      s.escape = s.escape_error.is_none();
    }

    s
  }
}

/// The places [Hotkeys::set] of `hotkeys` hands the platform again: those whose hotkey changed or
/// went, and those refused, tried again.
fn changed(places: &[Place], hotkeys: &[Option<Hotkey>]) -> Vec<usize> {
  let same = |which: usize| {
    let p = places.get(which).cloned().unwrap_or_default();
    p.error.is_none() && p.hotkey == hotkeys.get(which).copied().flatten()
  };
  (0..hotkeys.len().max(places.len())).filter(|&which| !same(which)).collect()
}

/// Why place `which` of `hotkeys` is refused before the OS is asked: past [MAX], or an earlier
/// place has its hotkey.
fn refused(hotkeys: &[Option<Hotkey>], which: usize) -> Option<String> {
  let hotkey = hotkeys[which]?;
  if which >= MAX {
    return Some(format!("more than {MAX} hotkeys"));
  }

  hotkeys[..which].contains(&Some(hotkey)).then(|| "the app has this hotkey already".into())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn keys(texts: &[&str]) -> Vec<Option<Hotkey>> {
    texts.iter().map(|t| Hotkey::parse(t).ok()).collect()
  }

  fn places(texts: &[&str], refused: &[usize]) -> Vec<Place> {
    let error = |i| refused.contains(&i).then(|| "taken".to_string());
    keys(texts)
      .into_iter()
      .enumerate()
      .map(|(i, hotkey)| Place { hotkey, error: error(i) })
      .collect()
  }

  /// A place as it was stays untouched (a held hotkey stays held); one changed, emptied, gone or
  /// refused before is handed over again.
  #[test]
  fn only_the_places_that_changed_are_handed_again() {
    let was = places(&["option+space", "f5", "right_option"], &[]);
    assert_eq!(changed(&was, &keys(&["option+space", "f5", "right_option"])), [] as [usize; 0]);
    assert_eq!(changed(&was, &keys(&["option+space", "f6", "right_option"])), [1]);
    assert_eq!(changed(&was, &keys(&["option+space", "", "right_option"])), [1]);
    assert_eq!(changed(&was, &keys(&["option+space"])), [1, 2]);
    assert_eq!(changed(&was, &keys(&["option+space", "f5", "right_option", "f7"])), [3]);
    assert_eq!(changed(&[], &keys(&["", "f5"])), [1]);

    let refused = places(&["option+space", "f5"], &[1]);
    assert_eq!(changed(&refused, &keys(&["option+space", "f5"])), [1], "tried again");
  }

  /// A hotkey an earlier place has is refused, the first one kept; so is a place past [MAX]; an
  /// empty place never is.
  #[test]
  fn repeats_and_places_past_the_most_are_refused() {
    let hotkeys = keys(&["option+space", "f5", "option+space", "", ""]);
    let refusals: Vec<_> = (0..hotkeys.len()).map(|i| refused(&hotkeys, i)).collect();
    assert_eq!(refusals[0], None);
    assert_eq!(refusals[1], None);
    assert!(refusals[2].as_deref().is_some_and(|e| e.contains("already")));
    assert_eq!((refusals[3].as_deref(), refusals[4].as_deref()), (None, None), "empty");

    let mut many = keys(&["f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9"]);
    assert_eq!(refused(&many, MAX - 1), None);
    assert!(refused(&many, MAX).is_some_and(|e| e.contains("more than 8")));
    many[MAX] = None;
    assert_eq!(refused(&many, MAX), None);
  }
}
