// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Lists and containers: a virtualized list of 100,000 rows (several selected by Cmd- and
//! Shift-click or Shift-arrows, Enter opens one) beside tabs, in a split to drag; a toolbar with
//! a menu button and a pop-up button (letters typed choose its option), and a page of notes in
//! a scroll area whose text fades under its edges.
//!
//! `cargo run --release --example lists [-- --shot <file.png>]`

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::time::Duration;

use nimbo::{
  Align, App, Config, Id, List, Mods, Selection, Split, Style, Tabs, TextStyle, Ui, grow,
};

pub const ROWS: usize = 100_000;
pub const ROW_H: f32 = 24.0;

pub const LIST: &str = "rows";
pub const SPLIT: &str = "split";
pub const TABS: &str = "tabs";
pub const MENU: &str = "actions";
pub const SORT: &str = "sort";
pub const NOTES: &str = "notes";

pub const TITLES: [&str; 2] = ["Details", "Notes"];
pub const SORTS: [&str; 4] = ["Name", "Size", "Kind", "Date Modified"];
/// The menu's items; the last one is disabled while nothing is selected.
pub const ACTIONS: [&str; 4] = ["Go to First", "Go to Last", "Select None", "Open"];

#[derive(Default)]
pub struct Lists {
  pub sel: Selection,
  /// The row opened last (Enter or a double click).
  pub opened: Option<usize>,
  pub tab: usize,
  pub sort: usize,
  /// The list's share of the split.
  pub side: f32,
  /// The menu's items run, by index.
  pub ran: Vec<usize>,
}

impl Lists {
  pub fn new() -> Self {
    Self { side: 0.45, ..Self::default() }
  }

  fn toolbar(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    let bar = Style::new().w(grow()).pad_xy(12.0, 8.0).gap(10.0).align(Align::Start, Align::Center);
    ui.row(bar.bg(t.panel).borders([0.0, 0.0, 1.0, 0.0], t.line), |ui| {
      let none = self.sel.is_empty();
      let mut run = None;
      ui.id(MENU).menu("Actions", |ui| {
        for (i, name) in ACTIONS.iter().enumerate() {
          if ui.disabled(i == 3 && none).key(i).menu_item(name).clicked {
            run = Some(i);
          }
        }
      });
      if let Some(i) = run {
        self.act(ui, i);
      }

      ui.text("Sort by", t.muted().nowrap());
      ui.id(SORT).pop_up(&mut self.sort, &SORTS);
      ui.spacer();
      let n = self.sel.len();
      ui.text(format_args!("{n} of {ROWS} selected"), t.muted().nowrap());
    });
  }

  /// Runs the menu's item `i`.
  fn act(&mut self, ui: &mut Ui, i: usize) {
    self.ran.push(i);
    let list = Id::of(LIST);
    match i {
      0 | 1 => {
        let at = if i == 0 { 0 } else { ROWS - 1 };
        self.sel.select(at as u64, at);
        ui.reveal_row(list, at);
      }
      2 => self.sel.clear(),
      _ => self.opened = self.sel.cursor().map(|k| k as usize),
    }
  }

  fn list(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    let name = t.label();
    let size = t.muted().font(t.small);
    let r = List::new(ROWS, ROW_H)
      .multi()
      .selection(&mut self.sel)
      .style(Style::new().grow().pad_xy(0.0, 4.0))
      .show(ui.id(LIST), |ui, row| {
        ui.text(format_args!("Item {:06}", row.index + 1), name);
        ui.spacer();
        ui.text(format_args!("{} KB", (row.index * 37) % 900 + 12), size.nowrap());
      });
    if r.activated.is_some() {
      self.opened = r.activated;
    }
  }

  fn details(&self, ui: &mut Ui) {
    let t = ui.theme();
    let text = TextStyle::new(t.font, t.text);
    ui.col(Style::new().grow().pad(16.0).gap(8.0), |ui| {
      ui.text("SELECTION", t.section());
      match self.sel.cursor() {
        Some(k) => ui.text(format_args!("Item {:06}, {} selected", k + 1, self.sel.len()), text),
        None => ui.text("Nothing selected", t.muted()),
      }

      ui.text("OPENED", t.section());
      match self.opened {
        Some(i) => ui.text(format_args!("Item {:06}", i + 1), text),
        None => ui.text("Press Enter or double-click a row", t.muted()),
      }

      ui.text("SORTED BY", t.section());
      ui.text(SORTS[self.sort], text);
    });
  }

  fn notes(&self, ui: &mut Ui) {
    let t = ui.theme();
    let text = TextStyle::new(t.font, t.text).line_h(19.0);
    let area = Style::new().grow().pad(16.0).gap(12.0);
    ui.id(NOTES).fade_edges(t.bg, 28.0).scroll_area(area, |ui| {
      for (i, p) in NOTE.iter().cycle().take(12).enumerate() {
        ui.text(format_args!("{}. {p}", i + 1), text);
      }
    });
  }
}

const NOTE: [&str; 3] = [
  "The list builds only the rows in view and two more past each edge, between two spacers \
   that give it the height of all its rows, so its scrollbar is right.",
  "Selection is by row key, not by element: it stays when rows scroll out of view.",
  "This page scrolls under fades at its top and bottom edges, each showing as the text moves \
   away from it.",
];

impl App for Lists {
  fn view(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    ui.col(Style::new().grow().bg(t.bg), |ui| {
      self.toolbar(ui);

      // The split and the tabs edit copies: their panes borrow the app.
      let (mut side, mut tab) = (self.side, self.tab);
      Split::row().ratio(&mut side).min(200.0, 220.0).show(ui.id(SPLIT), |ui, pane| match pane {
        0 => self.list(ui),
        _ => {
          _ = Tabs::new(&mut tab, &TITLES).show(ui.id(TABS), |ui, i| match i {
            0 => self.details(ui),
            _ => self.notes(ui),
          })
        }
      });
      (self.side, self.tab) = (side, tab);
    });
  }
}

#[allow(dead_code)]
fn main() {
  // The shot: rows 3 to 6 selected, the list scrolled a little, the pointer on the menu.
  nimbo::run(Config::new("lists", 760.0, 480.0), Lists::new(), |h| {
    h.frame();
    let row = |h: &nimbo::Harness<Lists>, k: u64| h.rect(Id::of(LIST).key(("row", k))).unwrap();
    let r = row(h, 2);
    h.click(r.x + 20.0, r.y + r.h / 2.0);
    let r = row(h, 5);
    h.press_with(r.x + 20.0, r.y + r.h / 2.0, 1, Mods::SHIFT);
    h.release(r.x + 20.0, r.y + r.h / 2.0);
    h.wait(Duration::from_millis(300));
  });
}
