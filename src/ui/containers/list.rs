// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The System Settings lists over the headless [List]: rows edge to edge, the selected one filled
//! ([InsetListStyle], the window's default, which the shell puts), or each section's rows on a
//! rounded plate as a grouped form's ([InsetGroupedListStyle]). No hover fill, as SwiftUI's on
//! macOS; [Row::hovered] is there for an app that draws its own.

use crate::scene::{Color, Frame, Rect, alpha};
use crate::ui::form::Piece;
use crate::ui::{FormStyle, ListConfiguration, ListLayout, ListStyle, Style, Theme, Ui};
#[cfg(doc)]
use crate::ui::{List, Row};

/// The selected rows' fill: the accent while the list has focus in the key window, else grey.
fn selection(t: &Theme, focused: bool) -> Color {
  match focused {
    true => t.selection,
    false => alpha(t.text, if t.scheme.is_dark() { 0.10 } else { 0.08 }),
  }
}

/// Rows edge to edge, a selected one filled: in the accent while the list has focus, else grey;
/// dimmed when disabled. The window's default, which the shell puts.
#[derive(Clone, Copy, Debug, Default)]
pub struct InsetListStyle;

impl ListStyle for InsetListStyle {
  fn make_body<'a>(&self, ui: &mut Ui<'a>, mut c: ListConfiguration<'_, 'a>) {
    let t = ui.theme();
    let fill = selection(&t, c.is_focused());
    let frame = if c.is_enabled() { c.frame() } else { c.frame().opacity(t.disabled) };
    ui.scroll_area(frame, |ui| {
      c.rows(ui, |ui, mut r| {
        let bg = if r.is_selected() { fill } else { [0.0; 4] };
        ui.row(r.frame().bg(bg), |ui| r.content(ui));
      })
    });
  }
}

/// Each section's rows on a rounded plate, as a grouped form's ([FormStyle::plates]): the header
/// above it, a hairline between rows (inside the row's height, at its top; none beside a
/// selected row), the selection filling a row's piece of the plate. The box is padded as a form
/// page and the rows as a form's, unless [List::style] or [List::row_style] say otherwise; a
/// header but one at the top has the gap between plates above it. SwiftUI's
/// `InsetGroupedListStyle` is this on iOS only; on macOS it has none.
#[derive(Clone, Copy, Debug, Default)]
pub struct InsetGroupedListStyle;

impl ListStyle for InsetGroupedListStyle {
  fn layout(&self, ui: &Ui<'_>) -> ListLayout {
    let (fs, d) = (FormStyle::plates(&ui.theme()), ListLayout::default());
    ListLayout {
      frame: d.frame.pads(fs.page.pad),
      row: d.row.pad_xy(fs.row.pad[1], 0.0).gap(fs.gap),
      section_spacing: fs.page.gap + fs.head_pad[0],
      header_padding: fs.head_pad[2],
    }
  }

  fn make_body<'a>(&self, ui: &mut Ui<'a>, mut c: ListConfiguration<'_, 'a>) {
    let t = ui.theme();
    let (fs, fill) = (FormStyle::plates(&t), selection(&t, c.is_focused()));
    let dim = if c.is_enabled() { 1.0 } else { t.disabled };
    let frame = if c.is_enabled() { c.frame() } else { c.frame().opacity(dim) };
    ui.scroll_area(frame, |ui| {
      c.rows(ui, |ui, mut r| {
        // A piece of its section's plate, rounded at the section's ends; the fill and the
        // hairline painted over it.
        let bg = if r.is_selected() { fill } else { [0.0; 4] };
        let corner = fs.group.radii[0];
        let (top, end) = (r.is_first() as u8 as f32 * corner, r.is_last() as u8 as f32 * corner);
        let radii = [top, top, end, end];
        let lined = (!r.is_first() && !r.is_selected() && !r.is_after_selected()) as u8 as f32;
        let line = alpha(fs.divider, fs.divider[3] * dim * lined);

        let (dimmed, inset, hair) = (alpha(bg, bg[3] * dim), fs.inset, fs.hairline);
        let piece = Piece { fill: dimmed, radii, line, inset, hair };
        let paint = move |f: &mut Frame, at: Rect| piece.paint(f, at);
        let s = Style { radii, ..r.frame().bg(fs.group.bg) };
        ui.canvas_row(s, paint, |ui| r.content(ui));
      })
    });
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::Modifiers;
  use crate::app::Event;
  use crate::keys::{CMD, Os};
  use crate::scene::{Point, Size};
  use crate::ui::input::ScrollState;
  use crate::ui::layout::tests::Fake;
  use crate::ui::{Id, List, PlainListStyle, Selection, UiState, grow, px};

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
    use crate::Harness;
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
    assert_eq!(h.focus_id(), None);
    let c = h.rect(row(498)).unwrap();
    h.click(c.x + 5.0, c.y + 5.0);
    h.frame();
    assert_eq!((h.app().sel.cursor(), h.focus_id()), (Some(497), None));
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
    use crate::{Harness, NamedKey as K};
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

  /// Two lists of 5 rows, multi-select or not as `.0` says: the app holds the first's
  /// selection, the second keeps its own.
  #[derive(Default)]
  struct Modes(bool, Selection);

  impl crate::App for Modes {
    fn view(&mut self, ui: &mut Ui) {
      ui.row(Style::new().w(px(300.0)).h(px(100.0)), |ui| {
        for name in ["held", "own"] {
          let list = List::new(5, 20.0).style(Style::new().w(px(100.0)).h(px(100.0)));
          let list = if self.0 { list.multi() } else { list };
          let list = if name == "held" { list.selection(&mut self.1) } else { list };
          list.show(ui.id(name), |ui, _| ui.label("row"));
        }
      });
    }
  }

  /// The mode is the build's: a list built again without `.multi()` selects one row at a time.
  #[test]
  fn a_list_selects_one_row_again_without_multi() {
    use crate::{Harness, NamedKey as K};
    let mut h = Harness::new(Modes(true, Selection::default()), Size::new(300.0, 100.0), 1.0);
    h.frame();
    h.app_mut().0 = false;
    h.frame();
    for name in ["held", "own"] {
      h.click_id(Id::of(name).key(("row", 0u64)));
      h.key(K::ArrowDown, Modifiers::SHIFT);
      h.key(K::ArrowDown, Modifiers::SHIFT);
      h.frame();
    }

    assert_eq!(sorted(&h.app().1), [2]);
    let tree = h.a11y();
    assert_eq!(tree.matches("selected").count(), 2, "{tree}");
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
    use crate::{Harness, NamedKey as K};
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

  /// A press on a header keeps the selection when a wheel moved the rows after the frame it hit.
  #[test]
  fn a_header_pressed_as_the_wheel_moves_keeps_the_selection() {
    use crate::app::{Event, Phase};
    use crate::{Harness, Point};
    let mut h = Harness::new(Threes::default(), Size::new(300.0, 300.0), 1.0);
    let id = Id::of("list");
    h.frame();
    h.click_id(id.key(("row", 0u64)));
    h.frame();

    // The second header is 90 to 120 down; the wheel takes the rows 30 up before it shows.
    let list = h.rect(id).unwrap();
    let (x, y) = (list.x + 50.0, list.y + 100.0);
    let (pos, delta, mods) = (Point::new(x, y), Point::new(0.0, -30.0), Modifiers::empty());
    let (phase, momentum) = (Phase::None, Phase::None);
    h.event(Event::Scroll { pos, delta, precise: true, phase, momentum, mods });
    h.press(x, y);
    h.release(x, y);
    h.frame();
    assert_eq!(h.app().sel.cursor(), Some(0));
  }

  /// A quick click that went down on a header keeps the selection wherever it was released.
  #[test]
  fn a_quick_click_down_on_a_header_keeps_the_selection() {
    use crate::Harness;
    let mut h = Harness::new(Threes::default(), Size::new(300.0, 300.0), 1.0);
    let id = Id::of("list");
    h.frame();
    h.click_id(id.key(("row", 0u64)));
    h.frame();

    // Down at the second header's foot (90 to 120), up on the row under it.
    let list = h.rect(id).unwrap();
    let x = list.x + 50.0;
    h.press(x, list.y + 118.0);
    h.release(x, list.y + 122.0);
    h.frame();
    assert_eq!(h.app().sel.cursor(), Some(0));
  }

  #[test]
  fn delete_asks_and_escape_clears() {
    use crate::{Harness, NamedKey as K};
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
      let list = List::new(6, 20.0).sections(&[0, 3], 20.0, &head).selection(&mut self.sel);
      list.show(ui.list_style(&InsetGroupedListStyle).id("list"), |_, _| {});
    }
  }

  #[test]
  fn inset_grouped_rows_lie_on_a_plate_per_section() {
    use crate::Harness;
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
    use crate::Harness;
    let mut h = Harness::new(Counted::default(), Size::new(200.0, 200.0), 1.0);
    h.frame();
    h.rt.ui.store.raw::<ScrollState>(Id::of("list")).offset[1] = 1_999_000.0;
    h.redraw();
    h.frame();
    let row = Id::of("list").key(("row", 99_955u64 * 7));
    let r = h.rect(row).expect("row 99955 built");

    h.app().calls.set(0);
    h.click(r.x + 5.0, r.y + 5.0);
    let calls = h.app().calls.get();
    assert_eq!(h.focus_id(), Some(Id::of("list")));
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
    use crate::Harness;
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
    use crate::Harness;
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
  /// disabled one; a box after the list. Scrolls in its 100 high; `tall`, 300 high, room under
  /// the rows.
  #[derive(Default)]
  struct Ground {
    sel: Selection,
    clicks: usize,
    tall: bool,
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
      ui.col(Style::new().w(px(200.0)).h(px(if self.tall { 300.0 } else { 100.0 })), |ui| {
        let list = List::new(6, 20.0).sections(&[0, 3], 30.0, &head).multi();
        let list = list.selection(&mut self.sel).style(Style::new().grow());
        list.show(ui.id("list"), |ui, _| ui.label("row"));
      });
      ui.id("after").el(Style::new().w(px(200.0)).h(px(50.0)));
      self.clicks += clicks.get();
    }
  }

  /// As a table view on macOS: a plain press on the room under the rows clears the selection;
  /// one on a header (its text, the room beside it, its button, enabled or not), with Cmd or
  /// Shift, on the scrollbar or off the list keeps it, as a source list's group row does.
  #[test]
  fn a_press_on_the_lists_ground_clears_the_selection() {
    use crate::Harness;
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

    // The room beside the header's text keeps it; its text too.
    h.click(x, y);
    h.click_id(Id::of(("t", 0)));
    h.frame();
    assert_eq!(sorted(&h.app().sel), [1]);

    // The room under the rows clears it.
    h.app_mut().tall = true;
    h.frame();
    let v = h.rect(id).unwrap();
    h.click(v.x + 50.0, v.y + v.h - 10.0);
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
    use crate::Harness;
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
        let ui = if self.bare { ui.list_style(&PlainListStyle) } else { ui };
        let inner = Style::new().w(px(10.0)).h(px(4.0));
        list.show(ui.id("list"), |ui, row| _ = ui.id(("in", row.index)).el(inner));
      });
    }
  }

  /// A row style's padding moves the row's content in.
  #[test]
  fn a_row_styles_padding_moves_the_content_in() {
    use crate::Harness;
    let mut h = Harness::new(Bare::default(), Size::new(300.0, 300.0), 1.0);
    h.frame();
    let row = h.rect(Id::of("list").key(("row", 1u64))).unwrap();
    let inner = h.rect(Id::of(("in", 1))).unwrap();
    assert_eq!((inner.x - row.x, inner.y - row.y), (8.0, 8.0));
  }

  /// Plain, a selected row keeps the list's ground for the app to paint.
  #[test]
  fn plain_rows_leave_the_selection_to_the_app() {
    use crate::Harness;
    let mut h = Harness::new(Bare::default(), Size::new(300.0, 300.0), 1.0);
    h.frame();
    let row = h.rect(Id::of("list").key(("row", 1u64))).unwrap();
    // Inset, the selection fills the row; plain, the row shows the ground as a row below.
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
