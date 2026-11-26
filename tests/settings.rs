// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests through the harness of the settings window below: the toolbar turns pages and the window
//! follows their height; switches flip, a row disabled by another setting ignores clicks; a select
//! opens over its button and chooses by letters typed, closed and open; the shortcut recorder
//! records a chord and a modifier key alone, Escape and a click elsewhere stop it, Backspace clears
//! it, the reset puts the default back; the slider drags and steps, greys with the selected
//! segment while the window isn't key; the dialog asks before clearing
//! the cache and resetting all: Return chooses its default, Escape cancels, Tab and Space the
//! action, its backdrop dims the page and takes the pointer; a row's switch keeps its width and
//! place beside a long description that wraps. A row's warning takes its ⓘ's place, in the
//! theme's warning colour, and shows in one tooltip with the ⓘ's tip under it; a disabled row's
//! ⓘ stays enabled and shows its tip. Assistive technology reads the controls by their
//! rows with their values, and the dialog as modal, its default button focused until it gives
//! the focus back, and the focus only while the window has it; its actions flip a switch, step
//! the slider, choose in a select, move the focus, and reach only the dialog while it is open.

mod app {

  //! A demo app's settings window in macOS 26 System Settings' look ([nimbo::FormStyle::plates]):
  //! toolbar tabs (General, Appearance, Advanced, About) over pages of grey plates, in a window
  //! 680 wide whose height follows the page. General has switches (one with a warning while it is
  //! on), pickers (one with an ⓘ tooltip, one with a description and a divider), a shortcut
  //! recorder with its reset (a warning and an ⓘ in one tooltip while it is the default) under a
  //! section header with a grey line, a row disabled by another setting, with a badge after its
  //! name, whose ⓘ still shows its tooltip, and a segmented control; Appearance the look (which
  //! applies at once), a slider and a switch with a long description that wraps beside it;
  //! Advanced tiles, badges, a section header with a note, a task's progress under its description
  //! and buttons, two of which ask in a modal dialog first, and a footer with the "?"; About the
  //! app and its licenses. Pages fade out under the edges of their scroll area; light or dark and
  //! in the accent the system has.

  use nimbo::{
    Align, Answer, App, ButtonRole, Color, ColorScheme, ControlSize, Dialog, FormRow, FormStyle,
    Frame, Hotkey, Icon, Id, Picker, Rect, Section, SegmentedPickerStyle, Style, Tag, TextStyle,
    Theme, Tone, Ui, grow, px,
  };

  pub const TABS: &str = "tabs";
  pub const LOGIN: &str = "login";
  pub const LOGIN_ROW: &str = "login-row";
  pub const LAUNCH: &str = "launch";
  pub const LAUNCH_ROW: &str = "launch-row";
  pub const LANG: &str = "lang";
  pub const HOTKEY: &str = "hotkey";
  pub const HOTKEY_ROW: &str = "hotkey-row";
  pub const RESET: &str = "reset";
  pub const UPDATES: &str = "updates";
  pub const DOWNLOAD: &str = "download";
  pub const DOWNLOAD_ROW: &str = "download-row";
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

  pub const PAGES: [Tag<Page>; 4] = [
    Tag::new(Page::General, "General").icon(Icon::GEAR),
    Tag::new(Page::Appearance, "Appearance").icon(Icon::CONTRAST),
    Tag::new(Page::Advanced, "Advanced").icon(Icon::SLIDERS),
    Tag::new(Page::About, "About").icon(Icon::INFO),
  ];

  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Launch {
    Window,
    Restore,
    Hidden,
  }

  pub const LAUNCHES: [Tag<Launch>; 3] = [
    Tag::new(Launch::Window, "Open a new window"),
    Tag::new(Launch::Restore, "Restore the last session"),
    Tag::new(Launch::Hidden, "Start hidden"),
  ];

  pub const LANGS: [Tag<usize>; 7] = [
    Tag::new(0, "System Default"),
    Tag::new(1, "English"),
    Tag::new(2, "Deutsch"),
    Tag::new(3, "Español"),
    Tag::new(4, "Français"),
    Tag::new(5, "Italiano"),
    Tag::new(6, "Українська"),
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

  pub const LOOKS: [Tag<Look>; 3] =
    [Tag::new(Look::System, "Auto"), Tag::new(Look::Light, "Light"), Tag::new(Look::Dark, "Dark")];

  pub const LOGGING: [Tag<u8>; 3] =
    [Tag::new(0, "Errors only"), Tag::new(1, "Warnings"), Tag::new(2, "Everything")];

  pub const TIP_LAUNCH: &str = "What the app shows when it starts. Restoring the last session \
    reopens its windows where they were.";
  pub const LOGIN_WARNING: &str = "Waits for your approval in System Settings, under General, Login \
    Items.";
  pub const TIP_HOTKEY: &str = "Shows the app's window from any other app, and hides it again.";
  pub const HOTKEY_WARNING: &str = "macOS switches input sources with it too.";
  pub const TIP_DOWNLOAD: &str = "Each update downloads when it comes out and installs when the app \
    quits.";
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

  /// A row's label: `name`, then the row's mark `Id::of(row).key("info")` with its tip: a warning
  /// when there is one, the ⓘ's tip under it, else the ⓘ alone.
  fn marked<'s>(
    name: &'s str,
    row: &'static str,
    warning: Option<&'s str>,
    info: Option<&'s str>,
  ) -> impl FnOnce(&mut Ui, TextStyle) + 's {
    move |ui: &mut Ui, s: TextStyle| {
      ui.text(name, s);
      let mark = Id::of(row).key("info");
      match (warning, info) {
        (Some(w), Some(i)) => _ = ui.help(&format!("{w}\n\n{i}")).with_id(mark).warning_button(),
        (Some(w), None) => _ = ui.help(w).with_id(mark).warning_button(),
        (None, Some(i)) => _ = ui.help(i).with_id(mark).info_button(),
        (None, None) => {}
      }
    }
  }

  impl Settings {
    fn general(&mut self, ui: &mut Ui, fs: &FormStyle) {
      ui.form_section(fs, "Startup", |ui| {
        let warning = self.login.then_some(LOGIN_WARNING);
        let row = FormRow::new(marked("Open at login", LOGIN_ROW, warning, None));
        ui.id(LOGIN_ROW).form_row(fs, row, |ui| ui.id(LOGIN).toggle(&mut self.login, ()));
        let row = FormRow::new(marked("On launch", LAUNCH_ROW, None, Some(TIP_LAUNCH)));
        ui.id(LAUNCH_ROW).form_row(fs, row, |ui| ui.id(LAUNCH).picker(&mut self.launch, &LAUNCHES));
        let row = FormRow::new("Language").caption("Applies the next time the app starts.");
        ui.form_row(fs, row, |ui| {
          let langs = Picker::new(&mut self.lang).options(&LANGS[..1]).divider();
          langs.options(&LANGS[1..]).show(ui.id(LANG))
        });
      });

      let shortcuts = |ui: &mut Ui, s: TextStyle| {
        ui.col(Style::new().w(grow()).gap(fs.subtitle_gap()), |ui| {
          ui.text("Shortcuts", s);
          ui.text("Work in every app, while this one runs.", fs.subtitle());
        });
      };
      ui.form_section(fs, Section::new(shortcuts), |ui| {
        let default = Hotkey::parse(DEFAULT_HOTKEY).ok();
        let warning = (self.hotkey == default).then_some(HOTKEY_WARNING);
        let row = FormRow::new(marked("Show the app", HOTKEY_ROW, warning, Some(TIP_HOTKEY)));
        ui.id(HOTKEY_ROW).form_row(fs, row, |ui| {
          ui.id(HOTKEY).hotkey_recorder(&mut self.hotkey);

          // An icon names itself to assistive technology only by a label.
          let ui = ui.disabled(self.hotkey == default).a11y_label("Reset");
          let ui = ui.help("Back to the default shortcut");
          if ui.id(RESET).icon_button(Icon::RESET).clicked {
            self.hotkey = default;
          }
        });
      });

      ui.form_section(fs, "Updates", |ui| {
        let row = FormRow::new("Check for updates automatically");
        ui.form_row(fs, row, |ui| ui.id(UPDATES).toggle(&mut self.updates, ()));
        let label = |ui: &mut Ui, s: TextStyle| {
          ui.text("Download them in the background", s);
          ui.control_size(ControlSize::Small).badge("Beta", Tone::Neutral);
          ui.help(TIP_DOWNLOAD).with_id(Id::of(DOWNLOAD_ROW).key("info")).info_button();
        };
        ui.disabled(!self.updates)
          .id(DOWNLOAD_ROW)
          .form_row(fs, FormRow::new(label), |ui| ui.id(DOWNLOAD).toggle(&mut self.download, ()));
        let channels = [Tag::new(Channel::Stable, "Stable"), Tag::new(Channel::Beta, "Beta")];
        ui.form_row(fs, FormRow::new("Channel"), |ui| {
          ui.picker_style(&SegmentedPickerStyle).id(CHANNEL).picker(&mut self.channel, &channels)
        });
      });
    }

    fn appearance(&mut self, ui: &mut Ui, fs: &FormStyle) {
      ui.form_section(fs, "Look", |ui| {
        ui.form_row(fs, FormRow::new("Appearance"), |ui| {
          ui.picker_style(&SegmentedPickerStyle).id(LOOK).picker(&mut self.look, &LOOKS)
        });
        ui.form_row(fs, FormRow::new("Text size"), |ui| {
          let t = ui.theme();
          ui.text("A", t.label().font(t.font().size(10.0)));
          ui.id(TEXT_SIZE).slider(&mut self.text_size, 10.0..=20.0);
          ui.text("A", t.label().font(t.font().size(16.0)));
        });
      });
      ui.form_section(fs, "Motion", |ui| {
        let row = FormRow::new("Reduce motion").caption(MOTION_NOTE);
        ui.id(MOTION_ROW).form_row(fs, row, |ui| ui.id(MOTION).toggle(&mut self.motion, ()));
      });
    }

    fn advanced(&mut self, ui: &mut Ui, fs: &FormStyle) {
      ui.form_section(fs, "Account", |ui| {
        let row = FormRow::new("Alex Doe").caption("alex@example.com").tile("AD");
        ui.form_row(fs, row, |ui| ui.button("Sign Out…"));
      });

      let used = format!("{} MB used", self.cache + 512);
      let storage = |ui: &mut Ui, s: TextStyle| {
        ui.text("Storage", s);
        ui.spacer();
        ui.text(&used, fs.note());
      };
      ui.form_section(fs, Section::new(storage), |ui| {
        let folder = |ui: &mut Ui, s: TextStyle| ui.text("Data folder", s.line_limit(1));
        let row = FormRow::new(folder).caption("~/Library/Application Support/Example");
        ui.form_row(fs, row.tile(Icon::FOLDER), |ui| {
          ui.button("Change…");
          ui.button("Show");
        });

        let caption = format!("{} MB of thumbnails and previews", self.cache);
        let row = FormRow::new("Cache").caption(&caption).tile(Icon::TRASH);
        ui.form_row(fs, row, |ui| {
          if ui.disabled(self.cache == 0).id(CLEAR_CACHE).button("Clear…").clicked {
            self.ask = Some(Ask::ClearCache);
          }
        });

        // The task's progress under its text, clear of the descenders.
        let index = |ui: &mut Ui, s: TextStyle| {
          ui.text("Indexing 4,210 of 10,000 files", s);
          ui.col(Style::new().w(grow()).pads([4.0, 0.0, 0.0, 0.0]), |ui| ui.progress(Some(0.42)));
        };
        let row = FormRow::new("Index").caption(index).tile(Icon::DOC);
        ui.form_row(fs, row, |ui| ui.badge("Running", Tone::Accent));
      });

      ui.form_section(fs, "Diagnostics", |ui| {
        ui.form_row(fs, FormRow::new("Send crash reports"), |ui| {
          match self.reports {
            true => ui.badge("On", Tone::Success),
            false => ui.badge("Off", Tone::Warning),
          };

          ui.toggle(&mut self.reports, ())
        });
        ui.form_row(fs, FormRow::new("Logging"), |ui| ui.picker(&mut self.logging, &LOGGING));
      });

      ui.help("Help on these settings").id(HELP).form_footer(fs, |ui| {
        if ui.id(RESET_ALL).button("Reset All…").clicked {
          self.ask = Some(Ask::ResetAll);
        }
      });
    }

    /// About's head above its page: the icon, the name, the version, a line and a link.
    fn about_head(&mut self, ui: &mut Ui, fs: &FormStyle) {
      let t = ui.theme();
      let head =
        Style::new().w(grow()).pads([22.0, 20.0, 0.0, 20.0]).align(Align::Center, Align::Start);

      ui.col(head, |ui| {
        let accent = t.accent();
        ui.canvas(Style::new().w(px(64.0)).h(px(64.0)), move |f, r| app_icon(f, r, accent));
        ui.el(Style::new().h(px(10.0)));
        ui.text("Example", t.label().font(t.bold().size(20.0)));
        ui.el(Style::new().h(px(2.0)));
        ui.text("Version 1.0.0", fs.note());
        ui.el(Style::new().h(px(8.0)));
        ui.text("A demo of nimbo's settings form, dialog and widgets.", t.label());
        ui.el(Style::new().h(px(10.0)));
        ui.link("github.com/vladkens/nimbo");
      });
    }

    fn about(&mut self, ui: &mut Ui, fs: &FormStyle) {
      ui.form_section(fs, "Licenses", |ui| {
        for (name, what) in [("nimbo", "MIT License"), ("objc2", "MIT License")] {
          ui.form_row(fs, FormRow::new(name).caption(what), |ui| ui.icon_button(Icon::EXTERNAL));
        }
      });
    }

    /// The page under the toolbar, in its scroll area.
    fn page(&mut self, ui: &mut Ui, fs: &FormStyle) {
      let page = match self.page {
        Page::About => {
          self.about_head(ui, fs);
          fs.page().pads([20.0, 20.0, 20.0, 20.0])
        }
        _ => fs.page(),
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
          .button("Clear", ButtonRole::Destructive),
        Ask::ResetAll => Dialog::new("Reset all settings?")
          .message("Every setting goes back to how it was when the app was installed.")
          .button("Reset", ButtonRole::Confirm),
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
  fn app_icon(f: &mut Frame, r: Rect, accent: Color) {
    f.shadow(Rect::new(r.x, r.y + 1.0, r.w, r.h), 15.0, 3.0, [0.0, 0.0, 0.0, 0.2]);
    f.rect(r, accent, 15.0);
    let inner = Rect::new(r.x + 14.0, r.y + 14.0, r.w - 28.0, r.h - 28.0);
    Icon::CHECK.stroke(2.4).draw(f, inner, [1.0; 4]);
  }

  impl App for Settings {
    fn view(&mut self, ui: &mut Ui) {
      let mut a = ui.appearance();
      match self.look {
        Look::System => ui.follow_system(),
        Look::Light | Look::Dark => {
          a.scheme = if self.look == Look::Light { ColorScheme::Light } else { ColorScheme::Dark };
          ui.set_theme(Theme::system(a));
        }
      }

      // The window as high as the page, in the OS's font.
      ui.fit_window_height(WIDTH, 300.0, 720.0);
      let fs = FormStyle::plates(&ui.theme());
      ui.col(Style::new().grow().bg(fs.bg()), |ui| {
        ui.id(TABS).toolbar_tabs(&fs, &mut self.page, &PAGES);
        ui.id(("page", self.page as u8)).form_scroll(&fs, Style::new().grow(), |ui| {
          ui.col(Style::new().w(grow()), |ui| self.page(ui, &fs));
        });
      });
      self.confirm(ui);
    }
  }
}

#[expect(dead_code, reason = "this file uses a part of the helpers")]
mod common;

use std::time::Duration;

use app::*;
use common::{mid, px, reads};
use nimbo::a11y::Action;
use nimbo::{
  Appearance, Code, Event, Harness, Hotkey, Id, Key, Modifiers, NamedKey, Rect, Size, Theme, hotkey,
};

type H = Harness<Settings>;

fn rect(h: &H, name: &str) -> Rect {
  h.rect(Id::of(name)).unwrap_or_else(|| panic!("no {name}"))
}

fn key(h: &mut H, k: impl Into<Key>) {
  h.key(k, Modifiers::empty());
}

/// Turns to `page` by a click on its tab, laid out and resized.
fn turn(h: &mut H, page: Page) {
  let i = PAGES.iter().position(|p| p.value() == page).unwrap();
  h.click_id(Id::of(TABS).key(i));
  h.wait(Duration::from_millis(50));
  assert_eq!(h.app().page, page);
}

/// The mark of row `row`: its ⓘ, or the warning in its place.
fn mark(h: &H, row: &str) -> Rect {
  h.rect(Id::of(row).key("info")).unwrap_or_else(|| panic!("no mark in {row}"))
}

/// Rests the pointer on the mark of row `row` until its tooltip shows; the tooltip's text.
fn tip(h: &mut H, row: &str) -> String {
  let (x, y) = mid(mark(h, row));
  h.move_to(x, y);
  h.wait(Duration::from_millis(400));
  let tip = Id::of(row).key("info").key("tip");
  assert!(h.rect(tip).is_some(), "no tooltip on the mark of {row}");
  h.text(tip.nth(0)).unwrap_or_default().to_owned()
}

fn hotkey(h: &H) -> Option<String> {
  h.app().hotkey.map(|k| k.to_string())
}

/// The window in the light appearance, its first page laid out.
fn start() -> H {
  let mut h = Harness::new(Settings::default(), Size::new(WIDTH, 560.0), 2.0);
  h.event(Event::Appearance(Appearance::LIGHT));
  h.wait(Duration::from_millis(50));
  h
}

#[test]
fn general_switches_select_and_recorder() {
  let mut h = start();
  // The switch that is on is in the accent.
  let r = rect(&h, LOGIN);
  assert_eq!(h.render().pixel(r.x + 6.0, r.y + r.h / 2.0), px(Theme::PLATES_LIGHT.accent()));

  // Switches flip; one disabled by another setting doesn't.
  h.click_id(Id::of(LOGIN));
  assert!(!h.app().login);
  h.click_id(Id::of(UPDATES));
  h.frame();
  h.click_id(Id::of(DOWNLOAD));
  assert!(!h.app().updates && !h.app().download);

  // The select: letters typed while it has focus choose an option; open, its menu is over it
  // and letters move the highlight, Enter picks.
  let launch = Id::of(LAUNCH);
  h.set_focus(Some(launch));
  key(&mut h, 'o');
  assert_eq!(h.app().launch, Launch::Window);

  h.click_id(launch);
  h.frame();
  h.frame();
  let (menu, button) = (h.rect(launch.key("menu")).expect("open"), rect(&h, LAUNCH));
  assert!(menu.y < button.y && menu.y + menu.h > button.y + button.h, "{menu:?} over {button:?}");
  key(&mut h, 's');
  key(&mut h, NamedKey::Enter);
  h.frame();
  assert_eq!(h.app().launch, Launch::Hidden);
  assert!(h.rect(launch.key("menu")).is_none());

  // The recorder: a click records the next chord, then a modifier alone.
  let rec = Id::of(HOTKEY);
  h.click_id(rec);
  assert_eq!(h.focus_id(), Some(rec));
  h.key('k', Modifiers::CONTROL | Modifiers::SHIFT);
  assert_eq!(hotkey(&h).as_deref(), Some("ctrl+shift+k"));
  assert_ne!(h.focus_id(), Some(rec));
  h.click_id(rec);
  h.event(Event::Modifier { key: Code::AltRight, down: true });
  h.event(Event::Modifier { key: Code::AltRight, down: false });
  // Where the platform grabs no modifier alone (Linux) it records nothing, and Escape stops it.
  let lone = match hotkey::modifier_alone() {
    true => "right_option",
    false => {
      key(&mut h, NamedKey::Escape);
      "ctrl+shift+k"
    }
  };

  assert_eq!(hotkey(&h).as_deref(), Some(lone));
  // A plain key is taken and records nothing; a modifier with another one between isn't alone.
  h.click_id(rec);
  assert!(h.key('a', Modifiers::empty()));
  for (key, down) in [(Code::ShiftLeft, true), (Code::ShiftRight, true)] {
    h.event(Event::Modifier { key, down });
  }

  for (key, down) in [(Code::ShiftLeft, false), (Code::ShiftRight, false)] {
    h.event(Event::Modifier { key, down });
  }

  // Escape stops it unchanged, Backspace clears it.
  key(&mut h, NamedKey::Escape);
  assert_eq!(hotkey(&h).as_deref(), Some(lone));
  assert_ne!(h.focus_id(), Some(rec));
  h.click_id(rec);
  key(&mut h, NamedKey::Backspace);
  assert_eq!(h.app().hotkey, None);

  // A click elsewhere stops it; the reset puts the default back.
  h.click_id(rec);
  h.click_id(Id::of(LOGIN));
  key(&mut h, NamedKey::F5);
  assert_eq!(h.app().hotkey, None);
  h.frame();
  h.click_id(Id::of(RESET));
  assert_eq!(hotkey(&h).as_deref(), Some(DEFAULT_HOTKEY));
}

#[test]
fn appearance_row_width_slider_and_look() {
  let mut h = start();
  // Appearance: the switch keeps its width at the row's right edge, its description wraps
  // beside it onto lines of their own.
  turn(&mut h, Page::Appearance);
  let (sw, row) = (rect(&h, MOTION), rect(&h, MOTION_ROW));
  let caption = h.rect(Id::of(MOTION_ROW).key("caption")).unwrap();
  assert_eq!(sw.w, Theme::PLATES_LIGHT.sizes().switch().track[0]);
  assert!((row.x + row.w - 10.0 - (sw.x + sw.w)).abs() < 0.5, "{sw:?} in {row:?}");
  assert!(caption.h >= 28.0 && caption.x + caption.w < sw.x, "{caption:?}");

  // The slider takes a click between frames, drags to its ends and steps by a twentieth.
  let s = rect(&h, TEXT_SIZE);
  h.click(s.x + s.w / 2.0, s.y + s.h / 2.0);
  assert_eq!(h.app().text_size, 15.0);
  h.frame();

  h.press(s.x + 2.0, s.y + s.h / 2.0);
  h.frame();
  assert_eq!(h.app().text_size, 10.0);
  h.move_to(s.x + s.w + 20.0, s.y + s.h / 2.0);
  h.frame();
  h.release(s.x + s.w + 20.0, s.y + s.h / 2.0);
  assert_eq!(h.app().text_size, 20.0);

  h.set_focus(Some(Id::of(TEXT_SIZE)));
  key(&mut h, NamedKey::ArrowLeft);
  assert_eq!(h.app().text_size, 19.5);

  // The look applies at once.
  h.click_id(Id::of(LOOK).key(("item", 2usize)));
  h.wait(Duration::from_millis(50));
  assert!(h.render().pixel(4.0, h.size().h - 4.0)[0] < 64);
}

#[test]
fn an_inactive_window_greys_the_accent() {
  let mut h = start();
  turn(&mut h, Page::Appearance);
  // The slider's fill and the selected segment: the accent while the window is key, a grey
  // when another window is, as AppKit's controls.
  let (s, auto) = (rect(&h, TEXT_SIZE), h.rect(Id::of(LOOK).key(("item", 0usize))).unwrap());
  let spots = [(s.x + 12.0, s.y + s.h / 2.0), (auto.x + 4.0, auto.y + 4.0)];
  let at = |h: &mut H| spots.map(|(x, y)| h.render().pixel(x, y));
  let accent = px(Theme::PLATES_LIGHT.accent());
  assert_eq!(at(&mut h), [accent; 2]);
  h.event(Event::Focus(false));
  for [r, g, b, _] in at(&mut h) {
    assert!(r == g && g == b && r < 0xe0, "{r} {g} {b}");
  }

  h.event(Event::Focus(true));
  assert_eq!(at(&mut h), [accent; 2]);
}

#[test]
fn advanced_asks_in_a_dialog() {
  let mut h = start();
  h.app_mut().login = false;
  h.redraw();

  // Advanced: the dialog asks before clearing the cache, over a dimmed page.
  turn(&mut h, Page::Advanced);
  let confirm = Id::of(CONFIRM);
  let clear = rect(&h, CLEAR_CACHE);
  let ground = h.render().pixel(4.0, h.size().h - 4.0);
  h.click_id(Id::of(CLEAR_CACHE));
  h.wait(Duration::from_millis(200));
  assert!(h.rect(confirm.key("card")).is_some());
  let dimmed = h.render().pixel(4.0, h.size().h - 4.0);
  assert!(dimmed[0] < ground[0], "{dimmed:?} over {ground:?}");

  // A click on the page under it does nothing; Return chooses Cancel (the action is
  // destructive) and closes it.
  h.click(clear.x + 2.0, clear.y + 2.0);
  assert_eq!(h.app().ask, Some(Ask::ClearCache));
  key(&mut h, NamedKey::Enter);
  h.frame();
  assert_eq!((h.app().ask, h.app().cache), (None, 128));
  assert!(h.rect(confirm.key("card")).is_none());

  // Tab focuses the action, then Cancel, then the action again; Alt-Space goes on to the
  // window menu; Space chooses.
  h.click_id(Id::of(CLEAR_CACHE));
  h.frame();
  key(&mut h, NamedKey::Tab);
  assert_eq!(h.focus_id(), Some(confirm.key("action")));
  key(&mut h, NamedKey::Tab);
  assert_eq!(h.focus_id(), Some(confirm.key("cancel")));
  key(&mut h, NamedKey::Tab);
  assert_eq!(h.focus_id(), Some(confirm.key("action")));

  h.key(' ', Modifiers::ALT);
  assert_eq!(h.app().ask, Some(Ask::ClearCache));
  key(&mut h, ' ');
  h.frame();
  assert_eq!((h.app().ask, h.app().cache), (None, 0));

  // Reset All: Escape cancels, Return resets (the action is the default), keeping the page.
  h.click_id(Id::of(RESET_ALL));
  h.frame();
  key(&mut h, NamedKey::Escape);
  h.frame();
  assert_eq!((h.app().ask, h.app().resets), (None, 0));
  h.click_id(Id::of(RESET_ALL));
  h.frame();
  key(&mut h, NamedKey::Enter);
  h.frame();
  let a = h.app();
  assert_eq!((a.resets, a.cache, a.page, a.login), (1, 128, Page::Advanced, true));
}

#[test]
fn the_window_follows_the_page() {
  // It takes the page's height.
  let mut h = start();
  let general = h.size().h;
  assert!((400.0..720.0).contains(&general), "{general}");
  turn(&mut h, Page::About);
  assert!(h.size().h < general, "{} vs {general}", h.size().h);
}

#[test]
fn a_switch_reads_its_row_and_its_state() {
  let mut h = start();
  reads(&h.a11y(), &["switch \"Open at login\" on"]);
  h.click_id(Id::of(LOGIN));
  h.frame();
  reads(&h.a11y(), &["switch \"Open at login\" off"]);
}

#[test]
fn a_select_reads_its_value_and_its_open_menu() {
  let mut h = start();
  let caption = "desc \"Applies the next time the app starts.\"";
  reads(&h.a11y(), &[&format!("pop up button \"Language\" value \"System Default\" {caption}")]);
  h.click_id(Id::of(LANG));
  h.wait(Duration::from_millis(50));
  let lines = [
    &format!("pop up button \"Language\" value \"System Default\" expanded {caption}"),
    "menu focused active \"System Default\"",
    "menu item \"System Default\" selected",
    "menu item \"English\"",
  ];
  reads(&h.a11y(), &lines);
}

#[test]
fn the_slider_reads_its_number() {
  let mut h = start();
  turn(&mut h, Page::Appearance);
  reads(&h.a11y(), &["slider \"Text size\" 13 in 10..20"]);
  h.set_focus(Some(Id::of(TEXT_SIZE)));
  key(&mut h, NamedKey::ArrowRight);
  h.frame();
  reads(&h.a11y(), &["slider \"Text size\" 13.5 in 10..20 focused"]);
}

#[test]
fn the_recorder_reads_its_hotkey_and_the_icon_its_label() {
  let h = start();
  let shown = Hotkey::parse(DEFAULT_HOTKEY).unwrap().label();
  let lines = [
    &format!("button \"Show the app\" value \"{shown}\""),
    "button \"Reset\" disabled desc \"Back to the default shortcut\"",
  ];
  reads(&h.a11y(), &lines);
}

#[test]
fn the_dialog_reads_modal_with_its_buttons() {
  let mut h = start();
  turn(&mut h, Page::Advanced);
  h.click_id(Id::of(CLEAR_CACHE));
  h.wait(Duration::from_millis(200));
  let dump = h.a11y();
  reads(&dump, &["dialog \"Clear the cache?\" modal"]);
  // Its buttons inside it, the default one focused.
  let buttons = "\n  button \"Cancel\" focused\n  button \"Clear\"\n";
  assert!(dump.contains(buttons), "{dump}");
}

/// Opening, the dialog takes the focus, its default button focused to assistive technology;
/// closed, it gives the focus back to the button that opened it.
#[test]
fn the_dialog_takes_the_focus_and_gives_it_back() {
  let mut h = start();
  turn(&mut h, Page::Advanced);
  let (clear, confirm) = (Id::of(CLEAR_CACHE), Id::of(CONFIRM));
  assert!(h.act(clear, Action::Focus));
  key(&mut h, ' ');
  h.frame();
  assert_eq!(h.app().ask, Some(Ask::ClearCache));
  assert_eq!(h.focus_id(), Some(confirm.key("backdrop")));
  reads(&h.a11y(), &["button \"Cancel\" focused"]);

  key(&mut h, NamedKey::Escape);
  h.frame();
  assert_eq!((h.app().ask, h.focus_id()), (None, Some(clear)));
  reads(&h.a11y(), &["button \"Clear…\" focused"]);
}

/// Assistive technology presses a switch as a click does, the pointer left alone.
#[test]
fn an_assistive_press_flips_a_switch() {
  let mut h = start();
  assert!(h.act(Id::of(LOGIN), Action::Press));
  assert!(!h.app().login);
  // The pointer and the hover stay where they were: nowhere.
  assert!(h.mouse().is_none() && h.hover().is_empty());
  reads(&h.a11y(), &["switch \"Open at login\" off"]);
}

/// Increment and decrement step the slider as the arrows do.
#[test]
fn assistive_steps_move_the_slider() {
  let mut h = start();
  turn(&mut h, Page::Appearance);
  let size = Id::of(TEXT_SIZE);
  assert!(h.act(size, Action::Increment));
  assert_eq!(h.app().text_size, 13.5);
  h.act(size, Action::Decrement);
  h.act(size, Action::Decrement);
  assert_eq!(h.app().text_size, 12.5);
  reads(&h.a11y(), &["slider \"Text size\" 12.5 in 10..20"]);
}

/// A press opens the select's menu, a press on an item chooses it and closes the menu.
#[test]
fn assistive_presses_choose_in_a_select() {
  let mut h = start();
  let lang = Id::of(LANG);
  assert!(h.act(lang, Action::Press));
  reads(&h.a11y(), &["menu focused active \"System Default\"", "menu item \"English\""]);
  assert!(h.act(lang.key(("item", 1)), Action::Press));
  assert_eq!(h.app().lang, 1);
  assert!(h.rect(lang.key("menu")).is_none());
  let caption = "desc \"Applies the next time the app starts.\"";
  reads(&h.a11y(), &[&format!("pop up button \"Language\" value \"English\" {caption}")]);
}

/// Focus moves to an enabled control, not to a disabled one.
#[test]
fn an_assistive_focus_moves_the_focus() {
  let mut h = start();
  assert!(h.act(Id::of(LANG), Action::Focus));
  assert_eq!(h.focus_id(), Some(Id::of(LANG)));
  assert!(!h.act(Id::of(RESET), Action::Focus));
  assert_eq!(h.focus_id(), Some(Id::of(LANG)));
}

/// The focus reads only while the window has it: leaving and coming back change the tree, so
/// a screen reader is told of the focus on return.
#[test]
fn the_focus_reads_only_in_the_focused_window() {
  let mut h = start();
  assert!(h.act(Id::of(LANG), Action::Focus));
  let focused = h.a11y();
  assert!(focused.contains(" focused"), "{focused}");
  h.event(Event::Focus(false));
  let away = h.a11y();
  assert!(!away.contains(" focused"), "{away}");
  h.event(Event::Focus(true));
  assert_eq!(h.a11y(), focused);
}

/// An open dialog leaves the page behind it out of the tree, and a press there is refused; its
/// own buttons are pressed.
#[test]
fn a_press_behind_a_modal_dialog_is_refused() {
  let mut h = start();
  turn(&mut h, Page::Advanced);
  assert!(h.act(Id::of(CLEAR_CACHE), Action::Press));
  assert_eq!(h.app().ask, Some(Ask::ClearCache));
  let dump = h.a11y();
  assert!(dump.starts_with("dialog \"Clear the cache?\" modal\n"), "{dump}");
  assert!(!dump.contains("Reset"), "{dump}");
  assert!(!h.act(Id::of(RESET_ALL), Action::Press));
  assert_eq!(h.app().ask, Some(Ask::ClearCache));
  assert!(h.act(Id::of(CONFIRM).key("action"), Action::Press));
  assert_eq!((h.app().ask, h.app().cache), (None, 0));
}

/// A row's warning takes its ⓘ's place, as big, in the theme's warning colour with the "!" cut
/// out of it, in both appearances; it reads as a warning, and goes with what it warns of.
#[test]
fn a_warning_marks_its_row_in_the_warning_colour() {
  let mut h = start();
  let looks = [(Appearance::LIGHT, Theme::PLATES_LIGHT), (Appearance::DARK, Theme::PLATES_DARK)];
  for (look, t) in looks {
    h.event(Event::Appearance(look));
    h.wait(Duration::from_millis(50));
    let (r, img) = (mark(&h, LOGIN_ROW), h.render());

    // In its 24-unit view box: the fill either side of the "!"'s dot, and the ground through
    // the middle of its stem.
    let at = |x: f32, y: f32| img.pixel(r.x + x / 24.0 * r.w, r.y + y / 24.0 * r.h);
    assert_eq!(at(7.0, 17.0), px(t.warning()));
    assert_eq!(at(17.0, 17.0), px(t.warning()));
    let ground = (-2..=2).any(|i| at(12.0 + i as f32 * 0.2, 11.0) == px(t.plate()));
    assert!(ground, "the plate through the \"!\": {:?}", at(12.0, 11.0));

    let info = mark(&h, LAUNCH_ROW);
    assert_eq!((r.w, r.h), (info.w, info.h), "as big as the ⓘ");
  }

  let warning = format!("button \"Warning\" desc {LOGIN_WARNING:?}");
  reads(&h.a11y(), &[&warning, &format!("button \"Info\" desc {TIP_LAUNCH:?}")]);
  h.click_id(Id::of(LOGIN));
  h.frame();
  assert!(h.rect(Id::of(LOGIN_ROW).key("info")).is_none());
}

/// A row's warning shows its text on a rest; with the ⓘ's tip too, both in one tooltip, the
/// warning first, read so; without the warning, the ⓘ and its tip alone.
#[test]
fn a_warning_and_a_tip_show_in_one_tooltip() {
  let mut h = start();
  assert_eq!(tip(&mut h, LOGIN_ROW), LOGIN_WARNING);
  let both = format!("{HOTKEY_WARNING}\n\n{TIP_HOTKEY}");
  assert_eq!(tip(&mut h, HOTKEY_ROW), both);
  reads(&h.a11y(), &[&format!("button \"Warning\" desc {both:?}")]);
  // Another shortcut: nothing to warn of.
  h.app_mut().hotkey = Hotkey::parse("ctrl+shift+k").ok();
  h.redraw();
  h.frame();
  assert_eq!(tip(&mut h, HOTKEY_ROW), TIP_HOTKEY);
  reads(&h.a11y(), &[&format!("button \"Info\" desc {TIP_HOTKEY:?}")]);
}

/// A disabled row's ⓘ stays enabled: as dark as an enabled row's, it reads enabled beside the
/// row's disabled switch and shows its tip on a rest.
#[test]
fn a_disabled_rows_info_shows_its_tip() {
  let mut h = start();
  h.click_id(Id::of(UPDATES));
  h.frame();
  assert!(!h.app().updates);
  let img = h.render();
  let darkest = |r: Rect| {
    let px = (0..(r.w * 2.0) as i32).flat_map(|x| (0..(r.h * 2.0) as i32).map(move |y| (x, y)));
    px.map(|(x, y)| img.pixel(r.x + x as f32 / 2.0, r.y + y as f32 / 2.0)[0]).min().unwrap()
  };
  let (off, on) = (darkest(mark(&h, DOWNLOAD_ROW)), darkest(mark(&h, LAUNCH_ROW)));
  assert!(off.abs_diff(on) <= 8, "the ⓘ at {off} in a disabled row, {on} in an enabled one");
  let info = format!("button \"Info\" desc {TIP_DOWNLOAD:?}");
  reads(&h.a11y(), &[&info, "switch \"Download them in the background\" off disabled"]);
  assert_eq!(tip(&mut h, DOWNLOAD_ROW), TIP_DOWNLOAD);
}
