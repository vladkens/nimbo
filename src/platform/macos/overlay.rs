// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Overlays on macOS ([crate::Config::overlay]): a borderless non-activating `NSPanel` at the
//! status bar level that ignores the mouse, never becomes key or main, joins every Space and
//! shows over full-screen apps, placed on the screen with the pointer and ordered in front
//! without activating the app.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
  NSBackingStoreType, NSColor, NSEvent, NSPanel, NSScreen, NSStatusWindowLevel, NSWindow,
  NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect};

use crate::scene::Size;
use crate::shell::Anchor;

/// The panel of an overlay with `frame` as its content rect, transparent where the view's layer
/// is.
pub fn panel(mtm: MainThreadMarker, frame: NSRect) -> Retained<NSWindow> {
  let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
  let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
    NSPanel::alloc(mtm),
    frame,
    style,
    NSBackingStoreType::Buffered,
    false,
  );
  panel.setLevel(NSStatusWindowLevel);
  panel.setIgnoresMouseEvents(true);
  panel.setOpaque(false);
  panel.setBackgroundColor(Some(&NSColor::clearColor()));
  // The app draws its own shadow, inside the panel.
  panel.setHasShadow(false);
  panel.setHidesOnDeactivate(false);
  // Not `setFloatingPanel:`, which would bring the level down to floating.
  panel.setBecomesKeyOnlyIfNeeded(true);
  panel.setAnimationBehavior(NSWindowAnimationBehavior::None);
  panel.setCollectionBehavior(
    NSWindowCollectionBehavior::CanJoinAllSpaces
      | NSWindowCollectionBehavior::FullScreenAuxiliary
      | NSWindowCollectionBehavior::Stationary
      | NSWindowCollectionBehavior::IgnoresCycle,
  );
  Retained::into_super(panel)
}

/// Places the overlay on the screen with the pointer and orders it in front, activating nothing.
pub fn show(window: &NSWindow, anchor: Anchor) {
  place(window, anchor, true);
  window.orderFrontRegardless();
}

/// Moves the overlay to `anchor` in the visible frame (no menu bar, no Dock) of the screen with
/// the pointer, or of its own screen unless `pointer`.
pub fn place(window: &NSWindow, anchor: Anchor, pointer: bool) {
  let mtm = window.mtm();
  let screen = match pointer {
    true => {
      let p = NSEvent::mouseLocation();
      let inside = |r: NSRect| {
        let (o, s) = (r.origin, r.size);
        p.x >= o.x && p.x < o.x + s.width && p.y >= o.y && p.y < o.y + s.height
      };
      NSScreen::screens(mtm).iter().find(|s| inside(s.frame()))
    }
    false => window.screen(),
  };

  let Some(screen) = screen.or_else(|| NSScreen::mainScreen(mtm)) else { return };

  let (area, size) = (screen.visibleFrame(), window.frame().size);
  let points = |w: f64, h: f64| Size::new(w as f32, h as f32);
  let at =
    anchor.offset(points(area.size.width, area.size.height), points(size.width, size.height));
  // AppKit's y goes up from the bottom of the screen.
  let top = area.origin.y + area.size.height - at.y as f64;
  window.setFrameOrigin(NSPoint::new(area.origin.x + at.x as f64, top - size.height));
}
