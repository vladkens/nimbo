// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A demo app's settings window in macOS 26 System Settings' look ([nimbo::FormStyle::plates]):
//! toolbar tabs (General, Appearance, Advanced, About) over pages of grey plates, in a window
//! 680 wide whose height follows the page. General has switches, pop-ups (one with an ⓘ
//! tooltip, one with a description and a separator), a shortcut recorder with its reset, a row
//! disabled by another setting and a segmented control; Appearance the look (which applies at
//! once), a slider and a switch with a long description that wraps beside it; Advanced tiles,
//! badges, a task's progress and buttons, two of which ask in a modal dialog first, and a footer
//! with the "?"; About the app and its licenses. Pages fade out under the edges of their scroll
//! area; light or dark and in the accent the system has.
//!
//! `cargo run --release --example settings [-- --shot <file.png>]`: the shot rests the pointer
//! on the ⓘ of "On launch" until its tooltip shows.

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::time::Duration;

use nimbo::{
  Align, Answer, App, Appearance, Config, Dialog, FormRow, FormStyle, Hotkey, Icon, Id, Rect,
  Select, Style, Theme, Tile, Tone, Ui, grow, px,
};

pub const TABS: &str = "tabs";
pub const LOGIN: &str = "login";
pub const LAUNCH: &str = "launch";
pub const LAUNCH_ROW: &str = "launch-row";
pub const LANG: &str = "lang";
pub const HOTKEY: &str = "hotkey";
pub const RESET: &str = "reset";
pub const UPDATES: &str = "updates";
pub const DOWNLOAD: &str = "download";
pub const CHANNEL: &str = "channel";
pub const LOOK: &str = "look";
pub const TEXT_SIZE: &str = "text-size";
pub const MOTION: &str = "motion";
pub const MOTION_ROW: &str = "motion-row";
pub const CLEAR_CACHE: &str = "clear-cache";
pub const RESET_ALL: &str = "reset-all";
pub const HELP: &str = "help";
/// The dialog that asks before clearing the cache or resetting everything.
pub const CONFIRM: &str = "confirm";

/// The shortcut out of the box: Control-Option-Space.
pub const DEFAULT_HOTKEY: &str = "ctrl+option+space";
/// The window's width; its height is the page's.
pub const WIDTH: f32 = 680.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
  General,
  Appearance,
  Advanced,
  About,
}

pub const PAGES: [(Page, Icon, &str); 4] = [
  (Page::General, Icon::GEAR, "General"),
  (Page::Appearance, Icon::CONTRAST, "Appearance"),
  (Page::Advanced, Icon::SLIDERS, "Advanced"),
  (Page::About, Icon::INFO, "About"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Launch {
  Window,
  Restore,
  Hidden,
}

pub const LAUNCHES: [(Launch, &str); 3] = [
  (Launch::Window, "Open a new window"),
  (Launch::Restore, "Restore the last session"),
  (Launch::Hidden, "Start hidden"),
];

pub const LANGS: [(usize, &str); 7] = [
  (0, "System Default"),
  (1, "English"),
  (2, "Deutsch"),
  (3, "Español"),
  (4, "Français"),
  (5, "Italiano"),
  (6, "Українська"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
  Stable,
  Beta,
}

/// Light, dark, or the system's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
  System,
  Light,
  Dark,
}

pub const LOOKS: [(Look, &str); 3] =
  [(Look::System, "Auto"), (Look::Light, "Light"), (Look::Dark, "Dark")];

pub const LOGGING: [(u8, &str); 3] = [(0, "Errors only"), (1, "Warnings"), (2, "Everything")];

pub const TIP_LAUNCH: &str = "What the app shows when it starts. Restoring the last session \
  reopens its windows where they were.";
pub const MOTION_NOTE: &str = "Turns off the animations of switches, menus and tooltips, and keeps \
  the window still while it resizes. Applies to every window of the app at once.";

/// What the dialog asks about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
  ClearCache,
  ResetAll,
}

pub struct Settings {
  pub page: Page,
  pub login: bool,
  pub launch: Launch,
  pub lang: usize,
  pub hotkey: Option<Hotkey>,
  pub updates: bool,
  pub download: bool,
  pub channel: Channel,
  pub look: Look,
  pub text_size: f32,
  pub motion: bool,
  pub reports: bool,
  pub logging: u8,
  /// The cache in MB; cleared by the dialog.
  pub cache: u32,
  /// The question the dialog asks, while it is open.
  pub ask: Option<Ask>,
  pub resets: usize,
}

impl Default for Settings {
  fn default() -> Self {
    Self {
      page: Page::General,
      login: true,
      launch: Launch::Restore,
      lang: 0,
      hotkey: Hotkey::parse(DEFAULT_HOTKEY).ok(),
      updates: true,
      download: false,
      channel: Channel::Stable,
      look: Look::System,
      text_size: 13.0,
      motion: false,
      reports: true,
      logging: 1,
      cache: 128,
      ask: None,
      resets: 0,
    }
  }
}

impl Settings {
  fn general(&mut self, ui: &mut Ui, fs: &FormStyle) {
    ui.form_section(fs, "Startup", |ui| {
      ui.form_row(fs, FormRow::new("Open at login"), |ui| ui.id(LOGIN).toggle(&mut self.login, ""));
      let row = FormRow::new("On launch").info(TIP_LAUNCH);
      ui.id(LAUNCH_ROW).form_row(fs, row, |ui| ui.id(LAUNCH).select(&mut self.launch, &LAUNCHES));
      let row = FormRow::new("Language").caption("Applies the next time the app starts.");
      ui.form_row(fs, row, |ui| {
        Select::new(&mut self.lang, &LANGS).separators(&[0]).show(ui.id(LANG))
      });
    });
    ui.form_section_sub(fs, "Shortcuts", "Work in every app, while this one runs.", |ui| {
      ui.form_row(fs, FormRow::new("Show the app"), |ui| {
        ui.id(HOTKEY).shortcut(&mut self.hotkey);
        let default = Hotkey::parse(DEFAULT_HOTKEY).ok();
        let r = ui.disabled(self.hotkey == default).id(RESET).icon_button(Icon::RESET);
        ui.tooltip(r.id, "Back to the default shortcut");
        if r.clicked {
          self.hotkey = default;
        }
      });
    });
    ui.form_section(fs, "Updates", |ui| {
      let row = FormRow::new("Check for updates automatically");
      ui.form_row(fs, row, |ui| ui.id(UPDATES).toggle(&mut self.updates, ""));
      let row = FormRow::new("Download them in the background").disabled(!self.updates);
      ui.form_row(fs, row, |ui| ui.id(DOWNLOAD).toggle(&mut self.download, ""));
      let channels = [(Channel::Stable, "Stable"), (Channel::Beta, "Beta")];
      ui.form_row(fs, FormRow::new("Channel"), |ui| {
        ui.id(CHANNEL).segmented(&mut self.channel, &channels)
      });
    });
  }

  fn appearance(&mut self, ui: &mut Ui, fs: &FormStyle) {
    ui.form_section(fs, "Look", |ui| {
      ui.form_row(fs, FormRow::new("Appearance"), |ui| {
        ui.id(LOOK).segmented(&mut self.look, &LOOKS)
      });
      ui.form_row(fs, FormRow::new("Text size"), |ui| {
        let t = ui.theme();
        ui.text("A", t.label().font(nimbo::Font { size: 10.0, ..t.font }));
        ui.id(TEXT_SIZE).slider(&mut self.text_size, 10.0..=20.0);
        ui.text("A", t.label().font(nimbo::Font { size: 16.0, ..t.font }));
      });
    });
    ui.form_section(fs, "Motion", |ui| {
      let row = FormRow::new("Reduce motion").caption(MOTION_NOTE);
      ui.id(MOTION_ROW).form_row(fs, row, |ui| ui.id(MOTION).toggle(&mut self.motion, ""));
    });
  }

  fn advanced(&mut self, ui: &mut Ui, fs: &FormStyle) {
    ui.form_section(fs, "Account", |ui| {
      let row = FormRow::new("Alex Doe").caption("alex@example.com").tile(Tile::Text("AD"));
      ui.form_row(fs, row, |ui| ui.button("Sign Out…"));
    });
    let used = format!("{} MB used", self.cache + 512);
    ui.form_section_with(
      fs,
      "Storage",
      |ui| ui.text(&used, fs.note),
      |ui| {
        let row = FormRow::new("Data folder")
          .caption("~/Library/Application Support/Example")
          .tile(Tile::Icon(Icon::FOLDER))
          .one_line();
        ui.form_row(fs, row, |ui| {
          ui.button("Change…");
          ui.button("Show");
        });
        let caption = format!("{} MB of thumbnails and previews", self.cache);
        let row = FormRow::new("Cache").caption(&caption).tile(Tile::Icon(Icon::TRASH));
        ui.form_row(fs, row, |ui| {
          if ui.disabled(self.cache == 0).id(CLEAR_CACHE).button("Clear…").clicked {
            self.ask = Some(Ask::ClearCache);
          }
        });
        let row = FormRow::new("Index")
          .caption("Indexing 4,210 of 10,000 files")
          .tile(Tile::Icon(Icon::DOC));
        ui.form_row_with(
          fs,
          row,
          // Clear of the description's descenders.
          |ui| {
            _ = ui
              .col(Style::new().w(grow()).pads([4.0, 0.0, 0.0, 0.0]), |ui| ui.progress(Some(0.42)))
          },
          |ui| ui.badge("Running", Tone::Accent),
        );
      },
    );
    ui.form_section(fs, "Diagnostics", |ui| {
      ui.form_row(fs, FormRow::new("Send crash reports"), |ui| {
        match self.reports {
          true => ui.badge("On", Tone::Success),
          false => ui.badge("Off", Tone::Warning),
        };

        ui.toggle(&mut self.reports, "")
      });
      ui.form_row(fs, FormRow::new("Logging"), |ui| ui.select(&mut self.logging, &LOGGING));
    });
    let help = ui.id(HELP).form_footer(fs, |ui| {
      if ui.id(RESET_ALL).button("Reset All…").clicked {
        self.ask = Some(Ask::ResetAll);
      }
    });
    ui.tooltip(help.id, "Help on these settings");
  }

  /// About's head above its page: the icon, the name, the version, a line and a link.
  fn about_head(&mut self, ui: &mut Ui, fs: &FormStyle) {
    let t = ui.theme();
    let head =
      Style::new().w(grow()).pads([22.0, 20.0, 0.0, 20.0]).align(Align::Center, Align::Start);
    ui.col(head, |ui| {
      let accent = t.accent;
      ui.canvas(Style::new().w(px(64.0)).h(px(64.0)), move |f, r| app_icon(f, r, accent));
      ui.el(Style::new().h(px(10.0)));
      ui.text("Example", t.label().font(nimbo::Font { size: 20.0, ..t.bold }));
      ui.el(Style::new().h(px(2.0)));
      ui.text("Version 1.0.0", fs.note);
      ui.el(Style::new().h(px(8.0)));
      ui.text("A demo of nimbo's settings form, dialog and widgets.", t.label());
      ui.el(Style::new().h(px(10.0)));
      ui.link("github.com/vladkens/nimbo", t.label().color(t.link));
    });
  }

  fn about(&mut self, ui: &mut Ui, fs: &FormStyle) {
    ui.form_section(fs, "Licenses", |ui| {
      for (name, what) in [("nimbo", "MIT License"), ("keyboard-types", "MIT or Apache-2.0")] {
        ui.form_row(fs, FormRow::new(name).caption(what), |ui| ui.icon_button(Icon::EXTERNAL));
      }
    });
  }

  /// The page under the toolbar, in its scroll area.
  fn page(&mut self, ui: &mut Ui, fs: &FormStyle) {
    let page = match self.page {
      Page::About => {
        self.about_head(ui, fs);
        fs.page.pads([20.0, 20.0, 10.0, 20.0])
      }
      _ => fs.page,
    };

    ui.col(page, |ui| match self.page {
      Page::General => self.general(ui, fs),
      Page::Appearance => self.appearance(ui, fs),
      Page::Advanced => self.advanced(ui, fs),
      Page::About => self.about(ui, fs),
    });
  }

  /// The dialog that asks before clearing the cache or resetting everything, while asking.
  fn confirm(&mut self, ui: &mut Ui) {
    let Some(ask) = self.ask else { return };
    let d = match ask {
      Ask::ClearCache => Dialog::new("Clear the cache?")
        .message("Thumbnails and previews are made again as you open files.")
        .icon(Icon::TRASH)
        .destructive("Clear"),
      Ask::ResetAll => Dialog::new("Reset all settings?")
        .message("Every setting goes back to how it was when the app was installed.")
        .action("Reset"),
    };

    let mut open = true;
    if d.show(ui.id(CONFIRM), &mut open) == Some(Answer::Action) {
      match ask {
        Ask::ClearCache => self.cache = 0,
        Ask::ResetAll => {
          let (page, resets) = (self.page, self.resets + 1);
          *self = Self { page, resets, ..Self::default() };
        }
      }
    }

    if !open {
      self.ask = None;
    }
  }
}

/// The demo app's icon: a white check on a rounded square in the accent.
fn app_icon(f: &mut nimbo::Frame, r: Rect, accent: nimbo::Color) {
  f.shadow(Rect::new(r.x, r.y + 1.0, r.w, r.h), 15.0, 3.0, [0.0, 0.0, 0.0, 0.2]);
  f.rect(r, accent, 15.0);
  let inner = Rect::new(r.x + 14.0, r.y + 14.0, r.w - 28.0, r.h - 28.0);
  nimbo::ui::widgets::icon::draw(f, Icon::CHECK, inner, [1.0; 4], 2.4);
}

impl App for Settings {
  fn view(&mut self, ui: &mut Ui) {
    let a = ui.appearance();
    match self.look {
      Look::System => ui.follow_system(),
      Look::Light => ui.set_theme(Theme::system(Appearance { dark: false, ..a })),
      Look::Dark => ui.set_theme(Theme::system(Appearance { dark: true, ..a })),
    }

    // The window as high as the page, in the OS's font.
    ui.fit_window_height(WIDTH, 300.0, 720.0);
    let fs = FormStyle::plates(&ui.theme());
    ui.col(Style::new().grow().bg(fs.bg), |ui| {
      ui.id(TABS).toolbar_tabs(&fs, &mut self.page, &PAGES);
      ui.id(("page", self.page as u8)).form_scroll(&fs, Style::new().grow(), |ui| {
        ui.col(Style::new().w(grow()), |ui| self.page(ui, &fs));
      });
    });
    self.confirm(ui);
  }
}

fn main() {
  nimbo::run(Config::new("Settings", WIDTH, 560.0).fixed(), Settings::default(), |h| {
    h.frame();
    h.frame();
    let r = h.rect(Id::of(LAUNCH_ROW).key("info")).expect("the ⓘ of On launch");
    h.move_to(r.x + r.w / 2.0, r.y + r.h / 2.0);
    h.wait(Duration::from_millis(400));
  });
}
