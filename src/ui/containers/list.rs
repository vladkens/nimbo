// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Virtualized lists: only the rows in view, and a few around them, are built each frame,
//! between two spacers that give the scroll area the whole content height, so a list of 100k
//! rows costs what the forty on screen do and its scrollbar is right.
//!
//! ```ignore
//! ui.list(Id::of("rows"), self.rows.len(), 22.0, |ui, i| ui.label(&self.rows[i]));
//! let r = List::new(self.files.len(), 22.0)
//!   .key(&|i| self.files[i].id)
//!   .multi()
//!   .selection(&mut self.sel)
//!   .show(ui.id("files"), |ui, row| ui.label(&self.files[row.index].name));
//! if let Some(i) = r.activated { self.open(i) }
//! ```
//!
//! The rows to build come from the scroll offset as of the last frame and the input since,
//! clamped to the new content; when the layout shows more than was built (the viewport grew,
//! the offset was clamped) the view runs again at once, so a frame never shows a gap.
//!
//! Selection is by row key ([List::key], the index by default), never by visible element: it
//! stays right when rows scroll out, are inserted above or the list is re-sorted. It lives in
//! the app ([List::selection]) or the list's own state. A press selects; with [List::multi],
//! Cmd-click (Ctrl-click on Windows and Linux) toggles a row and Shift-click selects from the
//! anchor; a secondary click selects the row alone unless it is selected, for the app's context
//! menu ([Ui::context_menu]). Keys, while the list has focus (a press gives it), come as the
//! platform's commands: Up and Down (Shift extends), Page Up and Page Down, Home and End or Cmd-Up
//! and Cmd-Down, Cmd-A, and Enter, which activates the row, as a double click does. The row the
//! keys moved to scrolls into view; so does [Ui::reveal_row]. Disabled ([Ui::disabled]), it takes
//! no input or focus and dims.
//!
//! The list is the behaviour, its rows' fills the look: [List::unstyled] leaves the fills to
//! the app, which draws them from [Row].

use std::collections::HashSet;
use std::hash::BuildHasherDefault;
use std::ops::Range;

use crate::app::MouseButton;
use crate::keys::{Binding, Command, Move, Os, bind};
use crate::scene::{Point, Rect, alpha};
use crate::text::FxHasher;
use crate::ui::{Align, Cursor, Id, Role, State, Style, Ui, grow, px};

/// Rows built past each edge of the viewport.
const OVERSCAN: usize = 2;

/// Cmd-A selects every row of a multi-select list.
const MULTI_KEYS: &[Binding] = &[bind("cmd+a", Command::SelectAll)];

type KeySet = HashSet<u64, BuildHasherDefault<FxHasher>>;

// MARK: Selection

/// The selected rows of a list, by row key. The cursor is the row keys move from (the last one
/// pressed or moved to), the anchor where Shift extends from. Each keeps the index it was last
/// seen at as a hint, so finding it again is one lookup unless rows moved, then a scan.
#[derive(Clone, Debug, Default)]
pub struct Selection {
  keys: KeySet,
  cursor: Option<(u64, usize)>,
  anchor: Option<(u64, usize)>,
}

/// Row `at`'s index among `n` rows keyed by `key`: its hint, else a scan.
fn find(at: (u64, usize), n: usize, key: &dyn Fn(usize) -> u64) -> Option<usize> {
  if at.1 < n && key(at.1) == at.0 {
    return Some(at.1);
  }

  (0..n).find(|&i| key(i) == at.0)
}

impl Selection {
  pub fn contains(&self, key: u64) -> bool {
    self.keys.contains(&key)
  }

  pub fn len(&self) -> usize {
    self.keys.len()
  }

  pub fn is_empty(&self) -> bool {
    self.keys.is_empty()
  }

  /// The selected keys, in no order.
  pub fn keys(&self) -> impl Iterator<Item = u64> + '_ {
    self.keys.iter().copied()
  }

  /// The cursor row's key.
  pub fn cursor(&self) -> Option<u64> {
    self.cursor.map(|c| c.0)
  }

  /// The cursor row's index among `n` rows keyed by `key`, if it is still there.
  pub fn cursor_index(&self, n: usize, key: &dyn Fn(usize) -> u64) -> Option<usize> {
    find(self.cursor?, n, key)
  }

  pub fn clear(&mut self) {
    self.keys.clear();
    (self.cursor, self.anchor) = (None, None);
  }

  /// Selects only row `i`, keyed `key`, with the cursor and the anchor on it.
  pub fn select(&mut self, key: u64, i: usize) {
    self.keys.clear();
    self.keys.insert(key);
    (self.cursor, self.anchor) = (Some((key, i)), Some((key, i)));
  }

  /// Adds row `i` or takes it out (Cmd-click); the cursor and the anchor go to it.
  pub fn toggle(&mut self, key: u64, i: usize) {
    if !self.keys.remove(&key) {
      self.keys.insert(key);
    }

    (self.cursor, self.anchor) = (Some((key, i)), Some((key, i)));
  }

  /// Selects the rows from the anchor to row `i` (Shift-click, Shift-arrows), in place of what
  /// was selected; the cursor goes to `i`, the anchor stays (or starts at `i`).
  pub fn extend(&mut self, i: usize, n: usize, key: &dyn Fn(usize) -> u64) {
    let a = self.anchor.and_then(|a| find(a, n, key)).unwrap_or(i);
    self.keys.clear();
    self.keys.extend((a.min(i)..=a.max(i)).map(key));
    self.cursor = Some((key(i), i));
    self.anchor = Some((key(a), a));
  }

  /// Selects all `n` rows, the cursor staying.
  pub fn select_all(&mut self, n: usize, key: &dyn Fn(usize) -> u64) {
    self.keys.clear();
    self.keys.extend((0..n).map(key));
  }
}

// MARK: List

/// A row being built: where it is and how it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
  pub index: usize,
  pub key: u64,
  /// The row's element: `list.key(("row", key))`.
  pub id: Id,
  pub selected: bool,
  /// The hover fade, 0 to 1.
  pub hover: f32,
  /// The row keys move from.
  pub cursor: bool,
  /// The list has focus (selected rows are drawn in the accent then).
  pub focused: bool,
}

/// What a list got this frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListResponse {
  pub id: Id,
  /// The selection changed.
  pub changed: bool,
  /// A row was pressed (and selected as the press asked).
  pub pressed: Option<usize>,
  /// A row was double-clicked, or Enter pressed on the cursor row.
  pub activated: Option<usize>,
  /// The rows built.
  pub shown: Range<usize>,
  pub focused: bool,
}

/// A list's state in the store, under its id.
#[derive(Default)]
struct ListState {
  /// The selection, when the app doesn't hold it.
  sel: Selection,
  /// A row to scroll into view once the list knows its viewport.
  reveal: Option<usize>,
  /// The press a pass saw last ([crate::ui::Press::seq]).
  pressed: Option<u32>,
  /// The rows built last time, keyed as then, from index `from`: what a press can be on.
  shown: Vec<u64>,
  from: usize,
}

/// A virtualized list ([Ui::list] for the simple case); see the module's docs.
pub struct List<'s> {
  n: usize,
  h: f32,
  key: Option<&'s dyn Fn(usize) -> u64>,
  sel: Option<&'s mut Selection>,
  multi: bool,
  style: Style,
  row: Style,
  /// Rows filled from the theme when selected or hovered.
  fill: bool,
}

impl<'s> List<'s> {
  /// `count` rows `row_h` high, growing to fill its parent.
  pub fn new(count: usize, row_h: f32) -> Self {
    Self {
      n: count,
      h: row_h,
      key: None,
      sel: None,
      multi: false,
      style: Style::new().grow(),
      row: Style::new()
        .w(grow())
        .pads([0.0, 14.0, 0.0, 10.0])
        .gap(8.0)
        .align(Align::Start, Align::Center),
      fill: true,
    }
  }

  /// Row keys, unique and stable while the row is the same thing; the index by default.
  pub fn key(self, key: &'s dyn Fn(usize) -> u64) -> Self {
    Self { key: Some(key), ..self }
  }

  /// The selection, held by the app; without it the list keeps its own.
  pub fn selection(self, sel: &'s mut Selection) -> Self {
    Self { sel: Some(sel), ..self }
  }

  /// Several rows at a time: Cmd-click toggles, Shift-click and Shift-arrows extend, Cmd-A
  /// selects all.
  pub fn multi(self) -> Self {
    Self { multi: true, ..self }
  }

  /// The list's box (a scroll area): its size and padding; its gap is 0.
  pub fn style(self, style: Style) -> Self {
    Self { style, ..self }
  }

  /// Each row's box: width, padding, gap and alignment; the list sets its height and fill.
  pub fn row_style(self, row: Style) -> Self {
    Self { row, ..self }
  }

  /// Rows with no fill: the app draws selection and hover itself from [Row], the list keeps
  /// the behaviour (selection, keys, scrolling, virtualization).
  pub fn unstyled(self) -> Self {
    Self { fill: false, ..self }
  }

  /// Builds the rows in view with `row(ui, row)` inside the list's scroll area, after applying
  /// the input since the last frame.
  pub fn show<'a>(self, ui: &mut Ui<'a>, mut f: impl FnMut(&mut Ui<'a>, Row)) -> ListResponse {
    let t = ui.theme();
    let id = ui.take_id();
    // Disabled: no input, no focus, dimmed.
    let disabled = ui.take_disabled();
    if disabled && ui.focused(id) {
      ui.focus(None);
    }

    let List { n, h, style, row, fill, multi, .. } = self;
    let keyf = self.key;
    let key = move |i: usize| keyf.map_or(i as u64, |k| k(i));
    let mut st = std::mem::take(ui.state::<ListState>(id));
    let mut own = None;
    let sel = match self.sel {
      Some(s) => s,
      None => own.insert(std::mem::take(&mut st.sel)),
    };

    // Its selection in the accent while it has focus in the key window.
    let focused = ui.focus_visible(id);
    let mut out = ListResponse { id, focused, ..Default::default() };
    let sc = ui.scroll(id);
    let view_h = sc.map_or(ui.size().h, |s| s.viewport.h);
    if n > 0 && !disabled {
      ui.list_keys(id, &mut st, sel, &mut out, (n, h, view_h, multi), &key);
      ui.list_press(id, &mut st, sel, &mut out, n, multi, &key);
    }

    // Scroll a row into view, by the last frame's viewport; the first and the last with the
    // list's padding.
    // Not laid out yet, it waits for the next frame, which comes at once.
    match (st.reveal, sc) {
      (Some(i), Some(sc)) => {
        st.reveal = None;
        let top = if i == 0 { 0.0 } else { style.pad[0] + i as f32 * h };
        let end = style.pad[0] + (i + 1) as f32 * h + if i + 1 == n { style.pad[2] } else { 0.0 };
        if i < n {
          ui.reveal(id, Rect::new(sc.offset.x, top, 1.0, end - top));
        }
      }
      (Some(_), None) => ui.st.animating = true,
      _ => {}
    }

    let sel_bg = match focused {
      true => t.selection,
      false => alpha(t.text, if t.dark { 0.10 } else { 0.08 }),
    };

    let hover_bg = alpha(t.text, 0.05);
    st.shown.clear();
    let mut shown = 0..0;
    if multi && !disabled {
      ui.keymap(MULTI_KEYS);
    }

    let style = match disabled {
      true => style.gap(0.0).opacity(t.disabled),
      false => style.focusable_quiet().gap(0.0),
    };

    // A list of rows, the cursor's the active one.
    let active = sel.cursor.map(|c| id.key(("row", c.0)));
    ui.a11y(id, Role::List).state(State::Disabled, disabled).active(active);
    ui.with_id(id).scroll_area(style, |ui| {
      // The offset after the input and the scroll area's own (a thumb drag), clamped to the
      // content as it is now.
      let (pad_t, pad_b) = (style.pad[0], style.pad[2]);
      let total = n as f32 * h;
      let max = (pad_t + total + pad_b - view_h).max(0.0);
      let off = ui.scroll_raw(id).y.clamp(0.0, max);
      let at = |y: f32| ((y / h).floor().max(0.0) as usize).min(n.saturating_sub(1));
      shown = match n {
        0 => 0..0,
        _ => {
          at(off - pad_t).saturating_sub(OVERSCAN)..(at(off - pad_t + view_h) + 1 + OVERSCAN).min(n)
        }
      };
      ui.el(Style::new().h(px(shown.start as f32 * h)));
      st.shown.extend(shown.clone().map(key));
      st.from = shown.start;
      for (i, &k) in shown.clone().zip(&st.shown) {
        let rid = id.key(("row", k));
        let selected = sel.contains(k);
        let hover = ui.hot(rid, !disabled && ui.hovered(rid));
        let bg = match (fill, selected) {
          (true, true) => sel_bg,
          (true, false) => alpha(hover_bg, hover_bg[3] * hover),
          _ => [0.0; 4],
        };

        ui.a11y(rid, Role::Row).state(State::Selected, selected);
        let cursor = sel.cursor.is_some_and(|c| c.0 == k);
        let r = Row { index: i, key: k, id: rid, selected, hover, cursor, focused };
        // Disabled, the same id (keyed in the list) taking no input.
        let s = row.h(px(h)).bg(bg);
        match disabled {
          true => ui.key(("row", k)).row(s, |ui| f(ui, r)),
          false => ui.with_id(rid).row(s.cursor(Cursor::Pointer), |ui| f(ui, r)),
        };
      }

      ui.el(Style::new().h(px(total - shown.end as f32 * h)));
      // What the built rows cover, for the runtime to run the view again if the layout shows
      // more (the ends are open when they reach the list's).
      let lo = if shown.start == 0 { f32::NEG_INFINITY } else { pad_t + shown.start as f32 * h };
      let hi = if shown.end == n { f32::INFINITY } else { pad_t + shown.end as f32 * h };
      let node = ui.st.stack.last().unwrap().0;
      ui.st.tree.partial.push((node, node, lo.into(), hi.into()));
    });

    if let Some(own) = own {
      st.sel = own;
    }

    *ui.state::<ListState>(id) = st;
    out.shown = shown;
    out
  }
}

impl<'a> Ui<'a> {
  /// A list of `count` rows `row_h` high, built with `row(ui, index)` only for the rows in view;
  /// one selected at a time ([List] for the rest).
  pub fn list(
    &mut self,
    id: Id,
    count: usize,
    row_h: f32,
    mut row: impl FnMut(&mut Ui<'a>, usize),
  ) -> ListResponse {
    List::new(count, row_h).show(self.with_id(id), |ui, r| row(ui, r.index))
  }

  /// Scrolls row `i` of list `id` into view, the least that shows it, in the list's next build.
  pub fn reveal_row(&mut self, id: Id, i: usize) {
    self.state::<ListState>(id).reveal = Some(i);
  }

  /// A scroll container's offset as last set, before the layout clamps it.
  fn scroll_raw(&self, id: Id) -> Point {
    let raw = self.st.store.peek::<crate::ui::input::ScrollState>(id);
    let raw = raw.map(|s| Point::new(s.offset[0] as f32, s.offset[1] as f32));
    raw.or_else(|| self.scroll(id).map(|s| s.offset)).unwrap_or_default()
  }

  /// The list's keys, as commands: the cursor moves (Shift extends a multi-select), Enter
  /// activates, Cmd-A selects all; `geo` is the rows' count and height, the viewport's height
  /// and whether several rows can be selected.
  fn list_keys(
    &mut self,
    id: Id,
    st: &mut ListState,
    sel: &mut Selection,
    out: &mut ListResponse,
    geo: (usize, f32, f32, bool),
    key: &dyn Fn(usize) -> u64,
  ) {
    let (n, h, view_h, multi) = geo;
    let Some(c) = self.command(id) else { return };
    let cur = sel.cursor_index(n, key);
    let page = |dir: isize| {
      let rows = ((view_h / h).floor() as isize - 1).max(1);
      (cur.unwrap_or(0) as isize + dir * rows).clamp(0, n as isize - 1) as usize
    };
    let step = |d: isize| match cur {
      Some(c) => (c as isize + d).clamp(0, n as isize - 1) as usize,
      None if d > 0 => 0,
      None => n - 1,
    };
    use Move::*;
    let (to, shift) = match c {
      Command::Move(Up, s) => (step(-1), s),
      Command::Move(Down, s) => (step(1), s),
      Command::Move(PageUp, s) => (page(-1), s),
      Command::Move(PageDown, s) => (page(1), s),
      Command::ScrollPageUp => (page(-1), false),
      Command::ScrollPageDown => (page(1), false),
      Command::Move(DocStart | LineStart, s) => (0, s),
      Command::Move(DocEnd | LineEnd, s) => (n - 1, s),
      Command::ScrollToTop => (0, false),
      Command::ScrollToBottom => (n - 1, false),
      Command::InsertNewline => {
        out.activated = cur;
        return;
      }
      Command::SelectAll if multi => {
        sel.select_all(n, key);
        out.changed = true;
        return;
      }
      _ => {
        self.pass(id);
        return;
      }
    };

    match shift && multi {
      true => sel.extend(to, n, key),
      false => sel.select(key(to), to),
    }

    (st.reveal, out.changed) = (Some(to), true);
  }

  /// A press on a row selects it (a click when no frame saw its press: a quick one), as the
  /// modifiers ask; a double click activates it. A secondary one, as a context menu opens
  /// ([Ui::context_press]), focuses the list and selects the row alone unless it is selected
  /// (Finder's rule): the app's [Ui::context_menu] after it reads that selection.
  #[allow(clippy::too_many_arguments)]
  fn list_press(
    &mut self,
    id: Id,
    st: &mut ListState,
    sel: &mut Selection,
    out: &mut ListResponse,
    n: usize,
    multi: bool,
    key: &dyn Fn(usize) -> u64,
  ) {
    // The secondary button's press, on Windows its release.
    let (win, right) = (self.st.os == Os::Windows, MouseButton::Right);
    let press = self.press().filter(|p| p.button == MouseButton::Left || p.button == right && !win);
    // A press is seen once; a click whose press no pass saw (a quick one) stands for it.
    let both = |b| matches!(b, MouseButton::Left | MouseButton::Right);
    let click = self.st.input.click.filter(|c| both(c.button));
    let hit = match (press, click) {
      (Some(p), _) if st.pressed != Some(p.seq) => Some((p.id, p.mods, p.clicks, p.button)),
      (None, Some(c)) if st.pressed != Some(c.seq) => Some((c.id, c.mods, c.clicks, c.button)),
      _ => None,
    };

    st.pressed = press.map(|p| p.seq).or(click.map(|c| c.seq)).or(st.pressed);
    let Some((at, mods, clicks, button)) = hit else { return };
    // Its index as built, a hint that saves a scan unless the rows moved since.
    let Some(at) = st.shown.iter().position(|&k| id.key(("row", k)) == at) else { return };
    let k = st.shown[at];
    let Some(i) = find((k, st.from + at), n, key) else { return };
    match (multi, mods.contains(crate::keys::CMD), mods.shift()) {
      _ if button == right => {
        self.focus(Some(id));
        if sel.contains(k) {
          return;
        }

        sel.select(k, i);
      }
      (true, true, _) => sel.toggle(k, i),
      (true, _, true) => sel.extend(i, n, key),
      _ => sel.select(k, i),
    }

    (out.pressed, out.changed) = (Some(i), true);
    if clicks == 2 && button != right {
      out.activated = Some(i);
    }
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::scene::{Point, Size};
  use crate::ui::UiState;
  use crate::ui::layout::tests::Fake;

  #[test]
  fn selection_by_key_survives_moves() {
    let rows = |v: &'static [u64]| move |i: usize| v[i];
    let mut s = Selection::default();
    let key = rows(&[10, 11, 12, 13, 14]);
    s.select(12, 2);
    assert_eq!((s.cursor(), s.cursor_index(5, &key)), (Some(12), Some(2)));
    // Shift extends from the anchor, both ways, replacing the range.
    s.extend(4, 5, &key);
    assert_eq!(sorted(&s), [12, 13, 14]);
    s.extend(0, 5, &key);
    assert_eq!(sorted(&s), [10, 11, 12]);
    // Cmd toggles and moves the anchor.
    s.toggle(11, 1);
    assert_eq!(sorted(&s), [10, 12]);
    s.extend(3, 5, &key);
    assert_eq!(sorted(&s), [11, 12, 13]);
    // Rows inserted above: found again by key.
    let moved = rows(&[1, 2, 10, 11, 12, 13, 14]);
    assert_eq!(s.cursor_index(7, &moved), Some(5));
    assert!(s.contains(13) && !s.contains(1));
    s.select_all(7, &moved);
    assert_eq!(s.len(), 7);
    s.clear();
    assert!(s.is_empty() && s.cursor().is_none());
  }

  fn sorted(s: &Selection) -> Vec<u64> {
    let mut v: Vec<_> = s.keys().collect();
    v.sort();
    v
  }

  /// A frame of a 200 x 100 window with a list of `n` rows 10 high, scrolled to `off` first
  /// unless it is negative: the state, the rows built, and whether they fall short.
  fn frame(st: UiState, n: usize, off: f32) -> (UiState, usize, bool) {
    let mut ui = Ui::new(st, Size::new(200.0, 100.0), None, Duration::ZERO);
    let id = Id::of("list");
    if off >= 0.0 {
      ui.scroll_to(id, Point::new(0.0, off));
    }

    let mut shown = 0;
    ui.col(Style::new().w(px(200.0)).h(px(100.0)), |ui| {
      let r = ui.list(id, n, 10.0, |ui, _| _ = ui.el(Style::new().w(px(5.0)).h(px(4.0))));
      shown = r.shown.len();
    });
    ui.layout(&mut Fake, 1.0);
    let short = ui.partial_short();
    (ui.finish(), shown, short)
  }

  /// A list of 1000 rows 20 high, the app holding the selection, then a button; the list
  /// can be disabled, and asks for row 500 before its first frame.
  #[derive(Default)]
  struct Rows {
    sel: Selection,
    off: bool,
    asked: bool,
    clicks: usize,
  }

  impl crate::App for Rows {
    fn view(&mut self, ui: &mut Ui) {
      if !std::mem::replace(&mut self.asked, true) {
        ui.reveal_row(Id::of("list"), 500);
      }

      ui.col(Style::new().w(px(200.0)).h(px(200.0)), |ui| {
        let list =
          List::new(1000, 20.0).multi().selection(&mut self.sel).style(Style::new().grow());
        list.show(ui.disabled(self.off).id("list"), |ui, _| ui.label("row"));
      });
      if ui.id("b").button("B").clicked {
        self.clicks += 1;
      }
    }
  }

  #[test]
  fn a_row_revealed_early_a_cancelled_press_and_a_disabled_list() {
    use crate::harness::Harness;
    let mut h = Harness::new(Rows::default(), Size::new(300.0, 300.0), 1.0);
    let row = |k: u64| Id::of("list").key(("row", k));
    // Asked before the list had a viewport: shown once it has one.
    h.frame();
    h.frame();
    assert!(h.rect(row(500)).is_some(), "row 500 revealed");
    // A press seen by a frame, released off the list (no click), then a quick click on another
    // row before the next frame: that click selects.
    let (a, b) = (h.rect(row(500)).unwrap(), h.rect(row(497)).unwrap());
    h.press(a.x + 5.0, a.y + 5.0);
    h.frame();
    h.move_to(250.0, 250.0);
    h.release(250.0, 250.0);
    h.click(b.x + 5.0, b.y + 5.0);
    assert_eq!(h.app().sel.cursor(), Some(497));
    // The window losing the keys: the selection turns from the accent to grey.
    let px = |h: &mut Harness<Rows>| h.render().pixel(b.x + 150.0, b.y + 10.0);
    let active = px(&mut h);
    h.event(crate::app::Event::Focus(false));
    let inactive = px(&mut h);
    assert_ne!(active, inactive);
    h.event(crate::app::Event::Focus(true));
    // The same again on one row: its press selects it, then the quick Cmd-click's toggles it.
    h.press(b.x + 5.0, b.y + 5.0);
    h.frame();
    h.move_to(250.0, 250.0);
    h.release(250.0, 250.0);
    h.move_to(b.x + 5.0, b.y + 5.0);
    h.press_with(b.x + 5.0, b.y + 5.0, 1, crate::keys::CMD);
    h.release(b.x + 5.0, b.y + 5.0);
    assert!(h.app().sel.is_empty());
    h.click(b.x + 5.0, b.y + 5.0);
    // Disabled: no selection by click, no focus, and the button after it still works.
    h.frame();
    h.app_mut().off = true;
    h.frame();
    assert_eq!(h.focused(), None);
    let c = h.rect(row(498)).unwrap();
    h.click(c.x + 5.0, c.y + 5.0);
    h.frame();
    assert_eq!((h.app().sel.cursor(), h.focused()), (Some(497), None));
    assert!(h.click_id(Id::of("b")));
    assert_eq!(h.app().clicks, 1);
  }

  /// 100k rows keyed by a function that counts its calls.
  #[derive(Default)]
  struct Counted {
    calls: std::cell::Cell<usize>,
  }

  impl crate::App for Counted {
    fn view(&mut self, ui: &mut Ui) {
      let calls = &self.calls;
      let key = move |i: usize| {
        calls.set(calls.get() + 1);
        i as u64 * 7
      };
      List::new(100_000, 20.0).key(&key).show(ui.id("list"), |ui, _| ui.label("row"));
    }
  }

  #[test]
  fn a_press_near_the_end_finds_its_row_without_a_scan() {
    use crate::harness::Harness;
    let mut h = Harness::new(Counted::default(), Size::new(200.0, 200.0), 1.0);
    h.frame();
    h.rt.ui.store.raw::<crate::ui::input::ScrollState>(Id::of("list")).offset[1] = 1_999_000.0;
    h.rt.redraw();
    h.frame();
    let row = Id::of("list").key(("row", 99_955u64 * 7));
    let r = h.rect(row).expect("row 99955 built");
    h.app().calls.set(0);
    h.click(r.x + 5.0, r.y + 5.0);
    let calls = h.app().calls.get();
    assert_eq!(h.focused(), Some(Id::of("list")));
    assert!(calls < 100, "{calls} key calls for one press");
  }

  #[test]
  fn builds_only_the_rows_in_view() {
    let id = Id::of("list");
    let (st, shown, short) = frame(UiState::default(), 100_000, -1.0);
    // The window's height before the list has a viewport: 11 rows and the overscan.
    assert_eq!((shown, short), (13, false));
    let sc = st.scroll(id).unwrap();
    assert_eq!((sc.content.h, sc.viewport.h), (1_000_000.0, 100.0));
    // Scrolled to the end: the last rows, nothing past them.
    let (st, shown, short) = frame(st, 100_000, 2_000_000.0);
    assert_eq!((shown, short), (12, false));
    assert_eq!(st.scroll(id).unwrap().offset.y, 999_900.0);
    assert!(st.last.rect(id.key(("row", 99_999u64))).is_some());
    assert!(st.last.rect(id.key(("row", 99_988u64))).is_some());
    assert!(st.last.rect(id.key(("row", 99_987u64))).is_none());
    // The count drops under the offset: clamped in the view too, rows shown from the top; the
    // scrollbar, showing since the offset moved, drew from the old content: the view runs again.
    let (st, shown, short) = frame(st, 5, -1.0);
    assert_eq!((shown, short), (5, true));
    assert_eq!(st.scroll(id).unwrap().offset.y, 0.0);
  }
}
