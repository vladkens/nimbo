// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Every drawing primitive of the scene, in the light or the dark appearance: a rect, rounded
//! corners (one radius, and one per corner), a border all round and one on a single side, a soft
//! shadow, gradients down and across, an image scaled up, lines of four widths, a polyline and
//! a clip.
//!
//! `cargo run --release --example shapes -- --shot shapes.png [--appearance light]`

use std::f32::consts::TAU;

use nimbo::{
  App, Color, Config, Cx, Event, Frame, ImageId, Point, Quad, Rect, Size, alpha, mix, rgb,
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
      Event::Appearance(a) => self.light = !a.dark,
      _ => return false,
    }

    true
  }

  fn paint(&mut self, f: &mut Frame) {
    let p = self.palette();
    f.rect(Rect::new(0.0, 0.0, f.size.w, f.size.h), p.bg, 0.0);

    f.rect(SQUARE, RED, 0.0);
    f.rect(ROUND, BLUE, ROUND_R);
    f.quad(Quad::new(LEAF, GREEN).radii([ROUND_R, 0.0, ROUND_R, 0.0]));
    f.quad(Quad::new(CARD, p.surface).radius(12.0).border(CARD_BORDER, p.border));

    let drop = Rect { y: LIFTED.y + SHADOW_DROP, ..LIFTED };
    f.shadow(drop, 12.0, SHADOW_BLUR, p.shadow);
    f.quad(Quad::new(LIFTED, p.surface).radius(12.0).borders([0.0, 0.0, 0.0, ACCENT_W], AMBER));
    f.quad(Quad::new(DOWN, BLUE).radius(12.0).gradient(GREEN, true));
    f.quad(Quad::new(ACROSS, RED).radius(12.0).gradient(alpha(RED, 0.0), false));
    let image = *self.image.get_or_insert_with(|| f.images.add(IMAGE_N, IMAGE_N, &disc()));
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

#[allow(dead_code)]
fn main() {
  nimbo::run(Config::new("shapes", SIZE.w, SIZE.h), Shapes::default(), |_| {});
}
