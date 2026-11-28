// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests through the harness of the app below, in the dark and the light appearance: the colours at
//! known points of every shape, a rounded corner's antialiased edge, a border all round and one on
//! a single side, a shadow fading out, the gradients, the image, the lines and a clip; and frames
//! built only when something changed.

#[expect(dead_code, reason = "what the tests don't read")]
mod app {

  //! Every drawing primitive of the scene, in the light or the dark appearance: a rect, rounded
  //! corners (one radius, and one per corner), a border all round and one on a single side, a soft
  //! shadow, gradients down and across, an image scaled up, lines of four widths, a polyline and
  //! a clip.

  use std::f32::consts::TAU;

  use nimbo::{
    App, Axis, Color, Cx, Event, Frame, ImageId, Point, Quad, Rect, Size, alpha, mix, rgb,
  };

  pub const SIZE: Size = Size::new(480.0, 340.0);

  /// The colours of one appearance.
  pub struct Palette {
    pub bg: Color,
    pub surface: Color,
    pub border: Color,
    pub ink: Color,
    pub shadow: Color,
  }

  pub const DARK: Palette = Palette {
    bg: rgb(0x1c1c1e),
    surface: rgb(0x2c2c2e),
    border: rgb(0x636366),
    ink: rgb(0xe5e5ea),
    shadow: [0.0, 0.0, 0.0, 0.6],
  };

  pub const LIGHT: Palette = Palette {
    bg: rgb(0xf2f2f7),
    surface: rgb(0xffffff),
    border: rgb(0xaeaeb2),
    ink: rgb(0x3a3a3c),
    shadow: [0.0, 0.0, 0.0, 0.3],
  };

  pub const RED: Color = rgb(0xe5484d);
  pub const BLUE: Color = rgb(0x3b82f6);
  pub const GREEN: Color = rgb(0x30a46c);
  pub const AMBER: Color = rgb(0xf5a524);

  // Where each shape is, in points. The first row:
  /// A plain rect.
  pub const SQUARE: Rect = Rect::new(24.0, 24.0, 88.0, 88.0);
  /// A rounded rect, [ROUND_R] at every corner.
  pub const ROUND: Rect = Rect::new(136.0, 24.0, 88.0, 88.0);
  pub const ROUND_R: f32 = 24.0;
  /// A leaf: two opposite corners rounded.
  pub const LEAF: Rect = Rect::new(248.0, 24.0, 88.0, 88.0);
  /// A card with a border of [CARD_BORDER] all round.
  pub const CARD: Rect = Rect::new(360.0, 24.0, 96.0, 88.0);
  pub const CARD_BORDER: f32 = 2.0;

  // The second row:
  /// A card over a soft shadow [SHADOW_DROP] lower and [SHADOW_BLUR] wide, with an accent border
  /// on its left side only.
  pub const LIFTED: Rect = Rect::new(24.0, 136.0, 88.0, 72.0);
  pub const SHADOW_DROP: f32 = 6.0;
  pub const SHADOW_BLUR: f32 = 12.0;
  pub const ACCENT_W: f32 = 4.0;
  /// A gradient from blue at the top to green at the bottom.
  pub const DOWN: Rect = Rect::new(136.0, 136.0, 88.0, 88.0);
  /// A gradient from red at the left fading out to the right.
  pub const ACROSS: Rect = Rect::new(248.0, 136.0, 88.0, 88.0);
  /// An image of [IMAGE_N] × [IMAGE_N] pixels scaled up: a disc shaded from blue at its top-left
  /// to amber at its bottom-right, transparent around it.
  pub const PICTURE: Rect = Rect::new(360.0, 136.0, 88.0, 88.0);
  pub const IMAGE_N: usize = 64;

  // The third row:
  /// Horizontal lines from x 24 to 224, at these heights and widths.
  pub const LINES: [(f32, f32); 4] = [(256.0, 0.5), (270.0, 1.0), (286.0, 2.0), (304.0, 4.0)];
  pub const LINES_X: (f32, f32) = (24.0, 224.0);
  /// A panel that clips a wave and a circle drawn past its edges to the inside of its border.
  pub const PANEL: Rect = Rect::new(248.0, 248.0, 208.0, 68.0);
  /// The circle in the panel, a rounded square cut by its top and right edges.
  pub const CIRCLE: Rect = Rect::new(416.0, 230.0, 80.0, 80.0);

  #[derive(Default)]
  pub struct Shapes {
    /// The light appearance; dark by default.
    pub light: bool,
    image: Option<ImageId>,
  }

  impl Shapes {
    pub fn palette(&self) -> &'static Palette {
      if self.light { &LIGHT } else { &DARK }
    }
  }

  /// The image's straight RGBA pixels, top row first.
  fn disc() -> Vec<u8> {
    let n = IMAGE_N as f32;
    let mut px = Vec::with_capacity(IMAGE_N * IMAGE_N * 4);
    for y in 0..IMAGE_N {
      for x in 0..IMAGE_N {
        let (cx, cy) = (x as f32 + 0.5 - n / 2.0, y as f32 + 0.5 - n / 2.0);
        let c = mix(BLUE, AMBER, (x + y) as f32 / (2.0 * n - 2.0));
        // Antialiased: the edge a pixel wide, a pixel in from the image's.
        let a = (n / 2.0 - 0.5 - (cx * cx + cy * cy).sqrt()).clamp(0.0, 1.0);
        px.extend(alpha(c, a).map(|v| (v * 255.0).round() as u8));
      }
    }

    px
  }

  impl App for Shapes {
    fn event(&mut self, _cx: &mut Cx, ev: &Event) -> bool {
      match ev {
        Event::Appearance(a) => self.light = !a.scheme.is_dark(),
        _ => return false,
      }

      true
    }

    fn paint(&mut self, f: &mut Frame) {
      let p = self.palette();
      f.rect(Rect::new(0.0, 0.0, f.size().w, f.size().h), p.bg, 0.0);

      f.rect(SQUARE, RED, 0.0);
      f.rect(ROUND, BLUE, ROUND_R);
      f.quad(Quad::new(LEAF, GREEN).radii([ROUND_R, 0.0, ROUND_R, 0.0]));
      f.quad(Quad::new(CARD, p.surface).radius(12.0).border(CARD_BORDER, p.border));

      let drop = Rect { y: LIFTED.y + SHADOW_DROP, ..LIFTED };
      f.shadow(drop, 12.0, SHADOW_BLUR, p.shadow);
      f.quad(Quad::new(LIFTED, p.surface).radius(12.0).borders([0.0, 0.0, 0.0, ACCENT_W], AMBER));
      f.quad(Quad::new(DOWN, BLUE).radius(12.0).gradient(GREEN, Axis::Vertical));
      f.quad(Quad::new(ACROSS, RED).radius(12.0).gradient(alpha(RED, 0.0), Axis::Horizontal));
      let image = *self.image.get_or_insert_with(|| f.images().add(IMAGE_N, IMAGE_N, &disc()));
      f.image(image, PICTURE, 1.0);

      for (y, w) in LINES {
        f.line(Point::new(LINES_X.0, y), Point::new(LINES_X.1, y), w, p.ink);
      }

      f.quad(Quad::new(PANEL, p.surface).border(1.0, p.border));
      f.push_clip(Rect::new(PANEL.x + 1.0, PANEL.y + 1.0, PANEL.w - 2.0, PANEL.h - 2.0));
      f.rect(CIRCLE, GREEN, CIRCLE.w / 2.0);
      // One and a half periods of a sine, taller than the panel.
      let wave: Vec<Point> = (0..=48)
        .map(|i| {
          let t = i as f32 / 48.0;
          let y = PANEL.y + PANEL.h / 2.0 - 44.0 * (t * 1.5 * TAU).sin();
          Point::new(PANEL.x + t * PANEL.w, y)
        })
        .collect();
      f.polyline(&wave, 3.0, AMBER);
      f.pop_clip();
    }
  }
}

use app::*;
#[expect(dead_code, reason = "this file uses a part of the helpers")]
mod common;

use common::{diff, mid, px};
use nimbo::harness::Image;
use nimbo::{Appearance, Event, Harness, mix};

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
  assert_eq!(img.size(), SIZE);
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
