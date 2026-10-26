// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A long form page built in view only ([nimbo::Ui::form_list]): 86,000 dictations, newest
//! first, a plate a day under its date and count, each row its text and when it was said, a
//! click selecting it (tinted) and Copy at its right. Only the rows on screen, and a margin,
//! are built; the scroll area is as tall as every row.
//!
//! `cargo run --release --example history [-- --shot <file.png>]`

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use nimbo::{App, Button, Config, FormRow, FormStyle, Icon, Id, Style, Ui};

pub const ROWS: usize = 86_000;
/// Dictations a day: the oldest day has what is left.
pub const PER_DAY: usize = 140;
/// A row: the text and the line under it, padded as the plates' rows are.
pub const ROW_H: f32 = 52.0;
pub const PAGE: &str = "page";
pub const LIST: &str = "list";

pub struct History {
  pub rows: usize,
  /// Each day's title and its dictations.
  pub days: Vec<(String, usize)>,
  pub selected: Option<u64>,
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
    Self { rows, days, selected: None, copied: None, reveal: None }
  }

  /// The dictation of row `i` of day `g`: its number, newest first.
  pub fn entry(&self, g: usize, i: usize) -> u64 {
    (self.rows - g * PER_DAY - i) as u64
  }
}

impl App for History {
  fn view(&mut self, ui: &mut Ui) {
    let fs = FormStyle::plates(&ui.theme());
    let reveal = self.reveal.take();
    let groups: Vec<(&str, usize)> = self.days.iter().map(|(t, n)| (t.as_str(), *n)).collect();
    let (selected, mut clicked, mut copied) = (self.selected, None, None);
    let note = |ui: &mut Ui, g: usize| ui.text(format!("{} dictations", groups[g].1), fs.note);
    let row = |ui: &mut Ui, g: usize, i: usize| {
      let e = self.entry(g, i);
      let s = FormStyle { row: fs.row.hit(), ..fs };
      let text = format!("Dictation {e}: what was said, on one line");
      let caption =
        format!("{:02}:{:02} · {}.{} s", 23 - i / 7 % 24, 59 - i % 60, 2 + e % 9, e % 10);
      let row = FormRow::new(&text).caption(&caption).one_line().selected(selected == Some(e));
      let r = ui.form_row(&s, row, |ui| {
        if Button::icon(Icon::COPY).show(ui.key("copy")).clicked {
          copied = Some(e);
        }
      });
      if r.clicked {
        clicked = Some(e);
      }
    };

    ui.col(Style::new().grow().bg(fs.bg), |ui| {
      ui.id(PAGE).form_scroll(&fs, Style::new().grow(), |ui| {
        ui.col(fs.page, |ui| {
          if let Some((g, i)) = reveal {
            ui.reveal_form_row(Id::of(LIST), &groups, g, i);
          }

          let key = |g, i| self.entry(g, i);
          ui.id(LIST).form_list(&fs, &groups, ROW_H, &key, note, row);
        });
      });
    });
    self.selected = clicked.or(self.selected);
    self.copied = copied.or(self.copied);
  }
}

fn main() {
  // The shot: the page's top, the second row selected.
  nimbo::run(Config::new("History", 560.0, 520.0), History::default(), |h| {
    h.frame();
    h.app_mut().selected = Some(h.app().entry(0, 1));
    h.frame();
  });
}
