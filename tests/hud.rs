// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests through the harness of a HUD's overlay: the overlay's config, its frame transparent around
//! the pill and its shadow, the pill, dot and bars where they are, and the meter moving on every
//! frame while it records.

mod hud {
  //! A HUD in an overlay ([nimbo::Config::overlay]): a recording pill at the bottom centre of the
  //! screen, drawn inside the overlay with its shadow, transparent around them: a red dot, a level
  //! meter of bars that follow a made-up level, and the time.

  use std::time::Duration;

  use nimbo::{Anchor, App, Color, Config, Font, Frame, Harness, Rect, Weight, alpha, rgb};

  /// The overlay: the pill and a margin around it for its shadow, 44 points above the Dock or the
  /// taskbar.
  pub const CONFIG: Config = Config::new("HUD", PILL_W + 2.0 * MARGIN, PILL_H + 2.0 * MARGIN)
    .overlay(Anchor::Bottom(44.0))
    .hidden();
  pub const PILL_W: f32 = 168.0;
  pub const PILL_H: f32 = 40.0;
  pub const MARGIN: f32 = 16.0;
  pub const BARS: usize = 12;

  pub const FILL: Color = rgb(0x262628);
  pub const DOT: Color = rgb(0xff453a);
  const TEXT: Color = rgb(0xf2f2f7);

  /// The pill: where it is in the overlay.
  pub fn pill() -> Rect {
    Rect::new(MARGIN, MARGIN, PILL_W, PILL_H)
  }

  /// A level from 0 to 1 that wanders like speech, at `t` seconds.
  pub fn level(t: f32) -> f32 {
    let s = (t * 3.1).sin() * 0.5 + (t * 7.3).sin() * 0.3 + (t * 13.7).sin() * 0.2;
    (s * 0.5 + 0.5).clamp(0.0, 1.0)
  }

  /// Recording since the first frame: the meter moves on each frame.
  #[derive(Default)]
  pub struct Hud {
    start: Option<Duration>,
  }

  impl App for Hud {
    fn paint(&mut self, f: &mut Frame) {
      let t = f.now().saturating_sub(*self.start.get_or_insert(f.now()));
      let p = pill();
      let r = p.h / 2.0;
      f.shadow(Rect::new(p.x, p.y + 2.0, p.w, p.h), r, 6.0, alpha([0.0, 0.0, 0.0, 1.0], 0.35));
      f.rect(p, FILL, r);
      f.rect(Rect::new(p.x + 14.0, p.y + 15.0, 10.0, 10.0), DOT, 5.0);

      // The bars, the newest on the right, each the level a tick earlier than its neighbour's.
      for i in 0..BARS {
        let ago = (BARS - 1 - i) as f32 / 30.0;
        let h = 4.0 + 20.0 * level(t.as_secs_f32() - ago);
        let x = p.x + 34.0 + i as f32 * 6.0;
        f.rect(Rect::new(x, p.y + (p.h - h) / 2.0, 3.0, h), TEXT, 1.5);
      }

      let s = t.as_secs();
      let time = format!("{}:{:02}", s / 60, s % 60);
      f.text(p.x + 116.0, p.y + 25.0, &time, Font::mono(13.0).weight(Weight::Medium), TEXT);
      f.animate();
    }
  }

  /// The shot: a second and a half into the recording.
  pub fn shot(h: &mut Harness<Hud>) {
    h.frame();
    h.wait(Duration::from_millis(1500));
  }
}

#[expect(dead_code, reason = "this file uses a part of the helpers")]
mod common;

use std::time::Duration;

use common::{diff, px};
use hud::{CONFIG, DOT, FILL, Hud, PILL_H, pill};
use nimbo::{Anchor, Harness};

/// The HUD as `--shot` renders it: transparent, as its config is an overlay's.
fn hud() -> Harness<Hud> {
  let mut h = Harness::new(Hud::default(), CONFIG.size(), 2.0);
  h.set_transparent(CONFIG.get_overlay().is_some());
  hud::shot(&mut h);
  h
}

#[test]
fn an_overlay_at_the_bottom_shown_on_demand() {
  let shown = (CONFIG.get_overlay(), CONFIG.is_hidden(), CONFIG.is_resizable());
  assert_eq!(shown, (Some(Anchor::Bottom(44.0)), true, false));
}

#[test]
fn transparent_around_the_pill_and_its_shadow() {
  let mut h = hud();
  let img = h.render();
  let p = pill();
  // The corners, clear of the pill's rounding and its shadow.
  for (x, y) in [(1.0, 1.0), (CONFIG.size().w - 1.0, 1.0), (1.0, CONFIG.size().h - 1.0)] {
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
  assert!(h.needs_frame(), "it animates");
  assert!(h.wait(Duration::from_millis(250)) >= 25, "a frame every refresh");
  assert_ne!(bars(&mut h), before);
}
