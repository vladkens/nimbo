// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The layout's text: a text's preferred and least widths, its lines wrapped at its final width
//! or cut to fit with an ellipsis, an editable one's caret stops, and what of them is kept
//! across frames per font and text ([Kept]). A styled text whose runs change its font ([Span])
//! measures each piece in its run's font and wraps as one text.

use std::hash::{Hash, Hasher};

use super::{
  ELLIPSIS, EPS, Elide, Kind, Line, Measure, Stop, TextLines, TextStyle, Tree, TruncationMode,
};
use crate::scene::Size;
use crate::text::edit::graphemes;
use crate::text::{Font, FxHasher, Ink, Memo, Metrics};
use crate::ui::Run;
use crate::ui::attributed::pieces;

/// What the layout keeps across frames per font and text ([Memo]): a wrapping text's least
/// width, the lines of each line of text by the width it wraps at, and an editable line's caret
/// stops, offsets from its start: a megabyte pasted is measured once, not every frame.
#[derive(Default)]
pub(crate) struct Kept {
  pub least: Memo<f32>,
  pub lines: Memo<Vec<Line>>,
  pub stops: Memo<Vec<Stop>>,
}

/// A text element's text in its style and the runs that change its font, by which its pieces
/// measure: each in its run's font, the text in one font as a whole.
#[derive(Clone, Copy)]
pub struct Span<'t> {
  pub s: &'t str,
  pub style: TextStyle,
  /// Its runs (ends from its start) when one of them changes the font, else none.
  runs: &'t [Run],
}

impl<'t> Span<'t> {
  /// Text in one font.
  pub fn plain(s: &'t str, style: TextStyle) -> Self {
    Self { s, style, runs: &[] }
  }

  /// Text styled by `runs`, which matter here only when one changes the font.
  pub fn new(s: &'t str, style: TextStyle, runs: &'t [Run]) -> Self {
    let base = style.font;
    let runs = if runs.iter().any(|r| r.attrs.font(base) != base) { runs } else { &[] };
    Self { s, style, runs }
  }

  /// Its pieces in `a..b`: each one's range and font.
  fn pieces(&self, a: usize, b: usize) -> impl Iterator<Item = (usize, usize, Font)> + '_ {
    let base = self.style.font;
    pieces(self.runs, (a, b)).map(move |(a, b, at)| (a, b, at.font(base)))
  }

  /// The advance width of `a..b`.
  pub fn width(&self, m: &mut dyn Measure, a: usize, b: usize) -> f32 {
    if self.runs.is_empty() {
      return m.width(&self.s[a..b], self.style.font);
    }

    self.pieces(a, b).map(|(a, b, f)| m.width(&self.s[a..b], f)).sum()
  }

  /// `a..b` as a line: its advance width and how far its ink reaches ([Measure::line]).
  pub fn line(&self, m: &mut dyn Measure, a: usize, b: usize) -> (f32, Ink) {
    if self.runs.is_empty() {
      return m.line(&self.s[a..b], self.style.font);
    }

    // How far right of the line's start the ink reaches.
    let (mut x, mut ink, mut reach) = (0.0, Ink::default(), 0.0f32);
    for (a, b, f) in self.pieces(a, b) {
      let (w, i) = m.line(&self.s[a..b], f);
      (ink.left, ink.up, ink.down) =
        (ink.left.max(i.left - x), ink.up.max(i.up), ink.down.max(i.down));
      reach = reach.max(x + w + i.right);
      x += w;
    }

    (x, Ink { right: (reach - x).max(0.0), ..ink })
  }

  /// The metrics its lines take: its font's, as tall as the tallest of its runs' fonts.
  pub fn metrics(&self, m: &mut dyn Measure) -> Metrics {
    let mut met = m.metrics(self.style.font);
    for r in self.runs {
      let k = m.metrics(r.attrs.font(self.style.font));
      (met.ascent, met.descent) = (met.ascent.max(k.ascent), met.descent.max(k.descent));
      met.leading = met.leading.max(k.leading);
    }

    met
  }

  /// Pushes the caret stops of line `a..b` ([Measure::stops]), offsets from the text's start;
  /// a piece's stops go on from where the one before it ends.
  pub fn stops(&self, m: &mut dyn Measure, a: usize, b: usize, out: &mut Vec<Stop>) {
    if self.runs.is_empty() {
      return m.stops(&self.s[a..b], self.style.font, a as u32, out);
    }

    let (mut x, from) = (0.0, out.len());
    for (pa, pb, f) in self.pieces(a, b) {
      // The stop where it starts is the last one's end.
      let at = out.len();
      m.stops(&self.s[pa..pb], f, pa as u32, out);
      let w = out.last().map_or(0.0, |s| s.x);
      out[at..].iter_mut().for_each(|s| s.x += x);
      if at > from {
        out.remove(at);
      }

      x += w;
    }

    if out.len() == from {
      out.push(Stop { at: a as u32, x: 0.0 });
    }
  }

  /// What its line `a..b` is kept under besides its text and font, from `key`: `key` itself in
  /// one font; else hashed with its pieces' ends and fonts (a collision of 64 bits aside).
  fn key(&self, a: usize, b: usize, key: u64) -> u64 {
    if self.runs.is_empty() {
      return key;
    }

    let mut h = FxHasher::default();
    key.hash(&mut h);
    for (_, end, f) in self.pieces(a, b) {
      (end - a, f.size.to_bits(), f.weight as u8, f.design, f.italic).hash(&mut h);
    }

    h.finish() | 1 << 63
  }
}

/// The preferred width of a text (its widest line) and its least: when it wraps, its widest
/// character (grapheme), as a word that doesn't fit a line breaks between characters
/// ([wrap_text]); measured once per line of text and kept.
pub fn text_width(sp: Span, m: &mut dyn Measure, kept: &mut Kept) -> (f32, f32) {
  let (s, style) = (sp.s, sp.style);
  if matches!(style.lines, TextLines::Limit(1, _)) && !s.contains('\n') {
    let w = sp.width(m, 0, s.len());
    let dots = m.width(ELLIPSIS, sp.pieces(0, s.len()).next().map_or(style.font, |p| p.2));
    return (w, w.min(dots));
  }

  let (mut pref, mut least, mut at) = (0.0f32, 0.0f32, 0);
  for line in s.split('\n') {
    let (a, b) = (at, at + line.len());
    at = b + 1;
    let w = sp.width(m, a, b);
    pref = pref.max(w);
    if !wraps(style) {
      least = least.max(w);
      continue;
    }

    // Every word's: a narrow word can hold the widest one ("iiii W").
    let widest = || {
      let (mut w, mut pos) = (0.0f32, a);
      for word in line.split(' ') {
        for g in graphemes(word) {
          w = w.max(sp.width(m, pos + g.start, pos + g.end));
        }

        pos += word.len() + 1;
      }

      w
    };
    least = least.max(*kept.least.get(style.font, line, sp.key(a, b, 0), widest));
  }

  (pref, least)
}

/// Wraps every text element at its final width and sets its line metrics; editable ones get
/// lines that cover their text and caret stops.
pub fn wrap(t: &mut Tree, m: &mut dyn Measure, kept: &mut Kept) {
  t.lines.clear();
  t.stops.clear();
  let mut edits = t.edits.iter_mut().peekable();
  let mut styled = t.styled.iter().peekable();
  for (i, n) in t.nodes.iter_mut().enumerate() {
    // Text is measured in `f32`: a line is never that long.
    let (width, wide) = (n.size[0] as f32, n.content[0] as f32);
    let Kind::Text { range, style, lines, baseline, line_h } = &mut n.kind else { continue };
    let runs = match styled.next_if(|s| s.0 == i as u32) {
      Some(&(_, a, b)) => &t.runs[a as usize..b as usize],
      None => &[],
    };

    let s = &t.text[range.0 as usize..range.1 as usize];
    let sp = Span::new(s, *style, runs);
    let met = sp.metrics(m);
    *line_h = style.line_h.unwrap_or_else(|| met.line_height());
    *baseline = (*line_h - met.ascent - met.descent) / 2.0 + met.ascent;
    let start = t.lines.len() as u32;
    let edit = edits.next_if(|e| e.node == i as u32).filter(|e| !e.placeholder);

    // One line that fits (or doesn't wrap): its width is the one measured for the fit.
    let fits = !wraps(*style) || wide <= width + EPS;
    let limit = match style.lines {
      TextLines::Limit(n, mode) if edit.is_none() => Some((n as usize, mode)),
      _ => None,
    };

    let cut = limit.filter(|_| wide > width + EPS);
    let (start_at, end, cover) = (range.0, range.1, edit.is_some());
    match fits && !s.contains('\n') {
      // Paint draws the ellipsis where the line says.
      true if cut.is_some() => {
        let mode = cut.map_or(TruncationMode::Tail, |c| c.1);
        let l = elide(sp, (0, s.len()), width, mode, m);
        let l = Line { start: start_at + l.start, end: start_at + l.end, ..l };
        let elide = match l.elide {
          Elide::Middle(a, b) => Elide::Middle(start_at + a, start_at + b),
          e => e,
        };

        t.lines.push(Line { elide, ..l })
      }
      true => {
        let ink = sp.line(m, 0, s.len()).1;
        let elide = Elide::None;
        t.lines.push(Line { start: start_at, end, width: wide, ink, stop: 0, elide })
      }
      false => {
        wrap_text(sp, start_at, width, cover, m, &mut kept.lines, &mut t.lines);
        // Past the limit, the lines go and the last kept one ends in "…".
        if let Some((n, _)) = limit
          && t.lines.len() - start as usize > n.max(1)
        {
          t.lines.truncate(start as usize + n.max(1));
          let last = t.lines.last_mut().unwrap();
          let a = (last.start - start_at) as usize;
          let b = s[a..].find('\n').map_or(s.len(), |k| a + k);
          let l = elide(sp, (a, b), width, TruncationMode::Tail, m);
          *last = Line { start: last.start, end: last.start + l.end, ..l };
        }
      }
    }

    *lines = (start, t.lines.len() as u32);
    if let Some(e) = edit {
      let first = t.stops.len() as u32;
      for l in &mut t.lines[start as usize..] {
        l.stop = t.stops.len() as u32;
        let (a, b) = ((l.start - start_at) as usize, (l.end - start_at) as usize);
        let stops = kept.stops.get(style.font, &sp.s[a..b], sp.key(a, b, 0), || {
          let mut v = Vec::new();
          sp.stops(m, a, b, &mut v);
          v.iter_mut().for_each(|s| s.at -= a as u32);
          v
        });
        t.stops.extend(stops.iter().map(|s| Stop { at: a as u32 + s.at, ..*s }));
      }

      e.stops = (first, t.stops.len() as u32);
    }
  }
}

/// Whether a text in `style` wraps at spaces: it does unless it is one line.
fn wraps(style: TextStyle) -> bool {
  !matches!(style.lines, TextLines::NoWrap | TextLines::Limit(1, _))
}

/// `a..b` of the text cut to fit `width` with an ellipsis where `mode` says, as a line of it:
/// what is kept (offsets from `a`), its width with the ellipsis and its ink. Cut at the tail it
/// keeps the longest prefix; at the head the longest suffix; in the middle a prefix and a suffix
/// of about as many graphemes. It is cut between graphemes (an emoji sequence or a syllable stays
/// whole), the spaces beside the ellipsis dropped; the ellipsis is in the font where it is cut.
pub fn elide(
  sp: Span,
  (a, b): (usize, usize),
  width: f32,
  mode: TruncationMode,
  m: &mut dyn Measure,
) -> Line {
  let s = &sp.s[a..b];
  // The font of the ellipsis, where the text is cut.
  let at = |k: usize| sp.pieces(a + k, b).next().map_or(sp.style.font, |p| p.2);
  let cut = |x: usize, y: usize| if mode == TruncationMode::Head { at(y) } else { at(x) };

  let bounds: Vec<usize> = std::iter::once(0).chain(graphemes(s).map(|g| g.end)).collect();
  let n = bounds.len() - 1;
  // The pieces kept with `k` graphemes, and the widest `k` that fits.
  let keep = |k: usize| match mode {
    TruncationMode::Tail => (bounds[k], bounds[n]),
    TruncationMode::Head => (0, bounds[n - k]),
    TruncationMode::Middle => (bounds[k.div_ceil(2)], bounds[n - k / 2]),
  };
  let shown = |k: usize| {
    let (x, y) = keep(k);
    (s[..x].trim_end().len(), s.len() - s[y..].trim_start().len())
  };
  let (mut lo, mut hi) = (0, n + 1);
  while hi - lo > 1 {
    let mid = (lo + hi) / 2;
    let (x, y) = shown(mid);
    let w = sp.width(m, a, a + x) + m.width(ELLIPSIS, cut(x, y)) + sp.width(m, a + y, b);
    match w <= width + EPS {
      true => lo = mid,
      false => hi = mid,
    }
  }

  let (x, y) = shown(lo);
  let (w, ink) = match sp.runs.is_empty() {
    true => m.line(&format!("{}{ELLIPSIS}{}", &s[..x], &s[y..]), sp.style.font),
    false => {
      let ((wa, ia), (wb, ib)) = (sp.line(m, a, a + x), sp.line(m, a + y, b));
      let ink =
        Ink { left: ia.left, right: ib.right, up: ia.up.max(ib.up), down: ia.down.max(ib.down) };
      (wa + m.width(ELLIPSIS, cut(x, y)) + wb, ink)
    }
  };

  let (pa, sb) = (x as u32, y as u32);
  let (start, end, elide) = match mode {
    TruncationMode::Tail => (0, pa, Elide::Tail),
    TruncationMode::Head => (sb, s.len() as u32, Elide::Head),
    TruncationMode::Middle => (0, s.len() as u32, Elide::Middle(pa, sb)),
  };

  Line { start, end, width: w, ink, stop: 0, elide }
}

/// The size of `s` in `style` wrapped at `w`, as [wrap_text] breaks it: its widest line, and
/// its lines (at most its limit) at its line height. Never narrower than its least width, as
/// the layout gives it.
pub fn wrapped_size(s: &str, style: TextStyle, w: f32, m: &mut dyn Measure, k: &mut Kept) -> Size {
  let sp = Span::plain(s, style);
  let w = w.max(text_width(sp, m, k).1);
  let mut lines = Vec::new();
  wrap_text(sp, 0, w, false, m, &mut k.lines, &mut lines);
  let n = match style.lines {
    TextLines::Limit(n, _) => lines.len().min(usize::from(n.max(1))),
    _ => lines.len(),
  };

  let line_h = style.line_h.unwrap_or_else(|| m.metrics(style.font).line_height());
  Size::new(lines.iter().fold(0.0, |w, l| w.max(l.width)), n as f32 * line_h)
}

/// Breaks the text into lines no wider than `width`, `base` added to their offsets: at
/// newlines, and greedily at spaces when it wraps. A word wider than `width` starts a line and
/// breaks between characters (graphemes), as AppKit's word wrapping does, each line at least one
/// character. With `cover` (an editable text) the same breaks give lines that cover the text:
/// each up to where the next starts, with the spaces it broke at, the first from the line break
/// before it. Each line of text is broken once per width and kept.
pub fn wrap_text(
  sp: Span,
  base: u32,
  width: f32,
  cover: bool,
  m: &mut dyn Measure,
  kept: &mut Memo<Vec<Line>>,
  out: &mut Vec<Line>,
) {
  let style = sp.style;
  let key = u64::from(width.to_bits()) << 2 | u64::from(wraps(style)) << 1 | u64::from(cover);
  let mut at = 0;
  for line in sp.s.split('\n') {
    let (a, b) = (at, at + line.len());
    at = b + 1;
    let key = sp.key(a, b, key);
    let lines = kept.get(style.font, line, key, || wrap_line(sp, (a, b), width, cover, m));
    let off = base + a as u32;
    out.extend(lines.iter().map(|l| Line { start: off + l.start, end: off + l.end, ..*l }));
  }
}

/// [wrap_text] of the line of text `a..b` (no line break in it), offsets from its start.
fn wrap_line(
  sp: Span,
  (a, b): (usize, usize),
  width: f32,
  cover: bool,
  m: &mut dyn Measure,
) -> Vec<Line> {
  let (s, mut out) = (&sp.s[a..b], Vec::new());
  let (w, ink) = sp.line(m, a, b);
  if !wraps(sp.style) || w <= width + EPS {
    return vec![Line {
      start: 0,
      end: s.len() as u32,
      width: w,
      ink,
      stop: 0,
      elide: Elide::None,
    }];
  }

  let base = a;
  let wide = |m: &mut dyn Measure, x: usize, y: usize| sp.width(m, base + x, base + y);
  let mut push = |m: &mut dyn Measure, x: usize, y: usize| {
    let (width, ink) = sp.line(m, base + x, base + y);
    out.push(Line { start: x as u32, end: y as u32, width, ink, stop: 0, elide: Elide::None });
  };
  // The line so far: start, end and width.
  let mut cur: Option<(usize, usize, f32)> = None;
  let mut pos = 0;
  for word in s.split(' ') {
    let (a, b) = (pos, pos + word.len());
    pos = b + 1;
    if word.is_empty() {
      continue;
    }

    let ww = wide(m, a, b);
    if let Some((start, end, cw)) = cur {
      // The spaces between, each as wide as the first.
      let next = cw + wide(m, end, end + 1) * (a - end) as f32 + ww;
      if next <= width + EPS {
        cur = Some((start, b, next));
        continue;
      }

      push(m, start, if cover { a } else { end });
    }

    // Covering, the first line starts at the line break with the spaces after it: they count,
    // and stay on a line of their own when the word doesn't fit after them.
    let (mut from, mut w) = (a, ww);
    if cover && cur.is_none() && a > 0 {
      let pw = wide(m, 0, b);
      match pw <= width + EPS {
        true => (from, w) = (0, pw),
        false => push(m, 0, a),
      }
    }

    cur = Some(match w <= width + EPS {
      true => (from, b, w),
      // Too wide for a line: its pieces, the last one going on with the words after it.
      false => {
        let mut from = from;
        for cut in breaks(sp, (base + a, base + b), width, m) {
          push(m, from, a + cut);
          from = a + cut;
        }

        (from, b, wide(m, from, b))
      }
    });
  }

  let (start, end) = cur.map_or((0, 0), |(a, b, _)| (a, b));
  push(m, start, if cover { s.len() } else { end });
  out
}

/// Where the word `a..b`, wider than `width`, breaks: after the longest runs of whole graphemes
/// that fit, each at least one, as offsets in it; not at its end. Each line measures runs
/// doubling from its start to one that doesn't fit, then halves between: about its own length,
/// not the rest of the word's.
fn breaks(sp: Span, (a, b): (usize, usize), width: f32, m: &mut dyn Measure) -> Vec<usize> {
  let word = &sp.s[a..b];
  let bounds: Vec<usize> = std::iter::once(0).chain(graphemes(word).map(|g| g.end)).collect();

  let (mut cuts, mut from, last) = (Vec::new(), 0, bounds.len() - 1);
  let fits = |m: &mut dyn Measure, x: usize, y: usize| {
    sp.width(m, a + bounds[x], a + bounds[y]) <= width + EPS
  };
  loop {
    // `lo` fits (or is the one grapheme a line takes anyway), `hi` doesn't; none when the rest
    // fits.
    let (mut lo, mut hi, mut step) = (from + 1, None, 1);
    while lo < last {
      let probe = (lo + step).min(last);
      if !fits(m, from, probe) {
        hi = Some(probe);
        break;
      }

      (lo, step) = (probe, step * 2);
    }

    let Some(mut hi) = hi else { break };
    while hi - lo > 1 {
      let mid = (lo + hi) / 2;
      match fits(m, from, mid) {
        true => lo = mid,
        false => hi = mid,
      }
    }

    cuts.push(bounds[lo]);
    from = lo;
  }

  cuts
}
