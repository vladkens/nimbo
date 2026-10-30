// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A tray app that follows the system's changes ([nimbo::system]): a microphone plugged in or
//! out, the default input changed, the lid opened or closed. The hook given to `Main::watch`
//! builds the tray's menu again (the lid and the count of audio changes); the window reads
//! again on each `Event::System` and redraws: the lid, the changes of each kind and the last.
//! Each change goes to stderr as `system: <Kind> lid_closed=<…>`, and with `NIMBO_TRACE` each
//! notice the OS gave as `nimbo: system raw <Kind>`, for the VM checks.
//!
//! `cargo run --release --example system [-- --shot <file.png>]`; the shot is the window after
//! a change of the lid.

// A Windows app, not a console one: no terminal opens with it.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::cell::RefCell;
use std::rc::Rc;

use nimbo::system::Change;
use nimbo::tray::{Item, Menu, Tray, TrayIcon};
use nimbo::{
  App, AppInfo, Config, Cx, Event, FormRow, FormStyle, Main, Presence, Style, Ui, Window, grow,
  hardware,
};

pub const INFO: AppInfo =
  AppInfo { id: "com.example.nimbo-system", name: "Nimbo System", presence: Presence::Accessory };
pub const KINDS: [Change; 3] = [Change::AudioInputs, Change::DefaultInput, Change::Lid];
pub const LID: &str = "lid";
pub const COUNT: &str = "count";
pub const LAST: &str = "last";
/// Two plates of rows.
pub const ICON: &str = "M3 3h12v5H3zM3 10h12v5H3z";

/// What came: the changes of each kind, the last, and the lid as read after it.
#[derive(Clone, Debug, PartialEq)]
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

/// The tray's menu: the lid and the audio changes, then the window and Quit.
pub fn menu(s: &Seen) -> Menu {
  let audio = s.counts[0] + s.counts[1];
  Menu::new(vec![
    Item::new("lid", &format!("Lid: {}", lid_label(s.lid))).enabled(false),
    Item::new("audio", &format!("Audio changes: {audio}")).enabled(false),
    Item::separator(),
    Item::new("show", "Show Window"),
    Item::separator(),
    Item::new("quit", "Quit").key("cmd+q"),
  ])
}

/// The window: what came, read again on each change.
pub struct System {
  pub seen: Seen,
  /// How the lid is read: [hardware::lid_closed], a fake in the tests.
  pub read_lid: fn() -> Option<bool>,
}

impl System {
  pub fn new(read_lid: fn() -> Option<bool>) -> Self {
    Self { seen: Seen::new(read_lid()), read_lid }
  }
}

impl App for System {
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

fn config() -> Config {
  Config::new("System", 420.0, 310.0)
}

#[derive(Default)]
struct State {
  window: Option<Window<System>>,
  seen: Option<Seen>,
}

thread_local! {
  static STATE: Rc<RefCell<State>> = Rc::default();
}

/// Shows the window, opening it when closed.
fn show(main: &mut Main) {
  let open = STATE.with(|s| s.borrow().window.clone().filter(|w| w.is_open()));
  match open {
    Some(w) => w.front(),
    None => {
      let w = main.open(&config(), System::new(hardware::lid_closed));
      w.on_close(|_| STATE.with(|s| s.borrow_mut().window = None));
      STATE.with(|s| s.borrow_mut().window = Some(w));
    }
  }
}

fn start(main: &mut Main) {
  let tray = Tray::new(TrayIcon::Path { size: 18.0, d: ICON.into(), even_odd: false }, INFO.name);
  if let Err(e) = nimbo::tray::available() {
    eprintln!("tray: {e}");
  }

  let seen = Seen::new(hardware::lid_closed());
  tray.set_menu(&menu(&seen));
  STATE.with(|s| s.borrow_mut().seen = Some(seen));
  // The hook: what lives outside the windows, before they hear it.
  let t = tray.clone();
  let watched = main.watch(&KINDS, move |_, change| {
    let lid = hardware::lid_closed();
    eprintln!("system: {change:?} lid_closed={lid:?}");
    let seen = STATE.with(|s| {
      let mut s = s.borrow_mut();
      let seen = s.seen.get_or_insert_with(|| Seen::new(lid));
      seen.add(change, lid);
      seen.clone()
    });
    t.set_menu(&menu(&seen));
  });
  if let Err(e) = watched {
    eprintln!("system: {e}");
  }

  tray.on_item(|main, _, id| match id {
    "show" => show(main),
    "quit" => main.quit(),
    _ => {}
  });
  show(main);
  main.on_reopen(show);
  main.on_quit(move |_| drop(tray));
}

fn main() {
  if std::env::args().any(|a| a == "--shot") {
    // The lid closed a moment ago, after two audio changes.
    let mut app = System::new(|| Some(true));
    app.seen.add(Change::AudioInputs, Some(false));
    app.seen.add(Change::DefaultInput, Some(false));
    return nimbo::run(config(), app, |h| {
      h.event(Event::System(Change::Lid));
      h.frame();
    });
  }

  nimbo::app(INFO, start);
}
