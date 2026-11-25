// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Diagnostics window, following the system's changes ([nimbo::system]): a microphone
//! plugged in or out, the default input changed, the lid opened or closed. It reads again on
//! each `Event::System` and redraws: the lid, the changes of each kind and the last. The app's
//! hook given to `Main::watch` keeps the same [Seen] for the tray's menu.
//!
//! `cargo run --release --example dictation -- diagnostics --shot <file.png>` renders it alone:
//! the window after a change of the lid.

use nimbo::system::Change;
use nimbo::{App, Config, Cx, Event, FormRow, FormStyle, Harness, Style, Ui, grow};

pub const KINDS: [Change; 3] = [Change::AudioInputs, Change::DefaultInput, Change::Lid];
pub const LID: &str = "lid";
pub const COUNT: &str = "count";
pub const LAST: &str = "last";

/// What came: the changes of each kind, the last, and the lid as read after it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seen {
  pub counts: [u32; 3],
  pub last: Option<Change>,
  pub lid: Option<bool>,
}

impl Seen {
  pub fn new(lid: Option<bool>) -> Self {
    Self { counts: [0; 3], last: None, lid }
  }

  /// `change` came; the lid read again after it is `lid`.
  pub fn add(&mut self, change: Change, lid: Option<bool>) {
    if let Some(i) = KINDS.iter().position(|&k| k == change) {
      self.counts[i] += 1;
    }

    self.last = Some(change);
    self.lid = lid;
  }
}

pub fn lid_label(lid: Option<bool>) -> &'static str {
  match lid {
    Some(true) => "Closed",
    Some(false) => "Open",
    None => "None",
  }
}

pub fn kind_label(kind: Change) -> &'static str {
  match kind {
    Change::AudioInputs => "Audio inputs",
    Change::DefaultInput => "Default input",
    Change::Lid => "Lid",
    _ => "Other",
  }
}

/// The window: what came, read again on each change.
pub struct Diagnostics {
  pub seen: Seen,
  /// How the lid is read: [nimbo::hardware::lid_closed], a fake in the tests.
  pub read_lid: fn() -> Option<bool>,
}

impl Diagnostics {
  pub fn new(read_lid: fn() -> Option<bool>) -> Self {
    Self { seen: Seen::new(read_lid()), read_lid }
  }
}

impl App for Diagnostics {
  fn event(&mut self, _: &mut Cx, ev: &Event) -> bool {
    let Event::System(change) = ev else { return false };
    self.seen.add(*change, (self.read_lid)());
    true
  }

  fn view(&mut self, ui: &mut Ui) {
    let fs = FormStyle::plates(&ui.theme());
    let s = &self.seen;
    ui.col(Style::new().grow().bg(fs.bg), |ui| {
      ui.form_scroll(&fs, Style::new().grow(), |ui| {
        ui.col(fs.page.pads([20.0, 20.0, 20.0, 20.0]).w(grow()), |ui| {
          ui.form_section(&fs, "Hardware", |ui| {
            ui.form_row(&fs, FormRow::new("Lid"), |ui| ui.id(LID).text(lid_label(s.lid), fs.note));
          });
          ui.form_section(&fs, "Changes", |ui| {
            for (i, &kind) in KINDS.iter().enumerate() {
              let row = FormRow::new(kind_label(kind));
              ui.form_row(&fs, row, |ui| ui.id((COUNT, i as u8)).text(s.counts[i], fs.note));
            }

            let last = s.last.map_or("None", kind_label);
            ui.form_row(&fs, FormRow::new("Last"), |ui| ui.id(LAST).text(last, fs.note));
          });
        });
      });
    });
  }
}

/// The window as the app opens it.
pub fn config() -> Config {
  Config::new("Diagnostics", 420.0, 310.0)
}

/// The shot: the lid closed a moment ago, after two audio changes.
pub fn shot(h: &mut Harness<Diagnostics>) {
  h.app_mut().read_lid = || Some(true);
  h.app_mut().seen.add(Change::AudioInputs, Some(false));
  h.app_mut().seen.add(Change::DefaultInput, Some(false));
  h.event(Event::System(Change::Lid));
  h.frame();
}
