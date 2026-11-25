// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app's icon in the menu bar or the tray, with a menu the app describes ([Menu]) and
//! rebuilds when it changes or as it opens ([Tray::on_open]) and closes ([Tray::on_close]). A click
//! on an item comes to [Tray::on_item] by the item's id. Main thread only, while [crate::app()]
//! runs.
//!
//! | | macOS | Windows | Linux |
//! |---|---|---|---|
//! | icon | NSStatusItem, a template NSImage | `Shell_NotifyIconW`, tinted for the taskbar | StatusNotifierItem over D-Bus (GIO), an SVG the panel colours |
//! | title | the NSStatusItem's | none | `XAyatanaLabel`, where the panel shows it |
//! | menu | NSMenu | `TrackPopupMenu` | `com.canonical.dbusmenu` |
//! | opens on | a click | a left or right click | as the panel shows menus (KDE: a click) |
//!
//! A Linux panel shows the item only with a StatusNotifierItem host: KDE Plasma, Xfce, LXQt,
//! Cinnamon and MATE have one, GNOME only with the AppIndicator extension (Ubuntu ships it on).
//! [available] says whether the session has one; the tray waits for one that starts later.
//! [Item::button] fits a long name (a device, a file) to an item.

#[cfg(any(test, not(target_os = "macos")))]
mod icon;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;

pub use crate::menu::{Item, LABEL, Menu};
use crate::platform::tray as imp;
use crate::{Main, svg};

/// The icon, a template: only its alpha counts, and the OS's colour fills it (macOS tints it
/// for the menu bar and the highlight; Windows fills it white on a dark taskbar, black on a light
/// one; a Linux panel with its text colour).
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
    let (size, d, even_odd) = svg::document(svg)?;
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

  /// The icon as an SVG a Linux panel fills with its text colour ([icon::symbolic]): path data
  /// as it is, an image as its alpha at `side` pixels.
  #[cfg(any(test, target_os = "linux"))]
  pub(crate) fn svg(&self, side: u32, tint: [u8; 3]) -> String {
    match self {
      TrayIcon::Rgba { w, h, px, .. } => icon::symbolic(
        side as f32,
        &icon::runs(&icon::scale_alpha(*w, *h, px, side), side as usize),
        tint,
      ),
      TrayIcon::Path { size, d, even_odd } => {
        icon::symbolic(*size, &[(d.clone(), *even_odd, 1.0)], tint)
      }
    }
  }
}

type OnMenu = Box<dyn FnMut(&mut Main, &imp::Tray)>;
type OnItem = Box<dyn FnMut(&mut Main, &imp::Tray, usize)>;
/// A menu's actions by number ([Menu::tagged]), of the app's type.
type Actions = Option<Box<dyn Any>>;

/// What becomes of a menu set ([State::set_menu]).
pub(crate) enum Set {
  Build,
  /// The open menu takes its labels, checks and enabled states.
  Update,
  Wait,
}

/// What each platform's tray keeps alike: the app's callbacks; the menu shown and the actions
/// of the menu built, by number ([Menu::tagged]), and of the one set last, which go with it once
/// it is built; and while the menu is open, the last menu set meanwhile, which waits for it to
/// close.
#[derive(Default)]
pub(crate) struct State {
  on_open: RefCell<Option<OnMenu>>,
  on_close: RefCell<Option<OnMenu>>,
  on_item: RefCell<Option<OnItem>>,
  open: Cell<bool>,
  shown: RefCell<Option<Menu<usize>>>,
  built: RefCell<Actions>,
  next: RefCell<Actions>,
  /// The next actions are the built ones: an open menu of the same items keeps their numbers.
  same_actions: Cell<bool>,
  /// The menu shown has the items and actions of the one before it: their ids may stay.
  kept: Cell<bool>,
  pending: RefCell<Option<(Menu<usize>, Actions, bool)>>,
}

/// Runs the app's callback in `slot`, kept for the next time unless it set another.
fn call(slot: &RefCell<Option<OnMenu>>, tray: &imp::Tray) {
  if let Some(mut f) = slot.take() {
    f(&mut Main::here(), tray);
    slot.borrow_mut().get_or_insert(f);
  }
}

impl State {
  /// The menu is about to open: the app's [Tray::on_open] runs.
  pub fn opening(&self, tray: &imp::Tray) {
    call(&self.on_open, tray);
  }

  /// The menu closed, if it was open: the app's [Tray::on_close] runs, a menu it sets going with
  /// the one set while it was open, which is to be built now.
  pub fn closed(&self, tray: &imp::Tray) -> Option<Menu<usize>> {
    if !self.open.get() {
      return None;
    }

    call(&self.on_close, tray);
    self.open.set(false);
    self.take_pending()
  }

  /// The item of action number `n` was chosen: the app's [Tray::on_item] runs, kept for the
  /// next time.
  pub fn chosen(&self, tray: &imp::Tray, n: usize) {
    if let Some(mut f) = self.on_item.take() {
      f(&mut Main::here(), tray, n);
      self.on_item.borrow_mut().get_or_insert(f);
    }
  }

  pub fn set_open(&self, open: bool) {
    self.open.set(open);
  }

  /// What becomes of `menu`: while the menu is open it waits (the last one set wins), unless the
  /// platform changes an open menu's items (`in_place`) and it has the same items and actions as
  /// the one shown ([Menu::same_items]); then, or when the menu is closed, one that waited is
  /// dropped, `menu`'s actions taking the built one's place.
  pub fn set_menu(&self, menu: &Menu<usize>, in_place: bool) -> Set {
    let next = self.next.take();
    let open = self.open.get();
    let same =
      self.same_actions.get() && self.shown.borrow().as_ref().is_some_and(|m| m.same_items(menu));
    if open && !(in_place && same) {
      *self.pending.borrow_mut() = Some((menu.clone(), next, same));
      return Set::Wait;
    }

    self.pending.take();
    self.show(menu.clone(), next, same);
    if open { Set::Update } else { Set::Build }
  }

  /// The menu set while it was open, open or not: a host fetching the menu builds it anew.
  pub fn take_pending(&self) -> Option<Menu<usize>> {
    let (menu, actions, same) = self.pending.take()?;
    self.show(menu.clone(), actions, same);
    Some(menu)
  }

  fn show(&self, menu: Menu<usize>, actions: Actions, same: bool) {
    *self.shown.borrow_mut() = Some(menu);
    *self.built.borrow_mut() = actions;
    self.kept.set(same);
  }

  /// The menu shown has the items and actions of the one before it ([Menu::same_items]).
  #[cfg(target_os = "linux")]
  pub fn kept(&self) -> bool {
    self.kept.get()
  }

  #[cfg(target_os = "macos")]
  pub fn is_open(&self) -> bool {
    self.open.get()
  }
}

/// The icon and its menu of actions of the app's type `A` ([Menu]), shown while any handle
/// lives: cheap to clone, so callbacks get one and the app may keep one. Main thread only.
pub struct Tray<A>(pub(crate) imp::Tray, pub(crate) PhantomData<fn(A) -> A>);

impl<A> Clone for Tray<A> {
  fn clone(&self) -> Self {
    Self(self.0.clone(), PhantomData)
  }
}

impl<A: Clone + PartialEq + 'static> Tray<A> {
  /// Shows `icon` with an empty menu; `tooltip` is also what accessibility says of it.
  pub fn new(icon: TrayIcon, tooltip: &str) -> Self {
    Self(imp::Tray::new(icon, tooltip), PhantomData)
  }

  pub fn set_icon(&self, icon: TrayIcon) {
    self.0.set_icon(icon);
  }

  pub fn set_tooltip(&self, tooltip: &str) {
    self.0.set_tooltip(tooltip);
  }

  /// Text beside the icon (a timer's time left), empty for none, as an NSStatusItem's title: in
  /// the menu bar's font, its digits of one width so that a countdown keeps its place. Linux shows
  /// it on panels that show an AppIndicator's label (GNOME's), Windows nowhere: the tooltip is
  /// the place for what must be read everywhere.
  pub fn set_title(&self, title: &str) {
    self.0.set_title(title);
  }

  /// Replaces the menu, also while it opens ([Tray::on_open]). While it is open, one of the same
  /// items but for labels, checks and enabled states ([Item]) changes it in place on macOS, its
  /// submenus staying open (a countdown in it); any other waits until it closes (the last one set
  /// wins): rebuilt, an open menu would close its submenus.
  pub fn set_menu(&self, menu: &Menu<A>) {
    let (menu, actions) = menu.tagged();
    let state = self.0.state();
    let built = state.built.borrow();
    let same =
      built.as_ref().and_then(|b| b.downcast_ref::<Vec<A>>()).is_some_and(|b| *b == actions);
    drop(built);
    state.same_actions.set(same);
    *state.next.borrow_mut() = Some(Box::new(actions));
    self.0.set_menu(&menu);
  }

  /// `f` runs as the menu is about to open: the moment to refresh it with [Tray::set_menu].
  pub fn on_open(&self, mut f: impl FnMut(&mut Main, &Tray<A>) + 'static) {
    let run = move |main: &mut Main, tray: &imp::Tray| f(main, &Tray(tray.clone(), PhantomData));
    *self.0.state().on_open.borrow_mut() = Some(Box::new(run));
  }

  /// `f` runs as the menu closes, after the chosen item's action (KDE says the menu closed before
  /// the click): the moment to stop what only an open menu needs (a countdown in it). A menu it
  /// sets takes the place of one set while the menu was open. Linux panels that don't say the
  /// menu closed run it on a click or as the menu opens next.
  pub fn on_close(&self, mut f: impl FnMut(&mut Main, &Tray<A>) + 'static) {
    let run = move |main: &mut Main, tray: &imp::Tray| f(main, &Tray(tray.clone(), PhantomData));
    *self.0.state().on_close.borrow_mut() = Some(Box::new(run));
  }

  /// `f` runs with the action of the item chosen as the menu closes: a menu it sets waits for
  /// the menu to close, as one set while it is open.
  pub fn on_item(&self, mut f: impl FnMut(&mut Main, &Tray<A>, A) + 'static) {
    let run = move |main: &mut Main, tray: &imp::Tray, n: usize| {
      // The actions of the menu shown, of this tray's type: set only by [Tray::set_menu].
      let built = tray.state().built.borrow();
      let action = built.as_ref().and_then(|b| b.downcast_ref::<Vec<A>>()?.get(n).cloned());
      drop(built);
      if let Some(a) = action {
        f(main, &Tray(tray.clone(), PhantomData), a);
      }
    };
    *self.0.state().on_item.borrow_mut() = Some(Box::new(run));
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
impl<A: Clone + PartialEq + 'static> Tray<A> {
  pub fn detached(icon: TrayIcon, tooltip: &str) -> Self {
    Self(imp::Tray::detached(icon, tooltip), PhantomData)
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

  /// A menu set while it is open waits for it to close, the last one set winning, or with the
  /// same items and actions changes it in place where the platform can, their ids kept; one set
  /// once it is closed drops what waited.
  #[test]
  fn a_menu_set_while_open_waits() {
    let menu = |label| Menu::new(vec![Item::button(0, label)]);
    let (a, b) = (menu("A"), Menu::new(vec![Item::toggle(0, "B", true)]));
    let s = State::default();
    let actions = |s: &State| s.built.borrow().as_ref()?.downcast_ref::<Vec<&str>>().cloned();
    let set = |s: &State, m: &Menu<usize>, v: &'static str, in_place| {
      s.same_actions.set(actions(s) == Some(vec![v]));
      *s.next.borrow_mut() = Some(Box::new(vec![v]));
      s.set_menu(m, in_place)
    };

    assert!(matches!(set(&s, &a, "a", true), Set::Build));
    assert_eq!(actions(&s), Some(vec!["a"]), "built at once");
    s.set_open(true);
    assert!(matches!(set(&s, &menu("A 2"), "a2", false), Set::Wait), "not in place");
    assert!(matches!(set(&s, &b, "b", true), Set::Wait), "other items");
    assert_eq!(actions(&s), Some(vec!["a"]), "the open menu's");
    assert_eq!(s.take_pending(), Some(b.clone()), "the last one set");
    assert_eq!(actions(&s), Some(vec!["b"]), "the one built now");
    assert_eq!(s.take_pending(), None);

    assert!(matches!(set(&s, &a, "a", true), Set::Wait));
    assert!(matches!(set(&s, &b, "c", true), Set::Wait), "another action");
    let b2 = Menu::new(vec![Item::toggle(0, "B 2", false)]);
    assert!(matches!(set(&s, &b2, "b", true), Set::Update), "the same items as shown");
    assert!(s.kept.get(), "their ids stay");
    assert_eq!(s.take_pending(), None, "what waited is dropped");
    set(&s, &b, "b", false);
    s.set_open(false);
    assert!(matches!(set(&s, &a, "a", false), Set::Build));
    assert_eq!(s.take_pending(), None);
    assert!(!s.kept.get(), "other items");
  }

  /// Path data goes in an icon's SVG as it is, an image as runs of its alpha.
  #[test]
  fn icons_as_svgs() {
    let ring = TrayIcon::Path { size: 18.0, d: "M0 0H9V9Z".into(), even_odd: true };
    let svg = ring.svg(44, [0; 3]);
    assert!(
      svg.contains(r#"viewBox="0 0 18 18""#)
        && svg.contains(r#"evenodd" fill-opacity="1" d="M0 0H9V9Z""#),
      "{svg}"
    );
    let dot = TrayIcon::Rgba { w: 1, h: 1, scale: 1.0, px: vec![0, 0, 0, 255] };
    assert!(dot.svg(2, [0; 3]).contains(r#"d="M0 0h2v1H0zM0 1h2v1H0z""#), "{}", dot.svg(2, [0; 3]));
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
