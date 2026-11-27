// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Columns built in view only ([Ui::lazy_col], SwiftUI's `LazyVStack`): items of any height
//! inside the nearest scroll area, of which only those it shows, and [MARGIN] around, are built,
//! between two spacers that keep the whole column's height. An item is as high as it lays out;
//! one not built yet counts as high as the first measured were on average, so a column of
//! 100k items costs what those on screen do, and the scrollbar, the wheel, paging,
//! [Ui::scroll_to] and [Ui::reveal_item] are those of the whole.
//!
//! ```ignore
//! ui.default_scroll_anchor(UnitPoint::BOTTOM).scroll_area(Style::new().grow(), |ui| {
//!   ui.id("history").lazy_col(self.msgs.len(), |i| self.msgs[i].id, |ui, i| bubble(ui, i));
//! });
//! ```
//!
//! Heights are measured as items show, by the layout, and kept per item key while the column's
//! width holds; a new width measures again those that show. When a measure replaces a guess
//! above what shows, the area's offset moves by the difference, so the content in view stays
//! put (an area held at its end by its anchor stays there instead).
//!
//! The column's place in its scroll area is the last frame's; when the layout shows more of it
//! than was built (the first frame, the viewport grew, measured items came out shorter than
//! guessed) the view runs again at once (`Ui::partial_short`), so a frame never shows a gap.
//! `List` is a list that is its own scroll area, of rows of one height, with selection and keys.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hash};
use std::ops::Range;

use super::input::{self, ScrollState};
use super::{Flow, Id, Kind, Style, Ui, grow, px};
use crate::text::FxHasher;

/// How far past the viewport items are built, in points: a wheel step and more.
const MARGIN: f64 = 200.0;

/// The passes a reveal may take to show its item.
const TRIES: u8 = 8;

/// An item's height before any is measured: a row of text.
const GUESS: f64 = 32.0;

/// A column's state in the store: the items' heights as measured or guessed, where each ends,
/// the items built last, and an item to reveal with the frames it may still take.
#[derive(Default)]
struct Lazy {
  /// Measured heights by item id, at `width`.
  seen: HashMap<Id, f64, BuildHasherDefault<FxHasher>>,
  width: f64,
  /// The height of an item not measured: the mean of the first measured at this width, or of
  /// the last width's until then.
  guess: Option<f64>,
  fresh: bool,
  /// By index: the height measured, NaN where none is; and where each item ends.
  heights: Vec<f64>,
  ends: Vec<f64>,
  /// The ids of the first and last items when `heights` was indexed: other ones mean the items
  /// changed, not only grew.
  edges: (Id, Id),
  /// The items built last pass, and the one at the viewport's top then: its index, id and
  /// top.
  built: Range<usize>,
  anchor: Option<(usize, Id, f64)>,
  /// The area's anchor held it at an end then.
  held: bool,
  reveal: Option<(usize, u8)>,
}

impl Lazy {
  /// The top of item `i`; the column's height for `i == len`.
  fn top(&self, i: usize) -> f64 {
    if i == 0 { 0.0 } else { self.ends[i - 1] }
  }

  /// The item at depth `y`: the first that ends below it, `len` past the end.
  fn at(&self, y: f64) -> usize {
    self.ends.partition_point(|&e| e <= y)
  }

  /// Indexes the heights for the `n` items `item` names: grown at the end, the old indices
  /// hold; else each is looked up by its id. Returns whether they changed.
  fn index(&mut self, n: usize, item: &impl Fn(usize) -> Id) -> bool {
    let edges = if n == 0 { (Id::ROOT, Id::ROOT) } else { (item(0), item(n - 1)) };
    let old = self.heights.len();
    if edges == self.edges && n == old {
      return false;
    }

    let grew = n > old && old > 0 && item(0) == self.edges.0 && item(old - 1) == self.edges.1;
    if !grew {
      self.heights.clear();
    }

    let from = self.heights.len();
    self.heights.extend((from..n).map(|i| self.seen.get(&item(i)).copied().unwrap_or(f64::NAN)));
    self.edges = edges;
    true
  }

  /// Where each item ends, by the heights measured and the guess for the rest.
  fn sum(&mut self) {
    let guess = self.guess.unwrap_or(GUESS);
    let mut end = 0.0;
    self.ends.clear();
    self.ends.extend(self.heights.iter().map(|&h| {
      end += if h.is_nan() { guess } else { h };
      end
    }));
  }
}

impl<'a> Ui<'a> {
  /// A column of `count` items, as SwiftUI's `LazyVStack`, each as high as `item(ui, i)` lays
  /// it out, built only for those the nearest vertically scrolling area shows and `MARGIN`
  /// around (all outside one); returns the items built. An item's box is keyed by `key(i)`,
  /// unique and stable while it is the same thing: it keeps its ids, hover, focus and measured
  /// height as the column scrolls and items come and go before it. The column's id is the one
  /// set ([Ui::id], [Ui::key]) or its place among its siblings, not taking input.
  pub fn lazy_col<K: Hash>(
    &mut self,
    count: usize,
    key: impl Fn(usize) -> K,
    mut item: impl FnMut(&mut Ui<'a>, usize),
  ) -> Range<usize> {
    let id = self.take_id();
    let item_id = |i: usize| id.key(key(i));
    let mut lz = std::mem::take(self.state::<Lazy>(id));
    let mut changed = lz.index(count, &item_id);

    // The scroll area around it.
    let t = &self.st.tree;
    let area =
      self.st.stack.iter().rev().map(|&(i, _)| i).find(|&i| t.nodes[i as usize].style.scroll.at(1));
    let area_id = area.map(|i| t.nodes[i as usize].id);

    // The item at the viewport's top: where the last pass meant it to be, or where it lay when
    // the anchor held the area at an end (the layout put the offset there, not the view).
    let last = &self.st.last;
    let keep = match (lz.anchor.take(), area_id.and_then(|a| last.get(a)), last.get(id)) {
      (Some(k), ..) if !lz.held => Some(k),
      (_, Some(a), Some(c)) if lz.held => (lz.built.start..lz.built.end.min(count)).find_map(|i| {
        let r = last.get(item_id(i))?;
        (r.pos[1] + r.size[1] > a.pos[1]).then_some((i, r.id, r.pos[1] - c.pos[1]))
      }),
      _ => None,
    };

    // Measures what the last layout laid out: a new width forgets what it measured before.
    if let Some(c) = last.get(id).filter(|c| c.size[0] != lz.width) {
      lz.width = c.size[0];
      lz.seen.clear();
      lz.heights.fill(f64::NAN);
      (lz.fresh, changed) = (true, true);
    }

    let (mut sum, mut k) = (0.0, 0);
    for i in lz.built.start..lz.built.end.min(count) {
      let Some(r) = last.get(item_id(i)) else { continue };
      let h = r.size[1];
      (sum, k) = (sum + h, k + 1);
      if lz.heights[i] != h {
        lz.heights[i] = h;
        lz.seen.insert(r.id, h);
        changed = true;
      }
    }

    if k > 0 && (lz.guess.is_none() || lz.fresh) {
      (lz.guess, lz.fresh, changed) = (Some(sum / f64::from(k)), false, true);
    }

    if changed {
      lz.sum();
    }

    // The item at the viewport's top moved by what changed above it: the offset follows, so
    // what shows stays put. Not while the anchor holds the area at its end.
    let held = |ui: &Ui| {
      let st = area_id.and_then(|a| ui.st.store.peek::<ScrollState>(a));
      st.is_some_and(ScrollState::held)
    };
    if let (Some((i, at, was)), Some(a)) = (keep, area_id)
      && i < count
      && item_id(i) == at
      && !held(self)
    {
      let by = lz.top(i) - was;
      if by != 0.0 {
        self.st.store.raw::<ScrollState>(a).offset[1] += by;
      }
    }

    // In `f64` throughout: a column of a million rows passes `f32`'s whole numbers.
    let sc = area_id.and_then(|a| self.st.scroll_exact(a));
    // Placed by the offset it was laid out at, seen through the offset the input set since.
    let laid = area_id.and_then(|a| self.st.last.get(a));
    let top = match (laid, self.st.last.get(id)) {
      (Some(a), Some(c)) => c.pos[1] - a.pos[1] + a.scroll[1],
      _ => 0.0,
    };

    // An item to reveal is scrolled to until a layout after the request shows it as the scroll
    // would put it (a tall one from its top): the column may move under it meanwhile, its
    // items be measured, or it not be laid out yet.
    let n = count;
    if let Some((i, tries)) = lz.reveal.filter(|&(i, t)| i < n && t > 0) {
      let vp = sc.map(|sc| sc.1);
      let shows = tries < TRIES
        && (vp.zip(self.rect(item_id(i)))).is_some_and(|(vp, r)| {
          let (top, end) = (vp.y + 0.5, vp.y + vp.h + 0.5);
          r.y >= vp.y - 0.5 && (r.y + r.h <= end || r.y <= top)
        });
      lz.reveal = (!shows).then_some((i, tries - 1));
      if let (false, Some((off, _))) = (shows, sc) {
        let (from, to) = (lz.top(i), lz.top(i + 1));
        self.reveal_exact(area_id.unwrap(), [off[0], top + from, 1.0, to - from]);
      }

      self.st.animating |= !shows;
    }

    // The offset the layout will take: the one asked within the last frame's content, grown or
    // shrunk by the column, or the end the anchor holds it at. Before the area is laid out, a
    // window's height where its anchor opens it; outside one, everything.
    let total = lz.top(n);
    let sc = area_id.and_then(|a| self.st.scroll_exact(a));
    let st = area_id.and_then(|a| self.st.store.peek::<ScrollState>(a));
    let (off, view) = match (sc, area_id.and_then(|a| self.st.last.get(a)), st) {
      (Some((_, vp)), Some(a), Some(s)) => {
        let col = self.st.last.get(id).map_or(total, |c| c.size[1]);
        let max = (input::max_scroll(a)[1] + total - col).max(0.0);
        let off = if s.held() { max } else { s.offset[1].clamp(0.0, max) };
        (off, f64::from(vp.h))
      }
      (Some((off, vp)), ..) => (off[1], f64::from(vp.h)),
      (None, _, st) if area.is_some() => {
        let view = f64::from(self.size().h);
        let y = st.and_then(|s| s.anchor).map_or(0.0, |a| f64::from(a.y));
        (y * (total - view).max(0.0), view)
      }
      _ => (0.0, f64::INFINITY),
    };

    // Items above the viewport come a margin's worth at a time, when under half a margin is
    // built: each new one moves what is below it by its guess's error, until the next pass.
    let y = off - top;
    let first = match lz.built.start.min(n) {
      s if s <= lz.at(y - MARGIN / 2.0) && s >= lz.at(y - 2.0 * MARGIN) => s,
      _ => lz.at(y - MARGIN).min(n),
    };

    let shown = first..(lz.at(y + view + MARGIN) + 1).min(n).max(first);
    let (from, to) = (lz.top(shown.start), lz.top(shown.end));

    // The item at the viewport's top, kept there as what is above it is measured.
    let anchor = area.is_some().then(|| lz.at(y)).filter(|&i| i < n);
    (lz.anchor, lz.held) = (anchor.map(|i| (i, item_id(i), lz.top(i))), held(self));
    let anchor = anchor.filter(|_| !lz.held);

    self.next_id = Some((id, false));
    self.col(Style::new().w(grow()), |ui| {
      spacer(ui, from);
      // The built items' edges and the kept item: the view runs again if the layout shows past
      // the edges (open where the column's are) or moved the kept item (new ones above it).
      let (head, _) = ui.push(Kind::Box, Style::new(), Flow::Row);
      let mut kept = None;
      for i in shown.clone() {
        if anchor == Some(i) {
          kept = Some(ui.st.tree.nodes.len() as u32);
        }

        ui.key(key(i)).col(Style::new().w(grow()), |ui| item(ui, i));
      }

      let (tail, _) = ui.push(Kind::Box, Style::new(), Flow::Row);
      spacer(ui, total - to);

      let Some(area) = area else { return };
      let lo = if shown.start == 0 { f64::NEG_INFINITY } else { 0.0 };
      let hi = if shown.end == n { f64::INFINITY } else { 0.0 };
      let partial = &mut ui.st.tree.partial;
      partial.push((area, head, lo, f64::INFINITY));
      partial.push((area, tail, f64::NEG_INFINITY, hi));
      // Short when the kept item's top is half a point off its place in the viewport.
      if let (Some(node), Some(i)) = (kept, anchor) {
        let s = top + lz.top(i) - off;
        partial.push((area, node, -s, view - s));
      }
    });

    // Items built for the first time are measured in the next frame, if no pass comes first.
    self.st.animating |= shown.clone().any(|i| lz.heights[i].is_nan());
    lz.built = shown.clone();
    *self.state::<Lazy>(id) = lz;
    shown
  }

  /// Scrolls item `i` of lazy column `id` into view, the least that shows it, in the column's
  /// next build; as SwiftUI's `ScrollViewReader.scrollTo(_:)`.
  pub fn reveal_item(&mut self, id: Id, i: usize) {
    self.state::<Lazy>(id).reveal = Some((i, TRIES));
  }
}

/// Space `h` high, if any: in two boxes, whole multiples of 4096 points and the rest, each held
/// exactly by an `f32` height, which the layout adds in `f64`.
fn spacer(ui: &mut Ui, h: f64) {
  let coarse = (h / 4096.0).floor() * 4096.0;
  for part in [coarse, h - coarse] {
    if part > 0.0 {
      ui.el(Style::new().h(px(part as f32)));
    }
  }
}

#[cfg(test)]
mod tests {
  use crate::app::{App, Event, Phase};
  use crate::scene::{Point, Size};
  use crate::ui::{Id, Style, TextStyle, Theme, Ui, UnitPoint, grow, px};
  use crate::{Harness, Modifiers};

  /// Item `i`'s height in [Mixed]: 20 to 119, in no order.
  fn height(i: usize) -> f32 {
    20.0 + (i * 37 % 100) as f32
  }

  /// A scroll area 300 high over a prefix `pre` high and a column of `n` items of mixed heights
  /// (`wide` makes each half as high), the item `reveal` asked for in the next view; at the
  /// bottom anchor when `bottom`. Counts the items it builds.
  struct Mixed {
    n: usize,
    pre: f32,
    bottom: bool,
    wide: bool,
    reveal: Option<usize>,
    built: usize,
  }

  impl Mixed {
    fn new(n: usize) -> Self {
      Self { n, pre: 0.0, bottom: false, wide: false, reveal: None, built: 0 }
    }
  }

  impl App for Mixed {
    fn view(&mut self, ui: &mut Ui) {
      if let Some(i) = self.reveal.take() {
        ui.reveal_item(Id::of("col"), i);
      }

      let wide = self.wide;
      let area = Style::new().w(grow()).h(px(300.0));
      let ui = if self.bottom { ui.default_scroll_anchor(UnitPoint::BOTTOM) } else { ui };
      let built = &mut self.built;
      ui.id("area").scroll_area(area, |ui| {
        ui.el(Style::new().h(px(self.pre)));
        ui.id("col").lazy_col(
          self.n,
          |i| i,
          |ui, i| {
            *built += 1;
            let h = if wide { height(i) / 2.0 } else { height(i) };
            ui.el(Style::new().h(px(h)));
          },
        );
      });
    }
  }

  fn shown(h: &Harness<Mixed>, i: usize) -> Option<(f32, f32)> {
    let (vp, r) = (h.rect(Id::of("area"))?, h.rect(Id::of("col").key(i))?);
    Some((r.y - vp.y, r.y + r.h - vp.y))
  }

  /// The first item whose box shows at the viewport's top, and where.
  fn at_top(h: &Harness<Mixed>) -> (usize, f32) {
    (0..h.app().n).find_map(|i| shown(h, i).filter(|s| s.1 > 0.0).map(|s| (i, s.0))).unwrap()
  }

  fn settle<A: App>(h: &mut Harness<A>) {
    for _ in 0..6 {
      h.frame();
    }
  }

  fn swipe(h: &mut Harness<Mixed>, dy: f32) {
    let (pos, delta, mods) = (Point::new(100.0, 150.0), Point::new(0.0, dy), Modifiers::empty());
    let (phase, momentum) = (Phase::Changed, Phase::None);
    h.event(Event::Scroll { pos, delta, precise: true, phase, momentum, mods });
    h.frame();
  }

  /// 100k items of mixed heights build what shows, each as high as it lays out, and no more.
  #[test]
  fn a_long_column_builds_what_shows_at_its_heights() {
    let mut h = Harness::new(Mixed::new(100_000), Size::new(200.0, 300.0), 1.0);
    h.frame();
    assert!(h.app().built < 60, "{} built", h.app().built);
    for i in 0..4 {
      let (top, end) = shown(&h, i).unwrap();
      assert_eq!(end - top, height(i), "item {i}");
    }

    assert!(shown(&h, 60).is_none());
  }

  /// Opened at the bottom, the last item ends at the viewport's end in the first frame.
  #[test]
  fn at_the_bottom_anchor_it_opens_on_its_last_item() {
    let mut h =
      Harness::new(Mixed { bottom: true, ..Mixed::new(3000) }, Size::new(200.0, 300.0), 1.0);
    h.frame();
    assert_eq!(shown(&h, 2999).map(|s| s.1), Some(300.0));
    assert!(h.app().built < 60, "{} built", h.app().built);
  }

  /// Scrolled up from the bottom through items never measured, each step moves what shows by
  /// just that step, though the guesses above it are replaced as they are measured.
  #[test]
  fn what_shows_moves_by_the_scroll_alone_as_items_are_measured() {
    let mut h =
      Harness::new(Mixed { bottom: true, ..Mixed::new(3000) }, Size::new(200.0, 300.0), 1.0);
    h.frame();
    for _ in 0..200 {
      let (i, y) = at_top(&h);
      swipe(&mut h, 37.0);
      let now = shown(&h, i).expect("still built").0;
      assert_eq!(now, y + 37.0, "item {i}");
    }
  }

  /// A new width measures again what shows, not every item; the item at the viewport's top is
  /// there again in the frame after (the first lays out at the new width what it built for the
  /// old).
  #[test]
  fn a_new_width_measures_what_shows() {
    let mut h = Harness::new(Mixed::new(3000), Size::new(200.0, 300.0), 1.0);
    h.frame();
    h.app_mut().reveal = Some(1500);
    settle(&mut h);
    swipe(&mut h, -10.0);
    let (i, y) = at_top(&h);
    (h.app_mut().wide, h.app_mut().built) = (true, 0);
    h.resize(Size::new(300.0, 300.0), 1.0);
    h.frame();
    assert!(h.app().built < 120, "{} built", h.app().built);
    assert_eq!(shown(&h, i).map(|s| s.0), Some(y));
    assert_eq!(shown(&h, i).map(|s| s.1 - s.0), Some(height(i) / 2.0));
  }

  /// An item far off, not measured, is revealed where it lays out once measured.
  #[test]
  fn a_far_item_is_revealed_once_measured() {
    let mut h = Harness::new(Mixed::new(3000), Size::new(200.0, 300.0), 1.0);
    h.frame();
    h.app_mut().reveal = Some(2000);
    settle(&mut h);
    let (top, end) = shown(&h, 2000).expect("built");
    assert!(top >= 0.0 && end <= 300.0, "{top}..{end}");
  }

  /// An item in view when it is asked for, pushed out by content growing above the column in
  /// the same frame: it still shows.
  #[test]
  fn a_reveal_follows_the_column_as_content_above_it_moves() {
    let mut h = Harness::new(Mixed::new(50), Size::new(200.0, 300.0), 1.0);
    h.frame();
    assert!(shown(&h, 2).is_some_and(|(_, end)| end <= 300.0), "in view before");
    (h.app_mut().pre, h.app_mut().reveal) = (1000.0, Some(2));
    settle(&mut h);
    let (top, end) = shown(&h, 2).expect("built");
    assert!(top >= 0.0 && end <= 300.0, "{top}..{end}");
  }

  /// Taller than the viewport, an item is revealed from its top.
  #[test]
  fn a_tall_item_is_revealed_from_its_top() {
    struct Tall(Option<usize>);
    impl App for Tall {
      fn view(&mut self, ui: &mut Ui) {
        if let Some(i) = self.0.take() {
          ui.reveal_item(Id::of("col"), i);
        }

        ui.id("area").scroll_area(Style::new().w(grow()).h(px(300.0)), |ui| {
          ui.id("col").lazy_col(50, |i| i, |ui, _| _ = ui.el(Style::new().h(px(400.0))));
        });
      }
    }

    let mut h = Harness::new(Tall(None), Size::new(200.0, 300.0), 1.0);
    h.frame();
    h.app_mut().0 = Some(1);
    settle(&mut h);
    let (vp, r) = (h.rect(Id::of("area")).unwrap(), h.rect(Id::of("col").key(1usize)).unwrap());
    assert_eq!(r.y, vp.y);
  }

  /// Paragraphs of 1 to 8 sentences, the last two lines at most, in a column 120 wide; and a
  /// text in a box narrower than its widest character, with the heights [Ui::measure_wrapped]
  /// gives them.
  struct Paragraphs {
    heights: Vec<f32>,
    narrow: f32,
  }

  const NARROW: &str = "iiiiiiii W";

  const SENTENCE: &str = "The quick brown fox jumps over the lazy dog. ";

  fn paragraph(i: usize) -> (String, TextStyle, Theme) {
    let t = Theme::PLATES_LIGHT;
    let style = TextStyle::new(t.font, t.text);
    let style = if i == 7 { style.line_limit(2) } else { style };
    (SENTENCE.repeat(i + 1).trim_end().to_owned(), style, t)
  }

  impl App for Paragraphs {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_theme(paragraph(0).2);
      self.heights = (0..8)
        .map(|i| {
          let (text, style, _) = paragraph(i);
          ui.measure_wrapped(&text, style, 120.0).h
        })
        .collect();
      let style = paragraph(0).1;
      self.narrow = ui.measure_wrapped(NARROW, style, 5.0).h;
      ui.col(Style::new().w(px(5.0)), |ui| ui.id("narrow").text(NARROW, style));
      ui.col(Style::new().w(px(120.0)), |ui| {
        for i in 0..8 {
          let (text, style, _) = paragraph(i);
          ui.id(("text", i)).text(&text, style);
        }
      });
    }
  }

  /// Measured wrapped, a paragraph is as high as the layout lays it out, and a limited one
  /// counts only the lines it shows. Narrower than a character, it is as wide as that, as laid
  /// out.
  #[test]
  fn wrapped_paragraphs_measure_as_they_lay_out() {
    let mut h =
      Harness::new(Paragraphs { heights: Vec::new(), narrow: 0.0 }, Size::new(200.0, 2000.0), 2.0);
    settle(&mut h);
    let line_h = h.metrics(paragraph(7).1.font).line_height();
    let text = |i: usize| h.rect(Id::of(("text", i))).unwrap();
    for i in 0..8 {
      assert_eq!(text(i).h, h.app().heights[i], "paragraph {i}");
    }

    assert!(text(6).h > text(0).h * 5.0, "it wraps: {:?}", text(6));
    assert_eq!(text(7).h, 2.0 * line_h, "two lines");
    let narrow = h.rect(Id::of("narrow")).unwrap();
    assert_eq!(narrow.h, h.app().narrow, "{narrow:?}");
  }
}
