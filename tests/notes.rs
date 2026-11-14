// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/notes.rs` through the harness, under macOS's title bar as the harness plays
//! it and under none (Windows, Linux): the sidebar's toggle at its trailing edge on the traffic
//! lights' middle, following it as it slides out to right of the lights, and back; the
//! empty bar moves the window and the items don't; the sidebar's ground up to the window's top
//! and its notes under the bar; the trailing button adds a note.

#[path = "../examples/notes.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod notes;

mod common;

use std::time::Duration;

use common::{diff, px};
use nimbo::harness::mac_titlebar;
use nimbo::ui::containers::toolbar::{AFTER_BUTTONS, LEAD};
use nimbo::{Appearance, Event, Harness, Id, Point, Rect, Size, Theme};
use notes::{LIST, NEW, Notes, TOGGLE};

type H = Harness<Notes>;

/// The notes under macOS's title bar (`mac`) or none, in the light appearance.
fn notes(mac: bool) -> H {
  let mut h = Harness::new(Notes::default(), Size::new(760.0, 480.0), 2.0);
  h.set_titlebar(mac.then(|| mac_titlebar("Notes", false)));
  h.event(Event::Appearance(Appearance::LIGHT));
  h.frame();
  h
}

fn mid(r: Rect) -> Point {
  Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0)
}

/// The toggle's rect.
fn toggle(h: &H) -> Rect {
  h.rect(Id::of(TOGGLE)).unwrap()
}

/// The sidebar column's trailing edge: its list is as wide as it.
fn column_end(h: &H) -> f32 {
  h.rect(Id::of(LIST)).map(|r| r.x + r.w).unwrap()
}

#[test]
fn the_toggle_trails_the_sidebar_and_follows_it_to_the_traffic_lights() {
  let mut h = notes(true);
  let lights = mac_titlebar("Notes", true).buttons.unwrap();
  let after = lights.x + lights.w + AFTER_BUTTONS;
  // Shown: at the sidebar's trailing edge, by the divider, on the lights' middle.
  let at = toggle(&h);
  assert_eq!((at.x + at.w, mid(at).y), (column_end(&h) - 8.0, mid(lights).y));
  let side = h.rect(Id::of(LIST)).unwrap().w;
  h.click(mid(at).x, mid(at).y);
  h.frame();
  assert!(!h.app().sidebar);
  // It slides out in 150 ms, not at once, the toggle with it.
  h.wait(Duration::from_millis(60));
  let (half, t) = (h.rect(Id::of(LIST)).map_or(0.0, |r| r.w), toggle(&h).x);
  assert!(half > 0.0 && half < side, "{half} of {side}");
  assert!(t > after && t < at.x, "the toggle at {t}, between {after} and {}", at.x);
  // Hidden: right of the lights.
  h.wait(Duration::from_millis(120));
  assert!(h.rect(Id::of(LIST)).is_none());
  assert_eq!((toggle(&h).x, mid(toggle(&h)).y), (after, mid(lights).y));
  let t = toggle(&h);
  h.click(mid(t).x, mid(t).y);
  h.wait(Duration::from_millis(200));
  assert!(h.app().sidebar);
  assert_eq!(h.rect(Id::of(LIST)).unwrap().w, side, "back at its share");
  assert_eq!(toggle(&h), at, "and the toggle by its edge");
}

#[test]
fn the_empty_bar_moves_the_window_and_the_items_do_not() {
  let h = notes(true);
  let (t, new) = (mid(toggle(&h)), mid(h.rect(Id::of(NEW)).unwrap()));
  assert!(!h.drags_window(t.x, t.y) && !h.drags_window(new.x, new.y));
  assert!(h.drags_window(400.0, 26.0) && h.drags_window(150.0, 5.0));
  // The notes start under the bar.
  assert!(h.rect(Id::of(LIST)).unwrap().y >= 52.0);
}

#[test]
fn the_sidebar_ground_goes_up_to_the_window_top() {
  let mut h = notes(true);
  let img = h.render();
  let panel = Theme::PLATES_LIGHT.panel;
  assert!(diff(img.pixel(5.0, 5.0), px(panel)) < 3, "{:?}", img.pixel(5.0, 5.0));
  assert!(diff(img.pixel(5.0, 40.0), px(panel)) < 3);
}

#[test]
fn under_the_os_title_bar_the_toolbar_is_the_top_row() {
  let mut h = notes(false);
  let at = toggle(&h);
  assert_eq!((at.x + at.w, mid(at).y), (column_end(&h) - 8.0, 26.0));
  assert!(at.x > LEAD);
  assert!(!h.drags_window(400.0, 26.0));
  let new = mid(h.rect(Id::of(NEW)).unwrap());
  h.click(new.x, new.y);
  h.frame();
  assert_eq!((h.app().notes.len(), h.app().open()), (4, 3));
}
