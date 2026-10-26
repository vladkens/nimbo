// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Columns built in view only ([Ui::virtual_col]): items of known heights inside the nearest
//! scroll area, of which only those it shows, and [MARGIN] around, are built, between two
//! spacers that keep the whole column's height. A page of 100k rows costs what the rows on
//! screen do, and the scrollbar, the wheel, paging and [Ui::scroll_to] are those of the whole.
//!
//! ```ignore
//! let heights = self.rows.iter().map(|r| if r.header { 40.0 } else { 52.0 });
//! let key = |i: usize| self.rows[i].id;
//! ui.id("history").virtual_col(heights, key, |ui, i| ui.label(&self.rows[i].text));
//! ```
//!
//! The column's place in its scroll area is the last frame's; when the layout shows more of it
//! than was built (the first frame, the viewport grew, content above moved it) the view runs
//! again at once ([Ui::partial_short]), so a frame never shows a gap. [List](super::List) is the
//! same for a list that is its own scroll area, with selection and keys.

use std::hash::Hash;
use std::ops::Range;

use super::super::{Id, Style, Ui, grow, px};
use crate::scene::Rect;

/// How far past the viewport items are built, in points: a wheel step and more.
const MARGIN: f64 = 200.0;

/// The passes a reveal may take to show its item.
const TRIES: u8 = 8;

/// Where a column's items lie, in points from its top (`f64`: a long column passes `f32`'s
/// whole numbers).
pub(crate) trait Spans {
  fn len(&self) -> usize;
  /// The top of item `i`; the column's height for `i == len()`.
  fn top(&self, i: usize) -> f64;
  /// The item at depth `y`: the first that ends below it, `len()` past the end.
  fn at(&self, y: f64) -> usize;
}

/// Items by where each ends.
struct Ends<'e>(&'e [f64]);

impl Spans for Ends<'_> {
  fn len(&self) -> usize {
    self.0.len()
  }

  fn top(&self, i: usize) -> f64 {
    if i == 0 { 0.0 } else { self.0[i - 1] }
  }

  fn at(&self, y: f64) -> usize {
    self.0.partition_point(|&e| e <= y)
  }
}

/// A column's state in the store: where its items end, for [Ui::virtual_col] (kept to build
/// them without allocating), and an item to reveal with the frames it may still take.
#[derive(Default)]
struct Lazy {
  ends: Vec<f64>,
  reveal: Option<(usize, u8)>,
}

impl<'a> Ui<'a> {
  /// A column of items `heights` high, each exactly that and clipped to it, built with
  /// `item(ui, i)` only for those the nearest vertically scrolling area shows and [MARGIN]
  /// around (all of them outside one); returns the items built. An item's box is keyed by
  /// `key(i)`, unique and stable while it is the same thing, so what it builds keeps its ids,
  /// hover and focus as the column scrolls and items come and go before it. The column's id is
  /// the one set ([Ui::id], [Ui::key]) or its place among its siblings, not taking input. The
  /// heights are summed each frame: for very long columns of sections, see [Ui::form_list].
  pub fn virtual_col<K: Hash>(
    &mut self,
    heights: impl IntoIterator<Item = f32>,
    key: impl Fn(usize) -> K,
    item: impl FnMut(&mut Ui<'a>, usize),
  ) -> Range<usize> {
    let id = self.take_id();
    let mut ends = std::mem::take(&mut self.state::<Lazy>(id).ends);
    ends.clear();
    let mut end = 0.0;
    ends.extend(heights.into_iter().map(|h| {
      end += h as f64;
      end
    }));

    let shown = self.lazy_col(id, &Ends(&ends), key, item);
    self.state::<Lazy>(id).ends = ends;
    shown
  }

  /// [Ui::virtual_col] over items laid out by `spans`, as column `id`.
  pub(crate) fn lazy_col<K: Hash>(
    &mut self,
    id: Id,
    spans: &dyn Spans,
    key: impl Fn(usize) -> K,
    mut item: impl FnMut(&mut Ui<'a>, usize),
  ) -> Range<usize> {
    // The scroll area around it, with its offset and viewport and the column's place in its
    // content as of the last frame (at its top before the column is laid out).
    let t = &self.st.tree;
    let area =
      self.st.stack.iter().rev().map(|&(i, _)| i).find(|&i| t.nodes[i as usize].style.scroll[1]);
    let area_id = area.map(|i| t.nodes[i as usize].id);
    let sc = area_id.and_then(|a| self.scroll(a));
    // Placed by the offset it was laid out at, seen through the offset the input set since.
    let laid = area_id.and_then(|a| self.st.last.get(a));
    let top = match (laid, self.rect(id)) {
      (Some(a), Some(r)) => (r.y - a.rect.y + a.scroll.y) as f64,
      _ => 0.0,
    };

    // An item to reveal is scrolled to until a layout after the request shows it as the scroll
    // would put it (a tall one from its top): the column may move under it meanwhile, or not be
    // laid out yet.
    let n = spans.len();
    if let Some((i, tries)) = self.state::<Lazy>(id).reveal.filter(|&(i, t)| i < n && t > 0) {
      let vp = sc.map(|sc| sc.viewport);
      let shows = tries < TRIES
        && (vp.zip(self.rect(id.key(key(i))))).is_some_and(|(vp, r)| {
          let (top, end) = (vp.y + 0.5, vp.y + vp.h + 0.5);
          r.y >= vp.y - 0.5 && (r.y + r.h <= end || r.y <= top)
        });
      self.state::<Lazy>(id).reveal = (!shows).then_some((i, tries - 1));
      if let (false, Some(sc)) = (shows, sc) {
        let (from, to) = (spans.top(i), spans.top(i + 1));
        let r = Rect::new(sc.offset.x, (top + from) as f32, 1.0, (to - from) as f32);
        self.reveal(area_id.unwrap(), r);
      }

      self.st.animating |= !shows;
    }

    // Before the area is laid out, a window's height from the top; outside one, everything.
    let (off, view) = match (area, sc) {
      (Some(_), Some(sc)) => (sc.offset.y as f64, sc.viewport.h as f64),
      (Some(_), None) => (0.0, self.size().h as f64),
      (None, _) => (0.0, f64::INFINITY),
    };

    let first = spans.at(off - top - MARGIN).min(n);
    let shown = first..(spans.at(off - top + view + MARGIN) + 1).min(n).max(first);
    let (from, to, total) = (spans.top(shown.start), spans.top(shown.end), spans.top(n));
    self.next_id = Some((id, false));
    self.col(Style::new().w(grow()), |ui| {
      if from > 0.0 {
        ui.el(Style::new().h(px(from as f32)));
      }

      for i in shown.clone() {
        let h = (spans.top(i + 1) - spans.top(i)) as f32;
        ui.key(key(i)).col(Style::new().w(grow()).h(px(h)).clip(), |ui| item(ui, i));
      }

      if total > to {
        ui.el(Style::new().h(px((total - to) as f32)));
      }

      // What the built items cover, from the column's top, for the runtime to run the view
      // again if the layout shows more (the ends are open where the column's are).
      if let Some(area) = area {
        let lo = if shown.start == 0 { f32::NEG_INFINITY } else { from as f32 };
        let hi = if shown.end == n { f32::INFINITY } else { to as f32 };
        let node = ui.st.stack.last().unwrap().0;
        ui.st.tree.partial.push((area, node, lo, hi));
      }
    });

    shown
  }

  /// Scrolls item `i` of virtual column `id` into view, the least that shows it, in the
  /// column's next build.
  pub fn reveal_item(&mut self, id: Id, i: usize) {
    self.state::<Lazy>(id).reveal = Some((i, TRIES));
  }
}

#[cfg(test)]
mod tests {
  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::Size;
  use crate::ui::{Id, Style, Ui, grow, px};

  /// A scroll area 300 high over a prefix `pre` high and a column of 50 items `h` high each, the
  /// item `reveal` asked for in the next view.
  struct Col {
    pre: f32,
    h: f32,
    reveal: Option<usize>,
  }

  impl App for Col {
    fn view(&mut self, ui: &mut Ui) {
      if let Some(i) = self.reveal.take() {
        ui.reveal_item(Id::of("col"), i);
      }

      ui.id("area").scroll_area(Style::new().w(grow()).h(px(300.0)), |ui| {
        ui.el(Style::new().h(px(self.pre)));
        ui.id("col").virtual_col((0..50).map(|_| self.h), |i| i, |_, _| {});
      });
    }
  }

  fn shown(h: &Harness<Col>, i: usize) -> Option<(f32, f32)> {
    let (vp, r) = (h.rect(Id::of("area"))?, h.rect(Id::of("col").key(i))?);
    Some((r.y - vp.y, r.y + r.h - vp.y))
  }

  fn settle(h: &mut Harness<Col>) {
    for _ in 0..6 {
      h.frame();
    }
  }

  /// Taller than the viewport, an item is revealed from its top.
  #[test]
  fn a_tall_item_is_revealed_from_its_top() {
    let mut h =
      Harness::new(Col { pre: 0.0, h: 400.0, reveal: None }, Size::new(200.0, 300.0), 1.0);
    h.frame();
    h.app_mut().reveal = Some(1);
    settle(&mut h);
    assert_eq!(shown(&h, 1).map(|s| s.0), Some(0.0));
  }

  /// An item in view when it is asked for, pushed out by content growing above the column in
  /// the same frame: it still shows.
  #[test]
  fn a_reveal_follows_the_column_as_content_above_it_moves() {
    let mut h = Harness::new(Col { pre: 0.0, h: 40.0, reveal: None }, Size::new(200.0, 300.0), 1.0);
    h.frame();
    assert!(shown(&h, 2).is_some_and(|(_, end)| end <= 300.0), "in view before");
    (h.app_mut().pre, h.app_mut().reveal) = (1000.0, Some(2));
    settle(&mut h);
    let (top, end) = shown(&h, 2).expect("built");
    assert!(top >= 0.0 && end <= 300.0, "{top}..{end}");
  }
}
