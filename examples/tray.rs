// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A tray app with no window of its own: an icon in the menu bar (macOS) or the tray (Windows,
//! Linux) whose menu opens a counter window and shows its count, a global hotkey that opens or
//! closes that window from any app, and launch at login as a check mark.
//!
//! `cargo run --release --example tray [-- --shot <file.png>]`; the shot is the counter window.
//! The hotkey is ⌃⌥Space on macOS, Ctrl+Alt+Space elsewhere (on Linux the desktop asks first).

// A Windows app, not a console one: no terminal opens with it.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "counter.rs"]
// The counter sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod counter;

use std::cell::RefCell;
use std::rc::Rc;

use counter::Counter;
use nimbo::hotkey::{Edge, Hotkeys};
use nimbo::tray::{Item, Menu, Tray, TrayIcon};
use nimbo::{AppInfo, Config, Main, Presence, Window, autostart};

pub const INFO: AppInfo =
  AppInfo { id: "com.example.nimbo-tray", name: "Nimbo Tray", presence: Presence::Accessory };
pub const HOTKEY: &str = "ctrl+option+space";
/// A ring, filled even-odd.
pub const ICON: &str = "M9 1a8 8 0 1 0 0 16A8 8 0 1 0 9 1Zm0 4a4 4 0 1 1 0 8 4 4 0 1 1 0-8Z";

/// What the menu shows: the window's count while it is open, and launch at login (`None` where
/// the app can't: a bare binary on macOS).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
  pub count: Option<i64>,
  pub login: Option<bool>,
  pub hotkey: String,
}

pub fn menu(s: &Shown) -> Menu {
  let (toggle, status) = match s.count {
    Some(n) => ("Close Counter", format!("Count: {n}")),
    None if s.hotkey.is_empty() => ("Open Counter", "No hotkey".into()),
    None => ("Open Counter", format!("{} opens the counter", s.hotkey)),
  };

  Menu::new(vec![
    Item::new("status", &status).enabled(false),
    Item::new("toggle", toggle),
    Item::separator(),
    Item::new("login", "Launch at Login").checked(s.login == Some(true)).enabled(s.login.is_some()),
    Item::separator(),
    Item::new("quit", "Quit").key("cmd+q"),
  ])
}

#[derive(Default)]
struct State {
  window: Option<Window<Counter>>,
  hotkey: String,
}

type Shared = Rc<RefCell<State>>;

fn shown(state: &Shared) -> Shown {
  let s = state.borrow();
  let count = s.window.as_ref().and_then(|w| w.with(|c| c.count));
  let login = autostart::state(&INFO).ok().map(|st| st.wanted());
  Shown { count, login, hotkey: s.hotkey.clone() }
}

/// Opens the counter, or closes it when open.
fn toggle(main: &mut Main, state: &Shared) {
  let open = state.borrow().window.clone().filter(|w| w.is_open());
  match open {
    Some(w) => w.close(),
    None => {
      let w = main.open(&Config::new("Counter", 360.0, 300.0), Counter::default());
      let gone = state.clone();
      w.on_close(move |_| gone.borrow_mut().window = None);
      state.borrow_mut().window = Some(w);
    }
  }
}

thread_local! {
  /// The app's state, on the main thread, where the hotkey's work is posted.
  static STATE: Shared = Shared::default();
}

fn start(main: &mut Main) {
  let state = STATE.with(Rc::clone);
  let tray = Tray::new(TrayIcon::Path { size: 18.0, d: ICON.into(), even_odd: true }, INFO.name);
  if let Err(e) = nimbo::tray::available() {
    eprintln!("tray: {e}");
  }

  // The hotkey's edges come on a thread of their own: a press goes to the main thread.
  let proxy = main.proxy();
  let mut hotkeys = Hotkeys::new(move |edge| {
    if let Edge::Down { repeat: false, .. } = edge {
      proxy.post(|main| STATE.with(|s| toggle(main, s)));
    }
  });
  match hotkeys.set(HOTKEY) {
    Ok(()) => {
      state.borrow_mut().hotkey = hotkeys.status().hotkey.map(|h| h.label()).unwrap_or_default()
    }
    Err(e) => eprintln!("hotkey: {e}"),
  }

  // The listener: the portal on Linux; on macOS a chord works without the event tap, which
  // would ask for Accessibility access.
  if !cfg!(target_os = "macos")
    && let Err(e) = hotkeys.start()
  {
    eprintln!("hotkey: {e}");
  }

  let refresh = |t: &Tray, state: &Shared| t.set_menu(&menu(&shown(state)));
  refresh(&tray, &state);
  let s = state.clone();
  tray.on_open(move |_, t| refresh(t, &s));
  tray.on_item(move |main, t, id| {
    match id {
      "toggle" => toggle(main, &state),
      "login" => {
        let on = !shown(&state).login.unwrap_or(false);
        if let Err(e) = autostart::set(&INFO, on) {
          eprintln!("launch at login: {e}");
        }
      }
      "quit" => main.quit(),
      _ => {}
    }

    refresh(t, &state);
  });

  // The tray and the hotkeys live as long as the app.
  let keep = (tray, hotkeys);
  main.on_quit(move |_| drop(keep));
}

fn main() {
  if std::env::args().any(|a| a == "--shot") {
    return nimbo::run(Config::new("Counter", 360.0, 300.0), Counter::default(), |h| _ = h.frame());
  }

  nimbo::app(INFO, start);
}
