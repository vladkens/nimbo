// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Icons from SVG path data: the path flattened to polylines and drawn as round-capped lines,
//! or filled ([Icon::filled]), so an icon is crisp at any size and scale and takes any colour
//! (the theme's text, muted or accent). The paths are in a 24 × 24 view box, as Lucide's
//! (<https://lucide.dev>).
//!
//! ```ignore
//! ui.icon(Icon::INFO, 16.0, t.muted);
//! ui.with_id(id).canvas(s, move |f, r| icon::draw(f, Icon::RESET, r, color, 1.5));
//! if ui.icon_button(Icon::TRASH).clicked { self.ask = true }
//! ```
//!
//! The path syntax is SVG's whole (`M L H V C S Q T A Z`, absolute and relative, implicit
//! repeats). A filled icon fills its subpaths by the non-zero rule, SVG's default, its edges
//! smoothed by a hairline of its outline. Draw icons in an opaque colour: the round joints of a
//! translucent one overlap darker. [Ui::icon] dims an icon in a disabled scope by mixing it
//! toward the ground instead. The built-in set is drawn for nimbo, thinner than Lucide's:
//! stroke them 1.6 units wide ([Ui::icon_stroke]).

use crate::scene::{Color, Frame, Point, Rect};
use crate::svg::{flatten, spans};
use crate::ui::{Response, Style, Ui, px};

/// SVG path data in a 24 × 24 view box, drawn as a stroke or filled ([draw], [Ui::icon]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Icon {
  /// The path data.
  pub d: &'static str,
  /// Filled rather than stroked: a play triangle.
  pub fill: bool,
}

impl Icon {
  /// A stroked icon from path data `d`.
  pub const fn new(d: &'static str) -> Self {
    Self { d, fill: false }
  }

  /// A filled one.
  pub const fn filled(d: &'static str) -> Self {
    Self { d, fill: true }
  }

  /// A gear of eight teeth.
  pub const GEAR: Self = Self::new(concat!(
    "M18.86 10.16L21.28 10.53L21.28 13.47L18.86 13.84L18.15 15.55L19.60 17.53L17.53 19.60",
    "L15.55 18.15L13.84 18.86L13.47 21.28L10.53 21.28L10.16 18.86L8.45 18.15L6.47 19.60",
    "L4.40 17.53L5.85 15.55L5.14 13.84L2.72 13.47L2.72 10.53L5.14 10.16L5.85 8.45L4.40 6.47",
    "L6.47 4.40L8.45 5.85L10.16 5.14L10.53 2.72L13.47 2.72L13.84 5.14L15.55 5.85L17.53 4.40",
    "L19.60 6.47L18.15 8.45Z",
    "M9 12a3 3 0 1 0 6 0a3 3 0 1 0-6 0"
  ));
  /// A circle halved: light and dark.
  pub const CONTRAST: Self = Self::new("M3.5 12a8.5 8.5 0 1 0 17 0a8.5 8.5 0 1 0-17 0M12 3.5v17");
  /// Two sliders.
  pub const SLIDERS: Self = Self::new("M4 7h9M17 7h3M15 4.8v4.4M4 17h3M11 17h9M9 14.8v4.4");
  /// A circled i.
  pub const INFO: Self =
    Self::new("M3.5 12a8.5 8.5 0 1 0 17 0a8.5 8.5 0 1 0-17 0M12 11v5.5M12 7.7v.1");
  /// A turning arrow: back to the default.
  pub const RESET: Self = Self::new("M5 12a7 7 0 1 0 2.1-5M5 4.5V8.5h4");
  /// A bin.
  pub const TRASH: Self = Self::new("M4.5 7h15M9.5 7V4.5h5V7M6.5 7l.9 12.5h9.2L17.5 7");
  /// A folder.
  pub const FOLDER: Self = Self::new(
    "M3.5 7.5a2 2 0 0 1 2-2h3.6l2 2h7.4a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z",
  );
  /// A page of text.
  pub const DOC: Self =
    Self::new("M7 3.5h7l4 4v13H7zM14 3.5v4h4M9.5 12.5h6M9.5 15.5h6M9.5 18.5h3.5");
  /// An arrow up and right: opens elsewhere.
  pub const EXTERNAL: Self = Self::new("M8 16 16 8M9.5 8H16v6.5");
  /// A check mark.
  pub const CHECK: Self = Self::new("m5.5 12.5 4 4 9-9.5");
  /// A magnifier: search.
  pub const SEARCH: Self = Self::new("M4.5 10.5a6 6 0 1 0 12 0a6 6 0 1 0-12 0M15 15l4.5 4.5");
  /// A filled triangle with round corners: play.
  pub const PLAY: Self =
    Self::filled("M8 5.8v12.4a.8.8 0 0 0 1.2.7l10-6.2a.8.8 0 0 0 0-1.4l-10-6.2A.8.8 0 0 0 8 5.8z");

  /// Every built-in icon, for a gallery and the tests.
  pub const ALL: [(&'static str, Self); 12] = [
    ("gear", Self::GEAR),
    ("contrast", Self::CONTRAST),
    ("sliders", Self::SLIDERS),
    ("info", Self::INFO),
    ("reset", Self::RESET),
    ("trash", Self::TRASH),
    ("folder", Self::FOLDER),
    ("doc", Self::DOC),
    ("external", Self::EXTERNAL),
    ("check", Self::CHECK),
    ("search", Self::SEARCH),
    ("play", Self::PLAY),
  ];
}

/// The side of an icon's view box.
const VIEW: f32 = 24.0;

/// Draws `icon` into `r` (its view box stretched over it) in `color`, stroked `width` view box
/// units wide (Lucide's are 2), or filled. Data that doesn't parse draws as far as it parsed.
pub fn draw(f: &mut Frame, icon: Icon, r: Rect, color: Color, width: f32) {
  let (kx, ky) = (r.w / VIEW, r.h / VIEW);
  let mut lines = Vec::new();
  _ = flatten(icon.d, kx.max(ky) * f.scale, &mut lines);
  for p in lines.iter_mut().flatten() {
    *p = Point::new(r.x + p.x * kx, r.y + p.y * ky);
  }

  if icon.fill {
    fill(f, &lines, color);
  }

  // A filled icon's outline a pixel wide, over the fill's edges.
  let stroke = if icon.fill { 1.0 / f.scale } else { width * kx.min(ky) };
  for l in &lines {
    f.polyline(l, stroke, color);
  }
}

/// Fills the closed polylines `polys` (in points) non-zero: a rect a pixel high for each span
/// of each pixel row.
fn fill(f: &mut Frame, polys: &[Vec<Point>], color: Color) {
  let pts = polys.iter().flatten();
  let (top, bottom) = pts.fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
  let row = 1.0 / f.scale;
  let mut y = (top * f.scale).floor() * row;
  let mut xs = Vec::new();
  while y < bottom {
    let r = |x0: f32, x1: f32| Rect::new(x0, y, x1 - x0, row);
    spans(polys, y + row / 2.0, false, &mut xs, |x0, x1| f.rect(r(x0, x1), color, 0.0));
    y += row;
  }
}

impl Ui<'_> {
  /// `icon` `size` points square in `color`, stroked as Lucide draws it at 24 points (thinner
  /// below: 1.5 points at 18). Dimmed toward the ground in a disabled scope
  /// ([Ui::disabled_scope]).
  pub fn icon(&mut self, icon: Icon, size: f32, color: Color) -> Response {
    self.icon_stroke(icon, size, color, 2.0)
  }

  /// [Ui::icon] stroked `width` view box units wide: 1.6 for the built-in ones. Disabled
  /// ([Ui::disabled], or in a disabled scope), dimmed toward the ground.
  pub fn icon_stroke(&mut self, icon: Icon, size: f32, color: Color, width: f32) -> Response {
    let off = self.take_disabled();
    let color = self.dim(color, off);
    let s = Style::new().w(px(size)).h(px(size));
    self.canvas(s, move |f, r| draw(f, icon, r, color, width))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn every_built_in_icon_parses_whole() {
    for (name, icon) in Icon::ALL {
      let mut out = Vec::new();
      flatten(icon.d, 2.0, &mut out).unwrap_or_else(|e| panic!("{name}: {e}"));
      assert!(!out.is_empty(), "{name}");
      for p in out.iter().flatten() {
        assert!((-0.5..=24.5).contains(&p.x) && (-0.5..=24.5).contains(&p.y), "{name}: {p:?}");
      }
    }
  }

  /// Play, filled, and the same path stroked.
  struct Icons;

  impl crate::app::App for Icons {
    fn view(&mut self, ui: &mut Ui) {
      let row = Style::new().w(crate::ui::grow()).h(crate::ui::grow()).bg([1.0; 4]);
      ui.row(row, |ui| {
        ui.icon(Icon::PLAY, 48.0, [0.0, 0.0, 0.0, 1.0]);
        ui.icon(Icon::new(Icon::PLAY.d), 48.0, [0.0, 0.0, 0.0, 1.0]);
      });
    }
  }

  #[test]
  fn a_filled_icon_fills_inside_its_outline() {
    let mut h = crate::Harness::new(Icons, crate::Size::new(96.0, 48.0), 2.0);
    h.frame();
    let img = h.render();
    // The triangle's middle (12.5, 12 in its view box), its tip, and outside it.
    let at = |x: f32, y: f32| img.pixel(x * 2.0, y * 2.0);
    assert_eq!(at(12.5, 12.0), [0, 0, 0, 255], "filled");
    assert_eq!(at(18.5, 12.0), [0, 0, 0, 255], "to its tip");
    assert_eq!(at(5.0, 12.0), [255; 4], "outside");
    assert_eq!(at(24.0 + 12.5, 12.0), [255; 4], "stroked, hollow");
    assert_eq!(at(24.0 + 8.0, 12.0), [0, 0, 0, 255], "stroked, its edge");
  }

  /// A square in a square drawn the same way, its inner edges between pixels.
  struct Nested;

  impl crate::app::App for Nested {
    fn view(&mut self, ui: &mut Ui) {
      let row = Style::new().w(crate::ui::grow()).h(crate::ui::grow()).bg([1.0; 4]);
      let icon = Icon::filled("M2 2H22V22H2ZM6.3 6.3H17.7V17.7H6.3Z");
      ui.row(row, |ui| _ = ui.icon(icon, 24.0, [0.0, 0.0, 0.0, 1.0]));
    }
  }

  #[test]
  fn a_compound_fill_has_no_seams_inside() {
    let mut h = crate::Harness::new(Nested, crate::Size::new(24.0, 24.0), 1.0);
    h.frame();
    let img = h.render();
    for x in 3..21 {
      assert_eq!(img.pixel(x as f32 + 0.5, 12.5), [0, 0, 0, 255], "at {x}");
    }
  }
}
