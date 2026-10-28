// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tray icons into pixels: SVG path data ([crate::svg]) filled with 16 samples a pixel ([fill]),
//! and RGBA images scaled by averaging ([scale_alpha]); only the alpha, the icons being
//! templates. On Linux also into an SVG a panel recolours ([symbolic]).

use crate::svg::{flatten, spans};

/// Samples a pixel's side is cut into: 4 × 4 per pixel.
const SUB: usize = 4;

/// The alpha of path data `d` (a `size` box) filled into `side` × `side` pixels, non-zero or
/// `even_odd`; what doesn't parse fills as far as it parsed.
pub fn fill(d: &str, size: f32, side: u32, even_odd: bool) -> Vec<u8> {
  let k = side as f32 / size.max(f32::EPSILON);
  let mut polys = Vec::new();
  _ = flatten(d, k * SUB as f32, &mut polys);
  let n = side as usize;
  let mut hits = vec![0u16; n * n];
  let mut xs = Vec::new();
  for row in 0..n * SUB {
    let y = (row as f32 + 0.5) / (SUB as f32 * k);
    spans(&polys, y, even_odd, &mut xs, |x0, x1| {
      // The samples whose centres the span covers.
      let (x0, x1) = (x0 * k * SUB as f32, x1 * k * SUB as f32);
      let first = (x0 - 0.5).ceil().max(0.0) as usize;
      let last = ((x1 - 0.5).ceil().max(0.0) as usize).min(n * SUB);
      for s in first..last {
        hits[row / SUB * n + s / SUB] += 1;
      }
    });
  }

  hits.iter().map(|&h| (h as u32 * 255 / (SUB * SUB) as u32) as u8).collect()
}

/// The alpha of straight RGBA `px` (`w` × `h`) scaled to `side` × `side` by averaging the
/// pixels each one covers; blank for a short buffer.
pub fn scale_alpha(w: u32, h: u32, px: &[u8], side: u32) -> Vec<u8> {
  let (w, h, n) = (w as usize, h as usize, side as usize);
  if w == 0 || h == 0 || px.len() < w * h * 4 {
    return vec![0; n * n];
  }

  let span =
    |i: usize, from: usize| (i * from / n, ((i + 1) * from).div_ceil(n).max(i * from / n + 1));
  let mut out = Vec::with_capacity(n * n);
  for y in 0..n {
    let (y0, y1) = span(y, h);
    for x in 0..n {
      let (x0, x1) = span(x, w);
      let sum: u32 =
        (y0..y1).flat_map(|r| (x0..x1).map(move |c| px[(r * w + c) * 4 + 3] as u32)).sum();
      out.push((sum / ((y1 - y0) * (x1 - x0)) as u32) as u8);
    }
  }

  out
}

/// Alpha levels an image's pixels are grouped by in [symbolic].
#[cfg(any(test, target_os = "linux"))]
const LEVELS: u32 = 8;

/// Paths filled with the panel's text colour: Plasma's by the `ColorScheme-Text` class of the
/// `current-color-scheme` stylesheet it rewrites, GTK's and GNOME Shell's by filling every path
/// of an icon named `-symbolic` (their `fill: … !important` beats the inline one); an image as
/// runs of pixels in `side` rows, one path per alpha level at its `fill-opacity`. `tint` is the
/// colour where Plasma's icon theme doesn't follow the colour scheme (`FollowsColorScheme`).
#[cfg(any(test, target_os = "linux"))]
pub fn symbolic(size: f32, paths: &[(String, bool, f32)], tint: [u8; 3]) -> String {
  let [r, g, b] = tint;
  let mut out = format!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}"><style id="current-color-scheme">.ColorScheme-Text {{ color:#{r:02x}{g:02x}{b:02x}; }}</style>"#
  );
  for (d, even_odd, opacity) in paths {
    let rule = if *even_odd { "evenodd" } else { "nonzero" };
    let d = d.replace(['<', '>', '"', '&'], "");
    out += &format!(
      r#"<path class="ColorScheme-Text" style="fill:currentColor" fill-rule="{rule}" fill-opacity="{opacity}" d="{d}"/>"#
    );
  }

  out + "</svg>"
}

/// The runs of `alpha` (`side` × `side`) as path data per alpha level, from the faintest.
#[cfg(any(test, target_os = "linux"))]
pub fn runs(alpha: &[u8], side: usize) -> Vec<(String, bool, f32)> {
  let level = |a: u8| ((a as u32 * LEVELS + 127) / 255) as usize;
  let mut ds = vec![String::new(); LEVELS as usize + 1];
  for (y, row) in alpha.chunks(side).enumerate() {
    let mut x = 0;
    while x < row.len() {
      let (l, start) = (level(row[x]), x);
      while x < row.len() && level(row[x]) == l {
        x += 1;
      }

      if l > 0 {
        ds[l] += &format!("M{start} {y}h{}v1H{start}z", x - start);
      }
    }
  }

  let paths = ds.into_iter().enumerate().filter(|(_, d)| !d.is_empty());
  paths.map(|(l, d)| (d, false, l as f32 / LEVELS as f32)).collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn fills_by_the_rule_with_smooth_edges() {
    let at = |a: &[u8], x: usize, y: usize| a[y * 24 + x];
    let ring = "M2 12a10 10 0 1 0 20 0a10 10 0 1 0-20 0ZM7 12a5 5 0 1 0 10 0a5 5 0 1 0-10 0Z";
    let a = fill(ring, 24.0, 24, true);
    assert_eq!((at(&a, 12, 12), at(&a, 12, 4), at(&a, 0, 0)), (0, 255, 0));
    assert!(a.iter().any(|&v| v > 0 && v < 255), "antialiased");
    let same_way = fill(ring, 24.0, 24, false);
    assert_eq!(at(&same_way, 12, 12), 255, "non-zero fills a hole drawn the same way");
    let half = fill("M0 0H1V2H0Z", 2.0, 2, false);
    assert_eq!(half, [255, 0, 255, 0]);
  }

  /// Path data goes in as it is; an image as runs of pixels by alpha level, recoloured by class.
  #[test]
  fn symbolic_svgs_fill_with_the_text_colour() {
    let svg = symbolic(18.0, &[("M0 0H9\"/>".into(), true, 1.0)], [255; 3]);
    assert!(svg.contains(r#"fill-rule="evenodd" fill-opacity="1" d="M0 0H9/"/>"#), "{svg}");
    assert!(
      svg.contains(r#"<style id="current-color-scheme">.ColorScheme-Text { color:#ffffff; }"#)
    );
    let alpha = [255, 255, 0, 128, 0, 0, 0, 0, 0];
    let r = runs(&alpha, 3);
    assert_eq!(r[0], ("M0 1h1v1H0z".into(), false, 0.5));
    assert_eq!(r[1], ("M0 0h2v1H0z".into(), false, 1.0));
    assert_eq!(r.len(), 2);
  }

  #[test]
  fn images_scale_by_averaging() {
    let px: Vec<u8> = (0..16).flat_map(|i| [0, 0, 0, if i % 2 == 0 { 255 } else { 0 }]).collect();
    assert_eq!(scale_alpha(4, 4, &px, 2), [127; 4]);
    assert_eq!(scale_alpha(4, 4, &px, 4)[..2], [255, 0]);
    assert_eq!(scale_alpha(1, 1, &[0, 0, 0, 200], 3), [200; 9]);
    assert_eq!(scale_alpha(4, 4, &px[..8], 2), [0; 4], "short");
  }
}
