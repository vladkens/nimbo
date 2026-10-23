// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The element tree and its flex layout, after Clay (github.com/nicbarker/clay): fit widths
//! bottom-up, distribute them top-down (percent, grow by weight, shrink on overflow), wrap
//! text at the final widths, then the same two passes for heights, then positions. Pure logic:
//! text and image sizes come through [Measure].

use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::sync::atomic::{AtomicBool, Ordering};

use super::textlayout::{Edit, Stop, reveal};
use super::{Align, Clip, Dim, Float, Id, Style, TextStyle};
use crate::keys::Binding;
use crate::scene::{Color, ImageId, Images, Point, Rect, Size};
use crate::text::edit::{graphemes, next_grapheme};
use crate::text::{Font, FxHasher, Ink, Metrics, TextSystem};

/// Slack for float sums when comparing sizes, in points.
const EPS: f32 = 0.01;

/// Ends a single line of text cut to fit ([TextStyle::ellipsis]).
pub const ELLIPSIS: &str = "…";

/// Text and image sizes for the layout; the runtime's measures with CoreText, tests fake it.
pub trait Measure {
  /// Advance width of `s` in points.
  fn width(&mut self, s: &str, font: Font) -> f32;
  /// The advance width of `s` as a line and how far its ink reaches, which the paint pass culls
  /// by; none past its pen and baseline unless told.
  fn line(&mut self, s: &str, font: Font) -> (f32, Ink) {
    (self.width(s, font), Ink::default())
  }

  fn metrics(&mut self, font: Font) -> Metrics;
  /// An image's natural size in points.
  fn image(&mut self, id: ImageId) -> Size;
  /// Pushes a caret stop for every grapheme boundary of `s`, the start and end included, with
  /// `base` added to its offset. By default from the widths of the prefixes.
  fn stops(&mut self, s: &str, font: Font, base: u32, out: &mut Vec<Stop>) {
    let mut i = 0;
    loop {
      out.push(Stop { at: base + i as u32, x: self.width(&s[..i], font) });
      if i >= s.len() {
        return;
      }

      i = next_grapheme(s, i);
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
  Box,
  /// `range` is the text in [Tree::text]; the layout fills the rest.
  Text {
    range: (u32, u32),
    style: TextStyle,
    /// Wrapped lines in [Tree::lines].
    lines: (u32, u32),
    /// From the top of a line to its baseline.
    baseline: f32,
    line_h: f32,
  },
  Image(ImageId),
  /// The index of its paint callback.
  Canvas(u32),
  /// Over a scroll container ([super::Ui::fade_edges]): its content fading into `color` under
  /// the top and bottom edges, over `len` points, as far as it is scrolled away from them.
  Fade {
    color: Color,
    len: f32,
  },
}

/// A wrapped line of a text element: a byte range of [Tree::text], its width and its ink. The
/// lines of an editable text cover it all: a soft-wrapped line keeps the spaces it broke at, and
/// its caret stops start at `stop` in [Tree::stops].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
  pub start: u32,
  pub end: u32,
  pub width: f32,
  pub ink: Ink,
  pub stop: u32,
}

#[derive(Clone, Debug)]
pub struct Node {
  pub id: Id,
  pub kind: Kind,
  pub style: Style,
  /// 0 for a row (children along x), 1 for a column.
  pub axis: u8,
  pub float: Option<Float>,
  pub parent: u32,
  /// One past the last node of the subtree. Nodes are in pre-order: the first child of `i` is
  /// `i + 1` and each next one starts at the previous one's `end`.
  pub end: u32,
  /// Columns of a grid ([super::Ui::grid]); 0 for a row or column.
  pub cols: u8,
  /// Columns it takes in a grid ([super::Ui::span]), at least 1.
  pub span: u8,
  /// Moves the children back by this much (scroll containers).
  pub scroll: Point,
  /// The laid-out box, edges snapped to pixels.
  pub rect: Rect,
  /// Width and height of the content (children, gaps and padding, or text lines); more than
  /// the box when it overflows.
  pub content: [f32; 2],
  size: [f32; 2],
  /// The least the box can shrink to: the content's minimum (the longest word of a text).
  min: [f32; 2],
  pub(super) pos: [f32; 2],
  /// Grow distribution: the size is final.
  frozen: bool,
}

impl Node {
  pub(super) fn new(
    id: Id,
    kind: Kind,
    style: Style,
    axis: u8,
    float: Option<Float>,
    parent: u32,
    end: u32,
  ) -> Self {
    Self {
      id,
      kind,
      style,
      axis,
      float,
      parent,
      end,
      cols: 0,
      span: 1,
      scroll: Point::default(),
      rect: Rect::default(),
      content: [0.0; 2],
      size: [0.0; 2],
      min: [0.0; 2],
      pos: [0.0; 2],
      frozen: false,
    }
  }
}

/// A box and its content's size, as a view saw them ([Tree::settle]).
pub type Seen = (Rect, [f32; 2]);

/// One frame's elements; node 0 is the root, the window.
#[derive(Default)]
pub struct Tree {
  pub nodes: Vec<Node>,
  /// The text of every text element, back to back.
  pub text: String,
  pub lines: Vec<Line>,
  /// Overlays, in paint order once laid out.
  pub floats: Vec<u32>,
  /// Interactive nodes ([Style::hit]), in tree order: what hit testing looks at.
  pub hits: Vec<u32>,
  /// Scroll containers, in tree order.
  pub scrolls: Vec<u32>,
  /// Elements with key bindings, in tree order.
  pub keymaps: Vec<(u32, &'static [Binding])>,
  /// Content built only in part (a virtual list's rows in view): its scroll container and the
  /// span of its content built ([super::Ui::partial_short]).
  pub partial: Vec<(u32, f32, f32)>,
  /// Elements whose last-frame box and content the view drew from, as it saw them
  /// ([super::Ui::settle]).
  pub settle: Vec<(Id, Option<Seen>)>,
  /// The selections of editable and selectable text elements, in tree order.
  pub edits: Vec<Edit>,
  /// Caret stops of the editable texts' lines.
  pub stops: Vec<Stop>,
  index: HashMap<Id, u32, BuildHasherDefault<FxHasher>>,
  /// A grid's row heights while it is laid out.
  rows: Vec<f32>,
}

impl Tree {
  /// Empties the tree down to the root for a window of `size`; buffers keep their capacity.
  pub fn clear(&mut self, size: Size) {
    self.nodes.clear();
    self.text.clear();
    self.lines.clear();
    self.floats.clear();
    self.hits.clear();
    self.scrolls.clear();
    self.keymaps.clear();
    self.partial.clear();
    self.settle.clear();
    self.edits.clear();
    self.stops.clear();
    self.index.clear();
    let root = Style::new().w(Dim::Px(size.w)).h(Dim::Px(size.h));
    self.nodes.push(Node::new(Id::ROOT, Kind::Box, root, 1, None, 0, 1));
  }

  /// A node by id, once laid out.
  pub fn get(&self, id: Id) -> Option<&Node> {
    self.index.get(&id).map(|&i| &self.nodes[i as usize])
  }

  pub fn rect(&self, id: Id) -> Option<Rect> {
    self.get(id).map(|n| n.rect)
  }

  /// A node's index by id, once laid out.
  pub fn find(&self, id: Id) -> Option<u32> {
    self.index.get(&id).copied()
  }

  /// The index in [Tree::edits] of node `i`'s selection, when it is an editable text.
  pub fn edit(&self, i: u32) -> Option<usize> {
    if self.edits.is_empty() {
      return None;
    }

    self.edits.binary_search_by_key(&i, |e| e.node).ok()
  }

  /// The key bindings of node `i`.
  pub fn keymap(&self, i: u32) -> Option<&'static [Binding]> {
    self.keymaps.iter().find(|(k, _)| *k == i).map(|(_, m)| *m)
  }

  /// The direct children of node `i`, overlays included.
  pub fn children(&self, i: u32) -> impl Iterator<Item = u32> + '_ {
    let end = self.nodes[i as usize].end;
    let mut c = i + 1;
    std::iter::from_fn(move || {
      (c < end).then(|| {
        let k = c;
        c = self.nodes[k as usize].end;
        k
      })
    })
  }

  pub fn str(&self, start: u32, end: u32) -> &str {
    &self.text[start as usize..end as usize]
  }

  /// The clip of overlay `i` when it keeps its ancestors' ([Float::clip]): theirs intersected,
  /// up to the window or an ancestor overlay that doesn't keep them.
  pub fn float_clip(&self, i: u32) -> Option<Rect> {
    let n = &self.nodes[i as usize];
    if !n.float.is_some_and(|f| f.clip) {
      return None;
    }

    let (mut clip, mut k) = (None::<Rect>, n.parent);
    loop {
      let a = &self.nodes[k as usize];
      if let Some(r) = a.style.clip_rect(a.rect) {
        clip = Some(clip.map_or(r, |c| c.intersect(r)));
      }

      if k == 0 || a.float.is_some_and(|f| !f.clip) {
        return clip;
      }

      k = a.parent;
    }
  }
}

/// Lays out `t` in a window of `win`, snapping edges to pixels at `scale` (0 doesn't snap),
/// sorts the overlays by z and indexes the nodes by id.
pub fn layout(t: &mut Tree, win: Size, scale: f32, m: &mut dyn Measure) {
  let n = t.nodes.len() as u32;
  let root = &mut t.nodes[0];
  root.end = n;
  root.style.size = [Dim::Px(win.w), Dim::Px(win.h)];

  fit(t, 0, m);
  grow(t, 0, win);
  wrap(t, m);
  fit(t, 1, m);
  grow(t, 1, win);
  place(t, win, scale);
  reveal(t, scale);

  let nodes = &t.nodes;
  t.floats.sort_unstable_by_key(|&i| (float_z(nodes, i), i));
  t.index.clear();
  for (i, n) in t.nodes.iter().enumerate() {
    if t.index.insert(n.id, i as u32).is_some() {
      duplicate(n.id);
    }
  }
}

/// The paint order of the overlay at node `i`: its own z, raised to that of every overlay it
/// is inside, so a menu's own overlays (its scrollbar) paint over it, not under.
fn float_z(nodes: &[Node], i: u32) -> i32 {
  let mut z = nodes[i as usize].float.map_or(0, |f| f.z);
  let mut k = i;
  while k != 0 {
    k = nodes[k as usize].parent;
    if let Some(f) = nodes[k as usize].float {
      z = z.max(f.z);
    }
  }

  z
}

/// Two elements with one id: input and state go to one of them, a bug in the view. A debug
/// build panics; a release one says so once.
#[cold]
fn duplicate(id: Id) {
  if cfg!(debug_assertions) {
    panic!("duplicate element id {id:?}");
  }

  static WARNED: AtomicBool = AtomicBool::new(false);
  if !WARNED.swap(true, Ordering::Relaxed) {
    eprintln!("nimbo: duplicate element id {id:?} (reported once)");
  }
}

fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
  v.min(hi).max(lo)
}

/// Padding before and after the content along an axis.
fn pads(s: &Style, ax: usize) -> (f32, f32) {
  match ax {
    0 => (s.pad[3], s.pad[1]),
    _ => (s.pad[0], s.pad[2]),
  }
}

/// Calls `f` with each direct child of node `i`.
fn each(t: &mut Tree, i: usize, mut f: impl FnMut(&mut Node)) {
  let end = t.nodes[i].end as usize;
  let mut c = i + 1;
  while c < end {
    let ch = &mut t.nodes[c];
    f(ch);
    c = ch.end as usize;
  }
}

// MARK: Sizes

/// Bottom-up: each box's size from its content, clamped to its min and max.
fn fit(t: &mut Tree, ax: usize, m: &mut dyn Measure) {
  for i in (0..t.nodes.len()).rev() {
    let (content, least) = match t.nodes[i].kind {
      Kind::Text { range, style, .. } if ax == 0 => {
        text_width(&t.text[range.0 as usize..range.1 as usize], style, m)
      }
      Kind::Text { lines, line_h, .. } => {
        let h = (lines.1 - lines.0) as f32 * line_h;
        (h, h)
      }
      Kind::Image(id) => {
        let s = m.image(id);
        let v = if ax == 0 { s.w } else { s.h };
        (v, v)
      }
      Kind::Box | Kind::Canvas(_) | Kind::Fade { .. } if t.nodes[i].cols > 0 => grid_fit(t, i, ax),
      Kind::Box | Kind::Canvas(_) | Kind::Fade { .. } => children_fit(t, i, ax),
    };

    let n = &mut t.nodes[i];
    let s = &n.style;
    let (lo, hi) = (s.min[ax], s.max[ax]);
    n.content[ax] = content;
    (n.size[ax], n.min[ax]) = match s.size[ax] {
      Dim::Px(v) => (clamp(v, lo, hi), clamp(v, lo, hi)),
      Dim::Rigid => (clamp(content, lo, hi), clamp(content, lo, hi)),
      _ if s.clip != Clip::None => (clamp(content, lo, hi), lo),
      _ => (clamp(content, lo, hi), clamp(least, lo, hi)),
    };
  }
}

/// The content size and minimum of a box from its children in the flow.
fn children_fit(t: &Tree, i: usize, ax: usize) -> (f32, f32) {
  let n = &t.nodes[i];
  let along = n.axis as usize == ax;
  let (mut size, mut least, mut count) = (0.0f32, 0.0f32, 0);
  let mut c = i + 1;
  while c < n.end as usize {
    let ch = &t.nodes[c];
    if ch.float.is_none() {
      count += 1;
      match along {
        true => (size, least) = (size + ch.size[ax], least + ch.min[ax]),
        false => (size, least) = (size.max(ch.size[ax]), least.max(ch.min[ax])),
      }
    }

    c = ch.end as usize;
  }

  let gaps = if along && count > 1 { n.style.gap * (count - 1) as f32 } else { 0.0 };
  let (a, b) = pads(&n.style, ax);
  (size + gaps + a + b, least + gaps + a + b)
}

/// The preferred width of a text (its widest line) and its least: when it wraps, its widest
/// character (grapheme), as a word that doesn't fit a line breaks between characters
/// ([wrap_text]).
fn text_width(s: &str, style: TextStyle, m: &mut dyn Measure) -> (f32, f32) {
  if style.ellipsis && !s.contains('\n') {
    let w = m.width(s, style.font);
    return (w, w.min(m.width(ELLIPSIS, style.font)));
  }

  let (mut pref, mut least) = (0.0f32, 0.0f32);
  for line in s.split('\n') {
    let w = m.width(line, style.font);
    pref = pref.max(w);
    if !style.wrap {
      least = least.max(w);
      continue;
    }

    // Every word's: a narrow word can hold the widest one ("iiii W").
    for word in line.split(' ') {
      for g in graphemes(word) {
        least = least.max(m.width(&word[g], style.font));
      }
    }
  }

  (pref, least)
}

/// Top-down: sizes children from their parent's final size, along the parent's axis
/// ([distribute]) and across it (fill or shrink to the inner size); overlays from their target.
fn grow(t: &mut Tree, ax: usize, win: Size) {
  let win = [win.w, win.h];
  for i in 0..t.nodes.len() {
    let n = &t.nodes[i];
    if n.end as usize == i + 1 {
      continue;
    }

    let (along, size, gap) = (n.axis as usize == ax, n.size[ax], n.style.gap);
    let (a, b) = pads(&n.style, ax);
    let inner = size - a - b;
    let scrolls = n.style.scroll[ax];
    if n.cols > 0 {
      grid_grow(t, i, ax, inner, win);
      continue;
    }

    let mut count = 0;
    each(t, i, |c| match c.float {
      Some(f) => {
        let space = if f.window { win[ax] } else { size };
        c.size[ax] = match c.style.size[ax] {
          Dim::Fit | Dim::Rigid => c.size[ax],
          _ => fill(c, ax, space),
        };
      }
      None if along => count += 1,
      // Across a scroll axis children keep their content's size, growing ones at least fill.
      None if scrolls => {
        c.size[ax] = match c.style.size[ax] {
          Dim::Fit | Dim::Rigid | Dim::Px(_) => c.size[ax],
          Dim::Grow(_) => inner.max(c.size[ax]).min(c.style.max[ax]),
          Dim::Pct(_) => fill(c, ax, inner),
        }
      }
      None => c.size[ax] = fill(c, ax, inner),
    });
    if count > 0 {
      distribute(t, i, ax, inner - gap * (count - 1) as f32);
    }
  }
}

/// A child's size across its parent's axis, or an overlay's, given the space it has.
fn fill(n: &Node, ax: usize, space: f32) -> f32 {
  let s = &n.style;
  match s.size[ax] {
    Dim::Grow(_) => clamp(space, n.min[ax], s.max[ax]),
    Dim::Pct(p) => clamp(space * p / 100.0, s.min[ax], s.max[ax]),
    Dim::Px(_) => n.size[ax],
    Dim::Fit | Dim::Rigid => n.size[ax].min(space).max(n.min[ax]),
  }
}

/// Sizes the children of node `i` along its axis in `avail` (the inner size less the gaps):
/// percent ones from it, growing ones share what the others leave by weight within their min
/// and max; on overflow the widest fit or growing ones shrink first, down to their minimum.
fn distribute(t: &mut Tree, i: usize, ax: usize, avail: f32) {
  let mut fixed = 0.0;
  each(t, i, |c| {
    if c.float.is_some() {
      return;
    }

    match c.style.size[ax] {
      Dim::Pct(p) => c.size[ax] = clamp(avail * p / 100.0, c.style.min[ax], c.style.max[ax]),
      Dim::Grow(_) => {
        c.frozen = false;
        return;
      }
      _ => {}
    }

    fixed += c.size[ax];
  });

  // CSS flexbox's flexible lengths: share what is left by weight, clamp each share within its
  // min and max, and freeze by the clamps' total: those raised to their minimum when it grew,
  // those cut to their maximum when it shrank, every one when it is none; again until all are.
  let weight = |c: &Node| match (c.float, c.style.size[ax]) {
    (None, Dim::Grow(w)) if !c.frozen => Some(w),
    _ => None,
  };
  loop {
    let (mut rest, mut wsum) = (avail - fixed, 0.0);
    each(t, i, |c| match (c.float, c.style.size[ax]) {
      (None, Dim::Grow(w)) if !c.frozen => wsum += w,
      (None, Dim::Grow(_)) => rest -= c.size[ax],
      _ => {}
    });
    if wsum == 0.0 {
      break;
    }

    let share = rest / wsum;
    let mut violation = 0.0;
    each(t, i, |c| {
      if let Some(w) = weight(c) {
        violation += clamp(share * w, c.min[ax], c.style.max[ax]) - share * w;
      }
    });
    each(t, i, |c| {
      let Some(w) = weight(c) else { return };
      let target = clamp(share * w, c.min[ax], c.style.max[ax]);
      let freeze = match violation {
        v if v > EPS => target > share * w,
        v if v < -EPS => target < share * w,
        _ => true,
      };

      if freeze {
        (c.size[ax], c.frozen) = (target, true);
      }
    });
  }

  let mut total = 0.0;
  each(t, i, |c| {
    if c.float.is_none() {
      total += c.size[ax];
    }
  });
  let over = total - avail;
  // Along a scroll axis the content overflows instead.
  if over <= EPS || t.nodes[i].style.scroll[ax] {
    return;
  }
  // Water-fill from the top: find the cap where cutting everything above it (but not below
  // each one's minimum) frees `over`.
  let shrinks = |c: &Node| c.float.is_none() && matches!(c.style.size[ax], Dim::Fit | Dim::Grow(_));
  let mut hi = 0.0f32;
  each(t, i, |c| {
    if shrinks(c) {
      hi = hi.max(c.size[ax]);
    }
  });
  let mut lo = 0.0f32;
  for _ in 0..32 {
    let cap = (lo + hi) / 2.0;
    let mut cut = 0.0;
    each(t, i, |c| {
      if shrinks(c) {
        cut += (c.size[ax] - cap.max(c.min[ax])).max(0.0);
      }
    });
    if cut > over { lo = cap } else { hi = cap }
  }

  each(t, i, |c| {
    if shrinks(c) {
      c.size[ax] = c.size[ax].min(hi.max(c.min[ax]));
    }
  });
}

/// Text and image sizes from the runtime's text system and images.
pub(crate) struct Sys<'a> {
  pub text: &'a mut TextSystem,
  pub images: &'a Images,
  pub scale: f32,
}

impl Measure for Sys<'_> {
  fn width(&mut self, s: &str, font: Font) -> f32 {
    self.text.measure(s, font)
  }

  fn line(&mut self, s: &str, font: Font) -> (f32, Ink) {
    let line = self.text.layout(s, font);
    (line.width, line.ink)
  }

  fn metrics(&mut self, font: Font) -> Metrics {
    self.text.metrics(font)
  }

  fn image(&mut self, id: ImageId) -> Size {
    let (w, h) = self.images.size(id).unwrap_or((0, 0));
    Size::new(w as f32 / self.scale, h as f32 / self.scale)
  }

  fn stops(&mut self, s: &str, font: Font, base: u32, out: &mut Vec<Stop>) {
    self.text.carets(s, font, |i, x| out.push(Stop { at: base + i as u32, x }));
  }
}

// MARK: Text

/// Wraps every text element at its final width and sets its line metrics; editable ones get
/// lines that cover their text and caret stops.
fn wrap(t: &mut Tree, m: &mut dyn Measure) {
  t.lines.clear();
  t.stops.clear();
  let mut edits = t.edits.iter_mut().peekable();
  for (i, n) in t.nodes.iter_mut().enumerate() {
    let width = n.size[0];
    let Kind::Text { range, style, lines, baseline, line_h } = &mut n.kind else { continue };
    let met = m.metrics(style.font);
    *line_h = if style.line_h > 0.0 { style.line_h } else { met.line_height() };
    *baseline = (*line_h - met.ascent - met.descent) / 2.0 + met.ascent;
    let start = t.lines.len() as u32;
    let s = &t.text[range.0 as usize..range.1 as usize];
    let edit = edits.next_if(|e| e.node == i as u32).filter(|e| !e.placeholder);
    // One line that fits (or doesn't wrap): its width is the one measured for the fit.
    let fits = !style.wrap || n.content[0] <= width + EPS;
    let cut = style.ellipsis && edit.is_none() && n.content[0] > width + EPS;
    let (start_at, end, cover) = (range.0, range.1, edit.is_some());
    match fits && !s.contains('\n') {
      // Paint adds the ellipsis after a line that ends before its text does.
      true if cut => {
        let (end, w) = ellipsize(s, style.font, width, m);
        let ink = m.line(&s[..end], style.font).1;
        t.lines.push(Line { start: start_at, end: start_at + end as u32, width: w, ink, stop: 0 })
      }
      true => {
        let ink = m.line(s, style.font).1;
        t.lines.push(Line { start: start_at, end, width: n.content[0], ink, stop: 0 })
      }
      false => wrap_text(&t.text, *range, *style, width, cover, m, &mut t.lines),
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

/// The longest prefix of `s` that fits `width` with an ellipsis after it (trailing spaces
/// dropped), as its length in bytes and its width with the ellipsis. It ends between graphemes:
/// an emoji sequence or a syllable stays whole.
fn ellipsize(s: &str, font: Font, width: f32, m: &mut dyn Measure) -> (usize, f32) {
  let dots = m.width(ELLIPSIS, font);
  let bounds: Vec<usize> = std::iter::once(0).chain(graphemes(s).map(|g| g.end)).collect();

  // Binary search over the graphemes: `lo` fits, `hi` doesn't.
  let (mut lo, mut hi) = (0, bounds.len());
  while hi - lo > 1 {
    let mid = (lo + hi) / 2;
    match m.width(&s[..bounds[mid]], font) + dots <= width + EPS {
      true => lo = mid,
      false => hi = mid,
    }
  }

  let end = s[..bounds[lo]].trim_end().len();
  (end, m.width(&s[..end], font) + dots)
}

/// Breaks the text at `range` into lines no wider than `width`: at newlines, and greedily at
/// spaces when it wraps. A word wider than `width` starts a line and breaks between characters
/// (graphemes), as AppKit's word wrapping does, each line at least one character. With `cover`
/// (an editable text) the same breaks give lines that cover the text: each up to where the next
/// starts, with the spaces it broke at, the first from the line break before it.
fn wrap_text(
  text: &str,
  range: (u32, u32),
  style: TextStyle,
  width: f32,
  cover: bool,
  m: &mut dyn Measure,
  out: &mut Vec<Line>,
) {
  let font = style.font;
  let mut at = range.0 as usize;
  for line in text[at..range.1 as usize].split('\n') {
    let base = at;
    at += line.len() + 1;
    let (w, ink) = m.line(line, font);
    if !style.wrap || w <= width + EPS {
      let end = (base + line.len()) as u32;
      out.push(Line { start: base as u32, end, width: w, ink, stop: 0 });
      continue;
    }

    let space = m.width(" ", font);
    let mut push = |m: &mut dyn Measure, a: usize, b: usize| {
      let (width, ink) = m.line(&line[a..b], font);
      out.push(Line { start: (base + a) as u32, end: (base + b) as u32, width, ink, stop: 0 });
    };
    // The line so far: start, end and width.
    let mut cur: Option<(usize, usize, f32)> = None;
    let mut pos = 0;
    for word in line.split(' ') {
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
        let pw = m.width(&line[..b], font);
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

          (from, b, m.width(&line[from..b], font))
        }
      });
    }

    let (start, end) = cur.map_or((0, 0), |(a, b, _)| (a, b));
    push(m, start, if cover { line.len() } else { end });
  }
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

// MARK: Positions

/// Top-down: places the children of each box along its axis with its alignment, overlays at
/// their target, then snaps every box's edges to pixels.
fn place(t: &mut Tree, win: Size, scale: f32) {
  t.nodes[0].pos = [0.0; 2];
  for i in 0..t.nodes.len() {
    let n = &mut t.nodes[i];
    if n.end as usize == i + 1 {
      // No children (a list emptied): its offset is clamped to what it has, and kept so.
      let (content, size) = (n.content, n.size);
      n.scroll = clamp_scroll(n.scroll, n.style.scroll, content, size);
      continue;
    }

    if n.cols > 0 {
      grid_place(t, i, win);
      continue;
    }

    let (ax, cr) = (n.axis as usize, 1 - n.axis as usize);
    let (pos, size, gap, align, style) = (n.pos, n.size, n.style.gap, n.style.align, n.style);
    let ((a, b), (ca, cb)) = (pads(&n.style, ax), pads(&n.style, cr));
    let inner = (size[ax] - a - b, size[cr] - ca - cb);

    let (mut used, mut across, mut count) = (0.0, 0.0f32, 0);
    each(t, i, |c| {
      if c.float.is_none() {
        used += c.size[ax];
        across = across.max(c.size[cr]);
        count += 1;
      }
    });
    if count > 1 {
      used += gap * (count - 1) as f32;
    }
    // The content at its final size; a scroll container's offset is clamped to it.
    let n = &mut t.nodes[i];
    (n.content[ax], n.content[cr]) = (used + a + b, across + ca + cb);
    n.scroll = clamp_scroll(n.scroll, style.scroll, n.content, size);
    let scroll = [n.scroll.x, n.scroll.y];
    let mut cur = pos[ax] + a + offset(align[ax], inner.0 - used) - scroll[ax];
    each(t, i, |c| {
      if let Some(f) = c.float {
        place_float(c, f, pos, size, win);
        return;
      }

      c.pos[ax] = cur;
      cur += c.size[ax] + gap;
      c.pos[cr] = pos[cr] + ca + offset(align[cr], inner.1 - c.size[cr]) - scroll[cr];
    });
  }

  let snap = |v: f32| if scale > 0.0 { (v * scale).round() / scale } else { v };
  for n in &mut t.nodes {
    let (x, y) = (snap(n.pos[0]), snap(n.pos[1]));
    let (r, b) = (snap(n.pos[0] + n.size[0]), snap(n.pos[1] + n.size[1]));
    n.rect = Rect::new(x, y, r - x, b - y);
  }
}

/// A scroll offset within `content` seen through `size`, 0 along an axis that doesn't scroll.
fn clamp_scroll(off: Point, scrolls: [bool; 2], content: [f32; 2], size: [f32; 2]) -> Point {
  let at = |k: usize, v: f32| match scrolls[k] {
    true => v.clamp(0.0, (content[k] - size[k]).max(0.0)),
    false => 0.0,
  };
  Point::new(at(0, off.x), at(1, off.y))
}

/// Places overlay `c` on its target: the parent at `pos` of `size`, or the window.
fn place_float(c: &mut Node, f: Float, pos: [f32; 2], size: [f32; 2], win: Size) {
  let (tp, ts) = if f.window { ([0.0; 2], [win.w, win.h]) } else { (pos, size) };
  let off = [f.offset.x, f.offset.y];
  for k in 0..2 {
    c.pos[k] = tp[k] + ts[k] * f.attach[k] - c.size[k] * f.anchor[k] + off[k];
  }
}

// MARK: Grid

/// The next cell of a grid of `cols` columns after one at `col` (the column after it) and `row`,
/// for a child `span` columns wide: on this row if it fits, else at the start of the next.
fn cell(col: u32, row: u32, span: u8, cols: u32) -> (u32, u32, u32) {
  let s = (span as u32).clamp(1, cols);
  match col + s > cols {
    true => (0, row + 1, s),
    false => (col, row, s),
  }
}

/// Calls `f(child, col, row, span)` for each child of grid `i` in the flow, left to right,
/// wrapping to the next row when a child's span doesn't fit what is left of one.
fn each_cell(t: &mut Tree, i: usize, mut f: impl FnMut(&mut Node, u32, u32, u32)) {
  let cols = t.nodes[i].cols as u32;
  let (mut col, mut row) = (0, 0);
  each(t, i, |c| {
    if c.float.is_none() {
      let (at, r, s) = cell(col, row, c.span, cols);
      f(c, at, r, s);
      (col, row) = (at + s, r);
    }
  });
}

/// A grid's content size and minimum: equal columns as wide as the widest child per column it
/// spans, rows as high as their highest child.
fn grid_fit(t: &mut Tree, i: usize, ax: usize) -> (f32, f32) {
  let (cols, gap) = (t.nodes[i].cols as u32, t.nodes[i].style.gap);
  let (mut size, mut least) = (0.0f32, 0.0f32);
  if ax == 0 {
    each_cell(t, i, |c, _, _, s| {
      let per = |v: f32| (v - gap * (s - 1) as f32) / s as f32;
      (size, least) = (size.max(per(c.size[0])), least.max(per(c.min[0])));
    });
    let gaps = gap * (cols - 1) as f32;
    (size, least) = (size * cols as f32 + gaps, least * cols as f32 + gaps);
  } else {
    // Sums of the rows' highest, the last row's still open.
    let (mut row, mut hi, mut lo, mut rows) = (0, 0.0f32, 0.0f32, 0);
    each_cell(t, i, |c, _, r, _| {
      if r != row {
        (size, least, hi, lo, row) = (size + hi, least + lo, 0.0, 0.0, r);
      }

      (hi, lo, rows) = (hi.max(c.size[1]), lo.max(c.min[1]), r + 1);
    });
    let gaps = gap * rows.saturating_sub(1) as f32;
    (size, least) = (size + hi + gaps, least + lo + gaps);
  }

  let (a, b) = pads(&t.nodes[i].style, ax);
  (size + a + b, least + a + b)
}

/// Sizes a grid's children to their cells in `inner`: across, equal columns; down, rows as high
/// as their highest child plus an equal share of the height left. Overlays from their target.
/// A child's row height and top are kept in its `pos` for [grid_place].
fn grid_grow(t: &mut Tree, i: usize, ax: usize, inner: f32, win: [f32; 2]) {
  let (cols, gap, size) = (t.nodes[i].cols as u32, t.nodes[i].style.gap, t.nodes[i].size[ax]);
  each(t, i, |c| {
    if let Some(f) = c.float
      && !matches!(c.style.size[ax], Dim::Fit | Dim::Rigid)
    {
      c.size[ax] = fill(c, ax, if f.window { win[ax] } else { size });
    }
  });
  if ax == 0 {
    let cw = (inner - gap * (cols - 1) as f32) / cols as f32;
    each_cell(t, i, |c, _, _, s| c.size[0] = fill(c, 0, cw * s as f32 + gap * (s - 1) as f32));
    return;
  }

  let mut rows = std::mem::take(&mut t.rows);
  rows.clear();
  each_cell(t, i, |c, _, r, _| match rows.get_mut(r as usize) {
    Some(h) => *h = h.max(c.size[1]),
    None => rows.push(c.size[1]),
  });
  let n = rows.len().max(1) as f32;
  let free = inner - rows.iter().sum::<f32>() - gap * (n - 1.0);
  let extra = (free / n).max(0.0);
  let (mut top, mut at) = (0.0, 0);
  each_cell(t, i, |c, _, r, _| {
    while at < r {
      (top, at) = (top + rows[at as usize] + extra + gap, at + 1);
    }

    let h = rows[r as usize] + extra;
    c.size[1] = fill(c, 1, h);
    c.pos = [h, top];
  });
  t.rows = rows;
}

/// Places a grid's children in their cells, aligned in them by the grid's alignment, and its
/// overlays; sets its content size.
fn grid_place(t: &mut Tree, i: usize, win: Size) {
  let n = &t.nodes[i];
  let (pos, size, gap, align, cols) = (n.pos, n.size, n.style.gap, n.style.align, n.cols as u32);
  let ((a, b), (ca, cb)) = (pads(&n.style, 0), pads(&n.style, 1));
  let cw = (size[0] - a - b - gap * (cols - 1) as f32) / cols as f32;
  let mut bottom = 0.0f32;
  each(t, i, |c| {
    if let Some(f) = c.float {
      place_float(c, f, pos, size, win);
    }
  });
  // Each cell holds its row's height and top until placed. The columns fit the width, but a
  // child wider than its cell (a fixed one) goes past it.
  let x = |c: &Node, col: u32, s: u32| {
    let w = cw * s as f32 + gap * (s - 1) as f32;
    (cw + gap) * col as f32 + offset(align[0], w - c.size[0])
  };
  let mut right = size[0] - a - b;
  each_cell(t, i, |c, col, _, s| {
    bottom = bottom.max(c.pos[1] + c.pos[0]);
    right = right.max(x(c, col, s) + c.size[0]);
  });
  // The offset is clamped to the content.
  let n = &mut t.nodes[i];
  n.content = [right + a + b, bottom + ca + cb];
  n.scroll = clamp_scroll(n.scroll, n.style.scroll, n.content, size);
  let (dx, dy) = (n.scroll.x, n.scroll.y);
  each_cell(t, i, |c, col, _, s| {
    let (h, top) = (c.pos[0], c.pos[1]);
    c.pos[0] = pos[0] + a + x(c, col, s) - dx;
    c.pos[1] = pos[1] + ca + top + offset(align[1], h - c.size[1]) - dy;
  });
}

fn offset(align: Align, free: f32) -> f32 {
  let free = free.max(0.0);
  match align {
    Align::Start => 0.0,
    Align::Center => free / 2.0,
    Align::End => free,
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use std::time::Duration;

  use super::*;
  use crate::ui::{Align, Float, Ui, UiState, grow, pct, px, rigid};

  /// Every character is 0.6 of the font size wide; ascent 0.8, descent 0.2.
  pub(crate) struct Fake;

  #[test]
  fn node_fits_five_cache_lines() {
    assert_eq!(size_of::<Node>(), 320);
  }

  impl Measure for Fake {
    fn width(&mut self, s: &str, font: Font) -> f32 {
      s.chars().count() as f32 * font.size * 3.0 / 5.0
    }

    fn metrics(&mut self, font: Font) -> Metrics {
      let (ascent, descent, leading) = (font.size * 4.0 / 5.0, font.size / 5.0, 0.0);
      let (cap_height, x_height) = (font.size * 0.7, font.size / 2.0);
      Metrics { ascent, descent, leading, cap_height, x_height }
    }

    fn image(&mut self, _: ImageId) -> Size {
      Size::new(32.0, 16.0)
    }
  }

  /// 6 points per character, 10 per line.
  const T: TextStyle = TextStyle::new(Font::ui(10.0), [1.0; 4]);

  fn run(w: f32, h: f32, scale: f32, f: impl FnOnce(&mut Ui)) -> Tree {
    let mut ui = Ui::new(UiState::default(), Size::new(w, h), None, Duration::ZERO);
    f(&mut ui);
    ui.layout(&mut Fake, scale);
    ui.finish().last
  }

  fn rect(t: &Tree, name: &str) -> (f32, f32, f32, f32) {
    let r = t.rect(Id::of(name)).unwrap_or_else(|| panic!("no {name}"));
    (r.x, r.y, r.w, r.h)
  }

  fn el(ui: &mut Ui, name: &str, style: Style) {
    ui.id(name).el(style);
  }

  #[test]
  fn grid_cells_spans_and_rows() {
    let key = Style::new().w(grow()).h(grow()).min_h(20.0);
    let t = run(400.0, 300.0, 1.0, |ui| {
      let s = Style::new().w(px(330.0)).h(px(150.0)).gap(10.0).center();
      ui.id("grid").grid(4, s, |ui| {
        for k in 0..4 {
          el(ui, &format!("k{k}"), key);
        }

        ui.id("wide").span(2).el(key);
        el(ui, "k5", key);
        // Fixed: centred in its cell.
        el(ui, "small", Style::new().w(px(20.0)).h(px(10.0)));
        // No room left on the row: the next one.
        ui.id("next").span(3).el(key.min_h(0.0));
      });
      // A fit grid: columns as wide as the widest cell, rows as high as their highest.
      ui.id("fit").grid(2, Style::new().gap(4.0), |ui| {
        el(ui, "a", Style::new().w(px(30.0)).h(px(10.0)));
        el(ui, "b", Style::new().w(px(10.0)).h(px(15.0)));
        ui.id("c").span(2).el(Style::new().w(px(80.0)).h(px(5.0)));
      });
    });
    // Columns (330 - 3 × 10) / 4 = 75; rows 20, 20 and 0 high, and 150 - 40 - 20 of gaps
    // leaves 90: 30 more each.
    assert_eq!(rect(&t, "k0"), (0.0, 0.0, 75.0, 50.0));
    assert_eq!(rect(&t, "k3"), (255.0, 0.0, 75.0, 50.0));
    assert_eq!(rect(&t, "wide"), (0.0, 60.0, 160.0, 50.0));
    assert_eq!(rect(&t, "k5"), (170.0, 60.0, 75.0, 50.0));
    assert_eq!(rect(&t, "small"), (283.0, 80.0, 20.0, 10.0));
    assert_eq!(rect(&t, "next"), (0.0, 120.0, 245.0, 30.0));
    let g = t.get(Id::of("grid")).unwrap();
    assert_eq!(g.content, [330.0, 150.0]);
    // 80 over two columns and a gap: 38 each; rows 15 and 5.
    assert_eq!(rect(&t, "fit"), (0.0, 150.0, 80.0, 24.0));
    assert_eq!(rect(&t, "b"), (42.0, 150.0, 10.0, 15.0));
    assert_eq!(rect(&t, "c"), (0.0, 169.0, 80.0, 5.0));
  }

  #[test]
  fn fit_wraps_children_padding_and_gap() {
    let t = run(400.0, 300.0, 1.0, |ui| {
      ui.id("row").row(Style::new().pad(10.0).gap(5.0), |ui| {
        el(ui, "a", Style::new().w(px(20.0)).h(px(10.0)));
        el(ui, "b", Style::new().w(px(30.0)).h(px(40.0)));
      });
    });
    assert_eq!(rect(&t, "row"), (0.0, 0.0, 75.0, 60.0));
    assert_eq!(rect(&t, "a"), (10.0, 10.0, 20.0, 10.0));
    assert_eq!(rect(&t, "b"), (35.0, 10.0, 30.0, 40.0));
    assert_eq!(t.get(Id::of("row")).unwrap().content, [75.0, 60.0]);
  }

  #[test]
  fn grow_shares_by_weight() {
    let t = run(400.0, 300.0, 1.0, |ui| {
      ui.row(Style::new().w(px(300.0)).h(px(50.0)), |ui| {
        el(ui, "a", Style::new().w(grow()));
        el(ui, "b", Style::new().w(Dim::Grow(2.0)).h(grow()));
        el(ui, "c", Style::new().w(px(60.0)).h(px(10.0)));
      });
    });
    assert_eq!(rect(&t, "a"), (0.0, 0.0, 80.0, 0.0));
    assert_eq!(rect(&t, "b"), (80.0, 0.0, 160.0, 50.0));
    assert_eq!(rect(&t, "c"), (240.0, 0.0, 60.0, 10.0));
  }

  #[test]
  fn percent_of_inner_size_less_gaps() {
    let t = run(400.0, 300.0, 1.0, |ui| {
      ui.row(Style::new().w(px(240.0)).pad_xy(10.0, 0.0).gap(10.0), |ui| {
        el(ui, "a", Style::new().w(pct(50.0)).h(pct(100.0)));
        el(ui, "b", Style::new().w(pct(25.0)));
        el(ui, "c", Style::new().w(px(10.0)).h(px(20.0)));
      });
    });
    // 240 - 20 padding - 20 gaps = 200; across, 100% of the row's fit height.
    assert_eq!(rect(&t, "a"), (10.0, 0.0, 100.0, 20.0));
    assert_eq!(rect(&t, "b"), (120.0, 0.0, 50.0, 0.0));
  }

  #[test]
  fn min_and_max_clamp() {
    let t = run(400.0, 300.0, 1.0, |ui| {
      ui.col(Style::new().w(px(300.0)), |ui| {
        ui.row(Style::new().w(grow()), |ui| {
          el(ui, "a", Style::new().w(grow()).max_w(50.0));
          el(ui, "b", Style::new().w(grow()));
        });
        ui.row(Style::new().w(grow()), |ui| {
          el(ui, "c", Style::new().w(grow()).min_w(200.0));
          el(ui, "d", Style::new().w(grow()));
          el(ui, "e", Style::new().w(grow()));
        });
        el(ui, "f", Style::new().w(px(500.0)).max_w(120.0).min_h(7.0));
        el(ui, "g", Style::new().w(grow()).max_w(100.0));
      });
    });
    assert_eq!(rect(&t, "a").2, 50.0);
    assert_eq!(rect(&t, "b"), (50.0, 0.0, 250.0, 0.0));
    assert_eq!((rect(&t, "c").2, rect(&t, "d").2, rect(&t, "e").2), (200.0, 50.0, 50.0));
    assert_eq!(rect(&t, "f"), (0.0, 0.0, 120.0, 7.0));
    assert_eq!(rect(&t, "g").2, 100.0);
  }

  /// A maximum that frees space gives it to a sibling held at its minimum.
  #[test]
  fn a_max_frees_space_for_a_min() {
    let t = run(400.0, 300.0, 1.0, |ui| {
      ui.row(Style::new().w(px(100.0)), |ui| {
        el(ui, "a", Style::new().w(grow()).min_w(80.0));
        el(ui, "b", Style::new().w(grow()).max_w(10.0));
      });
    });
    assert_eq!((rect(&t, "a").2, rect(&t, "b").2), (90.0, 10.0));
  }

  #[test]
  fn alignment() {
    let t = run(400.0, 300.0, 1.0, |ui| {
      let boxed = Style::new().w(px(200.0)).h(px(100.0));
      let child = Style::new().w(px(50.0)).h(px(20.0));
      ui.row(boxed.center(), |ui| el(ui, "a", child));
      ui.row(boxed.align(Align::End, Align::End).pad(10.0), |ui| el(ui, "b", child));
      ui.col(boxed.align(Align::End, Align::Center).gap(10.0), |ui| {
        el(ui, "c", child);
        el(ui, "d", child);
      });
    });
    assert_eq!(rect(&t, "a"), (75.0, 40.0, 50.0, 20.0));
    assert_eq!(rect(&t, "b"), (140.0, 170.0, 50.0, 20.0));
    // Column: 2 x 20 + 10 gap = 50 of 100, centred vertically, at the right.
    assert_eq!(rect(&t, "c"), (150.0, 225.0, 50.0, 20.0));
    assert_eq!(rect(&t, "d"), (150.0, 255.0, 50.0, 20.0));
  }

  #[test]
  fn nested_rows_and_columns() {
    let t = run(400.0, 300.0, 1.0, |ui| {
      ui.id("app").col(Style::new().grow(), |ui| {
        ui.id("bar").row(Style::new().w(grow()).h(px(40.0)).pad(5.0), |ui| {
          el(ui, "l", Style::new().w(px(30.0)).h(grow()));
          ui.spacer();
          el(ui, "r", Style::new().w(px(30.0)).h(grow()));
        });
        ui.id("body").row(Style::new().grow(), |ui| {
          el(ui, "side", Style::new().w(px(100.0)).h(grow()));
          ui.id("main").col(Style::new().grow().pad(10.0).gap(10.0), |ui| {
            el(ui, "m1", Style::new().w(grow()).h(px(50.0)));
            el(ui, "m2", Style::new().grow());
          });
        });
      });
    });
    assert_eq!(rect(&t, "app"), (0.0, 0.0, 400.0, 300.0));
    assert_eq!(rect(&t, "l"), (5.0, 5.0, 30.0, 30.0));
    assert_eq!(rect(&t, "r"), (365.0, 5.0, 30.0, 30.0));
    assert_eq!(rect(&t, "body"), (0.0, 40.0, 400.0, 260.0));
    assert_eq!(rect(&t, "side"), (0.0, 40.0, 100.0, 260.0));
    assert_eq!(rect(&t, "main"), (100.0, 40.0, 300.0, 260.0));
    assert_eq!(rect(&t, "m1"), (110.0, 50.0, 280.0, 50.0));
    assert_eq!(rect(&t, "m2"), (110.0, 110.0, 280.0, 180.0));
  }

  #[test]
  fn text_wraps_and_sets_height() {
    fn lines<'t>(t: &'t Tree, name: &str) -> Vec<&'t str> {
      let Kind::Text { lines: (a, b), .. } = t.get(Id::of(name)).unwrap().kind else { panic!() };
      t.lines[a as usize..b as usize].iter().map(|l| t.str(l.start, l.end)).collect()
    }
    let build = |w: f32| {
      run(400.0, 300.0, 1.0, move |ui| {
        ui.id("col").col(Style::new().w(px(w)).pad(2.0), |ui| {
          ui.id("t").text("aaaa bbbb  cccc\ndd", T);
        });
      })
    };
    // 15 characters, 90 points: fits in 100 - 4 padding.
    let t = build(100.0);
    assert_eq!(lines(&t, "t"), ["aaaa bbbb  cccc", "dd"]);
    assert_eq!(rect(&t, "t"), (2.0, 2.0, 90.0, 20.0));
    assert_eq!(rect(&t, "col").3, 24.0);
    // 60 - 4 = 56 wide: "aaaa bbbb" is 54, then two spaces and "cccc" go to the next line.
    let t = build(60.0);
    assert_eq!(lines(&t, "t"), ["aaaa bbbb", "cccc", "dd"]);
    assert_eq!(rect(&t, "t"), (2.0, 2.0, 56.0, 30.0));
    assert_eq!(rect(&t, "col").3, 34.0);
    assert_eq!(t.lines[0].width, 54.0);
    // Narrower than a word: each word starts a line and breaks between characters.
    let t = build(20.0);
    assert_eq!(lines(&t, "t"), ["aa", "aa", "bb", "bb", "cc", "cc", "dd"]);
    assert_eq!(rect(&t, "t").2, 16.0);
    // Narrower than a character: one a line, overflowing.
    let t = build(7.0);
    assert_eq!(lines(&t, "t")[..3], ["a", "a", "a"]);
    assert_eq!(rect(&t, "t").2, 6.0);
  }

  /// [Fake], counting the characters it measures.
  struct Counted(usize);

  impl Measure for Counted {
    fn width(&mut self, s: &str, font: Font) -> f32 {
      self.0 += s.chars().count();
      Fake.width(s, font)
    }

    fn metrics(&mut self, font: Font) -> Metrics {
      Fake.metrics(font)
    }

    fn image(&mut self, id: ImageId) -> Size {
      Fake.image(id)
    }
  }

  #[test]
  fn a_long_word_wraps_in_linear_work() {
    // 10 characters a line: each line measures about its own, not the rest of the word.
    let measured = |n: usize| {
      let text = "a".repeat(n);
      let (mut m, mut out) = (Counted(0), Vec::new());
      wrap_text(&text, (0, n as u32), T, 60.0, false, &mut m, &mut out);
      assert_eq!(out.len(), n.div_ceil(10));
      assert!(out.iter().all(|l| l.end - l.start == 10 || l.end == n as u32));
      m.0
    };
    let (a, b) = (measured(1000), measured(4000));
    assert!(b < 30 * 4000, "{b} characters measured for 4000");
    assert!(b < 5 * a, "{a} for 1000, {b} for 4000");
  }

  /// 'W' is 12 points wide, 'i' 2, the rest 6.
  struct Narrow;

  impl Measure for Narrow {
    fn width(&mut self, s: &str, _: Font) -> f32 {
      s.chars()
        .map(|c| {
          if c == 'W' {
            12.0
          } else if c == 'i' {
            2.0
          } else {
            6.0
          }
        })
        .sum()
    }

    fn metrics(&mut self, font: Font) -> Metrics {
      Fake.metrics(font)
    }

    fn image(&mut self, id: ImageId) -> Size {
      Fake.image(id)
    }
  }

  #[test]
  fn wrapped_text_is_never_narrower_than_its_widest_character() {
    // The widest word is "iiiiiiii" (16), its widest character 2; "W" is 12.
    assert_eq!(text_width("iiiiiiii W", T, &mut Narrow), (34.0, 12.0));
  }

  #[test]
  fn an_ellipsis_keeps_graphemes_whole() {
    // "ae" and the ellipsis fit 18, but the accent belongs to the "e": only "a" stays.
    assert_eq!(ellipsize("ae\u{301}bc", T.font, 18.0, &mut Fake), (1, 12.0));
    assert_eq!(ellipsize("ae\u{301}bc", T.font, 24.0, &mut Fake), (4, 24.0));
  }

  #[test]
  fn a_long_word_breaks_and_the_rest_follows_it() {
    let t = run(400.0, 300.0, 1.0, |ui| {
      ui.col(Style::new().w(px(60.0)), |ui| ui.id("t").text("ab cdefghijklmn op q", T));
    });
    let Kind::Text { lines: (a, b), .. } = t.get(Id::of("t")).unwrap().kind else { panic!() };
    let lines: Vec<&str> =
      t.lines[a as usize..b as usize].iter().map(|l| t.str(l.start, l.end)).collect();
    // 10 characters a line: the long word on a line of its own, its end then "op q".
    assert_eq!(lines, ["ab", "cdefghijkl", "mn op q"]);
  }

  #[test]
  fn nowrap_and_line_height() {
    let t = run(400.0, 300.0, 1.0, |ui| {
      ui.col(Style::new().w(px(30.0)), |ui| {
        ui.id("a").text("aa bb cc", T.nowrap());
        ui.id("b").text("x", T.line_h(16.0));
      });
    });
    assert_eq!(rect(&t, "a"), (0.0, 0.0, 48.0, 10.0));
    assert_eq!(rect(&t, "b"), (0.0, 10.0, 6.0, 16.0));
    // Baseline: half the extra line height above the ascent.
    let Kind::Text { baseline, .. } = t.get(Id::of("b")).unwrap().kind else { panic!() };
    assert_eq!(baseline, 3.0 + 8.0);
  }

  #[test]
  fn ellipsis_cuts_a_line_that_does_not_fit() {
    let t = run(400.0, 300.0, 1.0, |ui| {
      ui.row(Style::new().w(px(40.0)), |ui| ui.id("a").text("abc defghij", T.ellipsis()));
      ui.row(Style::new().w(px(100.0)), |ui| ui.id("b").text("abc", T.ellipsis()));
      ui.row(Style::new().w(px(40.0)), |ui| {
        el(ui, "fixed", Style::new().w(px(36.0)));
        ui.id("c").text("abc", T.ellipsis());
      });
    });
    let line = |name: &str| {
      let Kind::Text { lines, .. } = t.get(Id::of(name)).unwrap().kind else { panic!() };
      let l = t.lines[lines.0 as usize];
      (t.str(l.start, l.end), l.width)
    };
    // 66 wide in 40: "abc d" with "…" is 36; the space before a cut goes.
    assert_eq!(rect(&t, "a").2, 40.0);
    assert_eq!(line("a"), ("abc d", 36.0));
    assert_eq!(line("b"), ("abc", 18.0));
    // Shrinks down to the ellipsis alone, then overflows.
    assert_eq!(rect(&t, "c").2, 6.0);
    assert_eq!(line("c"), ("", 6.0));
    let t = run(400.0, 300.0, 1.0, |ui| {
      ui.row(Style::new().w(px(32.0)), |ui| ui.id("a").text("abc defghij", T.ellipsis()));
    });
    let Kind::Text { lines, .. } = t.get(Id::of("a")).unwrap().kind else { panic!() };
    let l = t.lines[lines.0 as usize];
    assert_eq!((t.str(l.start, l.end), l.width), ("abc", 24.0));
  }

  #[test]
  fn overflow_shrinks_the_widest_first() {
    let t = run(400.0, 300.0, 1.0, |ui| {
      ui.id("row").row(Style::new().w(px(100.0)), |ui| {
        // 78 wide, at least 60; 30 wide, at least 12; 108 > 100.
        ui.id("a").text("aaaaaaaaaa bb", T);
        ui.id("b").text("cc dd", T);
      });
      ui.row(Style::new().w(px(100.0)), |ui| {
        el(ui, "c", Style::new().w(px(80.0)));
        ui.id("d").text("dddddd eeeeee", T);
      });
    });
    assert_eq!(rect(&t, "a"), (0.0, 0.0, 70.0, 20.0));
    assert_eq!(rect(&t, "b"), (70.0, 0.0, 30.0, 10.0));
    assert_eq!(rect(&t, "row").3, 20.0);
    // Fixed sizes don't shrink; text takes the rest, breaking its words: "ddd", "ddd", "eee",
    // "eee".
    assert_eq!(rect(&t, "c").2, 80.0);
    assert_eq!(rect(&t, "d"), (80.0, 20.0, 20.0, 40.0));
  }

  #[test]
  fn rigid_keeps_its_content_and_the_others_give_way() {
    let build = |caption: &'static str| {
      run(400.0, 300.0, 1.0, move |ui| {
        ui.id("row").row(Style::new().w(px(120.0)), |ui| {
          ui.id("label").col(Style::new().w(grow()), |ui| ui.id("t").text(caption, T));
          // 60 wide, its text could end in "…" at 6.
          ui.id("ctl").row(Style::new().w(rigid()), |ui| ui.text("cccccccccc", T.ellipsis()));
        });
      })
    };
    // 120 - 60 = 60 for the label: its 10 words wrap two to a line, the row grows, the control
    // stays.
    let t = build("aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj");
    assert_eq!(rect(&t, "ctl"), (60.0, 0.0, 60.0, 10.0));
    assert_eq!(rect(&t, "label"), (0.0, 0.0, 60.0, 50.0));
    assert_eq!(rect(&t, "row").3, 50.0);
    assert_eq!(rect(&build("a"), "ctl"), (60.0, 0.0, 60.0, 10.0));
    // A word longer than the label's 60 (a path without spaces) breaks inside: the label keeps
    // its 60, the control its place in the row.
    let t = build("~/Library/Application/com.example.app/logs b");
    assert_eq!(rect(&t, "label"), (0.0, 0.0, 60.0, 50.0));
    assert_eq!(rect(&t, "ctl"), (60.0, 0.0, 60.0, 10.0));
    let Kind::Text { lines: (a, b), .. } = t.get(Id::of("t")).unwrap().kind else { panic!() };
    assert!(t.lines[a as usize..b as usize].iter().all(|l| l.width <= 60.0));
  }

  #[test]
  fn cross_axis_fit_shrinks_to_parent() {
    let t = run(400.0, 300.0, 1.0, |ui| {
      ui.col(Style::new().w(px(70.0)), |ui| {
        ui.id("card").col(Style::new().pad(5.0), |ui| {
          ui.id("t").text("aaaa bbbb cccc", T);
        });
      });
    });
    assert_eq!(rect(&t, "card"), (0.0, 0.0, 70.0, 30.0));
    assert_eq!(rect(&t, "t"), (5.0, 5.0, 60.0, 20.0));
  }

  #[test]
  fn clip_box_shrinks_below_content() {
    let t = run(400.0, 300.0, 1.0, |ui| {
      ui.row(Style::new().w(px(100.0)), |ui| {
        ui.id("a").row(Style::new().clip(), |ui| el(ui, "x", Style::new().w(px(80.0))));
        el(ui, "b", Style::new().w(px(60.0)));
      });
    });
    assert_eq!(rect(&t, "a").2, 40.0);
    assert_eq!(t.get(Id::of("a")).unwrap().content[0], 80.0);
    assert_eq!(rect(&t, "x").2, 80.0);
  }

  #[test]
  fn images_fit_their_natural_size() {
    let t = run(400.0, 300.0, 1.0, |ui| {
      ui.id("a").image(ImageId(0), Style::new());
      ui.id("b").image(ImageId(0), Style::new().w(px(64.0)).h(px(64.0)));
    });
    assert_eq!(rect(&t, "a"), (0.0, 0.0, 32.0, 16.0));
    assert_eq!(rect(&t, "b"), (0.0, 16.0, 64.0, 64.0));
  }

  #[test]
  fn overlays() {
    let t = run(400.0, 300.0, 1.0, |ui| {
      ui.row(Style::new().pad(10.0).gap(10.0), |ui| {
        el(ui, "x", Style::new().w(px(10.0)).h(px(10.0)));
        ui.id("p").row(Style::new().w(px(100.0)).h(px(20.0)), |ui| {
          // Under the parent, centred, 4 points below; not part of its fit size.
          let tip = Float::parent().attach(0.5, 1.0).anchor(0.5, 0.0).offset(0.0, 4.0).z(1);
          ui.id("tip").float(tip).col(Style::new().pad(5.0), |ui| {
            ui.text("tip", T);
            // An overlay of its own paints over it, though its z is lower.
            ui.id("inner").float(Float::parent()).el(Style::new().w(px(4.0)).h(px(4.0)));
          });
          // Covers the parent.
          ui.id("cover").float(Float::parent()).el(Style::new().grow());
        });
        // At the window's bottom-right corner, sized by percent of the window.
        let corner = Float::window().attach(1.0, 1.0).anchor(1.0, 1.0).z(-1);
        ui.id("win").float(corner).el(Style::new().w(pct(10.0)).h(px(500.0)).max_h(30.0));
      });
    });
    assert_eq!(rect(&t, "p"), (30.0, 10.0, 100.0, 20.0));
    assert_eq!(rect(&t, "tip"), (66.0, 34.0, 28.0, 20.0));
    assert_eq!(rect(&t, "cover"), (30.0, 10.0, 100.0, 20.0));
    assert_eq!(rect(&t, "win"), (360.0, 270.0, 40.0, 30.0));
    // The row fits only its flow children.
    assert_eq!(t.nodes[1].rect, Rect::new(0.0, 0.0, 140.0, 40.0));
    // Paint order: by z, raised to the outer overlay's, then tree order.
    let order: Vec<_> = t.floats.iter().map(|&i| t.nodes[i as usize].id).collect();
    assert_eq!(order, [Id::of("win"), Id::of("cover"), Id::of("tip"), Id::of("inner")]);
  }

  #[test]
  fn ids_are_stable_and_keyed() {
    let build = |skip: bool| {
      run(400.0, 300.0, 1.0, move |ui| {
        ui.col(Style::new(), |ui| {
          if !skip {
            ui.el(Style::new());
          }

          ui.el(Style::new().w(px(1.0)));
          ui.key("k").el(Style::new().w(px(2.0)));
          ui.key(7).el(Style::new().w(px(3.0)));
        });
      })
    };
    let (a, b) = (build(false), build(true));
    let ids = |t: &Tree| t.nodes.iter().map(|n| n.id).collect::<Vec<_>>();
    // Positional ids follow the position; keyed ones stay.
    let col = Id::ROOT.nth(0);
    assert_eq!(ids(&a), [Id::ROOT, col, col.nth(0), col.nth(1), col.key("k"), col.key(7)]);
    assert_eq!(ids(&b)[2..], [col.nth(0), col.key("k"), col.key(7)]);
    assert_eq!(a.rect(col.key(7)).unwrap().w, 3.0);
    assert_ne!(col.key(0), col.nth(0));
    assert_ne!(Id::of("k"), col.key("k"));
    assert_eq!(a.children(1).collect::<Vec<_>>(), [2, 3, 4, 5]);
  }

  #[test]
  fn last_frame_rects() {
    let size = Size::new(100.0, 100.0);
    let mut ui = Ui::new(UiState::default(), size, None, Duration::ZERO);
    ui.id("a").el(Style::new().w(px(10.0)).h(px(10.0)));
    ui.layout(&mut Fake, 1.0);
    let st = ui.finish();
    let ui = Ui::new(st, size, None, Duration::ZERO);
    assert_eq!(ui.rect(Id::of("a")), Some(Rect::new(0.0, 0.0, 10.0, 10.0)));
    assert_eq!(ui.tree().nodes.len(), 1);
  }

  #[test]
  fn edges_snap_to_pixels() {
    let t = run(400.0, 300.0, 2.0, |ui| {
      ui.row(Style::new().w(px(100.0)), |ui| {
        for name in ["a", "b", "c"] {
          el(ui, name, Style::new().w(grow()).h(px(1.0)));
        }
      });
    });
    let (a, b, c) = (rect(&t, "a"), rect(&t, "b"), rect(&t, "c"));
    assert_eq!((a.0, a.2), (0.0, 33.5));
    assert_eq!((b.0, b.2), (33.5, 33.0));
    assert_eq!((c.0, c.2), (66.5, 33.5));
  }
}
