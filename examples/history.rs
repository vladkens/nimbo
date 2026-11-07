// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A long list built in view only ([nimbo::List] in sections, grouped on a form's plates):
//! 86,000 dictations, newest first, a plate a day under its date and count, each row its text
//! and when it was said, a click selecting it and Copy at its right. Only the rows on screen, and
//! a margin, are built; the scroll area is as tall as every row.
//!
//! `cargo run --release --example history [-- --shot <file.png>]`

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use nimbo::ui::Role;
use nimbo::{App, Config, FormStyle, Icon, Id, List, ListStyle, Selection, Style, Ui, grow};

pub const ROWS: usize = 86_000;
/// Dictations a day: the oldest day has what is left.
pub const PER_DAY: usize = 140;
/// A row: the text and the line under it, padded as the plates' rows are.
pub const ROW_H: f32 = 52.0;
pub const LIST: &str = "list";

pub struct History {
  pub rows: usize,
  /// Each day's title and its dictations, and the row each starts at.
  pub days: Vec<(String, usize)>,
  pub starts: Vec<usize>,
  pub sel: Selection,
  pub copied: Option<u64>,
  /// A row to scroll into view: its day and its place in it.
  pub reveal: Option<(usize, usize)>,
}

impl Default for History {
  fn default() -> Self {
    Self::new(ROWS)
  }
}

impl History {
  /// `rows` dictations, [PER_DAY] a day.
  pub fn new(rows: usize) -> Self {
    let n = rows.div_ceil(PER_DAY);
    let day = |d: usize| match d {
      0 => "Today".to_string(),
      1 => "Yesterday".to_string(),
      d => format!("{d} days ago"),
    };
    let days = (0..n).map(|d| (day(d), PER_DAY.min(rows - d * PER_DAY))).collect();
    let starts = (0..n).map(|d| d * PER_DAY).collect();
    Self { rows, days, starts, sel: Selection::default(), copied: None, reveal: None }
  }

  /// The dictation of row `i` of day `g`: its number, newest first.
  pub fn entry(&self, g: usize, i: usize) -> u64 {
    (self.rows - g * PER_DAY - i) as u64
  }
}

impl App for History {
  fn view(&mut self, ui: &mut Ui) {
    let fs = FormStyle::plates(&ui.theme());
    if let Some((g, i)) = self.reveal.take() {
      ui.reveal_row(Id::of(LIST), self.starts[g] + i);
    }

    let (rows, days, starts) = (self.rows, &self.days, &self.starts);
    let key = |i: usize| (rows - i) as u64;
    let head = |ui: &mut Ui, g: usize| {
      ui.text(&days[g].0, fs.head);
      ui.spacer();
      ui.text(format!("{} dictations", days[g].1), fs.note);
    };
    let head_h = ui.metrics(fs.head.font).line_height() + fs.head_pad[2];
    let mut copied = None;
    let mut sel = std::mem::take(&mut self.sel);
    let list = List::new(rows, ROW_H).key(&key).selection(&mut sel);
    let list = list.list_style(ListStyle::Grouped);
    let list = list.sections(starts, head_h, &head);
    ui.col(Style::new().grow().bg(fs.bg), |ui| {
      list.show(ui.id(LIST), |ui, row| {
        let i = row.index - starts[starts.partition_point(|&s| s <= row.index) - 1];
        let e = row.key;
        let text = format!("Dictation {e}: what was said, on one line");
        let caption =
          format!("{:02}:{:02} · {}.{} s", 23 - i / 7 % 24, 59 - i % 60, 2 + e % 9, e % 10);
        ui.col(Style::new().w(grow()).gap(2.0), |ui| {
          ui.text(&text, fs.label.line_limit(1));
          ui.text(&caption, fs.caption.line_limit(1));
        });
        if ui.key("copy").icon_button(Icon::COPY).clicked {
          copied = Some(e);
        }

        // Read as its text, when it was said under it.
        ui.a11y(row.id, Role::Row).name(&text).description(&caption);
      });
    });
    self.sel = sel;
    self.copied = copied.or(self.copied);
  }
}

fn main() {
  // The shot: the page's top, the second row selected.
  nimbo::run_example(Config::new("History", 560.0, 520.0), History::default(), |h| {
    h.frame();
    let e = h.app().entry(0, 1);
    h.app_mut().sel.select(e, 1);
    h.frame();
  });
}
