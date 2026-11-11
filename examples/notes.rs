// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A window with a sidebar it shows and hides from its title bar, as Notes': a unified window
//! ([nimbo::Config::unified]) whose toolbar ([nimbo::Ui::toolbar]) has the sidebar toggle right of
//! the traffic lights and a new-note button at the trailing end, the empty part of the bar moving
//! the window; the sidebar (the notes on the panel's ground, up to the window's top) beside the
//! note in a split ([nimbo::Split]), the note alone while the sidebar is hidden; it slides in and
//! out in 150 ms. On Windows and
//! Linux the window keeps its title bar and the toolbar is the content's top row.
//!
//! `cargo run --release --example notes [-- --no-sidebar] [--shot <file.png>]`

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use nimbo::{
  App, Axis, Config, Icon, Id, List, Selection, Split, Style, TextStyle, ToolbarItemPlacement,
  Transition, Ui, px,
};

pub const TOGGLE: &str = "toggle";
pub const NEW: &str = "new";
pub const SPLIT: &str = "split";
pub const LIST: &str = "notes";
pub const NOTE: &str = "note";
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

  /// The sidebar: the notes on the panel's ground, under the toolbar's band.
  fn sidebar(&mut self, ui: &mut Ui, bar: f32) {
    let t = ui.theme();
    ui.col(Style::new().grow().bg(t.panel), |ui| {
      ui.el(Style::new().h(px(bar)));
      let notes = &self.notes;
      let list = List::new(notes.len(), 28.0).selection(&mut self.sel);
      let list = list.style(Style::new().grow().pad_xy(8.0, 0.0));
      list.show(ui.id(LIST), |ui, row| ui.text(&notes[row.index].0, t.label().nowrap()));
    });
  }

  /// The note open, under the toolbar's band.
  fn note(&self, ui: &mut Ui, bar: f32) {
    let t = ui.theme();
    let (title, body) = &self.notes[self.open()];
    let page = Style::new().grow().pads([bar, 24.0, 24.0, 24.0]).gap(10.0);
    ui.id(NOTE).col(page, |ui| {
      ui.text(title, TextStyle::new(t.bold, t.text));
      ui.text(body, t.label());
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
      if out == 0.0 {
        return self.note(ui, bar);
      }

      // Sliding, the sidebar takes that part of its share, the share kept for when it is out.
      let mut side = std::mem::take(&mut self.side);
      let mut sliding = side * out;
      let ratio = if out < 1.0 { &mut sliding } else { &mut side };
      let split = Split::new(Axis::Horizontal).ratio(ratio).min(180.0 * out, 280.0);
      split.show(ui.id(SPLIT), |ui, pane| match pane {
        0 => self.sidebar(ui, bar),
        _ => self.note(ui, bar),
      });
      self.side = side;
    });
    ui.toolbar(|ui| {
      ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
        let tip = if self.sidebar { "Hide Sidebar" } else { "Show Sidebar" };
        if ui.help(tip).id(TOGGLE).icon_button(Icon::SIDEBAR).clicked {
          self.sidebar ^= true;
        }
      });
      ui.toolbar_item(ToolbarItemPlacement::PrimaryAction, |ui| {
        if ui.help("New Note").id(NEW).icon_button(Icon::DOC).clicked {
          self.notes.push(("New Note".to_owned(), ""));
          let i = self.notes.len() - 1;
          self.sel.select(i as u64, i);
        }
      });
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
