// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The tray's menu as each OS gets it, with nothing on screen (a detached tray: no status item,
//! notify icon or D-Bus item): built from a [Menu] (labels, shortcuts, check marks, disabled
//! items, separators, a submenu), rebuilt as it opens, kept while open (a new one waits for it
//! to close), and items chosen through the OS's own path, reaching the app by id; icons from
//! RGBA and from path data. Then `examples/tray.rs`'s menu, hotkey and icon.

#[path = "../examples/tray.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod example;

use std::cell::RefCell;
use std::rc::Rc;

use example::Shown;
use nimbo::tray::{Item, Menu, Tray, TrayIcon};

fn menu(n: u32) -> Menu {
  let sizes =
    Menu::new(vec![Item::new("small", "Small").checked(true), Item::new("large", "Large")]);
  Menu::new(vec![
    Item::new("header", &format!("Status {n}")).enabled(false),
    Item::separator(),
    Item::new("size", "Size").submenu(sizes),
    Item::new("settings", "Settings…").key("cmd+comma"),
    Item::new("quit", "Quit").key("cmd+q"),
  ])
}

fn main() {
  // Menu actions go through NSApp on macOS.
  #[cfg(target_os = "macos")]
  objc2_app_kit::NSApplication::sharedApplication(objc2::MainThreadMarker::new().expect("main"));

  let px = (0..36 * 36).flat_map(|i| [0, 0, 0, if i % 2 == 0 { 255 } else { 0 }]).collect();
  let tray = Tray::detached(TrayIcon::Rgba { w: 36, h: 36, scale: 2.0, px }, "Idle");
  tray.set_menu(&menu(0));
  let (comma, q) = match cfg!(target_os = "macos") {
    true => ("⌘,", "⌘q"),
    false => ("Ctrl+,", "Ctrl+Q"),
  };

  let want = [
    "Status 0||off|disabled".to_string(),
    "-".into(),
    "Size||off|enabled".into(),
    "  Small||on|enabled".into(),
    "  Large||off|enabled".into(),
    format!("Settings…|{comma}|off|enabled"),
    format!("Quit|{q}|off|enabled"),
  ];
  let items = |t: &Tray| t.describe()[1..].to_vec();
  assert_eq!(items(&tray), want);
  assert!(tray.describe()[0].starts_with("icon "), "{:?}", tray.describe()[0]);

  // Opening refreshes it; the callbacks get the tray.
  let opened = Rc::new(RefCell::new(0));
  let o = opened.clone();
  tray.on_open(move |_, t| {
    *o.borrow_mut() += 1;
    t.set_menu(&menu(*o.borrow()));
  });
  let chosen = Rc::new(RefCell::new(Vec::new()));
  let c = chosen.clone();
  tray.on_item(move |_, _, id| c.borrow_mut().push(id.to_owned()));
  tray.opening();
  assert_eq!((*opened.borrow(), items(&tray)[0].as_str()), (1, "Status 1||off|disabled"));
  tray.choose(&[3]);
  tray.choose(&[2, 1]);
  tray.choose(&[4]);
  assert_eq!(*chosen.borrow(), ["settings", "large", "quit"]);

  // Open, the menu stays as it is (its submenus too); the last one set comes as it closes.
  tray.set_menu(&menu(7));
  tray.set_menu(&menu(8));
  assert_eq!(items(&tray)[0], "Status 1||off|disabled");
  tray.closing();
  assert_eq!(items(&tray)[0], "Status 8||off|disabled");
  tray.set_menu(&menu(9));
  assert_eq!(items(&tray)[0], "Status 9||off|disabled", "closed: at once");
  tray.opening();
  assert_eq!(items(&tray)[0], "Status 2||off|disabled", "the refresh as it opens");
  tray.closing();

  tray.set_icon(TrayIcon::path(24.0, "M2 2H22V22Z"));
  assert!(tray.describe()[0].starts_with("icon "));
  the_example();
  println!("tray scenario: ok");
}

/// The example's menu as the window opens and closes and launch at login changes, its hotkey
/// and its icon.
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
  nimbo::tray::flatten(example::ICON, 1.0, &mut rings).unwrap();
  assert_eq!(rings.len(), 2, "a ring: two circles");
}
