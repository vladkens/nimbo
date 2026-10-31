// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Harness tests for `examples/hotkeys.rs`, its edges fed to the example's own handler (no event
//! tap, no Carbon: the hotkeys are made in its `main` only): the first key per dictation picks
//! the model and later ones are ignored until the release or the abort; the log keeps its last
//! rows, each one height; the recorders record hotkeys and keys, pause while they record, and an
//! emptied one shifts the indices `set` gets; the status shows why keys are not taken.

#[path = "../examples/hotkeys.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod example;

use std::time::Instant;

use example::{Demo, HOTKEY, KEY, KEYS_STATUS, LOG, MODEL, ROWS, describe};
use nimbo::hotkey::{Edge, Status};
use nimbo::{Harness, Id, Modifiers, NamedKey, Size};

fn window() -> Harness<Demo> {
  let mut h = Harness::new(Demo::default(), Size::new(560.0, 880.0), 2.0);
  h.frame();
  h
}

fn feed(h: &mut Harness<Demo>, edges: &[Edge]) {
  for &e in edges {
    h.app_mut().edge(e);
  }

  h.frame();
}

fn log(h: &Harness<Demo>) -> Vec<String> {
  (0..ROWS as u8).filter_map(|r| h.text(Id::of((LOG, r))).map(str::to_owned)).collect()
}

#[test]
fn the_first_key_per_dictation_picks_the_model() {
  let mut h = window();
  let at = Instant::now();
  let down = |which| Edge::Down { which, at, repeat: false };
  let key = |index| Edge::Key { index, at };
  feed(&mut h, &[down(1), Edge::Down { which: 1, at, repeat: true }, key(1), key(0)]);
  assert_eq!(h.text(Id::of(MODEL)), Some("Key 2's model (X)"));
  assert_eq!(log(&h)[..3], ["Down: hotkey 2", "Key 2 (X)", "Key 1 (Z): ignored"]);
  // Released: the next dictation starts with its hotkey's model, and its first key picks again.
  feed(&mut h, &[Edge::Up { which: 1, at }, down(0)]);
  assert_eq!(h.text(Id::of(MODEL)), Some("Hotkey 1's model"));
  feed(&mut h, &[key(0)]);
  assert_eq!(h.text(Id::of(MODEL)), Some("Key 1's model (Z)"));
  // Aborted (the user's own chord): a key after it, with nothing held, picks nothing.
  feed(&mut h, &[Edge::Aborted { which: 0 }, key(2)]);
  assert_eq!(h.text(Id::of(MODEL)), Some("Key 1's model (Z)"));
  assert_eq!(log(&h).last().map(String::as_str), Some("Key 3 (1): ignored"));
  assert_eq!(describe(key(2)), "Key 2");
  assert_eq!(describe(Edge::Aborted { which: 0 }), "Aborted 0");
}

#[test]
fn the_log_keeps_its_last_rows_each_one_height() {
  let mut h = window();
  let heights = |h: &Harness<Demo>| -> Vec<f32> {
    (0..ROWS as u8).map(|r| h.rect(Id::of((LOG, r))).map_or(0.0, |r| r.h)).collect()
  };
  let empty = heights(&h);
  let at = Instant::now();
  let edges: Vec<Edge> = (0..ROWS + 3).map(|i| Edge::Up { which: i % 3, at }).collect();
  feed(&mut h, &edges);
  assert_eq!(heights(&h), empty, "rows keep their heights");
  assert_eq!(log(&h).len(), ROWS);
  assert_eq!(log(&h)[0], "Up: hotkey 1", "the oldest dropped");
}

#[test]
fn the_recorders_record_and_pause_the_hotkeys() {
  let mut h = window();
  assert_eq!(h.app().sent, (vec![0, 1, 2], vec![0, 1, 2]));
  h.click_id(Id::of((HOTKEY, 2u8)));
  assert!(h.app().paused, "paused while recording");
  h.key(NamedKey::F5, Modifiers::ALT);
  h.frame();
  assert!(!h.app().paused);
  assert_eq!(h.app().hotkeys[2].map(|k| k.to_string()).as_deref(), Some("option+f5"));
  h.click_id(Id::of((KEY, 1u8)));
  h.key('Q', Modifiers::SHIFT);
  assert_eq!(h.app().keys[1].map(|k| k.to_string()).as_deref(), Some("shift+q"));
  // An emptied recorder: the others move up in `set`, and the edges find their recorder.
  h.click_id(Id::of((HOTKEY, 0u8)));
  h.key(NamedKey::Backspace, Modifiers::empty());
  assert_eq!(h.app().sent.0, [1, 2]);
  feed(&mut h, &[Edge::Down { which: 0, at: Instant::now(), repeat: false }]);
  assert_eq!(h.text(Id::of(MODEL)), Some("Hotkey 2's model"));
}

#[test]
fn the_status_says_why_keys_are_not_taken() {
  let mut h = window();
  let mut s = Status::default();
  s.keys_error = Some("keys while held need the event tap: Accessibility access".into());
  h.app_mut().status = Some(s);
  h.frame();
  assert!(h.text(Id::of(KEYS_STATUS)).is_some_and(|t| t.contains("Accessibility")));
}
