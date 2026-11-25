// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/dictation/` through the harness, a module per window, each alone at its
//! size: [history], [diagnostics] and [hud]; the tray's menu in [tray] (with the detached tray's
//! scenario where it needs no main thread); here a dictation from start to History.

#[path = "../../examples/dictation/main.rs"]
#[expect(dead_code, reason = "its main and what these tests don't use")]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[cfg_attr(windows, expect(unused_attributes, reason = "its windows_subsystem"))]
mod dictation;

#[path = "../common/mod.rs"]
mod common;

#[cfg(not(target_os = "macos"))]
mod detached;
mod diagnostics;
mod history;
mod hud;
mod tray;

use std::cell::RefCell;

use nimbo::Harness;

use crate::common::reads;
use crate::dictation::history::{PER_DAY, SIZE};
use crate::dictation::{LINES, Shared};

thread_local! {
  /// What the fake paste pasted, on this test's thread.
  static PASTED: RefCell<Vec<String>> = RefCell::default();
}

/// A dictation records until the second press, then what it heard is pasted through the app's
/// paste and History opens with it on top; the next one hears the next line.
#[test]
fn a_dictation_reaches_history_and_the_paste() {
  let state = Shared::default();
  state.borrow_mut().dictation.paste = |line| PASTED.with(|p| p.borrow_mut().push(line.into()));

  assert_eq!(dictation::dictate(&state), None, "started");
  assert!(state.borrow().dictation.recording);
  dictation::paste_unpasted(&state);
  assert!(PASTED.with(|p| p.borrow().is_empty()), "nothing heard yet");

  assert_eq!(dictation::dictate(&state).as_deref(), Some(LINES[0]), "stopped");
  assert!(!state.borrow().dictation.recording);
  dictation::paste_unpasted(&state);
  assert_eq!(PASTED.with(|p| p.borrow().clone()), [LINES[0]]);
  dictation::paste_unpasted(&state);
  assert_eq!(PASTED.with(|p| p.borrow().len()), 1, "pasted once");

  dictation::dictate(&state);
  assert_eq!(dictation::dictate(&state).as_deref(), Some(LINES[1]));
  let mut h = Harness::new(dictation::history_of(&state), SIZE, 1.0);
  h.frame();
  let row = |line: &str| format!("row {line:?} desc \"23:59 · 9.2 s\"");
  reads(&h.a11y(), &[&row(LINES[1])]);
  assert_eq!(h.app().days[0].1, PER_DAY + 2, "both in Today");
}

/// A dictation stopped during another's countdown waits for it: both are pasted at its end.
#[test]
fn dictations_heard_meanwhile_paste_together() {
  let state = Shared::default();
  state.borrow_mut().dictation.paste = |line| PASTED.with(|p| p.borrow_mut().push(line.into()));
  for _ in 0..4 {
    dictation::dictate(&state);
  }

  dictation::paste_unpasted(&state);
  assert_eq!(PASTED.with(|p| p.borrow().clone()), [format!("{} {}", LINES[0], LINES[1])]);
}
