// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The list over its behaviour, drawn by the environment's [ListStyle], as SwiftUI's `List` and
//! `.listStyle`: virtualized, only the rows in view, and a few around them, built each frame
//! between two spacers that give the scroll area the whole content height, so a list of 100k
//! rows costs what the forty on screen do and its scrollbar is right. The style builds the list's
//! box and each row's from a [ListConfiguration], the app what a row and a section header show;
//! [PlainListStyle], the rows with no fill, unless the shell put its own.
//!
//! [List] is nimbo's take on SwiftUI's `List` (with its `Section`s, selection, context menus and
//! keyboard): its behaviour is the target to move towards, not a 1:1 copy. The list does the
//! selection, the clicks, the keys and the virtualization; the style the highlight.
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
//! let ui = ui.list_style(&InsetGroupedListStyle);
//! list.show(ui.id("days"), |ui, row| ui.label(&self.entries[row.index]));
//! ```
//!
//! Sections, as SwiftUI's `Section(header:)`: the rows stay one run of indices, `starts` says
//! where each section begins and a header is built with `head(ui, section)` above its first row.
//! Headers are not rows: they take no selection, focus or keys, Shift and Cmd-A cover the rows
//! only, and all are one height, so where a row lies is still arithmetic (a binary search of
//! `starts`) and a list of 100k rows in a thousand sections costs what one without them does. A
//! header's box has the rows' side padding, its content at the bottom.
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
//! so does [Ui::reveal_row]. Disabled ([Ui::disabled]), it takes no input or focus, and the style
//! dims it.

use std::ops::Range;

use super::{ListBehavior, ListRows, Selection};
use crate::ui::{Align, Cursor, EnvironmentValue, Id, Style, Ui, grow, px};

/// What builds a section's header: `head(ui, section)`.
type Head<'s> = &'s dyn Fn(&mut Ui<'_>, usize);

/// What builds a row's content: the app's `row(ui, row)`.
type RowContent<'c, 'a> = &'c mut dyn FnMut(&mut Ui<'a>, Row);

/// How a list looks, as SwiftUI's `ListStyle`: [ListStyle::make_body] builds the list's root, a
/// scroll area under its id in [ListConfiguration::frame], and each row's box in it
/// ([ListConfiguration::rows]), the app's content in the row. Chosen with [Ui::list_style] as
/// `&'static`, a unit struct's `&MyStyle`.
pub trait ListStyle: 'static {
  /// Where the rows lie, which the behaviour reads before the list is built (the row a page down
  /// goes to, one scrolled into view), and the boxes the style builds them in: the default's
  /// ([ListLayout::default]) unless the style says otherwise.
  fn layout(&self, ui: &Ui<'_>) -> ListLayout {
    _ = ui;
    ListLayout::default()
  }

  /// Builds the list `config` describes.
  fn make_body<'a>(&self, ui: &mut Ui<'a>, config: ListConfiguration<'_, 'a>);
}

/// Where a list's rows lie and the boxes a [ListStyle] builds them in ([ListStyle::layout]); the
/// app's [List::style] and [List::row_style] take the place of `frame` and `row`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct ListLayout {
  /// The list's box, a scroll area: its size and padding, the rows under the top's.
  pub frame: Style,
  /// Each row's box: width, padding, gap and alignment; the list sets its height. A header's box
  /// has its side padding.
  pub row: Style,
  /// The space above each section's header but one at the top: between grouped plates.
  pub section_spacing: f32,
  /// The space under a header's content, at its bottom.
  pub header_padding: f32,
}

/// Growing to fill its parent, rows edge to edge padded 10 and 14 at their sides with gaps of 8,
/// their content centred on their height; headers with 4 under their content, nothing between
/// sections.
impl Default for ListLayout {
  fn default() -> Self {
    let row = Style::new().w(grow()).align(Align::Start, Align::Center);
    let row = row.pads([0.0, 14.0, 0.0, 10.0]).gap(8.0);
    Self { frame: Style::new().grow(), row, section_spacing: 0.0, header_padding: 4.0 }
  }
}

/// What a [ListStyle] draws: the list's state this frame, its box, and its rows in view, built
/// with [ListConfiguration::rows].
#[non_exhaustive]
pub struct ListConfiguration<'c, 'a> {
  b: ListBehavior<'c>,
  rows: ListRows<'c>,
  n: usize,
  height: f32,
  frame: Style,
  row: Style,
  header_padding: f32,
  head: Head<'c>,
  content: RowContent<'c, 'a>,
  shown: &'c mut Range<usize>,
}

impl<'a> ListConfiguration<'_, 'a> {
  /// The list's id, its root's.
  pub fn id(&self) -> Id {
    self.b.id
  }

  /// Not disabled ([Ui::disabled]): it reads input, and is drawn so.
  pub fn is_enabled(&self) -> bool {
    !self.b.disabled
  }

  /// Focused in the key window: a style fills the selected rows in the accent then.
  pub fn is_focused(&self) -> bool {
    self.b.focused
  }

  /// The list's box, a scroll area to build first, as its root: the app's ([List::style]) or the
  /// layout's, with no gap, taking focus by a press while enabled, with no ring (its selection
  /// shows it).
  pub fn frame(&self) -> Style {
    self.frame
  }

  /// Builds the rows in view in the list's box, each with `row(ui, config)`, which builds the
  /// row's element first ([ListRowConfiguration::frame] or one of its own) and the app's content
  /// in it; a section's header above its first row, in the layout's box.
  pub fn rows(
    &mut self,
    ui: &mut Ui<'a>,
    mut row: impl FnMut(&mut Ui<'a>, ListRowConfiguration<'_, 'a>),
  ) {
    let Self { b, rows, n, height, row: rs, header_padding, head, content, shown, .. } = self;
    let (id, sel, disabled, focused) = (b.id, b.selection(), b.disabled, b.focused);
    let [_, pad_r, _, pad_l] = rs.pad;
    let hs = Style { pad: [0.0, pad_r, *header_padding, pad_l], ..*rs }.bg([0.0; 4]);
    let hs = hs.align(Align::Start, Align::End);
    let mut above = false;
    **shown = ui.list_rows(b, |ui, i, k| {
      let header = rows.header(i);
      if let Some(h) = header {
        ui.key(("head", k)).row(hs.h(px(h.height)), |ui| head(ui, h.section));
      }

      let rid = id.key(("row", k));
      let selected = sel.contains(k);
      let hovered = !disabled && ui.hovered(rid);
      let r = Row { index: i, key: k, id: rid, selected, hovered, focused };
      let (first, last) = (i == 0 || header.is_some(), i + 1 == *n || rows.header(i + 1).is_some());
      let frame = rs.h(px(*height));
      let content = &mut **content;
      let config =
        ListRowConfiguration { r, enabled: !disabled, first, last, after: above, frame, content };
      above = selected;

      // Disabled, the same id (keyed in the list) taking no input.
      match disabled {
        true => row(ui.key(("row", k)), config),
        false => row(ui.with_id(rid).pointer_style(Cursor::Pointer), config),
      }
    });
  }
}

/// What a [ListStyle] draws for a row ([ListConfiguration::rows]): its state this frame, its box
/// and its content, built once with [ListRowConfiguration::content].
#[non_exhaustive]
pub struct ListRowConfiguration<'r, 'a> {
  r: Row,
  enabled: bool,
  first: bool,
  last: bool,
  after: bool,
  frame: Style,
  content: RowContent<'r, 'a>,
}

impl<'a> ListRowConfiguration<'_, 'a> {
  /// The row's id, its element's: `list.key(("row", key))`.
  pub fn id(&self) -> Id {
    self.r.id
  }

  /// Its place among the list's rows.
  pub fn index(&self) -> usize {
    self.r.index
  }

  pub fn is_selected(&self) -> bool {
    self.r.selected
  }

  pub fn is_hovered(&self) -> bool {
    self.r.hovered
  }

  /// The list is focused in the key window: a selected row is filled in the accent then.
  pub fn is_focused(&self) -> bool {
    self.r.focused
  }

  /// The list is not disabled.
  pub fn is_enabled(&self) -> bool {
    self.enabled
  }

  /// The first row of its section, or of the list.
  pub fn is_first(&self) -> bool {
    self.first
  }

  /// The last row of its section, or of the list.
  pub fn is_last(&self) -> bool {
    self.last
  }

  /// The row above it, built this frame, is selected: no hairline between them.
  pub fn is_after_selected(&self) -> bool {
    self.after
  }

  /// The row's box: the app's ([List::row_style]) or the layout's, the list's rows' height.
  pub fn frame(&self) -> Style {
    self.frame
  }

  /// Builds the app's content of the row, once.
  pub fn content(&mut self, ui: &mut Ui<'a>) {
    (self.content)(ui, self.r)
  }
}

/// A row being built, as its content sees it: where it is and how it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Row {
  pub index: usize,
  pub key: u64,
  /// The row's element: `list.key(("row", key))`.
  pub id: Id,
  pub selected: bool,
  /// Under the pointer, the list enabled.
  pub hovered: bool,
  /// The list has focus (selected rows are drawn in the accent then).
  pub focused: bool,
}

/// What a list got this frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
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

/// A virtualized list ([Ui::list] for the simple case), drawn by the environment's [ListStyle];
/// see the module's docs.
pub struct List<'s> {
  n: usize,
  h: f32,
  key: Option<&'s dyn Fn(usize) -> u64>,
  sel: Option<&'s mut Selection>,
  multi: bool,
  /// Where each section starts, its header's height and what builds it.
  sections: Option<(&'s [usize], f32, Head<'s>)>,
  /// The box and the rows' as the app set them, else the style's.
  style: Option<Style>,
  row: Option<Style>,
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
  /// selects all. The list sets its selection's mode ([Selection::multiple]) as each build says:
  /// one built without it selects one row at a time.
  pub fn multi(self) -> Self {
    Self { multi: true, ..self }
  }

  /// Rows in sections, as SwiftUI's `Section(header:)`: section `g` starts at row `starts[g]`
  /// (ascending, none empty), under a header `head_h` high built with `head(ui, g)`, which
  /// takes no selection, focus or keys (see the module's docs). Rows before the first start have
  /// none; a header but one at the top has the style's space between sections above it
  /// ([ListLayout::section_spacing]), on top of `head_h`. One slice and one callback rather than a
  /// section per value: the app keeps the starts it has, the list finds a row's place by a binary
  /// search of them, and it builds only the headers in view, as it does the rows.
  pub fn sections(self, starts: &'s [usize], head_h: f32, head: Head<'s>) -> Self {
    Self { sections: Some((starts, head_h, head)), ..self }
  }

  /// The list's box (a scroll area): its size and padding; its gap is 0.
  pub fn style(self, style: Style) -> Self {
    Self { style: Some(style), ..self }
  }

  /// Each row's box: width, padding, gap and alignment; the list sets its height, the style its
  /// fill.
  pub fn row_style(self, row: Style) -> Self {
    Self { row: Some(row), ..self }
  }

  /// Builds the rows in view with `row(ui, row)` inside the list's scroll area, after applying
  /// the input since the last frame; the style builds the boxes ([ListStyle::make_body]).
  pub fn show<'a>(self, ui: &mut Ui<'a>, mut f: impl FnMut(&mut Ui<'a>, Row)) -> ListResponse {
    /// The selection, when the app doesn't hold it.
    #[derive(Default)]
    struct Own(Selection);

    let style = ui.environment_value::<&dyn ListStyle>();
    let id = ui.take_id();
    let layout = style.layout(ui);
    let List { n, h, multi, .. } = self;
    let (frame, row) = (self.style.unwrap_or(layout.frame), self.row.unwrap_or(layout.row));
    let (starts, head_h, head) = self.sections.unwrap_or((&[], 0.0, &|_, _| {}));
    let rows = ListRows::new(n, h).sections(starts, head_h);
    let rows = rows.section_spacing(layout.section_spacing).padding(frame.pad[0], frame.pad[2]);
    let keyf = self.key;
    let key = move |i: usize| keyf.map_or(i as u64, |k| k(i));
    let mut own = None;
    let sel = match self.sel {
      Some(s) => s,
      None => own.insert(std::mem::take(&mut ui.state::<Own>(id).0)),
    };

    // The mode is this build's, whatever the selection had.
    if sel.is_multiple() != multi {
      *sel = std::mem::take(sel).multiple(multi);
    }

    let b = ui.list_behavior(id, rows, sel, &key);
    let mut shown = 0..0;
    let (frame, header_padding) = (b.style(frame.gap(0.0)), layout.header_padding);
    let content = &mut f;
    let config = ListConfiguration {
      b,
      rows,
      n,
      height: h,
      frame,
      row,
      header_padding,
      head,
      content,
      shown: &mut shown,
    };
    let root = ui.st.tree.nodes.len();
    style.make_body(ui.with_id(id), config);
    debug_assert!(
      ui.st.tree.nodes.get(root).is_some_and(|n| n.id == id),
      "a list style builds the list's root first"
    );

    let (changed, pressed, activated, delete) = (b.changed, b.pressed, b.activated, b.delete);
    let focused = b.focused;
    if let Some(own) = own {
      ui.state::<Own>(id).0 = own;
    }

    ListResponse { id, changed, pressed, activated, delete, shown, focused }
  }
}

/// The rows edge to edge with no fill, dimmed when disabled: the app draws a selection of its own
/// from [Row]. The default under the shell's.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlainListStyle;

impl ListStyle for PlainListStyle {
  fn make_body<'a>(&self, ui: &mut Ui<'a>, mut c: ListConfiguration<'_, 'a>) {
    // As the theme's disabled controls.
    let frame = if c.is_enabled() { c.frame() } else { c.frame().opacity(0.4) };
    ui.scroll_area(frame, |ui| c.rows(ui, |ui, mut r| _ = ui.row(r.frame(), |ui| r.content(ui))));
  }
}

/// [PlainListStyle] where neither a container nor the window nor the shell put one.
impl EnvironmentValue for &'static dyn ListStyle {
  fn default_value(_: &Ui) -> Self {
    &PlainListStyle
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

  /// Draws the next list, or every list built in the next container, in `style`, as SwiftUI's
  /// `.listStyle`: `list.show(ui.list_style(&InsetGroupedListStyle).id("days"), …)`.
  pub fn list_style(&mut self, style: &'static dyn ListStyle) -> &mut Self {
    self.environment(style)
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::app::{App, Event};
  use crate::harness::Harness;
  use crate::scene::{Frame, Quad, Rect, Size};
  use crate::ui::UiState;

  const UP: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
  const OVER: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
  const ON: [f32; 4] = [1.0, 1.0, 0.0, 1.0];
  const AWAY: [f32; 4] = [0.0, 1.0, 1.0, 1.0];
  const OFF: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

  /// Rows of the app's: blue, green hovered, yellow selected (cyan while the list is not
  /// focused), red when disabled; it never sets focus or a pointer.
  struct Tiles;

  impl ListStyle for Tiles {
    fn make_body<'a>(&self, ui: &mut Ui<'a>, mut c: ListConfiguration<'_, 'a>) {
      let on = c.is_focused();
      ui.scroll_area(c.frame(), |ui| {
        c.rows(ui, |ui, mut r| {
          let fill = match (r.is_enabled(), r.is_selected(), r.is_hovered()) {
            (false, ..) => OFF,
            (_, true, _) => {
              if on {
                ON
              } else {
                AWAY
              }
            }
            (_, _, true) => OVER,
            _ => UP,
          };

          let paint = move |f: &mut Frame, at: Rect| f.quad(Quad::new(at, fill));
          ui.canvas_row(r.frame(), paint, |ui| r.content(ui));
        })
      });
    }
  }

  #[derive(Default)]
  struct Tiled {
    sel: Selection,
    off: bool,
  }

  impl App for Tiled {
    fn view(&mut self, ui: &mut Ui) {
      ui.col(Style::new().w(px(100.0)).h(px(100.0)), |ui| {
        let list = List::new(5, 20.0).selection(&mut self.sel);
        list.show(ui.list_style(&Tiles).disabled(self.off).id("list"), |_, _| {});
      });
    }
  }

  #[test]
  fn a_style_draws_over_the_behaviour() {
    let mut h = Harness::new(Tiled::default(), Size::new(100.0, 100.0), 1.0);
    let px = |h: &mut Harness<Tiled>, y: f32| h.render().pixel(50.0, y);
    let rgb = |c: [f32; 4]| c.map(|v| (v * 255.0) as u8);
    assert_eq!(px(&mut h, 30.0), rgb(UP));

    // Hovered under the hand the list gave it, selected by a click, which focuses the list.
    h.move_to(50.0, 30.0);
    assert_eq!((px(&mut h, 30.0), h.cursor()), (rgb(OVER), Cursor::Pointer));
    h.click(50.0, 30.0);
    assert_eq!((px(&mut h, 30.0), h.focus_id()), (rgb(ON), Some(Id::of("list"))));
    h.event(Event::Focus(false));
    assert_eq!(px(&mut h, 30.0), rgb(AWAY));
    h.event(Event::Focus(true));
    let a = h.a11y();
    assert!(a.contains("list") && a.contains("selected"), "{a}");

    // Disabled: its own red, no hand, no focus, no selection by a click.
    h.app_mut().off = true;
    h.redraw();
    assert_eq!(px(&mut h, 50.0), rgb(OFF));
    assert_eq!(h.focus_id(), None);
    h.move_to(50.0, 51.0);
    assert_eq!(h.cursor(), Cursor::Arrow);
    h.click(50.0, 50.0);
    assert_eq!(h.app().sel.cursor(), Some(1));
  }

  /// Lists in a container given the tiles, and one after it.
  struct Cascade;

  impl App for Cascade {
    fn view(&mut self, ui: &mut Ui) {
      let list = |ui: &mut Ui, name: &str| {
        List::new(2, 20.0).style(Style::new().w(px(60.0)).h(px(40.0))).show(ui.id(name), |_, _| {})
      };
      ui.list_style(&Tiles).row(Style::new(), |ui| {
        list(ui, "a");
        ui.col(Style::new(), |ui| _ = list(ui, "b"));
      });
      list(ui, "c");
    }
  }

  #[test]
  fn a_containers_style_reaches_its_lists_only() {
    let mut h = Harness::new(Cascade, Size::new(200.0, 100.0), 1.0);
    h.frame();
    let img = h.render();
    let at = |n: &str| h.rect(Id::of(n)).map(|r| img.pixel(r.x + 30.0, r.y + 10.0)).unwrap();
    let blue = UP.map(|v| (v * 255.0) as u8);
    assert_eq!((at("a"), at("b")), (blue, blue));
    assert_ne!(at("c"), blue);
  }

  /// With no shell, plain: a selected row unfilled.
  #[test]
  fn plain_is_the_default_without_the_shell() {
    let mut ui = Ui::new(UiState::default(), Size::new(100.0, 100.0), None, Duration::ZERO);
    let mut sel = Selection::default();
    sel.select(1, 1);
    List::new(3, 20.0).selection(&mut sel).show(ui.id("list"), |_, _| {});
    let row = Id::of("list").key(("row", 1u64));
    let n = ui.st.tree.nodes.iter().find(|n| n.id == row).unwrap();
    assert_eq!(n.style.bg, [0.0; 4]);
  }
}
