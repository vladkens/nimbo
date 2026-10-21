// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The seams between an app and the world: events in, a scene out, with the clock and waker
//! injected, so one app runs the same in a window ([crate::platform]) and in the headless
//! harness ([crate::harness]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crate::scene::{Color, Frame, Images, Scene, Size};

/// An application: owns its state, reacts to events and draws the frame from it. A frame is
/// built only when something changed.
pub trait App {
  /// Handles one event. Returns true when the event was used, which redraws the frame.
  fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
    let _ = (cx, ev);
    false
  }

  /// Paints the frame.
  fn paint(&mut self, f: &mut Frame) {
    let _ = f;
  }
}

impl<A: App + ?Sized> App for Box<A> {
  fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
    (**self).event(cx, ev)
  }

  fn paint(&mut self, f: &mut Frame) {
    (**self).paint(f)
  }
}

// MARK: Events

/// Input in view coordinates (points, y down), independent of NSEvent.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
  /// The window became or stopped being the key window.
  Focus(bool),
  /// The OS's light or dark setting or accent colour changed, or is first known (before the
  /// first frame).
  Appearance(Appearance),
}

/// The OS's look: light or dark, and the accent colour the user picked, if the OS has one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Appearance {
  pub dark: bool,
  /// sRGB; `None` where the OS has none or the user left it to apps (macOS Multicolor).
  pub accent: Option<Color>,
}

impl Appearance {
  pub const LIGHT: Self = Self { dark: false, accent: None };
  pub const DARK: Self = Self { dark: true, accent: None };
}

// MARK: Clock, waker

/// Real time in a window, virtual time in the harness (moved only by `Runtime::advance`).
#[derive(Clone, Copy, Debug)]
pub enum Clock {
  Real(Instant),
  Virtual(Duration),
}

impl Clock {
  pub fn real() -> Self {
    Self::Real(Instant::now())
  }

  pub fn now(&self) -> Duration {
    match self {
      Self::Real(start) => start.elapsed(),
      Self::Virtual(t) => *t,
    }
  }
}

type Notify = Box<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct WakerInner {
  woken: AtomicBool,
  notify: OnceLock<Notify>,
}

/// Requests a redraw from any thread (background loading, timers of other threads). Cheap to
/// clone; repeated wakes before the next frame coalesce into one.
#[derive(Clone, Default)]
pub struct Waker(Arc<WakerInner>);

impl Waker {
  pub fn wake(&self) {
    if !self.0.woken.swap(true, Ordering::AcqRel)
      && let Some(notify) = self.0.notify.get()
    {
      notify();
    }
  }

  /// Sets how a wake reaches the event loop (the window hops to the main thread).
  // Unused by the stub platforms until they are ported.
  #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
  pub(crate) fn set_notify(&self, f: Notify) {
    let _ = self.0.notify.set(f);
  }

  fn take(&self) -> bool {
    self.0.woken.swap(false, Ordering::AcqRel)
  }
}

// MARK: Runtime

/// What `App::event` gets besides the event: the window, the clock and the waker.
pub struct Cx<'a> {
  pub size: Size,
  pub scale: f32,
  pub now: Duration,
  pub waker: &'a Waker,
  quit: &'a mut bool,
  redraw: bool,
}

impl Cx<'_> {
  /// Ends the app (the loop stops, the window closes); the harness records it.
  pub fn quit(&mut self) {
    *self.quit = true;
  }

  /// Redraws the frame though the event goes on as unused (a key shown, then typed).
  pub fn redraw(&mut self) {
    self.redraw = true;
  }
}

/// Drives an app: feeds it events, tracks the size, and builds a scene when something
/// changed. The window and the harness both own one.
pub struct Runtime<A> {
  pub app: A,
  pub size: Size,
  pub scale: f32,
  pub scene: Scene,
  pub images: Images,
  clock: Clock,
  waker: Waker,
  dirty: bool,
  animating: bool,
  quit: bool,
}

impl<A: App> Runtime<A> {
  pub fn new(app: A, size: Size, scale: f32, clock: Clock) -> Self {
    let waker = Waker::default();
    Self {
      app,
      size,
      scale,
      scene: Scene::default(),
      images: Images::default(),
      clock,
      waker,
      dirty: true,
      animating: false,
      quit: false,
    }
  }

  pub fn now(&self) -> Duration {
    self.clock.now()
  }

  /// Moves virtual time forward. Panics on a real clock.
  pub fn advance(&mut self, dt: Duration) {
    let Clock::Virtual(t) = &mut self.clock else { panic!("advance needs a virtual clock") };
    *t += dt;
  }

  pub fn waker(&self) -> &Waker {
    &self.waker
  }

  pub fn quit_requested(&self) -> bool {
    self.quit
  }

  pub fn redraw(&mut self) {
    self.dirty = true;
  }

  pub fn resize(&mut self, size: Size, scale: f32) {
    if size == self.size && scale == self.scale {
      return;
    }

    (self.size, self.scale) = (size, scale);
    self.dirty = true;
  }

  /// Feeds one event to the app; returns whether it was used or changed what the view shows
  /// (and so redraws).
  pub fn event(&mut self, ev: Event) -> bool {
    if let Event::Appearance(a) = ev {
      return self.appearance(a);
    }

    let used = self.app_event(&ev);
    self.dirty |= used;
    used
  }

  /// A new appearance: the app hears of it, and the frame redraws.
  fn appearance(&mut self, a: Appearance) -> bool {
    self.app_event(&Event::Appearance(a));
    self.redraw();
    true
  }

  fn app_event(&mut self, ev: &Event) -> bool {
    let mut cx = Cx {
      size: self.size,
      scale: self.scale,
      now: self.clock.now(),
      waker: &self.waker,
      quit: &mut self.quit,
      redraw: false,
    };
    let used = self.app.event(&mut cx, ev);
    self.dirty |= cx.redraw;
    used
  }

  /// Whether the next display refresh should draw: an event changed something, an animation
  /// runs or a waker fired.
  pub fn needs_frame(&mut self) -> bool {
    self.dirty |= self.waker.take();
    self.dirty || self.animating
  }

  /// Builds the scene from the app's paint; the caller syncs `images` to the renderer and draws
  /// `scene`.
  pub fn frame(&mut self) {
    self.scene.clear();
    let (size, scale, now) = (self.size, self.scale, self.clock.now());
    let mut f =
      Frame { size, scale, now, scene: &mut self.scene, images: &mut self.images, animate: false };
    self.app.paint(&mut f);
    self.animating = f.animate;
    self.dirty = false;
  }
}
