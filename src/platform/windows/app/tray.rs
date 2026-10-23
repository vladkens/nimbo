// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The tray on Windows: a notification area icon (`Shell_NotifyIconW`, `NOTIFYICON_VERSION_4`)
//! whose callbacks reach the shell's hidden window ([super::shell::WM_TRAY]), and a popup menu
//! built from [Menu] that a left or right click opens with `TrackPopupMenuEx`, after the app's
//! `on_open`. The chosen item comes back as its command id, one more than its index in
//! [choices]. The icon is the template drawn white on a dark taskbar and black on a light one
//! (`SystemUsesLightTheme`), at the small icon size of the system DPI; it is drawn again when
//! the colours change, and added again when Explorer restarts (`TaskbarCreated`).

use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};
use std::rc::{Rc, Weak};

use windows_sys::Win32::Foundation::{LPARAM, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::super::{settings, wide};
use super::shell::{WM_TRAY, host};
use crate::Main;
use crate::tray::{Item, Menu, OnItem, OnOpen, TrayIcon, choices};

/// `NIN_KEYSELECT`: the icon chosen with the keyboard.
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;

struct Inner {
  /// The icon's `uID`, unique in the process.
  id: u32,
  /// Not in the notification area (tests).
  detached: bool,
  icon: RefCell<TrayIcon>,
  hicon: Cell<HICON>,
  /// The icon's side in pixels.
  side: Cell<i32>,
  tip: RefCell<String>,
  /// The menu as built, and the ids of its choices by command id less one.
  hmenu: Cell<HMENU>,
  ids: RefCell<Vec<String>>,
  on_open: RefCell<Option<OnOpen>>,
  on_item: RefCell<Option<OnItem>>,
  /// While the menu shows: a menu set meanwhile waits for it to close.
  open: Cell<bool>,
  pending: RefCell<Option<Menu>>,
}

impl Drop for Inner {
  fn drop(&mut self) {
    if !self.detached {
      notify_icon(NIM_DELETE, self);
    }

    unsafe {
      DestroyIcon(self.hicon.get());
      DestroyMenu(self.hmenu.get());
    }
  }
}

#[derive(Clone)]
pub struct Tray(Rc<Inner>);

thread_local! {
  /// The trays shown, for the shell window's callbacks.
  static TRAYS: RefCell<Vec<Weak<Inner>>> = const { RefCell::new(Vec::new()) };
  static NEXT: Cell<u32> = const { Cell::new(0) };
}

fn trays() -> Vec<Tray> {
  TRAYS.with_borrow_mut(|ts| {
    ts.retain(|t| t.strong_count() > 0);
    ts.iter().filter_map(Weak::upgrade).map(Tray).collect()
  })
}

/// Windows always has a notification area.
pub fn available() -> Result<(), String> {
  Ok(())
}

impl Tray {
  pub fn new(icon: TrayIcon, tooltip: &str) -> Self {
    let tray = Self::make(icon, tooltip, false);
    tray.add();
    TRAYS.with_borrow_mut(|ts| ts.push(Rc::downgrade(&tray.0)));
    tray
  }

  pub fn detached(icon: TrayIcon, tooltip: &str) -> Self {
    Self::make(icon, tooltip, true)
  }

  fn make(icon: TrayIcon, tooltip: &str, detached: bool) -> Self {
    let id = NEXT.with(|n| n.replace(n.get() + 1) + 1);
    let inner = Inner {
      id,
      detached,
      icon: RefCell::new(icon),
      hicon: Cell::new(null_mut()),
      side: Cell::new(0),
      tip: RefCell::new(tooltip.into()),
      hmenu: Cell::new(null_mut()),
      ids: RefCell::default(),
      on_open: RefCell::default(),
      on_item: RefCell::default(),
      open: Cell::new(false),
      pending: RefCell::default(),
    };
    let tray = Self(Rc::new(inner));
    tray.draw_icon();
    tray.build(&Menu::default());
    tray
  }

  /// Puts the icon in the notification area, with version 4's callbacks.
  fn add(&self) {
    if self.0.detached {
      return;
    }

    notify_icon(NIM_ADD, &self.0);
    notify_icon(NIM_SETVERSION, &self.0);
  }

  /// The icon at the small icon size, tinted for the taskbar.
  fn draw_icon(&self) {
    let side = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) }.max(16);
    let tint = if taskbar_light() { [0, 0, 0] } else { [255, 255, 255] };
    let px = self.0.icon.borrow().pixels(side as u32, tint);
    let old = self.0.hicon.replace(hicon(side, &px));
    self.0.side.set(side);
    unsafe { DestroyIcon(old) };
  }

  pub fn set_icon(&self, icon: TrayIcon) {
    *self.0.icon.borrow_mut() = icon;
    self.draw_icon();
    self.modify();
  }

  pub fn set_tooltip(&self, tooltip: &str) {
    *self.0.tip.borrow_mut() = tooltip.into();
    self.modify();
  }

  fn modify(&self) {
    if !self.0.detached {
      notify_icon(NIM_MODIFY, &self.0);
    }
  }

  pub fn set_menu(&self, menu: &Menu) {
    if self.0.open.get() {
      *self.0.pending.borrow_mut() = Some(menu.clone());
      return;
    }

    self.0.pending.take();
    self.build(menu);
  }

  fn build(&self, menu: &Menu) {
    let mut next = 0;
    let hmenu = build(&menu.items, &mut next);
    unsafe { DestroyMenu(self.0.hmenu.replace(hmenu)) };
    *self.0.ids.borrow_mut() = choices(menu);
  }

  pub fn on_open(&self, f: OnOpen) {
    *self.0.on_open.borrow_mut() = Some(f);
  }

  pub fn on_item(&self, f: OnItem) {
    *self.0.on_item.borrow_mut() = Some(f);
  }

  /// The app refreshes the menu, and it counts as open from here.
  pub fn opening(&self) {
    if let Some(mut f) = self.0.on_open.take() {
      f(&mut Main::here(), &crate::tray::Tray(self.clone()));
      self.0.on_open.borrow_mut().get_or_insert(f);
    }

    self.0.open.set(true);
  }

  /// The menu closed: the one set while it was open is built.
  pub fn closing(&self) {
    self.0.open.set(false);
    if let Some(menu) = self.0.pending.take() {
      self.build(&menu);
    }
  }

  /// The id of command `cmd` in the menu built now.
  fn id_of(&self, cmd: u32) -> Option<String> {
    self.0.ids.borrow().get((cmd as usize).wrapping_sub(1)).cloned()
  }

  /// Runs `on_item` with the id of command `cmd`.
  fn chosen(&self, cmd: u32) {
    self.run(self.id_of(cmd));
  }

  /// Runs `on_item` with `id`; nothing for none (the menu dismissed), the callback kept.
  fn run(&self, id: Option<String>) {
    let Some(id) = id else { return };
    let Some(mut f) = self.0.on_item.take() else { return };
    f(&mut Main::here(), &crate::tray::Tray(self.clone()), &id);
    self.0.on_item.borrow_mut().get_or_insert(f);
  }

  pub fn choose(&self, path: &[usize]) {
    let Some((last, parents)) = path.split_last() else { return };
    let mut menu = self.0.hmenu.get();
    for &i in parents {
      menu = unsafe { GetSubMenu(menu, i as i32) };
    }

    let cmd = unsafe { GetMenuItemID(menu, *last as i32) };
    if !menu.is_null() && cmd != u32::MAX {
      self.chosen(cmd);
    }
  }

  /// Opens the menu at `x`, `y` (screen pixels), as a click on the icon does, and runs the app's
  /// callbacks around it.
  fn show(&self, x: i32, y: i32) {
    self.opening();
    let host = host();
    let align =
      if unsafe { GetSystemMetrics(SM_MENUDROPALIGNMENT) } != 0 { TPM_RIGHTALIGN } else { 0 };
    let flags = TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | align;
    // The menu closes when the user clicks elsewhere only if its window is in front (the docs of
    // TrackPopupMenu), and needs a message after to settle.
    let cmd = unsafe {
      SetForegroundWindow(host);
      // Its loop runs the messages: the app's windows keep drawing from their timers.
      super::super::modal(1);
      let cmd = TrackPopupMenuEx(self.0.hmenu.get(), flags, x, y, host, null());
      super::super::modal(-1);
      PostMessageW(host, WM_NULL, 0, 0);
      cmd
    };
    // The item chosen in the menu shown, before a menu set meanwhile replaces it.
    let id = (cmd > 0).then(|| self.id_of(cmd as u32)).flatten();
    self.closing();
    self.run(id);
  }

  pub fn describe(&self) -> Vec<String> {
    let side = self.0.side.get();
    let mut out = vec![format!("icon {side}x{side}")];
    describe(self.0.hmenu.get(), 0, &mut out);
    out
  }
}

/// Adds, changes, deletes or versions `t`'s icon.
fn notify_icon(op: NOTIFY_ICON_MESSAGE, t: &Inner) -> bool {
  let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
  nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
  nid.hWnd = host();
  nid.uID = t.id;
  nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
  nid.uCallbackMessage = WM_TRAY;
  nid.hIcon = t.hicon.get();
  let tip: Vec<u16> = t.tip.borrow().encode_utf16().take(nid.szTip.len() - 1).collect();
  nid.szTip[..tip.len()].copy_from_slice(&tip);
  nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
  !nid.hWnd.is_null() && unsafe { Shell_NotifyIconW(op, &nid) } != 0
}

/// A callback of icon `HIWORD(lp)`: a click or the keyboard opens its menu at the anchor in
/// `wp`.
pub(super) fn notify(wp: WPARAM, lp: LPARAM) {
  let (event, id) = ((lp & 0xFFFF) as u32, (lp >> 16 & 0xFFFF) as u32);
  if !matches!(event, NIN_SELECT | NIN_KEYSELECT | WM_CONTEXTMENU) {
    return;
  }

  let (x, y) = ((wp & 0xFFFF) as i16 as i32, (wp >> 16 & 0xFFFF) as i16 as i32);
  if let Some(t) = trays().into_iter().find(|t| t.0.id == id) {
    t.show(x, y);
  }
}

/// The message Explorer broadcasts when the taskbar is made again.
pub(super) fn taskbar_created() -> u32 {
  thread_local!(static MSG: u32 = unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) });
  MSG.with(|m| *m)
}

/// Explorer restarted: every icon is added again.
pub(super) fn added_again() {
  for t in trays() {
    t.add();
  }
}

/// The app ends: its icons leave the notification area before the shell's window goes, as
/// none could after.
pub(super) fn removed() {
  for t in trays() {
    notify_icon(NIM_DELETE, &t.0);
  }

  TRAYS.with_borrow_mut(Vec::clear);
}

/// The colours changed: every icon is drawn again for the taskbar's.
pub(super) fn retint() {
  for t in trays() {
    t.draw_icon();
    t.modify();
  }
}

/// The taskbar is light (Settings › Personalization › Colors' Windows mode); dark when unset,
/// as before Windows 10 1903.
fn taskbar_light() -> bool {
  let key = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
  settings::dword(key, "SystemUsesLightTheme") == Some(1)
}

/// An icon of straight RGBA `px`, `side` pixels square: a 32-bit colour bitmap with its alpha,
/// and an empty mask.
fn hicon(side: i32, px: &[u8]) -> HICON {
  let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
  info.bmiHeader = BITMAPINFOHEADER {
    biSize: size_of::<BITMAPINFOHEADER>() as u32,
    biWidth: side,
    biHeight: -side,
    biPlanes: 1,
    biBitCount: 32,
    biCompression: BI_RGB,
    ..unsafe { std::mem::zeroed() }
  };
  let mut bits = null_mut();
  // SAFETY: a new section of `side²` pixels, filled before it is used.
  unsafe {
    let color = CreateDIBSection(null_mut(), &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    if color.is_null() {
      return null_mut();
    }

    let dst = std::slice::from_raw_parts_mut(bits.cast::<u8>(), px.len());
    for (d, s) in dst.as_chunks_mut::<4>().0.iter_mut().zip(px.as_chunks::<4>().0) {
      *d = [s[2], s[1], s[0], s[3]];
    }

    // The mask is unused with an alpha channel, but must hold defined bits: rows of whole words.
    let mask_bits = vec![0u8; (side as usize).div_ceil(16) * 2 * side as usize];
    let mask = CreateBitmap(side, side, 1, 1, mask_bits.as_ptr().cast());
    let ii = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
    let icon = CreateIconIndirect(&ii);
    DeleteObject(mask);
    DeleteObject(color);
    icon
  }
}

/// A popup menu of `items`, their commands numbered from `next` + 1 in [choices]' order.
fn build(items: &[Item], next: &mut u32) -> HMENU {
  let menu = unsafe { CreatePopupMenu() };
  for it in items {
    let text = wide(&text(it));
    let flags = flags(it);
    // A separator is only that, whatever else it carries.
    let id = match &it.submenu {
      _ if it.separator => 0,
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

  let mut f = MF_STRING;
  if it.submenu.is_some() {
    f |= MF_POPUP;
  }

  if it.checked {
    f |= MF_CHECKED;
  }

  if !it.enabled {
    f |= MF_GRAYED;
  }

  f
}

/// An item's text: its label with `&` doubled (one alone marks the access key), then its key
/// after a tab, which menus show right-aligned.
fn text(it: &Item) -> String {
  let label = it.label.replace('&', "&&");
  match it.key_label() {
    key if key.is_empty() => label,
    key => format!("{label}\t{key}"),
  }
}

/// `menu`'s items in the tray's shared format, a submenu's indented by `depth`.
fn describe(menu: HMENU, depth: usize, out: &mut Vec<String>) {
  let pad = "  ".repeat(depth);
  for i in 0..unsafe { GetMenuItemCount(menu) }.max(0) as u32 {
    let mut buf = [0u16; 256];
    let mut mi: MENUITEMINFOW = unsafe { std::mem::zeroed() };
    mi.cbSize = size_of::<MENUITEMINFOW>() as u32;
    mi.fMask = MIIM_FTYPE | MIIM_STATE | MIIM_SUBMENU | MIIM_STRING;
    mi.dwTypeData = buf.as_mut_ptr();
    mi.cch = buf.len() as u32 - 1;
    if unsafe { GetMenuItemInfoW(menu, i, 1, &mut mi) } == 0 {
      continue;
    }

    if mi.fType & MFT_SEPARATOR != 0 {
      out.push(format!("{pad}-"));
      continue;
    }

    let s = String::from_utf16_lossy(&buf[..mi.cch as usize]).replace("&&", "&");
    let (label, key) = s.split_once('\t').unwrap_or((&s, ""));
    let on = if mi.fState & MFS_CHECKED != 0 { "on" } else { "off" };
    let enabled = if mi.fState & MFS_DISABLED != 0 { "disabled" } else { "enabled" };
    out.push(format!("{pad}{label}|{key}|{on}|{enabled}"));
    if !mi.hSubMenu.is_null() {
      describe(mi.hSubMenu, depth + 1, out);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

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

  /// A separator's submenu, which [Item] says is ignored, takes no command numbers.
  #[test]
  fn a_separator_numbers_nothing() {
    let sep = Item { submenu: Some(Menu::new(vec![Item::new("x", "X")])), ..Item::separator() };
    let tray = Tray::detached(crate::tray::TrayIcon::path(4.0, "M0 0H4V4Z"), "t");
    tray.set_menu(&Menu::new(vec![sep, Item::new("quit", "Quit")]));
    let got = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let g = got.clone();
    tray.on_item(Box::new(move |_, _, id| g.borrow_mut().push(id.to_owned())));
    tray.choose(&[1]);
    assert_eq!(*got.borrow(), ["quit"]);
  }

  /// A menu dismissed (Escape, a click outside) chooses nothing and keeps the callback.
  #[test]
  fn a_dismissed_menu_keeps_the_callback() {
    let tray = Tray::detached(crate::tray::TrayIcon::path(4.0, "M0 0H4V4Z"), "t");
    tray.set_menu(&Menu::new(vec![Item::new("a", "A")]));
    let got = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let g = got.clone();
    tray.on_item(Box::new(move |_, _, id| g.borrow_mut().push(id.to_owned())));
    tray.run(None);
    tray.choose(&[0]);
    assert_eq!(*got.borrow(), ["a"]);
  }
}
