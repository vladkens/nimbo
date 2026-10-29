// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Popup menus on Windows built from [crate::Menu]: the tray's ([super::tray]) and the context menu
//! a window's view asks for ([crate::Ui::context_menu]), opened at its point by `TrackPopupMenuEx`
//! outside the window's borrow, the choice sent back as [Event::Menu]. A command id is one more
//! than the item's index in [choices]. The menus stay light in dark mode: Win32 menus follow it
//! only through uxtheme's undocumented ordinals.

use std::cell::Cell;
use std::ptr::null;

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::super::{wide, with_window};
use crate::Event;
use crate::menu::{Item, choices};

// A context menu shows: one asked for meanwhile (in a frame of its modal timer) gets `None`.
thread_local!(static OPEN: Cell<bool> = const { Cell::new(false) });

/// Shows the context menu window `hwnd` asked for ([crate::Runtime::take_context_menu]) at its
/// point and sends the answer as [Event::Menu]: the item chosen, `None` when dismissed or while
/// one is open; whether it asked. The menu's loop runs the thread's messages until it closes,
/// the window's frames coming from its modal timer meanwhile (WM_ENTERMENULOOP, which the menu
/// sends its owner, starts it).
pub(crate) fn apply(hwnd: HWND) -> bool {
  let asked = with_window(hwnd, |w| w.rt.take_context_menu().map(|m| (m, w.scale)));
  let Some(((on, at, menu), k)) = asked.flatten() else { return false };
  let item = (!OPEN.replace(true)).then(|| {
    let mut p = POINT { x: (at.x * k).round() as i32, y: (at.y * k).round() as i32 };
    unsafe { ClientToScreen(hwnd, &mut p) };
    let hmenu = build(&menu.items, &mut 0);
    let cmd = track(hmenu, TPM_RIGHTBUTTON, p, hwnd);
    unsafe { DestroyMenu(hmenu) };
    OPEN.set(false);
    choices(&menu).get((cmd as usize).wrapping_sub(1)).cloned()
  });
  with_window(hwnd, |w| w.send(Event::Menu { on, item: item.flatten() }));
  true
}

/// Runs `menu` at screen pixel `p`, owned by `owner`, dropping to the side the system's
/// handedness wants (`SM_MENUDROPALIGNMENT`): the command chosen, 0 for none.
pub(super) fn track(menu: HMENU, flags: TRACK_POPUP_MENU_FLAGS, p: POINT, owner: HWND) -> u32 {
  let right = unsafe { GetSystemMetrics(SM_MENUDROPALIGNMENT) } != 0;
  let flags = flags | TPM_RETURNCMD | if right { TPM_RIGHTALIGN } else { 0 };
  unsafe { TrackPopupMenuEx(menu, flags, p.x, p.y, owner, null()) }.max(0) as u32
}

/// A popup menu of `items`, their commands numbered from `next` + 1 in [choices]' order.
pub(super) fn build(items: &[Item], next: &mut u32) -> HMENU {
  let menu = unsafe { CreatePopupMenu() };
  for it in items {
    let text = wide(&text(it));
    let flags = flags(it);
    // A separator is only that, whatever else it carries; a header, greyed, chooses nothing.
    let id = match &it.submenu {
      _ if it.separator || it.header => 0,
      Some(sub) => build(&sub.items, next) as usize,
      None => {
        *next += 1;
        *next as usize
      }
    };

    unsafe { AppendMenuW(menu, flags, id, if it.separator { null() } else { text.as_ptr() }) };
  }

  menu
}

/// What an item is to `AppendMenuW`: a separator, a submenu, checked, greyed.
fn flags(it: &Item) -> MENU_ITEM_FLAGS {
  if it.separator {
    return MF_SEPARATOR;
  }

  let on = |c: bool, f| if c { f } else { 0 };
  MF_STRING
    | on(it.submenu.is_some(), MF_POPUP)
    | on(it.checked, MF_CHECKED)
    | on(!it.enabled, MF_GRAYED)
}

/// An item's text: its label with `&` doubled (one alone marks the access key), then its key
/// after a tab, which menus show right-aligned.
fn text(it: &Item) -> String {
  let (label, key) = (it.label.replace('&', "&&"), it.key_label());
  if key.is_empty() { label } else { format!("{label}\t{key}") }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::Menu;

  #[test]
  fn items_become_flags_and_text() {
    let quit = Item::new("quit", "Save & Quit").key("cmd+q");
    assert_eq!(text(&quit), "Save && Quit\tCtrl+Q");
    assert_eq!(flags(&quit), MF_STRING);
    assert_eq!(
      flags(&Item::new("a", "A").checked(true).enabled(false)),
      MF_STRING | MF_CHECKED | MF_GRAYED
    );
    assert_eq!(flags(&Item::separator()), MF_SEPARATOR);
    assert_eq!(flags(&Item::new("s", "S").submenu(Menu::default())), MF_STRING | MF_POPUP);
  }
}
