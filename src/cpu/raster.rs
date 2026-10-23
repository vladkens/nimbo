// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! One primitive into a band of the frame, its coverage computed per pixel centre and blended
//! premultiplied (One, OneMinusSourceAlpha) into `BGRA8Unorm`, each channel rounded to 8 bits
//! after every draw. Spans of one colour (a panel's inside) skip the per-pixel maths.

use crate::scene::{Color, Quad, Segment, Sprite};

/// A rect of whole pixels, half-open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct IRect {
  pub x0: i32,
  pub y0: i32,
  pub x1: i32,
  pub y1: i32,
}

impl IRect {
  pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
    Self { x0, y0, x1, y1 }
  }

  pub fn intersect(self, o: IRect) -> IRect {
    IRect::new(self.x0.max(o.x0), self.y0.max(o.y0), self.x1.min(o.x1), self.y1.min(o.y1))
  }

  pub fn is_empty(self) -> bool {
    self.x0 >= self.x1 || self.y0 >= self.y1
  }

  pub fn overlaps(self, o: IRect) -> bool {
    !self.intersect(o).is_empty()
  }

  pub fn area(self) -> usize {
    if self.is_empty() { 0 } else { (self.x1 - self.x0) as usize * (self.y1 - self.y0) as usize }
  }

  /// The pixels whose centres lie in a rect given in pixels, as the rasterizer picks them for
  /// an axis-aligned quad (a centre on the left or top edge is in, on the right or bottom out).
  pub fn centres(x0: f32, y0: f32, x1: f32, y1: f32) -> IRect {
    let at = |v: f32| (v - 0.5).ceil().clamp(-1e9, 1e9) as i32;
    IRect::new(at(x0), at(y0), at(x1), at(y1))
  }
}

/// Rows of the frame, BGRA in a `u32` each (B in the low byte), `stride` pixels apart; the
/// first row is frame row `y0`.
pub struct Target<'a> {
  pub px: &'a mut [u32],
  pub stride: usize,
  pub y0: i32,
}

impl Target<'_> {
  fn row(&mut self, y: i32, x0: i32, x1: i32) -> &mut [u32] {
    let at = (y - self.y0) as usize * self.stride;
    &mut self.px[at + x0 as usize..at + x1 as usize]
  }

  /// Fills a rect with one packed pixel.
  pub fn fill(&mut self, r: IRect, v: u32) {
    for y in r.y0..r.y1 {
      self.row(y, r.x0, r.x1).fill(v);
    }
  }
}

/// A straight sRGB colour to premultiplied, as `premul` in the shader (blending in sRGB space),
/// in the frame's channel order: blue, green, red, alpha.
pub fn premul(c: Color) -> [f32; 4] {
  [c[2] * c[3], c[1] * c[3], c[0] * c[3], c[3]]
}

fn scaled(c: [f32; 4], k: f32) -> [f32; 4] {
  [c[0] * k, c[1] * k, c[2] * k, c[3] * k]
}

/// A float to an 8-bit unorm: clamped to 0..=1, scaled by 255, rounded half to even.
#[inline]
fn unorm(x: f32) -> u32 {
  (x.clamp(0.0, 1.0) * 255.0).round_ties_even() as u32
}

/// A premultiplied BGRA colour packed as a pixel.
pub fn pack(c: [f32; 4]) -> u32 {
  u32::from_le_bytes(c.map(|v| unorm(v) as u8))
}

/// `src + dst * (1 - src.a)` per channel, `src` in BGRA order.
#[cfg(any(test, not(target_arch = "aarch64")))]
#[inline]
fn blend_scalar(dst: u32, s: [f32; 4]) -> u32 {
  let k = (1.0 - s[3]) * (1.0 / 255.0);
  let d = dst.to_le_bytes();
  u32::from_le_bytes([0, 1, 2, 3].map(|i| unorm(s[i] + d[i] as f32 * k) as u8))
}

#[cfg(not(target_arch = "aarch64"))]
use blend_scalar as blend;

/// [blend_scalar] in the four lanes of a NEON register (always there on aarch64): the same
/// operations in the same order, so the same pixels; frames draw 15-25% faster.
#[cfg(target_arch = "aarch64")]
#[inline]
fn blend(dst: u32, s: [f32; 4]) -> u32 {
  use std::arch::aarch64::*;

  // SAFETY: NEON is part of the aarch64 baseline; the load reads the 4 floats of `s`.
  unsafe {
    let src = vld1q_f32(s.as_ptr());
    let k = vdupq_n_f32((1.0 - s[3]) * (1.0 / 255.0));
    let d = vcvtq_f32_u32(vmovl_u16(vget_low_u16(vmovl_u8(vcreate_u8(dst as u64)))));
    // A multiply then an add, not a fused multiply-add: the scalar code rounds twice.
    let x = vaddq_f32(src, vmulq_f32(d, k));
    let x = vminq_f32(vmaxq_f32(x, vdupq_n_f32(0.0)), vdupq_n_f32(1.0));
    let x = vrndnq_f32(vmulq_f32(x, vdupq_n_f32(255.0)));
    let b = vmovn_u16(vcombine_u16(vmovn_u32(vcvtq_u32_f32(x)), vdup_n_u16(0)));
    vget_lane_u32::<0>(vreinterpret_u32_u8(b))
  }
}

/// Blends one colour over a run of pixels; an opaque one is stored, and runs of the same
/// destination pixel (a panel over a flat ground) reuse the last result.
fn blend_span(row: &mut [u32], s: [f32; 4]) {
  if s[3] >= 1.0 {
    return row.fill(pack(s));
  }

  let (mut last, mut out) = (!row.first().copied().unwrap_or(0), 0);
  for p in row {
    if *p != last {
      last = *p;
      out = blend(last, s);
    }

    *p = out;
  }
}

fn saturate(x: f32) -> f32 {
  x.clamp(0.0, 1.0)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
  let t = saturate((x - e0) / (e1 - e0));
  t * t * (3.0 - 2.0 * t)
}

fn length(x: f32, y: f32) -> f32 {
  (x * x + y * y).sqrt()
}

/// `mix` in MSL: `x + (y - x) * a`.
fn mix(x: [f32; 4], y: [f32; 4], a: f32) -> [f32; 4] {
  [0, 1, 2, 3].map(|i| x[i] + (y[i] - x[i]) * a)
}

/// Pixel centre in points.
#[inline]
fn centre(i: i32, scale: f32) -> f32 {
  (i as f32 + 0.5) / scale
}

/// Runs `shade` over a row's pixels outside `solid` (pixels the shader fills with `color`),
/// and fills `solid` with `color`.
fn row_spans(
  t: &mut Target,
  y: i32,
  r: IRect,
  solid: (i32, i32),
  color: [f32; 4],
  mut shade: impl FnMut(i32) -> [f32; 4],
) {
  let (s0, s1) = (solid.0.clamp(r.x0, r.x1), solid.1.clamp(r.x0, r.x1));
  let (s0, s1) = if s0 < s1 { (s0, s1) } else { (r.x1, r.x1) };
  let row = t.row(y, r.x0, r.x1);
  let mut px = |x: i32, p: &mut u32| {
    let s = shade(x);
    if s[3] > 0.0 || s[0] > 0.0 || s[1] > 0.0 || s[2] > 0.0 {
      *p = blend(*p, s);
    }
  };
  let (left, rest) = row.split_at_mut((s0 - r.x0) as usize);
  let (mid, right) = rest.split_at_mut((s1 - s0) as usize);
  for (k, p) in left.iter_mut().enumerate() {
    px(r.x0 + k as i32, p);
  }

  if !mid.is_empty() {
    blend_span(mid, color);
  }

  for (k, p) in right.iter_mut().enumerate() {
    px(s1 + k as i32, p);
  }
}

// MARK: Quads

/// The pixel rect a quad's geometry covers: its rect padded as `quad_vs` does; none for an empty
/// rect (an intersection that missed), which the padding would turn into a faint stripe.
pub fn quad_bounds(q: &Quad, scale: f32) -> IRect {
  let r = q.rect;
  if !(r.w > 0.0 && r.h > 0.0) {
    return IRect::default();
  }

  let pad = q.blur * 2.0 + 1.0 / scale;
  IRect::centres(
    (r.x - pad) * scale,
    (r.y - pad) * scale,
    (r.x + r.w + pad) * scale,
    (r.y + r.h + pad) * scale,
  )
}

/// The radius of the corner in the quadrant of `c`, as `pick_radius`.
fn pick_radius(c: (f32, f32), radii: [f32; 4]) -> f32 {
  match (c.0 < 0.0, c.1 < 0.0) {
    (true, true) => radii[0],
    (true, false) => radii[3],
    (false, true) => radii[1],
    (false, false) => radii[2],
  }
}

/// Signed distance to a rounded rect, as `rounded_rect`.
fn rounded_rect(c: (f32, f32), half: (f32, f32), r: f32) -> f32 {
  let q = (c.0.abs() - half.0 + r, c.1.abs() - half.1 + r);
  length(q.0.max(0.0), q.1.max(0.0)) + q.0.max(q.1).min(0.0) - r
}

/// `quad_fs` at the point `p` (points).
fn shade_quad(q: &Quad, p: (f32, f32), scale: f32, fill: [f32; 4], border: [f32; 4]) -> [f32; 4] {
  let half = (q.rect.w * 0.5, q.rect.h * 0.5);
  let c = (p.0 - (q.rect.x + half.0), p.1 - (q.rect.y + half.1));
  if q.blur == 0.0 && q.radii == [0.0; 4] && q.borders == [0.0; 4] {
    let e = (c.0.abs() - half.0, c.1.abs() - half.1);
    return scaled(fill, saturate(0.5 - e.0.max(e.1) * scale));
  }

  let r = pick_radius(c, q.radii).min(half.0.min(half.1));
  let d = rounded_rect(c, half, r);
  if q.blur > 0.0 {
    return scaled(fill, 1.0 - smoothstep(-q.blur, q.blur, d));
  }

  let outer = saturate(0.5 - d * scale);
  if q.borders == [0.0; 4] {
    return scaled(fill, outer);
  }

  let b = q.borders.map(|v| if v == 0.0 { -1.0 / scale } else { v });
  let lo = (q.rect.x + b[3], q.rect.y + b[0]);
  let hi = (q.rect.x + q.rect.w - b[1], q.rect.y + q.rect.h - b[2]);
  let inner_half = (((hi.0 - lo.0) * 0.5).max(0.0), ((hi.1 - lo.1) * 0.5).max(0.0));
  let ci = (p.0 - (lo.0 + hi.0) * 0.5, p.1 - (lo.1 + hi.1) * 0.5);
  // The inner corner is the one of the quadrant around the inner centre, which uneven borders
  // move off the outer one.
  let ro = pick_radius(ci, q.radii).min(half.0.min(half.1));
  let side = (if ci.0 < 0.0 { b[3] } else { b[1] }, if ci.1 < 0.0 { b[0] } else { b[2] });
  let ri = (ro - side.0.max(side.1)).max(0.0).min(inner_half.0.min(inner_half.1));
  // Borders that meet or cross leave no fill.
  let inner = match hi.0 > lo.0 && hi.1 > lo.1 {
    true => saturate(0.5 - rounded_rect(ci, inner_half, ri) * scale),
    false => 0.0,
  };

  scaled(mix(border, fill, inner), outer)
}

/// The pixels a quad fills with exactly its fill colour: each side inset by its corners, its
/// border, the blur and half a pixel. Above and below it, between the corners, the shader's
/// value depends on the row only (the distance to a straight edge).
pub fn quad_solid(q: &Quad, scale: f32) -> IRect {
  let half = (q.rect.w * 0.5).min(q.rect.h * 0.5);
  let [tl, tr, br, bl] = q.radii.map(|v| v.min(half));
  let b = q.borders.map(|v| v.max(0.0));
  let m = q.blur + 0.5 / scale;
  let r = q.rect;
  IRect::centres(
    (r.x + tl.max(bl) + b[3] + m) * scale,
    (r.y + tl.max(tr) + b[0] + m) * scale,
    (r.x + r.w - tr.max(br) - b[1] - m) * scale,
    (r.y + r.h - bl.max(br) - b[2] - m) * scale,
  )
}

/// The x range (points) a row at `cy` from the centre crosses of a rounded rect of half size
/// `half` grown by `m` (shrunk for a negative one), with the left and right corner radii `rl`,
/// `rr`: a rounded rect eroded by `m` is the rect inset by `m` with radii less `m`.
fn chord(cx: f32, cy: f32, half: (f32, f32), rl: f32, rr: f32, m: f32) -> Option<(f32, f32)> {
  let (h, v) = (half.0 + m, half.1 + m);
  let cy = cy.abs();
  if h <= 0.0 || cy > v {
    return None;
  }

  let side = |r: f32| {
    let r = (r + m).max(0.0);
    let a = cy - (v - r);
    if a <= 0.0 { h } else { h - r + (r * r - a * a).max(0.0).sqrt() }
  };
  Some((cx - side(rl), cx + side(rr)))
}

pub fn quad(t: &mut Target, q: &Quad, clip: IRect, scale: f32) {
  let r = quad_bounds(q, scale).intersect(clip);
  let (fill, border) = (premul(q.color), premul(q.border_color));
  if r.is_empty() || fill[3] == 0.0 && border[3] == 0.0 {
    return;
  }

  if q.gradient > 0.0 {
    return gradient(t, q, r, scale, fill, border);
  }

  let px = 1.0 / scale;
  let half = (q.rect.w * 0.5, q.rect.h * 0.5);
  let (cx, cy) = (q.rect.x + half.0, q.rect.y + half.1);
  let radii = q.radii.map(|v| v.min(half.0.min(half.1)));
  let border_w = q.borders.iter().fold(0.0f32, |a, &v| a.max(v));
  // Deeper than this inside, a pixel gets the fill (full coverage, inside the inner edge, past
  // the blur); farther than this outside, nothing.
  let inside = border_w + q.blur + 0.5 * px + px / 64.0;
  let outside = if q.blur > 0.0 { q.blur } else { 0.5 * px } + px;
  let solid = quad_solid(q, scale);
  let span = (solid.x0.max(r.x0), solid.x1.min(r.x1));
  if radii == [0.0; 4] {
    // Straight edges: the shader's value between the sides depends on the row only.
    for y in r.y0..r.y1 {
      let py = centre(y, scale);
      let shade = |x| shade_quad(q, (centre(x, scale), py), scale, fill, border);
      let inner = y >= solid.y0 && y < solid.y1 || span.0 >= span.1;
      row_spans(t, y, r, span, if inner { fill } else { shade(span.0) }, shade);
    }

    return;
  }

  let to_px = |(a, b): (f32, f32)| IRect::centres(a * scale, 0.0, b * scale, 0.0);
  for y in r.y0..r.y1 {
    let py = centre(y, scale);
    let (rl, rr) = if py - cy < 0.0 { (radii[0], radii[1]) } else { (radii[3], radii[2]) };
    let Some(ext) = chord(cx, py - cy, half, rl, rr, outside) else { continue };
    let ext = to_px(ext);
    let row = IRect::new(ext.x0.max(r.x0), y, ext.x1.min(r.x1), y + 1);
    if row.is_empty() {
      continue;
    }

    let shade = |x| shade_quad(q, (centre(x, scale), py), scale, fill, border);
    let (span, color) = match chord(cx, py - cy, half, rl, rr, -inside).map(to_px) {
      Some(c) if c.x0 < c.x1 => ((c.x0.max(row.x0), c.x1.min(row.x1)), fill),
      // Along a straight edge the shader's value depends on the row only.
      _ if span.0 < span.1 && (y < solid.y0 || y >= solid.y1) => (span, shade(span.0)),
      _ => ((0, 0), fill),
    };

    row_spans(t, y, row, span, color, shade);
  }
}

/// A quad filled with a linear gradient from `from` to `to`, as `quad_fs` mixes it per pixel
/// centre. Down the rect, the rows between the corners are one colour inside its sides.
fn gradient(t: &mut Target, q: &Quad, r: IRect, scale: f32, from: [f32; 4], to: [f32; 4]) {
  let down = q.gradient == 1.0;
  let at = |x: f32, y: f32| {
    let k = if down { (y - q.rect.y) / q.rect.h } else { (x - q.rect.x) / q.rect.w };
    mix(from, to, saturate(k))
  };
  let solid = quad_solid(q, scale);
  for y in r.y0..r.y1 {
    let py = centre(y, scale);
    let shade = |x| {
      let px = centre(x, scale);
      shade_quad(q, (px, py), scale, at(px, py), to)
    };
    match down && y >= solid.y0 && y < solid.y1 {
      true => row_spans(t, y, r, (solid.x0, solid.x1), at(0.0, py), shade),
      false => row_spans(t, y, r, (0, 0), [0.0; 4], shade),
    }
  }
}

// MARK: Lines

/// The pixel rect around a segment's geometry (the rotated quad of `line_vs`); none for a
/// butt-capped one of no length, which has no area (a round-capped one is a dot).
pub fn line_bounds(s: &Segment, scale: f32) -> IRect {
  if s.round <= 0.0 && s.a.x == s.b.x && s.a.y == s.b.y {
    return IRect::default();
  }

  let ext = s.width.max(1.0 / scale) * 0.5 + 1.0 / scale;
  let (x0, x1) = (s.a.x.min(s.b.x) - ext, s.a.x.max(s.b.x) + ext);
  let (y0, y1) = (s.a.y.min(s.b.y) - ext, s.a.y.max(s.b.y) + ext);
  IRect::centres(x0 * scale, y0 * scale, x1 * scale, y1 * scale)
}

/// `line_fs` at the point `p`.
fn shade_line(s: &Segment, p: (f32, f32), scale: f32, color: [f32; 4]) -> [f32; 4] {
  let px = 1.0 / scale;
  let w = s.width.max(px);
  let faint = (s.width / px).min(1.0);
  let pa = (p.0 - s.a.x, p.1 - s.a.y);
  let ab = (s.b.x - s.a.x, s.b.y - s.a.y);
  let len2 = ab.0 * ab.0 + ab.1 * ab.1;
  let d = if s.round > 0.0 {
    let h = if len2 > 0.0 { saturate((pa.0 * ab.0 + pa.1 * ab.1) / len2) } else { 0.0 };
    length(pa.0 - ab.0 * h, pa.1 - ab.1 * h) - w * 0.5
  } else {
    let len = len2.sqrt();
    let dir = if len > 0.0 { (ab.0 / len, ab.1 / len) } else { (1.0, 0.0) };
    let t = pa.0 * dir.0 + pa.1 * dir.1;
    let n = (pa.0 * -dir.1 + pa.1 * dir.0).abs();
    (-t).max(t - len).max(n - w * 0.5)
  };

  scaled(color, saturate(0.5 - d * scale) * faint)
}

/// The `x` interval (relative) where `|a * x + b| <= h`.
fn band(a: f32, b: f32, h: f32) -> (f32, f32) {
  if a.abs() < 1e-6 {
    return if b.abs() <= h { (f32::MIN, f32::MAX) } else { (1.0, 0.0) };
  }

  let (u, v) = ((-h - b) / a, (h - b) / a);
  (u.min(v), u.max(v))
}

pub fn line(t: &mut Target, s: &Segment, clip: IRect, scale: f32) {
  let r = line_bounds(s, scale).intersect(clip);
  if r.is_empty() || s.color[3] == 0.0 {
    return;
  }

  let color = premul(s.color);
  // The rotated quad: centre, direction, half its length and half its width, in points.
  let ab = (s.b.x - s.a.x, s.b.y - s.a.y);
  let len = length(ab.0, ab.1);
  let dir = if len > 0.0 { (ab.0 / len, ab.1 / len) } else { (1.0, 0.0) };
  let ext = s.width.max(1.0 / scale) * 0.5 + 1.0 / scale;
  let (mx, my) = ((s.a.x + s.b.x) * 0.5, (s.a.y + s.b.y) * 0.5);
  let hl = len * 0.5 + ext;
  for y in r.y0..r.y1 {
    let dy = centre(y, scale) - my;
    let u = band(dir.0, dy * dir.1, hl);
    let v = band(-dir.1, dy * dir.0, ext);
    let (lo, hi) = (u.0.max(v.0), u.1.min(v.1));
    if lo > hi {
      continue;
    }
    // Outside the quad the coverage is zero, so a pixel more on each side changes nothing.
    let x0 = (((mx + lo) * scale - 0.5).floor() as i32 - 1).max(r.x0);
    let x1 = (((mx + hi) * scale - 0.5).ceil() as i32 + 2).min(r.x1);
    if x0 >= x1 {
      continue;
    }

    let py = centre(y, scale);
    let span = IRect::new(x0, y, x1, y + 1);
    row_spans(t, y, span, (0, 0), color, |x| shade_line(s, (centre(x, scale), py), scale, color));
  }
}

// MARK: Sprites

pub fn sprite_bounds(s: &Sprite, scale: f32) -> IRect {
  let r = s.rect;
  IRect::centres(r.x * scale, r.y * scale, (r.x + r.w) * scale, (r.y + r.h) * scale)
}

/// A texture: `bpp` bytes per texel, rows of `w` texels.
#[derive(Clone, Copy)]
pub struct Tex<'a> {
  pub data: &'a [u8],
  pub w: usize,
  pub h: usize,
}

/// Where each pixel centre of a sprite falls in its texture, per axis: the interpolated `uv`.
struct Map {
  /// Texture coordinate at pixel 0 and per pixel.
  at0: f32,
  step: f32,
}

impl Map {
  fn new(rect_pos: f32, rect_len: f32, uv_pos: f32, uv_len: f32, scale: f32) -> Self {
    let (p0, len) = (rect_pos * scale, rect_len * scale);
    let step = uv_len / len;
    Self { at0: uv_pos + (0.5 - p0) * step, step }
  }

  fn at(&self, i: i32) -> f32 {
    self.at0 + i as f32 * self.step
  }
}

/// `glyph_fs`: coverage from the R8 atlas, nearest texel, times the colour.
pub fn glyph(t: &mut Target, s: &Sprite, atlas: Tex, clip: IRect, scale: f32) {
  let r = sprite_bounds(s, scale).intersect(clip);
  if r.is_empty() || s.color[3] == 0.0 {
    return;
  }

  let color = premul(s.color);
  let solid = pack(color);
  let mu = Map::new(s.rect.x, s.rect.w, s.uv.x, s.uv.w, scale);
  let mv = Map::new(s.rect.y, s.rect.h, s.uv.y, s.uv.h, scale);
  let texel = |v: f32, n: usize| (v.floor().max(0.0) as usize).min(n - 1);
  // Columns map to the same texels on every row: look them up once.
  let mut cols = [0usize; 256];
  let n = (r.x1 - r.x0) as usize;
  let cols = if n <= cols.len() {
    for (k, c) in cols[..n].iter_mut().enumerate() {
      *c = texel(mu.at(r.x0 + k as i32), atlas.w);
    }

    Some(&cols[..n])
  } else {
    None
  };

  for y in r.y0..r.y1 {
    let ty = texel(mv.at(y), atlas.h);
    let src = &atlas.data[ty * atlas.w..(ty + 1) * atlas.w];
    let row = t.row(y, r.x0, r.x1);
    for (k, p) in row.iter_mut().enumerate() {
      let tx = cols.map_or_else(|| texel(mu.at(r.x0 + k as i32), atlas.w), |c| c[k]);
      match src[tx] {
        0 => {}
        255 if color[3] >= 1.0 => *p = solid,
        a => *p = blend(*p, scaled(color, a as f32 * (1.0 / 255.0))),
      }
    }
  }
}

/// A texel of an RGBA texture as BGRA floats 0..1.
#[inline]
fn fetch(tex: Tex, x: usize, y: usize) -> [f32; 4] {
  let i = (y * tex.w + x) * 4;
  let p = &tex.data[i..i + 4];
  [p[2], p[1], p[0], p[3]].map(|v| v as f32 * (1.0 / 255.0))
}

/// Bilinear filtering in pixel coordinates, clamped to the edge, as the `linear` sampler.
fn sample(tex: Tex, u: f32, v: f32) -> [f32; 4] {
  let (u, v) = (u - 0.5, v - 0.5);
  let (fu, fv) = (u.floor(), v.floor());
  let (wu, wv) = (u - fu, v - fv);
  let clamp = |i: f32, n: usize| (i.max(0.0) as usize).min(n - 1);
  let (x0, x1) = (clamp(fu, tex.w), clamp(fu + 1.0, tex.w));
  let (y0, y1) = (clamp(fv, tex.h), clamp(fv + 1.0, tex.h));
  if wu == 0.0 && wv == 0.0 {
    return fetch(tex, x0, y0);
  }

  let (a, b) = (fetch(tex, x0, y0), fetch(tex, x1, y0));
  let (c, d) = (fetch(tex, x0, y1), fetch(tex, x1, y1));
  let top = mix(a, b, wu);
  let bottom = mix(c, d, wu);
  mix(top, bottom, wv)
}

/// `image_fs`: premultiplied RGBA, linear filtering, times the premultiplied tint.
pub fn image(t: &mut Target, s: &Sprite, tex: Tex, clip: IRect, scale: f32) {
  let r = sprite_bounds(s, scale).intersect(clip);
  if r.is_empty() || s.color[3] == 0.0 || tex.w == 0 || tex.h == 0 {
    return;
  }

  let tint = premul(s.color);
  let mu = Map::new(s.rect.x, s.rect.w, s.uv.x, s.uv.w, scale);
  let mv = Map::new(s.rect.y, s.rect.h, s.uv.y, s.uv.h, scale);
  for y in r.y0..r.y1 {
    let v = mv.at(y);
    let row = t.row(y, r.x0, r.x1);
    for (k, p) in row.iter_mut().enumerate() {
      let c = sample(tex, mu.at(r.x0 + k as i32), v);
      if c[3] == 0.0 && c[0] == 0.0 && c[1] == 0.0 && c[2] == 0.0 {
        continue;
      }

      *p = blend(*p, [0, 1, 2, 3].map(|i| c[i] * tint[i]));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scene::{Point, Rect};

  #[test]
  fn borders_that_meet_leave_no_fill() {
    // 5x5 with a 3-point border: the centre pixel is the border's, none of the fill.
    let (fill, border) = ([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
    let q = Quad::new(Rect::new(0.0, 0.0, 5.0, 5.0), fill).border(3.0, border);
    let mut px = [0u32; 25];
    quad(&mut Target { px: &mut px, stride: 5, y0: 0 }, &q, IRect::new(0, 0, 5, 5), 1.0);
    assert_eq!(px[12], pack(premul(border)));
  }

  #[test]
  fn uneven_borders_round_the_inner_corner_they_border() {
    // A circle with only a left border: right of it the top edge shows no border colour.
    let (fill, border) = ([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
    let q = Quad::new(Rect::new(0.0, 0.0, 20.0, 20.0), fill).radius(10.0);
    let q = q.borders([0.0, 0.0, 0.0, 8.0], border);
    let mut px = [0u32; 400];
    quad(&mut Target { px: &mut px, stride: 20, y0: 0 }, &q, IRect::new(0, 0, 20, 20), 1.0);
    for (y, row) in px.chunks(20).enumerate().take(4) {
      assert!(row[9..].iter().all(|p| p & 0xff == 0), "row {y}: {:08x?}", &row[9..]);
    }
  }

  #[test]
  fn empty_quads_draw_nothing() {
    // A plain one would cover the pixels at its edge by half; a shadow would spread.
    let mut px = [0u32; 25];
    for rect in [Rect::new(1.5, 1.0, 0.0, 4.0), Rect::new(1.0, 1.5, 4.0, 0.0)] {
      let q = Quad::new(rect, [1.0; 4]);
      quad(&mut Target { px: &mut px, stride: 5, y0: 0 }, &q, IRect::new(0, 0, 5, 5), 1.0);
      assert!(quad_bounds(&q.blur(1.0), 1.0).is_empty());
    }

    assert_eq!(px, [0; 25]);
  }

  #[test]
  fn a_point_draws_only_with_round_caps() {
    let a = Point::new(2.5, 2.5);
    let seg = |round| Segment { a, b: a, color: [1.0; 4], width: 2.0, round, _pad: [0.0; 2] };
    let draw = |s: &Segment| {
      let mut px = [0u32; 25];
      line(&mut Target { px: &mut px, stride: 5, y0: 0 }, s, IRect::new(0, 0, 5, 5), 1.0);
      px
    };
    assert!(line_bounds(&seg(0.0), 1.0).is_empty());
    assert_eq!(draw(&seg(0.0)), [0; 25]);
    assert_eq!(draw(&seg(1.0))[12], !0);
  }

  #[test]
  fn centres_pick_pixels() {
    assert_eq!(IRect::centres(0.0, 0.0, 2.0, 1.0), IRect::new(0, 0, 2, 1));
    assert_eq!(IRect::centres(0.5, 0.4, 1.6, 1.5), IRect::new(0, 0, 2, 1));
    assert_eq!(IRect::centres(-1.0, 0.0, 0.5, 0.5), IRect::new(-1, 0, 0, 0));
  }

  #[test]
  fn blend_matches_the_scalar_maths() {
    // Random destinations under random premultiplied colours.
    let mut seed = 1u32;
    let mut rand = || {
      seed ^= seed << 13;
      seed ^= seed >> 17;
      seed ^= seed << 5;
      seed
    };
    for _ in 0..200_000 {
      let a = (rand() % 1001) as f32 / 1000.0;
      let c = [0, 1, 2].map(|_| (rand() % 1001) as f32 / 1000.0);
      let s = scaled([c[0], c[1], c[2], 1.0], a);
      let dst = rand();
      assert_eq!(blend(dst, s), blend_scalar(dst, s), "{dst:08x} {s:?}");
    }
  }

  #[test]
  fn blend_rounds_like_unorm() {
    let black = 0xff00_0000;
    assert_eq!(blend(black, [1.0, 1.0, 1.0, 1.0]), 0xffff_ffff);
    // Half white over black: 127.5 rounds to even.
    assert_eq!(blend(black, [0.5, 0.5, 0.5, 0.5]) & 0xff, 128);
    // Red is the third byte.
    assert_eq!(pack(premul([1.0, 0.0, 0.0, 1.0])), 0xffff_0000);
  }

  #[test]
  fn plain_quad_covers_its_pixels() {
    let mut px = vec![0xff00_0000u32; 16];
    let mut t = Target { px: &mut px, stride: 4, y0: 0 };
    let q = Quad::new(crate::scene::Rect::new(1.0, 1.0, 2.0, 2.0), [1.0; 4]);
    quad(&mut t, &q, IRect::new(0, 0, 4, 4), 1.0);
    let lit: Vec<_> = px.iter().map(|&p| p == 0xffff_ffff).collect();
    let want = [0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0].map(|v| v == 1);
    assert_eq!(lit, want);
  }
}
