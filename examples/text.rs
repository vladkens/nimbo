// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Text in the system fonts, in the light or the dark appearance: lines in a few sizes and
//! weights and the monospaced font, Latin and Cyrillic, combining accents (stacked, and on a
//! capital), emoji (in colour where the OS has a colour font), CJK where a font has it, a long line
//! clipped by its panel, and one word drawn four times, each a quarter pixel right of the one above
//! (subpixel positioning).
//!
//! `cargo run --release --example text` opens it in a window, which follows the OS's light or
//! dark setting; `-- --shot text.png [--appearance light] [--scale 2]` renders it without one.

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use nimbo::{App, Color, Config, Cx, Event, Font, Frame, Quad, Rect, Size, Weight, rgb};

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
      Event::Appearance(a) => self.light = !a.dark,
      _ => return false,
    }

    true
  }

  fn paint(&mut self, f: &mut Frame) {
    let p = self.palette();
    f.rect(Rect::new(0.0, 0.0, f.size.w, f.size.h), p.bg, 0.0);

    for s in STYLES.iter().chain(&SCRIPTS) {
      f.text(X, s.y, s.text, s.font, p.ink);
    }

    // The size of each style, right-aligned in a column.
    for s in &STYLES {
      f.text_at(SIZES_X, s.y, s.font.size, NOTE, p.dim, 1.0);
    }

    f.quad(Quad::new(PANEL, p.panel).radius(6.0).border(1.0, p.border));
    f.push_clip(Rect::new(PANEL.x + 1.0, PANEL.y + 1.0, PANEL.w - 2.0, PANEL.h - 2.0));
    f.text(X, LONG.y, LONG.text, LONG.font, p.ink);
    f.pop_clip();

    for (k, y) in SUBPIXEL_Y.into_iter().enumerate() {
      f.text(subpixel_x(k, f.scale), y, SUBPIXEL, SUBPIXEL_FONT, p.ink);
      f.text_at(SIZES_X, y, SUBPIXEL_NOTES[k], NOTE, p.dim, 1.0);
    }
  }
}

fn main() {
  nimbo::run(Config::new("text", SIZE.w, SIZE.h), Text::default(), |_| {});
}
