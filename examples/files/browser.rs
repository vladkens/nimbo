// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The browser, the files window's content column: a virtualized list of 100,000 rows (several
//! selected by Cmd- and Shift-click or Shift-arrows, Enter opens one, a right-click shows their
//! menu) beside an inspector of tabs, Info and Notes, in a split to drag; over them a toolbar with
//! the folder's name, a menu button of actions, a pop-up button to sort by (letters typed choose
//! its option) and New Folder; under them a status bar. The Notes page scrolls under fades at its
//! edges. Files dropped on the window join the list at its top ([Browser::add]), and it lights
//! while they are dragged over it ([Browser::over]).
//!
//! `cargo run --release --example files -- browser [--shot <file.png>]` opens it alone.

use std::time::Duration;

use nimbo::{
  Align, App, Axis, ButtonRole, Config, Cx, Event, Float, Harness, Icon, Id, Item, List, Menu,
  Modifiers, Selection, Shortcut, Size, Split, Style, Tabs, Tag, TextStyle, ToolbarItemPlacement,
  Ui, alpha, grow, px,
};

/// The rows of the folder, besides those added ([Browser::added]).
pub const ROWS: usize = 100_000;
pub const ROW_H: f32 = 24.0;
/// The browser alone.
pub const SIZE: Size = Size::new(760.0, 480.0);

pub const LIST: &str = "rows";
pub const SPLIT: &str = "split";
pub const TABS: &str = "tabs";
pub const MENU: &str = "actions";
pub const SORT: &str = "sort";
pub const NOTES: &str = "notes";
pub const NEW: &str = "new";

pub const TITLES: [&str; 2] = ["Info", "Notes"];
pub const SORTS: [&str; 4] = ["Name", "Size", "Kind", "Date Modified"];
/// The menu's items; the last one is disabled while nothing is selected.
pub const ACTIONS: [&str; 4] = ["Go to First", "Go to Last", "Select None", "Open"];

/// A row added to the folder: a file dropped on it or a new folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
  pub name: String,
  /// In bytes; none for a folder.
  pub size: Option<u64>,
}

#[derive(Default)]
pub struct Browser {
  /// The folder's name, in the toolbar.
  pub folder: &'static str,
  /// The rows added, in order: shown above the folder's own, the last added first. Each one's key
  /// is [ROWS] and up, by its place here.
  pub added: Vec<Entry>,
  /// Files are dragged over the list.
  pub over: bool,
  /// The app's note after the count in the status bar.
  pub note: Option<String>,
  pub sel: Selection,
  /// The key of the row opened last (Enter or a double click).
  pub opened: Option<usize>,
  pub tab: usize,
  pub sort: usize,
  /// The list's share of the split.
  pub side: f32,
  /// The menu's items run, by index.
  pub ran: Vec<usize>,
  /// The rows show their sizes (the row menu's check).
  pub sizes: bool,
  /// The row menu's items chosen, each with the rows selected then.
  pub chosen: Vec<(RowAction, Vec<u64>)>,
  /// The row menu was open as the view last ran: closed with nothing chosen, it says so.
  menu_open: bool,
}

/// What the row menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowAction {
  Open,
  Sizes,
  Rename,
  Sort(usize),
  Delete,
}

impl Browser {
  pub fn new() -> Self {
    Self { folder: "Documents", side: 0.45, sizes: true, ..Self::default() }
  }

  /// The rows, the added ones and the folder's.
  pub fn count(&self) -> usize {
    self.added.len() + ROWS
  }

  /// The key of row `i`: an item's index, or [ROWS] and up for the added rows, which come first.
  pub fn key(&self, i: usize) -> u64 {
    let n = self.added.len();
    if i < n { (ROWS + n - 1 - i) as u64 } else { (i - n) as u64 }
  }

  /// The name of the row with `key`.
  pub fn name(&self, key: u64) -> String {
    match (key as usize).checked_sub(ROWS) {
      Some(j) => self.added[j].name.clone(),
      None => format!("Item {:06}", key + 1),
    }
  }

  /// Adds `entry` at the top of the list.
  pub fn add(&mut self, entry: Entry) {
    self.added.push(entry);
  }

  /// Files dragged over the list light it; dropped there, they join it. Whether `ev` was one of
  /// those.
  pub fn drags(&mut self, cx: &mut Cx, ev: &Event) -> bool {
    let list = cx.hovered(Id::of(LIST));
    match ev {
      Event::DragOver { .. } => self.over = list,
      Event::DragExit => self.over = false,
      Event::Drop { paths, .. } => {
        if list {
          for p in paths {
            let name =
              p.file_name().map_or_else(|| p.display().to_string(), |n| n.display().to_string());
            let size = std::fs::metadata(p).ok().filter(|m| m.is_file()).map(|m| m.len());
            self.add(Entry { name, size });
          }
        }

        self.over = false;
      }
      _ => return false,
    }

    true
  }

  /// A new folder at the top, selected and shown.
  fn new_folder(&mut self, ui: &mut Ui) {
    let n = self.added.iter().filter(|e| e.name.starts_with("untitled folder")).count();
    let name = if n == 0 { "untitled folder".into() } else { format!("untitled folder {}", n + 1) };
    self.add(Entry { name, size: None });
    self.sel.select(self.key(0), 0);
    ui.reveal_row(Id::of(LIST), 0);
  }

  /// The selected rows' context menu.
  pub fn row_menu(&self) -> Menu<RowAction> {
    let n = self.sel.len();
    let title = match self.sel.cursor() {
      Some(k) if n == 1 => self.name(k),
      _ => format!("{n} Items"),
    };

    let sort = |(i, s): (usize, &&str)| Item::toggle(RowAction::Sort(i), s, i == self.sort);
    Menu::new(vec![
      Item::section(&title),
      Item::button(RowAction::Open, "Open").key(Shortcut::char('o').cmd()),
      Item::toggle(RowAction::Sizes, "Show Sizes", self.sizes),
      Item::button(RowAction::Rename, "Rename…").enabled(n == 1),
      Item::submenu("Sort By", Menu::new(SORTS.iter().enumerate().map(sort).collect())),
      Item::Divider,
      Item::button(RowAction::Delete, "Delete").role(ButtonRole::Destructive),
    ])
  }

  /// Runs the row menu's item `item` on the selected rows.
  fn choose(&mut self, item: RowAction) {
    let mut rows: Vec<u64> = self.sel.keys().collect();
    rows.sort();
    match item {
      RowAction::Open => self.opened = self.sel.cursor().map(|k| k as usize),
      RowAction::Sizes => self.sizes = !self.sizes,
      RowAction::Sort(i) => self.sort = i,
      RowAction::Rename | RowAction::Delete => {}
    }

    // What a check outside the harness reads.
    eprintln!("menu: {item:?} {rows:?}");
    self.chosen.push((item, rows));
  }

  /// The browser's toolbar items: the folder's name leading; the actions menu, the sort pop-up
  /// and New Folder trailing, after the items an app built before them in the same toolbar.
  pub fn toolbar_items(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
      ui.text(self.folder, TextStyle::new(t.bold.size(15.0), t.text).nowrap());
    });

    ui.toolbar_item(ToolbarItemPlacement::PrimaryAction, |ui| {
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

      let sorts: Vec<Tag<usize>> = SORTS.iter().enumerate().map(|(i, s)| Tag::new(i, s)).collect();
      ui.help("Sort By").id(SORT).picker(&mut self.sort, &sorts);
      if ui.help("New Folder").id(NEW).icon_button(Icon::FOLDER).clicked {
        self.new_folder(ui);
      }
    });
  }

  /// Runs the menu's item `i`.
  fn act(&mut self, ui: &mut Ui, i: usize) {
    self.ran.push(i);
    let list = Id::of(LIST);
    match i {
      0 | 1 => {
        let at = if i == 0 { 0 } else { self.count() - 1 };
        self.sel.select(self.key(at), at);
        ui.reveal_row(list, at);
      }
      2 => self.sel.clear(),
      _ => self.opened = self.sel.cursor().map(|k| k as usize),
    }
  }

  /// The list's pane: the column heads over the rows, lit while files are dragged over it.
  fn list(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    let sizes = self.sizes;
    // Padded as the rows are, the heads over the names' and the sizes' edges.
    let head = Style::new().w(grow()).h(px(28.0)).pads([0.0, 14.0, 0.0, 10.0]);
    let head = head.align(Align::Start, Align::Center);
    ui.row(head.borders([0.0, 0.0, 1.0, 0.0], t.line), |ui| {
      let s = TextStyle::new(t.small, t.muted).nowrap();
      ui.text("Name", s);
      ui.spacer();
      if sizes {
        ui.text("Size", s);
      }
    });

    // In the name's font, as Finder's columns: a smaller one would sit off its baseline.
    let (name, size) = (t.label(), t.muted());
    let (added, n) = (&self.added, self.added.len());
    let key = |i: usize| if i < n { (ROWS + n - 1 - i) as u64 } else { (i - n) as u64 };
    let r = List::new(self.count(), ROW_H)
      .multi()
      .key(&key)
      .selection(&mut self.sel)
      .style(Style::new().grow().pad_xy(0.0, 4.0))
      .show(ui.id(LIST), |ui, row| {
        // The added rows first, the last added at the top; a folder has no size, as in Finder.
        let (label, kb) = match row.index.checked_sub(n) {
          Some(i) => (format!("Item {:06}", i + 1), Some((i * 37) % 900 + 12)),
          None => {
            let e = &added[n - 1 - row.index];
            (e.name.clone(), e.size.map(|b| b.div_ceil(1000) as usize))
          }
        };

        ui.text(label, name);
        ui.spacer();
        match kb.filter(|_| sizes) {
          Some(kb) => ui.text(format_args!("{kb} KB"), size.nowrap()),
          None if sizes => ui.text("--", size.nowrap()),
          None => {}
        }
      });
    if let Some(i) = r.activated {
      self.opened = Some(self.key(i) as usize);
    }

    match ui.context_menu(r.id, || self.row_menu()) {
      Some(item) => self.choose(item),
      None if self.menu_open && !ui.menu_open(r.id) => eprintln!("menu: none"),
      None => {}
    }

    self.menu_open = ui.menu_open(r.id);
    // Files dragged over it: a ring in the accent inside the pane, fading in and out.
    let lit = ui.hot(Id::of(LIST).key("drop"), self.over);
    if lit > 0.0 {
      let ring = Style::new().grow().radius(6.0).border(2.0, alpha(t.accent, lit));
      ui.float(Float::parent()).el(ring.bg(alpha(t.accent, 0.08 * lit)));
    }
  }

  fn info(&self, ui: &mut Ui) {
    let t = ui.theme();
    let text = TextStyle::new(t.font, t.text);
    // Each title over its value, the pairs further apart than a title from its value.
    let pair = Style::new().w(grow()).gap(6.0);
    ui.col(Style::new().grow().pad(16.0).gap(16.0), |ui| {
      ui.col(pair, |ui| {
        ui.text("SELECTION", t.section());
        match self.sel.cursor() {
          Some(k) => ui.text(format_args!("{}, {} selected", self.name(k), self.sel.len()), text),
          None => ui.text("Nothing selected", t.muted()),
        }
      });

      ui.col(pair, |ui| {
        ui.text("OPENED", t.section());
        match self.opened {
          Some(k) => ui.text(self.name(k as u64), text),
          None => ui.text("Press Enter or double-click a row", t.muted()),
        }
      });

      ui.col(pair, |ui| {
        ui.text("SORTED BY", t.section());
        ui.text(SORTS[self.sort], text);
      });
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

  /// The status bar: how many rows are selected of how many, and the app's note.
  fn status(&self, ui: &mut Ui) {
    let t = ui.theme();
    let bar = Style::new().w(grow()).h(px(24.0)).center().borders([1.0, 0.0, 0.0, 0.0], t.line);
    ui.row(bar, |ui| {
      let (n, all) = (thousands(self.sel.len()), thousands(self.count()));
      let s = TextStyle::new(t.small, t.muted).nowrap();
      match &self.note {
        Some(note) => ui.text(format_args!("{n} of {all} selected · {note}"), s),
        None => ui.text(format_args!("{n} of {all} selected"), s),
      }
    });
  }

  /// The browser under its toolbar, which the window builds: the list and the inspector in a
  /// split, over the status bar.
  pub fn content(&mut self, ui: &mut Ui) {
    ui.el(Style::new().h(px(ui.toolbar_height())));
    // The split and the tabs edit copies: their panes borrow the browser.
    let (mut side, mut tab) = (self.side, self.tab);
    Split::new(Axis::Horizontal).ratio(&mut side).min(200.0, 220.0).show(
      ui.id(SPLIT),
      |ui, pane| match pane {
        0 => self.list(ui),
        _ => {
          _ = Tabs::new(&mut tab, &TITLES).show(ui.id(TABS), |ui, i| match i {
            0 => self.info(ui),
            _ => self.notes(ui),
          })
        }
      },
    );
    (self.side, self.tab) = (side, tab);
    self.status(ui);
  }
}

/// `n` with a comma between thousands, as Finder counts.
fn thousands(n: usize) -> String {
  let s = n.to_string();
  let mut out = String::new();
  for (i, c) in s.chars().enumerate() {
    if i > 0 && (s.len() - i).is_multiple_of(3) {
      out.push(',');
    }

    out.push(c);
  }

  out
}

const NOTE: [&str; 3] = [
  "The list builds only the rows in view and two more past each edge, between two spacers \
   that give it the height of all its rows, so its scrollbar is right.",
  "Selection is by row key, not by element: it stays when rows scroll out of view.",
  "This page scrolls under fades at its top and bottom edges, each showing as the text moves \
   away from it.",
];

impl App for Browser {
  fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
    self.drags(cx, ev)
  }

  fn view(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    ui.col(Style::new().grow().bg(t.bg), |ui| {
      ui.toolbar(|ui| self.toolbar_items(ui));
      self.content(ui);
    });
  }
}

/// The browser alone, as `files -- browser` opens it; the shot has rows 3 to 6 selected.
pub fn run(args: &[String]) {
  nimbo::run_args(Config::new("Browser", SIZE.w, SIZE.h), Browser::new(), shot, args);
}

/// Rows 3 to 6 selected by a click and a Shift-click.
pub fn shot<A: App>(h: &mut Harness<A>) {
  h.frame();
  let row = |h: &Harness<A>, k: u64| h.rect(Id::of(LIST).key(("row", k))).unwrap();
  let r = row(h, 2);
  h.click(r.x + 20.0, r.y + r.h / 2.0);
  let r = row(h, 5);
  h.press_with(r.x + 20.0, r.y + r.h / 2.0, 1, Modifiers::SHIFT);
  h.release(r.x + 20.0, r.y + r.h / 2.0);
  h.wait(Duration::from_millis(300));
}
