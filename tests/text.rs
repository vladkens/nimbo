// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Harness scenario for `examples/text.rs`, in the dark and the light appearance, at scale 2 and
//! 1, with checks that hold for any system font: every line's ink inside its line box, wider
//! strings measuring wider, a monospaced font, a combining accent taking no room, the ink colour
//! reached, a clipped line cut at its panel, subpixel positions moving the ink a quarter pixel,
//! and the emoji in colour where the OS has a colour font. CJK is checked where a font has it.

#[path = "../examples/text.rs"]
#[allow(dead_code)]
mod text;

use nimbo::harness::Image;
use nimbo::{Appearance, Color, Event, Font, Harness, Rect, Weight};
use text::*;

/// A colour as the frame stores an opaque one.
fn px(c: Color) -> [u8; 4] {
  c.map(|v| (v * 255.0).round() as u8)
}

/// The largest difference of a channel.
fn diff(a: [u8; 4], b: [u8; 4]) -> u8 {
  (0..4).map(|i| a[i].abs_diff(b[i])).max().unwrap()
}

/// The pixels of `area` (in points) as (x, y, RGBA), in pixels.
fn pixels(img: &Image, area: Rect) -> impl Iterator<Item = (usize, usize, [u8; 4])> + '_ {
  let s = img.scale;
  let (x0, y0) = ((area.x * s).max(0.0) as usize, (area.y * s).max(0.0) as usize);
  let x1 = (((area.x + area.w) * s).ceil() as usize).min(img.pixels.w);
  let y1 = (((area.y + area.h) * s).ceil() as usize).min(img.pixels.h);
  (y0..y1).flat_map(move |y| {
    (x0..x1).map(move |x| {
      let i = (y * img.pixels.w + x) * 4;
      let [b, g, r, a] = img.pixels.data[i..i + 4].try_into().unwrap();
      (x, y, [r, g, b, a])
    })
  })
}

/// The box of the pixels in `area` that differ from the ground, in points: x0, y0, x1, y1.
fn ink_box(img: &Image, area: Rect, bg: [u8; 4]) -> Option<[f32; 4]> {
  let s = img.scale;
  pixels(img, area).filter(|p| diff(p.2, bg) > 8).fold(None, |b, (x, y, _)| {
    let (x, y) = (x as f32 / s, y as f32 / s);
    let [x0, y0, x1, y1] = b.unwrap_or([x, y, x, y]);
    Some([x0.min(x), y0.min(y), x1.max(x + 1.0 / s), y1.max(y + 1.0 / s)])
  })
}

/// The ink's mean x in pixels, weighted by how far each pixel is from the ground.
fn centroid(img: &Image, area: Rect, bg: [u8; 4]) -> f32 {
  let (sum, n) = pixels(img, area).fold((0.0, 0.0), |(sum, n), (x, _, c)| {
    let w = diff(c, bg) as f32;
    (sum + x as f32 * w, n + w)
  });
  sum / n
}

/// The line box of `s` at `x`: from the ascent above the baseline to the descent below, as
/// wide as its advance.
fn line_box(h: &mut Harness<Text>, x: f32, s: &Sample) -> Rect {
  let m = h.rt.text.metrics(s.font);
  let w = h.rt.text.measure(s.text, s.font);
  Rect::new(x, s.y - m.ascent, w, m.ascent + m.descent)
}

/// Asserts that `s` drew ink, all of it within its line box: a pixel's margin for antialiasing,
/// and a point more, where a glyph's ink may overhang its advance or an accent on a capital the
/// font's ascent. The area searched is larger, short of the lines around.
fn inside_its_box(h: &mut Harness<Text>, img: &Image, x: f32, s: &Sample, bg: [u8; 4]) {
  let b = line_box(h, x, s);
  let area = Rect::new(b.x - 6.0, b.y - 2.0, b.w + 12.0, b.h + 4.0);
  let ink = ink_box(img, area, bg).unwrap_or_else(|| panic!("no ink: {}", s.text));
  let m = 1.0 / img.scale + 1.01;
  let inside =
    ink[0] >= b.x - m && ink[1] >= b.y - m && ink[2] <= b.x + b.w + m && ink[3] <= b.y + b.h + m;
  assert!(inside, "{}: ink {ink:?} outside {b:?}", s.text);
}

/// Whether a font in the system has every glyph of `s` (no glyph 0, the missing one).
fn has_glyphs(h: &mut Harness<Text>, s: &Sample) -> bool {
  h.rt.text.layout(s.text, s.font).glyphs.iter().all(|g| g.id != 0)
}

fn check(h: &mut Harness<Text>, p: &Palette) {
  let img = h.render();
  let bg = px(p.bg);
  assert_eq!(img.pixel(4.0, 4.0), bg);

  // Sizes, weights, Latin and Cyrillic: in their line boxes.
  for s in STYLES.iter().chain(&SCRIPTS[..2]) {
    assert!(has_glyphs(h, s), "{}", s.text);
    inside_its_box(h, &img, X, s, bg);
  }

  // The circumflex on the Q, raised by the shaper, is drawn above the capital: ink over its
  // cap height.
  let marks = &SCRIPTS[1];
  let q = marks.text.find('Q').unwrap();
  let x0 = X + h.rt.text.measure(&marks.text[..q], marks.font);
  let cap = h.rt.text.metrics(marks.font).cap_height;
  let above = Rect::new(x0, marks.y - cap - 6.0, 16.0, 4.0);
  assert!(ink_box(&img, above, bg).is_some(), "no circumflex over the Q");

  // CJK where a font has it.
  let cjk = &SCRIPTS[3];
  match has_glyphs(h, cjk) {
    true => inside_its_box(h, &img, X, cjk, bg),
    false => eprintln!("no CJK font: skipped"),
  }

  // Fully covered pixels of the bold line reach the ink's colour.
  let bold = line_box(h, X, &STYLES[4]);
  let darkest = pixels(&img, bold).map(|(.., c)| diff(c, px(p.ink))).min().unwrap();
  assert!(darkest <= 12, "{darkest}");

  // The long line is wider than its panel and cut at it: ink near the panel's right edge, none
  // past it.
  let long = line_box(h, X, &LONG);
  assert!(long.w > PANEL.w + 40.0);
  let near = Rect::new(PANEL.x + PANEL.w - 30.0, long.y, 28.0, long.h);
  assert!(ink_box(&img, near, px(p.panel)).is_some());
  let past = Rect::new(PANEL.x + PANEL.w + 1.0, long.y, 60.0, long.h);
  assert_eq!(ink_box(&img, past, bg), None);

  // Each subpixel row is drawn a quarter pixel right of the row above, and its ink moves so;
  // measured from the baseline to the cap height, clear of the descenders of the row above.
  // DirectWrite moves light text on a dark ground unevenly (0.36, 0.44 and 0.09 pixels at scale
  // 2), so on Windows only dark text on the light ground is checked.
  let cap = h.rt.text.metrics(SUBPIXEL_FONT).cap_height;
  let rows: Vec<f32> = SUBPIXEL_Y
    .iter()
    .enumerate()
    .map(|(k, &y)| {
      let s = Sample { y, text: SUBPIXEL, font: SUBPIXEL_FONT };
      let b = line_box(h, subpixel_x(k, img.scale), &s);
      centroid(&img, Rect::new(b.x - 2.0, y - cap - 1.0, b.w + 4.0, cap + 1.5), bg)
    })
    .collect();
  let steps: Vec<f32> = rows.windows(2).map(|w| w[1] - w[0]).collect();
  // By value: each `&LIGHT` of a const may be its own address.
  if p.bg == LIGHT.bg || !cfg!(target_os = "windows") {
    assert!(steps.iter().all(|d| (0.1..0.4).contains(d)), "{steps:?}");
  }

  // The rocket in colour where the system has a colour emoji font: some pixel of it has
  // channels far apart.
  let line = h.rt.text.layout(SCRIPTS[2].text, SCRIPTS[2].font);
  let at = line.clusters.iter().position(|&c| c as usize == EMOJI_AT.len()).unwrap();
  let (g, next) = (line.glyphs[at], line.glyphs[at + 1]);
  // About an em wide, in any font: a colour one, or the missing glyph's box.
  let em = SCRIPTS[2].font.size;
  assert!((0.5 * em..2.0 * em).contains(&(next.x - g.x)), "{g:?} {next:?}");
  let color = h.rt.text.glyph(g.face, g.id, 0, [1.0; 4]).is_some_and(|a| a.color);
  if cfg!(target_os = "linux") && !color {
    eprintln!("no colour emoji font: skipped");
  } else {
    assert!(color, "the rocket is not a colour glyph");
    let b = line_box(h, X, &SCRIPTS[2]);
    let rocket = Rect::new(X + g.x, b.y - 2.0, next.x - g.x, b.h + 4.0);
    let colorful = pixels(&img, rocket).any(|(.., c)| c[0].abs_diff(c[2]) > 64);
    assert!(colorful, "no colour in the rocket");
  }

  assert!(!h.frame(), "a frame without changes");
}

/// Measures that hold for any system font.
fn measures(h: &mut Harness<Text>) {
  let t = &mut h.rt.text;
  let ui = Font::ui(13.0);
  assert!(t.measure("Hello, world", ui) > t.measure("Hello", ui));
  assert!(t.measure("Hello", ui.weight(Weight::Bold)) > t.measure("Hello", ui));
  assert!(t.measure("Hello", Font::ui(26.0)) > t.measure("Hello", ui) * 1.8);
  let mono = Font::mono(13.0);
  assert!((t.measure("iiii", mono) - t.measure("MMMM", mono)).abs() < 0.01);
  assert!(t.measure("iiii", ui) < t.measure("MMMM", ui));
  // A combining accent takes no room: e and U+0301 as wide as é.
  let (decomposed, composed) = (t.measure("cafe\u{301}", ui), t.measure("café", ui));
  assert!((decomposed - composed).abs() < 0.5, "{decomposed} vs {composed}");
  assert!(decomposed > t.measure("caf", ui));
  // Its glyphs start at the clusters of the text: the accent's cluster is the e's or its own.
  let line = t.layout("cafe\u{301}", ui);
  assert!(line.clusters.iter().all(|&c| "cafe\u{301}".is_char_boundary(c as usize)));
  // A mark on a capital is raised above the baseline, unlike the base.
  let q = t.layout("Q\u{302}", ui);
  assert_eq!(q.glyphs[0].y, 0.0);
  assert!(q.glyphs.len() == 1 || q.glyphs[1].y > 1.0, "{:?}", q.glyphs);
  let m = t.metrics(ui);
  assert!(m.ascent > 0.0 && m.descent > 0.0 && m.line_height() >= m.ascent + m.descent);
  assert!(m.cap_height > m.x_height && m.x_height > 0.0, "{m:?}");
}

fn main() {
  let mut h = Harness::new(Text::default(), SIZE, 2.0);
  measures(&mut h);
  check(&mut h, &DARK);

  assert!(h.event(Event::Appearance(Appearance::LIGHT)));
  check(&mut h, &LIGHT);

  let mut one = Harness::new(Text::default(), SIZE, 1.0);
  check(&mut one, &DARK);
  assert!(one.event(Event::Appearance(Appearance::LIGHT)));
  check(&mut one, &LIGHT);

  println!("text scenario: ok");
}
