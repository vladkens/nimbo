// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Global hotkeys: a chord or a modifier key alone ([Hotkey], as settings spell it) that the app
//! hears while any app has the keyboard ([Hotkeys]: its press, autorepeats and release as
//! [Edge]s, on a thread of its own).
//!
//! | | chords | a modifier alone | release | needs |
//! |---|---|---|---|---|
//! | macOS | event tap, Carbon `RegisterEventHotKey` as the fallback | event tap | both | the tap: Accessibility access |
//! | Windows | `RegisterHotKey` | low-level keyboard hook | key state polled | nothing |
//! | Linux | the `GlobalShortcuts` portal (KDE, GNOME 48+, Hyprland) | no | `Deactivated` | the user's approval |
//!
//! On Linux the portal binds the shortcut with the chord only as a preference: the desktop asks
//! the user, who may pick another. Without the portal (X11 desktops, older GNOME) there are no
//! hotkeys: [Status::listener] fails with the reason.
//!
//! [Hotkeys::escape] takes Escape from every app for a while (to cancel what the hotkey started)
//! and reports it as [Edge::Escape]: on macOS the event tap swallows it, and Carbon's hotkey takes
//! it without the tap (no Accessibility access, Secure Input); on Windows the keyboard hook
//! swallows it. The portal can't, so on Linux Escape stays the focused app's
//! ([Status::escape_error] says so).

use std::time::Instant;

use crate::keys::Hotkey;
use crate::platform::hotkey as imp;

/// What the hotkey did, on the hotkeys' thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
  /// Pressed, or held (`repeat`, the key's autorepeat), `at` when the key moved.
  Down {
    at: Instant,
    repeat: bool,
  },
  Up {
    at: Instant,
  },
  /// Escape pressed while [Hotkeys::escape] is on; the app in front never sees it.
  Escape,
  /// A modifier key alone was pressed, then another key within a moment: the user's own chord
  /// (⌥E), so what the press started is dropped. Its release comes as no edge.
  Aborted,
}

/// The system-wide key source [Hotkeys::start] starts: macOS's event tap, Windows' low-level
/// keyboard hook, Linux's portal session.
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

/// What [Hotkeys] does now, for a status line or diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
  /// The hotkey armed (or waiting for the pause to end).
  pub hotkey: Option<Hotkey>,
  /// Why the last [Hotkeys::set] left no hotkey.
  pub error: Option<String>,
  pub listener: Listener,
  /// Why the listener last failed.
  pub listener_error: Option<String>,
  /// The source that works without the listener holds the chord (macOS: Carbon's hotkey, which
  /// also works under Secure Input; Windows: `RegisterHotKey`).
  pub fallback: bool,
  pub fallback_error: Option<String>,
  pub paused: bool,
  /// Escape is taken now: [Hotkeys::escape] is on and a source here has it.
  pub escape: bool,
  /// Why Escape is on but not taken: the platform can't (Linux) or no source is up.
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

/// The app's global hotkey: one per app, made and used on the main thread while [crate::app]
/// runs. `on` hears its edges on a thread of its own, never the main one, so it can hand them
/// to a worker at once (or to the main thread through a [crate::Proxy]). Dropped, the hotkey
/// stops; the threads stay for the process.
pub struct Hotkeys {
  imp: imp::Hotkeys,
  text: Option<String>,
  hotkey: Option<Hotkey>,
  error: Option<String>,
  paused: bool,
  escape: bool,
}

impl Hotkeys {
  /// No hotkey until [Hotkeys::set]; the listener after [Hotkeys::start].
  pub fn new(on: impl Fn(Edge) + Send + 'static) -> Self {
    let imp = imp::Hotkeys::new(Box::new(on));
    Self { imp, text: None, hotkey: None, error: None, paused: false, escape: false }
  }

  /// Uses the hotkey spelled `text` ([Hotkey::parse]); one that doesn't parse, or that the OS
  /// can't take (another app has it, a key this platform can't see alone), disables the hotkey
  /// and is the error. While paused it is taken once the pause ends.
  pub fn set(&mut self, text: &str) -> Result<(), String> {
    // The same hotkey again after the OS refused it (another app had it) tries again.
    let refused = self.hotkey.is_some() && self.error.is_some();
    if self.text.as_deref() != Some(text) || refused {
      self.text = Some(text.to_owned());
      match Hotkey::parse(text) {
        Ok(h) => self.hotkey = Some(h),
        Err(e) => (self.hotkey, self.error) = (None, Some(e.to_string())),
      }

      self.arm();
    }

    self.error.clone().map_or(Ok(()), Err)
  }

  /// While a shortcut recorder records a new hotkey the current one is not matched: its keys
  /// reach the window.
  pub fn pause(&mut self, paused: bool) {
    if self.paused != paused {
      self.paused = paused;
      self.arm();
    }
  }

  /// While on, Escape (alone or with Shift; a modifier hotkey held past its chord window aside) is
  /// taken from every app and comes as [Edge::Escape], once per press; its repeats and release
  /// are swallowed too. A press that began before stays the app's. Whether it is taken is in
  /// [Status::escape]: macOS needs the listener or Carbon's hotkey, Windows the listener, Linux
  /// can't.
  pub fn escape(&mut self, on: bool) {
    if self.escape != on {
      self.escape = on;
      self.imp.escape(on);
    }
  }

  /// Hands the hotkey to the platform, none while paused or unparsed; a parse error stays.
  fn arm(&mut self) {
    let want = self.hotkey.filter(|_| !self.paused);
    let taken = self.imp.set(want);
    if self.hotkey.is_some() {
      self.error = taken.err();
    }
  }

  /// Starts the listener, unless it runs or is starting; again after it failed. Returns at once:
  /// whether it came up is in [Hotkeys::status] a moment later; the error here is only one
  /// starting it. macOS: the event tap needs Accessibility access (System Settings → Privacy &
  /// Security), chords work without it through the fallback; Linux: the portal asks the user to
  /// approve the shortcut.
  pub fn start(&mut self) -> Result<(), String> {
    self.imp.start()
  }

  pub fn status(&self) -> Status {
    let mut s = Status { paused: self.paused, error: self.error.clone(), ..Status::default() };
    s.hotkey = self.hotkey.filter(|_| s.error.is_none());
    self.imp.status(&mut s);
    if self.escape {
      s.escape = s.escape_error.is_none();
    }

    s
  }
}
