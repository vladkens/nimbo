// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The seams between an app and the world: events in, a scene out, with the clock, clipboard
//! and waker injected, so one app runs the same in a window ([crate::platform]) and in the
//! headless harness ([crate::harness]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crate::keys::{self, Binding, Command, KeyEvent};
use crate::scene::{Color, Frame, Images, Point, Scene, Size};
use crate::text::TextSystem;
use crate::ui::Cursor;

/// An application: owns its state, reacts to events and draws the frame from it. A frame is
/// built only when something changed.
pub trait App {
  /// Handles one event. Keys come here after the app's bindings ([App::keymap]) found no
  /// command for them (see [crate::keys] for the order). Returns true when the event was used,
  /// which redraws the frame; a key with Cmd that returns false goes on to the menu (Cmd-Q), an
  /// unused key on to text input.
  fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
    let _ = (cx, ev);
    false
  }

  /// The app's key bindings: a match comes to [App::event] as [Event::Command].
  fn keymap(&self) -> &[Binding] {
    &[]
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

  fn keymap(&self) -> &[Binding] {
    (**self).keymap()
  }

  fn paint(&mut self, f: &mut Frame) {
    (**self).paint(f)
  }
}

// MARK: Events

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
  pub shift: bool,
  pub ctrl: bool,
  pub alt: bool,
  pub cmd: bool,
}

impl Mods {
  pub const NONE: Self = Self { shift: false, ctrl: false, alt: false, cmd: false };
  pub const CMD: Self = Self { cmd: true, ..Self::NONE };
  pub const SHIFT: Self = Self { shift: true, ..Self::NONE };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
  Left,
  Right,
  Middle,
  Other(u8),
}

/// A gesture phase, as AppKit reports it for trackpad scrolls and their momentum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
  /// A mouse wheel, or no gesture.
  #[default]
  None,
  MayBegin,
  Began,
  Changed,
  Ended,
  Cancelled,
}

/// Input in view coordinates (points, y down), independent of NSEvent.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
  /// Pointer moved, with or without a button held (drag).
  MouseMove {
    pos: Point,
    mods: Mods,
  },
  MouseDown {
    pos: Point,
    button: MouseButton,
    clicks: u32,
    mods: Mods,
  },
  MouseUp {
    pos: Point,
    button: MouseButton,
    mods: Mods,
  },
  /// The pointer left the view.
  MouseExit,
  /// `delta` in points, positive y when content should move down (toward the top of the
  /// document). Trackpads set `precise` and report `phase`, then `momentum` after lift-off.
  Scroll {
    pos: Point,
    delta: Point,
    precise: bool,
    phase: Phase,
    momentum: Phase,
    mods: Mods,
  },
  KeyDown(KeyEvent),
  KeyUp(KeyEvent),
  /// Text typed: a key no one used, as AppKit's text input turns it into text (`insertText:`,
  /// dead keys composed). Never empty, no control characters.
  Text(String),
  /// A key's command: from a binding, or a key no one used as AppKit's text input turns it into
  /// one (`doCommandBySelector:`: arrows, Backspace, Enter, Ctrl-A).
  Command(Command),
  /// Modifier keys changed.
  Mods(Mods),
  /// A modifier key went down or up, by its place (left or right, fn): what a shortcut recorder
  /// needs to take one alone (`right_option`). After the [Event::Mods] it changed; to the
  /// focused element first ([Ui::modifier_key]), then the app. Autorepeats send none.
  Modifier {
    key: crate::hotkey::ModKey,
    down: bool,
  },
  /// A trackpad pinch: `delta` is the change in magnification (0.1 is 10% larger).
  Magnify {
    pos: Point,
    delta: f32,
    phase: Phase,
    mods: Mods,
  },
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

/// Points per line for mouse wheels, which report lines rather than points.
pub const WHEEL_LINE: f32 = 18.0;

// MARK: Clock, clipboard, waker

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

pub trait Clipboard {
  fn read(&self) -> Option<String>;
  fn write(&mut self, s: &str);
}

/// An in-memory clipboard: the harness uses it so tests never touch the real pasteboard.
#[derive(Default)]
pub struct MemClipboard(pub Option<String>);

impl Clipboard for MemClipboard {
  fn read(&self) -> Option<String> {
    self.0.clone()
  }

  fn write(&mut self, s: &str) {
    self.0 = Some(s.to_owned());
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

/// What `App::event` gets besides the event: the window, the pointer, the clock, the clipboard
/// and the waker.
pub struct Cx<'a> {
  pub size: Size,
  pub scale: f32,
  pub mouse: Option<Point>,
  pub now: Duration,
  pub waker: &'a Waker,
  quit: &'a mut bool,
  redraw: bool,
  clipboard: &'a mut dyn Clipboard,
  cursor: &'a mut Cursor,
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

  pub fn clipboard(&mut self) -> &mut dyn Clipboard {
    &mut *self.clipboard
  }

  /// Sets the pointer's cursor, kept until set again.
  // Temporary: the element under the pointer sets it once the element tree comes.
  pub fn set_cursor(&mut self, c: Cursor) {
    *self.cursor = c;
  }
}

/// Drives an app: feeds it events, tracks the pointer and size, and builds a scene when
/// something changed. The window and the harness both own one.
pub struct Runtime<A> {
  pub app: A,
  pub size: Size,
  pub scale: f32,
  pub mouse: Option<Point>,
  pub scene: Scene,
  pub text: TextSystem,
  pub images: Images,
  clock: Clock,
  waker: Waker,
  dirty: bool,
  animating: bool,
  quit: bool,
  clipboard: Box<dyn Clipboard>,
  cursor: Cursor,
}

impl<A: App> Runtime<A> {
  pub fn new(app: A, size: Size, scale: f32, clock: Clock, clipboard: Box<dyn Clipboard>) -> Self {
    let waker = Waker::default();
    Self {
      app,
      size,
      scale,
      mouse: None,
      scene: Scene::default(),
      text: TextSystem::new(scale),
      images: Images::default(),
      clock,
      waker,
      dirty: true,
      animating: false,
      quit: false,
      clipboard,
      cursor: Cursor::Arrow,
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

  pub fn clipboard(&mut self) -> &mut dyn Clipboard {
    &mut *self.clipboard
  }

  /// The pointer's cursor, as the app set it.
  pub fn cursor(&self) -> Cursor {
    self.cursor
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
    self.text.set_scale(scale);
    self.dirty = true;
  }

  /// Feeds one event to the app; returns whether it was used or changed what the view shows
  /// (and so redraws). A key goes through the app's bindings first (see [crate::keys] for the
  /// order).
  pub fn event(&mut self, ev: Event) -> bool {
    if let Event::Appearance(a) = ev {
      return self.appearance(a);
    }

    let used = match ev {
      Event::KeyDown(k) => self.key_down(k),
      ev => self.pointer(ev),
    };

    self.dirty |= used;
    used
  }

  /// A new appearance: the app hears of it, and the frame redraws.
  fn appearance(&mut self, a: Appearance) -> bool {
    self.app_event(&Event::Appearance(a));
    self.redraw();
    true
  }

  fn pointer(&mut self, ev: Event) -> bool {
    match ev {
      Event::MouseMove { pos, .. }
      | Event::MouseDown { pos, .. }
      | Event::MouseUp { pos, .. }
      | Event::Scroll { pos, .. }
      | Event::Magnify { pos, .. } => self.mouse = Some(pos),
      Event::MouseExit => self.mouse = None,
      _ => {}
    }

    self.app_event(&ev)
  }

  /// The app's bindings, then the key itself to the app.
  fn key_down(&mut self, k: KeyEvent) -> bool {
    if let Some(cmd) = keys::lookup(self.app.keymap(), &k)
      && self.app_event(&Event::Command(cmd))
    {
      return true;
    }
    self.app_event(&Event::KeyDown(k))
  }

  fn app_event(&mut self, ev: &Event) -> bool {
    let mut cx = Cx {
      size: self.size,
      scale: self.scale,
      mouse: self.mouse,
      now: self.clock.now(),
      waker: &self.waker,
      quit: &mut self.quit,
      redraw: false,
      clipboard: &mut *self.clipboard,
      cursor: &mut self.cursor,
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

  /// Builds the scene from the app's paint; the caller syncs `text` glyphs and `images` to the
  /// renderer and draws `scene`. When glyphs were dropped (a full atlas), the next refresh draws
  /// once more.
  pub fn frame(&mut self) {
    self.scene.clear();
    let (size, scale, now) = (self.size, self.scale, self.clock.now());
    let mut f = Frame {
      size,
      scale,
      mouse: self.mouse,
      now,
      scene: &mut self.scene,
      text: &mut self.text,
      images: &mut self.images,
      animate: false,
    };
    self.app.paint(&mut f);
    self.animating = f.animate;
    self.dirty = self.text.end_frame();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::keys::Key;
  use crate::scene::Rect;

  #[derive(Default)]
  struct Probe {
    events: usize,
    paints: usize,
    animate: bool,
  }

  impl App for Probe {
    fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
      self.events += 1;
      if let Event::KeyDown(k) = ev
        && k.is("c")
      {
        cx.clipboard().write("copied");
      }
      matches!(ev, Event::KeyDown { .. })
    }

    fn paint(&mut self, f: &mut Frame) {
      self.paints += 1;
      f.rect(Rect::new(0.0, 0.0, 1.0, 1.0), [1.0; 4], 0.0);
      if self.animate {
        f.animate();
      }
    }
  }

  fn runtime() -> Runtime<Probe> {
    let clock = Clock::Virtual(Duration::ZERO);
    let clipboard = Box::new(MemClipboard::default());
    Runtime::new(Probe::default(), Size::new(100.0, 100.0), 1.0, clock, clipboard)
  }

  #[test]
  fn redraws_only_when_needed() {
    let mut rt = runtime();
    assert!(rt.needs_frame());
    rt.frame();
    assert!(!rt.needs_frame());

    assert!(!rt.event(Event::MouseMove { pos: Point::new(5.0, 5.0), mods: Mods::NONE }));
    assert_eq!(rt.mouse, Some(Point::new(5.0, 5.0)));
    assert!(!rt.needs_frame());

    let key =
      KeyEvent { key: Key::Char('c'), text: None, code: 8, mods: Mods::NONE, repeat: false };
    assert!(rt.event(Event::KeyDown(key)));
    assert!(rt.needs_frame());
    rt.frame();
    assert_eq!(rt.clipboard().read().as_deref(), Some("copied"));
    assert_eq!(rt.scene.counts().quads, 1);

    rt.event(Event::MouseExit);
    assert_eq!(rt.mouse, None);
  }

  #[test]
  fn waker_and_animation() {
    let mut rt = runtime();
    rt.frame();
    let waker = rt.waker().clone();
    std::thread::spawn(move || waker.wake()).join().unwrap();
    assert!(rt.needs_frame());
    rt.frame();
    assert!(!rt.needs_frame());

    rt.app.animate = true;
    rt.redraw();
    rt.frame();
    assert!(rt.needs_frame());
    assert_eq!(rt.app.paints, 3);
  }

  #[test]
  fn virtual_clock() {
    let mut rt = runtime();
    rt.advance(Duration::from_millis(16));
    assert_eq!(rt.now(), Duration::from_millis(16));
  }
}
