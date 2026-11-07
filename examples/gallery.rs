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

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use nimbo::{
  Align, Answer, App, Axes, Axis, Button, ButtonRole, ButtonStyle, Config, ControlSize, Dialog,
  Float, Font, FormRow, FormStyle, Hotkey, Icon, Item, LinePattern, List, ListStyle, Menu, Picker,
  Section, Selection, Shortcut, Split, Style, Tabs, Tag, TextStyle, Tone, TruncationMode, Ui,
  UnitPoint, grow, px,
};

pub const SIDEBAR: &str = "sidebar";
pub const PAGE: &str = "page";
pub const DIALOG: &str = "dialog";
/// A block alone ([Gallery::only]).
pub const BLOCK: &str = "block";
/// The width of a page, and of a block's picture.
pub const WIDTH: f32 = 520.0;

/// A building block: its group, name, SwiftUI's line and nimbo's.
pub struct Block {
  pub group: &'static str,
  pub name: &'static str,
  pub swiftui: &'static str,
  pub nimbo: &'static str,
}

impl Block {
  /// Its name in lowercase with dashes: its picture's file name and its argument.
  pub fn slug(&self) -> String {
    let s: String = self.name.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    s.to_lowercase().split_whitespace().collect::<Vec<_>>().join("-")
  }
}

const fn b(
  group: &'static str,
  name: &'static str,
  swiftui: &'static str,
  nimbo: &'static str,
) -> Block {
  Block { group, name, swiftui, nimbo }
}

pub const GROUPS: [&str; 8] =
  ["Layout", "Text", "Controls", "Form", "List", "Containers", "Presentation", "Modifiers"];

#[rustfmt::skip]
pub const BLOCKS: &[Block] = &[
  b("Layout", "Stacks", "HStack(spacing: 8) { … }; VStack { … }", "ui.row(Style::new().gap(8.0), |ui| { … }); ui.col(style, |ui| { … })"),
  b("Layout", "Grid", "Grid { GridRow { … } }", "ui.grid(3, Style::new().gap(8.0), |ui| { … })"),
  b("Layout", "Spacer", "Spacer()", "ui.spacer()"),
  b("Layout", "Divider", "Divider()", "ui.divider()"),
  b("Layout", "Scroll view", "ScrollView(.vertical) { … }", "ui.scroll_area(Style::new().h(px(80.0)).scroll(Axes::VERTICAL), |ui| { … })"),
  b("Layout", "Overlay", ".overlay(alignment: .topTrailing) { … }", "ui.float(Float::parent().attach(UnitPoint::TOP_TRAILING).anchor(UnitPoint::TOP_TRAILING)).el(style)"),
  b("Text", "Text", "Text(\"Title\").font(.headline).foregroundStyle(.secondary)", "ui.text(\"Title\", TextStyle::new(t.bold, t.muted))"),
  b("Text", "Line limit", "Text(path).lineLimit(1).truncationMode(.middle)", "ui.text(path, t.body().line_limit(1).truncation(TruncationMode::Middle))"),
  b("Text", "Underline", "Text(\"WER\").underline(pattern: .dot)", "ui.text(\"WER\", s.underline(LinePattern::Dot)); s.strikethrough(LinePattern::Solid)"),
  b("Text", "Selectable text", "Text(log).textSelection(.enabled)", "ui.selectable(&log)"),
  b("Text", "Link", "Link(\"Learn more\", destination: url)", "if ui.link(\"Learn more…\").clicked { nimbo::open::url(URL)? }"),
  b("Text", "Image", "Image(systemName: \"gear\")", "ui.icon(Icon::GEAR, 20.0, t.text)"),
  b("Controls", "Button", "Button(\"Save\") { … }.buttonStyle(.borderedProminent)", "Button::new(\"Save\").style(ButtonStyle::BorderedProminent).show(ui)"),
  b("Controls", "Button role", "Button(\"Delete\", role: .destructive) { … }", "Button::new(\"Delete\").role(ButtonRole::Destructive).show(ui)"),
  b("Controls", "Icon button", "Button { Image(systemName: \"trash\") }.buttonStyle(.borderless)", "ui.icon_button(Icon::TRASH)"),
  b("Controls", "Switch", "Toggle(\"Live\", isOn: $live)", "ui.toggle(&mut self.live, \"Live\")"),
  b("Controls", "Check box", "Toggle(\"Wrap\", isOn: $wrap).toggleStyle(.checkbox)", "ui.checkbox(&mut self.wrap, \"Wrap\")"),
  b("Controls", "Slider", "Slider(value: $zoom, in: 0...200)", "ui.slider(&mut self.zoom, 0.0..=200.0)"),
  b("Controls", "Picker", "Picker(\"Size\", selection: $size) { …; Divider(); … }", "Picker::new(&mut self.size).options(&A).divider().options(&B).show(ui)"),
  b("Controls", "Segmented picker", "Picker(…) { … }.pickerStyle(.segmented)", "ui.segmented(&mut self.view, &[Tag::new(0, \"List\").count(128), Tag::new(1, \"Grid\")])"),
  b("Controls", "Text field", "TextField(\"Name\", text: $name)", "ui.field(&mut self.name, \"Name\")"),
  b("Controls", "Secure field", "SecureField(\"Password\", text: $key)", "ui.password_field(&mut self.key, \"Password\")"),
  b("Controls", "Search field", ".searchable(text: $query)", "ui.search(&mut self.query, \"Search\")"),
  b("Controls", "Progress", "ProgressView(value: 0.42)", "ui.progress(Some(0.42))"),
  b("Controls", "Spinner", "ProgressView()", "ui.spinner()"),
  b("Controls", "Badge", ".badge(\"New\")", "ui.badge(\"New\", Tone::Accent)"),
  b("Controls", "Shortcut recorder", "KeyboardShortcut (SwiftUI has no recorder)", "ui.shortcut(&mut self.hotkey)"),
  b("Controls", "Help marks", "Image(systemName: \"info.circle\").help(tip); HelpLink { … }", "ui.help(tip).info_button(); ui.help(why).warning_button(); ui.help_button()"),
  b("Form", "Section", "Section { … } header: { … } footer: { … }", "ui.form_section(&fs, Section::new(header).footer(\"…\"), |ui| { … })"),
  b("Form", "Form row", "LabeledContent { Toggle(…) } label: { Text(title); Text(subtitle) }", "ui.form_row(&fs, FormRow::new(\"Open at login\").caption(\"…\"), |ui| ui.toggle(&mut on, \"\"))"),
  b("Form", "Composed label", "label: { Text(name); Text(\"New\").badge(…) }", "FormRow::new(|ui: &mut Ui, s: TextStyle| { ui.text(name, s); ui.control_size(ControlSize::Small).badge(\"New\", Tone::Accent); })"),
  b("Form", "Row tile", "Label { Text(title) } icon: { Image(systemName: \"folder\") }", "FormRow::new(\"Data\").tile(Icon::FOLDER)"),
  b("Form", "Disclosure", "DisclosureGroup(\"Other models\") { … }", "ui.form_disclosure(&fs, FormRow::new(\"Other models\"), |ui| { … })"),
  b("Form", "Footer", "HelpLink { … } at a pane's end", "ui.help(\"Help\").form_footer(&fs, |ui| _ = ui.button(\"Reset All…\"))"),
  b("List", "List", "List(items, selection: $sel) { … }", "List::new(n, 24.0).selection(&mut sel).show(ui, |ui, row| { … })"),
  b("List", "List sections", "Section(header:) in a List; .listStyle(…)", "List::new(n, 32.0).sections(&starts, 24.0, &head).list_style(ListStyle::Grouped)"),
  b("Containers", "Tabs", "TabView { … }", "Tabs::new(&mut self.tab, &[\"One\", \"Two\"]).show(ui, |ui, i| { … })"),
  b("Containers", "Split", "HSplitView { … }", "Split::new(Axis::Horizontal).ratio(&mut self.side).show(ui, |ui, pane| { … })"),
  b("Containers", "Menu", "Menu(\"Actions\") { Button(…) }", "ui.menu(\"Actions\", |ui| { ui.menu_item(\"Copy\"); })"),
  b("Containers", "Lazy column", "LazyVStack { ForEach(…) }", "ui.virtual_col(heights, |i| i, |ui, i| { … })"),
  b("Presentation", "Alert", ".alert(\"Delete?\", isPresented: $ask) { Button(\"Delete\", role: .destructive) }", "Dialog::new(\"Delete?\").button(\"Delete\", ButtonRole::Destructive).show(ui, &mut self.ask)"),
  b("Presentation", "Context menu", ".contextMenu { Button(\"Copy\") { … } }", "ui.context_menu(r.id, || Menu::new(vec![Item::button(Action::Copy, \"Copy\")]))"),
  b("Presentation", "Tooltip", ".help(\"Saves it\")", "ui.help(\"Saves it\").button(\"Save\")"),
  b("Modifiers", "Disabled", ".disabled(true)", "ui.disabled(true).col(Style::new(), |ui| { … })"),
  b("Modifiers", "Control size", ".controlSize(.small)", "ui.control_size(ControlSize::Small).row(Style::new(), |ui| { … })"),
  b("Modifiers", "Accessibility label", ".accessibilityLabel(\"Reset\")", "ui.a11y_label(\"Reset\").icon_button(Icon::RESET)"),
  b("Modifiers", "Keyboard shortcut", ".keyboardShortcut(\"z\", modifiers: .command)", "ui.keymap(&[bind(Shortcut::char('z').cmd(), Command::Undo)]).col(…)"),
];

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

/// What the context menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
  Copy,
  Delete,
}

pub struct Gallery {
  /// The block shown, an index into [BLOCKS].
  pub page: usize,
  pub sel: Selection,
  /// One block alone, as high as its variants: a picture for `docs/api.md`.
  pub only: bool,
  pub live: bool,
  pub wrap: bool,
  pub zoom: f32,
  pub size: Size,
  pub view: u8,
  pub name: String,
  pub key: String,
  pub query: String,
  pub hotkey: Option<Hotkey>,
  pub tab: usize,
  pub side: f32,
  pub rows: Selection,
  pub ask: bool,
  pub deleted: usize,
  pub copied: usize,
  pub on: bool,
}

impl Default for Gallery {
  fn default() -> Self {
    Self {
      page: 0,
      sel: Selection::default(),
      only: false,
      live: true,
      wrap: false,
      zoom: 60.0,
      size: Size::Medium,
      view: 0,
      name: "Alex".into(),
      key: "secret".into(),
      query: "Switches".into(),
      hotkey: Hotkey::parse("ctrl+option+space").ok(),
      tab: 0,
      side: 0.4,
      rows: Selection::default(),
      ask: false,
      deleted: 0,
      copied: 0,
      on: true,
    }
  }
}

/// The variants a block shows: itself alone (layout and text, which nothing disables); as it is
/// and disabled; and at each size too.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shown {
  Alone,
  Plain,
  Sized,
}

/// A line of widgets, `gap` apart, centred on each other.
fn line(gap: f32) -> Style {
  Style::new().gap(gap).align(Align::Start, Align::Center)
}

impl Gallery {
  /// The gallery on `page`, alone when `only`.
  pub fn on(page: usize, only: bool) -> Self {
    Self { page, only, ..Self::default() }
  }

  /// Builds the block's variants with `f`: a labelled line each, keyed apart; disabled by
  /// [Ui::disabled] and sized by [Ui::control_size] on the line.
  fn variants(ui: &mut Ui, shown: Shown, mut f: impl FnMut(&mut Ui)) {
    let t = ui.theme();
    let sizes = [(ControlSize::Small, "Small"), (ControlSize::Large, "Large")];
    let sizes = if shown == Shown::Sized { &sizes[..] } else { &[] };
    if shown == Shown::Alone {
      ui.row(line(10.0).w(grow()), f);
      return;
    }

    ui.col(Style::new().w(grow()).gap(12.0), |ui| {
      let put = |ui: &mut Ui, k: &str, f: &mut dyn FnMut(&mut Ui)| {
        ui.key(k).row(line(12.0).w(grow()), |ui| {
          ui.col(Style::new().w(px(64.0)), |ui| ui.text(k, t.muted().nowrap()));
          ui.row(line(10.0).w(grow()), f);
        });
      };
      put(ui, "Enabled", &mut f);
      ui.disabled(true).col(Style::new().w(grow()), |ui| put(ui, "Disabled", &mut f));
      for &(size, k) in sizes {
        ui.control_size(size).col(Style::new().w(grow()), |ui| put(ui, k, &mut f));
      }
    });
  }

  /// The live block `name`.
  fn demo(&mut self, ui: &mut Ui, name: &str) {
    let t = ui.theme();
    let swatch = |c| Style::new().w(px(28.0)).h(px(28.0)).radius(6.0).bg(c);
    match name {
      "Stacks" => Self::variants(ui, Shown::Alone, |ui| {
        ui.row(Style::new().gap(8.0), |ui| (0..3).for_each(|_| _ = ui.el(swatch(t.accent))));
        ui.col(Style::new().gap(4.0), |ui| (0..2).for_each(|_| _ = ui.el(swatch(t.raised))));
      }),
      "Grid" => Self::variants(ui, Shown::Alone, |ui| {
        let grid = Style::new().w(px(120.0)).gap(6.0);
        ui.grid(3, grid, |ui| (0..6).for_each(|_| _ = ui.el(swatch(t.raised))));
      }),
      "Spacer" => Self::variants(ui, Shown::Alone, |ui| {
        ui.row(Style::new().w(grow()), |ui| {
          ui.text("Left", t.label());
          ui.spacer();
          ui.text("Right", t.label());
        });
      }),
      "Divider" => Self::variants(ui, Shown::Alone, |ui| {
        ui.row(line(8.0).h(px(24.0)), |ui| {
          ui.text("One", t.label());
          ui.divider();
          ui.text("Two", t.label());
        });
        ui.col(Style::new().w(px(120.0)).gap(4.0), |ui| {
          ui.text("Above", t.label());
          ui.divider();
          ui.text("Below", t.label());
        });
      }),
      "Scroll view" => Self::variants(ui, Shown::Alone, |ui| {
        let area = Style::new().w(px(200.0)).h(px(60.0)).scroll(Axes::VERTICAL);
        ui.scroll_area(area, |ui| {
          (1..=8).for_each(|i| ui.text(format_args!("Line {i}"), t.label()))
        });
      }),
      "Overlay" => Self::variants(ui, Shown::Alone, |ui| {
        ui.row(swatch(t.raised).w(px(80.0)).h(px(48.0)), |ui| {
          let at = UnitPoint::TOP_TRAILING;
          let dot = swatch(t.accent).w(px(12.0)).h(px(12.0));
          ui.float(Float::parent().attach(at).anchor(at).offset(-4.0, 4.0)).el(dot);
        });
      }),
      "Text" => Self::variants(ui, Shown::Alone, |ui| {
        ui.text("Title", TextStyle::new(t.bold, t.text));
        ui.text("Body", t.label());
        ui.text("Secondary", t.muted());
      }),
      "Line limit" => Self::variants(ui, Shown::Alone, |ui| {
        let path = "/Users/alex/Library/Application Support/Example/cache.db";
        ui.col(Style::new().w(px(300.0)).gap(4.0), |ui| {
          for mode in [TruncationMode::Head, TruncationMode::Middle, TruncationMode::Tail] {
            ui.text(path, t.body().truncation(mode));
          }
        });
      }),
      "Underline" => Self::variants(ui, Shown::Alone, |ui| {
        let patterns = [
          (LinePattern::Solid, "solid"),
          (LinePattern::Dot, "dot"),
          (LinePattern::Dash, "dash"),
          (LinePattern::DashDot, "dashDot"),
          (LinePattern::DashDotDot, "dashDotDot"),
        ];
        for (p, n) in patterns {
          ui.text(n, t.label().underline(p));
        }

        ui.text("struck", t.label().strikethrough(LinePattern::Solid));
      }),
      "Selectable text" => Self::variants(ui, Shown::Plain, |ui| {
        ui.col(Style::new().w(px(300.0)), |ui| _ = ui.selectable("Select this text and copy it."));
      }),
      "Link" => Self::variants(ui, Shown::Plain, |ui| _ = ui.link("Learn more…")),
      "Image" => Self::variants(ui, Shown::Alone, |ui| {
        for icon in [Icon::GEAR, Icon::FOLDER, Icon::TRASH, Icon::INFO] {
          ui.icon(icon.stroke(1.6), 20.0, t.text);
        }
      }),
      "Button" => Self::variants(ui, Shown::Sized, |ui| {
        ui.button("Bordered");
        Button::new("Prominent").style(ButtonStyle::BorderedProminent).show(ui);
        Button::new("Borderless").style(ButtonStyle::Borderless).show(ui);
      }),
      "Button role" => Self::variants(ui, Shown::Sized, |ui| {
        Button::new("Delete").role(ButtonRole::Destructive).show(ui);
        let red = Button::new("Delete").style(ButtonStyle::BorderedProminent);
        red.role(ButtonRole::Destructive).show(ui);
      }),
      "Icon button" => Self::variants(ui, Shown::Sized, |ui| {
        ui.a11y_label("Reset").icon_button(Icon::RESET);
        ui.icon_button(Icon::TRASH);
      }),
      "Switch" => Self::variants(ui, Shown::Sized, |ui| _ = ui.toggle(&mut self.live, "Live")),
      "Check box" => Self::variants(ui, Shown::Plain, |ui| _ = ui.checkbox(&mut self.wrap, "Wrap")),
      "Slider" => Self::variants(ui, Shown::Plain, |ui| {
        ui.a11y_label("Zoom").slider(&mut self.zoom, 0.0..=200.0);
        ui.text(format_args!("{:.0}%", self.zoom), t.muted().nowrap());
      }),
      "Picker" => Self::variants(ui, Shown::Plain, |ui| {
        let p = Picker::new(&mut self.size).options(&SIZES[..1]).divider();
        p.options(&SIZES[1..]).show(ui.a11y_label("Size"));
      }),
      "Segmented picker" => Self::variants(ui, Shown::Plain, |ui| {
        let views = [
          Tag::new(0, "List").count(128),
          Tag::new(1, "Grid").count(12),
          Tag::new(2, "Columns").count(3),
        ];
        ui.segmented(&mut self.view, &views);
      }),
      "Text field" => Self::variants(ui, Shown::Plain, |ui| {
        ui.col(Style::new().w(px(200.0)), |ui| _ = ui.field(&mut self.name, "Name"));
      }),
      "Secure field" => Self::variants(ui, Shown::Plain, |ui| {
        ui.col(Style::new().w(px(200.0)), |ui| _ = ui.password_field(&mut self.key, "Password"));
      }),
      "Search field" => Self::variants(ui, Shown::Plain, |ui| {
        ui.col(Style::new().w(px(200.0)), |ui| _ = ui.search(&mut self.query, "Search"));
      }),
      "Progress" => Self::variants(ui, Shown::Plain, |ui| {
        ui.col(Style::new().w(px(150.0)), |ui| ui.progress(Some(0.42)));
        ui.col(Style::new().w(px(150.0)), |ui| ui.progress(None));
      }),
      "Spinner" => Self::variants(ui, Shown::Sized, |ui| _ = ui.spinner()),
      "Badge" => Self::variants(ui, Shown::Sized, |ui| {
        ui.badge(128, Tone::Neutral);
        ui.badge("New", Tone::Accent);
        ui.badge("+5", Tone::Success);
        ui.badge("-4", Tone::Danger);
        ui.badge("Conflict", Tone::Warning);
      }),
      "Shortcut recorder" => Self::variants(ui, Shown::Plain, |ui| {
        ui.shortcut(&mut self.hotkey);
        // The ⊗ that clears one, which the plates themes leave to a reset button.
        let t = ui.theme();
        ui.set_theme(nimbo::Theme { shortcut_clear: true, ..t });
        ui.shortcut(&mut self.hotkey);
        ui.set_theme(t);
      }),
      "Help marks" => Self::variants(ui, Shown::Plain, |ui| {
        ui.help("An ⓘ explains what one line can't").info_button();
        ui.help("A warning tells what is wrong").warning_button();
        ui.help("Opens the page's help").help_button();
      }),
      "Section" | "Form row" | "Composed label" | "Row tile" | "Disclosure" | "Footer" => {
        self.form(ui, name)
      }
      "List" => Self::variants(ui, Shown::Plain, |ui| {
        let list = List::new(40, 22.0).selection(&mut self.rows);
        let list = list.style(Style::new().w(px(220.0)).h(px(88.0)));
        list.show(ui, |ui, row| ui.text(format_args!("Row {}", row.index + 1), t.label()));
      }),
      "List sections" => Self::variants(ui, Shown::Plain, |ui| {
        let head = |ui: &mut Ui, g: usize| ui.text(["Today", "Yesterday"][g], t.section());
        let list = List::new(6, 30.0).sections(&[0, 3], 24.0, &head);
        let list = list.list_style(ListStyle::Grouped).selection(&mut self.rows);
        let list = list.style(Style::new().w(px(320.0)).h(px(290.0)));
        list.show(ui, |ui, row| ui.text(format_args!("Dictation {}", row.index + 1), t.label()));
      }),
      "Tabs" => Self::variants(ui, Shown::Plain, |ui| {
        ui.col(Style::new().w(px(260.0)).h(px(70.0)), |ui| {
          Tabs::new(&mut self.tab, &["General", "Advanced"]).show(ui, |ui, i| {
            ui.text(["The general page", "The advanced page"][i], t.label());
          });
        });
      }),
      "Split" => Self::variants(ui, Shown::Plain, |ui| {
        ui.row(Style::new().w(px(260.0)).h(px(60.0)), |ui| {
          Split::new(Axis::Horizontal).ratio(&mut self.side).show(ui, |ui, pane| {
            ui.text(["Sidebar", "Content"][pane], t.label());
          });
        });
      }),
      "Menu" => Self::variants(ui, Shown::Plain, |ui| {
        ui.menu("Actions", |ui| {
          ui.menu_item("Copy");
          ui.menu_item("Paste");
        });
      }),
      "Lazy column" => Self::variants(ui, Shown::Plain, |ui| {
        ui.scroll_area(Style::new().w(px(200.0)).h(px(60.0)), |ui| {
          let item = |ui: &mut Ui, i: usize| ui.text(format_args!("Item {i}"), t.label());
          ui.virtual_col((0..1000).map(|_| 20.0), |i| i, item);
        });
      }),
      "Alert" => {
        Self::variants(ui, Shown::Plain, |ui| {
          if ui.button("Delete…").clicked {
            self.ask = true;
          }

          ui.text(format_args!("Deleted {} times", self.deleted), t.muted().nowrap());
        });
        let d = Dialog::new("Delete the file?").message("It moves to the Trash.").icon(Icon::TRASH);
        let d = d.button("Delete", ButtonRole::Destructive);
        if d.show(ui.id(DIALOG), &mut self.ask) == Some(Answer::Action) {
          self.deleted += 1;
        }
      }
      "Context menu" => Self::variants(ui, Shown::Plain, |ui| {
        let r = ui.button("Right-click me");
        let menu = || {
          let delete = Item::button(Action::Delete, "Delete").role(ButtonRole::Destructive);
          Menu::new(vec![Item::button(Action::Copy, "Copy"), Item::Divider, delete])
        };
        if ui.context_menu(r.id, menu) == Some(Action::Copy) {
          self.copied += 1;
        }
      }),
      "Tooltip" => Self::variants(ui, Shown::Plain, |ui| _ = ui.help("Saves it").button("Save")),
      "Disabled" => Self::variants(ui, Shown::Plain, |ui| {
        ui.button("Save");
        ui.toggle(&mut self.on, "");
      }),
      "Control size" => Self::variants(ui, Shown::Sized, |ui| {
        ui.button("Save");
        ui.toggle(&mut self.on, "");
        ui.badge("Beta", Tone::Neutral);
      }),
      "Accessibility label" => {
        Self::variants(ui, Shown::Plain, |ui| _ = ui.a11y_label("Reset").icon_button(Icon::RESET))
      }
      _ => Self::variants(ui, Shown::Plain, |ui| {
        let keys = Shortcut::char('z').cmd().label();
        ui.text(format_args!("{keys} undoes, in the window's keymap"), t.label());
      }),
    }
  }

  /// A form block in the form's look, on its ground.
  fn form(&mut self, ui: &mut Ui, name: &str) {
    let fs = FormStyle::plates(&ui.theme());
    let on = &mut self.on;
    Self::variants(ui, Shown::Plain, |ui| {
      let row = |ui: &mut Ui, on: &mut bool| match name {
        "Form row" => {
          let row = FormRow::new("Open at login").caption("Starts with your Mac, in the menu bar.");
          ui.form_row(&fs, row, |ui| ui.toggle(on, ""))
        }
        "Composed label" => {
          let label = |ui: &mut Ui, s: TextStyle| {
            ui.text("gigaam-v3", s.line_limit(1));
            let ui = ui.help("Best in Russian").control_size(ControlSize::Small);
            ui.badge("Recommended", Tone::Accent);
          };
          let caption = |ui: &mut Ui, s: TextStyle| {
            ui.row(Style::new().w(grow()), |ui| {
              ui.help("Errors per 100 words").text("WER 4.7%", s.underline(LinePattern::Dot));
              ui.text(" · 274 MB", s);
            });
          };
          ui.form_row(&fs, FormRow::new(label).caption(caption), |ui| ui.button("Use"))
        }
        "Row tile" => {
          let row = FormRow::new("Data folder").caption("~/Library/Application Support");
          ui.form_row(&fs, row.tile(Icon::FOLDER), |ui| ui.button("Show"))
        }
        "Disclosure" => {
          let row = FormRow::new("Other models").caption("2 more");
          ui.form_disclosure(&fs, row, |ui| ui.form_row(&fs, FormRow::new("whisper-tiny"), |_| {}))
        }
        _ => ui.form_row(&fs, FormRow::new("Row"), |ui| ui.toggle(on, "")),
      };
      let page = Style::new().w(grow()).gap(10.0).pad(10.0).radius(12.0).bg(fs.bg);
      ui.col(page, |ui| match name {
        "Section" => {
          let head = |ui: &mut Ui, s: TextStyle| {
            ui.text("Storage", s);
            ui.spacer();
            ui.text("640 MB used", fs.note);
          };
          let section = Section::new(head).footer("Kept until you clear it.");
          _ = ui.form_section(&fs, section, |ui| row(ui, on));
        }
        "Footer" => {
          ui.form_section(&fs, "Section", |ui| row(ui, on));
          ui.help("Help on these settings").form_footer(&fs, |ui| _ = ui.button("Reset All…"));
        }
        _ => _ = ui.form_section(&fs, "", |ui| row(ui, on)),
      });
    });
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
          ui.selectable_text(block.swiftui, &nimbo::FieldStyle { text: mono, ..t.field() });
          ui.text("NIMBO", t.section());
          ui.selectable_text(block.nimbo, &nimbo::FieldStyle { text: mono, ..t.field() });
        });
        self.demo(ui, block.name);
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
    // The system's look, whatever a block set last frame.
    ui.follow_system();
    let t = ui.theme();
    if self.only {
      ui.fit_window_height(WIDTH, 1.0, 900.0);
      let name = BLOCKS[self.page].name;
      ui.id(BLOCK).col(Style::new().w(grow()).pad(20.0).bg(t.bg), |ui| self.demo(ui, name));
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
     api-shots` makes them and this file again from the gallery's table (`examples/gallery.rs`), \
     so a block changes there.\n\n\
     On macOS SwiftUI's `List` has neither of the grouped styles (`InsetGroupedListStyle` and \
     `GroupedListStyle` are iOS's), so there is no name to take: nimbo's `ListStyle::Grouped` is \
     named after the grouped `Form` (`.formStyle(.grouped)`), whose plates its sections share.\n",
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
  nimbo::run_args(Config::new("Gallery", w, h), app, |h| _ = h.frame(), rest);
}
