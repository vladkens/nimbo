// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Every widget, in the light theme and the dark one side by side over the same state: buttons
//! (variants, an icon button with a tooltip, ⓘ and "?"), check boxes, switches in both sizes, a
//! slider, selects, a segmented control with counts, progress bars and a spinner, a link,
//! badges in every tone, shortcut recorders (with and without the ⊗ that clears one), search
//! fields, the icons (stroked; play and ⊗ filled), and a button that asks in a modal dialog.
//!
//! `cargo run --release --example gallery [-- --shot <file.png>]`

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::time::Duration;

use nimbo::{
  Align, Answer, App, Appearance, Button, Config, ControlSize, Dialog, Hotkey, Icon, Id, Style,
  Theme, Tone, Ui, grow, px,
};

pub const CHECK: &str = "check";
pub const TOGGLE: &str = "toggle";
pub const SLIDER: &str = "slider";
pub const SIZE: &str = "size";
pub const VIEW: &str = "view";
pub const HOTKEY: &str = "hotkey";
pub const CLEARABLE: &str = "clearable";
pub const ASK: &str = "ask";
pub const DIALOG: &str = "dialog";
pub const RESET: &str = "reset";
pub const SEARCH: &str = "search";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
  Small,
  Medium,
  Large,
}

pub const SIZES: [(Size, &str); 3] =
  [(Size::Small, "Small"), (Size::Medium, "Medium"), (Size::Large, "Large")];

pub struct Gallery {
  pub wrap: bool,
  pub live: bool,
  pub big: bool,
  pub zoom: f32,
  pub size: Size,
  pub view: u8,
  pub hotkey: Option<Hotkey>,
  pub search: String,
  /// The pane whose button opened the dialog: it asks in that pane's theme.
  pub ask: Option<usize>,
  pub deleted: usize,
}

impl Default for Gallery {
  fn default() -> Self {
    Self {
      wrap: true,
      live: false,
      big: true,
      zoom: 60.0,
      size: Size::Medium,
      view: 0,
      hotkey: Hotkey::parse("ctrl+option+space").ok(),
      search: "Switches".to_owned(),
      ask: None,
      deleted: 0,
    }
  }
}

/// The theme of pane `i`: light, then dark, in the system's accent.
fn theme(ui: &Ui, i: usize) -> Theme {
  Theme::system(Appearance { dark: i == 1, ..ui.appearance() })
}

/// A titled line of widgets.
fn line(ui: &mut Ui, title: &str, f: impl FnOnce(&mut Ui)) {
  let t = ui.theme();
  ui.col(Style::new().w(grow()).gap(6.0), |ui| {
    ui.text(title, t.section());
    ui.row(Style::new().w(grow()).gap(10.0).align(Align::Start, Align::Center), f);
  });
}

impl Gallery {
  /// Pane `i` (0 light, 1 dark): every widget, its ids keyed by the pane.
  fn pane(&mut self, ui: &mut Ui, i: usize) {
    let t = ui.theme();
    let pane = Style::new().grow().pad(20.0).gap(14.0).bg(t.bg);
    ui.col(pane, |ui| {
      line(ui, "BUTTONS", |ui| {
        ui.button("Normal");
        Button::new("Save").primary().show(ui);
        Button::new("Delete").danger().show(ui);
        Button::new("Ghost").ghost().show(ui);
        ui.disabled(true).button("Off");
        let r = ui.id((RESET, i)).icon_button(Icon::RESET);
        ui.tooltip(r.id, "Back to the default");
        let r = ui.info_button();
        ui.tooltip(r.id, "An ⓘ explains what one line can't");
        ui.help_button();
      });
      line(ui, "CHECK BOXES AND SWITCHES", |ui| {
        ui.id((CHECK, i)).checkbox(&mut self.wrap, "Wrap lines");
        ui.disabled(true).checkbox(&mut false, "Off");
        ui.id((TOGGLE, i)).toggle(&mut self.live, "Live");
        ui.toggle_sized(&mut self.big, "", ControlSize::Large);
      });
      line(ui, "SLIDER, SELECT, SEGMENTS", |ui| {
        ui.id((SLIDER, i)).slider(&mut self.zoom, 0.0..=200.0);
        ui.text(format_args!("{:.0}%", self.zoom), t.muted().nowrap());
      });
      ui.row(Style::new().gap(10.0).align(Align::Start, Align::Center), |ui| {
        ui.id((SIZE, i)).select(&mut self.size, &SIZES);
        ui.disabled(true).select(&mut Size::Small, &SIZES);
        let views = [(0, "List", 128), (1, "Grid", 12), (2, "Columns", 3)];
        ui.id((VIEW, i)).segmented(&mut self.view, &views);
      });
      line(ui, "PROGRESS", |ui| {
        ui.col(Style::new().w(px(150.0)), |ui| ui.progress(Some(0.42)));
        ui.col(Style::new().w(px(150.0)), |ui| ui.disabled(true).progress(None));
        ui.spinner(16.0);
      });
      line(ui, "LINK AND BADGES", |ui| {
        ui.link("Learn more…", t.label().color(t.link));
        ui.badge(128, Tone::Neutral);
        ui.badge("new", Tone::Accent);
        ui.badge("+5", Tone::Success);
        ui.badge("-4", Tone::Danger);
        ui.badge("conflict", Tone::Warning);
      });
      line(ui, "SHORTCUTS", |ui| {
        ui.id((HOTKEY, i)).shortcut(&mut self.hotkey);
        // The ⊗ that clears one, which the plates themes leave to a reset button.
        ui.set_theme(Theme { shortcut_clear: true, ..t });
        ui.id((CLEARABLE, i)).shortcut(&mut self.hotkey);
        ui.set_theme(t);
      });
      line(ui, "SEARCH", |ui| {
        ui.id((SEARCH, i)).search(&mut self.search, "Search widgets");
        ui.disabled(true).search(&mut String::new(), "Off");
      });
      line(ui, "ICONS", |ui| {
        ui.grid(10, Style::new().gap(10.0), |ui| {
          for (_, icon) in Icon::ALL {
            ui.icon_stroke(icon, 20.0, t.text, 1.6);
          }
        });
      });
      line(ui, "DIALOG", |ui| {
        if ui.id((ASK, i)).button("Delete…").clicked {
          self.ask = Some(i);
        }

        let n = self.deleted;
        ui.text(format_args!("Deleted {n} times"), t.muted().nowrap());
      });
    });
  }
}

impl App for Gallery {
  fn view(&mut self, ui: &mut Ui) {
    ui.row(Style::new().grow(), |ui| {
      for i in 0..2 {
        let t = theme(ui, i);
        ui.set_theme(t);
        ui.key(i).col(Style::new().grow(), |ui| self.pane(ui, i));
      }
    });

    let Some(i) = self.ask else { return };
    ui.set_theme(theme(ui, i));
    let mut open = true;
    let d = Dialog::new("Delete the file?")
      .message("It moves to the Trash, which empties in 30 days.")
      .icon(Icon::TRASH)
      .destructive("Delete");
    if d.show(ui.id(DIALOG), &mut open) == Some(Answer::Action) {
      self.deleted += 1;
    }

    if !open {
      self.ask = None;
    }
  }
}

fn main() {
  // The shot: the pointer on the light pane's switch.
  nimbo::run(Config::new("gallery", 1040.0, 590.0), Gallery::default(), |h| {
    h.frame();
    let r = h.rect(Id::of((TOGGLE, 0usize))).unwrap();
    h.move_to(r.x + 10.0, r.y + r.h / 2.0);
    h.wait(Duration::from_millis(300));
  });
}
