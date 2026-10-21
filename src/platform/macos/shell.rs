// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app on AppKit: NSApplication with a minimal app menu and its delegate, and the windows by
//! id (an NSWindow each, its content our [View] with its own runtime, its own delegate).
//!
//! The app ends one of two ways. [quit] stops the loop and `run` returns. A quit asked by the
//! system (the Dock's Quit, logout, Activity Monitor, AppleScript `quit`) comes as `terminate:`,
//! which never returns to `run`: it asks `applicationShouldTerminate:`, which answers «later»,
//! and spins the run loop in the modal panel mode, the main queue included, until
//! `replyToApplicationShouldTerminate:`; a yes there ends the process. So the windows' close runs
//! from that queue, then the reply.
//!
//! All of it is in a thread local of the main thread. No borrow of it is held while app code
//! runs (a callback), so that code may open, close and front windows.

use std::cell::{Cell, RefCell};

use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
  NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSApplicationTerminateReply,
  NSBackingStoreType, NSEvent, NSEventModifierFlags, NSEventType, NSMenu, NSMenuItem,
  NSSystemColorsDidChangeNotification, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
  NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
  NSString,
};

use super::appearance;
use super::view::View;
use crate::Config;
use crate::app::{App, Clock, Event, Runtime};

type OnClose = Box<dyn FnOnce()>;

struct Win {
  id: u64,
  window: Retained<NSWindow>,
  view: Retained<View>,
  _delegate: Retained<WindowDelegate>,
  on_close: Option<OnClose>,
}

#[derive(Default)]
struct Shell {
  windows: Vec<Win>,
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

fn mtm() -> MainThreadMarker {
  MainThreadMarker::new().expect("nimbo: the app and its windows are main thread only")
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

        finish();
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
pub fn run(name: &str, start: impl FnOnce()) {
  let mtm = mtm();
  let ns_app = NSApplication::sharedApplication(mtm);
  let delegate = Delegate::new(mtm);
  ns_app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
  ns_app.setMainMenu(Some(&main_menu(mtm, name, &delegate)));
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
  ns_app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
  ns_app.activate();

  start();
  if !shell(|s| s.quitting) {
    ns_app.run();
  }

  finish();
  // SAFETY: the observer is the delegate, added above.
  unsafe { center.removeObserver(&delegate) };
  ns_app.setDelegate(None);
  SHELL.take();
}

/// The app ends: the windows close.
fn finish() {
  for id in shell(|s| s.windows.iter().map(|w| w.id).collect::<Vec<_>>()) {
    close(id);
  }
}

/// Ends the loop after the event in progress (`stop:` takes effect after the next event: one
/// that does nothing is posted).
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
        view.send(Event::Focus(true));
      }
    }

    #[unsafe(method(windowDidResignKey:))]
    fn did_resign_key(&self, _note: &NSNotification) {
      if let Some(view) = view(self.ivars().id.get()) {
        view.send(Event::Focus(false));
      }
    }

    #[unsafe(method(windowDidChangeOcclusionState:))]
    fn did_change_occlusion(&self, _note: &NSNotification) {
      if let Some(view) = view(self.ivars().id.get()) {
        view.occlusion_changed();
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

fn view(id: u64) -> Option<Retained<View>> {
  shell(|s| s.windows.iter().find(|w| w.id == id).map(|w| w.view.clone()))
}

fn views() -> Vec<Retained<View>> {
  shell(|s| s.windows.iter().map(|w| w.view.clone()).collect())
}

/// Opens a window running `app`, in front.
pub fn open(cfg: &Config, app: Box<dyn App>) -> u64 {
  let mtm = mtm();
  let size = NSSize::new(cfg.size.w as f64, cfg.size.h as f64);
  let frame = NSRect::new(NSPoint::new(0.0, 0.0), size);
  let mut style =
    NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable;
  if cfg.resizable {
    style |= NSWindowStyleMask::Resizable;
  }

  let window = unsafe {
    NSWindow::initWithContentRect_styleMask_backing_defer(
      NSWindow::alloc(mtm),
      frame,
      style,
      NSBackingStoreType::Buffered,
      false,
    )
  };
  // SAFETY: the shell keeps the window until it closes.
  unsafe { window.setReleasedWhenClosed(false) };
  window.setTitle(&NSString::from_str(cfg.title));
  let min = cfg.min();
  window.setContentMinSize(NSSize::new(min.w as f64, min.h as f64));

  let scale = window.backingScaleFactor() as f32;
  let mut rt = Runtime::new(app, cfg.size, scale, Clock::real());
  rt.event(Event::Appearance(appearance()));
  let view = View::new(mtm, frame, rt);
  window.setContentView(Some(&view));
  let id = shell(|s| {
    s.next += 1;
    s.next
  });
  let delegate = WindowDelegate::new(mtm, id);
  window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
  window.center();
  let win = Win { id, window, view, _delegate: delegate, on_close: None };
  shell(|s| s.windows.push(win));
  front(id);
  id
}

pub fn front(id: u64) {
  let mtm = mtm();
  let Some(window) = shell(|s| s.windows.iter().find(|w| w.id == id).map(|w| w.window.clone()))
  else {
    return;
  };
  if window.isMiniaturized() {
    window.deminiaturize(None);
  }

  window.makeKeyAndOrderFront(None);
  NSApplication::sharedApplication(mtm).activate();
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

pub fn on_close(id: u64, f: OnClose) {
  shell(|s| {
    if let Some(w) = s.windows.iter_mut().find(|w| w.id == id) {
      w.on_close = Some(f);
    }
  });
}

/// Window `id` closes: its runtime stops and goes (after AppKit's call in progress), then its
/// close callback runs.
fn closed(id: u64) {
  let Some(win) = shell(|s| {
    let i = s.windows.iter().position(|w| w.id == id)?;
    Some(s.windows.remove(i))
  }) else {
    return;
  };
  win.view.stop();
  win.window.setDelegate(None);
  let Win { window, view, _delegate, on_close, .. } = win;
  defer(move || {
    window.setContentView(None);
    drop((window, view, _delegate));
  });
  if let Some(f) = on_close {
    f();
  }
}
