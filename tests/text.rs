// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests through the harness of the app below, in the dark and the light appearance, at scale 2 and
//! 1, with checks that hold for any system font: every line's ink inside its line box, wider
//! strings measuring wider, a monospaced font, a combining accent taking no room, the ink colour
//! reached, a clipped line cut at its panel, subpixel positions moving the ink a quarter pixel, and
//! the emoji in colour where the OS has a colour font. CJK is checked where a font draws it.

#[expect(dead_code, reason = "what the tests don't read")]
mod app {

  //! Text in the system fonts, in the light or the dark appearance: lines in a few sizes and
  //! weights and the monospaced font, Latin and Cyrillic, combining accents (stacked, and on a
  //! capital), emoji (in colour where the OS has a colour font), CJK where a font has it, a long
  //! line clipped by its panel, and one word drawn four times, each a quarter pixel right of the
  //! one above (subpixel positioning).

  use nimbo::{App, Color, Cx, Event, Font, Frame, Quad, Rect, Size, Weight, rgb};

  pub const SIZE: Size = Size::new(520.0, 400.0);

  /// The colours of one appearance.
  pub struct Palette {
    pub bg: Color,
    pub panel: Color,
    pub border: Color,
    pub ink: Color,
    /// The size column and the notes.
    pub dim: Color,
  }

  pub const DARK: Palette = Palette {
    bg: rgb(0x1c1c1e),
    panel: rgb(0x2c2c2e),
    border: rgb(0x636366),
    ink: rgb(0xe5e5ea),
    dim: rgb(0x8e8e93),
  };

  pub const LIGHT: Palette = Palette {
    bg: rgb(0xf2f2f7),
    panel: rgb(0xffffff),
    border: rgb(0xaeaeb2),
    ink: rgb(0x1c1c1e),
    dim: rgb(0x6c6c70),
  };

  /// Where the lines start, right of the column of sizes that ends at [SIZES_X].
  pub const X: f32 = 64.0;
  pub const SIZES_X: f32 = 52.0;
  /// The size column's font.
  pub const NOTE: Font = Font::ui(11.0);

  /// A line of text: its baseline, the text and the font.
  pub struct Sample {
    pub y: f32,
    pub text: &'static str,
    pub font: Font,
  }

  const fn sample(y: f32, text: &'static str, font: Font) -> Sample {
    Sample { y, text, font }
  }

  const MEDIUM: Font = Font::ui(13.0).weight(Weight::Medium);

  /// Sizes and weights, then the monospaced font.
  pub const STYLES: [Sample; 6] = [
    sample(36.0, "Regular · The quick brown fox jumps over the lazy dog", Font::ui(11.0)),
    sample(60.0, "Regular · The quick brown fox jumps over the lazy dog", Font::ui(13.0)),
    sample(84.0, "Medium · The quick brown fox jumps over the lazy dog", MEDIUM),
    sample(110.0, "Semibold · Hamburgefonstiv 0123456789", Font::ui(15.0).weight(Weight::Semibold)),
    sample(144.0, "Bold · Hamburgefonstiv", Font::ui(24.0).weight(Weight::Bold)),
    sample(170.0, "Mono · let x = [0O, 1lI];", Font::mono(13.0)),
  ];

  /// Scripts: Latin and Cyrillic, combining accents (stacked, and on a capital), emoji, CJK.
  pub const SCRIPTS: [Sample; 4] = [
    sample(204.0, "Héllo, naïve · Привет, мир · Ёжик", Font::ui(15.0)),
    sample(230.0, "cafe\u{301} = café · e\u{301}\u{302} · Q\u{302}", Font::ui(15.0)),
    sample(258.0, "Emoji 🚀 🎨 👍🏽 ❤️", Font::ui(15.0)),
    sample(284.0, "漢字 かな カナ 한글", Font::ui(15.0)),
  ];

  /// The emoji line's rocket, which starts after this text.
  pub const EMOJI_AT: &str = "Emoji ";
  pub const ROCKET: &str = "🚀";

  /// A panel that clips a line longer than itself to the inside of its border.
  pub const PANEL: Rect = Rect::new(X - 8.0, 298.0, 240.0, 28.0);
  pub const LONG: Sample =
    sample(317.0, "A long line runs past the edge of its panel and is cut there", Font::ui(13.0));

  /// One word four times, each row a quarter pixel right of the one above.
  pub const SUBPIXEL: &str = "Subpixel lines";
  pub const SUBPIXEL_FONT: Font = Font::ui(13.0);
  pub const SUBPIXEL_Y: [f32; 4] = [342.0, 358.0, 374.0, 390.0];
  const SUBPIXEL_NOTES: [&str; 4] = ["+0 px", "+¼", "+½", "+¾"];

  #[derive(Default)]
  pub struct Text {
    /// The light appearance; dark by default.
    pub light: bool,
  }

  impl Text {
    pub fn palette(&self) -> &'static Palette {
      if self.light { &LIGHT } else { &DARK }
    }
  }

  /// The x of subpixel row `k` at `scale`: a quarter pixel per row.
  pub fn subpixel_x(k: usize, scale: f32) -> f32 {
    X + k as f32 * 0.25 / scale
  }

  impl App for Text {
    fn event(&mut self, _cx: &mut Cx, ev: &Event) -> bool {
      match ev {
        Event::Appearance(a) => self.light = !a.scheme.is_dark(),
        _ => return false,
      }

      true
    }

    fn paint(&mut self, f: &mut Frame) {
      let p = self.palette();
      f.rect(Rect::new(0.0, 0.0, f.size().w, f.size().h), p.bg, 0.0);

      for s in STYLES.iter().chain(&SCRIPTS) {
        f.text(X, s.y, s.text, s.font, p.ink);
      }

      // The size of each style, right-aligned in a column.
      for s in &STYLES {
        f.text_at(SIZES_X, s.y, s.font.get_size(), NOTE, p.dim, 1.0);
      }

      f.quad(Quad::new(PANEL, p.panel).radius(6.0).border(1.0, p.border));
      f.push_clip(Rect::new(PANEL.x + 1.0, PANEL.y + 1.0, PANEL.w - 2.0, PANEL.h - 2.0));
      f.text(X, LONG.y, LONG.text, LONG.font, p.ink);
      f.pop_clip();

      for (k, y) in SUBPIXEL_Y.into_iter().enumerate() {
        f.text(subpixel_x(k, f.scale()), y, SUBPIXEL, SUBPIXEL_FONT, p.ink);
        f.text_at(SIZES_X, y, SUBPIXEL_NOTES[k], NOTE, p.dim, 1.0);
      }
    }
  }
}

use app::*;
#[expect(dead_code, reason = "this file uses a part of the helpers")]
mod common;

use common::{diff, px};
use nimbo::harness::Image;
use nimbo::{Appearance, Event, Font, Harness, Rect, Weight};

/// The pixels of `area` (in points) as (x, y, RGBA), in pixels.
fn pixels(img: &Image, area: Rect) -> impl Iterator<Item = (usize, usize, [u8; 4])> + '_ {
  let (s, size) = (img.scale(), img.size());
  let (x0, y0) = ((area.x * s).max(0.0) as usize, (area.y * s).max(0.0) as usize);
  let x1 = ((area.x + area.w).min(size.w) * s).ceil() as usize;
  let y1 = ((area.y + area.h).min(size.h) * s).ceil() as usize;
  // Each pixel's centre, in points.
  let at = move |i: usize| (i as f32 + 0.5) / s;
  (y0..y1).flat_map(move |y| (x0..x1).map(move |x| (x, y, img.pixel(at(x), at(y)))))
}

/// The box of the pixels in `area` that differ from the ground, in points: x0, y0, x1, y1.
fn ink_box(img: &Image, area: Rect, bg: [u8; 4]) -> Option<[f32; 4]> {
  let s = img.scale();
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
  let m = h.metrics(s.font);
  let w = h.measure(s.text, s.font);
  Rect::new(x, s.y - m.ascent, w, m.ascent + m.descent)
}

/// Asserts that `s` drew ink, all of it within its line box: a pixel's margin for antialiasing,
/// and a point more, where a glyph's ink may overhang its advance or an accent on a capital the
/// font's ascent. The area searched is larger, short of the lines around.
fn inside_its_box(h: &mut Harness<Text>, img: &Image, x: f32, s: &Sample, bg: [u8; 4]) {
  let b = line_box(h, x, s);
  let area = Rect::new(b.x - 6.0, b.y - 2.0, b.w + 12.0, b.h + 4.0);
  let ink = ink_box(img, area, bg).unwrap_or_else(|| panic!("no ink: {}", s.text));
  let m = 1.0 / img.scale() + 1.01;
  let inside =
    ink[0] >= b.x - m && ink[1] >= b.y - m && ink[2] <= b.x + b.w + m && ink[3] <= b.y + b.h + m;
  assert!(inside, "{}: ink {ink:?} outside {b:?}", s.text);
}

fn check(h: &mut Harness<Text>, p: &Palette) {
  let img = h.render();
  let bg = px(p.bg);
  assert_eq!(img.pixel(4.0, 4.0), bg);

  // Sizes, weights, Latin and Cyrillic: in their line boxes.
  for s in STYLES.iter().chain(&SCRIPTS[..2]) {
    inside_its_box(h, &img, X, s, bg);
  }

  // The circumflex on the Q, raised by the shaper, is drawn above the capital: ink over its
  // cap height.
  let marks = &SCRIPTS[1];
  let q = marks.text.find('Q').unwrap();
  let x0 = X + h.measure(&marks.text[..q], marks.font);
  let cap = h.metrics(marks.font).cap_height;
  let above = Rect::new(x0, marks.y - cap - 6.0, 16.0, 4.0);
  assert!(ink_box(&img, above, bg).is_some(), "no circumflex over the Q");

  // CJK where a font draws it.
  let cjk = &SCRIPTS[3];
  match ink_box(&img, line_box(h, X, cjk), bg) {
    Some(_) => inside_its_box(h, &img, X, cjk, bg),
    None => eprintln!("no CJK font: skipped"),
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
  let cap = h.metrics(SUBPIXEL_FONT).cap_height;
  let rows: Vec<f32> = SUBPIXEL_Y
    .iter()
    .enumerate()
    .map(|(k, &y)| {
      let s = Sample { y, text: SUBPIXEL, font: SUBPIXEL_FONT };
      let b = line_box(h, subpixel_x(k, img.scale()), &s);
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
  let emoji = &SCRIPTS[2];
  let to_rocket = format!("{EMOJI_AT}{ROCKET}");
  let (x0, x1) = (h.measure(EMOJI_AT, emoji.font), h.measure(&to_rocket, emoji.font));

  // About an em wide, in any font: a colour one, or the missing glyph's box.
  let em = emoji.font.get_size();
  assert!((0.5 * em..2.0 * em).contains(&(x1 - x0)), "{x0} {x1}");
  let b = line_box(h, X, emoji);
  let rocket = Rect::new(X + x0, b.y - 2.0, x1 - x0, b.h + 4.0);
  let colorful = pixels(&img, rocket).any(|(.., c)| c[0].abs_diff(c[2]) > 64);
  match cfg!(target_os = "linux") && !colorful {
    true => eprintln!("no colour emoji font: skipped"),
    false => assert!(colorful, "no colour in the rocket"),
  }

  assert!(!h.frame(), "a frame without changes");
}

/// Measures that hold for any system font.
#[test]
fn measures_hold_for_any_font() {
  let t = &mut Harness::new(Text::default(), SIZE, 2.0);
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

  let m = t.metrics(ui);
  assert!(m.ascent > 0.0 && m.descent > 0.0 && m.line_height() >= m.ascent + m.descent);
  assert!(m.cap_height > m.x_height && m.x_height > 0.0, "{m:?}");
}

/// The text at `scale`, in the light appearance or the dark one.
fn text(scale: f32, light: bool) -> Harness<Text> {
  let mut h = Harness::new(Text::default(), SIZE, scale);
  if light {
    h.render();
    assert!(h.event(Event::Appearance(Appearance::LIGHT)));
  }

  h
}

#[test]
fn dark_at_scale_2() {
  check(&mut text(2.0, false), &DARK);
}

#[test]
fn light_at_scale_2() {
  check(&mut text(2.0, true), &LIGHT);
}

#[test]
fn dark_at_scale_1() {
  check(&mut text(1.0, false), &DARK);
}

#[test]
fn light_at_scale_1() {
  check(&mut text(1.0, true), &LIGHT);
}
