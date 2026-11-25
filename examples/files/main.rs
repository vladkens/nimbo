// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A file browser as Finder on macOS 26: a unified window ([nimbo::Config::unified]) whose columns
//! have toolbars ([nimbo::Ui::toolbar]) in the title bar, the empty part of the bar moving the
//! window. The sidebar (places and tags on the panel's ground, up to the window's top) beside the
//! [browser] in a split ([nimbo::Split::navigation]); the sidebar's toggle at its trailing edge by
//! the divider (right of the traffic lights while it is hidden); it slides in and out in 150 ms,
//! the toggle with it. The browser's toolbar has Open… and Save…, which ask for the system's
//! panels (a text file to open, notes to save under a suggested name; the answer shows in the
//! status bar), before its own items, New Folder at the window's end. Files dragged over the list
//! light it; dropped there, they join it. On Windows and Linux the window keeps its title bar and
//! the toolbars are the columns' top rows.
//!
//! `cargo run --release --example files [-- --no-sidebar] [--shot <file.png>]` opens the window;
//! `-- browser [--shot <file.png>]` the browser alone.

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

pub mod browser;

use std::path::PathBuf;

use nimbo::{
  App, Color, Config, Cx, Event, FileDialog, Icon, Id, List, Selection, Size, Split, Style,
  ToolbarItemPlacement, Transition, Ui, px, rgb,
};

use self::browser::Browser;

pub const TOGGLE: &str = "toggle";
pub const SPLIT: &str = "sidebar";
pub const LIST: &str = "places";
pub const OPEN: &str = "open";
pub const SAVE: &str = "save";
pub use self::browser::NEW;
/// The sidebar's slide, 150 ms from in to out.
pub const SLIDE: Transition = Transition::smooth(1000.0 / 150.0);
pub const SIZE: Size = Size::new(960.0, 560.0);

/// The sidebar's places, each with its icon.
pub const PLACES: [(Icon, &str); 5] = [
  (Icon::HISTORY, "Recents"),
  (Icon::FOLDER, "Applications"),
  (Icon::FOLDER, "Desktop"),
  (Icon::DOC, "Documents"),
  (Icon::DOWNLOAD, "Downloads"),
];

/// The sidebar's tags, each with its colour, Finder's.
pub const TAGS: [(Color, &str); 6] = [
  (rgb(0xff3b30), "Red"),
  (rgb(0xff9500), "Orange"),
  (rgb(0xffcc00), "Yellow"),
  (rgb(0x34c759), "Green"),
  (rgb(0x007aff), "Blue"),
  (rgb(0x8e8e93), "Gray"),
];

/// Where the sidebar's sections start: places, then tags.
const SECTIONS: [usize; 2] = [0, PLACES.len()];

pub struct Files {
  pub browser: Browser,
  /// The place or tag chosen in the sidebar.
  pub sel: Selection,
  pub sidebar: bool,
  /// The sidebar's share of the window.
  pub side: f32,
  /// The last panel's answer: a path, or `None` when cancelled.
  pub picked: Option<Option<PathBuf>>,
}

impl Default for Files {
  fn default() -> Self {
    let mut sel = Selection::default();
    sel.select(3, 3);
    Self { browser: Browser::new(), sel, sidebar: true, side: 0.22, picked: None }
  }
}

impl Files {
  /// The name of the place or tag chosen.
  pub fn place(&self) -> &'static str {
    let n = PLACES.len() + TAGS.len();
    match self.sel.cursor_index(n, &|i| i as u64).unwrap_or(0) {
      i if i < PLACES.len() => PLACES[i].1,
      i => TAGS[i - PLACES.len()].1,
    }
  }

  /// The sidebar column's toolbar: the toggle at its trailing edge, by the divider, as on macOS
  /// 26; in a column too narrow for it, right of the traffic lights.
  fn toggle(&mut self, ui: &mut Ui) {
    ui.toolbar(|ui| toggle(ui, &mut self.sidebar));
  }

  /// The sidebar: places and tags on the panel's ground, under its toolbar.
  fn sidebar(&mut self, ui: &mut Ui, bar: f32) {
    let t = ui.theme();
    ui.col(Style::new().grow().bg(t.panel), |ui| {
      self.toggle(ui);
      ui.el(Style::new().h(px(bar)));

      // Each row an icon or a tag's dot, 16 wide, before its name.
      let head = |ui: &mut Ui, s: usize| ui.text(["Favorites", "Tags"][s], t.section());
      let list = List::new(PLACES.len() + TAGS.len(), 28.0).sections(&SECTIONS, 28.0, &head);
      let list = list.selection(&mut self.sel).style(Style::new().grow().pad_xy(8.0, 0.0));

      list.show(ui.id(LIST), |ui, r| match r.index.checked_sub(PLACES.len()) {
        None => {
          let (icon, name) = PLACES[r.index];
          ui.icon(icon, 16.0, t.accent);
          ui.text(name, t.label().nowrap());
        }
        Some(i) => {
          let (color, name) = TAGS[i];
          let dot = Style::new().w(px(10.0)).h(px(10.0)).radius(5.0).bg(color);
          ui.row(Style::new().w(px(16.0)).center(), |ui| _ = ui.el(dot));
          ui.text(name, t.label().nowrap());
        }
      });
    });
  }

  /// The browser column: its toolbar, Open… and Save… first, over the browser. With the sidebar
  /// hidden, the toggle leads it, right of the traffic lights, before the folder's name.
  fn browser(&mut self, ui: &mut Ui, hidden: bool) {
    self.browser.folder = self.place();
    let t = ui.theme();
    ui.col(Style::new().grow().bg(t.bg), |ui| {
      let (browser, sidebar) = (&mut self.browser, &mut self.sidebar);
      ui.toolbar(|ui| {
        if hidden {
          toggle(ui, sidebar);
        }

        ui.toolbar_item(ToolbarItemPlacement::PrimaryAction, |ui| {
          if ui.id(OPEN).button("Open…").clicked {
            ui.file_dialog(FileDialog::open("Pick a text file").types(&["txt", "md"]));
          }

          if ui.id(SAVE).button("Save…").clicked {
            ui.file_dialog(FileDialog::save("Save the notes").name("notes.txt"));
          }
        });
        browser.toolbar_items(ui);
      });
      browser.content(ui);
    });
  }
}

impl App for Files {
  fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
    if self.browser.drags(cx, ev) {
      return true;
    }

    let Event::FileDialog(path) = ev else { return false };
    self.browser.note = Some(match path {
      Some(p) => format!("Picked {}", p.display()),
      None => "Cancelled".into(),
    });
    self.picked = Some(path.clone());
    true
  }

  fn view(&mut self, ui: &mut Ui) {
    ui.follow_system();
    let (t, bar) = (ui.theme(), ui.toolbar_height());
    // How far the sidebar is out: it slides in and out in 150 ms.
    let out = ui.animate(Id::of(SPLIT).key("out"), self.sidebar as u8 as f32, SLIDE);
    ui.col(Style::new().grow().bg(t.bg), |ui| {
      // Hidden, the browser alone, the toggle in its toolbar.
      if out == 0.0 {
        return ui.row(Style::new().grow(), |ui| self.browser(ui, true));
      }

      // Sliding, the sidebar takes that part of its share, the share kept for when it is out.
      let mut side = std::mem::take(&mut self.side);
      let mut sliding = side * out;
      let ratio = if out < 1.0 { &mut sliding } else { &mut side };
      let split = Split::navigation().ratio(ratio).min(180.0 * out, 460.0);
      let r = split.show(ui.id(SPLIT), |ui, pane| match pane {
        0 => self.sidebar(ui, bar),
        _ => self.browser(ui, false),
      });
      self.side = side;
      r
    });
  }
}

/// The sidebar's toggle, a toolbar item at the leading end.
fn toggle(ui: &mut Ui, sidebar: &mut bool) {
  ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
    let tip = if *sidebar { "Hide Sidebar" } else { "Show Sidebar" };
    if ui.help(tip).id(TOGGLE).icon_button(Icon::SIDEBAR).clicked {
      *sidebar ^= true;
    }
  });
}

const USAGE: &str = "usage: files [browser] [--no-sidebar] [--shot <file.png>] [options]";

fn main() {
  let mut args: Vec<String> = std::env::args().skip(1).collect();
  match args.first().map(String::as_str) {
    Some("browser") => return browser::run(&args[1..]),
    Some(a) if !a.starts_with('-') => {
      eprintln!("unknown part {a}\n{USAGE}");
      std::process::exit(2);
    }
    _ => {}
  }

  let hidden = args.iter().position(|a| a == "--no-sidebar").map(|i| args.remove(i));
  let app = Files { sidebar: hidden.is_none(), ..Files::default() };
  let cfg = Config::new("Files", SIZE.w, SIZE.h).min_size(640.0, 360.0).unified();
  nimbo::run_args(cfg, app, browser::shot, &args);
}
