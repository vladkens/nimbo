// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/notes.rs` through the harness, under macOS's title bar as the harness plays
//! it and under none (Windows, Linux): the toolbar's toggle right of the traffic lights on their
//! middle, there with the sidebar shown or hidden; a click on it slides the sidebar out and in, the
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
use notes::{LIST, NEW, NOTE, Notes, TOGGLE};

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

#[test]
fn the_toggle_sits_right_of_the_traffic_lights_on_their_middle() {
  let mut h = notes(true);
  let lights = mac_titlebar("Notes", true).buttons.unwrap();
  let at = toggle(&h);
  assert_eq!(at.x, lights.x + lights.w + AFTER_BUTTONS);
  assert_eq!(mid(at).y, mid(lights).y);
  // Hidden, the sidebar takes nothing from it: the toggle stays where it was.
  let side = h.rect(Id::of(LIST)).unwrap().w;
  h.click(mid(at).x, mid(at).y);
  h.frame();
  assert!(!h.app().sidebar);
  // It slides out in 150 ms, not at once.
  h.wait(Duration::from_millis(75));
  let half = h.rect(Id::of(LIST)).map_or(0.0, |r| r.w);
  assert!(half > 0.0 && half < side, "{half} of {side}");
  h.wait(Duration::from_millis(100));
  assert_eq!(toggle(&h), at);
  assert!(h.rect(Id::of(LIST)).is_none());
  let t = toggle(&h);
  h.click(mid(t).x, mid(t).y);
  h.wait(Duration::from_millis(200));
  assert!(h.app().sidebar);
  assert_eq!(h.rect(Id::of(LIST)).unwrap().w, side, "back at its share");
}

#[test]
fn the_empty_bar_moves_the_window_and_the_items_do_not() {
  let h = notes(true);
  let (t, new) = (mid(toggle(&h)), mid(h.rect(Id::of(NEW)).unwrap()));
  assert!(!h.drags_window(t.x, t.y) && !h.drags_window(new.x, new.y));
  assert!(h.drags_window(400.0, 26.0) && h.drags_window(150.0, 5.0));
  // The notes start under the bar.
  assert!(h.rect(Id::of(LIST)).unwrap().y >= 52.0);
  assert!(h.rect(Id::of(NOTE)).is_some());
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
  assert_eq!((at.x, mid(at).y), (LEAD, 26.0));
  assert!(!h.drags_window(400.0, 26.0));
  let new = mid(h.rect(Id::of(NEW)).unwrap());
  h.click(new.x, new.y);
  h.frame();
  assert_eq!((h.app().notes.len(), h.app().open()), (4, 3));
}
