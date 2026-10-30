// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Context menus on Linux as GtkMenus ([crate::Ui::context_menu]): the menu a window asked for
//! ([crate::Runtime::take_context_menu]) is built as it asks, which runs none of our callbacks,
//! and popped up from an idle, with no window borrowed, at the point asked, the secondary press
//! that asked for it as its trigger event (the seat and button GTK grabs with, which Wayland's
//! `xdg_popup` grab needs).
//!
//! GTK doesn't block while the menu is up: frames and timers go on. It emits `deactivate` before
//! the chosen item's `activate` (`gtk_menu_shell_activate_item` in GTK 3.24's `gtkmenushell.c`,
//! read 2026-10-29), so the answer ([Event::Menu]) is sent from an idle after both, once the
//! window is free. A request while a menu is up replaces it, the old one answered `None`. GTK 3
//! menus have no destructive style: such an item shows plain; a header is a disabled item.

use std::cell::{Cell, RefCell};
use std::ffi::{CString, c_int, c_uint};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use super::super::dl::fns;
use super::super::{FALSE, Fp, P, S, connect_id, gtk, lib, slot, window_busy, with_data};
use super::hotkey::keysym;
use crate::app::Event;
use crate::keys::Shortcut;
use crate::menu::choices;
use crate::scene::Point;
use crate::ui::Id;
use crate::{Item, Menu};

/// `GDK_GRAVITY_NORTH_WEST` and `SOUTH_EAST`: the menu's corner at the point's, as
/// `gtk_menu_popup_at_pointer` places it.
const NORTH_WEST: c_int = 1;
const SOUTH_EAST: c_int = 9;
/// `GDK_KEY_VoidSymbol`: a keysym name GDK doesn't know.
const VOID: c_uint = 0xffffff;

fns!(Menus {
  fn gtk_menu_new() -> P;
  fn gtk_menu_item_new_with_label(label: S) -> P;
  fn gtk_check_menu_item_new_with_label(label: S) -> P;
  fn gtk_check_menu_item_set_active(item: P, on: c_int);
  fn gtk_separator_menu_item_new() -> P;
  fn gtk_menu_shell_append(shell: P, item: P);
  fn gtk_menu_shell_deactivate(shell: P);
  fn gtk_menu_item_set_submenu(item: P, menu: P);
  fn gtk_menu_attach_to_widget(menu: P, widget: P, detach: Fp);
  fn gtk_menu_popup_at_rect(menu: P, window: P, rect: *const [c_int; 4], anchor: c_int,
    menu_anchor: c_int, trigger: P);
  fn gtk_widget_set_sensitive(w: P, on: c_int);
  fn gtk_widget_get_visible(w: P) -> c_int;
  fn gtk_bin_get_child(bin: P) -> P;
  fn gtk_accel_label_set_accel(label: P, key: c_uint, mods: c_uint);
  fn gdk_keyval_from_name(name: S) -> c_uint;
  fn g_object_ref_sink(obj: P) -> P;
});

fn menus() -> &'static Menus {
  static MENUS: OnceLock<Option<Menus>> = OnceLock::new();
  MENUS.get_or_init(|| Menus::load(&lib()?)).as_ref().expect("GTK 3 has its menu functions")
}

/// A menu of ours, by number: its window and element, its items' ids by number ([choices]), the
/// one chosen; the GtkMenu (our reference), the press that asked for it (a GdkEvent copy, ours;
/// null once shown, or for none) and the rect it shows at.
struct Up {
  n: u64,
  win: u64,
  on: Id,
  ids: Vec<String>,
  chosen: Option<String>,
  menu: P,
  press: P,
  at: [c_int; 4],
}

thread_local! {
  static UP: RefCell<Vec<Up>> = const { RefCell::new(Vec::new()) };
  static NEXT: Cell<u64> = const { Cell::new(0) };
}

/// Window `win` (its area `area`) asks for `menu` on `on` at `at`, after the secondary `press`
/// (a copy, now ours): built now, it pops up from an idle, outside the window's borrow.
pub fn ask(win: u64, area: P, press: P, on: Id, at: Point, menu: &Menu) {
  let (f, n) = (menus(), NEXT.get() + 1);
  NEXT.set(n);
  // SAFETY: GTK's calls on the main thread; the menu is ours (its floating reference sunk)
  // until [on_done]. Building it runs no callback of ours.
  let m = unsafe {
    let m = (f.g_object_ref_sink)((f.gtk_menu_new)());
    fill(m, &menu.items, n, &mut 0);
    (f.gtk_menu_attach_to_widget)(m, area, null());
    connect_id(m, c"deactivate", on_deactivate as Fp, 0, n);
    m
  };
  let at = [at.x.round() as c_int, at.y.round() as c_int, 1, 1];
  let up = Up { n, win, on, ids: choices(menu), chosen: None, menu: m, press, at };
  UP.with_borrow_mut(|u| u.push(up));
  unsafe { (gtk().g_idle_add)(on_ask as Fp, n as usize as P) };
}

/// Pops menu `data` up, the others closing first (a request while one is up replaces it, the
/// old one answered `None`); one that can't show (its window gone, the pointer grabbed by
/// another client) is answered `None`.
unsafe extern "C" fn on_ask(data: P) -> c_int {
  let (g, f, n) = (gtk(), menus(), data as usize as u64);
  let others: Vec<P> = UP.with_borrow(|u| u.iter().filter(|u| u.n != n).map(|u| u.menu).collect());
  for m in others {
    unsafe { (f.gtk_menu_shell_deactivate)(m) };
  }

  let take = |u: &mut Up| (u.win, u.menu, std::mem::replace(&mut u.press, null_mut()), u.at);
  let up = UP.with_borrow_mut(|u| u.iter_mut().find(|u| u.n == n).map(take));
  let Some((win, menu, press, at)) = up else { return FALSE };
  // SAFETY: GTK's calls on our menu, over the window's area while it is open.
  unsafe {
    let window = slot(win, |s| s.area).map_or(null_mut(), |a| (g.gtk_widget_get_window)(a));
    if !window.is_null() {
      (f.gtk_menu_popup_at_rect)(menu, window, &at, SOUTH_EAST, NORTH_WEST, press);
    }

    if !press.is_null() {
      (g.gdk_event_free)(press);
    }

    // GTK shows the menu only once its grab took; it never deactivates one that didn't.
    if (f.gtk_widget_get_visible)(menu) == FALSE {
      on_deactivate(menu, data);
    }
  }

  FALSE
}

/// Appends `items` to `shell` (menu `n`'s, or a submenu of it), each that can be chosen
/// numbered from `next` on, depth first, as [choices] numbers them.
unsafe fn fill(shell: P, items: &[Item], n: u64, next: &mut u64) {
  let (g, f) = (gtk(), menus());
  for it in items {
    let label = CString::new(it.label.replace('\0', "")).unwrap_or_default();
    let plain = !it.separator && !it.header;
    // SAFETY: new items of ours, appended to our menu, which owns them from then.
    unsafe {
      let item = if it.separator {
        (f.gtk_separator_menu_item_new)()
      } else if plain && it.checkable && it.submenu.is_none() {
        let item = (f.gtk_check_menu_item_new_with_label)(label.as_ptr());
        // Before `activate` is connected: setting it activates the item.
        (f.gtk_check_menu_item_set_active)(item, it.checked as c_int);
        item
      } else {
        (f.gtk_menu_item_new_with_label)(label.as_ptr())
      };

      match &it.submenu {
        Some(sub) if plain => {
          let menu = (f.gtk_menu_new)();
          fill(menu, &sub.items, n, next);
          (f.gtk_menu_item_set_submenu)(item, menu);
        }
        _ if plain => {
          connect_id(item, c"activate", on_activate as Fp, 0, n << 32 | *next);
          *next += 1;
          if let Some((key, mods)) = it.key.as_deref().and_then(accel) {
            (f.gtk_accel_label_set_accel)((f.gtk_bin_get_child)(item), key, mods);
          }
        }
        _ => {}
      }

      (f.gtk_widget_set_sensitive)(item, (it.enabled && !it.header) as c_int);
      (f.gtk_menu_shell_append)(shell, item);
      (g.gtk_widget_show_all)(item);
    }
  }
}

/// An item's shortcut as GTK's accel label shows it: the keyval and GdkModifierType (Shift,
/// Control, Mod1 for Alt, Super).
fn accel(text: &str) -> Option<(c_uint, c_uint)> {
  let s = Shortcut::parse(text).ok()?;
  let name = CString::new(keysym(s.key)?).ok()?;
  let key = unsafe { (menus().gdk_keyval_from_name)(name.as_ptr()) };
  let m = |on: bool, bit: u32| (on as c_uint) << bit;
  let mods = m(s.mods.shift(), 0) | m(s.mods.ctrl(), 2) | m(s.mods.alt(), 3) | m(s.mods.meta(), 26);
  (key != VOID).then_some((key, mods))
}

/// An item of menu `data >> 32` was chosen, its number in the low half.
unsafe extern "C" fn on_activate(_: P, data: P) {
  let (n, i) = (data as usize as u64 >> 32, data as usize & 0xffff_ffff);
  let choose = |u: &mut Up| u.chosen = u.ids.get(i).cloned();
  UP.with_borrow_mut(|u| u.iter_mut().filter(|u| u.n == n).for_each(choose));
}

/// Menu `data` closed, chosen (its `activate` comes next) or dismissed: its answer goes from an
/// idle, after the `activate`.
unsafe extern "C" fn on_deactivate(_: P, data: P) {
  unsafe { (gtk().g_idle_add)(on_done as Fp, data) };
}

/// Sends menu `data`'s answer to its window and lets the menu go; while the window is in use
/// up the stack (a nested loop), the answer waits for it.
unsafe extern "C" fn on_done(data: P) -> c_int {
  let n = data as usize as u64;
  let Some(win) = UP.with_borrow(|u| u.iter().find(|u| u.n == n).map(|u| u.win)) else {
    return FALSE;
  };
  if window_busy(win) {
    unsafe { (gtk().g_timeout_add)(10, on_done as Fp, data) };
    return FALSE;
  }

  let up = UP.with_borrow_mut(|u| Some(u.remove(u.iter().position(|u| u.n == n)?)));
  let Some(up) = up else { return FALSE };
  with_data(win as usize as P, false, |w| w.send(Event::Menu { on: up.on, item: up.chosen }));
  let g = gtk();
  unsafe {
    (g.gtk_widget_destroy)(up.menu);
    (g.g_object_unref)(up.menu);
  }

  FALSE
}
