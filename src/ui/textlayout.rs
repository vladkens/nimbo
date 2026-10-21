// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The laid-out text of an editable or selectable element: its wrapped lines (the layout's
//! wrapping, at spaces, so a field wraps as plain text does) with a caret stop at every
//! grapheme boundary, from the shaper's glyph positions. It answers hit tests (a point to an
//! offset), caret and selection rects, and the visual lines the editor moves by ([Lines]).
//!
//! Offsets are bytes of the element's text. An offset at a soft wrap belongs to the line it
//! starts: there is no caret affinity, so the end of a wrapped line can't hold the caret (End
//! stops before the space it broke at). Right-to-left text is placed in logical order.

use std::ops::Range;

use super::layout::{Kind, Line, Tree};
use super::{Id, Response, Style, TextStyle, Ui};
use crate::app::{Event, Ime};
use crate::scene::{Color, Point, Rect};
use crate::text::edit::Lines;

/// Width of a selected line break, shown past the end of its line.
const BREAK_W: f32 = 4.0;

/// Where a caret goes in a line: a grapheme boundary, as a byte offset of the element's text,
/// and its x from the element's left edge.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stop {
  pub at: u32,
  pub x: f32,
}

/// The selection, caret and marked text of an editable or selectable text element, as byte
/// offsets of its text, and how to draw them ([super::Ui::edit_text]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edit {
  /// Anchor and head.
  pub sel: (u32, u32),
  /// Draws the caret at the head.
  pub caret: bool,
  /// Scrolls the nearest scroll container to show the head: the request's number, 0 for none
  /// (a field tells one request from the next by it).
  pub reveal: u32,
  /// An input method's marked text, underlined; empty when none.
  pub marked: (u32, u32),
  pub sel_color: Color,
  pub caret_color: Color,
  /// The text shown is a placeholder for an empty one.
  pub placeholder: bool,
  /// The text element.
  pub node: u32,
  /// Its caret stops in [Tree::stops], filled by the layout.
  pub stops: (u32, u32),
}

impl Edit {
  /// A selection (anchor, head) drawn with `sel_color`; no caret.
  pub const fn new(sel: (u32, u32), sel_color: Color, caret_color: Color) -> Self {
    Self {
      sel,
      caret: false,
      reveal: 0,
      marked: (0, 0),
      sel_color,
      caret_color,
      placeholder: false,
      node: 0,
      stops: (0, 0),
    }
  }
}

impl Ui<'_> {
  /// Text with a selection, caret and marked text ([Edit]), laid out over its whole text with
  /// caret stops ([TextLayout]); `node` styles the element (cursor, focus, input). What text
  /// fields and selectable text build on.
  pub fn edit_text(&mut self, s: &str, style: TextStyle, edit: Edit, node: Style) -> Response {
    let text = &mut self.st.tree.text;
    let start = text.len() as u32;
    text.push_str(s);
    let range = (start, text.len() as u32);
    let kind = Kind::Text { range, style, lines: (0, 0), baseline: 0.0, line_h: 0.0 };
    let (i, r) = self.push(kind, node, 0);
    self.st.tree.edits.push(Edit { node: i, ..edit });
    r
  }

  /// An input method's marked text or commit for `id` ([Event::Ime]), seen once. Reading it
  /// uses it.
  pub fn ime(&self, id: Id) -> Option<&Ime> {
    match self.st.peek(id)? {
      Event::Ime(_) => match self.st.take(id) {
        Some(Event::Ime(ime)) => Some(ime),
        _ => None,
      },
      _ => None,
    }
  }
}

/// An editable or selectable text element of a laid-out tree.
#[derive(Clone, Copy, Debug)]
pub struct TextLayout<'t> {
  /// The element's text; empty when it shows a placeholder.
  pub text: &'t str,
  /// The element's box in the window.
  pub rect: Rect,
  pub line_h: f32,
  /// Visual lines a page moves by; 0 moves to the start or end.
  pub page: usize,
  lines: &'t [Line],
  stops: &'t [Stop],
  /// Where the element's text starts in the tree's, which `lines` are offsets of.
  base: u32,
  /// One past the last stop of the last line.
  stop_end: u32,
  placeholder: bool,
}

impl<'t> TextLayout<'t> {
  /// The text element `id` of a laid-out tree, if it is editable or selectable.
  pub fn of(t: &'t Tree, id: Id) -> Option<Self> {
    Self::at(t, t.find(id)?)
  }

  pub(crate) fn at(t: &'t Tree, i: u32) -> Option<Self> {
    let n = &t.nodes[i as usize];
    let Kind::Text { range, lines, line_h, .. } = n.kind else { return None };
    let e = &t.edits[t.edit(i)?];
    Some(Self {
      text: if e.placeholder { "" } else { t.str(range.0, range.1) },
      rect: n.rect,
      line_h,
      page: 0,
      lines: &t.lines[lines.0 as usize..lines.1 as usize],
      stops: &t.stops,
      base: range.0,
      stop_end: e.stops.1,
      placeholder: e.placeholder,
    })
  }

  /// The number of visual lines.
  pub fn len(&self) -> usize {
    if self.placeholder { 1 } else { self.lines.len() }
  }

  pub fn is_empty(&self) -> bool {
    self.text.is_empty()
  }

  /// Visual line `k`: a byte range without its line break.
  pub fn range(&self, k: usize) -> Range<usize> {
    if self.placeholder {
      return 0..0;
    }

    let l = &self.lines[k];
    (l.start - self.base) as usize..(l.end - self.base) as usize
  }

  fn line_stops(&self, k: usize) -> &'t [Stop] {
    if self.placeholder {
      return &[Stop { at: 0, x: 0.0 }];
    }

    let a = self.lines[k].stop as usize;
    let b = self.lines.get(k + 1).map_or(self.stop_end, |l| l.stop) as usize;
    &self.stops[a..b]
  }

  /// The visual line holding offset `i`: the last one starting at or before it.
  pub fn line_of(&self, i: usize) -> usize {
    if self.placeholder {
      return 0;
    }

    let at = self.base as usize + i;
    self.lines.partition_point(|l| l.start as usize <= at).saturating_sub(1)
  }

  /// x of offset `i` in line `k`, from the element's left edge (the stop at or before it).
  pub fn x_in(&self, k: usize, i: usize) -> f32 {
    let s = self.line_stops(k);
    match s.partition_point(|s| s.at as usize <= i) {
      0 => 0.0,
      j => s[j - 1].x,
    }
  }

  /// The offset in line `k` nearest to `x` from the element's left edge, or with `inside` the
  /// one starting the grapheme `x` is over (a double click selects that one's word).
  pub fn hit_in(&self, k: usize, x: f32, inside: bool) -> usize {
    let s = self.line_stops(k);
    let j = s.partition_point(|s| s.x <= x);
    let at = match (j, inside) {
      (0, _) => 0,
      (j, _) if j == s.len() => s.len() - 1,
      (j, true) => j - 1,
      (j, false) if x - s[j - 1].x <= s[j].x - x => j - 1,
      (j, false) => j,
    };

    s.get(at).map_or(0, |s| s.at as usize)
  }

  /// The visual line at `y` in the window, clamped to the first and last.
  pub fn line_at(&self, y: f32) -> usize {
    let k = ((y - self.rect.y) / self.line_h).floor().max(0.0) as usize;
    k.min(self.len() - 1)
  }

  /// The offset nearest to `p` in the window (see [Self::hit_in] for `inside`).
  pub fn hit(&self, p: Point, inside: bool) -> usize {
    self.hit_in(self.line_at(p.y), p.x - self.rect.x, inside)
  }

  /// The caret at offset `i` in the window: zero wide, a line high.
  pub fn caret(&self, i: usize) -> Rect {
    let k = self.line_of(i);
    let y = self.rect.y + k as f32 * self.line_h;
    Rect::new(self.rect.x + self.x_in(k, i), y, 0.0, self.line_h)
  }

  /// Calls `f` with the rect of `r` on each visual line it spans, in the window. A selected line
  /// break shows a little past its line's end.
  pub fn rects(&self, r: Range<usize>, mut f: impl FnMut(Rect)) {
    if r.is_empty() || self.placeholder {
      return;
    }

    let (first, last) = (self.line_of(r.start), self.line_of(r.end));
    for k in first..=last.min(self.len() - 1) {
      let lr = self.range(k);
      let (a, b) = (r.start.max(lr.start), r.end.min(lr.end));
      let x0 = self.x_in(k, a);
      let mut x1 = self.x_in(k, b);
      if r.end > lr.end && self.text[lr.end..].starts_with('\n') {
        x1 += BREAK_W;
      }

      if x1 > x0 {
        let y = self.rect.y + k as f32 * self.line_h;
        f(Rect::new(self.rect.x + x0, y, x1 - x0, self.line_h));
      }
    }
  }
}

/// Scrolls the nearest scroll container of each editable text that asks for it the least that
/// shows its caret inside the container's padding, moving what is inside it along.
pub(super) fn reveal(t: &mut Tree, scale: f32) {
  for k in 0..t.edits.len() {
    let e = t.edits[k];
    if e.reveal == 0 {
      continue;
    }

    let Some(lay) = TextLayout::at(t, e.node) else { continue };
    let c = lay.caret(e.sel.1 as usize);
    let mut p = t.nodes[e.node as usize].parent as usize;
    while p != 0 && !(t.nodes[p].style.scroll[0] || t.nodes[p].style.scroll[1]) {
      p = t.nodes[p].parent as usize;
    }

    let n = &t.nodes[p];
    if p == 0 && !n.style.scroll.contains(&true) {
      continue;
    }

    let (r, pad) = (n.rect, n.style.pad);
    // Edges of the visible inner box and of the caret, per axis.
    let lo = [r.x + pad[3], r.y + pad[0]];
    let hi = [r.x + r.w - pad[1], r.y + r.h - pad[2]];
    let (clo, chi) = ([c.x, c.y], [c.x + 1.0, c.y + c.h]);
    let snap = |v: f32| if scale > 0.0 { (v * scale).round() / scale } else { v };
    let (mut d, mut scroll) = ([0.0; 2], [n.scroll.x, n.scroll.y]);
    for ax in 0..2 {
      if !n.style.scroll[ax] {
        continue;
      }

      let want = match () {
        _ if clo[ax] < lo[ax] => clo[ax] - lo[ax],
        _ if chi[ax] > hi[ax] => (chi[ax] - hi[ax]).min(clo[ax] - lo[ax]),
        _ => 0.0,
      };

      let size = if ax == 0 { r.w } else { r.h };
      let max = (n.content[ax] - size).max(0.0);
      let to = snap((scroll[ax] + want).clamp(0.0, max));
      (d[ax], scroll[ax]) = (to - scroll[ax], to);
    }

    if d == [0.0; 2] {
      continue;
    }

    let end = n.end as usize;
    t.nodes[p].scroll = Point::new(scroll[0], scroll[1]);
    // Overlays on the container itself (its scrollbars) or the window stay.
    let mut k = p + 1;
    while k < end {
      let c = &mut t.nodes[k];
      if c.float.is_some_and(|f| f.window || c.parent as usize == p) {
        k = c.end as usize;
        continue;
      }

      (c.rect.x, c.rect.y) = (c.rect.x - d[0], c.rect.y - d[1]);
      (c.pos[0], c.pos[1]) = (c.pos[0] - d[0], c.pos[1] - d[1]);
      k += 1;
    }
  }
}

impl Lines for TextLayout<'_> {
  fn line(&self, _s: &str, i: usize) -> Range<usize> {
    self.range(self.line_of(i))
  }

  fn x(&self, _s: &str, i: usize) -> f32 {
    self.x_in(self.line_of(i), i)
  }

  fn hit(&self, _s: &str, line: Range<usize>, x: f32) -> usize {
    self.hit_in(self.line_of(line.start), x, false)
  }

  fn page(&self) -> usize {
    self.page
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::{Edit, *};
  use crate::keys::Move;
  use crate::scene::Size;
  use crate::text::Font;
  use crate::text::edit::Editor;
  use crate::ui::layout::tests::Fake;
  use crate::ui::{Style, TextStyle, Ui, UiState, px};

  /// 6 points per character, 10 per line.
  const T: TextStyle = TextStyle::new(Font::ui(10.0), [1.0; 4]);

  /// `s` as an editable text in a column `w` wide at (10, 20).
  fn laid(s: &str, w: f32) -> UiState {
    let mut ui = Ui::new(UiState::default(), Size::new(400.0, 300.0), None, Duration::ZERO);
    ui.col(Style::new().pad(10.0).pads([20.0, 0.0, 0.0, 10.0]), |ui| {
      ui.col(Style::new().w(px(w)), |ui| {
        let edit = Edit::new((0, 0), [0.0; 4], [0.0; 4]);
        ui.id("t").edit_text(s, T, edit, Style::new());
      });
    });
    ui.layout(&mut Fake, 1.0);
    ui.finish()
  }

  #[test]
  fn lines_cover_the_text_and_wrap_like_plain_text() {
    // "aaaa bbbb" fits 56; the spaces it broke at stay on the first line.
    let st = laid("aaaa bbbb  cccc\ndd\n", 56.0);
    let l = TextLayout::of(&st.last, Id::of("t")).unwrap();
    let lines: Vec<_> = (0..l.len()).map(|k| l.range(k)).collect();
    assert_eq!(lines, [0..11, 11..15, 16..18, 19..19]);
    // Stops every character, from the line's start.
    assert_eq!(
      (l.x_in(0, 4), l.x_in(0, 11), l.x_in(1, 11), l.x_in(1, 13)),
      (24.0, 66.0, 0.0, 12.0)
    );
    // An offset at the wrap belongs to the next line.
    assert_eq!((l.line_of(10), l.line_of(11), l.line_of(15), l.line_of(16)), (0, 1, 1, 2));
    // Hit tests: nearest boundary, or the grapheme under the point.
    assert_eq!(l.hit(Point::new(10.0 + 14.0, 25.0), false), 2);
    assert_eq!(l.hit(Point::new(10.0 + 14.0, 25.0), true), 2);
    assert_eq!(l.hit(Point::new(10.0 + 16.0, 25.0), false), 3);
    assert_eq!(l.hit(Point::new(10.0 + 16.0, 25.0), true), 2);
    assert_eq!(l.hit(Point::new(500.0, 35.0), false), 15);
    assert_eq!(l.hit(Point::new(-5.0, 999.0), false), 19);
    assert_eq!(l.caret(13), Rect::new(22.0, 30.0, 0.0, 10.0));
    // A selection over three lines, its line break shown.
    let mut rects = Vec::new();
    l.rects(2..17, |r| rects.push(r));
    let want = [
      Rect::new(22.0, 20.0, 54.0, 10.0),
      Rect::new(10.0, 30.0, 28.0, 10.0),
      Rect::new(10.0, 40.0, 6.0, 10.0),
    ];
    assert_eq!(rects, want);
    // A word wider than a line breaks between characters, the lines still covering the text.
    let st = laid("ab cdefghijklmnop q", 56.0);
    let l = TextLayout::of(&st.last, Id::of("t")).unwrap();
    let lines: Vec<_> = (0..l.len()).map(|k| l.range(k)).collect();
    assert_eq!(lines, [0..3, 3..12, 12..19]);
  }

  #[test]
  fn editor_moves_over_laid_out_lines() {
    let s = "aaaa bbbb  cccc\ndd";
    let st = laid(s, 56.0);
    let l = TextLayout::of(&st.last, Id::of("t")).unwrap();
    let mut e = Editor::multiline(s);
    e.select(crate::text::edit::Sel::caret(2));
    let mut mv = |m| {
      e.move_by(m, false, &l);
      e.caret()
    };
    assert_eq!(mv(Move::Down), 13);
    assert_eq!(mv(Move::Down), 18);
    assert_eq!(mv(Move::Up), 13);
    assert_eq!(mv(Move::LineStart), 11);
    // The end of a wrapped line: before the space it broke at.
    assert_eq!(mv(Move::Up), 0);
    assert_eq!(mv(Move::LineEnd), 10);
  }
}
