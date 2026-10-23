// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Pixel helpers shared by the tests: a colour as the frame stores it, the difference of two
//! pixels, a rect's middle. Each test crate uses its own subset of them.
#![allow(dead_code)]

use nimbo::{Color, Rect};

/// A colour as the frame stores an opaque one.
pub fn px(c: Color) -> [u8; 4] {
  c.map(|v| (v * 255.0).round() as u8)
}

/// The largest difference of a channel.
pub fn diff(a: [u8; 4], b: [u8; 4]) -> u8 {
  (0..4).map(|i| a[i].abs_diff(b[i])).max().unwrap()
}

/// The middle of `r`.
pub fn mid(r: Rect) -> (f32, f32) {
  (r.x + r.w / 2.0, r.y + r.h / 2.0)
}
