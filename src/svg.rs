// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! SVG path data (`d`) into polylines: the reader the widget icons stroke
//! ([crate::ui::widgets::icon]) and the tray fills ([crate::tray::TrayIcon]). SVG's whole path
//! syntax (`M L H V C S Q T A Z`, absolute and relative, implicit repeats); curves and arcs in
//! pieces of about 2 pixels.

use crate::scene::Point;

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
    let (start, s) = (self.i, self.s);
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

/// Flattens SVG path data into polylines in view box units, one per subpath (a closed one ends
/// at its start); curves in pieces of about 2 pixels at `px` pixels per unit.
pub fn flatten(d: &str, px: f32, out: &mut Vec<Vec<Point>>) -> Result<(), String> {
  let mut c = Cur { s: d.as_bytes(), i: 0 };
  let (mut at, mut start) = (Point::new(0.0, 0.0), Point::new(0.0, 0.0));
  // The last control point, for S and T, and the command it came from.
  let (mut ctrl, mut last) = (at, b' ');
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
    let mirror = |kinds: [u8; 2]| match kinds.contains(&last) {
      true => Point::new(2.0 * at.x - ctrl.x, 2.0 * at.y - ctrl.y),
      false => at,
    };
    match cmd.to_ascii_uppercase() {
      b'M' => {
        finish(&mut cur, out);
        at = pt(&mut c)?;
        start = at;
        cur.push(at);
        // Pairs after the first are lines.
        (cmd, last) = (if rel { b'l' } else { b'L' }, b'M');
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
        let c1 = if u == b'C' { pt(&mut c)? } else { mirror(*b"CS") };
        let c2 = pt(&mut c)?;
        let end = pt(&mut c)?;
        cubic(&mut cur, [at, c1, c2, end], px);
        (ctrl, at) = (c2, end);
      }
      u @ (b'Q' | b'T') => {
        let q = if u == b'Q' { pt(&mut c)? } else { mirror(*b"QT") };
        let end = pt(&mut c)?;
        // A quadratic as the cubic it equals.
        let c1 = Point::new(at.x + 2.0 / 3.0 * (q.x - at.x), at.y + 2.0 / 3.0 * (q.y - at.y));
        let c2 = Point::new(end.x + 2.0 / 3.0 * (q.x - end.x), end.y + 2.0 / 3.0 * (q.y - end.y));
        cubic(&mut cur, [at, c1, c2, end], px);
        (ctrl, at) = (q, end);
      }
      b'A' => {
        let (rx, ry, rot) = (c.num()?, c.num()?, c.num()?);
        let (large, sweep) = (c.flag()?, c.flag()?);
        let end = pt(&mut c)?;
        arc(&mut cur, at, [rx, ry, rot], [large, sweep], end, px);
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
  match c.i < c.s.len() {
    true => Err(format!("unexpected {:?} at {}", c.s[c.i] as char, c.i)),
    false => Ok(()),
  }
}

/// Pieces for a curve whose control polygon is `len` units long: about 2 pixels each.
fn pieces(len: f32, px: f32) -> usize {
  ((len * px / 2.0).ceil() as usize).clamp(2, 64)
}

fn cubic(out: &mut Vec<Point>, [p0, p1, p2, p3]: [Point; 4], px: f32) {
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

/// An elliptical arc from `p0` to `p1` (radii and rotation, the large-arc and sweep flags), by
/// the endpoint-to-centre conversion of SVG 1.1
/// (<https://www.w3.org/TR/SVG11/implnote.html#ArcImplementationNotes>, F.6.5 and F.6.6).
fn arc(
  out: &mut Vec<Point>,
  p0: Point,
  [rx, ry, rot]: [f32; 3],
  [large, sweep]: [bool; 2],
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
  let t0 = ((y1 - cy1) / ry).atan2((x1 - cx1) / rx);
  let mut dt = ((-y1 - cy1) / ry).atan2((-x1 - cx1) / rx) - t0;
  let tau = std::f32::consts::TAU;
  if sweep && dt < 0.0 {
    dt += tau;
  } else if !sweep && dt > 0.0 {
    dt -= tau;
  }

  let n = pieces(dt.abs() * rx.max(ry), px);
  for k in 1..=n {
    let (s, c) = (t0 + dt * k as f32 / n as f32).sin_cos();
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
