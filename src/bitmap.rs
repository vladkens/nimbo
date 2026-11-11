// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Pictures decoded by the OS's own image decoders ([Bitmap::decode]): the photos people send, at
//! the size they show, never the camera's full size. JPEG, PNG and GIF (its first frame)
//! everywhere; HEIC and WebP where the OS reads them.
//!
//! | | decoder | HEIC, WebP |
//! |---|---|---|
//! | macOS | ImageIO's thumbnail at the size (`CGImageSourceCreateThumbnailAtIndex`), drawn in sRGB | both (WebP from 11) |
//! | Windows | WIC, scaled by its Fant scaler, turned in Rust by its orientation | with the Store's HEIF and WebP extensions |
//! | Linux | gdk-pixbuf's loader (loaded at run time), sized as it reads | with the distribution's loaders (`webp-pixbuf-loader`, `libheif`) |
//!
//! Decoding is plain data in, pixels out, callable from any thread: a large photo takes tens of
//! milliseconds, so an app decodes off the main thread and hands the [Bitmap] (`Send`) back
//! through its [crate::Proxy]. On Windows the call joins COM's multithreaded apartment for its
//! thread when the thread is in none.
//!
//! A GIF's first frame on Windows is that frame alone, without the rest of its screen when it
//! covers only part of it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::platform::image;

/// A decoded picture: its size in pixels and premultiplied RGBA pixels, rows top-down, shared by
/// its clones. [crate::Ui::image] shows it (sharing its pixels, no copy), added to the window's
/// images the first frame a view builds it and kept while each frame does, by its identity (a clone
/// is the same picture), so a view keeps its photos as [Bitmap]s and builds them every frame for
/// nothing.
#[derive(Clone)]
pub struct Bitmap {
  w: u32,
  h: u32,
  /// Shared with the window's image ([crate::Ui::image]): shown, it costs no copy.
  pub(crate) px: Arc<Vec<u8>>,
  /// Its identity: the same for its clones, another for each picture decoded.
  pub(crate) key: u64,
}

impl Bitmap {
  /// The picture in `file`, the bytes of an image file, shrunk on decoding so that its longer
  /// side is at most `side` pixels (keeping its aspect; `u32::MAX` for its full size): 600 for a
  /// photo shown 300 points wide on a Retina screen, which costs 600 px of memory however large
  /// the photo. Turned upright as its EXIF orientation says. An error when the OS can't read it
  /// (another format, a broken file), when the file declares over a gigapixel, when the picture
  /// would be over 128 Mi pixels (512 MiB; a smaller `side` decodes it), or when there is no
  /// memory for it. Colours come in sRGB: macOS converts an embedded profile (Display P3, Adobe
  /// RGB); Windows and Linux ignore it, taking the values as sRGB.
  ///
  /// ```ignore
  /// let photo = Bitmap::decode(&std::fs::read(path)?, 600)?;
  /// ```
  pub fn decode(file: &[u8], side: u32) -> Result<Self, String> {
    let (w, h, px) = image::decode(file, side.max(1))?;
    debug_assert_eq!(px.len(), w as usize * h as usize * 4);
    Ok(Self { w, h, px: Arc::new(px), key: next_key() })
  }

  pub fn width(&self) -> u32 {
    self.w
  }

  pub fn height(&self) -> u32 {
    self.h
  }

  /// Its premultiplied RGBA pixels, rows top-down.
  pub fn pixels(&self) -> &[u8] {
    &self.px
  }
}

impl std::fmt::Debug for Bitmap {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    write!(f, "Bitmap({} × {})", self.w, self.h)
  }
}

fn next_key() -> u64 {
  static NEXT: AtomicU64 = AtomicU64::new(1);
  NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The most pixels a file may declare, and a decoded picture have: a crafted file of a few bytes
/// can declare 60000 × 60000.
const MAX_SOURCE: u64 = 1 << 30;
pub(crate) const MAX_PIXELS: u64 = 1 << 27;

/// The size a `w` × `h` picture decodes to at `side` ([fit]): an error over the limits, read
/// from the file's header before decoding.
pub(crate) fn size(w: u32, h: u32, side: u32) -> Result<(u32, u32), String> {
  if w as u64 * h as u64 > MAX_SOURCE {
    return Err(format!("a {w} × {h} picture is too large"));
  }

  let (fw, fh) = fit(w, h, side);
  match fw as u64 * fh as u64 {
    0 => Err("an empty picture".into()),
    n if n > MAX_PIXELS => Err(format!("a {fw} × {fh} picture is too large: decode it smaller")),
    _ => Ok((fw, fh)),
  }
}

/// Zeroed pixels for a `w` × `h` picture, allocated once for a decoder to fill: an error over
/// [MAX_PIXELS] or out of memory.
pub(crate) fn zeroed(w: u32, h: u32) -> Result<Vec<u8>, String> {
  let n = w as u64 * h as u64;
  if n > MAX_PIXELS {
    return Err(format!("a {w} × {h} picture is too large: decode it smaller"));
  }

  let mut px = Vec::new();
  px.try_reserve_exact(n as usize * 4).map_err(|_| format!("no memory for a {w} × {h} picture"))?;
  px.resize(n as usize * 4, 0);
  Ok(px)
}

/// `rgba`'s straight pixels premultiplied, in place.
pub(crate) fn premultiply(rgba: &mut [u8]) {
  for p in rgba.as_chunks_mut::<4>().0 {
    let a = p[3] as u32;
    for c in &mut p[..3] {
      *c = ((*c as u32 * a + 127) / 255) as u8;
    }
  }
}

/// The size of a `w` × `h` picture shrunk so its longer side is at most `side`, keeping its
/// aspect.
pub(crate) fn fit(w: u32, h: u32, side: u32) -> (u32, u32) {
  let long = w.max(h);
  if long <= side {
    return (w, h);
  }

  let scale = |v: u32| ((v as u64 * side as u64 + long as u64 / 2) / long as u64).max(1) as u32;
  (scale(w), scale(h))
}

/// `px`, `w` × `h` pixels as stored, turned upright as EXIF `orientation` (1–8) says: its size
/// and pixels. Windows' decoder gives them as stored.
#[cfg_attr(not(any(test, target_os = "windows")), allow(dead_code))]
pub(crate) fn orient(w: u32, h: u32, px: Vec<u8>, o: u16) -> Result<(u32, u32, Vec<u8>), String> {
  let (w, h) = (w as usize, h as usize);
  // The stored pixel shown at `x`, `y`: mirrored (2, 4), turned half (3), across a diagonal (5,
  // 7), turned right (6) or left (8).
  let from: fn(usize, usize, usize, usize) -> (usize, usize) = match o {
    2 => |x, y, w, _| (w - 1 - x, y),
    3 => |x, y, w, h| (w - 1 - x, h - 1 - y),
    4 => |x, y, _, h| (x, h - 1 - y),
    5 => |x, y, _, _| (y, x),
    6 => |x, y, _, h| (y, h - 1 - x),
    7 => |x, y, w, h| (w - 1 - y, h - 1 - x),
    8 => |x, y, w, _| (w - 1 - y, x),
    _ => return Ok((w as u32, h as u32, px)),
  };

  let (ow, oh) = if o >= 5 { (h, w) } else { (w, h) };
  let mut turned = zeroed(ow as u32, oh as u32)?;
  for y in 0..oh {
    for x in 0..ow {
      let (sx, sy) = from(x, y, w, h);
      let (to, at) = ((y * ow + x) * 4, (sy * w + sx) * 4);
      turned[to..to + 4].copy_from_slice(&px[at..at + 4]);
    }
  }

  Ok((ow as u32, oh as u32, turned))
}

#[cfg(test)]
mod tests {
  use super::*;

  // `tests/images/`: a 64 × 48 picture of four quadrants (red, green, blue, yellow from the top
  // left), as a JPEG, a GIF, a PNG whose yellow is half transparent, and as JPEGs stored turned
  // or mirrored for each EXIF orientation (`orientation-N.jpg`); drawn in Python, made JPEGs and
  // a GIF by `sips`.
  const JPEG: &[u8] = include_bytes!("../tests/images/quadrants.jpg");
  const GIF: &[u8] = include_bytes!("../tests/images/quadrants.gif");
  const PNG: &[u8] = include_bytes!("../tests/images/quadrants.png");
  const ORIENTED: [&[u8]; 8] = [
    include_bytes!("../tests/images/orientation-1.jpg"),
    include_bytes!("../tests/images/orientation-2.jpg"),
    include_bytes!("../tests/images/orientation-3.jpg"),
    include_bytes!("../tests/images/orientation-4.jpg"),
    include_bytes!("../tests/images/orientation-5.jpg"),
    include_bytes!("../tests/images/orientation-6.jpg"),
    include_bytes!("../tests/images/orientation-7.jpg"),
    include_bytes!("../tests/images/orientation-8.jpg"),
  ];
  /// Stored turned left, shown upright.
  const TURNED: &[u8] = ORIENTED[5];

  const RED: [u8; 4] = [220, 40, 40, 255];
  const GREEN: [u8; 4] = [40, 180, 60, 255];
  const BLUE: [u8; 4] = [40, 80, 220, 255];
  const YELLOW: [u8; 4] = [240, 220, 40, 255];

  /// The pixel at `x`, `y` of `b`.
  fn at(b: &Bitmap, x: u32, y: u32) -> [u8; 4] {
    let i = (y * b.width() + x) as usize * 4;
    b.pixels()[i..i + 4].try_into().unwrap()
  }

  /// The quadrants' centres are their colours, within a JPEG's error.
  fn assert_quadrants(b: &Bitmap, yellow: [u8; 4]) {
    let (w, h) = (b.width(), b.height());
    let centres = [(w / 4, h / 4), (w * 3 / 4, h / 4), (w / 4, h * 3 / 4), (w * 3 / 4, h * 3 / 4)];
    for ((x, y), want) in centres.into_iter().zip([RED, GREEN, BLUE, yellow]) {
      let got = at(b, x, y);
      let off = got.iter().zip(want).map(|(a, b)| a.abs_diff(b)).max().unwrap();
      assert!(off <= 12, "{x}, {y} of {b:?}: {got:?}, not {want:?}");
    }
  }

  #[test]
  fn decodes_a_jpeg() {
    let b = Bitmap::decode(JPEG, u32::MAX).unwrap();
    assert_eq!((b.width(), b.height()), (64, 48));
    assert_quadrants(&b, YELLOW);
  }

  /// A PNG's transparency comes premultiplied.
  #[test]
  fn decodes_a_png_premultiplied() {
    let b = Bitmap::decode(PNG, u32::MAX).unwrap();
    assert_eq!((b.width(), b.height()), (64, 48));
    assert_quadrants(&b, [120, 110, 20, 128]);
  }

  /// Stored turned or mirrored, shown upright: each EXIF orientation.
  #[test]
  fn turns_by_the_exif_orientation() {
    for (i, file) in ORIENTED.into_iter().enumerate() {
      let b = Bitmap::decode(file, u32::MAX).unwrap();
      assert_eq!((b.width(), b.height()), (64, 48), "orientation {}", i + 1);
      assert_quadrants(&b, YELLOW);
    }
  }

  /// A GIF's first frame.
  #[test]
  fn decodes_a_gif() {
    let b = Bitmap::decode(GIF, u32::MAX).unwrap();
    assert_eq!((b.width(), b.height()), (64, 48));
    assert_quadrants(&b, YELLOW);
  }

  /// The longer side shrinks to the side asked, the shorter one in proportion; never grows.
  #[test]
  fn shrinks_to_the_side() {
    let b = Bitmap::decode(JPEG, 16).unwrap();
    assert_eq!((b.width(), b.height()), (16, 12));
    assert_quadrants(&b, YELLOW);
    let b = Bitmap::decode(TURNED, 32).unwrap();
    assert_eq!((b.width(), b.height()), (32, 24));
    assert_quadrants(&b, YELLOW);
    let b = Bitmap::decode(JPEG, 1000).unwrap();
    assert_eq!((b.width(), b.height()), (64, 48));
  }

  /// Not an image, or a broken one: an error, no panic.
  #[test]
  fn a_broken_file_is_an_error() {
    assert!(Bitmap::decode(b"not an image at all", 64).is_err());
    assert!(Bitmap::decode(&[], 64).is_err());
    assert!(Bitmap::decode(&JPEG[..20], 64).is_err());
    assert!(Bitmap::decode(&PNG[..40], 64).is_err());
  }

  /// `file` with its header declaring a `w` × `h` picture: a JPEG's SOF0, a PNG's IHDR (its
  /// CRC made again).
  fn declaring(file: &[u8], w: u16, h: u16) -> Vec<u8> {
    let mut f = file.to_vec();
    if let Some(at) = f.windows(2).position(|m| m == [0xff, 0xc0]) {
      f[at + 5..at + 9].copy_from_slice(&[h.to_be_bytes(), w.to_be_bytes()].concat());
      return f;
    }

    f[16..24].copy_from_slice(&[(w as u32).to_be_bytes(), (h as u32).to_be_bytes()].concat());
    let mut c = !0u32;
    for &b in &f[12..29] {
      c ^= b as u32;
      (0..8).for_each(|_| c = if c & 1 == 1 { 0xedb88320 ^ (c >> 1) } else { c >> 1 });
    }

    f[29..33].copy_from_slice(&(!c).to_be_bytes());
    f
  }

  /// A file of a few bytes declaring a huge picture: refused from its header (by the limits, or
  /// by the OS's decoder first), never decoded or allocated; at a side small enough the same size
  /// passes the limits.
  #[test]
  fn a_huge_picture_is_refused() {
    for file in [JPEG, PNG] {
      for (w, h, side) in [(60000, 60000, 300), (20000, 20000, u32::MAX)] {
        let got = Bitmap::decode(&declaring(file, w, h), side);
        assert!(got.is_err(), "{w} × {h} at {side}: {got:?}");
      }
    }

    assert_eq!(size(20000, 20000, 600), Ok((600, 600)));
    assert!(size(60000, 60000, 600).is_err() && size(0, 48, 600).is_err());
    assert!(zeroed(20000, 20000).is_err());
  }

  /// A clone is the same picture; another decode another one.
  #[test]
  fn clones_share_their_identity() {
    let (a, b) = (Bitmap::decode(JPEG, 64).unwrap(), Bitmap::decode(JPEG, 64).unwrap());
    assert_eq!(a.clone().key, a.key);
    assert_ne!(a.key, b.key);
  }

  #[test]
  fn fits_the_longer_side() {
    assert_eq!(fit(6000, 4000, 300), (300, 200));
    assert_eq!(fit(4000, 6000, 300), (200, 300));
    assert_eq!(fit(64, 48, 100), (64, 48));
    assert_eq!(fit(10000, 1, 100), (100, 1));
  }

  /// Each EXIF orientation of a 3 × 2 picture, numbered pixels, turned upright.
  #[test]
  fn orients_as_exif_says() {
    // Upright: 1 2 3 / 4 5 6.
    let upright = [1, 2, 3, 4, 5, 6];
    let stored: [(u16, u32, &[u8]); 8] = [
      (1, 3, &[1, 2, 3, 4, 5, 6]),
      (2, 3, &[3, 2, 1, 6, 5, 4]),
      (3, 3, &[6, 5, 4, 3, 2, 1]),
      (4, 3, &[4, 5, 6, 1, 2, 3]),
      (5, 2, &[1, 4, 2, 5, 3, 6]),
      (6, 2, &[3, 6, 2, 5, 1, 4]),
      (7, 2, &[6, 3, 5, 2, 4, 1]),
      (8, 2, &[4, 1, 5, 2, 6, 3]),
    ];
    for (o, w, px) in stored {
      let px = px.iter().flat_map(|&v| [v; 4]).collect();
      let (w, h, px) = orient(w, 6 / w, px, o).unwrap();
      let got: Vec<u8> = px.chunks(4).map(|p| p[0]).collect();
      assert_eq!((w, h, &got[..]), (3, 2, &upright[..]), "orientation {o}");
    }
  }

  #[test]
  fn premultiplies() {
    let mut px = [255, 128, 0, 128, 10, 20, 30, 255, 200, 200, 200, 0];
    premultiply(&mut px);
    assert_eq!(px, [128, 64, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0]);
  }
}
