// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Text: shaping through the platform's text backend ([crate::platform] `text`: CoreText on
//! macOS) with a shaped-line cache, glyphs rasterized by it into an R8 atlas and colour glyphs
//! (emoji) into an RGBA one ([atlas]). Fonts are shaped at their point size; only glyph bitmaps
//! are made at the pixel size. Everything here but the backend is portable.

mod atlas;
pub mod edit;

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::rc::Rc;

pub use atlas::{ATLAS_SIZE, AtlasGlyph, COLOR_ATLAS_SIZE, Upload};

use self::atlas::Atlas;
use crate::platform::text::{self as sys, Face};

/// Shaped lines kept before eviction starts dropping those not drawn in the last frame.
const LINE_CAP: usize = 4096;
/// Faces kept (fonts at every size asked, and their fallbacks) before the text system starts
/// over: an app animating a font size makes a face per size.
const FACE_CAP: usize = 256;
/// Font smoothing by default: it matched AppKit's text best in a side-by-side comparison.
pub const SMOOTH: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
  Regular,
  Medium,
  Semibold,
  Bold,
}

/// The system UI font, or its monospaced one when `mono` (SF Pro and SF Mono on macOS); `size`
/// in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Font {
  pub size: f32,
  pub weight: Weight,
  pub mono: bool,
}

impl Font {
  pub const fn ui(size: f32) -> Self {
    Self { size, weight: Weight::Regular, mono: false }
  }

  pub const fn mono(size: f32) -> Self {
    Self { size, weight: Weight::Regular, mono: true }
  }

  pub const fn weight(self, weight: Weight) -> Self {
    Self { weight, ..self }
  }
}

/// Vertical metrics of a font in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
  pub ascent: f32,
  pub descent: f32,
  pub leading: f32,
  /// Heights of the capitals and the lowercase letters above the baseline: where a mark set
  /// beside the text centres.
  pub cap_height: f32,
  pub x_height: f32,
}

impl Metrics {
  /// The distance between baselines, rounded up to whole points.
  pub fn line_height(&self) -> f32 {
    (self.ascent + self.descent + self.leading).ceil()
  }
}

/// A glyph's bitmap in pixels, from the backend: `left` right of the pen position and `bottom`
/// above the baseline (y up), `w` x `h` pixels with rows top-down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphBox {
  pub left: i32,
  pub bottom: i32,
  pub w: usize,
  pub h: usize,
}

/// How coverage glyphs are rasterized.
#[derive(Clone, Copy, Debug)]
pub struct Raster {
  pub scale: f32,
  /// Font smoothing (CoreGraphics' stem darkening on current macOS).
  pub smooth: bool,
  /// With smoothing: rasterize for dark text on a light background, which CoreGraphics
  /// darkens less than light text on dark.
  pub dark: bool,
}

/// A shaped glyph: `face` indexes the text system's faces (the run's font after fallback), `x`
/// is the pen position in points from the line start.
#[derive(Clone, Copy, Debug)]
pub struct Glyph {
  pub face: u16,
  pub id: u16,
  pub x: f32,
}

#[derive(Debug)]
pub struct Line {
  pub glyphs: Vec<Glyph>,
  /// Per glyph, the byte offset in the line's text where its cluster starts (apart from the
  /// glyphs, which drawing walks).
  pub clusters: Vec<u32>,
  /// Advance width in points.
  pub width: f32,
}

struct Entry {
  face: u16,
  text: Box<str>,
  line: Rc<Line>,
  /// The last frame that used it.
  frame: u64,
}

pub struct TextSystem {
  scale: f32,
  /// Requested fonts and the face each resolved to: a few, searched in order (a hash lookup
  /// cost more per text element); bounded by [FACE_CAP].
  fonts: Vec<(Font, u16)>,
  faces: Vec<Face>,
  /// Per face.
  metrics: Vec<Metrics>,
  /// Per face: its glyphs are colour bitmaps (Apple Color Emoji).
  color_faces: Vec<bool>,
  /// Keyed by a hash of (face, text); the entry keeps the text to rule out collisions, so a hit
  /// allocates nothing.
  lines: HashMap<u64, Entry, BuildHasherDefault<FxHasher>>,
  line_cap: usize,
  atlas: Atlas,
  color_atlas: Atlas,
  uploads: Vec<Upload>,
  smooth: bool,
  frame: u64,
  /// The last frame reset a full atlas and asked to be drawn again: if the redrawn frame fills
  /// it too (its own glyphs don't fit), it is not drawn a third time.
  refilled: bool,
  /// Text formatted for drawing ([crate::Frame::text_at]), kept so a steady frame allocates
  /// nothing.
  pub(crate) scratch: String,
}

impl TextSystem {
  pub fn new(scale: f32) -> Self {
    Self {
      scale,
      fonts: Vec::new(),
      faces: Vec::new(),
      metrics: Vec::new(),
      color_faces: Vec::new(),
      lines: HashMap::default(),
      line_cap: LINE_CAP,
      atlas: Atlas::new(ATLAS_SIZE, false),
      color_atlas: Atlas::new(COLOR_ATLAS_SIZE, true),
      uploads: Vec::new(),
      smooth: SMOOTH,
      frame: 0,
      refilled: false,
      scratch: String::new(),
    }
  }

  pub fn scale(&self) -> f32 {
    self.scale
  }

  /// Drops the glyph bitmaps made at the old scale; the atlas refills from the start. Shaped
  /// lines are in points and stay.
  pub fn set_scale(&mut self, scale: f32) {
    if scale == self.scale {
      return;
    }

    self.scale = scale;
    self.atlas.reset();
    self.color_atlas.reset();
  }

  pub fn smoothing(&self) -> bool {
    self.smooth
  }

  /// Turns font smoothing of coverage glyphs on or off (default [SMOOTH]); the
  /// atlas refills.
  pub fn set_smoothing(&mut self, smooth: bool) {
    if smooth == self.smooth {
      return;
    }

    self.smooth = smooth;
    self.atlas.reset();
  }

  /// Atlas pixels rasterized since the last call, for the renderer to copy into its textures.
  pub fn uploads(&mut self) -> std::vec::Drain<'_, Upload> {
    self.uploads.drain(..)
  }

  /// The uploads [Self::uploads] would drain, left in place: a second renderer reads them
  /// first.
  pub fn pending(&self) -> &[Upload] {
    &self.uploads
  }

  /// Ends a frame: evicts lines it didn't use once the cache is over its cap, starts over when
  /// there are too many faces, and resets a full atlas. Returns true when glyphs were dropped
  /// this frame, so it must be drawn again; not twice in a row, so a frame whose glyphs don't fit
  /// the atlas draws with some missing instead of looping.
  pub fn end_frame(&mut self) -> bool {
    if self.lines.len() > self.line_cap {
      let frame = self.frame;
      self.lines.retain(|_, e| e.frame == frame);
    }

    self.frame += 1;
    if self.faces.len() > FACE_CAP {
      // Lines and atlas slots name faces by index: they go too.
      self.fonts.clear();
      self.faces.clear();
      self.metrics.clear();
      self.color_faces.clear();
      self.lines.clear();
      self.atlas.reset();
      self.color_atlas.reset();
      self.refilled = true;
      return true;
    }

    let full = self.atlas.full || self.color_atlas.full;
    let dropped = full && !self.refilled;
    if dropped {
      for atlas in [&mut self.atlas, &mut self.color_atlas] {
        if atlas.full {
          atlas.reset();
        }
      }
    }

    self.refilled = dropped;
    dropped
  }

  pub fn measure(&mut self, s: &str, font: Font) -> f32 {
    self.layout(s, font).width
  }

  /// Ascent, descent, leading, cap and x-height of `font` (its primary face, before fallback).
  pub fn metrics(&mut self, font: Font) -> Metrics {
    let face = self.face(font);
    self.metrics[face as usize]
  }

  /// Shapes `s` (kerning, ligatures, font fallback), cached per (font, text).
  pub fn layout(&mut self, s: &str, font: Font) -> Rc<Line> {
    let face = self.face(font);
    let mut h = FxHasher::default();
    (face, s).hash(&mut h);
    let key = h.finish();
    if let Some(e) = self.lines.get_mut(&key)
      && e.face == face
      && *e.text == *s
    {
      e.frame = self.frame;
      return e.line.clone();
    }
    let line = Rc::new(self.shape(s, face));
    let entry = Entry { face, text: s.into(), line: line.clone(), frame: self.frame };
    self.lines.insert(key, entry);
    line
  }

  /// The atlas slot of a glyph at one of 4 horizontal subpixel offsets (colour glyphs have
  /// one), rasterized on first use for `dark` text (on a light background) or light text;
  /// `None` for glyphs with no pixels (spaces) or when the atlas is full.
  pub fn glyph(&mut self, face: u16, id: u16, subpixel: u8, dark: bool) -> Option<AtlasGlyph> {
    let font = &self.faces[face as usize];
    // Only smoothing depends on the text's polarity.
    let dark = dark && self.smooth;
    let raster = Raster { scale: self.scale, smooth: self.smooth, dark };
    match self.color_faces[face as usize] {
      true => self.color_atlas.glyph(font, raster, (face, id, 0), &mut self.uploads),
      false => {
        let key = (face, id, subpixel | (dark as u8) << 2);
        self.atlas.glyph(font, raster, key, &mut self.uploads)
      }
    }
  }

  fn face(&mut self, font: Font) -> u16 {
    if let Some(&(_, face)) = self.fonts.iter().find(|(f, _)| *f == font) {
      return face;
    }

    let face = self.face_index(sys::face(font));
    self.fonts.push((font, face));
    face
  }

  fn face_index(&mut self, face: Face) -> u16 {
    let i = self.faces.iter().position(|f| *f == face).unwrap_or_else(|| {
      self.color_faces.push(sys::is_color(&face));
      self.metrics.push(sys::metrics(&face));
      self.faces.push(face);
      self.faces.len() - 1
    });
    // Faces past the cap start the text system over at the frame's end; this many in one frame
    // would take a font per glyph.
    u16::try_from(i).expect("too many font faces in one frame")
  }

  fn shape(&mut self, s: &str, face: u16) -> Line {
    let base = self.faces[face as usize].clone();
    sys::shape(&base, s, |f| self.face_index(f))
  }
}

/// FxHash (rustc's hasher): a multiply and a rotate per word, enough for cache keys.
#[derive(Clone, Copy, Default)]
pub struct FxHasher(u64);

impl FxHasher {
  fn add(&mut self, word: u64) {
    self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
  }
}

impl Hasher for FxHasher {
  fn write(&mut self, bytes: &[u8]) {
    let (words, rest) = bytes.as_chunks::<8>();
    for w in words {
      self.add(u64::from_le_bytes(*w));
    }

    for &b in rest {
      self.add(b as u64);
    }
  }

  fn write_u8(&mut self, i: u8) {
    self.add(i as u64);
  }

  fn write_u16(&mut self, i: u16) {
    self.add(i as u64);
  }

  fn write_u64(&mut self, i: u64) {
    self.add(i);
  }

  fn write_usize(&mut self, i: usize) {
    self.add(i as u64);
  }

  fn finish(&self) -> u64 {
    self.0
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn line_cache_hits_and_evicts() {
    let mut ts = TextSystem::new(2.0);
    let font = Font::ui(13.0);
    let a = ts.layout("hello", font);
    assert!(a.width > 0.0);
    assert!(Rc::ptr_eq(&a, &ts.layout("hello", font)));
    assert!(!Rc::ptr_eq(&a, &ts.layout("hello", Font::mono(13.0))));
    assert_eq!(ts.measure("hello", font), a.width);

    ts.line_cap = 2;
    ts.end_frame();
    ts.layout("world", font);
    assert_eq!(ts.lines.len(), 3);
    ts.end_frame();
    assert_eq!(ts.lines.len(), 1);
    assert!(!Rc::ptr_eq(&a, &ts.layout("hello", font)));
  }

  #[test]
  fn metrics_of_system_font() {
    let mut ts = TextSystem::new(2.0);
    let m = ts.metrics(Font::ui(13.0));
    // SF Pro's ascent is 0.95 em, Segoe UI's 1.08.
    assert!(m.ascent > 11.0 && m.ascent < 14.5 && m.descent > 2.0 && m.descent < 5.0, "{m:?}");
    assert_eq!(m.line_height(), (m.ascent + m.descent + m.leading).ceil());
  }

  #[test]
  fn runs_share_faces() {
    let mut ts = TextSystem::new(2.0);
    let font = Font::ui(13.0);
    let rocket = ts.layout("\u{1F680}", font).glyphs[0];
    if cfg!(target_os = "linux") && !ts.color_faces[rocket.face as usize] {
      eprintln!("no colour emoji font (fonts-noto-color-emoji): skipped");
      return;
    }

    for s in ["a", "b", "a\u{1F680}", "b\u{1F680}", "\u{1F680}c"] {
      ts.layout(s, font);
    }

    assert_eq!(ts.faces.len(), 2);
  }

  #[test]
  fn glyphs_rasterize_once() {
    let mut ts = TextSystem::new(2.0);
    let line = ts.layout("ab ", Font::ui(13.0));
    assert_eq!(line.glyphs.len(), 3);
    let g = line.glyphs[0];
    let a = ts.glyph(g.face, g.id, 0, false).unwrap();
    assert_eq!(ts.uploads().count(), 1);
    assert_eq!(ts.glyph(g.face, g.id, 0, false).unwrap().uv, a.uv);
    assert_eq!(ts.uploads().count(), 0);
    let space = line.glyphs[2];
    assert!(ts.glyph(space.face, space.id, 0, false).is_none());
  }

  #[test]
  fn emoji_go_to_the_color_atlas() {
    let mut ts = TextSystem::new(2.0);
    let line = ts.layout("a\u{1F680}", Font::ui(13.0));
    let (a, rocket) = (line.glyphs[0], line.glyphs[1]);
    if cfg!(target_os = "linux") && !ts.color_faces[rocket.face as usize] {
      eprintln!("no colour emoji font (fonts-noto-color-emoji): skipped");
      return;
    }

    assert_ne!(a.face, rocket.face);
    assert!(!ts.glyph(a.face, a.id, 1, false).unwrap().color);
    let g = ts.glyph(rocket.face, rocket.id, 3, false).unwrap();
    assert!(g.color);
    let ups: Vec<_> = ts.uploads().collect();
    assert_eq!(ups.len(), 2);
    let up = &ups[1];
    assert!(up.color && up.data.len() == up.w * up.h * 4);
    // Colourful: some pixel has channels far apart.
    let colorful = up.data.as_chunks::<4>().0.iter().any(|p| p[0].abs_diff(p[2]) > 64);
    assert!(colorful);
    // One variant for every subpixel offset.
    assert_eq!(ts.glyph(rocket.face, rocket.id, 0, false).unwrap().uv, g.uv);
  }

  #[test]
  fn atlas_reset_bumps_epoch() {
    let mut ts = TextSystem::new(2.0);
    let g = ts.layout("x", Font::ui(13.0)).glyphs[0];
    ts.glyph(g.face, g.id, 0, false);
    assert_eq!(ts.uploads().next().unwrap().epoch, 0);
    ts.set_smoothing(!ts.smoothing());
    ts.glyph(g.face, g.id, 0, false);
    assert_eq!(ts.uploads().next().unwrap().epoch, 1);
  }

  #[test]
  fn full_atlas_redraws_once() {
    let mut ts = TextSystem::new(2.0);
    ts.atlas = Atlas::new(64, false);
    let line = ts.layout("abcdefghijklmnop", Font::ui(13.0));
    let frame = |ts: &mut TextSystem| {
      for g in &line.glyphs {
        ts.glyph(g.face, g.id, 0, false);
      }

      ts.end_frame()
    };
    // Full: reset and draw again; full again in that frame: no third draw; the next frame may.
    assert!(frame(&mut ts));
    assert!(!frame(&mut ts));
    assert!(frame(&mut ts));
    // A glyph larger than the atlas is left out without filling it.
    let big = ts.layout("W", Font::ui(200.0)).glyphs[0];
    ts.atlas.reset();
    assert!(ts.glyph(big.face, big.id, 0, false).is_none());
    assert!(!ts.atlas.full && !ts.end_frame());
  }

  #[test]
  fn faces_start_over_past_the_cap() {
    let mut ts = TextSystem::new(2.0);
    for i in 0..=FACE_CAP {
      ts.metrics(Font::ui(8.0 + i as f32 * 0.25));
    }

    assert_eq!(ts.metrics(Font::ui(8.0)), ts.metrics(Font::ui(8.0)));
    assert!(ts.faces.len() > FACE_CAP);
    assert!(ts.end_frame());
    assert!(ts.faces.is_empty() && ts.fonts.is_empty() && ts.lines.is_empty());
    let g = ts.layout("x", Font::ui(13.0)).glyphs[0];
    assert!(ts.glyph(g.face, g.id, 0, false).is_some());
    assert!(!ts.end_frame());
  }

  #[test]
  fn scale_change_keeps_lines() {
    let mut ts = TextSystem::new(1.0);
    let line = ts.layout("scale", Font::ui(13.0));
    let g = line.glyphs[0];
    let small = ts.glyph(g.face, g.id, 0, false).unwrap();
    ts.set_scale(2.0);
    assert!(Rc::ptr_eq(&line, &ts.layout("scale", Font::ui(13.0))));
    let big = ts.glyph(g.face, g.id, 0, false).unwrap();
    assert!(big.uv.h > small.uv.h * 1.5, "{big:?} vs {small:?}");
  }
}
