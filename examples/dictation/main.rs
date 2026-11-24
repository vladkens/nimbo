// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A menu-bar dictation app with no window at launch: an icon in the menu bar (macOS) or the
//! tray (Windows, Linux) whose menu starts and stops a dictation and opens the [history],
//! [settings] (its Account tab: [account]) and [diagnostics] windows, with launch at login as a
//! check mark and the lid and audio changes the system reported ([nimbo::system]). A global
//! hotkey starts a dictation from any app: the [hud] shows at the pointer's screen while it
//! records; the second press stops it, "hears" a canned line, pastes it where the user types
//! ([nimbo::paste]) and adds it to History. Stopped from the menu, it pastes three seconds later,
//! counting down beside the icon ([Tray::set_title]), time to go back to a field. On Linux it
//! writes its launcher entry at start ([desktop]). It runs once: a second launch has the first
//! show Settings ([instance]), and ends. Each change of the system goes to stderr as
//! `system: <Kind> lid_closed=<…>`, and with `NIMBO_TRACE` each notice the OS gave as
//! `nimbo: system raw <Kind>`, for the VM checks.
//!
//! `cargo run --release --example dictation` runs the app; `-- <window> [--shot <file.png>]`
//! opens one window alone (settings, account, history) or renders it without one (also
//! diagnostics and hud). The hotkey is ⌃⌥Space on macOS, Ctrl+Alt+Space elsewhere (on Wayland the
//! desktop asks first, as it does for the paste's keys).

// A Windows app, not a console one: no terminal opens with it.
#![cfg_attr(windows, windows_subsystem = "windows")]

pub mod account;
pub mod diagnostics;
pub mod history;
pub mod hud;
pub mod settings;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use nimbo::desktop::Launcher;
use nimbo::hotkey::{Edge, Hotkeys};
use nimbo::instance::{self, Claim, Instance};
use nimbo::paste::Paste;
use nimbo::tray::{Item, Menu, Tray, TrayIcon};
use nimbo::{
  App, AppInfo, Config, Hotkey, Main, Png, Presence, Shortcut, Window, autostart, desktop, focus,
  hardware, paste, tray,
};

use self::account::Account;
use self::diagnostics::{Diagnostics, KINDS, Seen, lid_label};
use self::history::History;
use self::hud::Hud;
use self::settings::Settings;

pub const INFO: AppInfo = AppInfo {
  id: "com.example.nimbo-dictation",
  name: "Nimbo Dictation",
  presence: Presence::Accessory,
};
pub const HOTKEY: &str = "ctrl+option+space";
/// A ring, filled even-odd.
pub const ICON: &str = "M9 1a8 8 0 1 0 0 16A8 8 0 1 0 9 1Zm0 4a4 4 0 1 1 0 8 4 4 0 1 1 0-8Z";
/// What a dictation "hears", in turn.
pub const LINES: [&str; 3] = [
  "Hello from nimbo",
  "Remind me to water the plants on Friday",
  "The meeting moves to half past three",
];

const USAGE: &str = "usage: dictation [settings|account|history [--shot <file.png>] | \
                     diagnostics|hud --shot <file.png>] [options]";

/// What the menu shows: whether a dictation records, launch at login (`None` where the app
/// can't: a bare binary on macOS), the hotkey's label and what the system reported.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
  pub recording: bool,
  pub login: Option<bool>,
  pub hotkey: String,
  pub seen: Seen,
}

/// What the tray's menu does; its status lines do nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
  None,
  Dictate,
  History,
  Settings,
  Diagnostics,
  Login,
  Quit,
}

pub fn menu(s: &Shown) -> Menu<Action> {
  let (dictate, status) = match (s.recording, s.hotkey.as_str()) {
    (false, "") => ("Start Dictation", "No hotkey".into()),
    (false, key) => ("Start Dictation", format!("{key} starts a dictation")),
    (true, "") => ("Stop Dictation", "Recording".into()),
    (true, key) => ("Stop Dictation", format!("Recording: {key} stops it")),
  };

  let audio = s.seen.counts[0] + s.seen.counts[1];

  Menu::new(vec![
    Item::button(Action::None, &status).enabled(false),
    Item::button(Action::Dictate, dictate),
    Item::Divider,
    Item::button(Action::History, "History…"),
    Item::button(Action::Settings, "Settings…").key(Shortcut::char(',').cmd()),
    Item::Divider,
    Item::button(Action::None, &format!("Lid: {}", lid_label(s.seen.lid))).enabled(false),
    Item::button(Action::None, &format!("Audio changes: {audio}")).enabled(false),
    Item::button(Action::Diagnostics, "Diagnostics…"),
    Item::Divider,
    Item::toggle(Action::Login, "Launch at Login", s.login == Some(true))
      .enabled(s.login.is_some()),
    Item::Divider,
    Item::button(Action::Quit, "Quit").key(Shortcut::char('q').cmd()),
  ])
}

/// The dictations: whether one records, what they heard, and how a line is pasted.
pub struct Dictation {
  pub recording: bool,
  /// What each heard, oldest first.
  pub said: Vec<String>,
  /// What was heard since the last paste: a dictation stopped during another's countdown waits
  /// for it.
  pub unpasted: Vec<String>,
  /// Pastes a line where the user types: [paste_here], a fake in the tests.
  pub paste: fn(&str),
}

impl Default for Dictation {
  fn default() -> Self {
    Self { recording: false, said: Vec::new(), unpasted: Vec::new(), paste: paste_here }
  }
}

#[derive(Default)]
pub struct State {
  pub dictation: Dictation,
  /// What the system reported, for the menu.
  pub seen: Seen,
  pub history: Option<Window<History>>,
  pub settings: Option<Window<Settings>>,
  pub diagnostics: Option<Window<Diagnostics>>,
  hud: Option<Window<Hud>>,
  hotkeys: Option<Hotkeys>,
  /// For the paste's countdown, from the main queue.
  tray: Option<Tray<Action>>,
  /// A paste counts down: another waits for its end.
  pasting: bool,
}

pub type Shared = Rc<RefCell<State>>;

fn shown(state: &Shared) -> Shown {
  let s = state.borrow();
  let login = autostart::state(&INFO).ok().map(|st| st.wanted());
  // Read each time: the listener may fail later (Linux: the user didn't approve the shortcut).
  let status = s.hotkeys.as_ref().map(|h| h.status());
  let bound = status.filter(|st| st.listener_error.is_none());
  let hotkey = bound.and_then(|st| st.slots.first()?.hotkey).map(|h| h.label()).unwrap_or_default();
  Shown { recording: s.dictation.recording, login, hotkey, seen: s.seen.clone() }
}

/// Starts a dictation, or stops it: then what it heard, kept for History.
pub fn dictate(state: &Shared) -> Option<String> {
  let d = &mut state.borrow_mut().dictation;
  d.recording = !d.recording;
  if d.recording {
    return None;
  }

  let line = LINES[d.said.len() % LINES.len()].to_owned();
  d.said.push(line.clone());
  d.unpasted.push(line.clone());
  Some(line)
}

/// Pastes what was heard since the last paste, the lines joined by spaces; the state's borrow
/// ends before the paste, which may run the loop.
pub fn paste_unpasted(state: &Shared) {
  let (lines, paste) = {
    let d = &mut state.borrow_mut().dictation;
    (std::mem::take(&mut d.unpasted), d.paste)
  };
  if !lines.is_empty() {
    paste(&lines.join(" "));
  }
}

/// History with what the dictations heard on top, as the window opens.
pub fn history_of(state: &Shared) -> History {
  let mut h = History::default();
  for line in &state.borrow().dictation.said {
    h.add(line.clone());
  }

  h
}

/// A window of the state's while it is open; the state's borrow ends before the caller opens or
/// closes it, which borrows the state again.
pub fn open_window<A: App + 'static>(
  state: &Shared,
  which: fn(&State) -> &Option<Window<A>>,
) -> Option<Window<A>> {
  which(&state.borrow()).clone().filter(|w| w.is_open())
}

/// Shows History, opening it when closed.
fn show_history(main: &mut Main, state: &Shared) {
  if let Some(w) = open_window(state, |s| &s.history) {
    return w.front();
  }

  let w = main.open(&history::config(), history_of(state));
  w.on_close(|_| STATE.with(|s| s.borrow_mut().history = None));
  state.borrow_mut().history = Some(w);
}

/// Shows Settings, opening it when closed.
fn show_settings(main: &mut Main, state: &Shared) {
  if let Some(w) = open_window(state, |s| &s.settings) {
    return w.front();
  }

  let w = main.open(&settings::config(), Settings::default());
  w.on_close(|_| STATE.with(|s| s.borrow_mut().settings = None));
  state.borrow_mut().settings = Some(w);
}

/// Shows Diagnostics, opening it when closed with what the system reported so far.
fn show_diagnostics(main: &mut Main, state: &Shared) {
  if let Some(w) = open_window(state, |s| &s.diagnostics) {
    return w.front();
  }

  let seen = state.borrow().seen.clone();
  let w = main.open(&diagnostics::config(), Diagnostics { seen, read_lid: hardware::lid_closed });
  w.on_close(|_| STATE.with(|s| s.borrow_mut().diagnostics = None));
  state.borrow_mut().diagnostics = Some(w);
}

/// Starts a dictation, showing the HUD; or stops it, hiding the HUD, adding the line to an open
/// History and pasting it `secs` from now, with what was heard meanwhile.
fn toggle_dictation(main: &mut Main, state: &Shared, secs: u64) {
  let hud = state.borrow().hud.clone();
  let Some(line) = dictate(state) else {
    if let Some(hud) = hud {
      // A new recording: its time from zero.
      hud.with(|h| *h = Hud::default());
      hud.front();
    }

    return;
  };

  if let Some(hud) = hud {
    hud.hide();
  }

  if let Some(w) = open_window(state, |s| &s.history) {
    w.with(|h| h.add(line));
  }

  if !state.borrow().pasting {
    paste_in(main, secs);
  }
}

/// Pastes `text` where the user types now: the clipboard comes back after the paste, unless
/// something else was copied meanwhile.
fn paste_here(text: &str) {
  let opts = Paste { restore: true, target: focus::current(), deadline: None };
  if let Err(e) = paste::text(text, opts) {
    eprintln!("paste: {e}");
  }
}

/// The seconds left to the paste beside the icon, each second, then the paste. It counts as
/// pasting until the paste ends (it may run the loop): what is heard meanwhile goes after it.
fn paste_in(main: &mut Main, secs: u64) {
  let tray = STATE.with(|s| {
    let mut s = s.borrow_mut();
    s.pasting = true;
    s.tray.clone()
  });
  let title = if secs == 0 { String::new() } else { secs.to_string() };
  if let Some(tray) = tray {
    tray.set_title(&title);
  }

  if secs > 0 {
    return main.proxy().after(Duration::from_secs(1), move |main| paste_in(main, secs - 1));
  }

  STATE.with(paste_unpasted);
  let more = STATE.with(|s| {
    let mut s = s.borrow_mut();
    s.pasting = false;
    !s.dictation.unpasted.is_empty()
  });
  if more {
    paste_in(main, 1);
  }
}

thread_local! {
  /// The app's state, on the main thread, where the hotkey's work is posted.
  static STATE: Shared = Shared::default();
}

fn start(main: &mut Main, instance: Option<Instance>) {
  // Nothing off Linux; there the entry is written once, again when the executable moves, and
  // only then is the icon decoded.
  let icon = Png::icon(include_bytes!("../../docs/brand/mark-iris.png"));
  let launcher =
    Launcher { comment: "A dictation app made with nimbo", categories: &["Utility"], icon };
  if let Err(e) = desktop::install(&INFO, &launcher) {
    eprintln!("desktop: {e}");
  }

  let state = STATE.with(Rc::clone);
  let tray = Tray::new(TrayIcon::Path { size: 18.0, d: ICON.into(), even_odd: true }, INFO.name);
  if let Err(e) = tray::available() {
    eprintln!("tray: {e}");
  }

  // The HUD waits hidden for a dictation.
  let hud = main.open(&hud::CONFIG, Hud::default());

  // The hotkey's edges come on a thread of their own: a press goes to the main thread.
  let proxy = main.proxy();
  let mut hotkeys = Hotkeys::new(move |edge| {
    if let Edge::Down { repeat: false, .. } = edge {
      proxy.post(|main| STATE.with(|s| toggle_dictation(main, s, 0)));
    }
  });
  let hotkey = Hotkey::parse(HOTKEY).map_err(|e| e.to_string());
  if let Err(e) = hotkey.and_then(|h| hotkeys.set(&[Some(h)])) {
    eprintln!("hotkey: {e}");
  }

  // The listener: the portal on Linux; on macOS a chord works without the event tap, which
  // would ask for Accessibility access.
  if !cfg!(target_os = "macos")
    && let Err(e) = hotkeys.start()
  {
    eprintln!("hotkey: {e}");
  }

  if let Some(e) = hotkeys.status().listener_error {
    eprintln!("hotkey: {e}");
  }

  {
    let mut s = state.borrow_mut();
    s.seen = Seen::new(hardware::lid_closed());
    s.hotkeys = Some(hotkeys);
    s.tray = Some(tray.clone());
    s.hud = Some(hud);
  }

  let refresh = |t: &Tray<Action>, state: &Shared| t.set_menu(&menu(&shown(state)));
  refresh(&tray, &state);

  // The hook: what lives outside the windows, before they hear it.
  let t = tray.clone();
  let watched = main.watch(&KINDS, move |_, change| {
    let lid = hardware::lid_closed();
    eprintln!("system: {change:?} lid_closed={lid:?}");
    STATE.with(|s| {
      s.borrow_mut().seen.add(change, lid);
      refresh(&t, s);
    });
  });
  if let Err(e) = watched {
    eprintln!("system: {e}");
  }

  let s = state.clone();
  tray.on_open(move |_, t| refresh(t, &s));
  tray.on_item(move |main, t, action| {
    match action {
      Action::Dictate => toggle_dictation(main, &state, 3),
      Action::History => show_history(main, &state),
      Action::Settings => show_settings(main, &state),
      Action::Diagnostics => show_diagnostics(main, &state),
      Action::Login => {
        let on = !shown(&state).login.unwrap_or(false);
        if let Err(e) = autostart::set(&INFO, on) {
          eprintln!("launch at login: {e}");
        }
      }
      Action::Quit => main.quit(),
      Action::None => {}
    }

    refresh(t, &state);
  });

  // A second launch's "open" shows Settings, as opening the app again does on macOS.
  main.on_reopen(|main| STATE.with(|s| show_settings(main, s)));
  // "quit" (an uninstaller's) quits after the handler: the tray has no commands of its own.
  if let Some(instance) = instance
    && let Err(e) = main.serve(instance, |_, _| {})
  {
    eprintln!("instance: {e}");
  }

  // The tray lives as long as the app; the hotkeys are in its state.
  main.on_quit(move |_| {
    drop(tray);
    STATE.with(|s| s.borrow_mut().tray = None);
  });
}

fn run() {
  // One instance: a second launch asks the first to show Settings, and ends.
  let instance = match instance::claim(&INFO) {
    Ok(Claim::Primary(instance)) => Some(instance),
    Ok(Claim::Running) => {
      return instance::send(&INFO, instance::OPEN).unwrap_or_else(|e| eprintln!("instance: {e}"));
    }
    Err(e) => {
      eprintln!("instance: {e}");
      None
    }
  };

  nimbo::app(INFO, |main| start(main, instance));
}

fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let Some((window, rest)) = args.split_first() else { return run() };
  let lid = hardware::lid_closed;
  match window.as_str() {
    "settings" => nimbo::run_args(settings::config(), Settings::default(), settings::shot, rest),
    "account" => {
      let cfg = Config::new("Account", account::SIZE.w, account::SIZE.h);
      nimbo::run_args(cfg, Account::new(), account::shot, rest)
    }
    "history" => nimbo::run_args(history::config(), History::default(), history::shot, rest),
    // Only their shots: the system's changes reach Diagnostics through the app's watch, and
    // the HUD alone would wait hidden.
    "diagnostics" if rest.iter().any(|a| a == "--shot") => {
      nimbo::run_args(diagnostics::config(), Diagnostics::new(lid), diagnostics::shot, rest)
    }
    "hud" if rest.iter().any(|a| a == "--shot") => {
      nimbo::run_args(hud::CONFIG, Hud::default(), hud::shot, rest)
    }
    _ => {
      eprintln!("unknown window {window}\n{USAGE}");
      std::process::exit(2);
    }
  }
}
