// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The CPU renderer: the scene rasterized into a BGRA8 premultiplied frame ([raster]), only
//! where each primitive covers. A frame is
//! prepared once (each primitive's pixel bounds under its layer's clip, and a hash per 64x64
//! tile of everything drawn over it), then drawn in bands of rows across threads; with the
//! tile hashes of what a target already shows, only the tiles that changed are drawn again.
//! Small damage is drawn on the calling thread; a big frame with the threads of [pool].

pub mod pool;
pub mod raster;

use std::sync::Mutex;

use self::pool::Pool;
use self::raster::{IRect, Target, Tex};
use crate::scene::{Images, Rect, Scene, Size};
use crate::text::{ATLAS_SIZE, COLOR_ATLAS_SIZE, TextSystem, Upload};

/// Side of a damage tile in pixels.
pub const TILE: i32 = 64;
/// Rows per band, the unit of work of a thread.
const BAND: i32 = 32;
/// Width of the pieces a band draws one after another.
const CHUNK: i32 = 256;
/// The clear colour, opaque black.
const CLEAR: u32 = 0xff00_0000;
/// Damage up to this many pixels (1/16 of a 2560x1600 frame, 0.3-0.5 ms on one core) is drawn
/// on the calling thread alone: the pool's threads would cut a few tenths of a ms for 15-25% more
/// CPU time.
pub const SMALL: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
  Quad,
  Image,
  Line,
  Glyph,
  Emoji,
}

/// One primitive to draw: its layer, its index there, the pixels it may touch (its geometry
/// within the layer's clip) and those it covers with an opaque colour.
#[derive(Clone, Copy, Debug)]
struct Item {
  kind: Kind,
  layer: u32,
  idx: u32,
  rect: IRect,
  opaque: IRect,
}

/// BGRA pixels of a frame (premultiplied), top row first: a harness shot's ([crate::harness]).
pub struct Pixels {
  pub w: usize,
  pub h: usize,
  pub data: Vec<u8>,
}

struct Image {
  w: usize,
  h: usize,
  data: Vec<u8>,
}

pub struct Renderer {
  /// The coverage and colour atlases, allocated on their first glyph.
  atlas: Vec<u8>,
  color_atlas: Vec<u8>,
  images: Vec<Option<Image>>,
  /// Threads to draw a big frame with (the calling thread and the pool's); 1 draws on the
  /// calling thread.
  pub threads: usize,
  /// Clears to transparent instead of opaque black: an overlay over the desktop
  /// ([crate::Config::overlay]).
  pub transparent: bool,
  /// The atlas epoch each atlas holds: coverage, colour.
  epochs: [u32; 2],
  /// Bumped when atlas or image pixels change under the same coordinates (an atlas reset, an
  /// image id reused): every tile hash differs.
  generation: u64,
  // The prepared frame.
  size: (usize, usize),
  scale: f32,
  items: Vec<Item>,
  /// Item indices per band, in drawing order.
  bins: Vec<Vec<u32>>,
  tiles: Vec<u64>,
  tiles_w: usize,
}

impl Default for Renderer {
  fn default() -> Self {
    Self::new()
  }
}

impl Renderer {
  /// A renderer drawing big frames on the performance cores.
  pub fn new() -> Self {
    let threads = pool::threads();
    Self {
      atlas: Vec::new(),
      color_atlas: Vec::new(),
      images: Vec::new(),
      threads,
      transparent: false,
      epochs: [0; 2],
      generation: 0,
      size: (0, 0),
      scale: 1.0,
      items: Vec::new(),
      bins: Vec::new(),
      tiles: Vec::new(),
      tiles_w: 0,
    }
  }

  /// Copies new glyph bitmaps into the atlases and adds or drops images, draining both queues.
  pub fn sync(&mut self, text: &mut TextSystem, images: &mut Images) {
    self.sync_pending(text.pending(), images);
    text.uploads().for_each(drop);
    images.added.clear();
    images.removed.clear();
  }

  /// [Self::sync] without draining.
  fn sync_pending(&mut self, uploads: &[Upload], images: &Images) {
    for u in uploads {
      let (atlas, size, bpp) = match u.color {
        true => (&mut self.color_atlas, COLOR_ATLAS_SIZE, 4),
        false => (&mut self.atlas, ATLAS_SIZE, 1),
      };

      if atlas.is_empty() {
        // Zeroed pages are mapped lazily: untouched rows cost nothing.
        *atlas = vec![0; size * size * bpp];
      }

      let n = u.w * bpp;
      for row in 0..u.h {
        let at = ((u.y + row) * size + u.x) * bpp;
        atlas[at..at + n].copy_from_slice(&u.data[row * n..(row + 1) * n]);
      }
      // Slots are reused only after a reset: frames drawn before show other glyphs there.
      if u.epoch != self.epochs[u.color as usize] {
        self.epochs[u.color as usize] = u.epoch;
        self.generation += 1;
      }
    }

    for id in &images.removed {
      if let Some(t) = self.images.get_mut(id.0 as usize) {
        *t = None;
      }

      self.generation += 1;
    }

    for u in &images.added {
      let i = u.id.0 as usize;
      match self.images.len() <= i {
        true => self.images.resize_with(i + 1, || None),
        false => self.generation += 1,
      }

      self.images[i] = Some(Image { w: u.w, h: u.h, data: u.data.clone() });
    }
  }

  /// The frame size in pixels for a view of `size` points.
  pub fn pixel_size(size: Size, scale: f32) -> (usize, usize) {
    (((size.w * scale) as usize).max(1), ((size.h * scale) as usize).max(1))
  }

  /// Lists what the scene draws, where, and hashes every tile.
  pub fn prepare(&mut self, scene: &Scene, size: Size, scale: f32) {
    let (w, h) = Self::pixel_size(size, scale);
    (self.size, self.scale) = ((w, h), scale);
    let frame = IRect::new(0, 0, w as i32, h as i32);
    let full = Rect::new(0.0, 0.0, size.w, size.h);
    self.items.clear();
    for (li, layer) in scene.layers().iter().enumerate() {
      let clip = scissor(layer.clip.unwrap_or(full), scale, frame);
      if clip.is_empty() {
        continue;
      }

      let mut add = |kind, idx: usize, r: IRect, opaque: IRect| {
        let rect = r.intersect(clip);
        if !rect.is_empty() {
          let opaque = opaque.intersect(rect);
          self.items.push(Item { kind, layer: li as u32, idx: idx as u32, rect, opaque });
        }
      };
      let none = IRect::default();
      for (i, q) in layer.quads.iter().enumerate() {
        let solid = q.color[3] >= 1.0 && (q.gradient == 0.0 || q.border_color[3] >= 1.0);
        let opaque = if solid { raster::quad_solid(q, scale) } else { none };
        add(Kind::Quad, i, raster::quad_bounds(q, scale), opaque);
      }

      for (i, s) in layer.images.iter().enumerate() {
        add(Kind::Image, i, raster::sprite_bounds(s, scale), none);
      }

      for (i, s) in layer.lines.iter().enumerate() {
        add(Kind::Line, i, raster::line_bounds(s, scale), none);
      }

      for (i, s) in layer.glyphs.iter().enumerate() {
        add(Kind::Glyph, i, raster::sprite_bounds(s, scale), none);
      }

      for (i, s) in layer.emoji.iter().enumerate() {
        add(Kind::Emoji, i, raster::sprite_bounds(s, scale), none);
      }
    }

    let bands = h.div_ceil(BAND as usize);
    self.bins.resize_with(bands, Vec::new);
    self.bins.truncate(bands);
    self.bins.iter_mut().for_each(Vec::clear);
    let (tw, th) = (w.div_ceil(TILE as usize), h.div_ceil(TILE as usize));
    self.tiles_w = tw;
    self.tiles.clear();
    // Items hash their geometry in points and pixels; the scale maps one to the other.
    let clear = self.transparent as u64;
    let seed = hash(&[w as u64, h as u64, scale.to_bits() as u64, self.generation, clear]);
    self.tiles.resize(tw * th, seed);
    for (n, it) in self.items.iter().enumerate() {
      let r = it.rect;
      for b in &mut self.bins[(r.y0 / BAND) as usize..=((r.y1 - 1) / BAND) as usize] {
        b.push(n as u32);
      }

      let k = item_hash(scene, it);
      for ty in (r.y0 / TILE) as usize..=((r.y1 - 1) / TILE) as usize {
        for t in
          &mut self.tiles[ty * tw + (r.x0 / TILE) as usize..=ty * tw + ((r.x1 - 1) / TILE) as usize]
        {
          *t = Hash(*t).add(k).0;
        }
      }
    }
  }

  /// The prepared frame's size in pixels.
  pub fn size(&self) -> (usize, usize) {
    self.size
  }

  /// The prepared frame's tile hashes, to keep with a target that shows it.
  pub fn tiles(&self) -> &[u64] {
    &self.tiles
  }

  /// Rects of tiles whose hash differs from `shown` (the hashes of what the target holds), merged
  /// into disjoint rects; the whole frame when `shown` is of another size.
  pub fn damage(&self, shown: &[u64]) -> Vec<IRect> {
    let (w, h) = (self.size.0 as i32, self.size.1 as i32);
    if shown.len() != self.tiles.len() {
      return vec![IRect::new(0, 0, w, h)];
    }

    let tw = self.tiles_w;
    let mut out: Vec<IRect> = Vec::new();
    // Runs of dirty tiles per tile row; a run under one of the same columns extends it.
    let mut open: Vec<usize> = Vec::new();
    for ty in 0..self.tiles.len() / tw {
      let (y0, y1) = (ty as i32 * TILE, ((ty as i32 + 1) * TILE).min(h));
      let mut next = Vec::new();
      let mut tx = 0;
      while tx < tw {
        let dirty = |x: usize| self.tiles[ty * tw + x] != shown[ty * tw + x];
        if !dirty(tx) {
          tx += 1;
          continue;
        }

        let start = tx;
        while tx < tw && dirty(tx) {
          tx += 1;
        }

        let (x0, x1) = (start as i32 * TILE, (tx as i32 * TILE).min(w));
        match open.iter().find(|&&i| out[i].x0 == x0 && out[i].x1 == x1 && out[i].y1 == y0) {
          Some(&i) => {
            out[i].y1 = y1;
            next.push(i);
          }
          None => {
            next.push(out.len());
            out.push(IRect::new(x0, y0, x1, y1));
          }
        }
      }

      open = next;
    }

    out
  }

  /// Clears and draws `rects` (disjoint) of the prepared frame into `px`, rows of `stride`
  /// pixels holding the whole frame; the last row needs no padding.
  pub fn draw(&mut self, scene: &Scene, px: &mut [u32], stride: usize, rects: &[IRect]) {
    let (w, h) = self.size;
    assert!(stride >= w && px.len() >= stride * (h - 1) + w, "frame buffer too small");
    let atlas = Tex { data: &self.atlas, w: ATLAS_SIZE, h: ATLAS_SIZE };
    let color = Tex { data: &self.color_atlas, w: COLOR_ATLAS_SIZE, h: COLOR_ATLAS_SIZE };
    let (items, images, scale) = (&self.items, &self.images, self.scale);
    let clear = if self.transparent { 0 } else { CLEAR };
    let job = Job { scene, items, images, atlas, color, scale, clear };
    let rows = BAND as usize * stride;
    let px = &mut px[..stride * (h - 1) + w];
    let bands: Vec<(usize, &mut [u32])> = px.chunks_mut(rows).enumerate().collect();
    let todo = |b: usize| {
      let band = IRect::new(0, b as i32 * BAND, w as i32, ((b as i32 + 1) * BAND).min(h as i32));
      rects.iter().any(|r| r.overlaps(band))
    };
    let bands: Vec<_> = bands.into_iter().filter(|(b, _)| todo(*b)).collect();
    let run = |(b, px): (usize, &mut [u32])| {
      let y0 = b as i32 * BAND;
      let band = IRect::new(0, y0, w as i32, (y0 + BAND).min(h as i32));
      let mut t = Target { px, stride, y0 };
      for r in rects.iter().map(|r| r.intersect(band)).filter(|r| !r.is_empty()) {
        // In chunks, so that a panel's background hides the window's under it.
        for x in (r.x0..r.x1).step_by(CHUNK as usize) {
          job.draw(&mut t, &self.bins[b], IRect { x0: x, x1: (x + CHUNK).min(r.x1), ..r });
        }
      }
    };
    let area: usize = rects.iter().map(|r| r.area()).sum();
    let threads = if area <= SMALL { 1 } else { self.threads.min(bands.len()) };
    if threads <= 1 {
      return bands.into_iter().for_each(run);
    }

    let queue = Mutex::new(bands.into_iter());
    Pool::get().run(threads - 1, &|| {
      loop {
        let Some(band) = queue.lock().unwrap().next() else { break };
        run(band);
      }
    });
  }

  /// Draws the whole prepared frame.
  #[cfg(test)]
  pub fn draw_full(&mut self, scene: &Scene, px: &mut [u32], stride: usize) {
    let r = IRect::new(0, 0, self.size.0 as i32, self.size.1 as i32);
    self.draw(scene, px, stride, &[r]);
  }

  /// Renders the whole frame into new pixels.
  pub fn snapshot(&mut self, scene: &Scene, size: Size, scale: f32) -> Pixels {
    let mut c = Canvas::default();
    c.draw(self, scene, size, scale, false);
    c.pixels()
  }
}

/// A frame buffer the renderer draws into, with the tile hashes of what it holds.
#[derive(Default)]
pub struct Canvas {
  pub w: usize,
  pub h: usize,
  pub px: Vec<u32>,
  tiles: Vec<u64>,
}

impl Canvas {
  /// Draws `scene`: with `damage` only the tiles that changed since the last draw, else all.
  /// Returns the pixels drawn.
  pub fn draw(
    &mut self,
    r: &mut Renderer,
    scene: &Scene,
    size: Size,
    scale: f32,
    damage: bool,
  ) -> usize {
    r.prepare(scene, size, scale);
    let (w, h) = r.size;
    if (w, h) != (self.w, self.h) {
      (self.w, self.h, self.px) = (w, h, vec![0; w * h]);
      self.tiles.clear();
    }

    let rects = match damage {
      true => r.damage(&self.tiles),
      false => vec![IRect::new(0, 0, w as i32, h as i32)],
    };

    r.draw(scene, &mut self.px, w, &rects);
    self.tiles.clear();
    self.tiles.extend_from_slice(r.tiles());
    rects.iter().map(|r| r.area()).sum()
  }

  pub fn pixels(&self) -> Pixels {
    let data = self.px.iter().flat_map(|p| p.to_le_bytes()).collect();
    Pixels { w: self.w, h: self.h, data }
  }
}

/// What a band draws from.
struct Job<'a> {
  scene: &'a Scene,
  items: &'a [Item],
  images: &'a [Option<Image>],
  atlas: Tex<'a>,
  color: Tex<'a>,
  scale: f32,
  clear: u32,
}

impl Job<'_> {
  /// Clears `clip` and draws the items of `bin` over it, from the last one that covers all of
  /// it with an opaque colour (a window's or panel's background) on.
  fn draw(&self, t: &mut Target, bin: &[u32], clip: IRect) {
    let covers = |n: &u32| self.items[*n as usize].opaque.intersect(clip) == clip;
    let start = bin.iter().rposition(covers).unwrap_or_else(|| {
      t.fill(clip, self.clear);
      0
    });
    let layers = self.scene.layers();
    for &n in &bin[start..] {
      let it = &self.items[n as usize];
      let r = it.rect.intersect(clip);
      if r.is_empty() {
        continue;
      }

      let (l, i) = (&layers[it.layer as usize], it.idx as usize);
      match it.kind {
        Kind::Quad => raster::quad(t, &l.quads[i], r, self.scale),
        Kind::Line => raster::line(t, &l.lines[i], r, self.scale),
        Kind::Glyph if !self.atlas.data.is_empty() => {
          raster::glyph(t, &l.glyphs[i], self.atlas, r, self.scale)
        }
        Kind::Emoji if !self.color.data.is_empty() => {
          raster::image(t, &l.emoji[i], self.color, r, self.scale)
        }
        Kind::Image => {
          let Some(Some(img)) = self.images.get(l.image_ids[i].0 as usize) else { continue };
          let tex = Tex { data: &img.data, w: img.w, h: img.h };
          raster::image(t, &l.images[i], tex, r, self.scale)
        }
        Kind::Glyph | Kind::Emoji => {}
      }
    }
  }
}

/// The clip in whole pixels, each edge rounded to the nearest.
fn scissor(r: Rect, scale: f32, frame: IRect) -> IRect {
  let (w, h) = (frame.x1 as f32, frame.y1 as f32);
  let x0 = (r.x * scale).round().clamp(0.0, w);
  let y0 = (r.y * scale).round().clamp(0.0, h);
  let x1 = ((r.x + r.w) * scale).round().clamp(x0, w);
  let y1 = ((r.y + r.h) * scale).round().clamp(y0, h);
  IRect::new(x0 as i32, y0 as i32, x1 as i32, y1 as i32)
}

// MARK: Hashing

const SEED: u64 = 0x517c_c1b7_2722_0a95;

/// FxHash-style mixing of 64-bit words, order-dependent.
#[derive(Clone, Copy)]
struct Hash(u64);

impl Hash {
  fn add(self, w: u64) -> Self {
    Self((self.0.rotate_left(5) ^ w).wrapping_mul(SEED))
  }

  fn floats(self, v: &[f32]) -> Self {
    v.iter().fold(self, |h, f| h.add(f.to_bits() as u64))
  }
}

fn hash(words: &[u64]) -> u64 {
  words.iter().fold(Hash(0), |h, &w| h.add(w)).0
}

/// A hash of what an item draws: its kind, its fields and its clipped rect.
fn item_hash(scene: &Scene, it: &Item) -> u64 {
  let l = &scene.layers()[it.layer as usize];
  let i = it.idx as usize;
  let r = it.rect;
  let h = Hash(it.kind as u64).add(r.x0 as u64).add(r.y0 as u64).add(r.x1 as u64).add(r.y1 as u64);
  let rect = |r: Rect| [r.x, r.y, r.w, r.h];
  let h = match it.kind {
    Kind::Quad => {
      let q = &l.quads[i];
      let h = h.floats(&rect(q.rect)).floats(&q.color).floats(&q.border_color);
      h.floats(&q.radii).floats(&q.borders).floats(&[q.blur, q.gradient])
    }
    Kind::Line => {
      let s = &l.lines[i];
      h.floats(&[s.a.x, s.a.y, s.b.x, s.b.y, s.width, s.round]).floats(&s.color)
    }
    Kind::Image => {
      let s = &l.images[i];
      h.add(l.image_ids[i].0 as u64).floats(&rect(s.rect)).floats(&rect(s.uv)).floats(&s.color)
    }
    Kind::Glyph | Kind::Emoji => {
      let s = if it.kind == Kind::Glyph { &l.glyphs[i] } else { &l.emoji[i] };
      h.floats(&rect(s.rect)).floats(&rect(s.uv)).floats(&s.color)
    }
  };

  h.0
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scene::{Point, Quad, Segment};

  /// Panels, a moving rounded card with a border and a shadow, a clip and a line.
  fn scene(t: f32) -> Scene {
    let mut s = Scene::default();
    s.quad(Quad::new(Rect::new(0.0, 0.0, 300.0, 200.0), [0.1, 0.1, 0.12, 1.0]));
    s.quad(Quad::new(Rect::new(0.0, 0.0, 80.0, 200.0), [0.15, 0.15, 0.2, 1.0]));
    let card = Rect::new(100.0 + t, 40.0, 90.0, 50.0);
    s.quad(Quad::new(card, [0.0, 0.0, 0.0, 0.5]).radius(8.0).blur(6.0));
    s.quad(Quad::new(card, [0.3, 0.5, 0.9, 1.0]).radius(8.0).border(1.0, [1.0, 1.0, 1.0, 0.6]));
    s.push_clip(Rect::new(20.0, 120.0, 150.0 + t, 50.0));
    let a = Point::new(10.0, 160.0);
    s.segment(Segment {
      a,
      b: Point::new(250.0, 125.0 + t),
      color: [0.9, 0.7, 0.2, 1.0],
      width: 1.5,
      round: 1.0,
      _pad: [0.0; 2],
    });
    s.quad(Quad::new(Rect::new(30.0, 130.0, 40.0, 20.0), [1.0, 0.2, 0.2, 0.7]));
    s.pop_clip();
    s
  }

  /// An overlay's frame: nothing where the scene draws nothing, premultiplied where it draws
  /// half through.
  #[test]
  fn a_transparent_frame_clears_to_nothing() {
    let mut s = Scene::default();
    s.quad(Quad::new(Rect::new(0.0, 0.0, 10.0, 10.0), [1.0, 0.0, 0.0, 0.5]));
    let mut r = Renderer::new();
    let size = Size::new(20.0, 10.0);
    let opaque = r.snapshot(&s, size, 1.0);
    let mut c = Canvas::default();
    c.draw(&mut r, &s, size, 1.0, false);
    r.transparent = true;
    let px = r.snapshot(&s, size, 1.0);
    // A canvas drawn opaque is drawn again, all of it.
    assert_eq!(c.draw(&mut r, &s, size, 1.0, true), 20 * 10);
    assert_eq!(c.pixels().data, px.data);
    let at = |p: &Pixels, x: usize| p.data[x * 4..x * 4 + 4].to_vec();
    assert_eq!(at(&opaque, 15), [0, 0, 0, 255]);
    assert_eq!(at(&px, 15), [0, 0, 0, 0]);
    let [b, g, r, a] = at(&px, 5)[..] else { unreachable!() };
    assert_eq!((b, g, a), (0, 0, 128));
    assert!(r.abs_diff(a) <= 1, "premultiplied: {r} of {a}");
  }

  #[test]
  fn damage_draws_what_a_full_frame_draws() {
    let (size, scale) = (Size::new(300.0, 200.0), 2.0);
    let mut r = Renderer::new();
    let mut c = Canvas::default();
    c.draw(&mut r, &scene(0.0), size, scale, false);
    // Unchanged, the card moving by a fraction of a pixel, far, by a point, back.
    for (t, most) in [(0.0, 0), (3.25, 70), (40.0, 70), (41.0, 70), (0.0, 70)] {
      let s = scene(t);
      let drawn = c.draw(&mut r, &s, size, scale, true);
      let full = r.snapshot(&s, size, scale);
      assert_eq!(c.pixels().data, full.data, "t = {t}");
      assert!(drawn <= c.w * c.h * most / 100, "t = {t}: {drawn} pixels drawn");
    }
  }

  #[test]
  fn threads_draw_the_same() {
    // Big enough for the pool.
    let (size, scale) = (Size::new(600.0, 400.0), 1.5);
    let (w, h) = Renderer::pixel_size(size, scale);
    assert!(w * h > SMALL);
    let s = scene(7.0);
    let mut r = Renderer::new();
    r.threads = 1;
    let one = r.snapshot(&s, size, scale);
    r.threads = 4;
    assert_eq!(r.snapshot(&s, size, scale).data, one.data);
  }

  #[test]
  fn damage_merges_tiles_into_disjoint_rects() {
    let mut r = Renderer::new();
    r.prepare(&Scene::default(), Size::new(300.0, 200.0), 1.0);
    // 5x4 tiles; the last column and row are cut to the frame.
    let mut shown = r.tiles().to_vec();
    for i in [1, 2, 6, 7, 19] {
      shown[i] ^= 1;
    }

    let got = r.damage(&shown);
    let want = [IRect::new(64, 0, 192, 128), IRect::new(256, 192, 300, 200)];
    assert_eq!(got, want);
    assert_eq!(r.damage(&[]), [IRect::new(0, 0, 300, 200)]);
  }

  #[test]
  fn damage_redraws_on_a_new_scale() {
    // The same pixel size, items and bounds, but the gradient spans twice the pixels.
    let mut s = Scene::default();
    let q = Quad::new(Rect::new(0.0, 0.0, 128.0, 128.0), [1.0, 0.0, 0.0, 1.0]);
    s.quad(q.gradient([0.0, 0.0, 1.0, 1.0], true));
    let mut r = Renderer::new();
    let mut c = Canvas::default();
    c.draw(&mut r, &s, Size::new(64.0, 64.0), 1.0, false);
    let (size, scale) = (Size::new(32.0, 32.0), 2.0);
    assert!(c.draw(&mut r, &s, size, scale, true) > 0);
    assert_eq!(c.pixels().data, r.snapshot(&s, size, scale).data);
  }

  #[test]
  fn draw_needs_no_padding_after_the_last_row() {
    let mut s = Scene::default();
    s.quad(Quad::new(Rect::new(0.0, 0.0, 2.0, 2.0), [1.0, 1.0, 1.0, 1.0]));
    let mut r = Renderer::new();
    r.prepare(&s, Size::new(2.0, 2.0), 1.0);
    let mut px = [0u32; 5];
    r.draw_full(&s, &mut px, 3);
    assert_eq!(px, [!0, !0, 0, !0, !0]);
  }
}
