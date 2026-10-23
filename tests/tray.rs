// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! `examples/tray.rs`'s menu, hotkey and icon, and the detached tray's scenario
//! (`tests/tray/detached.rs`) where it needs no main thread: everywhere but macOS.

#[path = "../examples/tray.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod example;

#[cfg(not(target_os = "macos"))]
#[path = "tray/detached.rs"]
mod detached;

use example::Shown;

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
    let m = example::menu(s);
    m.items.iter().map(|i| (i.label.clone(), i.enabled, i.checked)).collect::<Vec<_>>()
  };
  let idle = Shown { count: None, login: None, hotkey: "Ctrl+Alt+Space".into() };
  let l = labels(&idle);
  assert_eq!(l[0], ("Ctrl+Alt+Space opens the counter".into(), false, false));
  assert_eq!((l[1].0.as_str(), l[3].1), ("Open Counter", false), "no launch at login");
  let open = Shown { count: Some(3), login: Some(true), ..idle };
  let l = labels(&open);
  assert_eq!((l[0].0.as_str(), l[1].0.as_str()), ("Count: 3", "Close Counter"));
  assert_eq!(l[3], ("Launch at Login".into(), true, true));
  assert!(nimbo::Hotkey::parse(example::HOTKEY).is_ok());
  let mut rings = Vec::new();
  nimbo::svg::flatten(example::ICON, 1.0, &mut rings).unwrap();
  assert_eq!(rings.len(), 2, "a ring: two circles");
}
