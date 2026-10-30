// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/system.rs` through the harness, with a fake lid: a change of the system
//! reaches the window as `Event::System`, which reads again and redraws (the count, the last
//! kind, the lid), the rows keeping their heights; other events change nothing; the tray's menu
//! from what came.

#[path = "../examples/system.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod example;

use example::{COUNT, KINDS, LAST, LID, Seen, System};
use nimbo::harness::Image;
use nimbo::system::Change;
use nimbo::{Event, Harness, Id, Rect, Size};

fn closed() -> Option<bool> {
  Some(true)
}

fn window() -> Harness<System> {
  Harness::new(System::new(|| Some(false)), Size::new(420.0, 310.0), 2.0)
}

fn count(kind: Change) -> Id {
  let i = KINDS.iter().position(|&k| k == kind).unwrap();
  Id::of((COUNT, i as u8))
}

/// How many pixels of `r` differ between two frames.
fn changed(a: &Image, b: &Image, r: Rect) -> usize {
  let mut n = 0;
  for y in r.y as i32..(r.y + r.h) as i32 {
    for x in r.x as i32..(r.x + r.w) as i32 {
      n += (a.pixel(x as f32, y as f32) != b.pixel(x as f32, y as f32)) as usize;
    }
  }

  n
}

#[test]
fn a_change_redraws_the_window() {
  let mut h = window();
  let before = h.render();
  let rows: Vec<_> = KINDS.iter().map(|&k| h.rect(count(k)).unwrap().h).collect();
  assert_eq!(h.text(count(Change::AudioInputs)), Some("0"));
  assert_eq!(h.text(Id::of(LAST)), Some("None"));

  assert!(h.event(Event::System(Change::AudioInputs)));
  assert!(h.rt.needs_frame());
  let after = h.render();
  assert_eq!(h.text(count(Change::AudioInputs)), Some("1"));
  assert_eq!(h.text(Id::of(LAST)), Some("Audio inputs"));
  assert_eq!(h.app().seen.counts, [1, 0, 0]);
  let last = h.rect(Id::of(LAST)).unwrap();
  assert!(changed(&before, &after, last) > 0, "the last kind's row drawn again");
  let now: Vec<_> = KINDS.iter().map(|&k| h.rect(count(k)).unwrap().h).collect();
  assert_eq!(rows, now, "rows keep their heights");
}

/// The lid read again after its change: closed now.
#[test]
fn the_lid_is_read_again() {
  let mut h = window();
  h.render();
  assert_eq!(h.text(Id::of(LID)), Some("Open"));
  h.app_mut().read_lid = closed;
  assert!(h.event(Event::System(Change::Lid)));
  h.render();
  assert_eq!(h.text(Id::of(LID)), Some("Closed"));
  assert_eq!(h.text(count(Change::Lid)), Some("1"));
}

/// Input that isn't a change of the system leaves what came alone.
#[test]
fn other_events_change_nothing() {
  let mut h = window();
  h.render();
  h.event(Event::Focus(true));
  h.move_to(10.0, 10.0);
  assert_eq!(h.app().seen, Seen::new(Some(false)));
}

/// The tray's menu: the lid and the audio changes, both kinds counted.
#[test]
fn the_tray_menu() {
  let mut s = Seen::new(None);
  let labels =
    |s: &Seen| example::menu(s).items.iter().map(|i| i.label.clone()).collect::<Vec<_>>();
  assert_eq!(labels(&s)[..2], ["Lid: None", "Audio changes: 0"]);
  s.add(Change::AudioInputs, Some(false));
  s.add(Change::DefaultInput, Some(false));
  s.add(Change::Lid, Some(true));
  assert_eq!(labels(&s)[..2], ["Lid: Closed", "Audio changes: 2"]);
  assert_eq!(s.last, Some(Change::Lid));
}
