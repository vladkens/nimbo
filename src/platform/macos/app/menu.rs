// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Menus on macOS: an NSMenu built from [Menu] ([fill], the tray's and the menu bar's too) and
//! changed in place ([update]), a context menu popped up over a view ([popup]) outside the window's
//! borrow, and the key equivalents of shortcuts.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
  NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSEventModifierFlags, NSMenu, NSMenuItem,
  NSView,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSString};

use crate::keys::{Key, Shortcut, f_key};
use crate::menu::{Item, Menu};
use crate::scene::Point;
use crate::{Modifiers, NamedKey};

thread_local! {
  /// A context menu is tracking: AppKit tracks one menu at a time.
  static OPEN: Cell<bool> = const { Cell::new(false) };
}

define_class!(
  // SAFETY: NSObject has no subclassing requirements; we add only the action.
  #[unsafe(super(NSObject))]
  #[thread_kind = MainThreadOnly]
  #[name = "NimboMenuTarget"]
  #[ivars = Cell<Option<isize>>]
  struct Target;

  impl Target {
    /// Records the chosen item's tag.
    #[unsafe(method(chosen:))]
    fn chosen(&self, sender: Option<&AnyObject>) {
      let item = sender.and_then(|s| s.downcast_ref::<NSMenuItem>());
      self.ivars().set(item.map(|i| i.tag()));
    }
  }
);

/// Shows `menu` over `view` and returns the chosen item's action number once it closes: `None` when
/// dismissed, and at once while another menu is open. With the secondary press that asked for
/// it as a context menu (`popUpContextMenu:withEvent:forView:`), else at `at` in the view. The
/// chosen item's action comes before the call returns, from its tracking loop (the plan's VM
/// check confirms it). Only the menu's items show. A destructive item shows plain: NSMenuItem has
/// no such style (Apple's NSMenuItem docs, 2026-10-29).
pub fn popup(
  view: &NSView,
  press: Option<&NSEvent>,
  at: Point,
  menu: &Menu<usize>,
) -> Option<usize> {
  if OPEN.replace(true) {
    return None;
  }

  let mtm = super::super::mtm();
  let target = Target::alloc(mtm).set_ivars(Cell::new(None));
  let target: Retained<Target> = unsafe { msg_send![super(target), init] };
  let ns = NSMenu::new(mtm);
  ns.setAutoenablesItems(false);
  // No plug-in items, nor Writing Tools' where macOS has them (15.2+, Apple's NSMenu docs,
  // 2026-10-30).
  ns.setAllowsContextMenuPlugIns(false);
  if ns.respondsToSelector(sel!(setAutomaticallyInsertsWritingToolsItems:)) {
    ns.setAutomaticallyInsertsWritingToolsItems(false);
  }

  fill(mtm, &ns, &menu.items, &chooses(&target));
  // For the window's frame view, not ours: for a view that takes text AppKit adds its own
  // items (AutoFill, Writing Tools, Services), as Electron's menus show none for a view that
  // isn't a web page's (electron/electron#45138, 2025-03-28); its user defaults
  // (`NSAutoFillHeuristicsEnabled`) leave the AutoFill item there (macOS 27, 2026-10-30).
  let frame = unsafe { view.superview() };
  match press {
    Some(e) => NSMenu::popUpContextMenu_withEvent_forView(&ns, e, frame.as_deref().unwrap_or(view)),
    None => {
      let p = NSPoint::new(at.x as f64, at.y as f64);
      ns.popUpMenuPositioningItem_atLocation_inView(None, p, Some(view));
    }
  }

  OPEN.set(false);
  target.ivars().get().map(|tag| tag as usize)
}

/// Adds `items` to `menu`, `act` giving each button and toggle its action by its action's number
/// ([chooses]).
pub fn fill(
  mtm: MainThreadMarker,
  menu: &NSMenu,
  items: &[Item<usize>],
  act: &dyn Fn(&NSMenuItem, usize),
) {
  for it in items {
    if let Item::Divider = it {
      menu.addItem(&NSMenuItem::separatorItem(mtm));
      continue;
    }

    // A section header on macOS 14+; below it a disabled item with the title and no action, as
    // Windows and Linux show one.
    if let Item::Section(label) = it {
      let title = NSString::from_str(label);
      if objc2::available!(macos = 14.0) {
        menu.addItem(&NSMenuItem::sectionHeaderWithTitle(&title, mtm));
        continue;
      }

      // SAFETY: no action.
      let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
          NSMenuItem::alloc(mtm),
          &title,
          None,
          &NSString::new(),
        )
      };
      item.setEnabled(false);
      menu.addItem(&item);
      continue;
    }

    let none = (String::new(), NSEventModifierFlags::empty());
    let (key, mods) = it.shortcut().and_then(key_equivalent).unwrap_or(none);
    // SAFETY: no action yet.
    let item = unsafe {
      NSMenuItem::initWithTitle_action_keyEquivalent(
        NSMenuItem::alloc(mtm),
        &NSString::from_str(it.label()),
        None,
        &NSString::from_str(&key),
      )
    };
    if !key.is_empty() {
      item.setKeyEquivalentModifierMask(mods);
    }

    item.setEnabled(it.is_enabled());
    let on = it.is_on();
    item.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
    if let Some(&n) = it.action() {
      act(&item, n);
    }

    if let Item::Submenu { menu: sub, .. } = it {
      // The bar shows a submenu's title, not its item's.
      let submenu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(it.label()));
      submenu.setAutoenablesItems(false);
      fill(mtm, &submenu, &sub.items, act);
      item.setSubmenu(Some(&submenu));
    }

    menu.addItem(&item);
  }
}

/// What [fill] gives an app's item in a context menu, the tray or the bar: `chosen:` sent to
/// `target` with the action's number as the tag.
pub fn chooses(target: &AnyObject) -> impl Fn(&NSMenuItem, usize) {
  move |item, n| {
    // SAFETY: the target has `chosen:` and outlives the menu's use.
    unsafe {
      item.setAction(Some(sel!(chosen:)));
      item.setTarget(Some(target));
    }
    item.setTag(n as isize);
  }
}

/// Gives the items of `menu`, built by [fill] from items the same but for these, `items`' labels,
/// checks and enabled states: an open menu keeps its submenus open.
pub fn update(menu: &NSMenu, items: &[Item<usize>]) {
  for (i, it) in items.iter().enumerate() {
    let Some(item) = menu.itemAtIndex(i as isize) else { return };
    if let Item::Divider = it {
      continue;
    }

    item.setTitle(&NSString::from_str(it.label()));
    if let Item::Section(_) = it {
      continue;
    }

    item.setEnabled(it.is_enabled());
    let on = it.is_on();
    item.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
    if let (Item::Submenu { menu: sub, .. }, Some(submenu)) = (it, item.submenu()) {
      submenu.setTitle(&NSString::from_str(it.label()));
      update(&submenu, &sub.items);
    }
  }
}

/// A menu item's key equivalent for a shortcut: the character AppKit matches
/// (`NSF1FunctionKey` and the like for keys that type none) and the modifiers.
fn key_equivalent(s: Shortcut) -> Option<(String, NSEventModifierFlags)> {
  let fkey = |n: u32| char::from_u32(n);
  let c = match s.key {
    Key::Char(c) => Some(c),
    Key::Named(k) => match k {
      NamedKey::Enter => Some('\r'),
      NamedKey::Tab => Some('\t'),
      NamedKey::Backspace => Some('\u{8}'),
      NamedKey::Delete => Some('\u{7f}'),
      NamedKey::Escape => Some('\u{1b}'),
      NamedKey::ArrowUp => fkey(0xF700),
      NamedKey::ArrowDown => fkey(0xF701),
      NamedKey::ArrowLeft => fkey(0xF702),
      NamedKey::ArrowRight => fkey(0xF703),
      NamedKey::Home => fkey(0xF729),
      NamedKey::End => fkey(0xF72B),
      NamedKey::PageUp => fkey(0xF72C),
      NamedKey::PageDown => fkey(0xF72D),
      k => (1..=20u32)
        .find(|&n| f_key(n as u8).is_some_and(|f| f.1 == k))
        .and_then(|n| fkey(0xF703 + n)),
    },
  }?;
  let flags = [
    (Modifiers::CONTROL, NSEventModifierFlags::Control),
    (Modifiers::ALT, NSEventModifierFlags::Option),
    (Modifiers::SHIFT, NSEventModifierFlags::Shift),
    (Modifiers::META, NSEventModifierFlags::Command),
  ];
  let mods = flags
    .iter()
    .filter(|(m, _)| s.mods.contains(*m))
    .fold(NSEventModifierFlags::empty(), |a, f| a | f.1);
  Some((c.to_string(), mods))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn key_equivalents() {
    let cmd = NSEventModifierFlags::Command;
    let key = |s: &str| key_equivalent(Shortcut::parse(s).unwrap());
    assert_eq!(key("cmd+comma"), Some((",".into(), cmd)));
    assert_eq!(key("cmd+q"), Some(("q".into(), cmd)));
    let all = NSEventModifierFlags::Control | NSEventModifierFlags::Shift | cmd;
    assert_eq!(key("ctrl+shift+cmd+F2"), Some(("\u{F705}".into(), all)));
    assert_eq!(key("option+left").map(|k| k.0), Some("\u{F702}".into()));
  }
}
