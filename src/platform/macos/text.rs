// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The text backend on CoreText: system fonts, shaping with font fallback (CTLine), metrics, and
//! glyphs drawn by CoreGraphics. Fonts are created and shaped at their point size, as AppKit
//! does, so the system font picks the same optical size (SF Pro Text at 13 pt) at any backing
//! scale; only glyph bitmaps are made at the pixel size.

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2_app_kit::{
  NSFont, NSFontWeightBold, NSFontWeightMedium, NSFontWeightRegular, NSFontWeightSemibold,
};
use objc2_core_foundation::{
  CFAttributedString, CFDictionary, CFIndex, CFRange, CFRetained, CFString, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
  CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, kCGColorSpaceSRGB,
};
use objc2_core_text::{
  CTFont, CTFontOrientation, CTFontSymbolicTraits, CTLine, CTRun, kCTFontAttributeName,
};

use crate::text::{Font, Glyph, GlyphBox, Line, Metrics, Raster, Weight};

/// A CTFont at its point size.
#[derive(Clone)]
pub struct Face(CFRetained<CTFont>);

impl PartialEq for Face {
  fn eq(&self, other: &Self) -> bool {
    *self.0 == *other.0
  }
}

/// The system font (SF Pro, or SF Mono when `font.mono`) at its size and weight.
pub fn face(font: Font) -> Face {
  let weight = unsafe {
    match font.weight {
      Weight::Regular => NSFontWeightRegular,
      Weight::Medium => NSFontWeightMedium,
      Weight::Semibold => NSFontWeightSemibold,
      Weight::Bold => NSFontWeightBold,
    }
  };
  let ns = match font.mono {
    true => NSFont::monospacedSystemFontOfSize_weight(font.size as f64, weight),
    false => NSFont::systemFontOfSize_weight(font.size as f64, weight),
  };

  Face(ns_font(ns))
}

pub fn metrics(face: &Face) -> Metrics {
  let f = &face.0;
  let (ascent, descent, leading) = unsafe { (f.ascent(), f.descent(), f.leading()) };
  let (cap, x) = unsafe { (f.cap_height(), f.x_height()) };
  let (ascent, descent, leading) = (ascent as f32, descent as f32, leading as f32);
  Metrics { ascent, descent, leading, cap_height: cap as f32, x_height: x as f32 }
}

/// Its glyphs are colour bitmaps (Apple Color Emoji).
pub fn is_color(face: &Face) -> bool {
  let traits = unsafe { face.0.symbolic_traits() };
  traits.contains(CTFontSymbolicTraits::TraitColorGlyphs)
}

/// Shapes `s` with CoreText (kerning, ligatures, font fallback); `index` gives the text
/// system's index of each run's font.
pub fn shape(face: &Face, s: &str, mut index: impl FnMut(Face) -> u16) -> Line {
  let key = unsafe { kCTFontAttributeName };
  let attrs = CFDictionary::<CFString, CTFont>::from_slices(&[key], &[&face.0]);
  let string = CFString::from_str(s);
  let attributed = unsafe { CFAttributedString::new(None, Some(&string), Some(attrs.as_opaque())) };
  let line = unsafe { CTLine::with_attributed_string(&attributed.unwrap()) };
  let null = std::ptr::null_mut();
  let width = unsafe { line.typographic_bounds(null, null, null) } as f32;
  let runs = unsafe { line.glyph_runs() };
  let runs = unsafe { runs.cast_unchecked::<CTRun>() };
  // UTF-16 offsets (CoreText's string indices) to bytes, unless they are the same.
  let bytes: Vec<u32> = match s.is_ascii() {
    true => Vec::new(),
    false => s
      .char_indices()
      .flat_map(|(i, c)| std::iter::repeat_n(i as u32, c.len_utf16()))
      .chain([s.len() as u32])
      .collect(),
  };

  let (mut glyphs, mut clusters) = (Vec::new(), Vec::new());
  for i in 0..runs.len() {
    let run = runs.get(i).unwrap();
    let attrs = unsafe { run.attributes() };
    let attrs = unsafe { attrs.cast_unchecked::<CFString, CTFont>() };
    let face = index(Face(attrs.get(key).unwrap()));
    let n = unsafe { run.glyph_count() } as usize;
    let mut ids = vec![0u16; n];
    let mut pos = vec![CGPoint::new(0.0, 0.0); n];
    let mut idx = vec![0 as CFIndex; n];
    let all = CFRange { location: 0, length: 0 };
    unsafe {
      run.glyphs(all, NonNull::new(ids.as_mut_ptr()).unwrap());
      run.positions(all, NonNull::new(pos.as_mut_ptr()).unwrap());
      run.string_indices(all, NonNull::new(idx.as_mut_ptr()).unwrap());
    }
    let at = |&u: &CFIndex| match bytes.is_empty() {
      true => u as u32,
      false => bytes.get(u as usize).copied().unwrap_or(s.len() as u32),
    };
    glyphs.extend(ids.iter().zip(&pos).map(|(&id, p)| Glyph {
      face,
      id,
      x: p.x as f32,
      y: p.y as f32,
    }));
    clusters.extend(idx.iter().map(at));
  }

  Line { glyphs, clusters, width, epoch: 0 }
}

fn ns_font(font: Retained<NSFont>) -> CFRetained<CTFont> {
  // NSFont is toll-free bridged to CTFont.
  let ptr = NonNull::new(Retained::into_raw(font).cast::<CTFont>()).unwrap();
  unsafe { CFRetained::from_raw(ptr) }
}

// MARK: Glyph raster

/// The pixel box of glyph `id` at `scale`, drawn `subpixel` quarters of a pixel to the right:
/// its ink bounds rounded out, with one pixel of padding on each side for antialiasing (and
/// font smoothing's dilation). `None` for a glyph with no ink (a space).
pub fn glyph_box(face: &Face, mut id: u16, scale: f32, subpixel: u8) -> Option<GlyphBox> {
  let scale = scale as f64;
  let glyph = NonNull::from(&mut id);
  let b: CGRect = unsafe {
    face.0.bounding_rects_for_glyphs(CTFontOrientation::Default, glyph, std::ptr::null_mut(), 1)
  };
  if b.size.width <= 0.0 || b.size.height <= 0.0 {
    return None;
  }

  let (origin, size) = (b.origin, b.size);
  let b = CGRect::new(
    CGPoint::new(origin.x * scale, origin.y * scale),
    CGSize::new(size.width * scale, size.height * scale),
  );
  let shift = subpixel as f64 / 4.0;
  let left = (b.origin.x + shift).floor() - 1.0;
  let bottom = b.origin.y.floor() - 1.0;
  let w = ((b.origin.x + b.size.width + shift).ceil() + 1.0 - left) as usize;
  let h = ((b.origin.y + b.size.height).ceil() + 1.0 - bottom) as usize;
  Some(GlyphBox { left: left as i32, bottom: bottom as i32, w, h })
}

/// Draws glyph `id` into the pixels of `b` (from [glyph_box] with the same `subpixel`), rows
/// top-down: coverage, one byte per pixel, or premultiplied RGBA for a colour face (`color`).
pub fn rasterize(
  face: &Face,
  id: u16,
  subpixel: u8,
  b: &GlyphBox,
  raster: Raster,
  color: bool,
) -> Option<Vec<u8>> {
  let scale = raster.scale as f64;
  let shift = subpixel as f64 / 4.0;
  let origin = CGPoint::new((-b.left as f64 + shift) / scale, -b.bottom as f64 / scale);
  match color {
    true => draw_color(&face.0, id, origin, scale, b.w, b.h),
    false => draw_coverage(&face.0, id, origin, raster, b.w, b.h),
  }
}

fn context_setup(ctx: &CGContext, scale: f64, smooth: bool) {
  CGContext::set_should_antialias(Some(ctx), true);
  CGContext::set_allows_font_smoothing(Some(ctx), smooth);
  CGContext::set_should_smooth_fonts(Some(ctx), smooth);
  CGContext::set_allows_font_subpixel_positioning(Some(ctx), true);
  CGContext::set_should_subpixel_position_fonts(Some(ctx), true);
  CGContext::set_allows_font_subpixel_quantization(Some(ctx), false);
  CGContext::scale_ctm(Some(ctx), scale, scale);
}

/// Coverage, one byte per pixel. Without smoothing an alpha-only context gives it directly;
/// CoreGraphics smooths fonts only in a colour context, so with smoothing the glyph is drawn
/// in an opaque one, white on black (or black on white for dark text), and the green channel
/// gives the coverage.
fn draw_coverage(
  font: &CTFont,
  mut id: u16,
  mut origin: CGPoint,
  raster: Raster,
  w: usize,
  h: usize,
) -> Option<Vec<u8>> {
  let scale = raster.scale as f64;
  let glyph = NonNull::from(&mut id);
  let origin_ptr = NonNull::from(&mut origin);
  if !raster.smooth {
    let mut data = vec![0u8; w * h];
    let ptr = data.as_mut_ptr().cast::<c_void>();
    let alpha = CGImageAlphaInfo::Only.0;
    let ctx = unsafe { CGBitmapContextCreate(ptr, w, h, 8, w, None, alpha) }?;
    context_setup(&ctx, scale, false);
    unsafe { font.draw_glyphs(glyph, origin_ptr, 1, &ctx) };
    return Some(data);
  }

  let mut rgba = vec![0u8; w * h * 4];
  let ptr = rgba.as_mut_ptr().cast::<c_void>();
  let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }));
  let info = CGImageAlphaInfo::NoneSkipLast.0;
  let ctx = unsafe { CGBitmapContextCreate(ptr, w, h, 8, w * 4, space.as_deref(), info) }?;
  let (bg, fg) = if raster.dark { (1.0, 0.0) } else { (0.0, 1.0) };
  CGContext::set_rgb_fill_color(Some(&ctx), bg, bg, bg, 1.0);
  CGContext::fill_rect(Some(&ctx), CGRect::new(CGPoint::ZERO, CGSize::new(w as _, h as _)));
  CGContext::set_rgb_fill_color(Some(&ctx), fg, fg, fg, 1.0);
  context_setup(&ctx, scale, true);
  unsafe { font.draw_glyphs(glyph, origin_ptr, 1, &ctx) };
  drop(ctx);
  let flip = if raster.dark { 255 } else { 0 };
  Some(rgba.as_chunks::<4>().0.iter().map(|p| p[1] ^ flip).collect())
}

/// Premultiplied RGBA in sRGB, for colour glyphs.
fn draw_color(
  font: &CTFont,
  mut id: u16,
  mut origin: CGPoint,
  scale: f64,
  w: usize,
  h: usize,
) -> Option<Vec<u8>> {
  let mut data = vec![0u8; w * h * 4];
  let ptr = data.as_mut_ptr().cast::<c_void>();
  let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }));
  let info = CGImageAlphaInfo::PremultipliedLast.0;
  let ctx = unsafe { CGBitmapContextCreate(ptr, w, h, 8, w * 4, space.as_deref(), info) }?;
  context_setup(&ctx, scale, false);
  unsafe { font.draw_glyphs(NonNull::from(&mut id), NonNull::from(&mut origin), 1, &ctx) };
  drop(ctx);
  Some(data)
}
