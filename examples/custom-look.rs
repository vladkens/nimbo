// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A look entirely the app's own over the headless layer: Rill, a small git client in the layout
//! and density of a desktop merge tool. Repository tabs over a toolbar (the branch, a search,
//! fetch, pull and push); a sidebar of branches, remotes, tags and stashes in sections that fold;
//! the history as a graph of lanes beside each commit's labels, message, author and date; the
//! commit picked, its files and a unified diff, syntax-coloured, its lines tinted and the changed
//! words stronger. The panes part at dividers that drag. Every control is drawn by a style of
//! the app's (button, toggle, picker, text field, list, tooltip, split) from a palette of its own,
//! dark first and light beside it; focus, the pointing hand, keys, selection and what assistive
//! technology reads are the behaviours', as under nimbo's look. The history and its diffs are
//! made up from a seed, the same each run.
//!
//! `cargo run --release --example custom-look [-- big|file|search] [--shot <file.png>]`: `big`
//! shows a commit of a 2,400-line diff scrolled, `file` one file of a commit, `search` a search.

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "common/example.rs"]
mod example;

use std::f32::consts::FRAC_PI_2;
use std::fmt::Write;
use std::time::Duration;

use nimbo::{
  Align, App, AttributedString, Attributes, Axis, Binding, ButtonConfiguration, ButtonStyle, Color,
  Command, Config, Cursor, Cx, EnvironmentValue, Event, FieldStyle, Float, FocusRing, Font, Frame,
  HelpConfiguration, HelpStyle, Icon, Id, LinePattern, List, ListConfiguration, ListLayout,
  ListStyle, Modifiers, Phase, PickerConfiguration, PickerStyle, Point, Rect, ScrollerKnob,
  Selection, Shortcut, Style, Tag, TextFieldConfiguration, TextFieldStyle, TextStyle,
  ToggleConfiguration, ToggleStyle, Transition, Ui, UnitPoint, Weight, alpha, grow, mix, pct, px,
  rgb,
};

// MARK: The look

/// The look's colours: a dark slate first, a paper-grey light one beside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
  /// The strip of repository tabs, the deepest.
  pub chrome: Color,
  /// The toolbar and the open tab.
  pub bar: Color,
  pub side: Color,
  /// The history.
  pub list: Color,
  /// The commit and its diff.
  pub view: Color,
  /// Fields, file headers, tooltips.
  pub raised: Color,
  /// The lines between panes.
  pub edge: Color,
  pub ink: Color,
  pub muted: Color,
  pub faint: Color,
  /// What is selected or focused.
  pub accent: Color,
  /// A line added or removed, and the words it changed.
  pub added: Color,
  pub added_word: Color,
  pub removed: Color,
  pub removed_word: Color,
  /// A hunk's header and its text.
  pub hunk: Color,
  pub hunk_ink: Color,
  /// The graph's lanes, in turn.
  pub lanes: [Color; 7],
  /// Code by [Tok]: plain, keyword, function, type, string, number, comment, macro.
  pub code: [Color; 8],
}

impl Palette {
  pub const DARK: Self = Self {
    chrome: rgb(0x18191c),
    bar: rgb(0x26282c),
    side: rgb(0x212327),
    list: rgb(0x2a2c31),
    view: rgb(0x24262a),
    raised: rgb(0x31343a),
    edge: rgb(0x141517),
    ink: rgb(0xd8dbe0),
    muted: rgb(0x8d939c),
    faint: rgb(0x5d636c),
    accent: rgb(0x4f8fe0),
    added: rgb(0x26352a),
    added_word: rgb(0x3a5c3c),
    removed: rgb(0x3a272a),
    removed_word: rgb(0x6a363c),
    hunk: rgb(0x2b3140),
    hunk_ink: rgb(0x8ba4cc),
    lanes: [
      rgb(0x4f8fe0),
      rgb(0xe3a04a),
      rgb(0x8bc163),
      rgb(0xc47ad8),
      rgb(0xe0646f),
      rgb(0x4fb7c2),
      rgb(0xd6c062),
    ],
    code: [
      rgb(0xd8dbe0),
      rgb(0xc595c5),
      rgb(0x6fa3dc),
      rgb(0xf0c674),
      rgb(0x98c379),
      rgb(0xf5a85f),
      rgb(0x7d8590),
      rgb(0x5fb7b7),
    ],
  };
  pub const LIGHT: Self = Self {
    chrome: rgb(0xd7d8db),
    bar: rgb(0xebecee),
    side: rgb(0xe3e4e7),
    list: rgb(0xf7f7f8),
    view: rgb(0xfdfdfd),
    raised: rgb(0xffffff),
    edge: rgb(0xc4c6ca),
    ink: rgb(0x24272b),
    muted: rgb(0x686e77),
    faint: rgb(0x9fa4ab),
    accent: rgb(0x2b70cf),
    added: rgb(0xe4f4df),
    added_word: rgb(0xb5e0a8),
    removed: rgb(0xfbe6e6),
    removed_word: rgb(0xf1b5b5),
    hunk: rgb(0xe7edf7),
    hunk_ink: rgb(0x4a6a9a),
    lanes: [
      rgb(0x2b70cf),
      rgb(0xd07d0c),
      rgb(0x4c9628),
      rgb(0x9a4dbd),
      rgb(0xc93c4a),
      rgb(0x16949f),
      rgb(0xa98f12),
    ],
    code: [
      rgb(0x24272b),
      rgb(0xa23ea0),
      rgb(0x2d6bd3),
      rgb(0xa87000),
      rgb(0x3f8f2f),
      rgb(0xb3560e),
      rgb(0x8c9199),
      rgb(0x0f86a8),
    ],
  };
}

impl Palette {
  /// A row's secondary text: `muted`, or nearer the ink on a selected row's fill.
  pub fn quiet(&self, selected: bool) -> Color {
    if selected { mix(self.ink, self.muted, 0.3) } else { self.muted }
  }
}

/// The colour scheme's palette, unless a container puts its own.
impl EnvironmentValue for Palette {
  fn default_value(ui: &Ui) -> Self {
    if ui.color_scheme().is_dark() { Self::DARK } else { Self::LIGHT }
  }
}

/// Hover and fold fades: 140 ms.
const FADE: Transition = Transition::linear(1000.0 / 140.0);
/// A row of the history, the sidebar and the file list; a line of the diff.
pub const ROW: f32 = 22.0;
pub const LINE: f32 = 18.0;
/// A lane of the graph.
const LANE: f32 = 12.0;

fn text(size: f32, color: Color) -> TextStyle {
  TextStyle::new(Font::ui(size), color).line_limit(1)
}

fn mono(size: f32, color: Color) -> TextStyle {
  TextStyle::new(Font::mono(size), color).nowrap()
}

fn bold(size: f32, color: Color) -> TextStyle {
  text(size, color).font(Font::ui(size).weight(Weight::Semibold))
}

/// How far `id`'s hover has faded in.
fn hot(ui: &mut Ui, id: Id, on: bool) -> f32 {
  ui.animate(id.key("hot"), on as u8 as f32, FADE)
}

/// A hairline across its parent.
fn rule(ui: &mut Ui, color: Color, axis: Axis) {
  let s = match axis {
    Axis::Horizontal => Style::new().w(grow()).h(px(1.0)),
    Axis::Vertical => Style::new().w(px(1.0)).h(grow()),
  };

  ui.el(s.bg(color));
}

/// The toolbar's: no frame at rest, a tint under the pointer, deeper while held; the label
/// brightens as the tint comes.
pub struct Tool;

impl ButtonStyle for Tool {
  fn make_body(&self, ui: &mut Ui, mut c: ButtonConfiguration) {
    let p = ui.environment_value::<Palette>();
    let h = hot(ui, c.id(), c.is_hovered());
    let k = if c.is_pressed() { 0.14 } else { 0.07 * h };
    let x = if c.is_icon_only() { 5.0 } else { 8.0 };
    let s = Style::new().h(px(24.0)).pad_xy(x, 0.0).gap(5.0).center().radius(4.0);
    let s = s.bg(alpha(p.ink, k)).opacity(if c.is_enabled() { 1.0 } else { 0.35 });
    ui.row(s, |ui| c.label(ui, text(12.0, mix(p.muted, p.ink, h))));
  }
}

/// A sidebar row: the width of the sidebar, indented under its section, tinted under the pointer.
pub struct SideRow;

impl ButtonStyle for SideRow {
  fn make_body(&self, ui: &mut Ui, mut c: ButtonConfiguration) {
    let p = ui.environment_value::<Palette>();
    let h = hot(ui, c.id(), c.is_hovered());
    let k = if c.is_pressed() { 0.12 } else { 0.06 * h };
    let s = Style::new().w(grow()).h(px(ROW)).pads([0.0, 8.0, 0.0, 22.0]).gap(6.0);
    let s = s.align(Align::Start, Align::Center).radius(4.0).bg(alpha(p.ink, k));
    ui.row(s, |ui| c.label(ui, text(12.0, p.ink)));
  }
}

/// A hash to follow: in the accent, underlined under the pointer.
pub struct HashLink;

impl ButtonStyle for HashLink {
  fn make_body(&self, ui: &mut Ui, mut c: ButtonConfiguration) {
    let p = ui.environment_value::<Palette>();
    let h = hot(ui, c.id(), c.is_hovered());
    let s = mono(11.5, mix(p.accent, p.ink, 0.35 * h));
    let s = if h > 0.5 { s.underline(LinePattern::Solid) } else { s };
    ui.row(Style::new(), |ui| c.label(ui, s));
  }
}

/// A section's header: a chevron turning down as it opens, the label in small capitals.
pub struct Disclosure;

impl ToggleStyle for Disclosure {
  fn make_body(&self, ui: &mut Ui, mut c: ToggleConfiguration) {
    let p = ui.environment_value::<Palette>();
    let h = hot(ui, c.id(), c.is_hovered());
    let k = ui.animate(c.id().key("open"), c.is_on() as u8 as f32, FADE);
    let ink = mix(p.muted, p.ink, h);
    let s = Style::new().w(grow()).h(px(26.0)).pad_xy(8.0, 0.0).gap(6.0).radius(4.0);
    ui.row(s.align(Align::Start, Align::Center).bg(alpha(p.ink, 0.04 * h)), |ui| {
      ui.canvas(Style::new().w(px(9.0)).h(px(9.0)), move |f, r| chevron(f, r, k, ink));
      c.label(ui, bold(10.5, ink));
    });
  }
}

/// ▸ at `k` 0, turned to ▾ at 1.
fn chevron(f: &mut Frame, r: Rect, k: f32, color: Color) {
  let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
  let (s, c) = (k * FRAC_PI_2).sin_cos();
  let at = |x: f32, y: f32| Point::new(cx + x * c - y * s, cy + x * s + y * c);
  f.polyline(&[at(-1.5, -3.5), at(2.0, 0.0), at(-1.5, 3.5)], 1.4, color);
}

/// The repositories' tabs: the open one in the toolbar's colour, joined to it; the rest on the
/// strip, lifting under the pointer.
pub struct Tabs;

impl PickerStyle for Tabs {
  fn make_body(&self, ui: &mut Ui, mut c: PickerConfiguration) {
    let p = ui.environment_value::<Palette>();
    let b = c.segments(ui);
    ui.row(Style::new().h(grow()), |ui| {
      for i in 0..c.len() {
        let (id, on) = (c.item_id(i), c.selection() == Some(i));
        let h = hot(ui, id, !on && ui.hovered(id));
        let bg = if on { p.bar } else { mix(p.chrome, p.bar, 0.45 * h) };
        let ink = if on { p.ink } else { mix(p.muted, p.ink, h) };
        let seg = Style::new().h(grow()).pad_xy(16.0, 0.0).gap(7.0).center().bg(bg);
        ui.with_id(id).row(b.pointer(seg.borders([0.0, 1.0, 0.0, 0.0], p.edge)), |ui| {
          ui.icon(REPO, 13.0, if on { p.accent } else { ink });
          ui.text(c.label(i), if on { bold(12.0, ink) } else { text(12.0, ink) });
        });
      }
    });
  }
}

/// A sunken box with a magnifier, its edge taking the accent while focused.
pub struct Find;

impl TextFieldStyle for Find {
  fn make_body(&self, ui: &mut Ui, mut c: TextFieldConfiguration) {
    let p = ui.environment_value::<Palette>();
    let h = hot(ui, c.id(), c.is_hovered());
    let edge = if c.is_focused() { p.accent } else { mix(p.edge, p.faint, 0.7 * h) };
    let s = Style::new().w(grow()).h(px(24.0)).pads([0.0, 8.0, 0.0, 26.0]).radius(4.0);
    let s = s.bg(p.raised).border(1.0, edge).align(Align::Start, Align::Center);
    let field = FieldStyle::new(text(12.0, p.ink)).placeholder(p.faint).caret(p.accent);
    ui.row(s.focusable_quiet().scroll(c.axis().into()), |ui| {
      let at = Float::parent().attach(UnitPoint::LEADING).anchor(UnitPoint::LEADING);
      let glass = Icon::SEARCH.stroke(1.8);
      let s = Style::new().w(px(14.0)).h(px(14.0));
      ui.float(at.offset(8.0, 0.0)).canvas(s, move |f, r| glass.draw(f, r, p.muted));
      c.text(ui, &field.selection(alpha(p.accent, 0.35)));
    });
  }
}

/// Rows edge to edge on the pane's ground: the selected one in the accent while the list has
/// focus, a grey of the ink while not; a faint tint fading in under the pointer.
pub struct Rows;

impl ListStyle for Rows {
  fn layout(&self, _: &Ui) -> ListLayout {
    let mut l = ListLayout::default();
    l.row = Style::new().w(grow()).gap(6.0).align(Align::Start, Align::Center);
    l
  }

  fn make_body<'a>(&self, ui: &mut Ui<'a>, mut c: ListConfiguration<'_, 'a>) {
    let p = ui.environment_value::<Palette>();
    ui.scroll_area(c.frame(), |ui| {
      c.rows(ui, |ui, mut r| {
        let h = hot(ui, r.id(), r.is_hovered());
        let fill = match (r.is_selected(), r.is_focused()) {
          (true, true) => alpha(p.accent, 0.42),
          (true, false) => alpha(p.ink, 0.12),
          _ => alpha(p.ink, 0.045 * h),
        };

        ui.row(r.frame().bg(fill), |ui| r.content(ui));
      })
    });
  }
}

/// A raised slip with a hairline edge.
pub struct Slip;

impl HelpStyle for Slip {
  fn make_body(&self, ui: &mut Ui, c: HelpConfiguration) {
    let p = ui.environment_value::<Palette>();
    let s = Style::new().pad_xy(7.0, 4.0).radius(3.0).bg(p.raised).border(1.0, p.edge);
    ui.row(s.max_w(260.0).opacity(c.opacity()), |ui| ui.text(c.text(), text(11.0, p.ink)));
  }
}

/// Two panes side by side over the split's behaviour, a hairline between them that takes the
/// accent while dragged or under the pointer, a wider grip over it.
fn split<'a>(
  ui: &mut Ui<'a>,
  id: Id,
  ratio: &mut f32,
  min: [f32; 2],
  mut pane: impl FnMut(&mut Ui<'a>, usize),
) {
  let p = ui.environment_value::<Palette>();
  let handle = id.key("handle");
  let b = ui.split_behavior(id, handle, Axis::Horizontal, ratio, min);
  let h = hot(ui, handle, b.handle.hovered || b.handle.held);
  ui.with_id(id).row(Style::new().grow().clip(), |ui| {
    let first = Style::new().w(pct(b.shown * 100.0)).min_w(b.min[0]).h(grow()).clip();
    ui.col(first, |ui| pane(ui, 0));
    ui.row(Style::new().w(px(1.0)).h(grow()).bg(mix(p.edge, p.accent, h)), |ui| {
      let at = Float::parent().attach(UnitPoint::CENTER).anchor(UnitPoint::CENTER).clip();
      let grip = Style::new().w(px(7.0)).h(grow()).cursor(Cursor::ResizeX);
      ui.with_id(handle).float(at).el(grip);
    });
    ui.col(Style::new().grow().min_w(b.min[1]).clip(), |ui| pane(ui, 1));
  });
}

pub const REPO: Icon = Icon::new("M5 4.5h11.5v15H5zM8.5 4.5v15M11 9h3");
pub const BRANCH: Icon =
  Icon::new("M7 4v16M17 9.5a2.5 2.5 0 1 0 0-5a2.5 2.5 0 1 0 0 5M17 9.5c0 4.5-10 3.5-10 8.5");
pub const REMOTE: Icon =
  Icon::new("M7 18h10a4 4 0 0 0 .5-8a5.5 5.5 0 0 0-10.7-1A4.5 4.5 0 0 0 7 18");
pub const TAG: Icon = Icon::new("M4 4.5h7.5l8.5 8.5-7 7-9-8.5zM8.2 8.7h.1");
pub const STASH: Icon = Icon::new("M4 6.5h16v4H4zM5.5 10.5v8h13v-8M10 14h4");
pub const FETCH: Icon =
  Icon::new("M5 12a7 7 0 0 1 12.2-4.7M19 12a7 7 0 0 1-12.2 4.7M17.5 3.5v4h-4M6.5 20.5v-4h4");
pub const PULL: Icon = Icon::new("M12 4v12.5M6.5 11l5.5 5.5 5.5-5.5M5 20h14");
pub const PUSH: Icon = Icon::new("M12 20V7.5M6.5 13 12 7.5l5.5 5.5M5 4h14");

// MARK: The history

/// The people committing, and where they write from.
pub const AUTHORS: [(&str, &str); 6] = [
  ("Mara Lindqvist", "mara@quill.dev"),
  ("Tomás Aguilar", "tomas@quill.dev"),
  ("Priya Natarajan", "priya.n@fastmail.com"),
  ("Jonah Weiss", "jonah@weiss.io"),
  ("Yuki Tanaka", "yuki@quill.dev"),
  ("Ola Nwosu", "ola.nwosu@proton.me"),
];

const AREAS: [&str; 10] =
  ["buffer", "render", "lsp", "config", "keymap", "search", "ui", "fs", "undo", "syntax"];

const CHANGES: [&str; 24] = [
  "fix the off-by-one at the last line",
  "cache the shaped runs per line",
  "drop the unused allocator",
  "handle CRLF on paste",
  "faster word boundaries",
  "keep the cursor column on vertical moves",
  "retry with backoff",
  "read the user's file once at start",
  "group edits typed within a second",
  "a test for an empty document",
  "skip hidden files when watching",
  "release the lock before the callback",
  "trim the trailing whitespace on save",
  "follow the system's dark mode",
  "case-insensitive matches by Unicode fold",
  "split the view into panes",
  "no panic on a closed pipe",
  "debounce the reindex",
  "a smaller default font",
  "rename Doc to Document",
  "document the public API",
  "clippy's suggestions",
  "bump the toolchain to 1.92",
  "measure tabs as four columns",
];

const FEATURES: [&str; 8] = [
  "feature/panes",
  "feature/lsp-hover",
  "fix/crlf",
  "feature/minimap",
  "perf/shaping",
  "feature/vim-keys",
  "fix/watcher",
  "feature/themes",
];

/// The seeded numbers the history is made of.
struct Rng(u64);

impl Rng {
  fn next(&mut self) -> u64 {
    self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    self.0 >> 33
  }

  fn below(&mut self, n: usize) -> usize {
    (self.next() % n as u64) as usize
  }

  /// True `percent` times in a hundred.
  fn chance(&mut self, percent: u64) -> bool {
    self.next() % 100 < percent
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Commit {
  pub hash: String,
  /// Their places in the history, newest first: the first parent, then the merged.
  pub parents: Vec<usize>,
  /// Into [AUTHORS].
  pub author: usize,
  /// Minutes since 2026-01-01 00:00.
  pub time: u32,
  pub subject: String,
  /// Into the app's diffs.
  pub diff: usize,
}

/// A name for a commit: a branch, a remote's, a tag, a stash.
#[derive(Clone, Debug, PartialEq)]
pub struct Ref {
  pub name: String,
  pub commit: usize,
}

/// A row of the graph: the lanes through its top half (from a lane at the top to one at the
/// middle) and its bottom half (from the middle down), and the commit's own lane and colour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphRow {
  pub top: Vec<(u8, u8, u8)>,
  pub bottom: Vec<(u8, u8, u8)>,
  pub lane: u8,
  pub color: u8,
}

pub struct Repo {
  pub name: &'static str,
  /// Newest first.
  pub commits: Vec<Commit>,
  pub graph: Vec<GraphRow>,
  /// The most lanes a row has.
  pub lanes: usize,
  /// Branches, remotes, tags and stashes.
  pub refs: [Vec<Ref>; 4],
}

pub const SECTIONS: [&str; 4] = ["BRANCHES", "REMOTES", "TAGS", "STASHES"];
pub const SECTION_ICONS: [Icon; 4] = [BRANCH, REMOTE, TAG, STASH];

impl Repo {
  /// `n` commits made up from `seed`: a main line and feature branches forking off it, merged
  /// back (now and then merging main in first), some still open.
  pub fn generate(name: &'static str, seed: u64, n: usize, diffs: usize) -> Self {
    // Oldest first while made: (parents, author, subject, diff, branch).
    let mut rng = Rng(seed);
    let mut raw: Vec<(Vec<usize>, usize, String, usize)> = Vec::new();
    let subject = |rng: &mut Rng| {
      format!("{}: {}", AREAS[rng.below(AREAS.len())], CHANGES[rng.below(CHANGES.len())])
    };

    raw.push((vec![], 0, "Initial commit".into(), 1));
    let mut main = 0;
    // A feature: its name, its tip, how many commits it has to go, whether it made one.
    let mut feats: Vec<(usize, usize, usize, bool)> = Vec::new();
    let mut named = 0;
    while raw.len() < n - 4 {
      let at = raw.len();
      let diff = 2 + rng.below(diffs - 2);
      if feats.len() < 4 && rng.chance(8) {
        feats.push((named % FEATURES.len(), main, 2 + rng.below(9), false));
        named += 1;
        continue;
      }

      let author = rng.below(AUTHORS.len());
      if feats.is_empty() || rng.chance(40) {
        raw.push((vec![main], author, subject(&mut rng), diff));
        main = at;
        continue;
      }

      let k = rng.below(feats.len());
      let (f, tip, left, made) = feats[k];
      if left == 0 && made {
        let s = format!("Merge branch '{}'", FEATURES[f]);
        raw.push((vec![main, tip], author, s, diff));
        main = at;
        feats.remove(k);
      } else if left == 3 && rng.chance(40) {
        let s = format!("Merge branch 'main' into {}", FEATURES[f]);
        raw.push((vec![tip, main], author, s, diff));
        feats[k] = (f, at, left - 1, true);
      } else {
        raw.push((vec![tip], author, subject(&mut rng), diff));
        feats[k] = (f, at, left.saturating_sub(1), true);
      }
    }

    // The big diff on main, three commits before the newest.
    for diff in [0, 5, 3, 2] {
      let (at, author) = (raw.len(), rng.below(AUTHORS.len()));
      let s = if diff == 0 { "syntax: generated width tables".into() } else { subject(&mut rng) };
      raw.push((vec![main], author, s, diff));
      main = at;
    }

    let last = raw.len() - 1;
    let flip = |i: usize| last - i;
    // From 21 Aug 2026.
    let mut time = 232 * 1440u32;
    let mut commits: Vec<Commit> = raw
      .into_iter()
      .enumerate()
      .map(|(i, (parents, author, subject, diff))| {
        time += 20 + rng.below(600) as u32;
        let hash = hash(seed, i);
        Commit {
          hash,
          parents: parents.into_iter().map(flip).collect(),
          author,
          time,
          subject,
          diff,
        }
      })
      .collect();
    commits.reverse();

    let first_parents = |mut c: usize, k: usize| {
      for _ in 0..k {
        c = commits[c].parents.first().copied().unwrap_or(c);
      }

      c
    };
    let head = flip(main);
    let mut branches = vec![Ref { name: "main".into(), commit: head }];
    let open: Vec<_> = feats.iter().filter(|f| f.3).collect();
    branches.extend(open.iter().map(|f| Ref { name: FEATURES[f.0].into(), commit: flip(f.1) }));
    let mut remotes = vec![Ref { name: "origin/main".into(), commit: first_parents(head, 2) }];
    remotes.extend(open.iter().take(1).map(|f| Ref {
      name: format!("origin/{}", FEATURES[f.0]),
      commit: first_parents(flip(f.1), 1),
    }));
    let mut tags = Vec::new();
    let mut c = first_parents(head, 6);
    for v in (1..=6).rev() {
      tags.push(Ref { name: format!("v0.{v}.0"), commit: c });
      c = first_parents(c, 30 + v * 3);
    }

    let stash = |k: usize, c: usize| Ref {
      name: format!("stash@{{{k}}}: WIP on main: {}", commits[c].subject),
      commit: c,
    };
    let stashes = vec![stash(0, first_parents(head, 1)), stash(1, first_parents(head, 9))];
    let (graph, lanes) = graph(&commits);
    Self { name, commits, graph, lanes, refs: [branches, remotes, tags, stashes] }
  }

  /// The commits ahead of their remote on the branch checked out.
  pub fn ahead(&self) -> usize {
    let (mut c, remote) = (self.refs[0][0].commit, self.refs[1][0].commit);
    let mut n = 0;
    while c != remote && n < self.commits.len() {
      c = self.commits[c].parents.first().copied().unwrap_or(remote);
      n += 1;
    }

    n
  }
}

/// A made-up commit hash, 40 hex digits.
fn hash(seed: u64, i: usize) -> String {
  let mut x = seed ^ (i as u64).wrapping_mul(0x9e3779b97f4a7c15);
  let mut s = String::with_capacity(40);
  for _ in 0..3 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^= x >> 31;
    let _ = write!(s, "{x:016x}");
  }

  s.truncate(40);
  s
}

/// The graph's rows, from the newest commit down: each lane waits for a commit, the first lane
/// waiting for one is its own, the others end in it; its first parent takes its lane, the
/// others one waiting for them or a free one. Returns the rows and the most lanes.
fn graph(commits: &[Commit]) -> (Vec<GraphRow>, usize) {
  let mut lanes: Vec<Option<(usize, u8)>> = Vec::new();
  let (mut rows, mut most, mut colors) = (Vec::with_capacity(commits.len()), 0, 0u8);
  let free = |lanes: &mut Vec<Option<(usize, u8)>>| match lanes.iter().position(Option::is_none) {
    Some(j) => j,
    None => {
      lanes.push(None);
      lanes.len() - 1
    }
  };

  for (i, c) in commits.iter().enumerate() {
    let mut row = GraphRow::default();
    let waiting: Vec<usize> =
      (0..lanes.len()).filter(|&j| lanes[j].is_some_and(|l| l.0 == i)).collect();
    let (lane, color) = match waiting.first() {
      Some(&j) => (j, lanes[j].unwrap().1),
      None => {
        colors = colors.wrapping_add(1);
        (free(&mut lanes), (colors - 1) % 7)
      }
    };

    for (j, l) in lanes.iter().enumerate() {
      if let Some((want, color)) = *l {
        let to = if want == i { lane } else { j };
        row.top.push((j as u8, to as u8, color));
      }
    }

    for &j in &waiting {
      lanes[j] = None;
    }

    let mut fresh = Vec::new();
    if let Some(&p) = c.parents.first() {
      lanes[lane] = Some((p, color));
      fresh.push(lane);
    }

    for &p in c.parents.iter().skip(1) {
      match lanes.iter().position(|l| l.is_some_and(|l| l.0 == p)) {
        Some(k) => row.bottom.push((lane as u8, k as u8, lanes[k].unwrap().1)),
        None => {
          colors = colors.wrapping_add(1);
          let (k, color) = (free(&mut lanes), (colors - 1) % 7);
          lanes[k] = Some((p, color));
          row.bottom.push((lane as u8, k as u8, color));
          fresh.push(k);
        }
      }
    }

    for (j, l) in lanes.iter().enumerate() {
      match *l {
        Some((_, color)) if j == lane || !fresh.contains(&j) => {
          row.bottom.push((j as u8, j as u8, color))
        }
        _ => {}
      }
    }

    while lanes.last().is_some_and(Option::is_none) {
      lanes.pop();
    }

    (row.lane, row.color) = (lane as u8, color);
    most = most.max(lanes.len()).max(lane + 1);
    rows.push(row);
  }

  (rows, most)
}

/// A commit's time as the list shows it, `27 Nov 2026`, and as its view does, `Fri, 27 Nov 2026
/// 14:05`.
fn when(minutes: u32, long: bool) -> String {
  const MONTHS: [&str; 12] =
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
  // From 2026-01-01, a Thursday.
  let (mut day, mut month) = (minutes / 1440, 0);
  let lens = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  let year = 2026 + day / 365;
  day %= 365;
  while day >= lens[month] {
    day -= lens[month];
    month += 1;
  }

  let (h, m) = (minutes / 60 % 24, minutes % 60);
  match long {
    true => {
      let weekday = DAYS[(minutes / 1440 % 7) as usize];
      format!("{weekday}, {} {} {year} {h:02}:{m:02}", day + 1, MONTHS[month])
    }
    false => format!("{} {} {year}", day + 1, MONTHS[month]),
  }
}

// MARK: Diffs

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
  Added,
  Modified,
  Deleted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
  /// A file's header.
  File,
  Hunk,
  Same,
  Add,
  Del,
}

/// What a piece of code is, for its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
  Plain,
  Keyword,
  Func,
  Type,
  Str,
  Num,
  Comment,
  Macro,
}

/// A line of a diff: its numbers in the old and the new file, its code in pieces, and the bytes
/// it changed from the line it pairs with.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
  pub kind: LineKind,
  pub file: usize,
  pub old: u32,
  pub new: u32,
  pub text: String,
  pub toks: Vec<(u32, u32, Tok)>,
  pub word: Option<(u32, u32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileDiff {
  pub path: String,
  pub status: Status,
  pub added: u32,
  pub removed: u32,
}

/// A commit's diff: its files, and their lines in a row, each file from its header.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diff {
  pub files: Vec<FileDiff>,
  pub lines: Vec<Line>,
  /// Where each file starts in `lines`, and the end.
  pub starts: Vec<usize>,
}

const KEYWORDS: [&str; 34] = [
  "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "false",
  "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
  "return", "self", "Self", "static", "struct", "super", "trait", "true", "use", "where",
];

/// The pieces of Rust in `s` that aren't plain.
fn tokens(s: &str) -> Vec<(u32, u32, Tok)> {
  let b = s.as_bytes();
  let (mut out, mut i) = (Vec::new(), 0);
  let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
  while i < b.len() {
    let (c, start) = (b[i], i);
    let tok = match c {
      b'/' if b.get(i + 1) == Some(&b'/') => {
        i = b.len();
        Tok::Comment
      }
      b'"' => {
        i += 1;
        while i < b.len() && b[i] != b'"' {
          i += if b[i] == b'\\' { 2 } else { 1 };
        }

        i = (i + 1).min(b.len());
        Tok::Str
      }
      b'0'..=b'9' => {
        while i < b.len() && word(b[i]) {
          i += 1;
        }

        Tok::Num
      }
      c if word(c) => {
        while i < b.len() && word(b[i]) {
          i += 1;
        }

        match (&s[start..i], b.get(i)) {
          (w, _) if KEYWORDS.contains(&w) => Tok::Keyword,
          (_, Some(b'!')) => {
            i += 1;
            Tok::Macro
          }
          (_, Some(b'(')) => Tok::Func,
          _ if c.is_ascii_uppercase() => Tok::Type,
          _ => Tok::Plain,
        }
      }
      _ => {
        i += s[i..].chars().next().map_or(1, char::len_utf8);
        Tok::Plain
      }
    };

    if tok != Tok::Plain {
      out.push((start as u32, i as u32, tok));
    }
  }

  out
}

/// The bytes of `a` and of `b` between what they start and end with alike, unless they share
/// nothing.
fn changed(a: &str, b: &str) -> Option<((u32, u32), (u32, u32))> {
  let pre = a
    .char_indices()
    .zip(b.chars())
    .find(|&((_, x), y)| x != y)
    .map_or(a.len().min(b.len()), |((i, _), _)| i);
  let suf = a[pre..].chars().rev().zip(b[pre..].chars().rev()).take_while(|(x, y)| x == y);
  let suf: usize = suf.map(|(x, _)| x.len_utf8()).sum();
  let (ea, eb) = (a.len() - suf, b.len() - suf);
  let shared = pre + suf > 0 && (ea > pre || eb > pre);
  shared.then_some(((pre as u32, ea as u32), (pre as u32, eb as u32)))
}

impl Diff {
  /// A diff from its text: `--- A|M|D path` starts a file, `@@ -a,b +c,d @@` a hunk, then its
  /// lines after ` `, `+` or `-`.
  pub fn parse(src: &str) -> Self {
    let mut d = Diff::default();
    let (mut old, mut new) = (0, 0);
    for l in src.lines() {
      if let Some(rest) = l.strip_prefix("--- ") {
        d.file(rest);
        continue;
      }

      let file = d.files.len() - 1;
      let (kind, o, n, text) = match l.as_bytes().first() {
        _ if l.starts_with("@@ -") => {
          let num = |s: Option<&str>| {
            let s = s.unwrap_or_default().trim_start_matches(['-', '+']);
            s.split(',').next().and_then(|n| n.parse().ok()).unwrap_or(1)
          };
          let mut at = l.split(' ').skip(1);
          (old, new) = (num(at.next()), num(at.next()));
          (LineKind::Hunk, 0, 0, l)
        }

        Some(b'+') => {
          (new, d.files[file].added) = (new + 1, d.files[file].added + 1);
          (LineKind::Add, 0, new - 1, &l[1..])
        }

        Some(b'-') => {
          (old, d.files[file].removed) = (old + 1, d.files[file].removed + 1);
          (LineKind::Del, old - 1, 0, &l[1..])
        }

        _ => {
          (old, new) = (old + 1, new + 1);
          (LineKind::Same, old - 1, new - 1, l.get(1..).unwrap_or_default())
        }
      };

      let toks = if kind == LineKind::Hunk { Vec::new() } else { tokens(text) };
      d.lines.push(Line { kind, file, old: o, new: n, text: text.into(), toks, word: None });
    }

    d.starts.push(d.lines.len());
    d.pair();
    d
  }

  /// Starts a file from its header's `A|M|D path`.
  fn file(&mut self, header: &str) {
    let (st, path) = header.split_once(' ').unwrap_or(("M", header));
    let status = match st {
      "A" => Status::Added,
      "D" => Status::Deleted,
      _ => Status::Modified,
    };

    self.starts.push(self.lines.len());
    self.files.push(FileDiff { path: path.into(), status, added: 0, removed: 0 });
    let (kind, file, text) = (LineKind::File, self.files.len() - 1, path.into());
    self.lines.push(Line { kind, file, old: 0, new: 0, text, toks: Vec::new(), word: None });
  }

  /// Marks the words changed between each run of removed lines and the added ones after it,
  /// line by line.
  fn pair(&mut self) {
    let mut i = 0;
    while i < self.lines.len() {
      let dels = self.lines[i..].iter().take_while(|l| l.kind == LineKind::Del).count();
      let adds = self.lines[i + dels..].iter().take_while(|l| l.kind == LineKind::Add).count();
      for k in 0..dels.min(adds) {
        let (a, b) = (i + k, i + dels + k);
        if let Some((wa, wb)) = changed(&self.lines[a].text, &self.lines[b].text) {
          (self.lines[a].word, self.lines[b].word) = (Some(wa), Some(wb));
        }
      }

      i += (dels + adds).max(1);
    }
  }
}

/// Line `l`'s code in the palette's colours, the words it changed on `word`'s ground.
fn code(l: &Line, p: &Palette, word: Color) -> AttributedString {
  let mut s = AttributedString::new();
  let (w0, w1) = l.word.unwrap_or((u32::MAX, u32::MAX));
  let mut piece = |a: u32, b: u32, tok: Tok| {
    let attrs = Attributes::new().color(p.code[tok as usize]);
    let attrs = if tok == Tok::Comment { attrs.italic() } else { attrs };
    for (x, y, on) in [(a, b.min(w0), false), (a.max(w0), b.min(w1), true), (a.max(w1), b, false)] {
      if x < y {
        let at = if on { attrs.background(word) } else { attrs };
        s.push(&l.text[x as usize..y as usize], at);
      }
    }
  };

  let mut at = 0;
  for &(a, b, tok) in &l.toks {
    piece(at, a, Tok::Plain);
    piece(a, b, tok);
    at = b;
  }

  piece(at, l.text.len() as u32, Tok::Plain);
  s
}

/// A generated table of 2,400 lines: the big diff.
fn big() -> String {
  let mut s = String::from(concat!(
    "--- A src/syntax/width.rs\n@@ -0,0 +1,2407 @@\n",
    "+// Generated by `cargo xtask tables` from Unicode 16.0: do not edit.\n+\n",
    "+use super::Width;\n+\n",
    "+/// Code points by the columns they take, sorted, none overlapping.\n",
    "+pub static WIDTHS: &[(u32, u32, Width)] = &[\n",
  ));
  let (mut rng, mut lo) = (Rng(7), 0x1100u32);
  for _ in 0..2400 {
    let hi = lo + rng.below(48) as u32;
    let w = ["Wide", "Zero", "Ambiguous"][rng.below(3)];
    let _ = writeln!(s, "+    (0x{lo:05X}, 0x{hi:05X}, Width::{w}),");
    lo = hi + 1 + rng.below(24) as u32;
  }

  s.push_str("+];\n");
  s.push_str(concat!(
    "--- M src/syntax/mod.rs\n@@ -3,6 +3,7 @@\n mod highlight;\n mod scope;\n",
    "+mod width;\n \n pub use highlight::{Highlighter, Span};\n pub use scope::Scope;\n",
  ));
  s
}

/// The diffs the commits show: the big one first, then the root's, then the rest.
pub fn diffs() -> Vec<Diff> {
  let mut all = vec![Diff::parse(&big())];
  all.extend(DIFFS.iter().map(|d| Diff::parse(d)));
  all
}

const DIFFS: [&str; 6] = [
  r#"--- A Cargo.toml
@@ -0,0 +1,9 @@
+[package]
+name = "quill"
+version = "0.1.0"
+edition = "2024"
+
+[dependencies]
+ropey = "1.6"
+unicode-segmentation = "1.12"
+
--- A src/main.rs
@@ -0,0 +1,6 @@
+mod buffer;
+
+fn main() {
+    let doc = buffer::Document::open(std::env::args().nth(1));
+    println!("{} lines", doc.lines().len());
+}
"#,
  r#"--- M src/buffer/lines.rs
@@ -18,14 +18,17 @@ use crate::text::Rope;
 /// Where each line starts, kept in step with the rope.
 pub struct LineIndex {
-    starts: Vec<usize>,
+    starts: GapBuffer<usize>,
+    /// Lines past `dirty` are stale until the next query.
+    dirty: usize,
 }

 impl LineIndex {
     pub fn new(text: &Rope) -> Self {
-        let mut starts = vec![0];
+        let mut starts = GapBuffer::with_capacity(text.len() / 32);
+        starts.push(0);
         for (i, b) in text.bytes().enumerate() {
             if b == b'\n' {
                 starts.push(i + 1);
             }
         }
-        Self { starts }
+        Self { starts, dirty: usize::MAX }
     }
@@ -52,9 +55,13 @@ impl LineIndex {
     /// The line holding byte `at`.
     pub fn line_of(&self, at: usize) -> usize {
-        match self.starts.binary_search(&at) {
-            Ok(i) => i,
-            Err(i) => i - 1,
-        }
+        debug_assert!(self.dirty == usize::MAX, "query before reindex");
+        self.starts.partition_point(|&s| s <= at).saturating_sub(1)
     }
+
+    pub fn len(&self) -> usize {
+        self.starts.len()
+    }
 }
--- M src/buffer/mod.rs
@@ -7,7 +7,7 @@ mod lines;
 mod undo;

-pub use lines::LineIndex;
+pub use lines::{GapBuffer, LineIndex};
 pub use undo::{Edit, History};

 use crate::text::Rope;
"#,
  r#"--- M src/render/shape.rs
@@ -31,18 +31,24 @@ pub struct Shaper {
 impl Shaper {
     /// The glyphs of `line` in `font`, shaped once per text and size.
-    pub fn shape(&mut self, line: &str, font: &Font) -> Vec<Glyph> {
-        let face = self.faces.get(font.id).expect("a loaded face");
-        let mut buf = UnicodeBuffer::new();
-        buf.push_str(line);
-        rustybuzz::shape(face, &[], buf).glyph_infos().iter().map(Glyph::from).collect()
+    pub fn shape(&mut self, line: &str, font: &Font) -> Arc<[Glyph]> {
+        let key = RunKey { text: hash(line), font: font.id, size: font.size.to_bits() };
+        if let Some(run) = self.cache.get(&key) {
+            return Arc::clone(run);
+        }
+
+        let face = self.faces.get(font.id).expect("a loaded face");
+        let mut buf = UnicodeBuffer::new();
+        buf.push_str(line);
+        let run: Arc<[Glyph]> = rustybuzz::shape(face, &[], buf).glyph_infos().iter().map(Glyph::from).collect();
+        self.cache.insert(key, Arc::clone(&run));
+        run
     }

-    /// Drops nothing: every run is shaped again.
-    pub fn trim(&mut self) {}
+    /// Keeps the runs of the last 4 frames.
+    pub fn trim(&mut self, frame: u64) {
+        self.cache.retain(|_, run| run.frame + 4 >= frame);
+    }
 }
--- A src/render/cache.rs
@@ -0,0 +1,12 @@
+use std::collections::HashMap;
+
+/// What a shaped run is known by: its text's hash, its font and its size.
+#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
+pub struct RunKey {
+    pub text: u64,
+    pub font: u32,
+    pub size: u32,
+}
+
+pub type RunCache<T> = HashMap<RunKey, T>;
+
"#,
  r#"--- M src/lsp/client.rs
@@ -88,16 +88,25 @@ impl Client {
     /// Starts the server and waits for its answer to `initialize`.
     pub async fn start(cmd: &Command, root: &Path) -> Result<Self, Error> {
-        let mut child = cmd.spawn()?;
-        let io = Transport::new(child.stdin.take(), child.stdout.take());
-        let caps = io.request::<Initialize>(params(root)).await?;
-        Ok(Self { child, io, caps })
+        let mut delay = Duration::from_millis(50);
+        for attempt in 1..=5 {
+            let mut child = cmd.spawn()?;
+            let io = Transport::new(child.stdin.take(), child.stdout.take());
+            match timeout(Duration::from_secs(2), io.request::<Initialize>(params(root))).await {
+                Ok(Ok(caps)) => return Ok(Self { child, io, caps }),
+                Ok(Err(e)) if attempt == 5 => return Err(e),
+                _ => log::warn!("lsp: no answer to initialize, try {attempt} of 5"),
+            }
+            let _ = child.kill().await;
+            sleep(delay).await;
+            delay *= 2;
+        }
+        Err(Error::Timeout)
     }

     pub fn capabilities(&self) -> &ServerCapabilities {
         &self.caps
     }
--- M tests/lsp.rs
@@ -12,6 +12,15 @@ async fn hover_answers() {
     assert_eq!(hover.contents, "fn main()");
 }

+#[tokio::test]
+async fn a_slow_server_is_retried() {
+    let server = FakeServer::new().answer_after(Duration::from_secs(3)).times(2);
+    let client = Client::start(&server.command(), Path::new(".")).await;
+    assert!(client.is_ok());
+    assert_eq!(server.spawned(), 3);
+}
+
 #[tokio::test]
 async fn shutdown_is_clean() {
"#,
  r#"--- D src/config/legacy.rs
@@ -1,11 +0,0 @@
-//! Keymaps of 0.2, read until 0.5.
-
-use super::Keymap;
-
-pub fn load(text: &str) -> Option<Keymap> {
-    let mut map = Keymap::default();
-    for line in text.lines().filter(|l| !l.starts_with('#')) {
-        let (keys, cmd) = line.split_once('=')?;
-        map.bind(keys.trim(), cmd.trim());
-    }
-    Some(map)
-}
--- M src/config/mod.rs
@@ -1,5 +1,4 @@
 mod keymap;
-mod legacy;
 mod theme;

 pub use keymap::Keymap;
@@ -40,12 +39,9 @@ impl Config {
     pub fn load(dir: &Path) -> Result<Self, Error> {
         let text = fs::read_to_string(dir.join("keymap.toml"));
-        let keymap = match text {
-            Ok(t) => toml::from_str(&t)?,
-            Err(_) => legacy::load(&fs::read_to_string(dir.join("keys"))?).unwrap_or_default(),
-        };
+        let keymap = toml::from_str(&text?)?;
         Ok(Self { keymap, theme: Theme::system() })
     }
 }
"#,
  r#"--- M src/search/matcher.rs
@@ -14,15 +14,19 @@ pub struct Matcher {
 impl Matcher {
     pub fn new(query: &str, case: Case) -> Self {
-        let needle = match case {
-            Case::Sensitive => query.to_string(),
-            Case::Insensitive => query.to_lowercase(),
-        };
+        let needle = match case {
+            Case::Sensitive => query.to_string(),
+            Case::Insensitive => fold(query),
+        };
         Self { needle, case }
     }

     /// Where the needle starts in `line`, each match once.
     pub fn find_all<'a>(&'a self, line: &'a str) -> impl Iterator<Item = usize> + 'a {
-        let hay = if self.case == Case::Insensitive { line.to_lowercase() } else { line.into() };
+        let hay = if self.case == Case::Insensitive { fold(line) } else { line.into() };
         hay.match_indices(&self.needle).map(|(i, _)| i).collect::<Vec<_>>().into_iter()
     }
 }
+
+/// Unicode's simple case folding: `ß` stays, `Σ` and `ς` fold alike.
+fn fold(s: &str) -> String {
+    s.chars().flat_map(char::to_lowercase).map(|c| if c == 'ς' { 'σ' } else { c }).collect()
+}
"#,
];

// MARK: The app

pub const TABS: &str = "tabs";
pub const BRANCH_BUTTON: &str = "branch";
pub const SEARCH: &str = "search";
pub const FETCH_BUTTON: &str = "fetch";
pub const PULL_BUTTON: &str = "pull";
pub const PUSH_BUTTON: &str = "push";
pub const STATUS: &str = "status";
pub const SIDE: &str = "side";
pub const SIDEBAR: &str = "sidebar";
pub const SPLIT: &str = "split";
/// A section's header: `Id::of(SECTION).key(k)`; its rows `Id::of(REF).key((k, i))`.
pub const SECTION: &str = "section";
pub const REF: &str = "ref";
pub const COMMITS: &str = "commits";
pub const SUBJECT: &str = "subject";
/// A parent's link: `Id::of(PARENT).key(i)`.
pub const PARENT: &str = "parent";
pub const FILES: &str = "files";
pub const DIFF: &str = "diff";
pub const LINES: &str = "lines";

pub const REPOS: [Tag<usize>; 2] = [Tag::new(0, "quill"), Tag::new(1, "quill-plugins")];

/// The window's keys: Cmd-F (Ctrl-F) finds.
pub const KEYS: &[Binding] = &[Binding::new(Shortcut::char('f').cmd(), Command::Action("find"))];

pub struct Rill {
  pub repos: Vec<Repo>,
  pub repo: usize,
  pub diffs: Vec<Diff>,
  pub query: String,
  /// The commits the history shows: all, or those the search finds.
  pub shown: Vec<usize>,
  pub sel: Selection,
  pub files: Selection,
  /// The sidebar's sections open.
  pub open: [bool; 4],
  pub side: f32,
  pub split: f32,
  /// What the last action did.
  pub status: String,
  /// The repository and commit shown last, to start a new one at its top.
  viewing: Option<(usize, usize)>,
  /// A row to scroll into view in the next frame.
  reveal: Option<usize>,
  /// The commit selected before the search, and the one the search picked for it: cleared, the
  /// search goes back to the first unless another was picked meanwhile.
  before: Option<(usize, Option<usize>)>,
}

impl Default for Rill {
  fn default() -> Self {
    let diffs = diffs();
    let n = diffs.len();
    let repos = vec![
      Repo::generate(REPOS[0].label(), 11, 320, n),
      Repo::generate(REPOS[1].label(), 5, 140, n),
    ];
    let mut app = Self {
      repos,
      repo: 0,
      diffs,
      query: String::new(),
      shown: Vec::new(),
      sel: Selection::default(),
      files: Selection::default(),
      open: [true, true, true, false],
      side: 0.17,
      split: 0.5,
      status: String::new(),
      viewing: None,
      reveal: None,
      before: None,
    };
    app.filter();
    app.select(0);
    app
  }
}

impl Rill {
  pub fn repo(&self) -> &Repo {
    &self.repos[self.repo]
  }

  /// The commit selected, by its place in the history.
  pub fn selected(&self) -> Option<usize> {
    self.sel.keys().next().map(|k| k as usize)
  }

  /// The file of the commit's diff selected.
  pub fn file(&self) -> Option<usize> {
    self.files.keys().next().map(|k| k as usize)
  }

  /// Selects commit `c` and scrolls its row into view, clearing a search that hides it.
  pub fn select(&mut self, c: usize) {
    if !self.shown.contains(&c) {
      (self.query, self.before) = (String::new(), None);
      self.filter();
    }

    let row = self.shown.iter().position(|&s| s == c).unwrap_or(0);
    self.sel.select(c as u64, row);
    self.reveal = Some(row);
  }

  /// Filters the history by the search's text (or shows all of it again): a commit the filter
  /// hides gives its place to the first found, none if nothing is; cleared, the commit selected
  /// before the search comes back, unless one was picked while searching.
  pub fn search(&mut self) {
    let searching = !self.query.trim().is_empty();
    let before = *self.before.get_or_insert((self.selected().unwrap_or(0), None));
    self.filter();
    if !searching {
      self.before = None;
      let back = before.0.min(self.repo().commits.len() - 1);
      return match self.selected() {
        Some(c) if Some(c) != before.1 => self.select(c),
        _ => self.select(back),
      };
    }

    match (self.selected(), self.shown.first()) {
      (Some(c), _) if self.shown.contains(&c) => self.select(c),
      (_, Some(&first)) => {
        self.select(first);
        self.before = Some((before.0, Some(first)));
      }
      (_, None) => self.sel.clear(),
    }
  }

  /// The commits whose subject, author or hash has the search's text, any case.
  pub fn filter(&mut self) {
    let q = self.query.trim().to_lowercase();
    let r = &self.repos[self.repo];
    self.shown = (0..r.commits.len())
      .filter(|&i| {
        let c = &r.commits[i];
        q.is_empty()
          || c.subject.to_lowercase().contains(&q)
          || AUTHORS[c.author].0.to_lowercase().contains(&q)
          || c.hash.starts_with(&q)
      })
      .collect();
  }

  fn tabs(&mut self, ui: &mut Ui, p: Palette) {
    ui.row(Style::new().w(grow()).h(px(30.0)).bg(p.chrome), |ui| {
      let was = self.repo;
      ui.id(TABS).a11y_label("Repositories").picker(&mut self.repo, &REPOS);
      if self.repo != was {
        (self.query, self.viewing, self.before) = (String::new(), None, None);
        self.filter();
        self.select(0);
      }
    });
    rule(ui, p.edge, Axis::Horizontal);
  }

  fn toolbar(&mut self, ui: &mut Ui, p: Palette) {
    let s = Style::new().w(grow()).h(px(38.0)).pad_xy(10.0, 0.0).gap(4.0);
    ui.row(s.align(Align::Start, Align::Center).bg(p.bar), |ui| {
      let ahead = self.repo().ahead();
      let head = self.repo().refs[0][0].commit;
      let branch = |ui: &mut Ui, s: TextStyle| {
        ui.icon(BRANCH, 14.0, s.get_color());
        ui.text("main", bold(12.0, s.get_color()));
        ui.text(format_args!("↑{ahead} ↓0"), s.color(p.muted));
      };
      if ui.help("Show the commit checked out").id(BRANCH_BUTTON).button(branch).clicked {
        self.select(head);
      }

      ui.spacer();
      ui.row(Style::new().w(grow()).max_w(420.0), |ui| {
        let r = ui
          .id(SEARCH)
          .a11y_label("Search")
          .text_field(&mut self.query, "Search commits, authors, hashes");
        if r.cancelled {
          self.query.clear();
        }

        if r.changed || r.cancelled {
          self.search();
        }
      });
      ui.spacer();
      ui.id(STATUS).text(&self.status, text(11.5, p.muted));
      ui.el(Style::new().w(px(6.0)));
      if ui.help("Fetch from origin").a11y_label("Fetch").id(FETCH_BUTTON).button(FETCH).clicked {
        self.status = "Fetched origin: nothing new".into();
      }

      if ui.help("Pull main from origin").a11y_label("Pull").id(PULL_BUTTON).button(PULL).clicked {
        self.status = "Already up to date".into();
      }

      ui.disabled(ahead == 0).help("Push main to origin").a11y_label("Push");
      let push = ui.id(PUSH_BUTTON).button(PUSH);
      if push.clicked {
        let repo = &mut self.repos[self.repo];
        repo.refs[1][0].commit = repo.refs[0][0].commit;
        self.status = format!("Pushed {ahead} commits to origin/main");
      }
    });
    rule(ui, p.edge, Axis::Horizontal);
  }

  fn sidebar(&mut self, ui: &mut Ui, p: Palette) {
    let mut go = None;
    let repo = &self.repos[self.repo];
    let open = &mut self.open;
    let s = Style::new().grow().bg(p.side).pad_xy(6.0, 6.0);

    ui.button_style(&SideRow).id(SIDEBAR).scroll_area(s, |ui| {
      for (k, refs) in repo.refs.iter().enumerate() {
        let n = refs.len();
        let label = |ui: &mut Ui, s: TextStyle| {
          ui.text(SECTIONS[k], s);
          ui.spacer();
          ui.text(n, s.color(p.faint));
        };
        ui.with_id(Id::of(SECTION).key(k)).toggle(&mut open[k], label);
        if !open[k] {
          continue;
        }

        for (i, r) in refs.iter().enumerate() {
          let current = k == 0 && i == 0;
          let row = |ui: &mut Ui, s: TextStyle| {
            ui.icon(SECTION_ICONS[k], 13.0, if current { p.accent } else { p.muted });
            let s = if current { bold(12.0, p.ink) } else { s };
            ui.row(Style::new().w(grow()).clip(), |ui| ui.text(&r.name, s));
            if current {
              ui.icon(Icon::CHECK, 12.0, p.accent);
            }
          };
          if ui.with_id(Id::of(REF).key((k, i))).button(row).clicked {
            go = Some(r.commit);
          }
        }

        ui.el(Style::new().h(px(4.0)));
      }
    });

    if let Some(c) = go {
      self.select(c);
    }
  }

  fn history(&mut self, ui: &mut Ui, p: Palette) {
    if let Some(row) = self.reveal.take() {
      ui.reveal_row(Id::of(COMMITS), row);
    }

    let Self { repos, repo, shown, sel, query, .. } = self;
    let (repo, shown) = (&repos[*repo], &*shown);
    // While filtered, the lanes don't join up: only the dots, in the same column.
    let joined = query.trim().is_empty();
    let graph_w = 8.0 + LANE * repo.lanes as f32;

    let key = |i: usize| shown[i] as u64;
    let list = List::new(shown.len(), ROW).key(&key).selection(sel);
    let list = list.style(Style::new().grow().bg(p.list));
    ui.col(Style::new().grow().bg(p.list), |ui| {
      list.show(ui.autofocus().id(COMMITS).a11y_label("History"), |ui, r| {
        let c = shown[r.index];
        let commit = &repo.commits[c];
        let g = repo.graph[c].clone();

        let colors = p.lanes;
        let s = Style::new().w(px(graph_w)).h(px(ROW));
        ui.canvas(s, move |f, rect| lanes(f, rect, &g, joined, colors));
        refs(ui, repo, c, p, r.selected);

        let quiet = p.quiet(r.selected);
        let ink = if commit.parents.len() > 1 { quiet } else { p.ink };
        ui.row(Style::new().w(grow()).clip(), |ui| ui.text(&commit.subject, text(12.0, ink)));
        ui.row(Style::new().w(px(118.0)).clip(), |ui| {
          ui.text(AUTHORS[commit.author].0, text(12.0, quiet))
        });
        ui.row(
          Style::new().w(px(84.0)).pads([0.0, 10.0, 0.0, 0.0]).align(Align::End, Align::Center),
          |ui| ui.text(when(commit.time, false), text(11.5, quiet)),
        );
      });
    });
  }

  fn detail(&mut self, ui: &mut Ui, p: Palette) {
    let Some(c) = self.selected() else {
      ui.col(Style::new().grow().center().bg(p.view), |ui| {
        let none =
          if self.query.trim().is_empty() { "No commit selected" } else { "No commits match" };
        ui.text(none, text(13.0, p.faint))
      });
      return;
    };

    if self.viewing != Some((self.repo, c)) {
      self.viewing = Some((self.repo, c));
      self.files.clear();
      ui.scroll_to(Id::of(DIFF), Point::new(0.0, 0.0));
    }

    let mut go = None;
    let Self { repos, repo, diffs, files, .. } = self;
    let repo = &repos[*repo];
    let commit = &repo.commits[c];
    let diff = &diffs[commit.diff];

    ui.col(Style::new().grow().bg(p.view), |ui| {
      ui.col(Style::new().w(grow()).pad_xy(14.0, 10.0).gap(5.0), |ui| {
        let s = TextStyle::new(Font::ui(14.0).weight(Weight::Semibold), p.ink);
        ui.id(SUBJECT).text(&commit.subject, s);
        ui.el(Style::new().h(px(2.0)));

        let (name, mail) = AUTHORS[commit.author];
        meta(ui, "Author", p, |ui| {
          ui.text(name, text(12.0, p.ink));
          ui.text(format_args!("<{mail}>"), text(12.0, p.muted));
        });
        meta(ui, "Date", p, |ui| ui.text(when(commit.time, true), text(12.0, p.ink)));
        meta(ui, "Commit", p, |ui| ui.text(&commit.hash, mono(11.5, p.muted)));
        meta(ui, "Parents", p, |ui| {
          ui.button_style(&HashLink).row(Style::new().gap(8.0), |ui| {
            for (k, &parent) in commit.parents.iter().enumerate() {
              let short = &repo.commits[parent].hash[..10];
              if ui.with_id(Id::of(PARENT).key(k)).button(short).clicked {
                go = Some(parent);
              }
            }
          });
        });
      });

      rule(ui, p.edge, Axis::Horizontal);
      let (added, removed) =
        diff.files.iter().fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
      let s = Style::new().w(grow()).h(px(28.0)).pad_xy(14.0, 0.0).gap(8.0);
      ui.row(s.align(Align::Start, Align::Center), |ui| {
        let n = diff.files.len();
        ui.text(format_args!("{n} {}", if n == 1 { "file" } else { "files" }), bold(11.5, p.muted));
        ui.text(format_args!("+{added}"), mono(11.5, p.lanes[2]));
        ui.text(format_args!("−{removed}"), mono(11.5, p.lanes[4]));
        ui.spacer();
        if !files.is_empty() {
          ui.text("Escape shows all", text(11.0, p.faint));
        }
      });

      let n = diff.files.len();
      let list = List::new(n, ROW).selection(files);
      let list =
        list.style(Style::new().w(grow()).h(px(ROW * n.min(6) as f32 + 4.0)).pad_xy(0.0, 2.0));
      list.show(ui.id(FILES).a11y_label("Files"), |ui, r| {
        file_row(ui, &diff.files[r.index], p, r.selected)
      });
      rule(ui, p.edge, Axis::Horizontal);

      let file = files.keys().next().map(|f| f as usize);
      let lines = match file {
        Some(f) => &diff.lines[diff.starts[f]..diff.starts[f + 1]],
        None => &diff.lines[..],
      };

      ui.id(DIFF).scroll_area(Style::new().grow().bg(p.view), |ui| {
        let key = |i: usize| (c, file, i);
        ui.id(LINES)
          .lazy_col(Style::new(), lines.len(), key, |ui, i| line(ui, &lines[i], i == 0, diff, p));
      });
    });

    if let Some(c) = go {
      self.select(c);
    }
  }
}

/// A row of the commit's facts: a label in a column, the value after it.
fn meta(ui: &mut Ui, label: &str, p: Palette, value: impl FnOnce(&mut Ui)) {
  ui.row(Style::new().w(grow()).gap(8.0).align(Align::Start, Align::Center), |ui| {
    ui.row(Style::new().w(px(52.0)), |ui| ui.text(label, text(11.5, p.faint)));
    ui.row(Style::new().gap(6.0).align(Align::Start, Align::Center), value);
  });
}

/// The commit's names as small labels before its message: branches in their lane's colour,
/// remotes outlined, tags in the tag's yellow.
fn refs(ui: &mut Ui, repo: &Repo, c: usize, p: Palette, selected: bool) {
  let lane = p.lanes[repo.graph[c].color as usize];
  for (k, sec) in repo.refs.iter().enumerate().take(3) {
    for r in sec.iter().filter(|r| r.commit == c) {
      let (fill, edge, ink) = match k {
        0 => (alpha(lane, 0.28), alpha(lane, 0.7), p.ink),
        1 => ([0.0; 4], alpha(p.quiet(selected), 0.7), p.quiet(selected)),
        _ => (alpha(p.lanes[6], 0.22), alpha(p.lanes[6], 0.6), p.ink),
      };

      let s = Style::new().h(px(16.0)).pad_xy(5.0, 0.0).gap(3.0).center().radius(3.0);
      ui.row(s.bg(fill).border(1.0, edge), |ui| {
        if k == 2 {
          ui.icon(TAG, 10.0, ink);
        }

        ui.text(&r.name, text(10.5, ink));
      });
    }
  }
}

/// The graph's part of a row: lines from the top to the middle and on down, curving between
/// lanes, unless they don't join up (`joined`), and the commit's dot over them.
fn lanes(f: &mut Frame, r: Rect, g: &GraphRow, joined: bool, colors: [Color; 7]) {
  let x = |j: u8| r.x + 4.0 + LANE * (j as f32 + 0.5);
  let mid = r.y + r.h / 2.0;
  let curve = |f: &mut Frame, (a, b, c): (u8, u8, u8), y0: f32, y1: f32| {
    let color = colors[c as usize];
    if a == b {
      return f.line(Point::new(x(a), y0), Point::new(x(a), y1), 1.6, color);
    }

    let pts: Vec<Point> = (0..=8)
      .map(|k| {
        let t = k as f32 / 8.0;
        let s = t * t * (3.0 - 2.0 * t);
        Point::new(x(a) + (x(b) - x(a)) * s, y0 + (y1 - y0) * t)
      })
      .collect();
    f.polyline(&pts, 1.6, color);
  };
  for &s in g.top.iter().filter(|_| joined) {
    curve(f, s, r.y, mid);
  }

  for &s in g.bottom.iter().filter(|_| joined) {
    curve(f, s, mid, r.y + r.h);
  }

  let (cx, color) = (x(g.lane), colors[g.color as usize]);
  f.rect(Rect::new(cx - 4.5, mid - 4.5, 9.0, 9.0), color, 4.5);
}

/// A file of the commit: its status in a letter of its colour, its folder faint before its
/// name, the lines it added and removed.
fn file_row(ui: &mut Ui, f: &FileDiff, p: Palette, selected: bool) {
  let (letter, color) = match f.status {
    Status::Added => ("A", p.lanes[2]),
    Status::Modified => ("M", p.lanes[1]),
    Status::Deleted => ("D", p.lanes[4]),
  };

  ui.el(Style::new().w(px(8.0)));
  ui.row(Style::new().w(px(16.0)).h(px(16.0)).center().radius(3.0).bg(alpha(color, 0.2)), |ui| {
    ui.text(letter, bold(10.5, color))
  });
  let (dir, name) = f.path.rsplit_once('/').map_or(("", f.path.as_str()), |(d, n)| (d, n));
  ui.row(Style::new().w(grow()).clip().gap(0.0), |ui| {
    if !dir.is_empty() {
      ui.text(format_args!("{dir}/"), text(12.0, p.quiet(selected)));
    }

    ui.text(name, text(12.0, p.ink));
  });
  // On the selection's fill the counts take the ink: red and green on blue don't read.
  let (plus, minus) = if selected { (p.ink, p.ink) } else { (p.lanes[2], p.lanes[4]) };
  ui.text(format_args!("+{}", f.added), mono(11.0, plus));
  ui.row(
    Style::new().w(px(48.0)).pads([0.0, 12.0, 0.0, 0.0]).align(Align::End, Align::Center),
    |ui| ui.text(format_args!("−{}", f.removed), mono(11.0, minus)),
  );
}

/// A row of the diff: a file's header (apart from the file above, unless `first`), a hunk's, or
/// a line with its numbers, tinted as added or removed.
fn line(ui: &mut Ui, l: &Line, first: bool, d: &Diff, p: Palette) {
  let row = Style::new().w(grow()).align(Align::Start, Align::Center).clip();
  match l.kind {
    LineKind::File => {
      let f = &d.files[l.file];
      if !first {
        ui.el(Style::new().h(px(10.0)));
      }

      let s = row.h(px(28.0)).pad_xy(10.0, 0.0).gap(8.0).bg(p.raised);
      ui.row(s.borders([1.0, 0.0, 1.0, 0.0], p.edge), |ui| {
        ui.icon(Icon::DOC, 13.0, p.muted);
        ui.text(&f.path, bold(12.0, p.ink));
        ui.spacer();
        ui.text(format_args!("+{}", f.added), mono(11.0, p.lanes[2]));
        ui.text(format_args!("−{}", f.removed), mono(11.0, p.lanes[4]));
      });
    }

    LineKind::Hunk => {
      let s = row.h(px(22.0)).pads([0.0, 10.0, 0.0, 92.0]).bg(p.hunk);
      ui.row(s, |ui| ui.text(&l.text, mono(11.5, p.hunk_ink)));
    }

    kind => {
      let (bg, word, mark) = match kind {
        LineKind::Add => (p.added, p.added_word, "+"),
        LineKind::Del => (p.removed, p.removed_word, "-"),
        _ => (p.view, p.view, ""),
      };

      let gutter =
        if kind == LineKind::Same { mix(p.view, p.list, 0.6) } else { mix(bg, word, 0.45) };
      let num = |ui: &mut Ui, n: u32| {
        let s = Style::new().w(px(38.0)).h(grow()).pads([0.0, 6.0, 0.0, 0.0]).bg(gutter);
        ui.row(s.align(Align::End, Align::Center), |ui| {
          if n > 0 {
            ui.text(n, mono(11.0, p.faint));
          }
        });
      };
      ui.row(row.h(px(LINE)).bg(bg), |ui| {
        num(ui, l.old);
        num(ui, l.new);
        ui.row(Style::new().w(px(16.0)).center(), |ui| ui.text(mark, mono(12.0, p.muted)));
        ui.text(&code(l, &p, word), mono(12.0, p.ink));
      });
    }
  }
}

impl App for Rill {
  fn keymap(&self) -> &[Binding] {
    KEYS
  }

  fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
    match ev {
      Event::Command(Command::Action("find")) => cx.focus(Some(Id::of(SEARCH))),
      _ => return false,
    }

    true
  }

  fn view(&mut self, ui: &mut Ui) {
    let p = ui.environment_value::<Palette>();
    ui.help_style(&Slip);
    ui.set_focus_ring(Some(FocusRing::new(alpha(p.accent, 0.7)).width(1.5).gap(0.0)));
    ui.set_environment(Some(ScrollerKnob::LIGHT.color(alpha(p.ink, 0.28))));

    // The look, window-wide: on the root, so every control built in it reads it.
    let ui = ui.button_style(&Tool).toggle_style(&Disclosure).picker_style(&Tabs);
    let ui = ui.text_field_style(&Find).list_style(&Rows);
    ui.col(Style::new().grow().bg(p.view), |ui| {
      self.tabs(ui, p);
      self.toolbar(ui, p);
      let (mut side, mut mid) = (self.side, self.split);
      split(ui, Id::of(SIDE), &mut side, [150.0, 560.0], |ui, i| match i {
        0 => self.sidebar(ui, p),
        _ => split(ui, Id::of(SPLIT), &mut mid, [300.0, 300.0], |ui, i| match i {
          0 => self.history(ui, p),
          _ => self.detail(ui, p),
        }),
      });
      (self.side, self.split) = (side, mid);
    });
  }
}

fn main() {
  // The state the shot shows: the newest commit but one, the pointer resting on a row.
  let mut args: Vec<String> = std::env::args().skip(1).collect();
  let state = match args.first().map(String::as_str) {
    Some(s @ ("big" | "file" | "search")) => s.to_owned(),
    _ => String::new(),
  };

  if !state.is_empty() {
    args.remove(0);
  }

  example::run(Config::new("Rill", 1240.0, 760.0), Rill::default(), &args, |h| {
    h.frame();
    match state.as_str() {
      "big" => {
        h.app_mut().select(3);
        h.frame();

        let r = h.rect(Id::of(DIFF)).unwrap();
        h.move_to(r.x + 200.0, r.y + 200.0);
        for _ in 0..20 {
          h.event(Event::Scroll {
            pos: Point::new(r.x + 200.0, r.y + 200.0),
            delta: Point::new(0.0, -120.0),
            precise: true,
            phase: Phase::None,
            momentum: Phase::None,
            mods: Modifiers::empty(),
          });
          h.frame();
        }
      }
      "file" => {
        let r = h.rect(Id::of(FILES).key(("row", 1u64))).unwrap();
        h.click(r.x + 60.0, r.y + 10.0);
      }
      "search" => {
        h.click_id(Id::of(SEARCH));
        h.type_text("merge");
      }
      _ => {}
    }

    let r = h.rect(Id::of(COMMITS)).unwrap();
    h.move_to(r.x + 160.0, r.y + ROW * 4.5);
    h.wait(Duration::from_millis(300));
  });
}
