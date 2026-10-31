// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/shapes.rs` through the harness, in the dark and the light appearance: the
//! colours at known points of every shape, a rounded corner's antialiased edge, a border all
//! round and one on a single side, a shadow fading out, the gradients, the image, the lines and a
//! clip; and frames built only when something changed.

#[path = "../examples/shapes.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod shapes;

mod common;

use common::{diff, mid, px};
use nimbo::harness::Image;
use nimbo::{Appearance, Event, Harness, mix};
use shapes::*;

fn check(img: &Image, p: &Palette) {
  let at = |x: f32, y: f32| img.pixel(x, y);
  let bg = px(p.bg);
  assert_eq!(at(5.0, 5.0), bg);

  // A plain rect: its colour to its edge, the ground a pixel out.
  let (x, y) = mid(SQUARE);
  assert_eq!(at(x, y), px(RED));
  assert_eq!(at(SQUARE.x, y), px(RED));
  assert_eq!(at(SQUARE.x - 0.5, y), bg);

  // A rounded corner: the ground in the corner, then along a row 4 points down the colour comes
  // in steadily, over a pixel or two of antialiasing where the arc crosses it.
  let (x, y) = mid(ROUND);
  assert_eq!(at(x, y), px(BLUE));
  assert_eq!(at(ROUND.x + 1.0, ROUND.y + 1.0), bg);
  let row: Vec<_> =
    (0..ROUND_R as usize * 2).map(|k| at(ROUND.x + k as f32 / 2.0, ROUND.y + 4.0)).collect();
  assert_eq!((row[0], *row.last().unwrap()), (bg, px(BLUE)));
  let to_blue: Vec<_> = row.iter().map(|&c| diff(c, px(BLUE))).collect();
  assert!(to_blue.is_sorted_by(|a, b| a >= b), "{row:?}");
  let edge = row.iter().filter(|&&c| c != bg && c != px(BLUE)).count();
  assert!((1..=2).contains(&edge), "{edge} antialiased pixels: {row:?}");

  // Radii per corner: the top-left rounded, the top-right square.
  assert_eq!(at(LEAF.x + 1.0, LEAF.y + 1.0), bg);
  assert_eq!(at(LEAF.x + LEAF.w - 1.0, LEAF.y + 1.0), px(GREEN));

  // A border all round, the fill inside it.
  let (x, y) = mid(CARD);
  assert_eq!(at(CARD.x - 1.0, y), bg);
  assert_eq!(at(CARD.x + CARD_BORDER / 2.0, y), px(p.border));
  assert_eq!(at(CARD.x + CARD.w - CARD_BORDER / 2.0, y), px(p.border));
  assert_eq!(at(x, CARD.y + CARD_BORDER / 2.0), px(p.border));
  assert_eq!(at(CARD.x + CARD_BORDER + 1.0, y), px(p.surface));

  // A border on the left side only.
  let (x, y) = mid(LIFTED);
  assert_eq!(at(LIFTED.x + 1.0, y), px(AMBER));
  assert_eq!(at(LIFTED.x + ACCENT_W + 1.0, y), px(p.surface));
  assert_eq!(at(LIFTED.x + LIFTED.w - 1.0, y), px(p.surface));
  assert_eq!(at(x, LIFTED.y + 0.5), px(p.surface));

  // Its shadow below it fades out steadily, gone past the blur.
  let bottom = LIFTED.y + LIFTED.h;
  let shade: Vec<_> = [2.0, 6.0, 10.0, 14.0, SHADOW_DROP + SHADOW_BLUR + 1.0]
    .map(|dy| diff(at(x, bottom + dy), bg))
    .to_vec();
  assert!(shade.is_sorted_by(|a, b| a > b) && shade[4] == 0, "{shade:?}");

  // Gradients: blue at the top to green at the bottom; red at the left fading into the ground.
  let (x, y) = mid(DOWN);
  assert!(diff(at(x, DOWN.y), px(BLUE)) <= 3);
  assert!(diff(at(x, y), px(mix(BLUE, GREEN, 0.5))) <= 3);
  assert!(diff(at(x, DOWN.y + DOWN.h - 0.5), px(GREEN)) <= 3);
  let (x, y) = mid(ACROSS);
  assert!(diff(at(ACROSS.x, y), px(RED)) <= 3);
  assert!(diff(at(x, y), px(mix(RED, p.bg, 0.5))) <= 3);
  assert!(diff(at(ACROSS.x + ACROSS.w - 0.5, y), bg) <= 3);

  // The image: its colours in the middle, the ground through its transparent corner.
  let (x, y) = mid(PICTURE);
  assert!(diff(at(x, y), px(mix(BLUE, AMBER, 0.5))) <= 4, "{:?}", at(x, y));
  assert_eq!(at(PICTURE.x + 2.0, PICTURE.y + 2.0), bg);

  // Lines a point wide and more cover their pixels; half a point is one pixel, half on each of
  // two rows here.
  let x = (LINES_X.0 + LINES_X.1) / 2.0;
  for (y, w) in &LINES[1..] {
    assert_eq!(at(x, *y), px(p.ink), "{w} wide");
    assert_eq!(at(x, y + w / 2.0 + 0.5), bg, "{w} wide");
  }

  let thin = at(x, LINES[0].0);
  assert!(thin != bg && thin != px(p.ink), "{thin:?}");

  // The clip: the circle shows inside the panel, not above it, and neither it nor the wave
  // covers the panel's border.
  assert_eq!(at(450.0, 260.0), px(GREEN));
  assert_eq!(at(450.0, 240.0), bg);
  assert_eq!(at(PANEL.x + PANEL.w - 0.5, 270.0), px(p.border));
  assert_eq!(at(PANEL.x + 0.5, PANEL.y + PANEL.h / 2.0), px(p.border));
}

#[test]
fn every_shape_in_the_dark() {
  let mut h = Harness::new(Shapes::default(), SIZE, 2.0);
  check(&h.render(), &DARK);
  assert!(!h.frame(), "a frame without changes");
}

#[test]
fn every_shape_in_the_light() {
  let mut h = Harness::new(Shapes::default(), SIZE, 2.0);
  h.render();
  assert!(h.event(Event::Appearance(Appearance::LIGHT)));
  check(&h.render(), &LIGHT);
  assert!(!h.frame());
}

#[test]
fn at_scale_1() {
  let mut h = Harness::new(Shapes::default(), SIZE, 1.0);
  h.event(Event::Appearance(Appearance::LIGHT));
  let img = h.render();
  assert_eq!((img.pixels.w, img.pixels.h), (SIZE.w as usize, SIZE.h as usize));
  assert_eq!(img.pixel(5.0, 5.0), px(LIGHT.bg));
  assert_eq!(img.pixel(mid(SQUARE).0, mid(SQUARE).1), px(RED));
}

/// An event the app doesn't use builds no frame; a wake from another thread builds one.
#[test]
fn frames_only_when_something_changed() {
  let mut h = Harness::new(Shapes::default(), SIZE, 2.0);
  h.render();
  assert!(!h.move_to(5.0, 5.0));
  assert!(!h.frame());
  let waker = h.waker().clone();
  std::thread::spawn(move || waker.wake()).join().unwrap();
  assert!(h.frame());
  assert!(!h.frame());
}

#[test]
fn a_change_through_app_mut_redraws() {
  let mut h = Harness::new(Shapes::default(), SIZE, 2.0);
  h.event(Event::Appearance(Appearance::LIGHT));
  check(&h.render(), &LIGHT);
  h.app_mut().light = false;
  check(&h.render(), &DARK);
}

/// The Metal renderer draws every shape as the CPU does, in both appearances at scale 2 and 1:
/// within 2% of the pixels, for the GPU's bilinear weights in the up-scaled image and the edges
/// of the diagonal lines.
#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn metal_draws_as_the_cpu() {
  let mut h = Harness::new(Shapes::default(), SIZE, 2.0);
  let (dark, light) = (Appearance::DARK, Appearance::LIGHT);
  for (scale, look) in [(2.0, dark), (2.0, light), (1.0, light), (1.0, dark)] {
    h.resize(SIZE, scale);
    h.event(Event::Appearance(look));
    common::metal_parity(&mut h, 0.02);
  }
}
