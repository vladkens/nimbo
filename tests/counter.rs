// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/counter.rs` through the harness: clicks change the count before the next
//! frame, the label and pixels show it, hover fades in on virtual time and the window goes idle
//! after, a press darkens the button, a release elsewhere is no click, and a change through
//! `app_mut` redraws. Assistive technology reads the buttons by their text.

#[path = "../examples/counter.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod counter;

mod common;

use std::time::Duration;

use common::reads;
use counter::{COUNT_ID, Counter, MINUS, PLUS, RESET};
use nimbo::harness::Image;
use nimbo::{Harness, Id, Size};

fn counter() -> Harness<Counter> {
  Harness::new(Counter::default(), Size::new(360.0, 300.0), 2.0)
}

/// A point inside −, clear of its label, once the pointer is away and the hover faded out.
fn inside_minus(h: &mut Harness<Counter>) -> (f32, f32) {
  h.render();
  let m = h.rect(Id::of(MINUS)).unwrap();
  h.leave();
  h.wait(Duration::from_millis(300));
  (m.x + 4.0, m.y + m.h / 2.0)
}

#[test]
fn clicks_change_the_count_before_the_next_frame() {
  let mut h = counter();
  let idle = h.render();
  assert!(!h.frame(), "a frame without changes");
  assert_eq!(h.text(Id::of(COUNT_ID)), Some("0"));

  // + three times and − once: the app has the count at once, the next frame shows it.
  for _ in 0..3 {
    assert!(h.click_id(Id::of(PLUS)));
  }

  assert!(h.click_id(Id::of(MINUS)));
  assert_eq!(h.app().count, 2);
  let img = h.render();
  assert_eq!(h.text(Id::of(COUNT_ID)), Some("2"));
  // The label's ink differs from the idle frame's "0".
  let c = h.rect(Id::of(COUNT_ID)).unwrap();
  let ink = |img: &Image| {
    let mut n = 0;
    for y in (c.y as i32..(c.y + c.h) as i32).step_by(2) {
      for x in (c.x as i32..(c.x + c.w) as i32).step_by(2) {
        n += (img.pixel(x as f32, y as f32)[0] > 0x80) as u32;
      }
    }

    n
  };
  assert!(ink(&img) > 0 && ink(&img) != ink(&idle), "{} vs {}", ink(&img), ink(&idle));
}

#[test]
fn hover_fades_in_on_virtual_time_then_idles() {
  let mut h = counter();
  let (x, y) = inside_minus(&mut h);
  let off = h.render().pixel(x, y);
  assert!(h.move_to(x, y));
  assert!(h.frame());
  let start = h.render().pixel(x, y);
  assert_eq!(start, off, "the fade starts at the resting fill");
  h.wait(Duration::from_millis(80));
  let mid = h.render().pixel(x, y);
  let frames = h.wait(Duration::from_millis(500));
  let full = h.render().pixel(x, y);
  assert!(off[0] < mid[0] && mid[0] < full[0], "{off:?} {mid:?} {full:?}");
  assert!(frames > 0 && !h.frame(), "idle once the fade is done");
}

#[test]
fn a_press_fills_stronger_and_a_release_elsewhere_is_no_click() {
  let mut h = counter();
  let (x, y) = inside_minus(&mut h);
  h.move_to(x, y);
  h.wait(Duration::from_millis(500));
  let hovered = h.render().pixel(x, y);
  h.press(x, y);
  let pressed = h.render().pixel(x, y);
  assert!(pressed[0] > hovered[0], "{pressed:?} vs {hovered:?}");
  h.move_to(5.0, 5.0);
  h.release(5.0, 5.0);
  assert_eq!(h.app().count, 0);
}

#[test]
fn reset_sets_the_count_to_zero() {
  let mut h = counter();
  h.render();
  h.click_id(Id::of(PLUS));
  h.click_id(Id::of(PLUS));
  h.render();
  assert!(h.click_id(Id::of(RESET)));
  h.render();
  assert_eq!((h.app().count, h.text(Id::of(COUNT_ID))), (0, Some("0")));
}

#[test]
fn a_change_through_app_mut_redraws_once_idle() {
  let mut h = counter();
  h.render();
  h.wait(Duration::from_millis(500));
  assert!(!h.frame());
  h.app_mut().count = 7;
  h.render();
  assert_eq!(h.text(Id::of(COUNT_ID)), Some("7"));
}

#[test]
fn the_buttons_are_named_by_their_text() {
  let mut h = counter();
  h.frame();
  reads(
    &h.a11y(),
    &["static text \"0\"", "button \"−\"", "button \"+\"", "button \"Reset\" disabled"],
  );
  h.click_id(Id::of(PLUS));
  h.frame();
  reads(&h.a11y(), &["static text \"1\"", "button \"Reset\""]);
}

/// The Metal renderer draws the counter as the CPU does, and again with a new count and the +
/// button's hover fading in.
#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn metal_draws_as_the_cpu() {
  let mut h = counter();
  common::metal_parity(&mut h, 0.005);
  for _ in 0..7 {
    h.click_id(Id::of(PLUS));
  }

  h.wait(Duration::from_millis(60));
  common::metal_parity(&mut h, 0.005);
}
