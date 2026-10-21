// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The frame's draw list and the paint API that fills it. Coordinates are points, y down; the
//! renderer scales them to pixels. Buffers are kept across frames, so a steady frame allocates
//! nothing.

use std::fmt::{Display, Write};
use std::time::Duration;

use crate::text::{Font, TextSystem};

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

// MARK: Instances

/// One instance of the rounded-rect pipeline.
/// Corners and sides go clockwise from the top-left, as in CSS.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Quad {
  pub rect: Rect,
  pub color: Color,
  pub border_color: Color,
  /// Corner radii: top-left, top-right, bottom-right, bottom-left.
  pub radii: [f32; 4],
  /// Border widths inside the rect: top, right, bottom, left.
  pub borders: [f32; 4],
  /// Blur radius; a quad with a blur draws only its soft shadow.
  pub blur: f32,
  /// A linear gradient from `color` to `border_color` ([Quad::gradient]): 1 down the rect, 2
  /// across it, 0 none.
  pub gradient: f32,
  pub _pad: [f32; 2],
}

impl Quad {
  pub const fn new(rect: Rect, color: Color) -> Self {
    let z = [0.0; 4];
    let (blur, gradient, _pad) = (0.0, 0.0, [0.0; 2]);
    Self { rect, color, border_color: z, radii: z, borders: z, blur, gradient, _pad }
  }

  /// Fills with a linear gradient from `color` at the top (or left) edge to `to` at the bottom
  /// (or right), down the rect when `vertical`, else across it: an edge that fades. Mixed
  /// premultiplied, so a fade to a transparent colour thins out evenly. No border: the border
  /// colour is the gradient's end.
  pub const fn gradient(self, to: Color, vertical: bool) -> Self {
    let gradient = if vertical { 1.0 } else { 2.0 };
    Self { border_color: to, borders: [0.0; 4], gradient, ..self }
  }

  pub const fn radius(self, r: f32) -> Self {
    Self { radii: [r; 4], ..self }
  }

  pub const fn radii(self, radii: [f32; 4]) -> Self {
    Self { radii, ..self }
  }

  pub const fn border(self, w: f32, color: Color) -> Self {
    Self { borders: [w; 4], border_color: color, ..self }
  }

  pub const fn borders(self, borders: [f32; 4], color: Color) -> Self {
    Self { borders, border_color: color, ..self }
  }

  /// Draws only a soft shadow of the shape, `blur` points wide on each side of its edge.
  pub const fn blur(self, blur: f32) -> Self {
    Self { blur, ..self }
  }
}

/// A rect on screen in points and its source rect in a texture in pixels. Glyphs use `color` as
/// their colour, images and emoji as a tint.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Sprite {
  pub rect: Rect,
  pub uv: Rect,
  pub color: Color,
}

/// A thick antialiased line from `a` to `b`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Segment {
  pub a: Point,
  pub b: Point,
  pub color: Color,
  /// In points; lines under a pixel wide draw one pixel wide and fainter.
  pub width: f32,
  /// 1 for round caps (polyline joints), 0 for butt caps.
  pub round: f32,
  pub _pad: [f32; 2],
}

/// An RGBA image added through [Images].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageId(pub u32);

/// What a layer draws, in this order: quads, images, lines, glyphs, colour emoji.
#[derive(Default)]
pub struct Layer {
  pub clip: Option<Rect>,
  pub quads: Vec<Quad>,
  pub images: Vec<Sprite>,
  /// The image of each sprite in `images`.
  pub image_ids: Vec<ImageId>,
  pub lines: Vec<Segment>,
  pub glyphs: Vec<Sprite>,
  pub emoji: Vec<Sprite>,
}

impl Layer {
  fn clear(&mut self) {
    self.quads.clear();
    self.images.clear();
    self.image_ids.clear();
    self.lines.clear();
    self.glyphs.clear();
    self.emoji.clear();
  }

  fn is_empty(&self) -> bool {
    self.quads.is_empty()
      && self.images.is_empty()
      && self.lines.is_empty()
      && self.glyphs.is_empty()
      && self.emoji.is_empty()
  }
}

/// Instance counts of a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
  pub quads: usize,
  pub images: usize,
  pub lines: usize,
  pub glyphs: usize,
}

/// The layers of one frame and the clip stack. Cleared, not dropped, between frames: `layers`
/// keeps every `Vec` it ever grew and `len` says how many are in use.
#[derive(Default)]
pub struct Scene {
  layers: Vec<Layer>,
  len: usize,
  clips: Vec<Rect>,
}

impl Scene {
  pub fn clear(&mut self) {
    self.len = 0;
    self.clips.clear();
  }

  pub fn layers(&self) -> &[Layer] {
    &self.layers[..self.len]
  }

  /// The current clip: the intersection of every pushed one, `None` for the whole frame.
  pub fn clip(&self) -> Option<Rect> {
    self.clips.last().copied()
  }

  /// Starts a new layer drawn over the previous ones, under the current clip. An empty top
  /// layer is reused.
  pub fn layer(&mut self) {
    let clip = self.clip();
    if self.len > 0 && self.layers[self.len - 1].is_empty() {
      self.layers[self.len - 1].clip = clip;
      return;
    }

    if self.len == self.layers.len() {
      self.layers.push(Layer::default());
    }

    let layer = &mut self.layers[self.len];
    layer.clip = clip;
    layer.clear();
    self.len += 1;
  }

  /// Clips what follows to `r` within the current clip, in a new layer.
  pub fn push_clip(&mut self, r: Rect) {
    let r = self.clip().map_or(r, |c| c.intersect(r));
    self.clips.push(r);
    self.layer();
  }

  /// Restores the clip before the last `push_clip`, in a new layer.
  pub fn pop_clip(&mut self) {
    self.clips.pop();
    self.layer();
  }

  fn top(&mut self) -> &mut Layer {
    if self.len == 0 {
      self.layer();
    }

    &mut self.layers[self.len - 1]
  }

  pub fn quad(&mut self, q: Quad) {
    self.top().quads.push(q);
  }

  pub fn image(&mut self, id: ImageId, s: Sprite) {
    let top = self.top();
    top.images.push(s);
    top.image_ids.push(id);
  }

  pub fn segment(&mut self, s: Segment) {
    self.top().lines.push(s);
  }

  pub fn glyph(&mut self, s: Sprite) {
    self.top().glyphs.push(s);
  }

  pub fn emoji(&mut self, s: Sprite) {
    self.top().emoji.push(s);
  }

  pub fn counts(&self) -> Counts {
    let sum = |f: fn(&Layer) -> usize| self.layers().iter().map(f).sum();
    Counts {
      quads: sum(|l| l.quads.len()),
      images: sum(|l| l.images.len()),
      lines: sum(|l| l.lines.len()),
      glyphs: sum(|l| l.glyphs.len() + l.emoji.len()),
    }
  }
}

// MARK: Images

/// A new image waiting for the renderer: premultiplied RGBA rows, top row first.
pub struct ImageUpload {
  pub id: ImageId,
  pub w: usize,
  pub h: usize,
  pub data: Vec<u8>,
}

/// RGBA images by id. Adding one queues its pixels for the renderer, which keeps a texture
/// per image until it is removed.
#[derive(Default)]
pub struct Images {
  /// Pixel size per id, `None` for a removed one.
  sizes: Vec<Option<(usize, usize)>>,
  free: Vec<u32>,
  pub(crate) added: Vec<ImageUpload>,
  pub(crate) removed: Vec<ImageId>,
}

impl Images {
  /// Adds an image from straight (not premultiplied) RGBA pixels, `w * h * 4` bytes, top row
  /// first.
  pub fn add(&mut self, w: usize, h: usize, rgba: &[u8]) -> ImageId {
    assert_eq!(rgba.len(), w * h * 4, "image pixels");
    let id = match self.free.pop() {
      Some(i) => {
        self.sizes[i as usize] = Some((w, h));
        ImageId(i)
      }
      None => {
        self.sizes.push(Some((w, h)));
        ImageId(self.sizes.len() as u32 - 1)
      }
    };

    let premultiply = |p: &[u8; 4]| {
      let a = p[3] as u32;
      [0, 1, 2].map(|i| ((p[i] as u32 * a + 127) / 255) as u8).into_iter().chain([p[3]])
    };
    let data = rgba.as_chunks::<4>().0.iter().flat_map(premultiply).collect();
    self.added.push(ImageUpload { id, w, h, data });
    id
  }

  pub fn remove(&mut self, id: ImageId) {
    let Some(slot) = self.sizes.get_mut(id.0 as usize).filter(|s| s.is_some()) else { return };
    *slot = None;
    self.added.retain(|u| u.id != id);
    self.removed.push(id);
    self.free.push(id.0);
  }

  /// Width and height in pixels.
  pub fn size(&self, id: ImageId) -> Option<(usize, usize)> {
    self.sizes.get(id.0 as usize).copied().flatten()
  }
}

// MARK: Paint API

/// What `App::paint` draws with: the scene, the text system, the images and the frame's
/// inputs.
pub struct Frame<'a> {
  pub size: Size,
  /// Pixels per point.
  pub scale: f32,
  /// The pointer in points, `None` when it is outside the view.
  pub mouse: Option<Point>,
  /// Time from the runtime's clock: real in a window, virtual in the harness.
  pub now: Duration,
  pub scene: &'a mut Scene,
  pub text: &'a mut TextSystem,
  pub images: &'a mut Images,
  pub(crate) animate: bool,
}

impl Frame<'_> {
  /// Asks for another frame after this one (a running animation).
  pub fn animate(&mut self) {
    self.animate = true;
  }

  pub fn hover(&self, r: Rect) -> bool {
    self.mouse.is_some_and(|m| r.contains(m))
  }

  /// Starts a new layer over everything drawn so far. Within a layer each kind draws in a fixed
  /// order, not the order of the calls: quads, images, lines, glyphs, colour emoji ([Layer]);
  /// so a quad drawn after a line (a label over a plot's lines) needs a layer of its own.
  pub fn layer(&mut self) {
    self.scene.layer();
  }

  /// Clips what follows to `r`, intersected with the current clip, until `pop_clip`.
  pub fn push_clip(&mut self, r: Rect) {
    self.scene.push_clip(r);
  }

  pub fn pop_clip(&mut self) {
    self.scene.pop_clip();
  }

  pub fn rect(&mut self, rect: Rect, color: Color, radius: f32) {
    self.scene.quad(Quad::new(rect, color).radius(radius));
  }

  /// A rect with any corner radii, borders and blur; see [Quad].
  pub fn quad(&mut self, q: Quad) {
    self.scene.quad(q);
  }

  pub fn shadow(&mut self, rect: Rect, radius: f32, blur: f32, color: Color) {
    self.scene.quad(Quad::new(rect, color).radius(radius).blur(blur));
  }

  /// A straight line with butt caps.
  pub fn line(&mut self, a: Point, b: Point, width: f32, color: Color) {
    self.scene.segment(Segment { a, b, color, width, round: 0.0, _pad: [0.0; 2] });
  }

  /// Connected lines with round joints, one segment per pair of points. The joints overlap,
  /// so a translucent colour shows them darker.
  pub fn polyline(&mut self, points: &[Point], width: f32, color: Color) {
    for w in points.windows(2) {
      self.scene.segment(Segment { a: w[0], b: w[1], color, width, round: 1.0, _pad: [0.0; 2] });
    }
  }

  /// Draws an image into `rect`, scaled with linear filtering; `opacity` 0..1.
  pub fn image(&mut self, id: ImageId, rect: Rect, opacity: f32) {
    let Some((w, h)) = self.images.size(id) else { return };
    let uv = Rect::new(0.0, 0.0, w as f32, h as f32);
    self.scene.image(id, Sprite { rect, uv, color: [1.0, 1.0, 1.0, opacity] });
  }

  /// The advance width of `s` in points.
  pub fn measure(&mut self, s: &str, font: Font) -> f32 {
    self.text.measure(s, font)
  }

  /// Draws `s`, anything `Display` (formatted into a buffer the frame keeps), with its baseline
  /// at `y`, placed by `anchor`: 0 starts it at `x`, 0.5 centres it there, 1 ends it there
  /// (numbers right-aligned in a column, an axis label). Returns the advance width in points.
  pub fn text_at(
    &mut self,
    x: f32,
    y: f32,
    s: impl Display,
    font: Font,
    color: Color,
    anchor: f32,
  ) -> f32 {
    let mut buf = std::mem::take(&mut self.text.scratch);
    buf.clear();
    _ = write!(buf, "{s}");
    let w = if anchor == 0.0 { 0.0 } else { self.text.measure(&buf, font) };
    let w = self.text(x - w * anchor, y, &buf, font, color);
    self.text.scratch = buf;
    w
  }

  /// Draws `s` with its baseline at `y`; returns the advance width in points. Glyphs snap to
  /// the pixel grid vertically and to a quarter pixel horizontally; colour glyphs (emoji) to
  /// whole pixels, drawn with `color`'s alpha only. Glyphs whose ink can't reach the frame and
  /// the clip are skipped before they take an atlas slot (a long line scrolled sideways).
  pub fn text(&mut self, x: f32, y: f32, s: &str, font: Font, color: Color) -> f32 {
    let scale = self.scale;
    let line = self.text.layout(s, font);
    let baseline = (y * scale).round();
    // The line's ink bounds, and a pixel more each way for the snapping below.
    let frame = Rect::new(0.0, 0.0, self.size.w, self.size.h);
    let view = self.scene.clip().map_or(frame, |c| c.intersect(frame));
    let (ink, pad) = (line.ink, 1.0 / scale);
    let empty = view.w <= 0.0 || view.h <= 0.0;
    if empty || y + ink.down + pad < view.y || y - ink.up - pad > view.y + view.h {
      return line.width;
    }

    for g in &line.glyphs {
      let pen = x + g.x;
      if pen + ink.right + pad < view.x || pen - ink.left - pad > view.x + view.w {
        continue;
      }

      let px = pen * scale;
      // Quarters right of the pixel the glyph starts in: left of the origin too.
      let subpixel = (((px - px.floor()) * 4.0) as u8).min(3);
      let Some(a) = self.text.glyph(g.face, g.id, subpixel, color) else { continue };
      let left = if a.color { px.round() } else { px.floor() };
      // A mark the shaper raised, by whole pixels like the baseline.
      let top = baseline - (g.y * scale).round() - a.top;
      let rect = Rect::new((left + a.left) / scale, top / scale, a.uv.w / scale, a.uv.h / scale);
      match a.color {
        true => self.scene.emoji(Sprite { rect, uv: a.uv, color: [1.0, 1.0, 1.0, color[3]] }),
        false => self.scene.glyph(Sprite { rect, uv: a.uv, color }),
      }
    }

    line.width
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn text_left_of_the_origin_keeps_its_subpixel_offset() {
    let (mut scene, mut text, mut images) =
      (Scene::default(), TextSystem::new(2.0), Images::default());
    let mut f = Frame {
      size: Size::new(100.0, 50.0),
      scale: 2.0,
      mouse: None,
      now: Duration::ZERO,
      scene: &mut scene,
      text: &mut text,
      images: &mut images,
      animate: false,
    };
    let mut at = |x: f32| {
      f.scene.clear();
      f.text(x, 20.0, "H", Font::ui(13.0), [1.0; 4]);
      f.scene.layers()[0].glyphs[0]
    };
    // -0.25 and 0.75 pixels: the same quarter, a pixel apart.
    let (a, b) = (at(-0.125), at(0.375));
    assert_eq!(a.uv, b.uv);
    assert_eq!(b.rect.x - a.rect.x, 0.5);
  }

  #[test]
  fn clipped_glyphs_take_no_atlas_slots() {
    let (mut scene, mut text, mut images) =
      (Scene::default(), TextSystem::new(1.0), Images::default());
    let mut f = Frame {
      size: Size::new(100.0, 50.0),
      scale: 1.0,
      mouse: None,
      now: Duration::ZERO,
      scene: &mut scene,
      text: &mut text,
      images: &mut images,
      animate: false,
    };
    let font = Font::ui(13.0);
    // A line scrolled far left: its hidden start takes no slots, its visible end does.
    let s = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 OK";
    let w = f.measure(s, font);
    let width = f.text(60.0 - w, 20.0, s, font, [1.0; 4]);
    assert_eq!(width, w, "the whole line's width");
    let drawn = f.scene.layers()[0].glyphs.len();
    assert!((1..=12).contains(&drawn), "{drawn} of {} glyphs", s.len());
    assert_eq!(f.text.uploads().count(), drawn);
    // Below the frame, and outside a clip: none.
    f.text(0.0, 200.0, s, font, [1.0; 4]);
    f.push_clip(Rect::new(0.0, 0.0, 10.0, 10.0));
    f.text(40.0, 40.0, "Z", font, [1.0; 4]);
    f.pop_clip();
    // Across a clip with no height: none either.
    f.push_clip(Rect::new(0.0, 20.0, 100.0, 0.0));
    f.text(0.0, 20.0, "Z", font, [1.0; 4]);
    f.pop_clip();
    assert_eq!(f.text.uploads().count(), 0);
  }

  /// Ink far from its pen or baseline still draws when only it shows: the bismillah ligature
  /// reaches some 3 em right of its origin, stacked accents several em above the baseline.
  #[test]
  fn ink_far_from_the_pen_is_drawn() {
    let (mut scene, mut text, mut images) =
      (Scene::default(), TextSystem::new(1.0), Images::default());
    let mut f = Frame {
      size: Size::new(100.0, 50.0),
      scale: 1.0,
      mouse: None,
      now: Duration::ZERO,
      scene: &mut scene,
      text: &mut text,
      images: &mut images,
      animate: false,
    };
    let font = Font::ui(13.0);
    let wide = f.text.layout("\u{FDFD}", font);
    if wide.ink.right > 2.5 * font.size {
      // Its pen 2.2 em left of the frame, its ink the rest of the way.
      f.text(-2.2 * font.size, 20.0, "\u{FDFD}", font, [1.0; 4]);
      assert_eq!(f.scene.counts().glyphs, 1, "ink to {} pt", wide.ink.right);
    } else {
      println!("no face here draws U+FDFD wider than 2.5 em: skipped");
    }

    let tall = format!("e{}", "\u{301}".repeat(24));
    let up = f.text.layout(&tall, font).ink.up;
    assert!(up > 4.0 * font.size, "the accents stack {up} pt high");
    // The baseline 3.5 em below the frame; the top accents inside it.
    f.scene.clear();
    f.text(0.0, 50.0 + 3.5 * font.size, &tall, font, [1.0; 4]);
    assert!(f.scene.counts().glyphs > 0);
  }

  #[test]
  fn text_at_anchors_to_x() {
    let (mut scene, mut text, mut images) =
      (Scene::default(), TextSystem::new(1.0), Images::default());
    let mut f = Frame {
      size: Size::new(200.0, 50.0),
      scale: 1.0,
      mouse: None,
      now: Duration::ZERO,
      scene: &mut scene,
      text: &mut text,
      images: &mut images,
      animate: false,
    };
    let font = Font::ui(13.0);
    let w = f.measure("12.50", font);
    for (anchor, left) in [(0.0, 100.0), (0.5, 100.0 - w / 2.0), (1.0, 100.0 - w)] {
      f.scene.clear();
      assert_eq!(f.text_at(100.0, 30.0, format_args!("{:.2}", 12.5), font, [1.0; 4], anchor), w);
      let g = &f.scene.layers()[0].glyphs;
      let (lo, hi) = g
        .iter()
        .fold((f32::MAX, 0.0f32), |(a, b), s| (a.min(s.rect.x), b.max(s.rect.x + s.rect.w)));
      // The glyphs' bitmaps (with a pixel of margin) start at the left edge and end at the
      // advance.
      assert!((lo - left).abs() < 2.0, "{anchor}: {lo} vs {left}");
      assert!((hi - (left + w)).abs() < 2.0, "{anchor}: {hi} vs {}", left + w);
    }
  }

  #[test]
  fn scene_reuses_layers() {
    let mut s = Scene::default();
    s.layer();
    s.quad(Quad::default());
    s.push_clip(Rect::new(0.0, 0.0, 10.0, 10.0));
    s.glyph(Sprite::default());
    assert_eq!(s.counts(), Counts { quads: 1, glyphs: 1, ..Counts::default() });
    let cap = s.layers[0].quads.capacity();

    s.clear();
    assert_eq!(s.layers().len(), 0);
    s.quad(Quad::default());
    assert_eq!(s.layers().len(), 1);
    assert_eq!(s.layers()[0].clip, None);
    assert_eq!(s.counts(), Counts { quads: 1, ..Counts::default() });
    assert_eq!(s.layers[0].quads.capacity(), cap);
  }

  #[test]
  fn clips_nest_and_restore() {
    let mut s = Scene::default();
    s.quad(Quad::default());
    s.push_clip(Rect::new(0.0, 0.0, 100.0, 100.0));
    s.quad(Quad::default());
    s.push_clip(Rect::new(50.0, 60.0, 100.0, 100.0));
    s.quad(Quad::default());
    s.pop_clip();
    s.quad(Quad::default());
    s.pop_clip();
    // An empty layer is reused rather than stacked.
    s.push_clip(Rect::new(1.0, 1.0, 1.0, 1.0));
    s.pop_clip();
    s.quad(Quad::default());
    let clips: Vec<_> = s.layers().iter().map(|l| l.clip).collect();
    let (outer, inner) = (Rect::new(0.0, 0.0, 100.0, 100.0), Rect::new(50.0, 60.0, 50.0, 40.0));
    assert_eq!(clips, [None, Some(outer), Some(inner), Some(outer), None]);
  }

  #[test]
  fn rect_contains_and_intersects() {
    let r = Rect::new(10.0, 10.0, 5.0, 5.0);
    assert!(r.contains(Point::new(10.0, 14.9)));
    assert!(!r.contains(Point::new(15.0, 12.0)));
    assert_eq!(r.intersect(Rect::new(12.0, 0.0, 10.0, 12.0)), Rect::new(12.0, 10.0, 3.0, 2.0));
    assert_eq!(r.intersect(Rect::new(20.0, 0.0, 1.0, 1.0)).w, 0.0);
  }

  #[test]
  fn images_premultiply_and_reuse_ids() {
    let mut im = Images::default();
    let a = im.add(1, 1, &[255, 128, 0, 128]);
    assert_eq!(im.added[0].data, [128, 64, 0, 128]);
    let b = im.add(2, 1, &[0; 8]);
    assert_eq!(im.size(b), Some((2, 1)));
    im.remove(a);
    assert_eq!((im.size(a), im.added.len(), im.removed.len()), (None, 1, 1));
    assert_eq!(im.add(1, 1, &[0; 4]), a);
  }
}
