// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! `examples/dictation/main.rs`'s tray menu, hotkey and icon, and the detached tray's scenario
//! (`tests/dictation/detached.rs`) where it needs no main thread: everywhere but macOS.

use nimbo::system::Change;
use nimbo::tray::TrayIcon;

#[cfg(not(target_os = "macos"))]
use crate::detached;
use crate::dictation::diagnostics::Seen;
use crate::dictation::{self, Shown};

#[cfg(not(target_os = "macos"))]
#[test]
fn detached_tray() {
  detached::run();
}

/// The app's menu as a dictation starts and stops and launch at login changes, its hotkey and
/// its icon.
#[test]
fn the_example() {
  let labels = |s: &Shown| {
    let m = dictation::menu(s);
    let items = m.items().iter().map(|i| (i.label().to_owned(), i.is_enabled(), i.is_on()));
    items.collect::<Vec<_>>()
  };
  let login = |l: &[(String, bool, bool)]| l.iter().find(|i| i.0 == "Launch at Login").cloned();

  let idle = Shown { hotkey: "Ctrl+Alt+Space".into(), ..Shown::default() };
  let l = labels(&idle);
  assert_eq!(l[0], ("Ctrl+Alt+Space starts a dictation".into(), false, false));
  assert_eq!(l[1].0, "Start Dictation");
  assert!(l.iter().any(|i| i.0 == "Settings…" && i.1), "Settings… whether open or not");
  assert_eq!(login(&l), Some(("Launch at Login".into(), false, false)), "no launch at login");

  let recording = Shown { recording: true, login: Some(true), ..idle.clone() };
  let l = labels(&recording);
  assert_eq!(
    (l[0].0.as_str(), l[1].0.as_str()),
    ("Recording: Ctrl+Alt+Space stops it", "Stop Dictation")
  );
  assert_eq!(login(&l), Some(("Launch at Login".into(), true, true)));
  let unbound = labels(&Shown { hotkey: String::new(), ..idle });
  assert_eq!((unbound[0].0.as_str(), unbound[0].1), ("No hotkey", false));

  assert!(nimbo::Hotkey::parse(dictation::HOTKEY).is_ok());
  // The icon's path data reads, as an icon file of it does.
  let file = format!(
    r#"<svg viewBox="0 0 18 18"><path fill-rule="evenodd" d="{}"/></svg>"#,
    dictation::ICON
  );
  let icon = TrayIcon::Path { size: 18.0, d: dictation::ICON.into(), even_odd: true };
  assert_eq!(TrayIcon::from_svg(&file).unwrap(), icon);
}

/// The tray's menu: the lid and the audio changes, both kinds counted.
#[test]
fn the_tray_menu() {
  let mut s = Seen::new(None);
  // The two lines from the lid's on.
  let labels = |s: &Seen| {
    let m = dictation::menu(&Shown { seen: s.clone(), ..Shown::default() });
    let lid = m.items().iter().position(|i| i.label().starts_with("Lid:")).unwrap();
    m.items()[lid..lid + 2].iter().map(|i| i.label().to_owned()).collect::<Vec<_>>()
  };

  assert_eq!(labels(&s), ["Lid: None", "Audio changes: 0"]);
  s.add(Change::AudioInputs, Some(false));
  s.add(Change::DefaultInput, Some(false));
  s.add(Change::Lid, Some(true));
  assert_eq!(labels(&s), ["Lid: Closed", "Audio changes: 2"]);
  assert_eq!(s.last, Some(Change::Lid));
}

/// Showing Settings when it is closed (a second launch's "open", a reopen) opens it, which
/// borrows the state again: the lookup lets its borrow go first.
#[test]
fn a_closed_window_is_looked_up_without_holding_the_state() {
  let state = dictation::Shared::default();
  assert!(dictation::open_window(&state, |s| &s.settings).is_none());
  assert!(state.try_borrow_mut().is_ok(), "the state is free to open the window");
}
