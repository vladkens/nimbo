// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The title bar over a unified window's content ([crate::Config::unified]): what the runtime is
//! told of it before each frame ([read]: its height and the traffic lights' frames as AppKit lays
//! them out now, so the content follows them through a toolbar's coming, full screen and another
//! macOS's metrics), and the empty toolbar that gives it a toolbar's height while the view builds
//! one ([sync], [crate::Ui::toolbar]). AppKit may lay the lights out after the frame that moved
//! them: the window hears it ([watch]) and draws again.
//!
//! An empty unified `NSToolbar` (as Electron's `hiddenInset` has, hidden in full screen too)
//! makes AppKit lay the bar
//! out as Finder's and Notes': 52 high on macOS 27, the lights 19 in and centred in it; the bar
//! has no item of its own, so every press in it reaches the view, which moves the window from its
//! empty part. In full screen the toolbar hides: AppKit would show it in a window of its own over
//! the content's top, taking the presses on the items drawn there; the lights come down with the
//! menu bar instead, over a band the view keeps free.

use objc2::MainThreadOnly;
use objc2::runtime::{AnyObject, Sel};
use objc2_app_kit::{
  NSTitlebarSeparatorStyle, NSToolbar, NSToolbarDisplayMode, NSView,
  NSViewFrameDidChangeNotification, NSWindow, NSWindowButton, NSWindowStyleMask,
  NSWindowToolbarStyle,
};
use objc2_foundation::{NSNotificationCenter, NSString};

use crate::TitleBar;
use crate::scene::Rect;

/// The title bar over `view`, the content of `window`, titled `title`: its height and the
/// traffic lights in the view's points, those that show over it.
pub(super) fn read(view: &NSView, window: &NSWindow, title: &'static str) -> TitleBar {
  // The frame's height over the layout rect's.
  let height = (window.frame().size.height - window.contentLayoutRect().size.height) as f32;
  let lights =
    [NSWindowButton::CloseButton, NSWindowButton::MiniaturizeButton, NSWindowButton::ZoomButton];
  let buttons = lights
    .into_iter()
    .filter_map(|b| window.standardWindowButton(b))
    // In full screen they are in the menu bar's window.
    .filter(|b| !b.isHidden() && b.window().is_some_and(|w| std::ptr::eq(&*w, window)))
    .map(|b| {
      let r = view.convertRect_fromView(b.convertRect_toView(b.bounds(), None), None);
      let (x, y) = (r.origin.x as f32, r.origin.y as f32);
      [x, y, x + r.size.width as f32, y + r.size.height as f32]
    })
    .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]);
  let buttons = buttons.map(|[x0, y0, x1, y1]| Rect::new(x0, y0, x1 - x0, y1 - y0));
  TitleBar { height, title, buttons }
}

/// Tells `observer` by `sel` (taking a notification) whenever AppKit moves the traffic lights:
/// a toolbar's coming or going, laid out after the frame that asked for it, another toolbar
/// style, the end of full screen.
pub(super) fn watch(window: &NSWindow, observer: &AnyObject, sel: Sel) {
  let Some(zoom) = window.standardWindowButton(NSWindowButton::ZoomButton) else { return };
  zoom.setPostsFrameChangedNotifications(true);
  let center = NSNotificationCenter::defaultCenter();
  let name = unsafe { NSViewFrameDidChangeNotification };
  // SAFETY: the observer has `sel`, taking a notification, and stops observing as its window
  // closes (`shell::closed`).
  unsafe { center.addObserver_selector_name_object(observer, sel, Some(name), Some(&zoom)) };
}

/// Puts an empty unified toolbar on `window` while the view builds a toolbar (`want`) and the
/// window isn't in full screen, hides it otherwise; returns whether the bar changed.
pub(super) fn sync(window: &NSWindow, want: bool) -> bool {
  let want = want && !window.styleMask().contains(NSWindowStyleMask::FullScreen);
  let bar = window.toolbar();
  if bar.as_ref().is_some_and(|t| t.isVisible()) == want {
    return false;
  }

  match bar {
    Some(bar) => bar.setVisible(want),
    None => {
      // One identifier per window: AppKit keeps toolbars of one identifier alike.
      let name = NSString::from_str(&format!("nimbo-{}", window.windowNumber()));
      let bar = NSToolbar::initWithIdentifier(NSToolbar::alloc(window.mtm()), &name);
      // Labels would make it 66 high.
      bar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
      window.setToolbar(Some(&bar));
      window.setToolbarStyle(NSWindowToolbarStyle::Unified);
      // No line under it: the content runs under the bar.
      window.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::None);
    }
  }

  true
}
