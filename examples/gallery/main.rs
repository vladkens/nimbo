// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A catalog of nimbo's building blocks, each beside its SwiftUI counterpart: a sidebar of every
//! block by group (layout, text, controls, form, list, containers, presentation, modifiers) and a
//! page for the one chosen, with the SwiftUI line, the nimbo line and the block live in its
//! variants (enabled and disabled, small and large where it has sizes), in the system's light or
//! dark. The same table makes `docs/api.md` and its pictures (`make api-shots`).
//!
//! `cargo run --release --example gallery [-- <block>] [--shot <file.png>]`: with a block's slug
//! the window is that block's variants alone, as high as they are; `--api` prints `docs/api.md`,
//! `--list` the slugs.
//!
//! The table is in `blocks.rs`, the blocks built live in a module per group in `pages/`.

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod blocks;
mod pages;

use nimbo::{
  App, Bitmap, Config, Font, HelpStyle, Hotkey, List, Selection, Style, Tag, TextStyle, Ui, grow,
  px,
};

pub use self::blocks::{BLOCKS, Block, GROUPS};

pub const SIDEBAR: &str = "sidebar";
pub const PAGE: &str = "page";
pub const DIALOG: &str = "dialog";
/// A block alone ([Gallery::only]).
pub const BLOCK: &str = "block";
/// The width of a page, and of a block's picture.
pub const WIDTH: f32 = 520.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
  Small,
  Medium,
  Large,
}

pub const SIZES: [Tag<Size>; 3] = [
  Tag::new(Size::Small, "Small"),
  Tag::new(Size::Medium, "Medium"),
  Tag::new(Size::Large, "Large"),
];

pub struct Gallery {
  /// The block shown, an index into [BLOCKS].
  pub page: usize,
  pub sel: Selection,
  /// One block alone, as high as its variants: a picture for `docs/api.md`.
  pub only: bool,
  pub live: bool,
  pub wrap: bool,
  pub numbers: bool,
  pub bold: bool,
  pub italic: bool,
  pub zoom: f32,
  pub level: f32,
  pub size: Size,
  pub view: u8,
  pub name: String,
  pub key: String,
  pub query: String,
  /// The text field blocks' others, empty at first.
  pub notes: String,
  pub city: String,
  pub email: String,
  pub hotkey: Option<Hotkey>,
  pub tab: usize,
  pub side: f32,
  /// The toolbar's sidebar share.
  pub bar_side: f32,
  pub rows: Selection,
  /// The list behaviour block's, its second row selected at first.
  pub own_rows: Selection,
  /// The list style blocks' plain and styled lists', their second row selected at first.
  pub styled_rows: [Selection; 2],
  pub ask: bool,
  pub deleted: usize,
  pub copied: usize,
  pub on: bool,
  /// The photo block's, decoded by the OS at twice its size in points.
  pub photo: Option<Bitmap>,
}

/// The localized text block's Russian (`--locale ru`).
pub const RU: &str = r#"
msgid ""
msgstr "Plural-Forms: nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);\n"

msgid "Keep the history"
msgstr "Хранить историю"

msgid "{name} is not on disk"
msgstr "{name} нет на диске"

msgid "{n} file"
msgid_plural "{n} files"
msgstr[0] "{n} файл"
msgstr[1] "{n} файла"
msgstr[2] "{n} файлов"
"#;

/// A sunset drawn for the photo block, 240 × 160.
pub const SUNSET: &[u8] = include_bytes!("../../tests/images/sunset.jpg");

/// A selection of the second row.
fn second() -> Selection {
  let mut s = Selection::default();
  s.select(1, 1);
  s
}

impl Default for Gallery {
  fn default() -> Self {
    Self {
      page: 0,
      sel: Selection::default(),
      only: false,
      live: true,
      wrap: false,
      numbers: true,
      bold: true,
      italic: false,
      zoom: 60.0,
      level: 30.0,
      size: Size::Medium,
      view: 0,
      name: "Alex".into(),
      key: "secret".into(),
      query: "Switches".into(),
      notes: String::new(),
      city: String::new(),
      email: String::new(),
      hotkey: Hotkey::parse("ctrl+option+space").ok(),
      tab: 0,
      side: 0.4,
      bar_side: 0.4,
      rows: Selection::default(),
      own_rows: second(),
      styled_rows: [second(), second()],
      ask: false,
      deleted: 0,
      copied: 0,
      on: true,
      photo: Bitmap::decode(SUNSET, 240).ok(),
    }
  }
}

impl Gallery {
  /// The gallery on `page`, alone when `only`.
  pub fn on(page: usize, only: bool) -> Self {
    Self { page, only, ..Self::default() }
  }

  /// The page of the block shown: its name, SwiftUI's and nimbo's lines, then the block live.
  fn page(&mut self, ui: &mut Ui) {
    let (t, block) = (ui.theme(), &BLOCKS[self.page]);
    let mono = TextStyle::new(Font::mono(12.0), t.text);
    let code = Style::new().w(grow()).pad(10.0).gap(4.0).radius(8.0).bg(t.raised);
    ui.id(PAGE).scroll_area(Style::new().grow(), |ui| {
      ui.col(Style::new().w(grow()).pad(20.0).gap(14.0), |ui| {
        ui.text(block.name, TextStyle::new(Font { size: 20.0, ..t.bold }, t.text));
        ui.col(code, |ui| {
          ui.text("SWIFTUI", t.section());
          ui.selectable_text(block.swiftui, mono);
          ui.text("NIMBO", t.section());
          ui.selectable_text(block.nimbo, mono);
        });
        self.demo(ui, block);
      });
    });
  }

  /// The sidebar: every block by group, the one shown selected.
  fn sidebar(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    let starts: Vec<usize> =
      GROUPS.iter().filter_map(|g| BLOCKS.iter().position(|b| b.group == *g)).collect();
    let head = |ui: &mut Ui, g: usize| ui.text(GROUPS[g], t.section());
    let list = List::new(BLOCKS.len(), 24.0).sections(&starts, 26.0, &head);
    let list = list.selection(&mut self.sel).style(Style::new().w(px(200.0)).h(grow()).bg(t.panel));
    list.show(ui.id(SIDEBAR), |ui, row| ui.text(BLOCKS[row.index].name, t.label()));
    if let Some(i) = self.sel.cursor_index(BLOCKS.len(), &|i| i as u64) {
      self.page = i;
    }
  }
}

impl App for Gallery {
  fn view(&mut self, ui: &mut Ui) {
    // The system's look and the shell's tips, whatever a block set last frame.
    ui.follow_system();
    ui.set_environment::<&dyn HelpStyle>(None);
    let t = ui.theme();
    if self.only {
      ui.fit_window_height(WIDTH, 1.0, 900.0);
      let block = &BLOCKS[self.page];
      ui.id(BLOCK).col(Style::new().w(grow()).pad(20.0).bg(t.bg), |ui| self.demo(ui, block));
      return;
    }

    if self.sel.is_empty() {
      self.sel.select(self.page as u64, self.page);
    }

    ui.row(Style::new().grow().bg(t.bg), |ui| {
      self.sidebar(ui);
      ui.col(Style::new().grow(), |ui| self.page(ui));
    });
  }
}

/// Where `docs/api.md`'s pictures are: the repo's `assets` branch, which `make api-shots` fills.
pub const PICTURES: &str = "https://raw.githubusercontent.com/vladkens/nimbo/assets/api";

/// `docs/api.md`: per group, each block's SwiftUI line, nimbo's and its picture.
pub fn api_md() -> String {
  let mut out = String::from(
    "# nimbo and SwiftUI\n\n\
     Each building block of nimbo beside its SwiftUI counterpart, as the gallery shows it live \
     (`cargo run --release --example gallery`, a page per block in its variants). A block aims \
     at its SwiftUI counterpart's behaviour and API shape without copying it 1:1. The pictures \
     are the gallery's, in the light appearance, kept on the repo's `assets` branch: `make \
     api-shots` makes them and this file again from the gallery's table \
     (`examples/gallery/blocks.rs`), so a block changes there.\n\n\
     On macOS SwiftUI's `List` has neither of the grouped styles (`InsetGroupedListStyle` and \
     `GroupedListStyle` are iOS's): nimbo's `InsetGroupedListStyle` takes the iOS name for the \
     plates of the grouped `Form` (`.formStyle(.grouped)`), which its sections share.\n",
  );
  for g in GROUPS {
    out += &format!("\n## {g}\n");
    for b in BLOCKS.iter().filter(|b| b.group == g) {
      let (name, slug) = (b.name, b.slug());
      out += &format!(
        "\n### {name}\n\nSwiftUI: `{}`\n\nnimbo: `{}`\n\n![{name}]({PICTURES}/{slug}.png)\n",
        b.swiftui, b.nimbo
      );
    }
  }

  out
}

fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  match args.first().map(String::as_str) {
    Some("--api") => return print!("{}", api_md()),
    Some("--list") => return BLOCKS.iter().for_each(|b| println!("{}", b.slug())),
    _ => {}
  }

  // A block's slug first: that block alone.
  let page = args.first().and_then(|a| BLOCKS.iter().position(|b| b.slug() == *a));
  let rest = &args[page.is_some() as usize..];
  let app = Gallery::on(page.unwrap_or(0), page.is_some());
  let (w, h) = if page.is_some() { (WIDTH, 300.0) } else { (WIDTH + 200.0, 600.0) };
  let cfg = Config::new("Gallery", w, h).localizations(&[("ru", RU)]);
  nimbo::run_args(cfg, app, |h| _ = h.frame(), rest);
}
