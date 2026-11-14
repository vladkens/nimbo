// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A window with a sidebar it shows and hides from its title bar, as Notes' on macOS 26: a unified
//! window ([nimbo::Config::unified]) whose columns have toolbars ([nimbo::Ui::toolbar]) in the
//! title bar, the sidebar's toggle at its trailing edge by the divider (right of the traffic
//! lights while the sidebar is hidden) and the note's new-note button at the window's end, the
//! empty part of the bar moving the window; the sidebar (the notes on the panel's ground, up to
//! the window's top) beside the note in a split ([nimbo::Split]), the note alone while the
//! sidebar is hidden; it slides in and out in 150 ms, the toggle with it. On Windows and Linux the
//! window keeps its title bar and the toolbars are the columns' top rows.
//!
//! `cargo run --release --example notes [-- --no-sidebar] [--shot <file.png>]`

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use nimbo::{
  App, Config, Icon, Id, List, Selection, Split, Style, TextStyle, ToolbarItemPlacement,
  Transition, Ui, grow, px,
};

pub const TOGGLE: &str = "toggle";
pub const NEW: &str = "new";
pub const SPLIT: &str = "split";
pub const LIST: &str = "notes";
/// The sidebar's slide, 150 ms from in to out.
pub const SLIDE: Transition = Transition::smooth(1000.0 / 150.0);

pub struct Notes {
  pub notes: Vec<(String, &'static str)>,
  pub sel: Selection,
  pub sidebar: bool,
  /// The sidebar's share of the window.
  pub side: f32,
}

impl Default for Notes {
  fn default() -> Self {
    let notes = [
      ("Groceries", "Oat milk, lemons, a loaf of rye, coffee beans."),
      ("Trip to Porto", "Trains from Lisbon at 9 and 11; the tiles museum is shut on Mondays."),
      ("Ideas", "A toolbar in the title bar, its toggle right of the traffic lights."),
    ];
    let notes = notes.map(|(t, b)| (t.to_owned(), b)).to_vec();
    let mut sel = Selection::default();
    sel.select(0, 0);
    Self { notes, sel, sidebar: true, side: 0.3 }
  }
}

impl Notes {
  /// The note selected, the first when none is.
  pub fn open(&self) -> usize {
    self.sel.cursor_index(self.notes.len(), &|i| i as u64).unwrap_or(0)
  }

  /// The sidebar column's toolbar: the toggle at its trailing edge, by the divider, as on macOS
  /// 26; in a column too narrow for it, or hidden, right of the traffic lights.
  fn toggle(&mut self, ui: &mut Ui) {
    ui.toolbar(|ui| {
      ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
        let tip = if self.sidebar { "Hide Sidebar" } else { "Show Sidebar" };
        if ui.help(tip).id(TOGGLE).icon_button(Icon::SIDEBAR).clicked {
          self.sidebar ^= true;
        }
      });
    });
  }

  /// The sidebar: the notes on the panel's ground, under its toolbar.
  fn sidebar(&mut self, ui: &mut Ui, bar: f32) {
    let t = ui.theme();
    ui.col(Style::new().grow().bg(t.panel), |ui| {
      self.toggle(ui);
      ui.el(Style::new().h(px(bar)));
      let notes = &self.notes;
      let list = List::new(notes.len(), 28.0).selection(&mut self.sel);
      let list = list.style(Style::new().grow().pad_xy(8.0, 0.0));
      list.show(ui.id(LIST), |ui, row| ui.text(&notes[row.index].0, t.label().nowrap()));
    });
  }

  /// The note open under the note column's toolbar, a new-note button at its end.
  fn note(&mut self, ui: &mut Ui, bar: f32) {
    let t = ui.theme();
    let page = Style::new().grow().pads([bar, 24.0, 24.0, 24.0]).gap(10.0);
    ui.col(page, |ui| {
      let (title, body) = &self.notes[self.open()];
      ui.text(title, TextStyle::new(t.bold, t.text));
      ui.text(body, t.label());
      ui.toolbar(|ui| {
        ui.toolbar_item(ToolbarItemPlacement::PrimaryAction, |ui| {
          if ui.help("New Note").id(NEW).icon_button(Icon::DOC).clicked {
            self.notes.push(("New Note".to_owned(), ""));
            let i = self.notes.len() - 1;
            self.sel.select(i as u64, i);
          }
        });
      });
    });
  }
}

impl App for Notes {
  fn view(&mut self, ui: &mut Ui) {
    ui.follow_system();
    let (t, bar) = (ui.theme(), ui.toolbar_height());
    // How far the sidebar is out: it slides in and out in 150 ms.
    let out = ui.animate(Id::of(SPLIT).key("out"), self.sidebar as u8 as f32, SLIDE);
    ui.col(Style::new().grow().bg(t.bg), |ui| {
      // Hidden, the sidebar's column is no wider than nothing, its toolbar there all the same.
      if out == 0.0 {
        return ui.row(Style::new().grow(), |ui| {
          ui.col(Style::new().w(px(0.0)).h(grow()), |ui| self.toggle(ui));
          self.note(ui, bar);
        });
      }

      // Sliding, the sidebar takes that part of its share, the share kept for when it is out.
      let mut side = std::mem::take(&mut self.side);
      let mut sliding = side * out;
      let ratio = if out < 1.0 { &mut sliding } else { &mut side };
      let split = Split::navigation().ratio(ratio).min(180.0 * out, 280.0);
      let r = split.show(ui.id(SPLIT), |ui, pane| match pane {
        0 => self.sidebar(ui, bar),
        _ => self.note(ui, bar),
      });
      self.side = side;
      r
    });
  }
}

fn main() {
  let mut args: Vec<String> = std::env::args().skip(1).collect();
  let hidden = args.iter().position(|a| a == "--no-sidebar").map(|i| args.remove(i));
  let app = Notes { sidebar: hidden.is_none(), ..Notes::default() };
  let cfg = Config::new("Notes", 760.0, 480.0).min_size(480.0, 300.0).unified();
  nimbo::run_args(cfg, app, |h| _ = h.frame(), &args);
}
