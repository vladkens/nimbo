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
//! menu ([Ui::context_menu]), which one off the rows (the scrollbar, a header) doesn't open. A
//! plain press on the list's ground (a header, the padding, the room under the rows; not a
//! control there nor a scrollbar) clears the selection, as a table view's on macOS. Keys,
//! while the list has focus (a press gives it), come as the platform's commands: Up and Down (Shift
//! extends), Page Up and Page Down, Home and End or Cmd-Up and Cmd-Down, Cmd-A, and Enter, which
//! activates the row, as a double click does. Delete or Backspace (Cmd-Backspace too on a Mac) ask
//! to delete the selection ([ListResponse::delete], SwiftUI's `onDeleteCommand`); Escape clears it.
//! Either, with nothing selected, goes on to the app. The row the keys moved to scrolls into view;
//! so does [Ui::reveal_row]. Disabled ([Ui::disabled]), it takes no input or focus and dims.
//!
//! The behaviour is [Ui::list_behavior] and [Ui::list_rows] (selection, keys, presses,
//! virtualization), its rows' fills the look: [List::unstyled] leaves the fills to the app, which
//! draws them from [Row].

use std::ops::Range;

use crate::scene::{Frame, Rect, alpha};
pub use crate::ui::behavior::Selection;
use crate::ui::form::Piece;
use crate::ui::{Align, Cursor, FormStyle, Id, ListRows, Style, Ui, grow, px};

/// What builds a section's header: `head(ui, section)`.
type Head<'s> = &'s dyn Fn(&mut Ui<'_>, usize);

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
  /// selects all. Makes its selection [Selection::multiple], as one already so does.
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
    /// The selection, when the app doesn't hold it.
    #[derive(Default)]
    struct Own(Selection);

    let t = ui.theme();
    let id = ui.take_id();
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
    let rows = ListRows::new(n, h).sections(starts, head_h).section_spacing(gap);
    let rows = rows.padding(style.pad[0], style.pad[2]);
    let keyf = self.key;
    let key = move |i: usize| keyf.map_or(i as u64, |k| k(i));
    let mut own = None;
    let sel = match self.sel {
      Some(s) => s,
      None => own.insert(std::mem::take(&mut ui.state::<Own>(id).0)),
    };

    if multi && !sel.is_multiple() {
      *sel = std::mem::take(sel).multiple();
    }

    let b = ui.list_behavior(id, rows, sel, &key);
    let (disabled, focused) = (b.disabled, b.focused);
    // Its selection in the accent while it has focus in the key window.
    let sel_bg = match focused {
      true => t.selection,
      false => alpha(t.text, if t.scheme.is_dark() { 0.10 } else { 0.08 }),
    };

    // Disabled, it dims.
    let style = b.style(style.gap(0.0));
    let style = if disabled { style.opacity(t.disabled) } else { style };
    let sel = b.selection();
    let mut shown = 0..0;
    ui.with_id(id).scroll_area(style, |ui| {
      // A header: the rows' side padding, its content at the bottom, by its first row.
      let [_, pad_r, _, pad_l] = row.pad;
      let below = if grouped { fs.head_pad[2] } else { 4.0 };
      let hs = Style { pad: [0.0, pad_r, below, pad_l], ..row }.bg([0.0; 4]);
      let hs = hs.align(Align::Start, Align::End);
      let dim = if disabled { t.disabled } else { 1.0 };
      let mut above = false;

      shown = ui.list_rows(&b, |ui, i, k| {
        let header = rows.header(i);
        if let Some(head_at) = header {
          ui.key(("head", k)).row(hs.h(px(head_at.height)), |ui| head(ui, head_at.section));
        }

        let rid = id.key(("row", k));
        let selected = sel.contains(k);
        let hover = ui.hot(rid, !disabled && ui.hovered(rid));
        let bg = if fill && selected { sel_bg } else { [0.0; 4] };
        let cursor = sel.cursor() == Some(k);
        let r = Row { index: i, key: k, id: rid, selected, hover, cursor, focused };

        // Disabled, the same id (keyed in the list) taking no input.
        let ui = match disabled {
          true => ui.key(("row", k)),
          false => ui.with_id(rid),
        };

        let s = if disabled { row.h(px(h)) } else { row.h(px(h)).cursor(Cursor::Pointer) };
        if !grouped {
          ui.row(s.bg(bg), |ui| f(ui, r));
          return;
        }

        // A piece of its section's plate, rounded at the section's ends; the fill and the
        // hairline painted over it.
        let first = i == 0 || header.is_some();
        let last = i + 1 == n || rows.header(i + 1).is_some();
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
      });
    });

    let (changed, pressed, activated, delete) = (b.changed, b.pressed, b.activated, b.delete);
    if let Some(own) = own {
      ui.state::<Own>(id).0 = own;
    }

    ListResponse { id, changed, pressed, activated, delete, shown, focused }
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
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use keyboard_types::Modifiers;

  use super::*;
  use crate::app::Event;
  use crate::keys::{CMD, Os};
  use crate::scene::{Point, Size};
  use crate::ui::UiState;
  use crate::ui::input::ScrollState;
  use crate::ui::layout::tests::Fake;

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
    h.set_native_menus(true);
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

  /// Six rows 20 high in two sections under headers 30 high, each with a text, a button and a
  /// disabled one; a box after the list. Scrolls in its 100 high.
  #[derive(Default)]
  struct Ground {
    sel: Selection,
    clicks: usize,
  }

  impl crate::App for Ground {
    fn view(&mut self, ui: &mut Ui) {
      let clicks = std::cell::Cell::new(0);
      let head = |ui: &mut Ui, g: usize| {
        ui.id(("t", g)).label("head");
        ui.spacer();
        clicks.set(clicks.get() + ui.id(("b", g)).button("B").clicked as usize);
        ui.disabled(true).id(("off", g)).button("Off");
      };
      ui.col(Style::new().w(px(200.0)).h(px(100.0)), |ui| {
        let list = List::new(6, 20.0).sections(&[0, 3], 30.0, &head).multi();
        let list = list.selection(&mut self.sel).style(Style::new().grow());
        list.show(ui.id("list"), |ui, _| ui.label("row"));
      });
      ui.id("after").el(Style::new().w(px(200.0)).h(px(50.0)));
      self.clicks += clicks.get();
    }
  }

  /// As a table view on macOS: a plain press on a header, its text or the room beside it,
  /// clears the selection; one with Cmd or Shift, on a header's button, enabled or not, on the
  /// scrollbar or off the list keeps it.
  #[test]
  fn a_press_on_the_lists_ground_clears_the_selection() {
    use crate::harness::Harness;
    let mut h = Harness::new(Ground::default(), Size::new(300.0, 300.0), 1.0);
    let id = Id::of("list");
    let row = |k: u64| id.key(("row", k));
    h.frame();
    let pick = |h: &mut Harness<Ground>| {
      h.click_id(row(1));
      h.frame();
      assert_eq!(sorted(&h.app().sel), [1]);
    };

    pick(&mut h);
    let (t, b) = (h.rect(Id::of(("t", 0))).unwrap(), h.rect(Id::of(("b", 0))).unwrap());
    let (x, y) = ((t.x + t.w + b.x) / 2.0, t.y + t.h / 2.0);
    for mods in [CMD, Modifiers::SHIFT] {
      h.press_with(x, y, 1, mods);
      h.release(x, y);
      h.frame();
    }

    h.click_id(Id::of(("b", 0)));
    h.click_id(Id::of(("off", 0)));
    h.click_id(Id::of("after"));
    h.frame();
    assert_eq!((sorted(&h.app().sel), h.app().clicks), (vec![1], 1));

    // The scrollbar, shown with the pointer in its strip: its thumb keeps the selection.
    let v = h.rect(id).unwrap();
    h.move_to(v.x + v.w - 3.0, v.y + 20.0);
    h.frame();
    let thumb = h.rect(id.key(("thumb", 1usize))).expect("a thumb");
    h.click(thumb.x + thumb.w / 2.0, thumb.y + thumb.h / 2.0);
    h.frame();
    assert_eq!(sorted(&h.app().sel), [1]);

    // The room beside the header's text clears it; its text too.
    h.click(x, y);
    h.frame();
    assert!(h.app().sel.is_empty());
    pick(&mut h);
    h.click_id(Id::of(("t", 0)));
    h.frame();
    assert!(h.app().sel.is_empty());
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
    h.set_native_menus(true);
    h.frame();
    h.right_click(100.0, 150.0);
    h.frame();
    let asked = h.context_menu(None).map(|(_, m)| m.items[0].label().to_owned());
    assert_eq!(asked.as_deref(), Some("Outer"));
  }
  #[derive(Default)]
  struct Bare {
    sel: Selection,
    bare: bool,
  }

  impl crate::App for Bare {
    fn view(&mut self, ui: &mut Ui) {
      ui.col(Style::new().w(px(200.0)).h(px(100.0)), |ui| {
        let list = List::new(3, 20.0).selection(&mut self.sel).style(Style::new().grow());
        let list = list.row_style(Style::new().w(grow()).pad(8.0));
        let list = if self.bare { list.unstyled() } else { list };
        let inner = Style::new().w(px(10.0)).h(px(4.0));
        list.show(ui.id("list"), |ui, row| _ = ui.id(("in", row.index)).el(inner));
      });
    }
  }

  /// A row style's padding moves the row's content in.
  #[test]
  fn a_row_styles_padding_moves_the_content_in() {
    use crate::harness::Harness;
    let mut h = Harness::new(Bare::default(), Size::new(300.0, 300.0), 1.0);
    h.frame();
    let row = h.rect(Id::of("list").key(("row", 1u64))).unwrap();
    let inner = h.rect(Id::of(("in", 1))).unwrap();
    assert_eq!((inner.x - row.x, inner.y - row.y), (8.0, 8.0));
  }

  /// Unstyled, a selected row keeps the list's ground for the app to paint.
  #[test]
  fn unstyled_rows_leave_the_selection_to_the_app() {
    use crate::harness::Harness;
    let mut h = Harness::new(Bare::default(), Size::new(300.0, 300.0), 1.0);
    h.frame();
    let row = h.rect(Id::of("list").key(("row", 1u64))).unwrap();
    // Styled, the selection fills the row; unstyled, the row shows the ground as a row below.
    h.click_id(Id::of("list").key(("row", 1u64)));
    h.frame();
    let filled = h.render().pixel(row.x + 150.0, row.y + 10.0);

    h.app_mut().bare = true;
    h.frame();
    let img = h.render();
    let (bare, other) =
      (img.pixel(row.x + 150.0, row.y + 10.0), img.pixel(row.x + 150.0, row.y + 30.0));
    assert_eq!(h.app().sel.cursor(), Some(1));
    assert_ne!(bare, filled);
    assert_eq!(bare, other);
  }
}
