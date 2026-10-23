// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! SVG path data (`d`) into polylines: the reader the widget icons stroke or fill
//! ([crate::ui::widgets::icon]) and the tray fills ([crate::tray::TrayIcon]), and the spans a
//! scanline fills ([spans]). SVG's whole path syntax (`M L H V C S Q T A Z`, absolute and
//! relative, implicit repeats); curves and arcs in pieces of about 2 pixels. [document] reads an
//! icon file's `<path>`s into one path.

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

/// Calls `span` with each stretch, left to right, of the horizontal line at `y` inside the
/// closed polylines `polys`, by the non-zero rule or `even_odd`; `xs` is scratch for the
/// crossings. What the tray icons and the filled widget icons fill.
pub fn spans(
  polys: &[Vec<Point>],
  y: f32,
  even_odd: bool,
  xs: &mut Vec<(f32, i32)>,
  mut span: impl FnMut(f32, f32),
) {
  xs.clear();
  for poly in polys {
    for (a, b) in poly.iter().zip(poly.iter().cycle().skip(1)) {
      if (a.y <= y) != (b.y <= y) {
        let x = a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x);
        xs.push((x, if b.y > a.y { 1 } else { -1 }));
      }
    }
  }

  xs.sort_by(|a, b| a.0.total_cmp(&b.0));
  // A span runs from where the line goes in to where it comes out, whole across the edges
  // inside it.
  let inside = |wind: i32| if even_odd { wind % 2 != 0 } else { wind != 0 };
  let (mut wind, mut start, mut was) = (0, 0.0, false);
  for (i, &(x, dir)) in xs.iter().enumerate() {
    wind += dir;
    // Crossings at one x count together: where two subpaths touch, a span goes on.
    if xs.get(i + 1).is_some_and(|n| n.0 == x) {
      continue;
    }

    let now = inside(wind);
    match (was, now) {
      (false, true) => start = x,
      (true, false) => span(start, x),
      _ => {}
    }

    was = now;
  }
}

/// An icon file's `<path>` elements joined into one path: its view box's side, the path data and
/// whether it fills even-odd. What an icon file has, and nothing else: a square `viewBox` from
/// `0 0`, and paths filled by one rule (`fill-rule="evenodd"`, on each or a group around them,
/// or none) that don't overlap (joined, they fill as one path); groups, titles and comments are
/// passed over. Any other element, a `transform` or `style`, an unfilled path (`fill="none"` on
/// it or a group around it) or data that doesn't parse is an error.
pub fn document(svg: &str) -> Result<(f32, String, bool), String> {
  let (mut size, mut paths, mut rules) = (None, Vec::new(), Vec::new());
  // Each open element's fill and fill rule, which the elements in it inherit.
  let mut fills = vec![(true, false)];
  let mut rest = svg;
  while let Some(at) = rest.find('<') {
    rest = &rest[at + 1..];
    if let Some(comment) = rest.strip_prefix("!--") {
      rest = &comment[comment.find("-->").ok_or("an unclosed comment")? + 3..];
      continue;
    }

    let end = rest.find('>').filter(|&e| !rest[..e].contains('<')).ok_or("an unclosed tag")?;
    let tag = &rest[..end];
    rest = &rest[end + 1..];
    if tag.starts_with(['?', '!']) {
      continue;
    }

    if let Some(name) = tag.strip_prefix('/') {
      if matches!(name.trim(), "svg" | "g") && fills.len() > 1 {
        fills.pop();
      }

      continue;
    }

    let (name, attrs) = attributes(tag)?;
    let get = |k: &str| attrs.iter().find(|a| a.0 == k).map(|a| a.1);
    if let Some(k) = ["transform", "style"].into_iter().find(|k| get(k).is_some()) {
      return Err(format!("<{name}> has a {k}"));
    }

    let (filled, even_odd) = *fills.last().unwrap_or(&(true, false));
    let fill = (
      get("fill").map_or(filled, |f| f != "none"),
      get("fill-rule").map_or(even_odd, |r| r == "evenodd"),
    );
    match name {
      "title" | "desc" => {}
      "svg" | "g" => {
        if name == "svg" {
          let view = get("viewBox").ok_or("no viewBox")?;
          let v: Vec<f32> = view.split([' ', ',']).filter_map(|n| n.parse().ok()).collect();
          size = match v[..] {
            [0.0, 0.0, w, h] if w == h && w > 0.0 => Some(w),
            _ => return Err(format!("a viewBox not square from 0 0: {view}")),
          };
        }

        if !tag.ends_with('/') {
          fills.push(fill);
        }
      }
      "path" => {
        let d = get("d").ok_or("a <path> without d")?;
        if !fill.0 {
          return Err("a <path> not filled".into());
        }

        flatten(d, 1.0, &mut Vec::new())?;
        // A path's first `m` is from the origin, not from where the path before it ended.
        paths.push(if d.trim_start().starts_with('m') { format!("M0 0{d}") } else { d.into() });
        rules.push(fill.1);
      }
      _ => return Err(format!("<{name}> is not read")),
    }
  }

  let size = size.ok_or("no <svg>")?;
  let even_odd = rules.first().copied().ok_or("no <path>")?;
  if rules.iter().any(|&r| r != even_odd) {
    return Err("paths filled by different rules".into());
  }

  Ok((size, paths.join(" "), even_odd))
}

/// An element's attributes, `(name, value)`.
type Attrs<'a> = Vec<(&'a str, &'a str)>;

/// A start tag's name and its attributes, `name = "value"` with either quote and any spaces.
fn attributes(tag: &str) -> Result<(&str, Attrs<'_>), String> {
  let tag = tag.strip_suffix('/').unwrap_or(tag);
  let word = |s: &str| s.find(|c: char| c.is_whitespace() || c == '=').unwrap_or(s.len());
  let (name, mut rest) = tag.split_at(word(tag));
  let mut out = Vec::new();
  loop {
    rest = rest.trim_start();
    if rest.is_empty() {
      return Ok((name, out));
    }

    let (key, after) = rest.split_at(word(rest));
    let after = after.trim_start().strip_prefix('=').map(str::trim_start);
    let quote = after.and_then(|a| a.chars().next()).filter(|q| matches!(q, '"' | '\''));
    let (Some(after), Some(q)) = (after, quote) else {
      return Err(format!("<{name}>: an attribute {key} without a quoted value"));
    };
    let value = &after[1..];
    let len = value.find(q).ok_or_else(|| format!("<{name}>: {key} is not closed"))?;
    out.push((key, &value[..len]));
    rest = &value[len + 1..];
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// An icon file's paths joined; what isn't an icon's is refused.
  #[test]
  fn icon_files() {
    let svg = r#"<?xml version="1.0"?><!-- a > b --><svg xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 18 18"><title>w</title><g><path fill-rule="evenodd" d="M1 1h4v4H1Z"/>
      <path d='M6 6h4v4H6Z' fill-rule='evenodd'></path></g></svg>"#;
    assert_eq!(document(svg).unwrap(), (18.0, "M1 1h4v4H1Z M6 6h4v4H6Z".into(), true));
    let one = |path: &str| format!(r#"<svg viewBox="0,0,24,24">{path}</svg>"#);
    assert!(!document(&one(r#"<path id="a" d="M1 1h2"/>"#)).unwrap().2);

    // Spaces around `=`; fill and rule inherited from a group; a relative first move from the
    // origin.
    let svg = r#"<svg viewBox = '0 0 24 24' fill="none"><g fill-rule="evenodd" fill="black">
      <path d="M1 1h2v2H1Z"/><path d = "m5 5h2v2h-2z"/></g></svg>"#;
    let (_, d, even_odd) = document(svg).unwrap();
    assert_eq!((d.as_str(), even_odd), ("M1 1h2v2H1Z M0 0m5 5h2v2h-2z", true));
    let mut out = Vec::new();
    flatten(&d, 1.0, &mut out).unwrap();
    assert!(near(out[1][0], 5.0, 5.0), "{:?}", out[1][0]);

    for (bad, why) in [
      (one(""), "no <path>"),
      (one(r#"<path d="M1 1h2" fill-rule="evenodd"/><path d="M1 1h2"/>"#), "different rules"),
      (one(r#"<circle r="2"/>"#), "<circle> is not read"),
      (one(r#"<g transform="scale(2)"><path d="M1 1h2"/></g>"#), "transform"),
      (one(r#"<path d="M1 1h2" fill="none"/>"#), "not filled"),
      (r#"<svg viewBox="0 0 9 9" fill="none"><path d="M1 1h2"/></svg>"#.into(), "not filled"),
      (one(r#"<g fill="none"><g><path d="M1 1h2"/></g></g>"#), "not filled"),
      (one(r#"<path transform = "scale(2)" d="M1 1h2"/>"#), "transform"),
      (one(r#"<path style="fill:none" d="M1 1h2"/>"#), "style"),
      (one(r#"<path d=M1/>"#), "quoted"),
      (one(r#"<path d="M1 1Q"/>"#), ""),
      (one(r#"<path fill="red"/>"#), "without d"),
      (r#"<svg viewBox="0 0 24 18"><path d="M1 1h2"/></svg>"#.into(), "not square"),
      (r#"<svg viewBox="2 0 24 24"><path d="M1 1h2"/></svg>"#.into(), "not square"),
      (r#"<path d="M1 1h2"/>"#.into(), "no <svg>"),
      (one("<path d=\"M1 1h2\""), "unclosed"),
    ] {
      let e = document(&bad).unwrap_err();
      assert!(e.contains(why), "{bad}: {e}");
    }
  }

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
  fn spans_are_whole_by_the_rule() {
    // A square in a square, drawn the same way: one span non-zero, two even-odd.
    let squares = lines("M0 0H10V10H0ZM2 2H8V8H2Z");
    let mut xs = Vec::new();
    let mut got = Vec::new();
    spans(&squares, 5.0, false, &mut xs, |a, b| got.push((a, b)));
    assert_eq!(got, [(0.0, 10.0)]);
    got.clear();
    spans(&squares, 5.0, true, &mut xs, |a, b| got.push((a, b)));
    assert_eq!(got, [(0.0, 2.0), (8.0, 10.0)]);
    // Two squares side by side, touching: one span.
    got.clear();
    spans(&lines("M0 0H5V10H0ZM5 0H10V10H5Z"), 5.0, false, &mut xs, |a, b| got.push((a, b)));
    assert_eq!(got, [(0.0, 10.0)]);
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
