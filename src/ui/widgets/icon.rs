// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Stroke icons from SVG path data: the path flattened to polylines and drawn as round-capped
//! lines, so an icon is crisp at any size and scale and takes any colour (the theme's text,
//! muted or accent). The paths are in a 24 × 24 view box, as Lucide's (<https://lucide.dev>).
//!
//! ```ignore
//! ui.icon(Icon::INFO, 16.0, t.muted);
//! ui.with_id(id).canvas(s, move |f, r| icon::draw(f, Icon::RESET, r, color, 1.5));
//! if ui.icon_button(Icon::TRASH).clicked { self.ask = true }
//! ```
//!
//! The path syntax is SVG's whole (`M L H V C S Q T A Z`, absolute and relative, implicit
//! repeats); only strokes are drawn. Draw them in an opaque colour: the round joints of a
//! translucent one overlap darker. [Ui::icon] dims an icon in a disabled scope by mixing it
//! toward the ground instead. The built-in set is drawn for nimbo, thinner than Lucide's:
//! stroke them 1.6 units wide ([Ui::icon_stroke]).

use crate::scene::{Color, Frame, Point, Rect};
use crate::ui::{Response, Style, Ui, px};

/// SVG path data in a 24 × 24 view box, drawn as a stroke ([draw], [Ui::icon]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Icon(pub &'static str);

impl Icon {
  /// A gear of eight teeth.
  pub const GEAR: Self = Self(concat!(
    "M18.86 10.16L21.28 10.53L21.28 13.47L18.86 13.84L18.15 15.55L19.60 17.53L17.53 19.60",
    "L15.55 18.15L13.84 18.86L13.47 21.28L10.53 21.28L10.16 18.86L8.45 18.15L6.47 19.60",
    "L4.40 17.53L5.85 15.55L5.14 13.84L2.72 13.47L2.72 10.53L5.14 10.16L5.85 8.45L4.40 6.47",
    "L6.47 4.40L8.45 5.85L10.16 5.14L10.53 2.72L13.47 2.72L13.84 5.14L15.55 5.85L17.53 4.40",
    "L19.60 6.47L18.15 8.45Z",
    "M9 12a3 3 0 1 0 6 0a3 3 0 1 0-6 0"
  ));
  /// A circle halved: light and dark.
  pub const CONTRAST: Self = Self("M3.5 12a8.5 8.5 0 1 0 17 0a8.5 8.5 0 1 0-17 0M12 3.5v17");
  /// Two sliders.
  pub const SLIDERS: Self = Self("M4 7h9M17 7h3M15 4.8v4.4M4 17h3M11 17h9M9 14.8v4.4");
  /// A circled i.
  pub const INFO: Self = Self("M3.5 12a8.5 8.5 0 1 0 17 0a8.5 8.5 0 1 0-17 0M12 11v5.5M12 7.7v.1");
  /// A turning arrow: back to the default.
  pub const RESET: Self = Self("M5 12a7 7 0 1 0 2.1-5M5 4.5V8.5h4");
  /// A bin.
  pub const TRASH: Self = Self("M4.5 7h15M9.5 7V4.5h5V7M6.5 7l.9 12.5h9.2L17.5 7");
  /// A folder.
  pub const FOLDER: Self =
    Self("M3.5 7.5a2 2 0 0 1 2-2h3.6l2 2h7.4a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z");
  /// A page of text.
  pub const DOC: Self = Self("M7 3.5h7l4 4v13H7zM14 3.5v4h4M9.5 12.5h6M9.5 15.5h6M9.5 18.5h3.5");
  /// An arrow up and right: opens elsewhere.
  pub const EXTERNAL: Self = Self("M8 16 16 8M9.5 8H16v6.5");
  /// A check mark.
  pub const CHECK: Self = Self("m5.5 12.5 4 4 9-9.5");

  /// Every built-in icon, for a gallery and the tests.
  pub const ALL: [(&'static str, Self); 10] = [
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
  ];
}

/// The side of an icon's view box.
const VIEW: f32 = 24.0;

/// Draws `icon` into `r` (its view box stretched over it) in `color`, stroked `width` view box
/// units wide (Lucide's are 2). Data that doesn't parse draws as far as it parsed.
pub fn draw(f: &mut Frame, icon: Icon, r: Rect, color: Color, width: f32) {
  let (kx, ky) = (r.w / VIEW, r.h / VIEW);
  let mut lines = Vec::new();
  _ = flatten(icon.0, kx.max(ky) * f.scale, &mut lines);
  let stroke = width * kx.min(ky);
  let mut pts = Vec::new();
  for l in &lines {
    pts.clear();
    pts.extend(l.iter().map(|p| Point::new(r.x + p.x * kx, r.y + p.y * ky)));
    f.polyline(&pts, stroke, color);
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

// MARK: Path data

/// Reads path data: commands, numbers and arc flags.
struct Cur<'a> {
  s: &'a [u8],
  i: usize,
}

impl Cur<'_> {
  fn skip(&mut self) {
    while self.i < self.s.len() && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',')
    {
      self.i += 1;
    }
  }

  /// The next command letter, if one is next.
  fn command(&mut self) -> Option<u8> {
    self.skip();
    let c = *self.s.get(self.i)?;
    if c.is_ascii_alphabetic() && c != b'e' && c != b'E' {
      self.i += 1;
      return Some(c);
    }

    None
  }

  /// Whether a number comes next (an implicit repeat of the command).
  fn more(&mut self) -> bool {
    self.skip();
    self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.'))
  }

  fn num(&mut self) -> Result<f32, String> {
    self.skip();
    let start = self.i;
    let s = self.s;
    if self.i < s.len() && matches!(s[self.i], b'-' | b'+') {
      self.i += 1;
    }

    let mut dot = false;
    while self.i < s.len() {
      match s[self.i] {
        b'0'..=b'9' => {}
        b'.' if !dot => dot = true,
        b'e' | b'E' => {
          self.i += 1;
          if self.i < s.len() && matches!(s[self.i], b'-' | b'+') {
            self.i += 1;
          }

          continue;
        }
        _ => break,
      }

      self.i += 1;
    }

    let text = std::str::from_utf8(&s[start..self.i]).unwrap_or("");
    text.parse().map_err(|_| format!("a number at {start}: {text:?}"))
  }

  /// An arc's flag: one digit, which may touch the next number (`a1 1 0 01 2 2`).
  fn flag(&mut self) -> Result<bool, String> {
    self.skip();
    let on = match self.s.get(self.i) {
      Some(b'0') => false,
      Some(b'1') => true,
      _ => return Err(format!("an arc flag at {}", self.i)),
    };

    self.i += 1;
    Ok(on)
  }
}

/// Flattens path data into polylines in view box units, one per subpath (a closed one ends at
/// its start); curves in pieces of about 2 pixels at `px` pixels per unit.
pub fn flatten(d: &str, px: f32, out: &mut Vec<Vec<Point>>) -> Result<(), String> {
  let mut c = Cur { s: d.as_bytes(), i: 0 };
  let (mut at, mut start) = (Point::new(0.0, 0.0), Point::new(0.0, 0.0));
  // The last control point, for S and T, and the command it came from.
  let mut ctrl = at;
  let mut last = b' ';
  let mut cur: Vec<Point> = Vec::new();
  let mut cmd = c.command().ok_or("path data starts with a command")?;
  let finish = |cur: &mut Vec<Point>, out: &mut Vec<Vec<Point>>| {
    if cur.len() > 1 {
      out.push(std::mem::take(cur));
    }

    cur.clear();
  };
  loop {
    let rel = cmd.is_ascii_lowercase();
    let base = if rel { at } else { Point::new(0.0, 0.0) };
    let pt = |c: &mut Cur| -> Result<Point, String> {
      let x = c.num()?;
      let y = c.num()?;
      Ok(Point::new(base.x + x, base.y + y))
    };
    match cmd.to_ascii_uppercase() {
      b'M' => {
        finish(&mut cur, out);
        at = pt(&mut c)?;
        start = at;
        cur.push(at);
        // Pairs after the first are lines.
        cmd = if rel { b'l' } else { b'L' };
        last = b'M';
        if c.more() {
          continue;
        }
      }
      b'L' => {
        at = pt(&mut c)?;
        cur.push(at);
      }
      b'H' => {
        let x = c.num()?;
        at = Point::new(if rel { at.x + x } else { x }, at.y);
        cur.push(at);
      }
      b'V' => {
        let y = c.num()?;
        at = Point::new(at.x, if rel { at.y + y } else { y });
        cur.push(at);
      }
      u @ (b'C' | b'S') => {
        let c1 = match u {
          b'C' => pt(&mut c)?,
          _ if matches!(last, b'C' | b'S') => Point::new(2.0 * at.x - ctrl.x, 2.0 * at.y - ctrl.y),
          _ => at,
        };

        let c2 = pt(&mut c)?;
        let end = pt(&mut c)?;
        cubic(&mut cur, at, c1, c2, end, px);
        (ctrl, at) = (c2, end);
      }
      u @ (b'Q' | b'T') => {
        let q = match u {
          b'Q' => pt(&mut c)?,
          _ if matches!(last, b'Q' | b'T') => Point::new(2.0 * at.x - ctrl.x, 2.0 * at.y - ctrl.y),
          _ => at,
        };

        let end = pt(&mut c)?;
        // A quadratic as the cubic it equals.
        let c1 = Point::new(at.x + 2.0 / 3.0 * (q.x - at.x), at.y + 2.0 / 3.0 * (q.y - at.y));
        let c2 = Point::new(end.x + 2.0 / 3.0 * (q.x - end.x), end.y + 2.0 / 3.0 * (q.y - end.y));
        cubic(&mut cur, at, c1, c2, end, px);
        (ctrl, at) = (q, end);
      }
      b'A' => {
        let (rx, ry, rot) = (c.num()?, c.num()?, c.num()?);
        let (large, sweep) = (c.flag()?, c.flag()?);
        let end = pt(&mut c)?;
        arc(&mut cur, at, rx, ry, rot, large, sweep, end, px);
        at = end;
      }
      b'Z' => {
        if cur.last() != Some(&start) && !cur.is_empty() {
          cur.push(start);
        }

        finish(&mut cur, out);
        at = start;
        cur.push(at);
      }
      _ => return Err(format!("unknown command {:?}", cmd as char)),
    }

    last = cmd.to_ascii_uppercase();
    if last == b'Z' || !c.more() {
      match c.command() {
        Some(next) => cmd = next,
        None => break,
      }
    }
  }

  finish(&mut cur, out);
  if c.i < c.s.len() {
    return Err(format!("unexpected {:?} at {}", c.s[c.i] as char, c.i));
  }

  Ok(())
}

/// Pieces for a curve whose control polygon is `len` units long: about 2 pixels each.
fn pieces(len: f32, px: f32) -> usize {
  ((len * px / 2.0).ceil() as usize).clamp(2, 64)
}

fn cubic(out: &mut Vec<Point>, p0: Point, p1: Point, p2: Point, p3: Point, px: f32) {
  let d = |a: Point, b: Point| ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
  let n = pieces(d(p0, p1) + d(p1, p2) + d(p2, p3), px);
  for k in 1..=n {
    let t = k as f32 / n as f32;
    let u = 1.0 - t;
    let (a, b, c, e) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    let x = a * p0.x + b * p1.x + c * p2.x + e * p3.x;
    let y = a * p0.y + b * p1.y + c * p2.y + e * p3.y;
    out.push(Point::new(x, y));
  }
}

/// An elliptical arc from `p0` to `p1`, by the endpoint-to-centre conversion of SVG 1.1
/// (<https://www.w3.org/TR/SVG11/implnote.html#ArcImplementationNotes>, F.6.5 and F.6.6).
#[allow(clippy::too_many_arguments)]
fn arc(
  out: &mut Vec<Point>,
  p0: Point,
  rx: f32,
  ry: f32,
  rot: f32,
  large: bool,
  sweep: bool,
  p1: Point,
  px: f32,
) {
  let (mut rx, mut ry) = (rx.abs(), ry.abs());
  if p0 == p1 {
    return;
  }

  if rx == 0.0 || ry == 0.0 {
    out.push(p1);
    return;
  }

  let (sin, cos) = rot.to_radians().sin_cos();
  let (dx, dy) = ((p0.x - p1.x) / 2.0, (p0.y - p1.y) / 2.0);
  let (x1, y1) = (cos * dx + sin * dy, -sin * dx + cos * dy);
  // Radii too small for the chord grow to fit it.
  let l = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
  if l > 1.0 {
    (rx, ry) = (rx * l.sqrt(), ry * l.sqrt());
  }

  let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
  let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
  let k = (num / den).max(0.0).sqrt() * if large == sweep { -1.0 } else { 1.0 };
  let (cx1, cy1) = (k * rx * y1 / ry, -k * ry * x1 / rx);
  let (cx, cy) =
    (cos * cx1 - sin * cy1 + (p0.x + p1.x) / 2.0, sin * cx1 + cos * cy1 + (p0.y + p1.y) / 2.0);
  let angle = |ux: f32, uy: f32| uy.atan2(ux);
  let t0 = angle((x1 - cx1) / rx, (y1 - cy1) / ry);
  let mut dt = angle((-x1 - cx1) / rx, (-y1 - cy1) / ry) - t0;
  let tau = std::f32::consts::TAU;
  if sweep && dt < 0.0 {
    dt += tau;
  } else if !sweep && dt > 0.0 {
    dt -= tau;
  }

  let n = pieces(dt.abs() * rx.max(ry), px);
  for k in 1..=n {
    let t = t0 + dt * k as f32 / n as f32;
    let (s, c) = t.sin_cos();
    let (x, y) = (rx * c, ry * s);
    out.push(Point::new(cos * x - sin * y + cx, sin * x + cos * y + cy));
  }
  // The end exactly, not as the floats land.
  if let Some(p) = out.last_mut() {
    *p = p1;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn lines(d: &str) -> Vec<Vec<Point>> {
    let mut out = Vec::new();
    flatten(d, 2.0, &mut out).unwrap();
    out
  }

  fn near(a: Point, x: f32, y: f32) -> bool {
    (a.x - x).abs() < 1e-3 && (a.y - y).abs() < 1e-3
  }

  #[test]
  fn every_built_in_icon_parses_whole() {
    for (name, icon) in Icon::ALL {
      let mut out = Vec::new();
      flatten(icon.0, 2.0, &mut out).unwrap_or_else(|e| panic!("{name}: {e}"));
      assert!(!out.is_empty(), "{name}");
      for p in out.iter().flatten() {
        assert!((-0.5..=24.5).contains(&p.x) && (-0.5..=24.5).contains(&p.y), "{name}: {p:?}");
      }
    }
  }

  #[test]
  fn commands_absolute_relative_and_repeated() {
    let l = lines("M1 2 L3,4 5 6h1v-1H0V0z m2 2 1 1");
    assert_eq!(l.len(), 2);
    let a = &l[0];
    assert!(near(a[0], 1.0, 2.0) && near(a[2], 5.0, 6.0) && near(a[3], 6.0, 6.0));
    assert!(near(a[4], 6.0, 5.0) && near(a[6], 0.0, 0.0) && near(a[7], 1.0, 2.0));
    // After Z the pen is at the start: m2 2 is (3, 4), the repeat a line to (4, 5).
    assert!(near(l[1][0], 3.0, 4.0) && near(l[1][1], 4.0, 5.0));
    // Numbers touching: signs and dots split them.
    let l = lines("M1-2.5.5.5L-1e1 0");
    assert!(near(l[0][0], 1.0, -2.5) && near(l[0][1], 0.5, 0.5) && near(l[0][2], -10.0, 0.0));
    for bad in ["1 2", "M1", "M1 2 X3", "M1 2 A1 1 0 2 0 3 3"] {
      assert!(flatten(bad, 2.0, &mut Vec::new()).is_err(), "{bad}");
    }
  }

  #[test]
  fn arcs_and_curves_land_where_they_should() {
    // A half circle of radius 10 around (12, 12) through the bottom (sweep 0 from the left goes
    // down, y being down).
    let l = lines("M2 12a10 10 0 1 0 20 0");
    let pts = &l[0];
    assert!(near(*pts.last().unwrap(), 22.0, 12.0));
    for p in pts {
      let r = ((p.x - 12.0).powi(2) + (p.y - 12.0).powi(2)).sqrt();
      assert!((r - 10.0).abs() < 1e-3, "{p:?}");
      assert!(p.y >= 12.0 - 1e-3, "below the centre: {p:?}");
    }
    // A cubic ends at its end; S mirrors the last control point.
    let l = lines("M0 0C0 10 10 10 10 0S20-10 20 0");
    assert!(near(*l[0].last().unwrap(), 20.0, 0.0));
    let mid = l[0][l[0].len() * 3 / 4];
    assert!(mid.y < 0.0, "the mirrored half bows up: {mid:?}");
    // A chord longer than the radii allow: they grow to a half circle.
    let l = lines("M0 0A1 1 0 0 1 10 0");
    assert!(l[0].iter().any(|p| (p.y + 5.0).abs() < 0.1), "{:?}", l[0]);
  }
}
