// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Geometry and colour, the vocabulary every layer draws and lays out with: points, sizes, rects,
//! sRGB colours and the two axes.

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
  pub x: f32,
  pub y: f32,
}

impl Point {
  pub const fn new(x: f32, y: f32) -> Self {
    Self { x, y }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
  pub w: f32,
  pub h: f32,
}

impl Size {
  pub const fn new(w: f32, h: f32) -> Self {
    Self { w, h }
  }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
  pub x: f32,
  pub y: f32,
  pub w: f32,
  pub h: f32,
}

impl Rect {
  pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
    Self { x, y, w, h }
  }

  pub fn contains(&self, p: Point) -> bool {
    p.x >= self.x && p.x < self.x + self.w && p.y >= self.y && p.y < self.y + self.h
  }

  /// The overlap of two rects; empty (zero size) when they don't overlap.
  pub fn intersect(&self, o: Rect) -> Rect {
    let (x, y) = (self.x.max(o.x), self.y.max(o.y));
    let w = ((self.x + self.w).min(o.x + o.w) - x).max(0.0);
    let h = ((self.y + self.h).min(o.y + o.h) - y).max(0.0);
    Rect::new(x, y, w, h)
  }
}

/// Straight (not premultiplied) RGBA, 0..1, in sRGB.
pub type Color = [f32; 4];

pub const fn rgb(hex: u32) -> Color {
  let (r, g, b) = ((hex >> 16) & 0xff, (hex >> 8) & 0xff, hex & 0xff);
  [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}

pub const fn alpha(c: Color, a: f32) -> Color {
  [c[0], c[1], c[2], a]
}

/// From `a` at `t` = 0 to `b` at 1, per channel (in sRGB, as Sublime's theme blends).
pub const fn mix(a: Color, b: Color, t: f32) -> Color {
  [
    a[0] + (b[0] - a[0]) * t,
    a[1] + (b[1] - a[1]) * t,
    a[2] + (b[2] - a[2]) * t,
    a[3] + (b[3] - a[3]) * t,
  ]
}

/// A direction, as SwiftUI's `Axis`: along x or along y.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
  Horizontal,
  Vertical,
}

impl Axis {
  /// Its place in a `[x, y]` pair.
  pub(crate) const fn index(self) -> usize {
    self as usize
  }
}
