// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The list blocks, built live: a list, its styles and one of the gallery's own, rows of the app's
//! own over the list's behaviour, and sections.

use nimbo::{
  Align, Id, InsetGroupedListStyle, List, ListConfiguration, ListRows, ListStyle, PlainListStyle,
  Row, Style, TextStyle, Ui, alpha, grow, px,
};

use super::{Gallery, Shown};

/// A list style of the gallery's own: on a rounded panel, every other row tinted, the selected one
/// a rounded tile of the accent, fainter while the list is not focused; faded when disabled.
struct Stripes;

impl ListStyle for Stripes {
  fn make_body<'a>(&self, ui: &mut Ui<'a>, mut c: ListConfiguration<'_, 'a>) {
    let t = ui.theme();
    let frame = c.frame().radius(8.0).bg(t.panel());
    let frame = if c.is_enabled() { frame } else { frame.opacity(t.disabled()) };
    ui.scroll_area(frame, |ui| {
      c.rows(ui, |ui, mut r| {
        let fill = match (r.is_selected(), r.index() % 2) {
          (true, _) => alpha(t.accent(), if r.is_focused() { 0.35 } else { 0.15 }),
          (false, 1) => alpha(t.text(), 0.04),
          _ => [0.0; 4],
        };

        ui.row(r.frame().radius(6.0).bg(fill), |ui| r.content(ui));
      })
    });
  }
}

impl Gallery {
  /// The live list block `name`.
  pub(super) fn list(&mut self, ui: &mut Ui, name: &str) {
    let t = ui.theme();
    match name {
      "List" => Self::variants(ui, Shown::Plain, |ui| {
        let list = List::new(40, 22.0).selection(&mut self.rows);
        let list = list.style(Style::new().w(px(220.0)).h(px(88.0)));
        list.show(ui, |ui, row| ui.text(format_args!("Row {}", row.index + 1), t.label()));
      }),

      // Plain, the app draws the selection: its text in the accent. The same rows in the window's
      // inset style beside it.
      "List style" => Self::variants(ui, Shown::Plain, |ui| {
        let frame = Style::new().w(px(160.0)).h(px(88.0));
        let text = |row: Row| match row.selected {
          true => TextStyle::new(t.font(), t.accent()),
          false => t.label(),
        };
        let [plain_rows, rows] = &mut self.styled_rows;
        let plain = List::new(40, 22.0).style(frame).selection(plain_rows);
        plain.show(ui.list_style(&PlainListStyle), |ui, row| {
          ui.text(format_args!("Row {}", row.index + 1), text(row));
        });

        let inset = List::new(40, 22.0).style(frame).selection(rows);
        inset.show(ui, |ui, row| ui.text(format_args!("Row {}", row.index + 1), t.label()));
      }),
      "Custom list style" => Self::variants(ui, Shown::Plain, |ui| {
        // Padded in the list's box, where its keys scroll the rows to.
        let list = List::new(40, 24.0).style(Style::new().w(px(220.0)).h(px(96.0)).pad(4.0));
        let list = list.selection(&mut self.styled_rows[1]);
        let ui = ui.list_style(&Stripes);
        list.show(ui, |ui, row| ui.text(format_args!("Row {}", row.index + 1), t.label()));
      }),

      // Rows of its own over the list's behaviour: the selected one filled with the accent.
      "List behavior" => Self::variants(ui, Shown::Alone, |ui| {
        let (id, key) = (Id::of("own list"), |i: usize| i as u64);
        let b = ui.list_behavior(id, ListRows::new(40, 22.0), &mut self.own_rows, &key);
        let sel = b.selection();

        // The box's focus and the rows built in view, their ids the list's.
        let style = Style::new().w(px(220.0)).h(px(96.0)).pad(4.0).radius(8.0).bg(t.panel());
        let style = b.style(style);
        ui.with_id(id).scroll_area(style, |ui| {
          ui.list_rows(&b, |ui, i, k| {
            let on = sel.contains(k);
            let row = Style::new().w(grow()).h(px(22.0)).pad_xy(8.0, 0.0).radius(5.0);
            let fill = if on { t.accent() } else { [0.0; 4] };
            let row = row.align(Align::Start, Align::Center).bg(fill);
            let text = TextStyle::new(t.font(), if on { t.on_accent() } else { t.text() });
            ui.with_id(id.key(("row", k)))
              .row(row, |ui| ui.text(format_args!("Row {}", i + 1), text));
          });
        });
      }),

      "List sections" => Self::variants(ui, Shown::Plain, |ui| {
        let head = |ui: &mut Ui, g: usize| ui.text(["Today", "Yesterday"][g], t.section());
        let list = List::new(6, 30.0).sections(&[0, 3], 24.0, &head).selection(&mut self.rows);
        let list = list.style(Style::new().w(px(320.0)).h(px(290.0)));
        let ui = ui.list_style(&InsetGroupedListStyle);
        list.show(ui, |ui, row| ui.text(format_args!("Dictation {}", row.index + 1), t.label()));
      }),
      _ => unreachable!("{name} is not a list block"),
    }
  }
}
