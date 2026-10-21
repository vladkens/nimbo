// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The NSView subclass: a layer we draw with the CPU renderer (IOSurfaces as its contents), and
//! a display link that runs only while the runtime needs frames (an event, animations). While the
//! window is hidden (minimized, covered, on another space) frames wait until it shows again.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::rc::{Retained, Weak};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSResponder, NSView, NSWindow, NSWindowOcclusionState};
use objc2_foundation::{NSObject, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange, CALayer, kCAGravityTopLeft};

use super::surface::Presenter;
use crate::app::{App, Event, Runtime};
use crate::scene::Size;

type WeakView = Arc<MainThreadBound<Weak<View>>>;

pub struct State {
  rt: Runtime<Box<dyn App>>,
  /// The CPU renderer's frames, presented as the layer's contents.
  presenter: Presenter,
  link: Retained<CADisplayLink>,
}

#[derive(Default)]
pub struct Ivars {
  state: RefCell<Option<State>>,
  /// The display link was invalidated (the view left its window): a new one starts when it
  /// moves to a window again.
  stopped: Cell<bool>,
}

define_class!(
  // SAFETY: NSView has no subclassing requirements; we override only the methods below.
  #[unsafe(super(NSView, NSResponder, NSObject))]
  #[thread_kind = MainThreadOnly]
  #[name = "NimboView"]
  #[ivars = Ivars]
  pub struct View;

  impl View {
    #[unsafe(method(isFlipped))]
    fn is_flipped(&self) -> bool {
      true
    }

    #[unsafe(method(setFrameSize:))]
    fn set_frame_size(&self, size: NSSize) {
      let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
      // Draw right away, inside the resize transaction, so the content never stretches.
      self.render();
    }

    /// The display link retains the view: leaving the window breaks the cycle.
    #[unsafe(method(viewWillMoveToWindow:))]
    fn will_move_to_window(&self, window: Option<&NSWindow>) {
      let _: () = unsafe { msg_send![super(self), viewWillMoveToWindow: window] };
      if window.is_none() {
        self.stop();
      }
    }

    #[unsafe(method(viewDidMoveToWindow))]
    fn did_move_to_window(&self) {
      let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
      if self.window().is_some() && self.ivars().stopped.replace(false) {
        let link = self.display_link();
        if let Some(st) = self.ivars().state.borrow_mut().as_mut() {
          st.link = link;
        }
        self.wake();
      }
    }

    #[unsafe(method(viewDidChangeEffectiveAppearance))]
    fn appearance_changed(&self) {
      self.send(Event::Appearance(super::appearance()));
    }

    #[unsafe(method(viewDidChangeBackingProperties))]
    fn backing_changed(&self) {
      // `render` picks up the new scale.
      self.wake();
    }

    #[unsafe(method(tick:))]
    fn tick(&self, _link: &CADisplayLink) {
      let pause = {
        let mut state = self.ivars().state.borrow_mut();
        let Some(st) = state.as_mut() else { return };
        let idle = !st.rt.needs_frame() || !self.visible();
        if idle {
          // Nothing changed since the last frame: stop until the next event or wake. Hidden:
          // until the window shows again.
          st.link.setPaused(true);
        }
        idle
      };
      if !pause {
        self.render();
        self.after_frame();
      }
    }
  }
);

impl View {
  pub fn new(mtm: MainThreadMarker, frame: NSRect, rt: Runtime<Box<dyn App>>) -> Retained<Self> {
    let this = Self::alloc(mtm).set_ivars(Ivars::default());
    let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
    let layer = CALayer::new();
    layer.setContentsGravity(unsafe { kCAGravityTopLeft });
    view.setLayer(Some(&layer));
    view.setWantsLayer(true);
    let mut presenter = Presenter::new(layer);
    presenter.trace = std::env::var_os("NIMBO_TRACE").is_some();

    let link = view.display_link();

    // A wake from any thread hops to the main queue and restarts the display link.
    let weak = view.weak();
    rt.waker().set_notify(Box::new(move || {
      let weak = weak.clone();
      DispatchQueue::main().exec_async(move || {
        let mtm = MainThreadMarker::new().unwrap();
        if let Some(view) = weak.get(mtm).load() {
          view.wake();
        }
      });
    }));
    *view.ivars().state.borrow_mut() = Some(State { rt, presenter, link });
    view
  }

  /// A display link calling `tick:`, at up to 120 Hz, on the main run loop in every mode.
  fn display_link(&self) -> Retained<CADisplayLink> {
    let link = unsafe { self.displayLinkWithTarget_selector(self, sel!(tick:)) };
    link.setPreferredFrameRateRange(CAFrameRateRange::new(60.0, 120.0, 120.0));
    unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
    link
  }

  /// Invalidates the display link, which releases the view (its window closed, or it left it).
  pub fn stop(&self) {
    if let Ok(mut state) = self.ivars().state.try_borrow_mut()
      && let Some(st) = state.as_mut()
    {
      st.link.invalidate();
      self.ivars().stopped.set(true);
    }
  }

  /// Whether any of the window is on screen (not minimized, covered or on another space); a
  /// view with no window counts as shown.
  fn visible(&self) -> bool {
    let shown =
      |w: Retained<NSWindow>| w.occlusionState().contains(NSWindowOcclusionState::Visible);
    self.window().is_none_or(shown)
  }

  /// The window was shown or hidden: a frame that waited for it draws now.
  pub fn occlusion_changed(&self) {
    if self.visible() && !self.ivars().stopped.get() {
      self.wake();
    }
  }

  /// Feeds an event to the runtime and restarts the display link if it needs a frame.
  pub fn send(&self, ev: Event) -> bool {
    let (used, quit) = {
      let mut state = self.ivars().state.borrow_mut();
      let Some(st) = state.as_mut() else { return false };
      let used = st.rt.event(ev);
      if st.rt.needs_frame() {
        st.link.setPaused(false);
      }
      (used, st.rt.quit_requested())
    };
    if quit {
      super::shell::quit();
    }

    used
  }

  fn wake(&self) {
    if let Some(st) = self.ivars().state.borrow_mut().as_mut() {
      st.rt.redraw();
      st.link.setPaused(false);
    }
  }

  fn weak(&self) -> WeakView {
    Arc::new(MainThreadBound::new(Weak::new(self), self.mtm()))
  }

  /// After a frame: the display link runs on while the runtime needs frames (animations), else
  /// it stops at once, not a refresh later.
  fn after_frame(&self) {
    let mut state = self.ivars().state.borrow_mut();
    let Some(st) = state.as_mut() else { return };
    st.link.setPaused(!st.rt.needs_frame());
  }

  /// Builds and presents a frame at the view's current size and backing scale.
  fn render(&self) {
    let scale = self.window().map_or(1.0, |w| w.backingScaleFactor() as f32);
    let bounds = self.bounds().size;
    let size = Size::new(bounds.width as f32, bounds.height as f32);
    let mut state = self.ivars().state.borrow_mut();
    let Some(st) = state.as_mut() else { return };
    st.rt.resize(size, scale);
    st.rt.frame();
    st.presenter.renderer.sync(&mut st.rt.images);
    st.presenter.present(&st.rt.scene, size, scale);
  }
}
