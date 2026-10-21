// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Shelf-packed glyph atlases, filled by the platform's text backend: coverage into an R8 one
//! with 4 horizontal subpixel variants per glyph, colour glyphs (emoji) into an RGBA one. New
//! bitmaps queue as uploads for the renderer, stamped with the atlas epoch so it can tell when
//! a reset makes them overwrite pixels that frames still in flight read.

use std::collections::HashMap;
use std::hash::BuildHasherDefault;

use super::{FxHasher, Raster};
use crate::platform::text::{self as sys, Face};
use crate::scene::Rect;

/// Width and height of the coverage atlas in pixels (R8: 4 MB).
pub const ATLAS_SIZE: usize = 2048;
/// Width and height of the colour atlas in pixels (RGBA: 4 MB).
pub const COLOR_ATLAS_SIZE: usize = 1024;

/// Where a glyph's bitmap sits in its atlas, and its offset from the pen position in pixels:
/// `left` to the right of it, `top` above the baseline.
#[derive(Clone, Copy, Debug)]
pub struct AtlasGlyph {
  pub uv: Rect,
  pub left: f32,
  pub top: f32,
  /// In the RGBA atlas (a colour glyph), else in the R8 one.
  pub color: bool,
}

/// A rectangle of new atlas pixels, rows top-down: one byte each, or premultiplied RGBA for
/// the colour atlas.
pub struct Upload {
  pub color: bool,
  /// The atlas generation the pixels belong to; it changes on every reset.
  pub epoch: u32,
  pub x: usize,
  pub y: usize,
  pub w: usize,
  pub h: usize,
  pub data: Vec<u8>,
}

pub(super) struct Atlas {
  size: usize,
  color: bool,
  glyphs: HashMap<(u16, u16, u8), Option<AtlasGlyph>, BuildHasherDefault<FxHasher>>,
  /// Shelf packing cursor: x, y and the height of the current row.
  shelf: (usize, usize, usize),
  /// A glyph didn't fit this frame; the text system resets the atlas at the frame's end.
  pub full: bool,
  pub epoch: u32,
}

impl Atlas {
  pub fn new(size: usize, color: bool) -> Self {
    let glyphs = HashMap::default();
    Self { size, color, glyphs, shelf: (0, 0, 0), full: false, epoch: 0 }
  }

  pub fn reset(&mut self) {
    self.glyphs.clear();
    self.shelf = (0, 0, 0);
    self.full = false;
    self.epoch += 1;
  }

  /// The atlas slot of a glyph, rasterized on first use with `raster`; `None` for glyphs with
  /// no pixels (spaces) or when the atlas is full.
  pub fn glyph(
    &mut self,
    face: &Face,
    raster: Raster,
    key: (u16, u16, u8),
    uploads: &mut Vec<Upload>,
  ) -> Option<AtlasGlyph> {
    if let Some(g) = self.glyphs.get(&key) {
      return *g;
    }

    // The variant's low 2 bits are the subpixel offset; the bit above, the text's polarity
    // ([super::TextSystem::glyph]), only picks the raster.
    let g = self.rasterize(face, raster, key.1, key.2 & 3, uploads);
    // One skipped for lack of room stays out until the reset clears the cache and gives it a
    // slot, rather than rendering again at every use.
    self.glyphs.insert(key, g);
    g
  }

  /// Draws the glyph of a point-sized font scaled to pixels, `subpixel` quarters of a pixel to
  /// the right.
  fn rasterize(
    &mut self,
    face: &Face,
    raster: Raster,
    id: u16,
    subpixel: u8,
    uploads: &mut Vec<Upload>,
  ) -> Option<AtlasGlyph> {
    let b = sys::glyph_box(face, id, raster.scale, subpixel)?;
    let (x, y) = self.pack(b.w, b.h)?;
    let data = sys::rasterize(face, id, subpixel, &b, raster, self.color)?;
    uploads.push(Upload { color: self.color, epoch: self.epoch, x, y, w: b.w, h: b.h, data });
    let uv = Rect::new(x as f32, y as f32, b.w as f32, b.h as f32);
    let top = (b.bottom + b.h as i32) as f32;
    Some(AtlasGlyph { uv, left: b.left as f32, top, color: self.color })
  }

  /// Shelf packing: fill rows left to right, start a new row when one is full. A bitmap larger
  /// than the atlas never fits: it is left out without filling the atlas.
  fn pack(&mut self, w: usize, h: usize) -> Option<(usize, usize)> {
    if w > self.size || h > self.size {
      return None;
    }

    let (x, y, row_h) = &mut self.shelf;
    if *x + w > self.size {
      (*x, *y, *row_h) = (0, *y + *row_h, 0);
    }

    if *y + h > self.size {
      self.full = true;
      return None;
    }

    let at = (*x, *y);
    *x += w;
    *row_h = (*row_h).max(h);
    Some(at)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::text::Font;

  #[test]
  fn pack_rows_and_overflow() {
    let mut a = Atlas::new(ATLAS_SIZE, false);
    assert_eq!(a.pack(ATLAS_SIZE - 10, 20), Some((0, 0)));
    assert_eq!(a.pack(20, 10), Some((0, 20)));
    assert_eq!(a.pack(5, ATLAS_SIZE), None);
    assert!(a.full);
    // Larger than the atlas: never fits, and doesn't fill it.
    let mut b = Atlas::new(64, false);
    assert_eq!((b.pack(65, 5), b.pack(5, 65), b.full), (None, None, false));
    a.reset();
    assert_eq!((a.pack(5, 5), a.epoch), (Some((0, 0)), 1));
  }

  #[test]
  fn a_glyph_without_room_waits_for_the_reset() {
    let face = sys::face(Font::ui(13.0));
    let id = sys::shape(&face, "W", |_| 0).glyphs[0].id;
    let raster = Raster { scale: 2.0, smooth: false, dark: true };
    let (mut a, mut uploads, key) = (Atlas::new(64, false), Vec::new(), (0, id, 0));
    // Rows for 4 pixels left: a 26-pixel W doesn't fit.
    a.pack(64, 60);
    assert!(a.glyph(&face, raster, key, &mut uploads).is_none() && a.full);
    // Not rendered again at every use until the atlas starts over.
    assert!(a.glyphs.contains_key(&key));
    a.reset();
    assert!(a.glyph(&face, raster, key, &mut uploads).is_some());
  }
}
