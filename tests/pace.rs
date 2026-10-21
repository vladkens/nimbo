// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Harness scenario for `examples/pace.rs`: each frame spends its work while the bar moves, the
//! bar is drawn where the time puts it, and once the run is over one more frame stills it and
//! none follows.

#[path = "../examples/pace.rs"]
#[allow(dead_code)]
mod pace;

use std::time::{Duration, Instant};

use nimbo::Harness;
use pace::*;

const BLUE: [u8; 4] = [0x3b, 0x82, 0xf6, 0xff];

fn main() {
  let mut h = Harness::new(Pace::default(), SIZE, 1.0);
  let t0 = Instant::now();
  assert!(h.frame());
  assert!(t0.elapsed() >= WORK, "a moving frame spends its work");

  // A quarter of a second in: the bar a quarter of the way across.
  h.advance(Duration::from_millis(250));
  let x = 0.25 * (SIZE.w - BAR_W) + BAR_W / 2.0;
  assert_eq!(h.render().pixel(x, SIZE.h / 2.0), BLUE);

  // Steps of a tenth of a second past the end of the run: a frame each, the last one still.
  let step = Duration::from_millis(100);
  let mut frames = 0;
  while h.rt.now() < RUN {
    h.advance(step);
    frames += h.frame() as usize;
  }

  assert_eq!(frames, 28);
  assert_eq!(h.app().frames, 2 + 27, "moving frames: the first, at 0.25 s, 0.35 to 2.95 s");
  assert!(!h.frame(), "still after the run");
  assert_eq!(h.wait(Duration::from_secs(1)), 0);
  println!("pace scenario: ok");
}
