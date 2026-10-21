// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The frame's draw list and the paint API that fills it. Coordinates are points, y down; the
//! renderer scales them to pixels. Buffers are kept across frames, so a steady frame allocates
//! nothing.

use std::time::Duration;

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

/// A rect on screen in points and its source rect in a texture in pixels. Images use `color` as
/// a tint.
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

/// What a layer draws, in this order: quads, images, lines.
#[derive(Default)]
pub struct Layer {
  pub clip: Option<Rect>,
  pub quads: Vec<Quad>,
  pub images: Vec<Sprite>,
  /// The image of each sprite in `images`.
  pub image_ids: Vec<ImageId>,
  pub lines: Vec<Segment>,
}

impl Layer {
  fn clear(&mut self) {
    self.quads.clear();
    self.images.clear();
    self.image_ids.clear();
    self.lines.clear();
  }

  fn is_empty(&self) -> bool {
    self.quads.is_empty() && self.images.is_empty() && self.lines.is_empty()
  }
}

/// Instance counts of a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
  pub quads: usize,
  pub images: usize,
  pub lines: usize,
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

  pub fn counts(&self) -> Counts {
    let sum = |f: fn(&Layer) -> usize| self.layers().iter().map(f).sum();
    Counts {
      quads: sum(|l| l.quads.len()),
      images: sum(|l| l.images.len()),
      lines: sum(|l| l.lines.len()),
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

/// What `App::paint` draws with: the scene, the images and the frame's inputs.
pub struct Frame<'a> {
  pub size: Size,
  /// Pixels per point.
  pub scale: f32,
  /// Time from the runtime's clock: real in a window, virtual in the harness.
  pub now: Duration,
  pub scene: &'a mut Scene,
  pub images: &'a mut Images,
  pub(crate) animate: bool,
}

impl Frame<'_> {
  /// Asks for another frame after this one (a running animation).
  pub fn animate(&mut self) {
    self.animate = true;
  }

  /// Starts a new layer over everything drawn so far. Within a layer each kind draws in a fixed
  /// order, not the order of the calls: quads, images, lines ([Layer]); so a quad drawn after a
  /// line (a label over a plot's lines) needs a layer of its own.
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
}

#[cfg(test)]
mod tests {
  use super::*;

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
