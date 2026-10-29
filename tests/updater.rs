// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/updater.rs` through the harness: the update's window offers the release
//! with its three choices, the notes and the checkbox, a failure offers Retry and Close, and each
//! state shows its title.

#[path = "../examples/updater.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod updater;

use nimbo::update::dialog::{self, Act, State};
use nimbo::{Harness, Id, Size};
use updater::{Updater, state};

fn window(name: &str) -> Harness<Updater> {
  let app = Updater { state: state(name).unwrap(), auto: true, acts: Vec::new() };
  Harness::new(app, Size::new(dialog::SIZE.0, dialog::SIZE.1), 2.0)
}

#[test]
fn an_offer_has_its_three_choices_the_notes_and_the_checkbox() {
  for (id, act) in [
    (dialog::SKIP, Act::Skip),
    (dialog::LATER, Act::Later),
    (dialog::NOTES, Act::Notes),
    (dialog::AUTO, Act::Auto(false)),
    (dialog::INSTALL, Act::Install),
  ] {
    let mut h = window("available");
    h.render();
    assert!(h.click_id(Id::of(id)), "{id}");
    assert_eq!(h.app().acts, [act], "{id}");
  }

  let mut h = window("available");
  h.render();
  h.click_id(Id::of(dialog::INSTALL));
  h.render();
  assert!(matches!(h.app().state, State::Downloading(..)), "Install downloads");
}

#[test]
fn a_failure_offers_retry_and_close() {
  for (id, act) in [(dialog::RETRY, Act::Retry), (dialog::CLOSE, Act::Close)] {
    let mut h = window("failed");
    h.render();
    assert!(h.click_id(Id::of(id)), "{id}");
    assert_eq!(h.app().acts, [act], "{id}");
  }
}

/// Every state draws its title and one way out of the window at least.
#[test]
fn every_state_draws_and_can_be_left() {
  for name in ["available", "checking", "up-to-date", "downloading", "restarting", "failed"] {
    let mut h = window(name);
    h.render();
    let out = [dialog::CLOSE, dialog::LATER].iter().any(|id| h.rect(Id::of(*id)).is_some());
    assert!(out, "{name}: no way out");
  }
}
