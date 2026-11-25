// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! `examples/tray.rs`'s menu, hotkey and icon, and the detached tray's scenario
//! (`tests/tray/detached.rs`) where it needs no main thread: everywhere but macOS.

#[path = "../examples/tray.rs"]
#[expect(dead_code, reason = "its main and what this file doesn't use")]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[cfg_attr(windows, expect(unused_attributes, reason = "its windows_subsystem"))]
mod tray;

#[cfg(not(target_os = "macos"))]
#[path = "tray/detached.rs"]
mod detached;

use nimbo::tray::TrayIcon;
use tray::Shown;

#[cfg(not(target_os = "macos"))]
#[test]
fn detached_tray() {
  detached::run();
}

/// The example's menu as the window opens and closes and launch at login changes, its hotkey
/// and its icon.
#[test]
fn the_example() {
  let labels = |s: &Shown| {
    let m = tray::menu(s);
    let items = m.items().iter().map(|i| (i.label().to_owned(), i.is_enabled(), i.is_on()));
    items.collect::<Vec<_>>()
  };

  let idle = Shown { count: None, login: None, hotkey: "Ctrl+Alt+Space".into() };
  let l = labels(&idle);
  assert_eq!(l[0], ("Ctrl+Alt+Space opens the counter".into(), false, false));
  assert_eq!((l[1].0.as_str(), l[3].1), ("Open Counter", false), "no launch at login");

  let open = Shown { count: Some(3), login: Some(true), ..idle };
  let l = labels(&open);
  assert_eq!((l[0].0.as_str(), l[1].0.as_str()), ("Count: 3", "Close Counter"));
  assert_eq!(l[3], ("Launch at Login".into(), true, true));

  assert!(nimbo::Hotkey::parse(tray::HOTKEY).is_ok());
  // The icon's path data reads, as an icon file of it does.
  let file =
    format!(r#"<svg viewBox="0 0 18 18"><path fill-rule="evenodd" d="{}"/></svg>"#, tray::ICON);
  let icon = TrayIcon::Path { size: 18.0, d: tray::ICON.into(), even_odd: true };
  assert_eq!(TrayIcon::from_svg(&file).unwrap(), icon);
}

/// Showing the counter when it is closed (a second launch's "open", a reopen) opens it, which
/// borrows the state again: the lookup lets its borrow go first.
#[test]
fn a_closed_counter_is_looked_up_without_holding_the_state() {
  let state = tray::Shared::default();
  assert!(tray::open_counter(&state).is_none());
  assert!(state.try_borrow_mut().is_ok(), "the state is free to open the counter");
}
