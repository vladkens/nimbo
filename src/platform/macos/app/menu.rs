// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Menus on macOS: an NSMenu built from [Menu] ([fill], the tray's too), a context menu popped
//! up over a view ([popup]) outside the window's borrow, and the key equivalents of shortcuts.

use std::cell::Cell;

use keyboard_types::{Modifiers, NamedKey};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
  NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSEventModifierFlags, NSMenu, NSMenuItem,
  NSView,
};
use objc2_foundation::{NSObject, NSPoint, NSString};

use crate::keys::{Key, Shortcut, f_key};
use crate::menu::{Item, Menu, choices};
use crate::scene::Point;

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

/// Shows `menu` over `view` and returns the chosen item's id once it closes: `None` when
/// dismissed, and at once while another menu is open. With the secondary press that asked for
/// it as a context menu (`popUpContextMenu:withEvent:forView:`), else at `at` in the view. The
/// chosen item's action comes before the call returns, from its tracking loop (the plan's VM
/// check confirms it). A destructive item shows plain: NSMenuItem has no such style (Apple's
/// NSMenuItem docs, 2026-10-29).
pub fn popup(view: &NSView, press: Option<&NSEvent>, at: Point, menu: &Menu) -> Option<String> {
  if OPEN.replace(true) {
    return None;
  }

  let mtm = super::super::mtm();
  let target = Target::alloc(mtm).set_ivars(Cell::new(None));
  let target: Retained<Target> = unsafe { msg_send![super(target), init] };
  let ns = NSMenu::new(mtm);
  ns.setAutoenablesItems(false);
  fill(mtm, &ns, &menu.items, &target, &mut 0);
  match press {
    Some(e) => NSMenu::popUpContextMenu_withEvent_forView(&ns, e, view),
    None => {
      let p = NSPoint::new(at.x as f64, at.y as f64);
      ns.popUpMenuPositioningItem_atLocation_inView(None, p, Some(view));
    }
  }

  OPEN.set(false);
  let tag = target.ivars().get()?;
  choices(menu).get(tag as usize).cloned()
}

/// Adds `items` to `menu`; the chosen ones send `chosen:` to `target` with their tag, counted
/// from `tag` in the order of [choices].
pub fn fill(
  mtm: MainThreadMarker,
  menu: &NSMenu,
  items: &[Item],
  target: &AnyObject,
  tag: &mut isize,
) {
  for it in items {
    if it.separator {
      menu.addItem(&NSMenuItem::separatorItem(mtm));
      continue;
    }

    // macOS 14's section header, as the deployment target allows.
    if it.header {
      menu.addItem(&NSMenuItem::sectionHeaderWithTitle(&NSString::from_str(&it.label), mtm));
      continue;
    }

    let action = it.submenu.is_none().then_some(sel!(chosen:));
    let none = (String::new(), NSEventModifierFlags::empty());
    let (key, mods) = it.key.as_deref().and_then(key_equivalent).unwrap_or(none);
    // SAFETY: the action is the target's, which outlives the menu's use.
    let item = unsafe {
      NSMenuItem::initWithTitle_action_keyEquivalent(
        NSMenuItem::alloc(mtm),
        &NSString::from_str(&it.label),
        action,
        &NSString::from_str(&key),
      )
    };
    if action.is_some() {
      unsafe { item.setTarget(Some(target)) };
      item.setTag(*tag);
      *tag += 1;
    }

    if !key.is_empty() {
      item.setKeyEquivalentModifierMask(mods);
    }

    item.setEnabled(it.enabled);
    item.setState(if it.checked { NSControlStateValueOn } else { NSControlStateValueOff });
    if let Some(sub) = &it.submenu {
      let submenu = NSMenu::new(mtm);
      submenu.setAutoenablesItems(false);
      fill(mtm, &submenu, &sub.items, target, tag);
      item.setSubmenu(Some(&submenu));
    }

    menu.addItem(&item);
  }
}

/// A menu item's key equivalent for a shortcut as [Shortcut::parse] spells it: the character
/// AppKit matches (`NSF1FunctionKey` and the like for keys that type none) and the modifiers.
fn key_equivalent(text: &str) -> Option<(String, NSEventModifierFlags)> {
  let s = Shortcut::parse(text).ok()?;
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
    assert_eq!(key_equivalent("cmd+comma"), Some((",".into(), cmd)));
    assert_eq!(key_equivalent("cmd+q"), Some(("q".into(), cmd)));
    let all = NSEventModifierFlags::Control | NSEventModifierFlags::Shift | cmd;
    assert_eq!(key_equivalent("ctrl+shift+cmd+F2"), Some(("\u{F705}".into(), all)));
    assert_eq!(key_equivalent("option+left").map(|k| k.0), Some("\u{F702}".into()));
    assert_eq!(key_equivalent("right_option"), None, "no modifier alone");
    assert_eq!(key_equivalent("cmd+nope"), None);
  }
}
