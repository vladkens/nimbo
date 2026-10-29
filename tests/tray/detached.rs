// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The tray's menu as each OS gets it, with nothing on screen (a detached tray: no status item,
//! notify icon or D-Bus item): built from a [Menu] (a header, labels, shortcuts, check marks,
//! disabled and destructive items, separators, a submenu), rebuilt as it opens, kept while open
//! (a new one waits for it to close), and items chosen through the OS's own path, reaching the
//! app by id; icons from RGBA and from path data. A plain test on Windows and Linux
//! (`tests/tray.rs`); on macOS the NSMenu and NSImage are main thread only, which the test
//! harness isn't.

use std::cell::RefCell;
use std::rc::Rc;

use nimbo::tray::{Item, Menu, Tray, TrayIcon};

fn menu(n: u32) -> Menu {
  let sizes =
    Menu::new(vec![Item::new("small", "Small").checked(true), Item::new("large", "Large")]);
  Menu::new(vec![
    Item::header(&format!("Status {n}")),
    Item::separator(),
    Item::new("size", "Size").submenu(sizes),
    Item::new("settings", "Settings…").key("cmd+comma"),
    Item::new("reset", "Reset").destructive(),
    Item::new("quit", "Quit").key("cmd+q"),
  ])
}

pub fn run() {
  let px = (0..36 * 36).flat_map(|i| [0, 0, 0, if i % 2 == 0 { 255 } else { 0 }]).collect();
  let tray = Tray::detached(TrayIcon::Rgba { w: 36, h: 36, scale: 2.0, px }, "Idle");
  tray.set_menu(&menu(0));
  let (comma, q) = match cfg!(target_os = "macos") {
    true => ("⌘,", "⌘q"),
    false => ("Ctrl+,", "Ctrl+Q"),
  };

  let want = [
    "Status 0||off|disabled|header".to_string(),
    "-".into(),
    "Size||off|enabled".into(),
    "  Small||on|enabled".into(),
    "  Large||off|enabled".into(),
    format!("Settings…|{comma}|off|enabled"),
    // No destructive style in the OS's menus: a plain item.
    "Reset||off|enabled".into(),
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
  assert_eq!((*opened.borrow(), items(&tray)[0].as_str()), (1, "Status 1||off|disabled|header"));
  tray.choose(&[3]);
  tray.choose(&[2, 1]);
  // A header chooses nothing.
  tray.choose(&[0]);
  tray.choose(&[4]);
  tray.choose(&[5]);
  assert_eq!(*chosen.borrow(), ["settings", "large", "reset", "quit"]);

  // Open, the menu stays as it is (its submenus too); the last one set comes as it closes.
  // (A click closes the menu on Linux: it is opened again first.)
  tray.closing();
  tray.opening();
  assert_eq!(items(&tray)[0], "Status 2||off|disabled|header", "the refresh as it opens");
  tray.set_menu(&menu(7));
  tray.set_menu(&menu(8));
  assert_eq!(items(&tray)[0], "Status 2||off|disabled|header");
  tray.closing();
  assert_eq!(items(&tray)[0], "Status 8||off|disabled|header");
  tray.set_menu(&menu(9));
  assert_eq!(items(&tray)[0], "Status 9||off|disabled|header", "closed: at once");

  tray.set_icon(TrayIcon::path(24.0, "M2 2H22V22Z"));
  assert!(tray.describe()[0].starts_with("icon "));
}
