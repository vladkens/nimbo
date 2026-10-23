// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The tray on macOS: an NSStatusItem with an NSMenu built from [Menu], its delegate asking the
//! app as it opens (`menuNeedsUpdate:`) and taking the chosen item (`chosen:`, by tag); the icon
//! a template NSImage, from RGBA through CoreGraphics, or path data filled whenever AppKit draws
//! it. A menu set while the menu is open (`menuWillOpen:` to `menuDidClose:`) waits for it to
//! close: rebuilding an open NSMenu closes its open submenus. It is built from the main queue
//! after `menuDidClose:`, where AppKit forbids changing the menu, and after the chosen item's
//! action, which still finds its id.

use std::cell::{Cell, RefCell};
use std::ptr::null_mut;
use std::rc::{Rc, Weak};

use block2::RcBlock;
use dispatch2::{DispatchQueue, MainThreadBound};
use keyboard_types::{Modifiers, NamedKey};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{
  AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use objc2_app_kit::{
  NSBezierPath, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSEventModifierFlags,
  NSImage, NSMenu, NSMenuDelegate, NSMenuItem, NSStatusBar, NSStatusItem,
  NSVariableStatusItemLength, NSWindingRule,
};
use objc2_core_foundation::CFRunLoop;
use objc2_core_graphics::{
  CGBitmapContextCreate, CGBitmapContextCreateImage, CGBitmapContextGetBytesPerRow,
  CGBitmapContextGetData, CGColorSpace, CGImageAlphaInfo, kCGColorSpaceSRGB,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use super::super::mtm;
use crate::Main;
use crate::keys::{Key, Shortcut, f_key};
use crate::svg::flatten;
use crate::tray::{Item, Menu, OnItem, OnOpen, TrayIcon};

/// The menu bar's icon box, in points.
const SIZE: NSSize = NSSize::new(18.0, 18.0);

struct Inner {
  mtm: MainThreadMarker,
  /// `None` for a detached tray (tests): nothing on screen.
  item: Option<Retained<NSStatusItem>>,
  menu: Retained<NSMenu>,
  delegate: Retained<Delegate>,
  image: RefCell<Retained<NSImage>>,
  /// The ids of the menu's items, by tag.
  ids: RefCell<Vec<String>>,
  on_open: RefCell<Option<OnOpen>>,
  on_item: RefCell<Option<OnItem>>,
  /// Between `menuWillOpen:` and `menuDidClose:`.
  open: Cell<bool>,
  /// The last menu set while it was open.
  pending: RefCell<Option<Menu>>,
}

impl Drop for Inner {
  fn drop(&mut self) {
    if let Some(item) = &self.item {
      NSStatusBar::systemStatusBar().removeStatusItem(item);
    }
  }
}

#[derive(Clone)]
pub struct Tray(Rc<Inner>);

define_class!(
  // SAFETY: NSObject has no subclassing requirements; we implement only delegate methods.
  #[unsafe(super(NSObject))]
  #[thread_kind = MainThreadOnly]
  #[name = "NimboTrayDelegate"]
  #[ivars = RefCell<Weak<Inner>>]
  struct Delegate;

  unsafe impl NSObjectProtocol for Delegate {}

  unsafe impl NSMenuDelegate for Delegate {
    #[unsafe(method(menuNeedsUpdate:))]
    fn menu_needs_update(&self, _menu: &NSMenu) {
      let Some(tray) = self.tray() else { return };
      let Some(mut f) = tray.0.on_open.take() else { return };
      f(&mut Main::here(), &crate::tray::Tray(tray.clone()));
      tray.0.on_open.borrow_mut().get_or_insert(f);
    }

    #[unsafe(method(menuWillOpen:))]
    fn menu_will_open(&self, _menu: &NSMenu) {
      if let Some(tray) = self.tray() {
        tray.0.open.set(true);
      }
    }

    #[unsafe(method(menuDidClose:))]
    fn menu_did_close(&self, _menu: &NSMenu) {
      let Some(tray) = self.tray() else { return };
      tray.0.open.set(false);
      let weak = MainThreadBound::new(Rc::downgrade(&tray.0), tray.0.mtm);
      DispatchQueue::main().exec_async(move || {
        let mtm = MainThreadMarker::new().unwrap();
        let Some(tray) = weak.into_inner(mtm).upgrade().map(Tray) else { return };
        if !tray.0.open.get()
          && let Some(menu) = tray.0.pending.take()
        {
          tray.build(&menu);
        }
      });
    }
  }

  impl Delegate {
    #[unsafe(method(chosen:))]
    fn chosen(&self, sender: Option<&AnyObject>) {
      let Some(item) = sender.and_then(|s| s.downcast_ref::<NSMenuItem>()) else { return };
      let Some(tray) = self.tray() else { return };
      let id = tray.0.ids.borrow().get(item.tag() as usize).cloned();
      let (Some(id), Some(mut f)) = (id, tray.0.on_item.take()) else { return };
      f(&mut Main::here(), &crate::tray::Tray(tray.clone()), &id);
      tray.0.on_item.borrow_mut().get_or_insert(f);
    }
  }
);

impl Delegate {
  fn new(mtm: MainThreadMarker) -> Retained<Self> {
    let this = Self::alloc(mtm).set_ivars(RefCell::default());
    unsafe { msg_send![super(this), init] }
  }

  fn tray(&self) -> Option<Tray> {
    self.ivars().borrow().upgrade().map(Tray)
  }
}

/// The menu bar always has room for a status item.
pub fn available() -> Result<(), String> {
  Ok(())
}

impl Tray {
  pub fn new(icon: TrayIcon, tooltip: &str) -> Self {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    let tray = Self::make(mtm(), Some(item), icon, tooltip);
    if let Some(item) = &tray.0.item {
      item.setMenu(Some(&tray.0.menu));
    }

    tray
  }

  pub fn detached(icon: TrayIcon, tooltip: &str) -> Self {
    Self::make(mtm(), None, icon, tooltip)
  }

  fn make(
    mtm: MainThreadMarker,
    item: Option<Retained<NSStatusItem>>,
    icon: TrayIcon,
    tooltip: &str,
  ) -> Self {
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    let delegate = Delegate::new(mtm);
    menu.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    let inner = Inner {
      mtm,
      item,
      menu,
      delegate,
      image: RefCell::new(image(&icon)),
      ids: RefCell::default(),
      on_open: RefCell::default(),
      on_item: RefCell::default(),
      open: Cell::new(false),
      pending: RefCell::default(),
    };
    let tray = Self(Rc::new(inner));
    *tray.0.delegate.ivars().borrow_mut() = Rc::downgrade(&tray.0);
    tray.show_icon();
    tray.set_tooltip(tooltip);
    tray
  }

  fn show_icon(&self) {
    let Some(button) = self.0.item.as_ref().and_then(|i| i.button(self.0.mtm)) else { return };
    button.setImage(Some(&self.0.image.borrow()));
  }

  pub fn set_icon(&self, icon: TrayIcon) {
    let description = self.0.image.borrow().accessibilityDescription();
    let img = image(&icon);
    img.setAccessibilityDescription(description.as_deref());
    *self.0.image.borrow_mut() = img;
    self.show_icon();
  }

  pub fn set_tooltip(&self, tooltip: &str) {
    let s = NSString::from_str(tooltip);
    self.0.image.borrow().setAccessibilityDescription(Some(&s));
    if let Some(button) = self.0.item.as_ref().and_then(|i| i.button(self.0.mtm)) {
      button.setToolTip(Some(&s));
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
    let mut ids = Vec::new();
    self.0.menu.removeAllItems();
    fill(self.0.mtm, &self.0.menu, &menu.items, &self.0.delegate, &mut ids);
    *self.0.ids.borrow_mut() = ids;
  }

  pub fn on_open(&self, f: OnOpen) {
    *self.0.on_open.borrow_mut() = Some(f);
  }

  pub fn on_item(&self, f: OnItem) {
    *self.0.on_item.borrow_mut() = Some(f);
  }

  pub fn describe(&self) -> Vec<String> {
    fn walk(menu: &NSMenu, depth: usize, out: &mut Vec<String>) {
      for item in menu.itemArray() {
        let pad = "  ".repeat(depth);
        if item.isSeparatorItem() {
          out.push(format!("{pad}-"));
          continue;
        }

        let key = item.keyEquivalent().to_string();
        let mods = item.keyEquivalentModifierMask();
        let held = [
          (NSEventModifierFlags::Control, "⌃"),
          (NSEventModifierFlags::Option, "⌥"),
          (NSEventModifierFlags::Shift, "⇧"),
          (NSEventModifierFlags::Command, "⌘"),
        ];
        let held: String =
          held.iter().filter(|(m, _)| !key.is_empty() && mods.contains(*m)).map(|m| m.1).collect();
        let on = if item.state() == NSControlStateValueOn { "on" } else { "off" };
        let enabled = if item.isEnabled() { "enabled" } else { "disabled" };
        out.push(format!("{pad}{}|{held}{key}|{on}|{enabled}", item.title()));
        if let Some(sub) = item.submenu() {
          walk(&sub, depth + 1, out);
        }
      }
    }

    let image = self.0.image.borrow();
    let (size, template) = (image.size(), image.isTemplate());
    let mut out = vec![format!("icon {}x{} template={template}", size.width, size.height)];
    walk(&self.0.menu, 0, &mut out);
    out
  }

  pub fn opening(&self) {
    let (delegate, menu) = (&*self.0.delegate, &*self.0.menu);
    let _: () = unsafe { msg_send![delegate, menuNeedsUpdate: menu] };
    let _: () = unsafe { msg_send![delegate, menuWillOpen: menu] };
  }

  /// `menuDidClose:`, then the main queue's work it left (the menu set meanwhile).
  pub fn closing(&self) {
    let _: () = unsafe { msg_send![&*self.0.delegate, menuDidClose: &*self.0.menu] };
    // SAFETY: a static of the CoreFoundation framework.
    let mode = unsafe { objc2_core_foundation::kCFRunLoopDefaultMode };
    for _ in 0..50 {
      if self.0.pending.borrow().is_none() {
        break;
      }

      CFRunLoop::run_in_mode(mode, 0.01, true);
    }
  }

  pub fn choose(&self, path: &[usize]) {
    let Some((last, parents)) = path.split_last() else { return };
    let mut menu = self.0.menu.clone();
    for &i in parents {
      let Some(sub) = menu.itemAtIndex(i as isize).and_then(|item| item.submenu()) else { return };
      menu = sub;
    }

    menu.performActionForItemAtIndex(*last as isize);
  }
}

/// Adds `items` to `menu`; the chosen ones send `chosen:` to `target` with their tag, an index
/// into `ids`.
fn fill(
  mtm: MainThreadMarker,
  menu: &NSMenu,
  items: &[Item],
  target: &Delegate,
  ids: &mut Vec<String>,
) {
  for it in items {
    if it.separator {
      menu.addItem(&NSMenuItem::separatorItem(mtm));
      continue;
    }

    let action = it.submenu.is_none().then_some(sel!(chosen:));
    let none = (String::new(), NSEventModifierFlags::empty());
    let (key, mods) = it.key.as_deref().and_then(key_equivalent).unwrap_or(none);
    // SAFETY: the action is the target's, which lives as long as the tray.
    let item = unsafe {
      NSMenuItem::initWithTitle_action_keyEquivalent(
        NSMenuItem::alloc(mtm),
        &NSString::from_str(&it.label),
        action,
        &NSString::from_str(&key),
      )
    };
    if action.is_some() {
      let target: &AnyObject = target;
      unsafe { item.setTarget(Some(target)) };
      item.setTag(ids.len() as isize);
      ids.push(it.id.clone());
    }

    if !key.is_empty() {
      item.setKeyEquivalentModifierMask(mods);
    }

    item.setEnabled(it.enabled);
    item.setState(if it.checked { NSControlStateValueOn } else { NSControlStateValueOff });
    if let Some(sub) = &it.submenu {
      let submenu = NSMenu::new(mtm);
      submenu.setAutoenablesItems(false);
      fill(mtm, &submenu, &sub.items, target, ids);
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

// MARK: Icons

/// The template image of `icon`.
fn image(icon: &TrayIcon) -> Retained<NSImage> {
  let image = match icon {
    TrayIcon::Rgba { w, h, scale, px } => rgba(*w, *h, *scale, px),
    TrayIcon::Path { size, d, even_odd } => vector(*size, d, *even_odd),
  };

  image.setTemplate(true);
  image
}

/// RGBA pixels, premultiplied into a CGImage; a blank image for a short buffer.
fn rgba(w: u32, h: u32, scale: f32, px: &[u8]) -> Retained<NSImage> {
  let points = NSSize::new((w as f32 / scale) as f64, (h as f32 / scale) as f64);
  let blank = || NSImage::initWithSize(NSImage::alloc(), points);
  let n = (w * h * 4) as usize;
  if px.len() < n || w == 0 || h == 0 {
    return blank();
  }

  let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }));
  let info = CGImageAlphaInfo::PremultipliedLast.0;
  // SAFETY: no data: the context allocates and owns its buffer, which the image may share.
  let ctx = unsafe {
    CGBitmapContextCreate(null_mut(), w as usize, h as usize, 8, 0, space.as_deref(), info)
  };

  let Some(ctx) = ctx else { return blank() };
  let (dst, stride) =
    (CGBitmapContextGetData(Some(&ctx)), CGBitmapContextGetBytesPerRow(Some(&ctx)));
  if dst.is_null() {
    return blank();
  }

  // SAFETY: the context's buffer is `stride` × `h` bytes, only ours while we fill it.
  let dst = unsafe { std::slice::from_raw_parts_mut(dst.cast::<u8>(), stride * h as usize) };
  for (row, src) in dst.chunks_exact_mut(stride).zip(px[..n].chunks_exact(w as usize * 4)) {
    for (d, s) in row.as_chunks_mut::<4>().0.iter_mut().zip(src.as_chunks::<4>().0) {
      let a = s[3] as u32;
      let pre = |c: u8| ((c as u32 * a + 127) / 255) as u8;
      *d = [pre(s[0]), pre(s[1]), pre(s[2]), s[3]];
    }
  }

  match CGBitmapContextCreateImage(Some(&ctx)) {
    Some(cg) => NSImage::initWithCGImage_size(NSImage::alloc(), &cg, points),
    None => blank(),
  }
}

/// An image that fills path data `d` (in a `size` box) whenever it is drawn, flattened finely
/// enough for a Retina menu bar.
fn vector(size: f32, d: &str, even_odd: bool) -> Retained<NSImage> {
  let k = SIZE.width / size.max(f32::EPSILON) as f64;
  let mut polys = Vec::new();
  _ = flatten(d, (k * 4.0) as f32, &mut polys);
  let draw = RcBlock::new(move |_rect: NSRect| {
    NSColor::blackColor().setFill();
    let path = NSBezierPath::bezierPath();
    for poly in &polys {
      let p = |i: usize| NSPoint::new(poly[i].x as f64 * k, poly[i].y as f64 * k);
      path.moveToPoint(p(0));
      for i in 1..poly.len() {
        path.lineToPoint(p(i));
      }

      path.closePath();
    }

    if even_odd {
      path.setWindingRule(NSWindingRule::EvenOdd);
    }

    path.fill();
    Bool::YES
  });
  // Flipped: the path's y grows downward.
  NSImage::imageWithSize_flipped_drawingHandler(SIZE, true, &draw)
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
