// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Virtualized lists: only the rows in view, and a few around them, are built each frame,
//! between two spacers that give the scroll area the whole content height, so a list of 100k
//! rows costs what the forty on screen do and its scrollbar is right.
//!
//! [List] is nimbo's take on SwiftUI's `List` (with its `Section`s, selection, context menus and
//! keyboard): its behaviour is the target to move towards, not a 1:1 copy. The app builds only
//! what a row and a section header show; the list does the selection, the clicks, the keys and
//! the highlight.
//!
//! ```ignore
//! ui.list(Id::of("rows"), self.rows.len(), 22.0, |ui, i| ui.label(&self.rows[i]));
//! let r = List::new(self.files.len(), 22.0)
//!   .key(&|i| self.files[i].id)
//!   .multi()
//!   .selection(&mut self.sel)
//!   .show(ui.id("files"), |ui, row| ui.label(&self.files[row.index].name));
//! if let Some(i) = r.activated { self.open(i) }
//! if r.delete { self.ask_delete() } // the app's confirmation, then `self.sel.retain(..)`
//!
//! // Rows grouped by day: `starts` the first row of each day, its header 32 high.
//! let head = |ui: &mut Ui, g: usize| {
//!   ui.text(&self.days[g].title, t.label());
//!   ui.spacer();
//!   ui.text(&self.days[g].note, t.muted());
//! };
//! let list = List::new(self.entries.len(), 44.0).sections(&self.starts, 32.0, &head);
//! ```
//!
//! Sections, as SwiftUI's `Section(header:)`: the rows stay one run of indices, `starts` says
//! where each section begins and a header is built with `head(ui, section)` above its first row.
//! Headers are not rows: they take no selection, focus or keys, Shift and Cmd-A cover the rows
//! only, and all are one height, so where a row lies is still arithmetic (a binary search of
//! `starts`) and a list of 100k rows in a thousand sections costs what one without them does. A
//! header's box has the rows' side padding, its content at the bottom.
//!
//! The look is a plain list, or [ListStyle::Grouped] ([List::list_style]): each section's rows on
//! a rounded plate as a grouped form's ([FormStyle::plates]), the header over it, the list's
//! own selection filling a row's piece of the plate. No hover fill, as SwiftUI's on macOS;
//! [Row::hover] is there for an app that draws its own.
//!
//! The rows to build come from the scroll offset as of the last frame and the input since,
//! clamped to the new content; when the layout shows more than was built (the viewport grew,
//! the offset was clamped) the view runs again at once, so a frame never shows a gap.
//!
//! Selection is by row key ([List::key], the index by default), never by visible element: it
//! stays right when rows scroll out, are inserted above or the list is re-sorted. It lives in
//! the app ([List::selection]) or the list's own state. A press selects, on a row's text too,
//! but one on a control in it (a button) is the control's, as in SwiftUI; with [List::multi],
//! Cmd-click (Ctrl-click on Windows and Linux) toggles a row and Shift-click selects from the
//! anchor; a secondary click selects the row alone unless it is selected, for the app's context
//! menu ([Ui::context_menu]), which one off the rows (the scrollbar, a header) doesn't open. Keys,
//! while the list has focus (a press gives it), come as the platform's commands: Up and Down (Shift
//! extends), Page Up and Page Down, Home and End or Cmd-Up and Cmd-Down, Cmd-A, and Enter, which
//! activates the row, as a double click does. Delete or Backspace (Cmd-Backspace too on a Mac) ask
//! to delete the selection ([ListResponse::delete], SwiftUI's `onDeleteCommand`); Escape clears it.
//! Either, with nothing selected, goes on to the app. The row the keys moved to scrolls into view;
//! so does [Ui::reveal_row]. Disabled ([Ui::disabled]), it takes no input or focus and dims.
//!
//! The list is the behaviour, its rows' fills the look: [List::unstyled] leaves the fills to
//! the app, which draws them from [Row].

use std::collections::HashSet;
use std::hash::BuildHasherDefault;
use std::ops::Range;

use keyboard_types::Modifiers;

use crate::app::MouseButton;
use crate::keys::{Binding, CMD, Command, Delete, Extent, Move, Os, Shortcut, bind};
use crate::scene::{Frame, Point, Rect, alpha};
use crate::text::FxHasher;
use crate::ui::form::Piece;
use crate::ui::input::ScrollState;
use crate::ui::{Align, Cursor, Focus, FormStyle, Id, Role, State, Style, Ui, grow, px};

/// Rows built past each edge of the viewport.
const OVERSCAN: usize = 2;

/// Cmd-A selects every row of a multi-select list.
const MULTI_KEYS: &[Binding] = &[bind(Shortcut::char('a').cmd(), Command::SelectAll)];

type KeySet = HashSet<u64, BuildHasherDefault<FxHasher>>;

/// What builds a section's header: `head(ui, section)`.
type Head<'s> = &'s dyn Fn(&mut Ui<'_>, usize);

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

  /// Keeps the keys `f` keeps (rows still there after a delete or a reload), the cursor and the
  /// anchor too while theirs is kept; `f` is asked once per key.
  pub fn retain(&mut self, mut f: impl FnMut(u64) -> bool) {
    // A cursor or an anchor off the selection (Cmd-click took its row out) is asked about here;
    // one on a selected row goes with its key.
    let off = |at: Option<(u64, usize)>| at.map(|a| a.0).filter(|k| !self.keys.contains(k));
    let (c, a) = (off(self.cursor), off(self.anchor));
    let keep_c = c.map(&mut f);
    let keep_a = if a == c { keep_c } else { a.map(&mut f) };
    self.keys.retain(|&k| f(k));
    self.cursor = self.cursor.filter(|c| keep_c.unwrap_or_else(|| self.keys.contains(&c.0)));
    self.anchor = self.anchor.filter(|a| keep_a.unwrap_or_else(|| self.keys.contains(&a.0)));
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

  /// What a click on row `i` of `n` rows keyed by `key` does in a multi-select [List]: Cmd
  /// toggles it ([Self::toggle]), else Shift extends to it ([Self::extend]), else it is selected
  /// alone ([Self::select]). A list's rows do it already; an app calls it for rows it lays out
  /// itself, such as rows grouped under headings.
  pub fn click(&mut self, i: usize, mods: Modifiers, n: usize, key: &dyn Fn(usize) -> u64) {
    match (mods.contains(CMD), mods.shift()) {
      (true, _) => self.toggle(key(i), i),
      (_, true) => self.extend(i, n, key),
      _ => self.select(key(i), i),
    }
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
  /// Delete or Backspace with rows selected: the app asks to delete them.
  pub delete: bool,
  /// The rows built.
  pub shown: Range<usize>,
  pub focused: bool,
}

/// Where a list's rows lie from the top of its content: `n` rows `h` high, and a header `head`
/// high above the first row of each section (`starts`, ascending, none empty), `gap` above it
/// unless it is at the top.
#[derive(Clone, Copy)]
struct Spans<'s> {
  n: usize,
  h: f32,
  starts: &'s [usize],
  head: f32,
  gap: f32,
}

impl Spans<'_> {
  /// The headers above row `i`'s own: those of the sections that start before it.
  fn heads(&self, i: usize) -> usize {
    self.starts.partition_point(|&s| s < i)
  }

  /// The top of row `i`, or of its section's header when it starts one; the content's height
  /// for `n`.
  fn top(&self, i: usize) -> f32 {
    i as f32 * self.h + self.above(self.heads(i))
  }

  /// The height of the first `k` headers with their gaps.
  fn above(&self, k: usize) -> f32 {
    let top = (self.starts.first() == Some(&0)) as usize;
    k as f32 * self.head + k.saturating_sub(top) as f32 * self.gap
  }

  /// The row at depth `y`, or under the header there, clamped to the rows.
  fn at(&self, y: f32) -> usize {
    let g = self.starts.partition_point(|&s| self.top(s) <= y);
    let (from, y) = match g {
      0 => (0, y),
      g => (self.starts[g - 1], y - self.starts[g - 1] as f32 * self.h - self.above(g)),
    };

    (from + (y / self.h).floor().max(0.0) as usize).min(self.n.saturating_sub(1))
  }
}

/// A [List]'s look ([List::list_style]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListStyle {
  /// Rows edge to edge, a selected one filled, as SwiftUI's `.plain`.
  #[default]
  Plain,
  /// Each section's rows on a rounded plate, as a grouped form's ([FormStyle::plates]): the
  /// header above it, a hairline between rows (inside the row's height, at its top; none beside
  /// a selected row), the selection filling a row's piece of the plate. The box is padded as a
  /// form page and the rows as a form's, unless [List::style] or [List::row_style] say otherwise.
  /// SwiftUI's `InsetGroupedListStyle` is this on iOS only; on macOS it has none.
  Grouped,
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
  /// Where each section starts, its header's height and what builds it.
  sections: Option<(&'s [usize], f32, Head<'s>)>,
  /// The box and the rows' as the app set them, else the look's.
  style: Option<Style>,
  row: Option<Style>,
  /// Rows filled from the theme when selected.
  fill: bool,
  /// Plain, or each section's rows on a plate ([List::list_style]).
  look: ListStyle,
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
      sections: None,
      style: None,
      row: None,
      fill: true,
      look: ListStyle::Plain,
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

  /// Rows in sections, as SwiftUI's `Section(header:)`: section `g` starts at row `starts[g]`
  /// (ascending, none empty), under a header `head_h` high built with `head(ui, g)`, which
  /// takes no selection, focus or keys (see the module's docs). Rows before the first start have
  /// none; grouped, a header but one at the top has the gap between plates above it, on
  /// top of `head_h`. One slice and one callback rather than a section per value: the app keeps the
  /// starts it has, the list finds a row's place by a binary search of them, and it builds only
  /// the headers in view, as it does the rows.
  pub fn sections(self, starts: &'s [usize], head_h: f32, head: Head<'s>) -> Self {
    Self { sections: Some((starts, head_h, head)), ..self }
  }

  /// The list's box (a scroll area): its size and padding; its gap is 0.
  pub fn style(self, style: Style) -> Self {
    Self { style: Some(style), ..self }
  }

  /// Each row's box: width, padding, gap and alignment; the list sets its height and fill.
  pub fn row_style(self, row: Style) -> Self {
    Self { row: Some(row), ..self }
  }

  /// The list's look, as SwiftUI's `.listStyle` ([ListStyle]); plain by default.
  pub fn list_style(self, look: ListStyle) -> Self {
    Self { look, ..self }
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

    let List { n, h, fill, multi, look, .. } = self;
    let grouped = look == ListStyle::Grouped;
    let fs = FormStyle::plates(&t);
    let row = Style::new().w(grow()).align(Align::Start, Align::Center);
    let (style, row) = match grouped {
      true => (Style::new().grow().pads(fs.page.pad), row.pad_xy(fs.row.pad[1], 0.0).gap(fs.gap)),
      false => (Style::new().grow(), row.pads([0.0, 14.0, 0.0, 10.0]).gap(8.0)),
    };

    let (style, row) = (self.style.unwrap_or(style), self.row.unwrap_or(row));
    let (starts, head_h, head) = self.sections.unwrap_or((&[], 0.0, &|_, _| {}));
    // Grouped, the space between plates above each header but one at the top.
    let gap = if grouped { fs.page.gap + fs.head_pad[0] } else { 0.0 };
    let sp = Spans { n, h, starts, head: head_h, gap };
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
      ui.list_keys(id, &mut st, sel, &mut out, (sp, view_h, multi), &key);
    }

    if !disabled {
      ui.list_press(id, &mut st, sel, &mut out, n, multi, &key);
    }

    // Scroll a row into view, by the last frame's viewport; the first and the last with the
    // list's padding.
    // Not laid out yet, it waits for the next frame, which comes at once.
    match (st.reveal, sc) {
      (Some(i), Some(sc)) => {
        st.reveal = None;
        let top = if i == 0 { 0.0 } else { style.pad[0] + sp.top(i) };
        let end = style.pad[0] + sp.top(i + 1) + if i + 1 == n { style.pad[2] } else { 0.0 };
        if i < n {
          ui.reveal(id, Rect::new(sc.offset.x, top, 1.0, end - top));
        }
      }
      (Some(_), None) => ui.st.animating = true,
      _ => {}
    }

    let sel_bg = match focused {
      true => t.selection,
      false => alpha(t.text, if t.scheme.is_dark() { 0.10 } else { 0.08 }),
    };

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
      let total = sp.top(n);
      let max = (pad_t + total + pad_b - view_h).max(0.0);
      let off = ui.scroll_raw(id).y.clamp(0.0, max);
      let at = |y: f32| sp.at(y - pad_t);
      shown = match n {
        0 => 0..0,
        _ => at(off).saturating_sub(OVERSCAN)..(at(off + view_h) + 1 + OVERSCAN).min(n),
      };
      ui.el(Style::new().h(px(sp.top(shown.start))));
      st.shown.extend(shown.clone().map(key));
      st.from = shown.start;
      // A header: the rows' side padding, its content at the bottom, by its first row.
      let [_, pad_r, _, pad_l] = row.pad;
      let below = if grouped { fs.head_pad[2] } else { 4.0 };
      let hs = Style { pad: [0.0, pad_r, below, pad_l], ..row }.bg([0.0; 4]);
      let hs = hs.align(Align::Start, Align::End);
      let dim = if disabled { t.disabled } else { 1.0 };
      let mut above = false;
      for (i, &k) in shown.clone().zip(&st.shown) {
        let g = sp.heads(i);
        let starts_here = starts.get(g) == Some(&i);
        if starts_here {
          let hs = hs.h(px(sp.above(g + 1) - sp.above(g)));
          ui.key(("head", k)).row(hs, |ui| head(ui, g));
        }

        let rid = id.key(("row", k));
        let selected = sel.contains(k);
        let hover = ui.hot(rid, !disabled && ui.hovered(rid));
        let bg = if fill && selected { sel_bg } else { [0.0; 4] };

        ui.a11y(rid, Role::Row).state(State::Selected, selected);
        let cursor = sel.cursor.is_some_and(|c| c.0 == k);
        let r = Row { index: i, key: k, id: rid, selected, hover, cursor, focused };
        // Disabled, the same id (keyed in the list) taking no input.
        let ui = match disabled {
          true => ui.key(("row", k)),
          false => ui.with_id(rid),
        };

        let s = if disabled { row.h(px(h)) } else { row.h(px(h)).cursor(Cursor::Pointer) };
        if !grouped {
          ui.row(s.bg(bg), |ui| f(ui, r));
          continue;
        }

        // A piece of its section's plate, rounded at the section's ends; the fill and the
        // hairline painted over it.
        let first = i == 0 || starts_here;
        let last = i + 1 == n || starts.get(sp.heads(i + 1)) == Some(&(i + 1));
        let corner = fs.group.radii[0];
        let (top, end) = (first as u8 as f32 * corner, last as u8 as f32 * corner);
        let radii = [top, top, end, end];
        let lined = (!first && !selected && !above) as u8 as f32;
        let line = alpha(fs.divider, fs.divider[3] * dim * lined);
        let (dimmed, inset, hair) = (alpha(bg, bg[3] * dim), fs.inset, fs.hairline);
        let piece = Piece { fill: dimmed, radii, line, inset, hair };
        let paint = move |f: &mut Frame, at: Rect| piece.paint(f, at);
        ui.canvas_row(Style { radii, ..s.bg(fs.group.bg) }, paint, |ui| f(ui, r));
        above = selected;
      }

      ui.el(Style::new().h(px(total - sp.top(shown.end))));
      // What the built rows cover, for the runtime to run the view again if the layout shows
      // more (the ends are open when they reach the list's).
      let lo = if shown.start == 0 { f32::NEG_INFINITY } else { pad_t + sp.top(shown.start) };
      let hi = if shown.end == n { f32::INFINITY } else { pad_t + sp.top(shown.end) };
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
    let raw = self.st.store.peek::<ScrollState>(id);
    let raw = raw.map(|s| Point::new(s.offset[0] as f32, s.offset[1] as f32));
    raw.or_else(|| self.scroll(id).map(|s| s.offset)).unwrap_or_default()
  }

  /// The list's keys, as commands: the cursor moves (Shift extends a multi-select), Enter
  /// activates, Cmd-A selects all, Delete asks to delete the selection, Escape clears it; `geo` is
  /// where the rows lie, the viewport's height and whether several rows can be selected.
  fn list_keys(
    &mut self,
    id: Id,
    st: &mut ListState,
    sel: &mut Selection,
    out: &mut ListResponse,
    geo: (Spans, f32, bool),
    key: &dyn Fn(usize) -> u64,
  ) {
    let (sp, view_h, multi) = geo;
    let n = sp.n;
    let Some(c) = self.command(id) else { return };
    let cur = sel.cursor_index(n, key);
    // A page: the farthest row within a viewport of the cursor's row, by the rows' own tops
    // (under their headers), its bottom from the cursor's top going down, its top from the
    // cursor's bottom going up; at least one row.
    let page = |dir: isize| {
      let c = cur.unwrap_or(0);
      let top = |i: usize| i as f32 * sp.h + sp.above(sp.heads(i + 1));
      let reach = view_h - sp.h;
      match dir > 0 {
        true => {
          let y = top(c) + reach;
          let i = sp.at(y);
          let i = if top(i) > y { i.saturating_sub(1) } else { i };
          i.max(c + 1).min(n - 1)
        }
        false => {
          let y = top(c) - reach;
          let i = sp.at(y);
          let i = if top(i) < y { i + 1 } else { i };
          i.min(c.saturating_sub(1))
        }
      }
    };
    let step = |d: isize| match cur {
      Some(c) => (c as isize + d).clamp(0, n as isize - 1) as usize,
      None if d > 0 => 0,
      None => n - 1,
    };
    use Delete::*;
    use Move::*;
    let (to, shift) = match c {
      Command::Move(Up, s) => (step(-1), s),
      Command::Move(Down, s) => (step(1), s),
      Command::Move(PageUp, s) => (page(-1), s),
      Command::Move(PageDown, s) => (page(1), s),
      Command::ScrollPageUp => (page(-1), Extent::Caret),
      Command::ScrollPageDown => (page(1), Extent::Caret),
      Command::Move(DocStart | LineStart, s) => (0, s),
      Command::Move(DocEnd | LineEnd, s) => (n - 1, s),
      Command::ScrollToTop => (0, Extent::Caret),
      Command::ScrollToBottom => (n - 1, Extent::Caret),
      Command::InsertNewline => {
        out.activated = cur;
        return;
      }
      Command::SelectAll if multi => {
        sel.select_all(n, key);
        out.changed = true;
        return;
      }
      Command::Delete(Backward | Forward | ToLineStart) if !sel.is_empty() => {
        out.delete = true;
        return;
      }
      Command::Cancel if !sel.is_empty() => {
        sel.clear();
        out.changed = true;
        return;
      }
      _ => {
        self.pass(id);
        return;
      }
    };

    match shift == Extent::Selection && multi {
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
    // The secondary button's press, on Windows its release (a Control-click is the Mac's).
    let win = self.st.os == Os::Windows;
    let press = self
      .press()
      .filter(|p| p.button == MouseButton::Left || p.button == MouseButton::Right && !win);
    // A press is seen once; a click whose press no pass saw (a quick one) stands for it.
    let both = |b| matches!(b, MouseButton::Left | MouseButton::Right);
    let click = self.st.input.click.filter(|c| both(c.button));
    let hit = match (press, click) {
      (Some(p), _) if st.pressed != Some(p.seq) => Some((p.id, p.mods, p.clicks, p.button, p.seq)),
      (None, Some(c)) if st.pressed != Some(c.seq) => {
        Some((c.id, c.mods, c.clicks, c.button, c.seq))
      }
      _ => None,
    };

    st.pressed = press.map(|p| p.seq).or(click.map(|c| c.seq)).or(st.pressed);
    let Some((on, mods, clicks, button, seq)) = hit else { return };
    let right = button == MouseButton::Right;
    // Its index as built, a hint that saves a scan unless the rows moved since; a press on an
    // element in a row is the row's (text with a tooltip), unless that element is a control
    // (focusable: a button) or, for a secondary one, has a menu of its own, as in SwiftUI.
    let row = |on: Id| st.shown.iter().position(|&k| id.key(("row", k)) == on);
    let (t, mut own) = (&self.st.last, false);
    let mut up = t.find(on);
    let at = row(on).or_else(|| {
      loop {
        let n = &t.nodes[up? as usize];
        if let Some(at) = row(n.id) {
          break Some(at);
        }

        if n.id == id || up == Some(0) {
          break None;
        }

        own |= if right { t.menus.contains(&n.id) } else { n.style.focus != Focus::None };
        up = Some(n.parent);
      }
    });
    if own {
      return;
    }

    let Some((k, i)) =
      at.and_then(|at| Some((st.shown[at], find((st.shown[at], st.from + at), n, key)?)))
    else {
      // Off the rows (the scrollbar, the padding) the list's menu, a row's, opens none; with
      // none of its own, its parent's does.
      if right && self.st.last.menus.contains(&id) && self.st.last.within(on, id) {
        self.st.menu_seq = seq;
      }

      return;
    };
    match (right, multi) {
      (true, _) => {
        self.focus(Some(id));
        if sel.contains(k) {
          return;
        }

        sel.select(k, i);
      }
      (_, true) => sel.click(i, mods, n, key),
      _ => sel.select(k, i),
    }

    (out.pressed, out.changed) = (Some(i), true);
    if clicks == 2 && !right {
      out.activated = Some(i);
    }
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::app::Event;
  use crate::keys::CMD;
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
    // Rows gone: their keys go, the cursor with its row; the rest stays.
    s.retain(|k| k != 13 && k != 2);
    assert_eq!((s.len(), s.cursor()), (5, None));
    s.select(12, 4);
    s.retain(|k| k != 2);
    assert_eq!((sorted(&s), s.cursor()), (vec![12], Some(12)));
    // Each key is asked once: a predicate that drops 12 and would keep it when asked again
    // leaves no cursor on it.
    s.select_all(7, &moved);
    let mut asked = Vec::new();
    s.retain(|k| {
      asked.push(k);
      k != 12 || asked.iter().filter(|&&a| a == 12).count() > 1
    });
    asked.sort_unstable();
    assert_eq!(asked, [1, 2, 10, 11, 12, 13, 14]);
    assert!(!s.contains(12) && s.cursor().is_none());
    // A row Cmd-click took out keeps the cursor and the anchor while `f` keeps it, asked once.
    s.select_all(7, &moved);
    s.toggle(13, 5);
    asked.clear();
    s.retain(|k| {
      asked.push(k);
      k != 2
    });
    asked.sort_unstable();
    assert_eq!(asked, [1, 2, 10, 11, 12, 13, 14]);
    assert_eq!((s.contains(13), s.cursor()), (false, Some(13)));
    s.extend(6, 7, &moved);
    assert_eq!(sorted(&s), [13, 14]);
    s.toggle(13, 5);
    s.retain(|k| k != 13);
    assert_eq!((sorted(&s), s.cursor()), (vec![14], None));
    s.clear();
    assert!(s.is_empty() && s.cursor().is_none());
  }

  #[test]
  fn rows_lie_under_their_headers() {
    // Rows 0-2 and 3-5, 10 high, under headers 5 high.
    let sp = Spans { n: 6, h: 10.0, starts: &[0, 3], head: 5.0, gap: 0.0 };
    let tops: Vec<f32> = (0..=6).map(|i| sp.top(i)).collect();
    assert_eq!(tops, [0.0, 15.0, 25.0, 35.0, 50.0, 60.0, 70.0]);
    // A header's depth is its first row's; past the end, the last row.
    let at = [(0.0, 0), (4.9, 0), (5.0, 0), (15.0, 1), (34.9, 2), (36.0, 3), (49.9, 3)];
    for (y, i) in at.into_iter().chain([(50.0, 4), (1e6, 5), (-3.0, 0)]) {
      assert_eq!(sp.at(y), i, "at {y}");
    }

    // Rows before the first section have no header; with none, rows only.
    let sp = Spans { starts: &[2], ..sp };
    assert_eq!((sp.top(2), sp.top(3), sp.at(26.0)), (20.0, 35.0, 2));
    let sp = Spans { starts: &[], ..sp };
    assert_eq!((sp.top(6), sp.at(25.0)), (60.0, 2));
    // A gap of 20 above each header but one at the top.
    let sp = Spans { starts: &[0, 3], gap: 20.0, ..sp };
    assert_eq!((sp.top(1), sp.top(3), sp.top(4), sp.top(6)), (15.0, 35.0, 70.0, 90.0));
    assert_eq!((sp.at(50.0), sp.at(64.0), sp.at(75.0)), (3, 3, 4));
    let sp = Spans { starts: &[2], ..sp };
    assert_eq!((sp.top(2), sp.top(3), sp.at(30.0)), (20.0, 55.0, 2));
  }

  #[test]
  fn selection_click_follows_modifiers() {
    let key = |i: usize| [10, 11, 12, 13, 14][i];
    let (none, shift) = (Modifiers::empty(), Modifiers::SHIFT);
    let mut s = Selection::default();
    // Shift with no anchor yet starts at the row.
    s.click(2, shift, 5, &key);
    assert_eq!(sorted(&s), [12]);
    // A plain click selects the row alone.
    s.select_all(5, &key);
    s.click(2, none, 5, &key);
    assert_eq!((sorted(&s), s.cursor()), (vec![12], Some(12)));
    // Shift extends from the anchor, both ways, replacing the range.
    s.click(4, shift, 5, &key);
    assert_eq!(sorted(&s), [12, 13, 14]);
    s.click(0, shift, 5, &key);
    assert_eq!((sorted(&s), s.cursor()), (vec![10, 11, 12], Some(10)));
    // Cmd toggles, out and back in, and wins over Shift.
    s.click(1, CMD, 5, &key);
    assert_eq!(sorted(&s), [10, 12]);
    s.click(4, CMD | shift, 5, &key);
    assert_eq!(sorted(&s), [10, 12, 14]);
    s.click(4, CMD, 5, &key);
    assert_eq!(sorted(&s), [10, 12]);
    // Shift after Cmd extends from the row Cmd clicked last.
    s.click(2, shift, 5, &key);
    assert_eq!(sorted(&s), [12, 13, 14]);
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
    h.event(Event::Focus(false));
    let inactive = px(&mut h);
    assert_ne!(active, inactive);
    h.event(Event::Focus(true));
    // The same again on one row: its press selects it, then the quick Cmd-click's toggles it.
    h.press(b.x + 5.0, b.y + 5.0);
    h.frame();
    h.move_to(250.0, 250.0);
    h.release(250.0, 250.0);
    h.move_to(b.x + 5.0, b.y + 5.0);
    h.press_with(b.x + 5.0, b.y + 5.0, 1, CMD);
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

  /// 30 rows 20 high in three sections of ten under headers 30 high, the app holding the
  /// selection and counting the deletes the list asked for.
  #[derive(Default)]
  struct Days {
    sel: Selection,
    deletes: usize,
  }

  impl crate::App for Days {
    fn view(&mut self, ui: &mut Ui) {
      let head = |ui: &mut Ui, _: usize| ui.label("head");
      ui.col(Style::new().w(px(200.0)).h(px(200.0)), |ui| {
        let list = List::new(30, 20.0).sections(&[0, 10, 20], 30.0, &head);
        let r = list.multi().selection(&mut self.sel).show(ui.id("list"), |ui, _| ui.label("row"));
        self.deletes += r.delete as usize;
      });
    }
  }

  #[test]
  fn headers_take_no_selection_keys_or_cmd_a() {
    use crate::NamedKey as K;
    use crate::harness::Harness;
    let mut h = Harness::new(Days::default(), Size::new(300.0, 300.0), 1.0);
    let id = Id::of("list");
    let (row, head) = (|k: u64| id.key(("row", k)), |k: u64| id.key(("head", k)));
    h.frame();
    // Each section's first row under its header.
    let (top, r0) = (h.rect(id).unwrap().y, h.rect(row(0)).unwrap());
    assert_eq!((h.rect(head(0)).unwrap().h, r0.y - top), (30.0, 30.0));
    // A click on a header selects nothing.
    h.click_id(head(0));
    assert!(h.app().sel.is_empty());
    // Down from a section's last row goes to the next one's first, Up back over its header;
    // Shift extends over the rows only, Cmd-A selects the 30 rows.
    h.click_id(row(7));
    for _ in 0..3 {
      h.key(K::ArrowDown, Modifiers::empty());
    }

    assert_eq!(h.app().sel.cursor(), Some(10));
    h.frame();
    let (r9, r10) = (h.rect(row(9)).unwrap(), h.rect(row(10)).unwrap());
    assert_eq!(r10.y - (r9.y + r9.h), 30.0);
    h.key(K::ArrowUp, Modifiers::SHIFT);
    h.key(K::ArrowUp, Modifiers::SHIFT);
    assert_eq!(sorted(&h.app().sel), [8, 9, 10]);
    h.key(crate::Key::Char('a'), CMD);
    assert_eq!(h.app().sel.len(), 30);
    // End: the last row in view, under its section's header, the first rows no longer built.
    h.key(K::End, Modifiers::empty());
    h.frame();
    let (v, last) = (h.rect(id).unwrap(), h.rect(row(29)).unwrap());
    assert!(last.y + last.h <= v.y + v.h + 0.5, "{last:?} in {v:?}");
    assert!(h.rect(head(20)).is_some() && h.rect(row(0)).is_none());
  }

  /// 30 rows 20 high in sections of three under headers 30 high, the app holding the selection.
  #[derive(Default)]
  struct Threes {
    sel: Selection,
  }

  impl crate::App for Threes {
    fn view(&mut self, ui: &mut Ui) {
      let head = |ui: &mut Ui, _: usize| ui.label("head");
      let starts: Vec<usize> = (0..30).step_by(3).collect();
      ui.col(Style::new().w(px(200.0)).h(px(200.0)), |ui| {
        let list = List::new(30, 20.0).sections(&starts, 30.0, &head).selection(&mut self.sel);
        list.show(ui.id("list"), |ui, _| ui.label("row"));
      });
    }
  }

  #[test]
  fn a_page_is_a_viewport_over_the_headers() {
    use crate::NamedKey as K;
    use crate::harness::Harness;
    let mut h = Harness::new(Threes::default(), Size::new(300.0, 300.0), 1.0);
    let id = Id::of("list");
    let row = |k: u64| id.key(("row", k));
    h.frame();
    h.click_id(row(0));
    // Rows 0 to 6 span the 200 of the viewport over three headers: a page down goes to row 6,
    // not the nine rows the height alone would give, and back up the same way; in view after.
    let none = Modifiers::empty();
    let mut at = Vec::new();
    for k in [K::PageDown, K::PageDown, K::PageUp, K::PageUp] {
      h.key(k, none);
      h.frame();
      at.push(h.app().sel.cursor());
    }

    assert_eq!(at, [Some(6), Some(12), Some(6), Some(0)]);
    h.key(K::PageDown, none);
    h.frame();
    let (v, r6) = (h.rect(id).unwrap(), h.rect(row(6)).unwrap());
    assert!(r6.y >= v.y && r6.y + r6.h <= v.y + v.h + 0.5, "{r6:?} in {v:?}");
  }

  #[test]
  fn delete_asks_and_escape_clears() {
    use crate::NamedKey as K;
    use crate::harness::Harness;
    let mut h = Harness::new(Days::default(), Size::new(300.0, 300.0), 1.0);
    let none = Modifiers::empty();
    h.set_os(Os::Mac);
    h.frame();
    h.click_id(Id::of("list").key(("row", 2u64)));
    // Backspace, Delete and Cmd-Backspace ask to delete the selection, which stays.
    h.key(K::Backspace, none);
    h.key(K::Delete, none);
    h.key(K::Backspace, Os::Mac.cmd());
    assert_eq!((h.app().deletes, h.app().sel.len()), (3, 1));
    // Escape clears it; with nothing selected, neither key is the list's.
    h.key(K::Escape, none);
    assert!(h.app().sel.is_empty());
    assert!(!h.key(K::Backspace, none) && !h.key(K::Escape, none));
    assert_eq!(h.app().deletes, 3);
  }

  /// Two sections of three rows 20 high, inset grouped, under headers 20 high.
  #[derive(Default)]
  struct Plates {
    sel: Selection,
  }

  impl crate::App for Plates {
    fn view(&mut self, ui: &mut Ui) {
      let head = |ui: &mut Ui, _: usize| ui.label("head");
      let list = List::new(6, 20.0).sections(&[0, 3], 20.0, &head).list_style(ListStyle::Grouped);
      list.selection(&mut self.sel).show(ui.id("list"), |_, _| {});
    }
  }

  #[test]
  fn inset_grouped_rows_lie_on_a_plate_per_section() {
    use crate::harness::Harness;
    let mut h = Harness::new(Plates::default(), Size::new(300.0, 300.0), 1.0);
    let id = Id::of("list");
    let (row, head) = (|k: u64| id.key(("row", k)), |k: u64| id.key(("head", k)));
    h.frame();
    // Inset as a form page; the second header the gap between plates under the first plate.
    let list = h.rect(id).unwrap();
    let r: Vec<Rect> = (0..6).map(|k| h.rect(row(k)).unwrap()).collect();
    assert_eq!((r[0].x - list.x, list.x + list.w - r[0].x - r[0].w), (20.0, 20.0));
    let (h0, h3) = (h.rect(head(0)).unwrap(), h.rect(head(3)).unwrap());
    assert_eq!((h0.y - list.y, r[0].y, h3.y, h3.h), (12.0, h0.y + 20.0, r[2].y + 20.0, 50.0));
    assert_eq!(r[3].y, h3.y + h3.h);
    // The plate under each section's rows, the ground between; rounded at a section's ends.
    let img = h.render();
    let at = |r: Rect, dy: f32| img.pixel(r.x + r.w - 30.0, r.y + dy);
    let (plate, ground) = (at(r[1], 10.0), img.pixel(r[2].x + 50.0, h3.y + 10.0));
    assert_ne!(plate, ground);
    assert_eq!((at(r[4], 10.0), at(r[2], 19.0)), (plate, plate));
    assert_eq!(img.pixel(r[0].x + 0.5, r[0].y + 0.5), ground);
    assert_eq!(img.pixel(r[1].x + 0.5, r[1].y + 0.5), plate);
    // A hairline at the top of every row but a section's first, none beside a selected row.
    assert_ne!(at(r[1], 0.5), plate);
    assert_eq!((at(r[0], 0.5), at(r[3], 0.5)), (plate, plate));
    h.click_id(row(1));
    let img = h.render();
    let at = |r: Rect, dy: f32| img.pixel(r.x + r.w - 30.0, r.y + dy);
    assert_ne!(at(r[1], 10.0), plate);
    assert_eq!(at(r[2], 0.5), plate);
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
    h.rt.ui.store.raw::<ScrollState>(Id::of("list")).offset[1] = 1_999_000.0;
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

  /// Rows with a button each, an odd row's with a menu of its own, under the list's row menu,
  /// and a text with an id of its own (a tooltip's); the buttons' clicks.
  #[derive(Default)]
  struct Buttons {
    sel: Selection,
    clicks: usize,
  }

  impl crate::App for Buttons {
    fn view(&mut self, ui: &mut Ui) {
      let list = List::new(10, 20.0).multi().selection(&mut self.sel).style(Style::new().grow());
      let mut clicks = 0;
      let r = list.show(ui.id("list"), |ui, row| {
        ui.id(("t", row.index)).label("Tip");
        let b = ui.id(("b", row.index)).button("B");
        clicks += b.clicked as usize;
        if row.index % 2 == 1 {
          ui.context_menu(b.id, || crate::Menu::new(vec![crate::Item::button("own", "Own")]));
        }
      });
      ui.context_menu(r.id, || crate::Menu::new(vec![crate::Item::button("row", "Row")]));
      self.clicks += clicks;
    }
  }

  /// As in SwiftUI: a click on a button in a row is the button's, the selection as it was; one
  /// on a row's text, named for a tooltip, selects the row, Cmd-click adding it.
  #[test]
  fn a_click_on_a_rows_button_is_the_buttons_on_its_text_the_rows() {
    use crate::harness::Harness;
    let mut h = Harness::new(Buttons::default(), Size::new(200.0, 200.0), 1.0);
    h.frame();
    h.click_id(Id::of(("t", 1)));
    h.frame();
    assert_eq!((h.app().sel.cursor(), h.app().clicks), (Some(1), 0));
    h.click_id(Id::of(("b", 3)));
    h.frame();
    assert_eq!((sorted(&h.app().sel), h.app().clicks), (vec![1], 1));
    let r = h.rect(Id::of(("t", 4))).unwrap();
    let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    h.press_with(x, y, 1, crate::CMD);
    h.release(x, y);
    h.frame();
    assert_eq!(sorted(&h.app().sel), [1, 4]);
  }

  #[test]
  fn a_right_click_on_a_rows_element_is_the_rows_unless_it_has_a_menu() {
    use crate::harness::Harness;
    let mut h = Harness::new(Buttons::default(), Size::new(200.0, 200.0), 1.0);
    h.native_menus();
    h.frame();
    let at = |h: &Harness<Buttons>, i: usize| {
      let r = h.rect(Id::of(("b", i))).unwrap();
      (r.x + r.w / 2.0, r.y + r.h / 2.0)
    };
    let asked =
      |h: &mut Harness<Buttons>| h.context_menu(None).map(|(_, m)| m.items[0].label().to_owned());
    // A button with no menu: its row's menu, that row selected.
    let (x, y) = at(&h, 2);
    h.right_click(x, y);
    h.frame();
    assert_eq!(asked(&mut h).as_deref(), Some("Row"));
    assert_eq!(h.app().sel.cursor(), Some(2));
    // One with its own: that menu, the selection as it was.
    let (x, y) = at(&h, 3);
    h.right_click(x, y);
    h.frame();
    assert_eq!(asked(&mut h).as_deref(), Some("Own"));
    assert_eq!(h.app().sel.cursor(), Some(2));
  }

  /// Two rows in a list with no menu, inside an element with one.
  struct Inside;

  impl crate::App for Inside {
    fn view(&mut self, ui: &mut Ui) {
      let r = ui.id("outer").col(Style::new().w(px(200.0)).h(px(200.0)), |ui| {
        let list = List::new(2, 20.0).style(Style::new().grow());
        list.show(ui.id("list"), |ui, _| ui.label("row"));
      });
      ui.context_menu(r.id, || crate::Menu::new(vec![crate::Item::button("outer", "Outer")]));
    }
  }

  #[test]
  fn a_list_with_no_menu_leaves_the_press_off_its_rows_to_its_parent() {
    use crate::harness::Harness;
    let mut h = Harness::new(Inside, Size::new(300.0, 300.0), 1.0);
    h.native_menus();
    h.frame();
    h.right_click(100.0, 150.0);
    h.frame();
    let asked = h.context_menu(None).map(|(_, m)| m.items[0].label().to_owned());
    assert_eq!(asked.as_deref(), Some("Outer"));
  }
}
