// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Pixel helpers shared by the tests: a colour as the frame stores it, the difference of two
//! pixels, a rect's middle; and the lines assistive technology reads. Each test crate uses its
//! own subset of them.
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

/// Asserts that the dump of what assistive technology sees ([nimbo::Harness::a11y]) has each of
/// `lines`, indented as it may be.
pub fn reads(dump: &str, lines: &[&str]) {
  for l in lines {
    assert!(dump.lines().any(|d| d.trim_start() == *l), "no {l:?} in\n{dump}");
  }
}

/// Draws the last frame on the CPU and with Metal and asserts they match: no channel more than
/// 2 apart and at most `share` of the pixels differing (both printed, with the 99th percentile of
/// the differences). No Metal device fails.
#[cfg(all(feature = "metal", target_os = "macos"))]
pub fn metal_parity<A: nimbo::App>(h: &mut nimbo::Harness<A>, share: f64) {
  let (cpu, gpu) = (h.render(), h.render_metal().expect("no Metal frame"));
  let (a, b) = (&cpu.pixels, &gpu.pixels);
  assert_eq!((a.w, a.h), (b.w, b.h));
  let pairs = a.data.as_chunks::<4>().0.iter().zip(b.data.as_chunks::<4>().0);
  let mut diffs: Vec<u8> = pairs.map(|(&p, &q)| diff(p, q)).filter(|&d| d > 0).collect();
  diffs.sort_unstable();
  let max = diffs.last().copied().unwrap_or(0);
  let p99 = diffs.get(diffs.len() * 99 / 100).copied().unwrap_or(0);
  let got = diffs.len() as f64 / (a.w * a.h) as f64;
  let what = format!("{}x{} px: max {max}, {:.3}% of pixels, p99 {p99}", a.w, a.h, got * 100.0);
  println!("metal parity, {what}");
  assert!(max <= 2 && got <= share, "{what}");
}
