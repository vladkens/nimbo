// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app's icon in the menu bar or the tray, with a menu the app describes ([Menu]) and
//! rebuilds when it changes or as it opens ([Tray::on_open]). A click on an item comes to
//! [Tray::on_item] by the item's id. Main thread only, while [crate::app] runs.
//!
//! | | macOS | Windows | Linux |
//! |---|---|---|---|
//! | icon | NSStatusItem, a template NSImage | `Shell_NotifyIconW`, tinted for the taskbar | StatusNotifierItem over D-Bus (GIO) |
//! | menu | NSMenu | `TrackPopupMenu` | `com.canonical.dbusmenu` |
//! | opens on | a click | a left or right click | as the panel shows menus (KDE: a click) |
//!
//! A Linux panel shows the item only with a StatusNotifierItem host: KDE Plasma, Xfce, LXQt,
//! Cinnamon and MATE have one, GNOME only with the AppIndicator extension (Ubuntu ships it on).
//! [available] says whether the session has one; the tray waits for one that starts later.

#[cfg(any(test, not(target_os = "macos")))]
mod icon;

use std::cell::{Cell, RefCell};

use crate::Main;
use crate::platform::tray as imp;

/// The icon, a template: only its alpha counts, and the OS's colour fills it (macOS tints it
/// for the menu bar and the highlight; Windows and Linux fill it white on a dark taskbar or
/// panel, black on a light one).
#[derive(Clone, Debug, PartialEq)]
pub enum TrayIcon {
  /// Straight (not premultiplied) RGBA, rows top-down, `scale` pixels per point (2 for an image
  /// drawn for Retina): 36 × 36 at 2 fills the menu bar's 18 pt.
  Rgba { w: u32, h: u32, scale: f32, px: Vec<u8> },
  /// SVG path data in a `size` × `size` view box, filled (non-zero, or even-odd), drawn at the
  /// size the OS shows it, so crisp at every scale. Arcs and curves are flattened
  /// ([crate::svg::flatten]).
  Path { size: f32, d: String, even_odd: bool },
}

impl TrayIcon {
  /// Path data `d` in a `size` box, filled non-zero.
  pub fn path(size: f32, d: &str) -> Self {
    Self::Path { size, d: d.into(), even_odd: false }
  }

  /// An icon file: an SVG of `<path>`s in a square view box ([crate::svg::document] says what
  /// it reads), drawn as [TrayIcon::Path].
  pub fn from_svg(svg: &str) -> Result<Self, String> {
    let (size, d, even_odd) = crate::svg::document(svg)?;
    Ok(Self::Path { size, d, even_odd })
  }

  /// The icon as `side` × `side` straight RGBA filled with `tint`: an RGBA image scaled by
  /// averaging, path data rasterized with 16 samples a pixel.
  #[cfg(any(test, not(target_os = "macos")))]
  pub(crate) fn pixels(&self, side: u32, tint: [u8; 3]) -> Vec<u8> {
    let alpha = match self {
      TrayIcon::Rgba { w, h, px, .. } => icon::scale_alpha(*w, *h, px, side),
      TrayIcon::Path { size, d, even_odd } => icon::fill(d, *size, side, *even_odd),
    };

    alpha.iter().flat_map(|&a| [tint[0], tint[1], tint[2], a]).collect()
  }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Menu {
  pub items: Vec<Item>,
}

impl Menu {
  pub fn new(items: Vec<Item>) -> Self {
    Self { items }
  }
}

/// A menu item: `id` comes to [Tray::on_item] when it is chosen (not for one with a submenu).
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
  pub id: String,
  pub label: String,
  pub enabled: bool,
  /// Shows a check mark when on.
  pub checked: bool,
  /// Can be checked ([Item::checked] sets it): Linux panels show an empty box while it is off,
  /// and a host adds the box to an item only as it builds it.
  pub checkable: bool,
  pub submenu: Option<Menu>,
  /// Its shortcut as [crate::Shortcut::parse] spells it (`cmd+comma`, `cmd+q`), shown beside it
  /// and, on macOS, working while the menu is open.
  pub key: Option<String>,
  /// A line between groups; the other fields are ignored.
  pub separator: bool,
}

impl Item {
  pub fn new(id: &str, label: &str) -> Self {
    Self {
      id: id.into(),
      label: label.into(),
      enabled: true,
      checked: false,
      checkable: false,
      submenu: None,
      key: None,
      separator: false,
    }
  }

  pub fn separator() -> Self {
    Self { separator: true, ..Self::new("", "") }
  }

  pub fn enabled(self, on: bool) -> Self {
    Self { enabled: on, ..self }
  }

  /// A check mark, shown when `on`.
  pub fn checked(self, on: bool) -> Self {
    Self { checked: on, checkable: true, ..self }
  }

  pub fn submenu(self, menu: Menu) -> Self {
    Self { submenu: Some(menu), ..self }
  }

  pub fn key(self, key: &str) -> Self {
    Self { key: Some(key.into()), ..self }
  }

  /// The label of [Item::key] as the OS shows it beside the item: `⌘,` on macOS, `Ctrl+,`
  /// elsewhere; empty when it does not parse.
  pub fn key_label(&self) -> String {
    let key = self.key.as_deref().and_then(|k| crate::Shortcut::parse(k).ok());
    key.map(|k| k.label()).unwrap_or_default()
  }
}

/// The items of `menu` that can be chosen, depth first, in the order they show: the item
/// numbers the OS menus use (an index here is a tag, a command id or a D-Bus menu id).
#[cfg(any(test, windows))]
pub(crate) fn choices(menu: &Menu) -> Vec<String> {
  fn walk(items: &[Item], out: &mut Vec<String>) {
    for it in items.iter().filter(|it| !it.separator) {
      match &it.submenu {
        Some(sub) => walk(&sub.items, out),
        None => out.push(it.id.clone()),
      }
    }
  }

  let mut out = Vec::new();
  walk(&menu.items, &mut out);
  out
}

type OnOpen = Box<dyn FnMut(&mut Main, &Tray)>;
type OnItem = Box<dyn FnMut(&mut Main, &Tray, &str)>;

/// What each platform's tray keeps alike: the app's callbacks, and while the menu is open, the
/// last menu set meanwhile, which waits for it to close.
#[derive(Default)]
pub(crate) struct State {
  on_open: RefCell<Option<OnOpen>>,
  on_item: RefCell<Option<OnItem>>,
  open: Cell<bool>,
  pending: RefCell<Option<Menu>>,
}

impl State {
  /// The menu is about to open: the app's [Tray::on_open] runs, kept for the next time.
  pub fn opening(&self, tray: &imp::Tray) {
    if let Some(mut f) = self.on_open.take() {
      f(&mut Main::here(), &Tray(tray.clone()));
      self.on_open.borrow_mut().get_or_insert(f);
    }
  }

  /// Item `id` was chosen: the app's [Tray::on_item] runs, kept for the next time.
  pub fn chosen(&self, tray: &imp::Tray, id: &str) {
    if let Some(mut f) = self.on_item.take() {
      f(&mut Main::here(), &Tray(tray.clone()), id);
      self.on_item.borrow_mut().get_or_insert(f);
    }
  }

  pub fn set_open(&self, open: bool) {
    self.open.set(open);
  }

  /// Whether `menu` waits, the menu being open (the last one set wins); if not, one that waited
  /// is dropped, `menu` to be built now.
  pub fn hold(&self, menu: &Menu) -> bool {
    if self.open.get() {
      *self.pending.borrow_mut() = Some(menu.clone());
      return true;
    }

    self.pending.take();
    false
  }

  /// The menu set while it was open, once it is closed: to be built now.
  pub fn released(&self) -> Option<Menu> {
    if self.open.get() { None } else { self.pending.take() }
  }

  /// A menu set while it was open still waits.
  #[cfg(target_os = "macos")]
  pub fn waiting(&self) -> bool {
    self.pending.borrow().is_some()
  }
}

/// The icon and its menu, shown while any handle lives: cheap to clone, so callbacks get one
/// and the app may keep one. Main thread only.
#[derive(Clone)]
pub struct Tray(pub(crate) imp::Tray);

impl Tray {
  /// Shows `icon` with an empty menu; `tooltip` is also what accessibility says of it.
  pub fn new(icon: TrayIcon, tooltip: &str) -> Self {
    Self(imp::Tray::new(icon, tooltip))
  }

  pub fn set_icon(&self, icon: TrayIcon) {
    self.0.set_icon(icon);
  }

  pub fn set_tooltip(&self, tooltip: &str) {
    self.0.set_tooltip(tooltip);
  }

  /// Replaces the menu, also while it opens ([Tray::on_open]). While it is open the new one waits
  /// until it closes (the last one set wins): rebuilt, an open menu would close its submenus.
  pub fn set_menu(&self, menu: &Menu) {
    self.0.set_menu(menu);
  }

  /// `f` runs as the menu is about to open: the moment to refresh it with [Tray::set_menu].
  pub fn on_open(&self, f: impl FnMut(&mut Main, &Tray) + 'static) {
    *self.0.state().on_open.borrow_mut() = Some(Box::new(f));
  }

  /// `f` runs with the id of the item chosen, after the menu closed.
  pub fn on_item(&self, f: impl FnMut(&mut Main, &Tray, &str) + 'static) {
    *self.0.state().on_item.borrow_mut() = Some(Box::new(f));
  }
}

/// Whether the OS shows a tray icon now: `Err` with the reason when it doesn't (Linux: no
/// StatusNotifierItem host on the session bus). A [Tray] made anyway shows once a host starts.
pub fn available() -> Result<(), String> {
  imp::available()
}

/// For the tray scenario (`tests/tray.rs`): a tray with nothing on screen, the menu as the OS
/// got it, and its items chosen without opening it.
#[doc(hidden)]
impl Tray {
  pub fn detached(icon: TrayIcon, tooltip: &str) -> Self {
    Self(imp::Tray::detached(icon, tooltip))
  }

  /// The icon as `icon <w>x<h>` and what else the OS keeps of it, then each item as
  /// `label|key|on or off|enabled or disabled` (`-` a separator), a submenu's items indented.
  pub fn describe(&self) -> Vec<String> {
    self.0.describe()
  }

  /// Does what the OS does as the menu opens (it stays open until [Tray::closing]).
  pub fn opening(&self) {
    self.0.opening();
  }

  /// Does what the OS does as the menu closes.
  pub fn closing(&self) {
    self.0.closing();
  }

  /// Chooses the item at `path` (indices into the menu, then its submenus).
  pub fn choose(&self, path: &[usize]) {
    self.0.choose(path);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// An SVG file's ring of two paths fills even-odd, as the file draws it.
  #[test]
  fn an_svg_file_fills_as_drawn() {
    let svg = r#"<svg viewBox="0 0 24 24">
      <path fill-rule="evenodd" d="M2 12a10 10 0 1 0 20 0a10 10 0 1 0-20 0Z"/>
      <path fill-rule="evenodd" d="M7 12a5 5 0 1 0 10 0a5 5 0 1 0-10 0Z"/></svg>"#;
    let px = TrayIcon::from_svg(svg).unwrap().pixels(24, [1, 2, 3]);
    let alpha = |x: usize, y: usize| px[(y * 24 + x) * 4 + 3];
    assert_eq!((alpha(12, 12), alpha(12, 4), alpha(0, 0)), (0, 255, 0));
    assert_eq!(px[..3], [1, 2, 3]);
    assert!(TrayIcon::from_svg("<svg/>").is_err());
  }

  /// A menu set while it is open waits for it to close, the last one set winning; one set
  /// once it is closed drops what waited.
  #[test]
  fn a_menu_set_while_open_waits() {
    let (a, b) = (Menu::new(vec![Item::new("a", "A")]), Menu::new(vec![Item::new("b", "B")]));
    let s = State::default();
    assert!(!s.hold(&a));
    s.set_open(true);
    assert!(s.hold(&a) && s.hold(&b));
    assert_eq!(s.released(), None, "still open");
    s.set_open(false);
    assert_eq!(s.released(), Some(b.clone()));
    assert_eq!(s.released(), None);

    s.set_open(true);
    s.hold(&a);
    s.set_open(false);
    assert!(!s.hold(&b));
    assert_eq!(s.released(), None);
  }

  #[test]
  fn choices_are_numbered_depth_first() {
    let sub = Menu::new(vec![Item::new("a", "A"), Item::separator(), Item::new("b", "B")]);
    let menu = Menu::new(vec![
      Item::new("top", "Top"),
      Item::separator(),
      Item::new("sub", "Sub").submenu(sub),
      Item::new("quit", "Quit").key("cmd+q"),
    ]);
    assert_eq!(choices(&menu), ["top", "a", "b", "quit"]);
    let label = if cfg!(target_os = "macos") { "⌘Q" } else { "Ctrl+Q" };
    assert_eq!(menu.items[3].key_label(), label);
    assert_eq!(Item::new("x", "X").key("cmd+nope").key_label(), "");
  }

  #[test]
  fn icons_tint_their_alpha() {
    let px = [255, 0, 0, 255, 0, 0, 0, 0].repeat(2);
    let icon = TrayIcon::Rgba { w: 2, h: 2, scale: 1.0, px };
    assert_eq!(icon.pixels(2, [1, 2, 3])[..8], [1, 2, 3, 255, 1, 2, 3, 0]);
    let square = TrayIcon::path(4.0, "M0 0H4V4H0Z");
    assert!(square.pixels(8, [0; 3]).chunks(4).all(|p| p[3] == 255));
  }
}
