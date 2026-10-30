// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/history.rs` through the harness: of 86,000 rows a day at a time only those
//! in view are built, under a scroll area as tall as all of them; the end builds the last rows;
//! rows and days sit where a whole build would put them; a selection and the rows' ids hold
//! across scrolling; a row from the middle is revealed; a million rows, past `f32`'s whole
//! numbers, scroll to their end and their middle a row apart; rows read as rows and press.

#[path = "../examples/history.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod history;

mod common;

use common::{mid, reads};
use history::{History, LIST, PAGE, PER_DAY, ROW_H, ROWS};
use nimbo::ui::{Action, Role};
use nimbo::{Event, Harness, Id, Modifiers, Phase, Point, Rect, Size};

type H = Harness<History>;

fn history() -> H {
  let mut h = Harness::new(History::default(), Size::new(560.0, 520.0), 1.0);
  h.frame();
  h
}

/// Row `i` of day `g`'s box, keyed by its dictation.
fn row(g: usize, i: usize) -> Id {
  Id::of(LIST).key((1u8, entry(g, i)))
}

/// Row `i` of day `g`'s dictation, as the example numbers them.
fn entry(g: usize, i: usize) -> u64 {
  (ROWS - g * PER_DAY - i) as u64
}

fn rect(h: &H, id: Id) -> Option<Rect> {
  h.rect(id)
}

/// A trackpad's scroll of `dy` points over the page.
fn swipe(h: &mut H, dy: f32) {
  let (x, y) = mid(h.rect(Id::of(PAGE)).unwrap());
  let (pos, delta, mods) = (Point::new(x, y), Point::new(0.0, dy), Modifiers::empty());
  h.event(Event::Scroll {
    pos,
    delta,
    precise: true,
    phase: Phase::Changed,
    momentum: Phase::None,
    mods,
  });
  h.frame();
}

fn built(h: &H) -> usize {
  h.built()
}

#[test]
fn only_the_rows_in_view_are_built() {
  let h = history();
  assert!(rect(&h, row(0, 0)).is_some() && rect(&h, row(0, 20)).is_none());
  assert!(built(&h) < 400, "{} nodes", built(&h));
  let sc = h.scroll(Id::of(PAGE)).unwrap();
  assert!(sc.content.h > ROWS as f32 * ROW_H, "content {} high", sc.content.h);
}

#[test]
fn the_end_builds_the_last_rows() {
  let mut h = history();
  swipe(&mut h, -1e9);
  let days = ROWS.div_ceil(PER_DAY);
  let last = ROWS - (days - 1) * PER_DAY - 1;
  let view = rect(&h, Id::of(PAGE)).unwrap();
  let r = rect(&h, row(days - 1, last)).expect("the last row built");
  assert!(r.y + r.h <= view.y + view.h && r.y >= view.y, "{r:?} in {view:?}");
  assert!(rect(&h, row(0, 0)).is_none());
  assert!(built(&h) < 400, "{} nodes", built(&h));
}

/// Rows follow each other a row and a hairline apart, and the gap a day's title takes is the
/// same deep in the list as at its top: the spacers stand for exactly what they replace.
#[test]
fn rows_and_days_sit_where_a_whole_build_puts_them() {
  let mut h = history();
  let step = |h: &H, g, i| rect(h, row(g, i + 1)).unwrap().y - rect(h, row(g, i)).unwrap().y;
  let title =
    |h: &H, g: usize| rect(h, row(g + 1, 0)).unwrap().y - rect(h, row(g, PER_DAY - 1)).unwrap().y;

  // The first day's end and the second's start, near the top.
  swipe(&mut h, -(PER_DAY as f32 - 4.0) * (ROW_H + 1.0));
  let (near, step_near) = (title(&h, 0), step(&h, 0, PER_DAY - 3));

  // The same 300 days down.
  let deep = 300;
  let y = rect(&h, row(1, 0)).unwrap().y;
  swipe(
    &mut h,
    -(deep as f32 - 1.0) * (PER_DAY as f32 * (ROW_H + 1.0) + near - ROW_H - 1.0) - (y - 200.0),
  );
  h.frame();
  assert_eq!(title(&h, deep - 1), near, "a day's title as high deep in the list");
  assert_eq!(step(&h, deep, 1), step_near, "rows a row and a hairline apart");
  assert!((step_near - ROW_H - 1.0).abs() < 0.01, "{step_near}");
}

#[test]
fn a_selection_and_the_rows_ids_hold_across_scrolling() {
  let mut h = history();
  h.click_id(row(0, 2));
  assert_eq!(h.app().selected, Some(entry(0, 2)));
  let at = rect(&h, row(0, 2)).unwrap();

  swipe(&mut h, -5000.0);
  assert!(rect(&h, row(0, 2)).is_none(), "scrolled out, not built");
  swipe(&mut h, 5000.0);
  assert_eq!(rect(&h, row(0, 2)), Some(at), "back where it was, under the same id");
  assert_eq!(h.app().selected, Some(entry(0, 2)));
}

/// Asked before the page is laid out, a row is revealed once it is.
#[test]
fn a_row_asked_for_before_the_first_frame_is_revealed() {
  let app = History { reveal: Some((120, 3)), ..History::default() };
  let mut h = Harness::new(app, Size::new(560.0, 520.0), 1.0);
  for _ in 0..4 {
    h.frame();
  }

  let view = rect(&h, Id::of(PAGE)).unwrap();
  let r = rect(&h, row(120, 3)).expect("built");
  assert!(r.y >= view.y && r.y + r.h <= view.y + view.h, "{r:?} in {view:?}");
}

#[test]
fn a_row_from_the_middle_is_revealed() {
  let mut h = history();
  h.app_mut().reveal = Some((200, 10));
  h.frame();
  h.frame();
  let view = rect(&h, Id::of(PAGE)).unwrap();
  let r = rect(&h, row(200, 10)).expect("built");
  assert!(r.y >= view.y && r.y + r.h <= view.y + view.h, "{r:?} in {view:?}");
}

/// A million rows, about 53 million points: past the end, the last row in view and rows a row
/// and a hairline apart; a row from the middle revealed, its neighbours a row apart too, and
/// swipes there move it by exactly as much.
#[test]
fn a_million_rows_scroll_to_the_point() {
  const MILLION: usize = 1_000_000;
  let mut h = Harness::new(History::new(MILLION), Size::new(560.0, 520.0), 2.0);
  h.frame();
  let row = |g: usize, i: usize| Id::of(LIST).key((1u8, (MILLION - g * PER_DAY - i) as u64));
  let step = |h: &H, g, i| rect(h, row(g, i + 1)).unwrap().y - rect(h, row(g, i)).unwrap().y;

  swipe(&mut h, -1e9);
  let days = MILLION.div_ceil(PER_DAY);
  let last = MILLION - (days - 1) * PER_DAY - 1;
  let view = rect(&h, Id::of(PAGE)).unwrap();
  let r = rect(&h, row(days - 1, last)).expect("the last row built");
  assert!(r.y >= view.y && r.y + r.h <= view.y + view.h, "{r:?} in {view:?}");
  assert_eq!(step(&h, days - 1, last - 1), ROW_H + 1.0, "a row and a hairline apart");
  // The scrollbars, drawn in `f32`, leave the exact end where it is.
  for _ in 0..3 {
    h.frame();
  }

  assert_eq!(rect(&h, row(days - 1, last)), Some(r), "still at the very end");

  let (g, i) = (days / 2, 70);
  h.app_mut().reveal = Some((g, i));
  for _ in 0..4 {
    h.frame();
  }

  let r = rect(&h, row(g, i)).expect("built");
  assert!(r.y >= view.y && r.y + r.h <= view.y + view.h, "{r:?} in {view:?}");
  assert_eq!((step(&h, g, i - 1), step(&h, g, i)), (ROW_H + 1.0, ROW_H + 1.0));

  // A swipe this deep moves the rows by exactly as much: `f32` would round the offset to 4.
  for dy in [-37.0, -1.5, 3.0] {
    let y = rect(&h, row(g, i)).unwrap().y;
    swipe(&mut h, dy);
    assert_eq!(rect(&h, row(g, i)).unwrap().y - y, dy, "moved by the swipe's {dy}");
  }
}

/// A row a click selects reads as a row named by its text and time, selected when it is, its
/// Copy button by its icon; an assistive press selects it.
#[test]
fn rows_read_and_press_as_rows() {
  let mut h = history();
  let name = |g, i| format!("Dictation {}: what was said, on one line", entry(g, i));
  reads(&h.a11y(), &[&format!("row {:?} desc \"23:59 · 7.0 s\"", name(0, 0)), "button \"Copy\""]);
  let t = h.a11y_tree();
  let third =
    t.nodes.iter().find(|n| n.props.role == Role::Row && t.str(n.props.name) == name(0, 2));
  assert!(h.act(third.unwrap().id, Action::Press));
  assert_eq!(h.app().selected, Some(entry(0, 2)));
  reads(&h.a11y(), &[&format!("row {:?} selected desc \"23:57 · 5.8 s\"", name(0, 2))]);
}
