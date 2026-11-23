// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The menu bar: an NSMenu built from the bar's model ([crate::commands]), the standard items on
//! AppKit's own actions, sent to the first responder (Edit's to our view, which runs the command
//! their key would: [super::view]), the app's items to the app delegate's `chosen:`; AppKit fills
//! Services, the Window menu and Help's search, and enables the items as they can act.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};

use super::app::menu::{chooses, fill};
use crate::commands::{Bar, Standard};
use crate::keys::Command;

/// The bar of `bar`: Quit and the app's items go to `delegate`, which has `quit:` and `chosen:`.
pub fn menu(
  mtm: MainThreadMarker,
  (menu, standard): Bar,
  delegate: &AnyObject,
) -> Retained<NSMenu> {
  let ns_app = NSApplication::sharedApplication(mtm);
  let bar = NSMenu::new(mtm);
  let chosen = chooses(delegate);
  fill(mtm, &bar, &menu.items, &|item, n| match standard[n] {
    Some(s) => set(mtm, &ns_app, item, s, delegate),
    // A disabled item has no action: AppKit enables the bar's items that have one.
    None if item.isEnabled() => chosen(item, n),
    None => {}
  });
  for menu in bar.itemArray().iter().filter_map(|m| m.submenu()) {
    menu.setAutoenablesItems(true);
  }

  let last = |back: isize| bar.itemAtIndex(bar.numberOfItems() - 1 - back)?.submenu();
  ns_app.setWindowsMenu(last(1).as_deref());
  ns_app.setHelpMenu(last(0).as_deref());
  bar
}

/// Gives a standard item its action: to the first responder (a window, our view, the app) unless
/// it is Quit; Services gets the menu AppKit fills.
fn set(
  mtm: MainThreadMarker,
  ns_app: &NSApplication,
  item: &NSMenuItem,
  s: Standard,
  quit: &AnyObject,
) {
  use Standard as S;
  let action = match s {
    S::About => sel!(orderFrontStandardAboutPanel:),
    S::Services => {
      let services = NSMenu::new(mtm);
      item.setSubmenu(Some(&services));
      return ns_app.setServicesMenu(Some(&services));
    }
    S::Hide => sel!(hide:),
    S::HideOthers => sel!(hideOtherApplications:),
    S::ShowAll => sel!(unhideAllApplications:),
    S::Quit => sel!(quit:),
    S::Close => sel!(performClose:),
    S::Edit(Command::Undo) => sel!(undo:),
    S::Edit(Command::Redo) => sel!(redo:),
    S::Edit(Command::Cut) => sel!(cut:),
    S::Edit(Command::Copy) => sel!(copy:),
    S::Edit(Command::Paste) => sel!(paste:),
    S::Edit(Command::SelectAll) => sel!(selectAll:),
    S::Edit(_) => return,
    S::FullScreen => sel!(toggleFullScreen:),
    S::Minimize => sel!(performMiniaturize:),
    S::Zoom => sel!(performZoom:),
    S::BringAllToFront => sel!(arrangeInFront:),
  };
  // SAFETY: AppKit's actions, sent along the responder chain; Quit's to the delegate, which has
  // it and lives as long as the app.
  unsafe {
    item.setAction(Some(action));
    if s == S::Quit {
      item.setTarget(Some(quit));
    }
  }
}
