// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Checks frame pacing: a bar slides across for [RUN], each frame spending [WORK] of CPU time
//! first, then everything stands still. Frames paced from their start still come once per refresh
//! (60 fps at 60 Hz); paced from their end they would come every 16.7 + 8 ms (about 40 fps). Once
//! still, the loop sleeps until something happens.
//!
//! `scripts/pace.py` (`make pace`) runs it in a window, traced, and counts the frames while it
//! moves and the wakeups once it is still. `cargo run --release --example pace` opens it;
//! `-- --shot pace.png` renders its first frame.

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::time::{Duration, Instant};

use nimbo::{App, Config, Frame, Rect, Size, rgb};

pub const SIZE: Size = Size::new(320.0, 80.0);
/// How long the bar moves, from the first frame.
pub const RUN: Duration = Duration::from_secs(3);
/// The CPU time each moving frame spends before it paints.
pub const WORK: Duration = Duration::from_millis(8);
/// The bar's width; it crosses the window once a second.
pub const BAR_W: f32 = 40.0;

#[derive(Default)]
pub struct Pace {
  /// The first frame's time.
  start: Option<Duration>,
  /// Frames painted while moving.
  pub frames: usize,
}

impl App for Pace {
  fn paint(&mut self, f: &mut Frame) {
    let start = *self.start.get_or_insert(f.now);
    let t = f.now.saturating_sub(start);
    let moving = t < RUN;
    if moving {
      let busy = Instant::now();
      while busy.elapsed() < WORK {
        std::hint::spin_loop();
      }

      self.frames += 1;
      f.animate();
    }

    f.rect(Rect::new(0.0, 0.0, f.size.w, f.size.h), rgb(0x1c1c1e), 0.0);
    let x = t.min(RUN).as_secs_f32().fract() * (f.size.w - BAR_W);
    f.rect(Rect::new(x, 0.0, BAR_W, f.size.h), rgb(0x3b82f6), 0.0);
  }
}

#[allow(dead_code)]
fn main() {
  nimbo::run(Config::new("pace", SIZE.w, SIZE.h).fixed(), Pace::default(), |_| {});
}
