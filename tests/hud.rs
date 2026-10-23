// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/hud.rs` through the harness: the overlay's config, its frame transparent
//! around the pill and its shadow, the pill, dot and bars where they are, and the meter moving
//! on every frame while it records.

#[path = "../examples/hud.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod hud;

mod common;

use std::time::Duration;

use common::{diff, px};
use hud::{CONFIG, DOT, FILL, Hud, PILL_H, pill};
use nimbo::{Anchor, Harness};

/// The HUD as `--shot` renders it: transparent, as its config is an overlay's.
fn hud() -> Harness<Hud> {
  let mut h = Harness::new(Hud::default(), CONFIG.size, 2.0);
  h.cpu.transparent = CONFIG.overlay.is_some();
  hud::shot(&mut h);
  h
}

#[test]
fn an_overlay_at_the_bottom_shown_on_demand() {
  let shown = (CONFIG.overlay, CONFIG.hidden, CONFIG.resizable);
  assert_eq!(shown, (Some(Anchor::Bottom(44.0)), true, false));
}

#[test]
fn transparent_around_the_pill_and_its_shadow() {
  let mut h = hud();
  let img = h.render();
  let p = pill();
  // The corners, clear of the pill's rounding and its shadow.
  for (x, y) in [(1.0, 1.0), (CONFIG.size.w - 1.0, 1.0), (1.0, CONFIG.size.h - 1.0)] {
    assert_eq!(img.pixel(x, y), [0; 4], "({x}, {y})");
  }

  // The shadow under the pill: dark, partly through.
  let [r, _, _, a] = img.pixel(p.x + p.w / 2.0, p.y + p.h + 2.0);
  assert!(r < 40 && a > 20 && a < 200, "the shadow: {r} at {a}");

  // The pill between the dot and the bars, and the dot.
  assert!(diff(img.pixel(p.x + 29.0, p.y + 4.0), px(FILL)) <= 1);
  assert!(diff(img.pixel(p.x + 19.0, p.y + PILL_H / 2.0), px(DOT)) <= 1);
}

#[test]
fn the_meter_moves_while_recording() {
  let mut h = hud();
  let p = pill();
  let bars = |h: &mut Harness<Hud>| {
    let img = h.render();
    (0..40).map(|y| img.pixel(p.x + 35.0, p.y + y as f32)).collect::<Vec<_>>()
  };
  let before = bars(&mut h);
  assert!(h.rt.needs_frame(), "it animates");
  assert!(h.wait(Duration::from_millis(250)) >= 25, "a frame every refresh");
  assert_ne!(bars(&mut h), before);
}
