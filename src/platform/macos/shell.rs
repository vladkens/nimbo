// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app on AppKit: NSApplication with a minimal app menu and its delegate, the windows by id
//! (an NSWindow each, its content our [View] with its own runtime, its own delegate), the
//! activation policy that follows [Presence], and the main dispatch queue for [crate::Proxy].
//!
//! The app ends one of two ways, its quit hook ([Main::on_quit]) running in both. [quit] stops
//! the loop and `run` returns. A quit asked by the system (the Dock's Quit, logout, Activity
//! Monitor, AppleScript `quit`) comes as `terminate:`, which never returns to `run`: it asks
//! `applicationShouldTerminate:`, which answers «later», and spins the run loop in the modal
//! panel mode, the main queue included, until
//! `replyToApplicationShouldTerminate:`; a yes there ends the process. So the hook and the
//! windows' close run from that queue, then the reply.
//!
//! All of it is in a thread local of the main thread. No borrow of it is held while app code
//! runs (a callback, an app's `with`), so that code may open, close and front windows.

use std::any::TypeId;
use std::cell::{Cell, RefCell};

use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
  NSApplication, NSApplicationDelegate, NSApplicationTerminateReply, NSBackingStoreType, NSEvent,
  NSEventModifierFlags, NSEventType, NSMenu, NSMenuItem, NSSystemColorsDidChangeNotification,
  NSWindow, NSWindowDelegate, NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{
  NSDictionary, NSNotification, NSNotificationCenter, NSNumber, NSObject, NSObjectProtocol,
  NSPoint, NSRect, NSSize, NSString, NSUserDefaults, ns_string,
};

pub use super::app::{alert, is_main, post, send, set_presence};
use super::view::View;
use super::{Pasteboard, activate, appearance, mtm, overlay};
use crate::Config;
use crate::app::{App, Clock, Event, Runtime};
use crate::shell::{Anchor, AppInfo, Main, Presence};

struct Win {
  id: u64,
  window: Retained<NSWindow>,
  view: Retained<View>,
  _delegate: Retained<WindowDelegate>,
  /// Shown and not hidden since: it counts for the Dock. Never an overlay.
  shown: bool,
  overlay: Option<Anchor>,
}

/// What a closed window leaves: let go after AppKit's call that closed it ([release]).
struct Gone {
  window: Retained<NSWindow>,
  _view: Retained<View>,
  _delegate: Retained<WindowDelegate>,
}

#[derive(Default)]
struct Shell {
  windows: Vec<Win>,
  /// Closed windows not yet let go.
  gone: Vec<Gone>,
  next: u64,
  /// [quit] ran: the loop ends, or does not start.
  quitting: bool,
  /// The system asked to quit (`applicationShouldTerminate:` answered «later»): the process ends
  /// once the app has finished.
  terminating: bool,
}

thread_local! {
  static SHELL: RefCell<Shell> = RefCell::default();
}

fn shell<R>(f: impl FnOnce(&mut Shell) -> R) -> R {
  SHELL.with_borrow_mut(f)
}

// MARK: The app

define_class!(
  // SAFETY: NSObject has no subclassing requirements; we implement only delegate methods.
  #[unsafe(super(NSObject))]
  #[thread_kind = MainThreadOnly]
  #[name = "NimboDelegate"]
  struct Delegate;

  unsafe impl NSObjectProtocol for Delegate {}

  unsafe impl NSApplicationDelegate for Delegate {
    /// `terminate:` (the Dock's Quit, logout, Activity Monitor, AppleScript): the app finishes
    /// from the main queue, which AppKit keeps serving while it waits for the answer, then
    /// answers yes and the process exits (see the module docs).
    #[unsafe(method(applicationShouldTerminate:))]
    fn should_terminate(&self, _app: &NSApplication) -> NSApplicationTerminateReply {
      if shell(|s| std::mem::replace(&mut s.terminating, true)) {
        return NSApplicationTerminateReply::TerminateLater;
      }

      defer(|| {
        if !shell(|s| s.terminating) {
          return;
        }

        crate::shell::finish(close);
        release();
        NSApplication::sharedApplication(mtm()).replyToApplicationShouldTerminate(true);
      });
      NSApplicationTerminateReply::TerminateLater
    }

    /// Windows close; the app quits only through [quit] (`nimbo::run` quits when its window
    /// closes).
    #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
    fn should_terminate_after_last(&self, _app: &NSApplication) -> bool {
      false
    }

    /// Opened again while it runs: LaunchServices starts no second process but tells.
    #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
    fn should_handle_reopen(&self, _app: &NSApplication, _visible: bool) -> bool {
      defer(crate::shell::reopened);
      false
    }
  }

  impl Delegate {
    /// `NSSystemColorsDidChangeNotification`: the accent colour (or another system colour)
    /// changed. Light and dark reach each view itself (`viewDidChangeEffectiveAppearance`).
    #[unsafe(method(systemColorsChanged:))]
    fn system_colors_changed(&self, _note: &NSNotification) {
      for view in views() {
        view.send(Event::Appearance(appearance()));
      }
    }

    /// The app menu's Quit: the same as [quit].
    #[unsafe(method(quit:))]
    fn quit_item(&self, _sender: Option<&AnyObject>) {
      quit();
    }
  }
);

impl Delegate {
  fn new(mtm: MainThreadMarker) -> Retained<Self> {
    let this = Self::alloc(mtm).set_ivars(());
    unsafe { msg_send![super(this), init] }
  }
}

fn main_menu(mtm: MainThreadMarker, name: &str, delegate: &Delegate) -> Retained<NSMenu> {
  let item = |title: &str, action, key: &str| unsafe {
    NSMenuItem::initWithTitle_action_keyEquivalent(
      NSMenuItem::alloc(mtm),
      &NSString::from_str(title),
      Some(action),
      &NSString::from_str(key),
    )
  };
  let bar = NSMenu::new(mtm);
  let app_item = NSMenuItem::new(mtm);
  let app_menu = NSMenu::new(mtm);
  // To the key window through the responder chain.
  app_menu.addItem(&item("Close Window", sel!(performClose:), "w"));
  let quit = item(&format!("Quit {name}"), sel!(quit:), "q");
  // SAFETY: the delegate has `quit:` and lives as long as the app.
  unsafe { quit.setTarget(Some(delegate)) };
  app_menu.addItem(&quit);
  app_item.setSubmenu(Some(&app_menu));
  bar.addItem(&app_item);
  bar
}

/// Sets the app up, runs `start`, then the AppKit loop until [quit].
pub fn run(info: &AppInfo, start: impl FnOnce(&mut Main)) {
  // AppKit's AutoFill heuristics off before any text input: a nimbo field is never a login,
  // address or one-time-code form, and with them on macOS starts an "AutoFill (<app>)" helper
  // process once a field takes focus. The keys are undocumented:
  // `NSAutoFillHeuristicControllerEnabled` (found in AppKit by Ghostty, ghostty-org/ghostty#8625,
  // 2025-09-14; Zed and Chromium set it too) and `NSAutoFillHeuristicsEnabled`, reportedly the
  // one macOS 27 reads (kstackhq/kstack#510). Registered, not written: a value the person set
  // wins.
  let no = NSNumber::new_bool(false);
  let keys =
    [ns_string!("NSAutoFillHeuristicControllerEnabled"), ns_string!("NSAutoFillHeuristicsEnabled")];
  let defaults =
    NSDictionary::<NSString, AnyObject>::from_slices(&keys, &[no.as_ref(), no.as_ref()]);
  // SAFETY: strings to numbers, property list values as the registration domain takes them.
  unsafe { NSUserDefaults::standardUserDefaults().registerDefaults(&defaults) };
  let mtm = mtm();
  let ns_app = NSApplication::sharedApplication(mtm);
  let delegate = Delegate::new(mtm);
  ns_app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
  ns_app.setMainMenu(Some(&main_menu(mtm, info.name, &delegate)));
  let center = NSNotificationCenter::defaultCenter();
  // SAFETY: the delegate lives until the app ends and has the selector, taking a notification.
  unsafe {
    center.addObserver_selector_name_object(
      &delegate,
      sel!(systemColorsChanged:),
      Some(NSSystemColorsDidChangeNotification),
      None,
    )
  };
  shell(|s| *s = Shell::default());
  super::app::set_presence(info.presence);
  if info.presence == Presence::Regular {
    activate(mtm);
  }

  start(&mut Main::here());
  if !shell(|s| s.quitting) {
    ns_app.run();
  }

  // The main queue isn't served once the loop ends: the closed windows go now.
  crate::shell::finish(close);
  release();
  // SAFETY: the observer is the delegate, added above.
  unsafe { center.removeObserver(&delegate) };
  ns_app.setDelegate(None);
  SHELL.take();
}

/// Ends the loop after the event in progress (`stop:` takes effect after the next event: one
/// that does nothing is posted).
pub fn overlay_takes_focus() -> bool {
  false
}

pub fn quit() {
  let mtm = mtm();
  shell(|s| s.quitting = true);
  let ns_app = NSApplication::sharedApplication(mtm);
  ns_app.stop(None);
  let event = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
    NSEventType::ApplicationDefined,
    NSPoint::ZERO,
    NSEventModifierFlags::empty(),
    0.0,
    0,
    None,
    0,
    0,
    0,
  );
  if let Some(event) = event {
    ns_app.postEvent_atStart(&event, false);
  }
}

pub fn terminating() -> bool {
  shell(|s| s.terminating)
}

/// A window is shown: the app is in the Dock ([super::app::sync_presence]).
pub(super) fn any_shown() -> bool {
  shell(|s| s.windows.iter().any(|w| w.shown))
}

/// Runs `f` on the main queue, after what AppKit is doing now.
fn defer(f: impl FnOnce() + 'static) {
  let f = MainThreadBound::new(f, mtm());
  DispatchQueue::main().exec_async(move || {
    // SAFETY: the main queue runs on the main thread.
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    f.into_inner(mtm)();
  });
}

// MARK: Windows

#[derive(Default)]
struct WindowIvars {
  id: Cell<u64>,
}

define_class!(
  // SAFETY: NSObject has no subclassing requirements; we implement only delegate methods.
  #[unsafe(super(NSObject))]
  #[thread_kind = MainThreadOnly]
  #[name = "NimboWindowDelegate"]
  #[ivars = WindowIvars]
  struct WindowDelegate;

  unsafe impl NSObjectProtocol for WindowDelegate {}

  unsafe impl NSWindowDelegate for WindowDelegate {
    #[unsafe(method(windowDidBecomeKey:))]
    fn did_become_key(&self, _note: &NSNotification) {
      if let Some(view) = view(self.ivars().id.get()) {
        view.focus(true);
      }
    }

    #[unsafe(method(windowDidResignKey:))]
    fn did_resign_key(&self, _note: &NSNotification) {
      if let Some(view) = view(self.ivars().id.get()) {
        view.focus(false);
      }
    }

    #[unsafe(method(windowDidChangeOcclusionState:))]
    fn did_change_occlusion(&self, _note: &NSNotification) {
      if let Some(view) = view(self.ivars().id.get()) {
        view.occlusion_changed();
      }
    }

    #[unsafe(method(windowDidChangeScreen:))]
    fn did_change_screen(&self, _note: &NSNotification) {
      if let Some(view) = view(self.ivars().id.get()) {
        view.screen_changed();
      }
    }

    #[unsafe(method(windowWillClose:))]
    fn will_close(&self, _note: &NSNotification) {
      closed(self.ivars().id.get());
    }
  }
);

impl WindowDelegate {
  fn new(mtm: MainThreadMarker, id: u64) -> Retained<Self> {
    let this = Self::alloc(mtm).set_ivars(WindowIvars { id: Cell::new(id) });
    unsafe { msg_send![super(this), init] }
  }
}

pub(super) fn view(id: u64) -> Option<Retained<View>> {
  shell(|s| s.windows.iter().find(|w| w.id == id).map(|w| w.view.clone()))
}

fn views() -> Vec<Retained<View>> {
  shell(|s| s.windows.iter().map(|w| w.view.clone()).collect())
}

/// Opens a window running `app` (an app of type `ty`), in front unless `cfg.hidden`.
pub fn open(cfg: &Config, app: Box<dyn App>, ty: TypeId) -> u64 {
  let mtm = mtm();
  let size = NSSize::new(cfg.size.w as f64, cfg.size.h as f64);
  let frame = NSRect::new(NSPoint::new(0.0, 0.0), size);
  let window = match cfg.overlay {
    Some(_) => overlay::panel(mtm, frame),
    None => {
      let mut style =
        NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable;
      style.set(NSWindowStyleMask::Resizable, cfg.resizable);
      style.set(NSWindowStyleMask::FullSizeContentView, cfg.unified);

      unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
          NSWindow::alloc(mtm),
          frame,
          style,
          NSBackingStoreType::Buffered,
          false,
        )
      }
    }
  };
  // SAFETY: the shell keeps the window until it closes.
  unsafe { window.setReleasedWhenClosed(false) };
  window.setTitle(&NSString::from_str(cfg.title));
  // A unified window draws its title bar: AppKit's turns clear, its title hidden.
  window.setTitlebarAppearsTransparent(cfg.unified);
  window.setTitleVisibility(NSWindowTitleVisibility(cfg.unified as _));

  let min = cfg.min();
  window.setContentMinSize(NSSize::new(min.w as f64, min.h as f64));

  let scale = window.backingScaleFactor() as f32;
  let mut rt = Runtime::new(app, cfg.size, scale, Clock::real(), Box::new(Pasteboard));
  rt.event(Event::Appearance(appearance()));
  rt.set_native_menus(true);
  // The title bar over the content: the frame's height over the layout rect's.
  let bar = (window.frame().size.height - window.contentLayoutRect().size.height) as f32;
  rt.set_titlebar(cfg.unified.then_some((bar, cfg.title)));
  let view = View::new(mtm, frame, rt, cfg.overlay);
  window.setContentView(Some(&view));
  let id = shell(|s| {
    s.next += 1;
    s.next
  });
  let delegate = WindowDelegate::new(mtm, id);
  window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
  view.prepare();
  window.center();
  window.makeFirstResponder(Some(&view));
  let win = Win { id, window, view, _delegate: delegate, shown: false, overlay: cfg.overlay };
  shell(|s| s.windows.push(win));
  crate::shell::opened(id, ty);
  if !cfg.hidden {
    front(id);
  }

  id
}

pub fn front(id: u64) {
  let mtm = mtm();
  let Some((window, overlay)) = shell(|s| {
    let w = s.windows.iter_mut().find(|w| w.id == id)?;
    w.shown = w.overlay.is_none();
    Some((w.window.clone(), w.overlay))
  }) else {
    return;
  };
  if let Some(anchor) = overlay {
    return overlay::show(&window, anchor);
  }

  super::app::sync_presence();
  if window.isMiniaturized() {
    window.deminiaturize(None);
  }

  window.makeKeyAndOrderFront(None);
  activate(mtm);
}

/// Orders the window out; the Dock follows.
pub fn hide(id: u64) {
  let Some(window) = shell(|s| {
    let w = s.windows.iter_mut().find(|w| w.id == id)?;
    w.shown = false;
    Some(w.window.clone())
  }) else {
    return;
  };
  window.orderOut(None);
  super::app::sync_presence();
}

pub fn close(id: u64) {
  let Some(window) = shell(|s| s.windows.iter().find(|w| w.id == id).map(|w| w.window.clone()))
  else {
    return;
  };
  // `windowWillClose:` only comes for a window on screen: the shell lets it go itself.
  closed(id);
  window.close();
}

/// Window `id` closes: its runtime stops and goes (after AppKit's call in progress: [release]),
/// the Dock follows, then its close callback runs.
fn closed(id: u64) {
  let Some(win) = shell(|s| {
    let i = s.windows.iter().position(|w| w.id == id)?;
    Some(s.windows.remove(i))
  }) else {
    return;
  };
  let on_close = crate::shell::closed(id);
  win.view.close();
  win.window.setDelegate(None);
  let Win { window, view, _delegate, .. } = win;
  shell(|s| s.gone.push(Gone { window, _view: view, _delegate }));
  defer(release);
  super::app::sync_presence();
  if let Some(f) = on_close {
    f(&mut Main::here());
  }
}

/// Runs `f` on window `id`'s app if it is not busy, then redraws the window.
pub fn with_app(id: u64, f: &mut dyn FnMut(&mut dyn App)) -> bool {
  let Some(view) = view(id) else { return false };
  view.try_runtime(|rt| f(&mut *rt.app)).is_some()
}

/// Lets go of the closed windows, their views and runtimes (the apps drop): from the main queue
/// after AppKit's call that closed them, or as the app ends, whichever comes first.
fn release() {
  for gone in shell(|s| std::mem::take(&mut s.gone)) {
    gone.window.setContentView(None);
    drop(gone);
  }
}
