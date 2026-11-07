// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The layout's text: a text's preferred and least widths, its lines wrapped at its final width
//! or cut to fit with an ellipsis, and what of them is kept across frames per font and text
//! ([Kept]).

use super::{
  ELLIPSIS, EPS, Elide, Kind, Line, Measure, TextLines, TextStyle, Tree, TruncationMode,
};
use crate::text::edit::graphemes;
use crate::text::{Font, Memo};

/// What the layout keeps across frames per font and text ([Memo]): a wrapping text's least
/// width, and the lines of each line of text by the width it wraps at, offsets from its start.
#[derive(Default)]
pub(crate) struct Kept {
  pub least: Memo<f32>,
  pub lines: Memo<Vec<Line>>,
}

/// The preferred width of a text (its widest line) and its least: when it wraps, its widest
/// character (grapheme), as a word that doesn't fit a line breaks between characters
/// ([wrap_text]); measured once per line of text and kept.
pub fn text_width(s: &str, style: TextStyle, m: &mut dyn Measure, kept: &mut Kept) -> (f32, f32) {
  if matches!(style.lines, TextLines::Limit(1, _)) && !s.contains('\n') {
    let w = m.width(s, style.font);
    return (w, w.min(m.width(ELLIPSIS, style.font)));
  }

  let (mut pref, mut least) = (0.0f32, 0.0f32);
  for line in s.split('\n') {
    let w = m.width(line, style.font);
    pref = pref.max(w);
    if !wraps(style) {
      least = least.max(w);
      continue;
    }

    // Every word's: a narrow word can hold the widest one ("iiii W").
    let chars = line.split(' ').flat_map(|w| graphemes(w).map(move |g| &w[g]));
    let widest = || chars.fold(0.0f32, |a, g| a.max(m.width(g, style.font)));
    least = least.max(*kept.least.get(style.font, line, 0, widest));
  }

  (pref, least)
}

/// Wraps every text element at its final width and sets its line metrics; editable ones get
/// lines that cover their text and caret stops.
pub fn wrap(t: &mut Tree, m: &mut dyn Measure, kept: &mut Kept) {
  t.lines.clear();
  t.stops.clear();
  let mut edits = t.edits.iter_mut().peekable();
  for (i, n) in t.nodes.iter_mut().enumerate() {
    // Text is measured in `f32`: a line is never that long.
    let (width, wide) = (n.size[0] as f32, n.content[0] as f32);
    let Kind::Text { range, style, lines, baseline, line_h } = &mut n.kind else { continue };
    let met = m.metrics(style.font);
    *line_h = style.line_h.unwrap_or_else(|| met.line_height());
    *baseline = (*line_h - met.ascent - met.descent) / 2.0 + met.ascent;
    let start = t.lines.len() as u32;
    let s = &t.text[range.0 as usize..range.1 as usize];
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
        let l = elide(s, style.font, width, mode, m);
        let l = Line { start: start_at + l.start, end: start_at + l.end, ..l };
        let elide = match l.elide {
          Elide::Middle(a, b) => Elide::Middle(start_at + a, start_at + b),
          e => e,
        };

        t.lines.push(Line { elide, ..l })
      }
      true => {
        let ink = m.line(s, style.font).1;
        let elide = Elide::None;
        t.lines.push(Line { start: start_at, end, width: wide, ink, stop: 0, elide })
      }
      false => {
        wrap_text(&t.text, *range, *style, width, cover, m, &mut kept.lines, &mut t.lines);
        // Past the limit, the lines go and the last kept one ends in "…".
        if let Some((n, _)) = limit
          && t.lines.len() - start as usize > n.max(1)
        {
          t.lines.truncate(start as usize + n.max(1));
          let last = t.lines.last_mut().unwrap();
          let rest = &t.text[last.start as usize..range.1 as usize];
          let rest = rest.split('\n').next().unwrap_or_default();
          let l = elide(rest, style.font, width, TruncationMode::Tail, m);
          *last = Line { start: last.start, end: last.start + l.end, ..l };
        }
      }
    }

    *lines = (start, t.lines.len() as u32);
    if let Some(e) = edit {
      let first = t.stops.len() as u32;
      for l in &mut t.lines[start as usize..] {
        l.stop = t.stops.len() as u32;
        let s = &t.text[l.start as usize..l.end as usize];
        m.stops(s, style.font, l.start - range.0, &mut t.stops);
      }

      e.stops = (first, t.stops.len() as u32);
    }
  }
}

/// Whether a text in `style` wraps at spaces: it does unless it is one line.
fn wraps(style: TextStyle) -> bool {
  !matches!(style.lines, TextLines::Fixed | TextLines::Limit(1, _))
}

/// `s` cut to fit `width` with an ellipsis where `mode` says, as a line of it: what is kept
/// (offsets from its start), its width with the ellipsis and its ink. Cut at the tail it keeps
/// the longest prefix; at the head the longest suffix; in the middle a prefix and a suffix of
/// about as many graphemes. It is cut between graphemes (an emoji sequence or a syllable stays
/// whole), the spaces beside the ellipsis dropped.
pub fn elide(s: &str, font: Font, width: f32, mode: TruncationMode, m: &mut dyn Measure) -> Line {
  let dots = m.width(ELLIPSIS, font);
  let bounds: Vec<usize> = std::iter::once(0).chain(graphemes(s).map(|g| g.end)).collect();
  let n = bounds.len() - 1;
  // The pieces kept with `k` graphemes, and the widest `k` that fits.
  let keep = |k: usize| match mode {
    TruncationMode::Tail => (bounds[k], bounds[n]),
    TruncationMode::Head => (0, bounds[n - k]),
    TruncationMode::Middle => (bounds[k.div_ceil(2)], bounds[n - k / 2]),
  };
  let shown = |k: usize| {
    let (a, b) = keep(k);
    (s[..a].trim_end(), s[b..].trim_start())
  };
  let (mut lo, mut hi) = (0, n + 1);
  while hi - lo > 1 {
    let mid = (lo + hi) / 2;
    let (a, b) = shown(mid);
    match m.width(a, font) + dots + m.width(b, font) <= width + EPS {
      true => lo = mid,
      false => hi = mid,
    }
  }

  let (a, b) = shown(lo);
  let (pa, sb) = (a.len() as u32, (s.len() - b.len()) as u32);
  let (w, ink) = m.line(&format!("{a}{ELLIPSIS}{b}"), font);
  let (start, end, elide) = match mode {
    TruncationMode::Tail => (0, pa, Elide::Tail),
    TruncationMode::Head => (sb, s.len() as u32, Elide::Head),
    TruncationMode::Middle => (0, s.len() as u32, Elide::Middle(pa, sb)),
  };

  Line { start, end, width: w, ink, stop: 0, elide }
}

/// The widest line of `s` in `style` wrapped at `width`, as [wrap_text] breaks it.
pub fn wrapped_width(s: &str, style: TextStyle, w: f32, m: &mut dyn Measure, k: &mut Kept) -> f32 {
  let mut lines = Vec::new();
  wrap_text(s, (0, s.len() as u32), style, w, false, m, &mut k.lines, &mut lines);
  lines.iter().fold(0.0, |w, l| w.max(l.width))
}

/// Breaks the text at `range` into lines no wider than `width`: at newlines, and greedily at
/// spaces when it wraps. A word wider than `width` starts a line and breaks between characters
/// (graphemes), as AppKit's word wrapping does, each line at least one character. With `cover`
/// (an editable text) the same breaks give lines that cover the text: each up to where the next
/// starts, with the spaces it broke at, the first from the line break before it. Each line of
/// text is broken once per width and kept.
#[allow(clippy::too_many_arguments)]
pub fn wrap_text(
  text: &str,
  range: (u32, u32),
  style: TextStyle,
  width: f32,
  cover: bool,
  m: &mut dyn Measure,
  kept: &mut Memo<Vec<Line>>,
  out: &mut Vec<Line>,
) {
  let key = u64::from(width.to_bits()) << 2 | u64::from(wraps(style)) << 1 | u64::from(cover);
  let mut at = range.0;
  for line in text[at as usize..range.1 as usize].split('\n') {
    let base = at;
    at += line.len() as u32 + 1;
    let lines = kept.get(style.font, line, key, || wrap_line(line, style, width, cover, m));
    out.extend(lines.iter().map(|l| Line { start: base + l.start, end: base + l.end, ..*l }));
  }
}

/// [wrap_text] of a line of text `s` (no line break in it), offsets from its start.
fn wrap_line(s: &str, style: TextStyle, width: f32, cover: bool, m: &mut dyn Measure) -> Vec<Line> {
  let (font, mut out) = (style.font, Vec::new());
  let (w, ink) = m.line(s, font);
  if !wraps(style) || w <= width + EPS {
    return vec![Line {
      start: 0,
      end: s.len() as u32,
      width: w,
      ink,
      stop: 0,
      elide: Elide::None,
    }];
  }

  let space = m.width(" ", font);
  let mut push = |m: &mut dyn Measure, a: usize, b: usize| {
    let (width, ink) = m.line(&s[a..b], font);
    out.push(Line { start: a as u32, end: b as u32, width, ink, stop: 0, elide: Elide::None });
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

    let ww = m.width(word, font);
    if let Some((start, end, cw)) = cur {
      let next = cw + space * (a - end) as f32 + ww;
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
      let pw = m.width(&s[..b], font);
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
        for cut in breaks(word, font, width, m) {
          push(m, from, a + cut);
          from = a + cut;
        }

        (from, b, m.width(&s[from..b], font))
      }
    });
  }

  let (start, end) = cur.map_or((0, 0), |(a, b, _)| (a, b));
  push(m, start, if cover { s.len() } else { end });
  out
}

/// Where `word`, wider than `width`, breaks: after the longest runs of whole graphemes that fit,
/// each at least one, as offsets in it; not at its end. Each line measures runs doubling from its
/// start to one that doesn't fit, then halves between: about its own length, not the rest of the
/// word's.
fn breaks(word: &str, font: Font, width: f32, m: &mut dyn Measure) -> Vec<usize> {
  let bounds: Vec<usize> = std::iter::once(0).chain(graphemes(word).map(|g| g.end)).collect();

  let (mut cuts, mut from, last) = (Vec::new(), 0, bounds.len() - 1);
  let fits = |m: &mut dyn Measure, a: usize, b: usize| {
    m.width(&word[bounds[a]..bounds[b]], font) <= width + EPS
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
