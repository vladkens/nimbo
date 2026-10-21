// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Harness scenario for `examples/counter.rs`: clicks change the count before the next frame,
//! the label and pixels show it, hover fades in on virtual time and the window goes idle after,
//! a press darkens the button, a release elsewhere is no click, and a change through `app_mut`
//! redraws.

#[path = "../examples/counter.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod counter;

use std::time::Duration;

use counter::{COUNT_ID, Counter, MINUS, PLUS, RESET};
use nimbo::{Harness, Id, Size};

fn main() {
  let mut h = Harness::new(Counter::default(), Size::new(360.0, 300.0), 2.0);
  let (plus, minus) = (Id::of(PLUS), Id::of(MINUS));
  let idle = h.render();
  assert!(!h.frame(), "a frame without changes");
  assert_eq!(h.text(Id::of(COUNT_ID)), Some("0"));

  // + three times and − once: the app has the count at once, the next frame shows it.
  for _ in 0..3 {
    assert!(h.click_id(plus));
  }

  assert!(h.click_id(minus));
  assert_eq!(h.app().count, 2);
  let img = h.render();
  assert_eq!(h.text(Id::of(COUNT_ID)), Some("2"));
  // The label's ink differs from the idle frame's "0".
  let c = h.rect(Id::of(COUNT_ID)).unwrap();
  let ink = |img: &nimbo::harness::Image| {
    let mut n = 0;
    for y in (c.y as i32..(c.y + c.h) as i32).step_by(2) {
      for x in (c.x as i32..(c.x + c.w) as i32).step_by(2) {
        n += (img.pixel(x as f32, y as f32)[0] > 0x80) as u32;
      }
    }

    n
  };
  assert!(ink(&img) > 0 && ink(&img) != ink(&idle), "{} vs {}", ink(&img), ink(&idle));

  // Hover over −: the fade runs frames on virtual time, lightening the fill, then stops.
  let m = h.rect(minus).unwrap();
  let (x, y) = (m.x + 4.0, m.y + m.h / 2.0); // inside, clear of the label
  h.leave();
  h.wait(Duration::from_millis(300));
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

  // A press fills it stronger than the hover; released elsewhere it is no click.
  h.press(x, y);
  let pressed = h.render().pixel(x, y);
  assert!(pressed[0] > full[0], "{pressed:?} vs {full:?}");
  h.move_to(5.0, 5.0);
  h.release(5.0, 5.0);
  assert_eq!(h.app().count, 2);

  // Reset.
  assert!(h.click_id(Id::of(RESET)));
  h.render();
  assert_eq!((h.app().count, h.text(Id::of(COUNT_ID))), (0, Some("0")));

  // State changed through `app_mut` redraws, once idle.
  h.wait(Duration::from_millis(500));
  assert!(!h.frame());
  h.app_mut().count = 7;
  h.render();
  assert_eq!(h.text(Id::of(COUNT_ID)), Some("7"));

  println!("counter scenario: ok");
}
