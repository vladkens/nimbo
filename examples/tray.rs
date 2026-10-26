// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A tray app with no window of its own: an icon in the menu bar (macOS) or the tray (Windows,
//! Linux) whose menu opens a counter window and shows its count, a global hotkey that opens or
//! closes that window from any app, launch at login as a check mark, and an item that pastes a
//! line where the user types three seconds later, as a dictation app does ([paste]). On Linux it
//! writes its launcher entry at start ([desktop]), so the dock shows its icon for the window.
//!
//! `cargo run --release --example tray [-- --shot <file.png>]`; the shot is the counter window.
//! The hotkey is ⌃⌥Space on macOS, Ctrl+Alt+Space elsewhere (on Wayland the desktop asks first,
//! as it does for the paste's keys).

// A Windows app, not a console one: no terminal opens with it.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "counter.rs"]
// The counter sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod counter;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use counter::Counter;
use nimbo::desktop::Launcher;
use nimbo::hotkey::{Edge, Hotkeys};
use nimbo::tray::{Item, Menu, Tray, TrayIcon};
use nimbo::{
  AppInfo, Config, Main, Presence, Shortcut, SystemClipboard, Window, autostart, desktop,
};

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
    Item::new("paste", "Paste a Line in 3 Seconds"),
    Item::separator(),
    Item::new("quit", "Quit").key("cmd+q"),
  ])
}

#[derive(Default)]
struct State {
  window: Option<Window<Counter>>,
  hotkeys: Option<Hotkeys>,
}

type Shared = Rc<RefCell<State>>;

fn shown(state: &Shared) -> Shown {
  let s = state.borrow();
  let count = s.window.as_ref().and_then(|w| w.with(|c| c.count));
  let login = autostart::state(&INFO).ok().map(|st| st.wanted());
  // Read each time: the listener may fail later (Linux: the user didn't approve the shortcut).
  let status = s.hotkeys.as_ref().map(|h| h.status());
  let bound = status.filter(|st| st.error.is_none() && st.listener_error.is_none());
  let hotkey = bound.and_then(|st| st.hotkey).map(|h| h.label()).unwrap_or_default();
  Shown { count, login, hotkey }
}

/// Opens the counter, or closes it when open.
fn toggle(main: &mut Main, state: &Shared) {
  let open = state.borrow().window.clone().filter(|w| w.is_open());
  match open {
    Some(w) => w.close(),
    None => {
      let cfg = Config::new("Counter", 360.0, 300.0).icon(64, &counter::ICON);
      let w = main.open(&cfg, Counter::default());
      let gone = state.clone();
      w.on_close(move |_| gone.borrow_mut().window = None);
      state.borrow_mut().window = Some(w);
    }
  }
}

/// Pastes `text` where the user types: the clipboard taken, the text offered, the paste shortcut
/// posted, and a second later what the clipboard held put back, unless something else was copied
/// meanwhile.
fn paste(main: &mut Main, text: &str) {
  let clip = SystemClipboard;
  let before = clip.snapshot().filter(|s| !s.unreadable());
  let Some(offer) = clip.offer(text, None) else { return eprintln!("paste: no clipboard") };
  if let Err(e) = Shortcut::parse("cmd+v").map_err(|e| e.to_string()).and_then(nimbo::keys::post) {
    eprintln!("paste: {e}");
  }

  main.proxy().after(Duration::from_secs(1), move |_| {
    if let Some(before) = before.filter(|_| clip.holds(offer)) {
      clip.restore(before);
    }
  });
}

thread_local! {
  /// The app's state, on the main thread, where the hotkey's work is posted.
  static STATE: Shared = Shared::default();
}

fn start(main: &mut Main) {
  // Nothing off Linux; there the entry is written once, again when the executable moves.
  let icon = (64, &counter::ICON[..]);
  let launcher = Launcher { comment: "A tray app made with nimbo", categories: &["Utility"], icon };
  if let Err(e) = desktop::install(&INFO, &launcher) {
    eprintln!("desktop: {e}");
  }

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
  if let Err(e) = hotkeys.set(HOTKEY) {
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

  state.borrow_mut().hotkeys = Some(hotkeys);
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
      "paste" => main.proxy().after(Duration::from_secs(3), |main| paste(main, "Hello from nimbo")),
      "quit" => main.quit(),
      _ => {}
    }

    refresh(t, &state);
  });

  // The tray lives as long as the app; the hotkeys are in its state.
  main.on_quit(move |_| drop(tray));
}

fn main() {
  if std::env::args().any(|a| a == "--shot") {
    return nimbo::run(Config::new("Counter", 360.0, 300.0), Counter::default(), |h| _ = h.frame());
  }

  nimbo::app(INFO, start);
}
