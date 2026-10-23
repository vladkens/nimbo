// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The seams between an app and the world: events in, a scene out, with the clock, clipboard
//! and waker injected, so one app runs the same in a window ([crate::platform]) and in the
//! headless harness ([crate::harness]).

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use keyboard_types::{Code, Modifiers};

use crate::keys::{self, Binding, Command, KeyEvent};
use crate::scene::{Color, Frame, Images, Point, Rect, Scene, Size};
use crate::text::TextSystem;
use crate::ui::{Click, Cursor, FrameStats, Id, Sys, TextInput, Ui, UiState};

/// An application: owns its state, reacts to events and draws the frame from it, as an element
/// tree (`view`), by hand (`paint`), or both. A frame is built only when something changed.
pub trait App {
  /// Handles one input event, after the runtime routed it through the last frame's tree:
  /// `cx` tells what is under the pointer and what was clicked ([Cx::clicked]). Keys, text and
  /// commands come here when the focused element did not use them (see [crate::keys] for the
  /// order). Returns true when the event was used, which redraws the frame; a key with Cmd that
  /// returns false goes on to the menu (Cmd-Q), an unused key on to text input.
  fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
    let _ = (cx, ev);
    false
  }

  /// The app's key bindings, tried after the focused element's: a match comes to
  /// [App::event] as [Event::Command].
  fn keymap(&self) -> &[Binding] {
    &[]
  }

  /// Builds the frame's element tree, which is then laid out and painted. Write it as
  /// `fn view(&mut self, ui: &mut Ui)`, or with `<'a>` on `self` and `Ui` when canvas callbacks
  /// borrow the app's data.
  fn view<'a>(&'a mut self, ui: &mut Ui<'a>) {
    let _ = ui;
  }

  /// Paints over the view, or the whole frame for an app without one.
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

  fn view<'a>(&'a mut self, ui: &mut Ui<'a>) {
    (**self).view(ui)
  }

  fn paint(&mut self, f: &mut Frame) {
    (**self).paint(f)
  }
}

// MARK: Events

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
    mods: Modifiers,
  },
  MouseDown {
    pos: Point,
    button: MouseButton,
    clicks: u32,
    mods: Modifiers,
  },
  MouseUp {
    pos: Point,
    button: MouseButton,
    mods: Modifiers,
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
    mods: Modifiers,
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
  Mods(Modifiers),
  /// A modifier key went down or up, by its place ([Code::AltRight], [Code::Fn]): what a
  /// shortcut recorder needs to take one alone (`right_option`). After the [Event::Mods] it
  /// changed; to the focused element first ([Ui::modifier_key]), then the app. Autorepeats send
  /// none.
  Modifier {
    key: Code,
    down: bool,
  },
  /// A trackpad pinch: `delta` is the change in magnification (0.1 is 10% larger).
  Magnify {
    pos: Point,
    delta: f32,
    phase: Phase,
    mods: Modifiers,
  },
  /// The window became or stopped being the key window.
  Focus(bool),
  /// An input method composing or committing text, for the focused text field.
  Ime(Ime),
  /// The OS's light or dark setting or accent colour changed, or is first known (before the
  /// first frame): the theme follows it unless the app set one ([Ui::set_theme]).
  Appearance(Appearance),
}

/// The OS's look: light or dark, and the accent colour the user picked, if the OS has one
/// ([crate::platform::appearance]).
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

/// What an input method does to the focused text field (macOS `NSTextInputClient`, Windows
/// IMM32, GTK's input method context). Ranges are UTF-16 code units of the field's text, as
/// AppKit and IMM32 count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ime {
  /// Shows `text` as marked text (the composition: underlined at the caret, not yet part of the
  /// value) in place of the marked text, or `replace` (a range of the field's text), or the
  /// selection; `sel` is the selection within `text` (the input method's caret or converted
  /// clause). Empty, it ends composing with nothing typed.
  Mark { text: String, sel: Range<usize>, replace: Option<Range<usize>> },
  /// Commits `text` in place of the marked text, or `replace`, or the selection.
  Commit { text: String, replace: Option<Range<usize>> },
  /// Keeps the marked text as typed (a click elsewhere, the input method switched off).
  Unmark,
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

/// The system clipboard, plain text, as each window's runtime has it: for code outside a window
/// (a tray menu's Copy, a paste of the app's own). On the main thread, while the app runs
/// ([crate::app]): Windows writes through the app's hidden window, Linux through GTK, which also
/// serves the text to other apps; with no app running it reads nothing and writes nothing there.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClipboard;

impl Clipboard for SystemClipboard {
  fn read(&self) -> Option<String> {
    crate::platform::clipboard()?.read()
  }

  fn write(&mut self, s: &str) {
    if let Some(mut c) = crate::platform::clipboard() {
      c.write(s);
    }
  }
}

/// An in-memory one, until the runtime puts its own in.
impl Default for Box<dyn Clipboard> {
  fn default() -> Self {
    Box::new(MemClipboard::default())
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
  pub(crate) fn set_notify(&self, f: Notify) {
    let _ = self.0.notify.set(f);
  }

  fn take(&self) -> bool {
    self.0.woken.swap(false, Ordering::AcqRel)
  }
}

// MARK: Runtime

/// What `App::event` gets besides the event: the window, the clock, the clipboard and the
/// pointer routed through the last frame's tree.
pub struct Cx<'a> {
  pub size: Size,
  pub scale: f32,
  pub mouse: Option<Point>,
  pub now: Duration,
  pub waker: &'a Waker,
  quit: &'a mut bool,
  redraw: bool,
  ui: &'a mut UiState,
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
    &mut *self.ui.clipboard
  }

  /// The focused element.
  pub fn focused(&self) -> Option<Id> {
    self.ui.focus()
  }

  /// Moves focus to `id`, or clears it.
  pub fn focus(&mut self, id: Option<Id>) {
    self.ui.set_focus(id);
  }

  /// Pointer state and the last frame's tree (`cx.ui().last`).
  pub fn ui(&self) -> &UiState {
    self.ui
  }

  /// An element's rect in the last frame.
  pub fn rect(&self, id: Id) -> Option<Rect> {
    self.ui.last.rect(id)
  }

  /// The innermost interactive element under the pointer.
  pub fn target(&self) -> Option<Id> {
    self.ui.hover().first().copied()
  }

  pub fn hovered(&self, id: Id) -> bool {
    self.ui.hovered(id)
  }

  /// The click this event completed, if any.
  pub fn click(&self) -> Option<Click> {
    self.ui.input.click
  }

  /// Whether this event completed a left click on `id`.
  pub fn clicked(&self, id: Id) -> bool {
    self.ui.clicked(id)
  }

  /// Shows `id` pressed for a moment: the button a key stands for ([UiState::flash]).
  pub fn flash(&mut self, id: Id) {
    self.ui.flash(id, self.now);
  }

  /// Asks the window for a content size of `w` × `h` points ([Ui::set_window_size]).
  pub fn set_window_size(&mut self, w: f32, h: f32) {
    self.ui.ask_size(Size::new(w, h), self.size);
  }

  /// Asks for a frame `dt` from now: a timer, which the app's view checks the time on.
  pub fn after(&mut self, dt: Duration) {
    let at = self.now + dt;
    self.ui.after = Some(self.ui.after.map_or(at, |d| d.min(at)));
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
  /// The element trees: the one being built and the last frame's, laid out.
  pub ui: UiState,
  /// Time the last frame spent in `view`, layout and painting the tree.
  pub ui_times: [Duration; 3],
  /// Times the last frame ran the view: 2 when the hover moved under the new layout.
  pub ui_passes: u32,
  clock: Clock,
  waker: Waker,
  dirty: bool,
  /// An event changed something since the last layout: key routing and input method queries
  /// lay the view out again first, so they see the text as it is.
  stale: bool,
  animating: bool,
  /// The last frame is drawn again because the hover moved under its layout: if it moves once
  /// more (a hover that moves what it hovers), it stays as it is until the pointer moves.
  rehovered: bool,
  /// The last frame is drawn again because its layout shifted from the one before
  /// ([Ui::shifted]); a shift again right after waits for an event.
  settled: bool,
  /// Something outside the frame asked for this one (an event, a redraw, a wake, a resize): a
  /// shift in it settles again, though the frame before was a settling one.
  asked: bool,
  /// The app heard an appearance: the first one reaches it even when it equals the default.
  told: bool,
  quit: bool,
}

impl<A: App> Runtime<A> {
  pub fn new(app: A, size: Size, scale: f32, clock: Clock, clipboard: Box<dyn Clipboard>) -> Self {
    let waker = Waker::default();
    let mut ui = UiState::default();
    ui.waker = waker.clone();
    ui.clipboard = clipboard;
    Self {
      app,
      size,
      scale,
      mouse: None,
      scene: Scene::default(),
      text: TextSystem::new(scale),
      images: Images::default(),
      ui,
      ui_times: [Duration::ZERO; 3],
      ui_passes: 0,
      clock,
      waker,
      dirty: true,
      stale: true,
      animating: false,
      rehovered: false,
      settled: false,
      asked: false,
      told: false,
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

  pub fn clipboard(&mut self) -> &mut dyn Clipboard {
    &mut *self.ui.clipboard
  }

  /// The pointer's cursor: the pressed or hovered element's.
  pub fn cursor(&self) -> Cursor {
    self.ui.cursor()
  }

  pub fn quit_requested(&self) -> bool {
    self.quit
  }

  pub fn redraw(&mut self) {
    self.ask(true);
    self.stale = true;
  }

  /// The OS took the pointer while a button was held (another window captured it): the press
  /// ends without a click. The platform then sends the button's [Event::MouseUp] as usual, which
  /// ends the app's drags.
  pub fn cancel_press(&mut self) {
    let cancelled = self.ui.cancel_press();
    self.ask(cancelled);
  }

  /// A frame asked for from outside the last one, when `yes`.
  fn ask(&mut self, yes: bool) {
    self.dirty |= yes;
    self.asked |= yes;
  }

  /// The content size the app asked the window for ([Ui::set_window_size]), once: the platform
  /// resizes the window to it after an event or a frame, then reports the new size by
  /// [Runtime::resize] as it does for any.
  pub fn take_resize(&mut self) -> Option<Size> {
    self.ui.resize.take()
  }

  pub fn resize(&mut self, size: Size, scale: f32) {
    if size == self.size && scale == self.scale {
      return;
    }

    (self.size, self.scale) = (size, scale);
    self.text.set_scale(scale);
    // The tree is laid out again before the next key: its bindings may follow the size.
    self.redraw();
  }

  /// Feeds one event to the app, routed through the last frame's tree first; returns whether
  /// it was used or, for the pointer, changed what the view shows. Either redraws, as does a
  /// focus moved by it. A key that only moved the focus is unused: the platform's text input
  /// still gets it (an app focusing a search field on the first letter typed). A click runs
  /// the view once at once, without drawing, so the app sees it before the next frame is
  /// built; so does a key, text or command for the focused element (see [crate::keys] for
  /// their order).
  pub fn event(&mut self, ev: Event) -> bool {
    if let Event::Appearance(a) = ev {
      return self.appearance(a);
    }

    let keys = matches!(ev, Event::KeyDown(_) | Event::Text(_) | Event::Command(_) | Event::Ime(_));
    let keys = keys || matches!(ev, Event::Modifier { .. });
    if keys {
      self.take_wake();
      self.fresh();
    }

    let focus = self.ui.focus();
    let used = match ev {
      Event::KeyDown(k) => self.key_down(k),
      Event::Text(_) | Event::Command(_) | Event::Ime(_) | Event::Modifier { .. } => self.typed(ev),
      ev => self.pointer(ev),
    };

    let changed = used | (self.ui.focus() != focus);
    self.ask(changed);
    self.stale |= changed;
    if keys { used } else { changed }
  }

  /// A new appearance: the theme follows (unless the app set one) and the app hears of it;
  /// redraws when it is the first or differs from the last.
  fn appearance(&mut self, a: Appearance) -> bool {
    if self.told && a == self.ui.appearance() {
      return false;
    }

    self.told = true;
    self.ui.set_appearance(a);
    self.app_event(&Event::Appearance(a));
    self.redraw();
    true
  }

  /// Lays the view out again, without painting, when an event changed it since the last
  /// frame: keys and input methods then see the text and lines as they are.
  fn fresh(&mut self) {
    if !self.stale {
      return;
    }

    self.stale = false;
    let (size, scale, now) = (self.size, self.scale, self.clock.now());
    let mut ui = Ui::new(std::mem::take(&mut self.ui), size, self.mouse, now);
    run_view(&mut self.app, &mut self.text, &mut self.images, &mut ui);
    ui.layout(&mut Sys { text: &mut self.text, images: &self.images, scale }, scale);
    self.ui = ui.finish();
  }

  /// The focused text field as an input method sees it: its text, selection, marked text and
  /// where they are; `None` unless an editable field that takes input methods has focus (not a
  /// password). Lays the view out again first when an event changed it.
  pub fn text_input(&mut self) -> Option<TextInput<'_>> {
    self.fresh();
    self.ui.text_input()
  }

  /// The pointer follows the mouse's events; a scroll or pinch carries its own position and
  /// may come after the pointer left (macOS momentum goes on to the view it started in).
  fn pointer(&mut self, ev: Event) -> bool {
    match ev {
      Event::MouseMove { pos, .. } | Event::MouseDown { pos, .. } | Event::MouseUp { pos, .. } => {
        self.mouse = Some(pos)
      }
      Event::MouseExit => self.mouse = None,
      _ => {}
    }

    let changed = self.ui.pointer(&ev, self.clock.now());
    let used = self.app_event(&ev);
    if self.ui.input.click.is_some() {
      self.pass();
      self.ui.input.click = None;
    }

    used | changed
  }

  /// Bindings from the focused element up, then the app's; then the key itself, to the
  /// focused element and the app.
  fn key_down(&mut self, k: KeyEvent) -> bool {
    // A recorder recording or a dialog open takes the keys before any binding.
    if let Some(id) = self.ui.grab.filter(|&g| self.ui.focus() == Some(g))
      && self.deliver(id, Event::KeyDown(k))
    {
      return true;
    }

    let mut at = self.ui.focus().and_then(|id| self.ui.last.find(id));
    while let Some(i) = at {
      let n = &self.ui.last.nodes[i as usize];
      let (id, parent) = (n.id, n.parent);
      if let Some(cmd) = self.ui.last.keymap(i).and_then(|m| keys::lookup(m, &k))
        && self.deliver(id, Event::Command(cmd))
      {
        return true;
      }
      at = (i != 0).then_some(parent);
    }

    if let Some(cmd) = keys::lookup(self.app.keymap(), &k)
      && self.app_event(&Event::Command(cmd))
    {
      return true;
    }
    if let Some(id) = self.ui.focus()
      && self.deliver(id, Event::KeyDown(k))
    {
      return true;
    }
    self.app_event(&Event::KeyDown(k))
  }

  /// Text, a command, an input method's event or a modifier key: the focused element, the app,
  /// then Tab and Shift-Tab move focus.
  fn typed(&mut self, ev: Event) -> bool {
    if let Some(id) = self.ui.focus()
      && self.deliver(id, ev.clone())
    {
      return true;
    }
    if self.app_event(&ev) {
      return true;
    }

    match ev {
      Event::Command(Command::InsertTab) => self.ui.focus_next(true),
      Event::Command(Command::InsertBacktab) => self.ui.focus_next(false),
      _ => false,
    }
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
      ui: &mut self.ui,
    };
    let used = self.app.event(&mut cx, ev);
    let redraw = cx.redraw;
    self.ask(redraw);
    self.stale |= redraw;
    used
  }

  /// Runs the view to deliver `ev` to `id`; returns whether the view used it. One that read it
  /// and gave it back (Enter, Escape in a field) may have changed what it shows: that redraws.
  fn deliver(&mut self, id: Id, ev: Event) -> bool {
    self.ui.deliver(id, ev);
    self.pass();
    let (used, seen) = self.ui.delivered();
    self.ask(seen);
    self.stale |= seen;
    used
  }

  /// A wake from another thread asks for a frame, and lays the view out again before the next
  /// key: what the view shows and binds may follow the work it woke for.
  fn take_wake(&mut self) {
    let woken = self.waker.take();
    self.ask(woken);
    self.stale |= woken;
  }

  /// Runs the view once to deliver input, without layout or drawing.
  fn pass(&mut self) {
    let mut ui = Ui::new(std::mem::take(&mut self.ui), self.size, self.mouse, self.clock.now());
    run_view(&mut self.app, &mut self.text, &mut self.images, &mut ui);
    self.ui = ui.discard();
  }

  /// Whether the next display refresh should draw: an event changed something, an animation
  /// runs, a timer is due or a waker fired.
  pub fn needs_frame(&mut self) -> bool {
    self.take_wake();
    self.dirty || self.animating || self.ui.next_due().is_some_and(|d| self.clock.now() >= d)
  }

  /// Time until the next timer, for a window to sleep until then.
  pub fn due_in(&self) -> Option<Duration> {
    self.ui.next_due().map(|d| d.saturating_sub(self.clock.now()))
  }

  /// Builds the scene from the app (its view laid out and painted, then its paint); the caller
  /// syncs `text` glyphs and `images` to the renderer and draws `scene`. When the hover under
  /// the new layout differs from the one the view saw (the layout moved, content scrolled),
  /// the view runs once more, so hover is never a frame behind; so it does, on the new
  /// layout, when a virtual list shows more than the rows it built (its viewport grew). When
  /// an overlay opened or closed or the window resized ([Ui::shifted]), the next refresh draws
  /// once more, so what the view read from the last frame (a trigger's open look) catches up;
  /// so it does when glyphs were dropped (a full atlas): returns whether this frame must be.
  pub fn frame(&mut self) -> bool {
    self.scene.clear();
    let (size, scale, now) = (self.size, self.scale, self.clock.now());
    // The view asks again for the timers it still needs.
    self.ui.due = None;
    if self.ui.after.is_some_and(|d| d <= now) {
      self.ui.after = None;
    }

    // A flash that ended asks for no more frames.
    if self.ui.input.flash.is_some_and(|(_, until)| until <= now) {
      self.ui.input.flash = None;
    }

    self.ui.tick(now);
    let (mut view, mut layout) = (Duration::ZERO, Duration::ZERO);
    let mut passes = 0;
    let (ui, moved) = loop {
      passes += 1;
      let t0 = Instant::now();
      let mut ui = Ui::new(std::mem::take(&mut self.ui), size, self.mouse, now);
      run_view(&mut self.app, &mut self.text, &mut self.images, &mut ui);
      let t1 = Instant::now();
      ui.layout(&mut Sys { text: &mut self.text, images: &self.images, scale }, scale);
      let moved = ui.rehover();
      // Content built in part shows what it didn't build: again, on this layout's viewports.
      let short = ui.partial_short();
      (view, layout) = (view + (t1 - t0), layout + t1.elapsed());
      if !(short && passes < 3 || moved && passes < 2) {
        break (ui, moved);
      }

      self.ui = if short { ui.finish() } else { ui.discard() };
    };

    let shifted = ui.shifted();
    let t2 = Instant::now();
    self.stale = false;
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
    self.ui = ui.paint(&mut f);
    self.ui_times = [view, layout, t2.elapsed()];
    self.ui_passes = passes;
    let count = self.ui.stats.count + 1;
    self.ui.stats = FrameStats { view, layout, paint: self.ui_times[2], passes, count };
    // In a layer of its own, so the app's paint covers the view's text too (a layer draws its
    // glyphs over its quads).
    f.layer();
    self.app.paint(&mut f);
    self.animating = f.animate || self.ui.animating || !self.ui.input.moving.is_empty();
    // Glyphs dropped, the hover moved again, or an overlay opened or closed or the window
    // resized, so what the view read from the last frame is behind: draw once more, but not
    // over and over.
    let again = moved && !self.rehovered;
    self.rehovered = again;
    let settle = shifted && (!self.settled || self.asked);
    self.settled = settle;
    self.asked = false;
    self.ui.sweep_images(&mut self.images);
    let glyphs = self.text.end_frame();
    self.dirty = glyphs || again || settle;
    glyphs
  }
}

/// Runs `app`'s view on `ui`, lending it the text system for [Ui::measure] and the images for
/// [Ui::rgba_image] (moved in and back: an empty one holds its place, which allocates nothing).
fn run_view<'a, A: App>(
  app: &'a mut A,
  text: &mut TextSystem,
  images: &mut Images,
  ui: &mut Ui<'a>,
) {
  let scale = text.scale();
  ui.text = Some(std::mem::replace(text, TextSystem::new(scale)));
  std::mem::swap(images, &mut ui.images);
  app.view(ui);
  std::mem::swap(images, &mut ui.images);
  if let Some(t) = ui.text.take() {
    *text = t;
  }
}

#[cfg(test)]
mod tests {
  use std::sync::mpsc;

  use super::*;
  use crate::keys::{CMD, Key};
  use crate::scene::Rect;
  use crate::ui::{Kind, Style, Theme, Transition, px};

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

    assert!(!rt.event(Event::MouseMove { pos: Point::new(5.0, 5.0), mods: Modifiers::empty() }));
    assert_eq!(rt.mouse, Some(Point::new(5.0, 5.0)));
    assert!(!rt.needs_frame());

    let key = KeyEvent {
      key: Key::Char('c'),
      text: None,
      code: Code::KeyC,
      mods: Modifiers::empty(),
      repeat: false,
    };
    assert!(rt.event(Event::KeyDown(key)));
    assert!(rt.needs_frame());
    rt.frame();
    assert_eq!(rt.clipboard().read().as_deref(), Some("copied"));
    assert_eq!(rt.scene.counts().quads, 1);

    rt.event(Event::MouseExit);
    assert_eq!(rt.mouse, None);

    // Momentum scrolling after the pointer left leaves it out.
    let (pos, delta) = (Point::new(5.0, 5.0), Point::new(0.0, 3.0));
    let momentum = Phase::Changed;
    let (precise, phase, mods) = (true, Phase::None, Modifiers::empty());
    rt.event(Event::Scroll { pos, delta, precise, phase, momentum, mods });
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
  fn the_view_sees_the_last_frames_cost() {
    #[derive(Default)]
    struct Meter(Vec<u64>);
    impl App for Meter {
      fn view(&mut self, ui: &mut Ui) {
        self.0.push(ui.last_frame().count);
        ui.el(crate::ui::Style::new().w(crate::ui::px(10.0)).h(crate::ui::px(10.0)));
      }
    }
    let clock = Clock::Virtual(Duration::ZERO);
    let size = Size::new(100.0, 100.0);
    let mut rt =
      Runtime::new(Meter::default(), size, 1.0, clock, Box::new(MemClipboard::default()));
    for _ in 0..3 {
      rt.redraw();
      rt.frame();
    }

    assert_eq!(rt.app.0, [0, 1, 2]);
    let st = rt.ui.stats;
    assert_eq!((st.count, st.passes), (3, 1));
    assert_eq!(st.total(), rt.ui_times.iter().sum());
  }

  /// Builds an overlay while `open`, and records whether the last frame laid it out.
  #[derive(Default)]
  struct Pop {
    open: bool,
    seen: Vec<bool>,
  }

  impl App for Pop {
    fn view(&mut self, ui: &mut Ui) {
      self.seen.push(ui.rect(crate::Id::of("pop")).is_some());
      if self.open {
        let s = Style::new().w(px(10.0)).h(px(10.0));
        ui.id("pop").float(crate::ui::Float::window()).el(s);
      }
    }
  }

  #[test]
  fn an_overlay_closing_or_a_resize_draws_once_more() {
    let clock = Clock::Virtual(Duration::ZERO);
    let size = Size::new(100.0, 100.0);
    let mut rt = Runtime::new(Pop::default(), size, 1.0, clock, Box::new(MemClipboard::default()));
    rt.frame();
    assert!(!rt.needs_frame());
    // Opened, then closed: the frame after each sees the last one's overlay, so one more
    // follows, which sees it as it is; then nothing.
    for open in [true, false] {
      rt.app.open = open;
      rt.redraw();
      rt.frame();
      assert!(rt.needs_frame(), "open {open}");
      rt.frame();
      assert!(!rt.needs_frame(), "open {open}");
      assert_eq!(rt.app.seen.last(), Some(&open));
    }

    rt.resize(Size::new(120.0, 100.0), 1.0);
    rt.frame();
    assert!(rt.needs_frame(), "resized");
    rt.frame();
    assert!(!rt.needs_frame());

    // Opened and closed again before the settling frame: the closing frame settles too.
    for open in [true, false] {
      rt.app.open = open;
      rt.redraw();
      rt.frame();
    }

    assert!(rt.needs_frame(), "the closing frame read the open one");
    rt.frame();
    assert!(!rt.needs_frame());
    assert_eq!(rt.app.seen.last(), Some(&false));
  }

  /// An app's own theme: the default dark one with its own ground and accent.
  const MINE: Theme =
    Theme { bg: crate::rgb(0x203040), accent: crate::rgb(0xe06c75), ..Theme::PLATES_DARK };

  /// Draws its ground in the theme's background, optionally over its own theme.
  #[derive(Default)]
  struct Themed {
    pin: bool,
    seen: Vec<(bool, crate::Color)>,
  }

  impl App for Themed {
    fn view(&mut self, ui: &mut Ui) {
      match self.pin {
        true => ui.set_theme(MINE),
        false => ui.follow_system(),
      }

      let t = ui.theme();
      self.seen.push((t.dark, t.accent));
      ui.el(Style::new().grow().bg(t.bg));
    }
  }

  #[test]
  fn the_theme_follows_the_appearance_in_the_same_frame() {
    let mut h = crate::Harness::new(Themed::default(), Size::new(40.0, 40.0), 1.0);
    let ground = |h: &mut crate::Harness<Themed>| h.render().pixel(20.0, 20.0);
    let px = |c: crate::Color| c.map(|v| (v * 255.0).round() as u8);
    assert_eq!(ground(&mut h), px(Theme::PLATES_DARK.bg));
    // The OS turns light: the next frame is light, from the view up.
    assert!(h.event(Event::Appearance(Appearance::LIGHT)));
    assert!(h.rt.needs_frame());
    assert_eq!(ground(&mut h), px(Theme::PLATES_LIGHT.bg));
    assert_eq!(h.app().seen.last(), Some(&(false, Theme::PLATES_LIGHT.accent)));
    // The same again is nothing new; an accent comes with it.
    assert!(!h.event(Event::Appearance(Appearance::LIGHT)));
    let red = crate::rgb(0xff3b30);
    h.event(Event::Appearance(Appearance { dark: true, accent: Some(red) }));
    assert_eq!(ground(&mut h), px(Theme::PLATES_DARK.bg));
    assert_eq!(h.app().seen.last(), Some(&(true, red)));
    // The app's own theme draws, and stays over the OS's appearance.
    h.app_mut().pin = true;
    h.event(Event::Appearance(Appearance::LIGHT));
    assert_eq!(ground(&mut h), px(MINE.bg));
    assert_eq!(h.app().seen.last(), Some(&(true, MINE.accent)));
    assert_eq!(h.rt.ui.appearance(), Appearance::LIGHT);
  }

  #[test]
  fn paint_covers_the_view() {
    /// White text from the view under a black box from paint.
    struct Over;
    impl App for Over {
      fn view(&mut self, ui: &mut Ui) {
        ui.text("Hello", crate::ui::TextStyle::new(crate::Font::ui(20.0), [1.0; 4]));
      }

      fn paint(&mut self, f: &mut Frame) {
        f.rect(Rect::new(0.0, 0.0, 80.0, 40.0), [0.0, 0.0, 0.0, 1.0], 0.0);
      }
    }

    let mut h = crate::Harness::new(Over, Size::new(80.0, 40.0), 1.0);
    let img = h.render();
    assert!(h.rt.ui.last.nodes.len() > 1, "the view built its text");
    for y in 0..40 {
      for x in 0..80 {
        assert_eq!(img.pixel(x as f32, y as f32), [0, 0, 0, 255], "at {x},{y}");
      }
    }
  }

  #[test]
  fn the_app_asks_for_its_window_size() {
    /// A window whose height follows a page, as a preferences window's does.
    struct Tall(f32);
    impl App for Tall {
      fn view(&mut self, ui: &mut Ui) {
        ui.set_window_size(100.0, self.0);
        ui.el(Style::new().grow());
      }
    }
    let mut h = crate::Harness::new(Tall(100.0), Size::new(100.0, 100.0), 1.0);
    h.frame();
    assert_eq!((h.rt.size, h.rt.take_resize()), (Size::new(100.0, 100.0), None));
    // Another page: the frame is built at the new size.
    h.app_mut().0 = 60.0;
    h.rt.redraw();
    h.frame();
    assert_eq!(h.rt.size, Size::new(100.0, 60.0));
    assert_eq!(h.rt.ui.last.nodes[1].rect.h, 60.0);
    // Asked every frame, the same size is nothing new: a size the user set stays.
    h.rt.resize(Size::new(200.0, 80.0), 1.0);
    h.frame();
    assert_eq!((h.rt.size, h.rt.take_resize()), (Size::new(200.0, 80.0), None));
    // From an event handler too.
    let cx_size = |rt: &mut Runtime<Tall>| {
      let mut quit = false;
      let waker = Waker::default();
      let mut cx = Cx {
        size: rt.size,
        scale: 1.0,
        mouse: None,
        now: Duration::ZERO,
        waker: &waker,
        quit: &mut quit,
        redraw: false,
        ui: &mut rt.ui,
      };
      cx.set_window_size(300.0, 90.0);
    };
    cx_size(&mut h.rt);
    assert_eq!(h.rt.take_resize(), Some(Size::new(300.0, 90.0)));
  }

  /// A window as high as its content, a scroll area's whole, between the least and the most.
  #[test]
  fn the_window_fits_its_content() {
    struct Page {
      rows: usize,
    }
    impl App for Page {
      fn view(&mut self, ui: &mut Ui) {
        ui.fit_window_height(200.0, 50.0, 500.0);
        ui.col(Style::new().grow(), |ui| {
          ui.el(Style::new().w(crate::ui::grow()).h(px(40.0)));
          ui.id("list").scroll_area(Style::new().grow(), |ui| {
            for _ in 0..self.rows {
              ui.el(Style::new().w(crate::ui::grow()).h(px(30.0)));
            }
          });
        });
      }
    }
    let mut h = crate::Harness::new(Page { rows: 3 }, Size::new(100.0, 100.0), 1.0);
    h.frame();
    assert_eq!(h.rt.size, Size::new(200.0, 130.0));

    // More than the most: the rest scrolls.
    h.app_mut().rows = 30;
    h.rt.redraw();
    h.frame();
    let list = h.rt.ui.last.get(crate::Id::of("list")).unwrap();
    assert_eq!((h.rt.size, list.rect.h, list.content[1]), (Size::new(200.0, 500.0), 460.0, 900.0));

    // Nothing in it: the least.
    h.app_mut().rows = 0;
    h.rt.redraw();
    h.frame();
    assert_eq!(h.rt.size, Size::new(200.0, 50.0));
  }

  /// An image from pixels shows; one image for the same pixels, kept while built, then dropped.
  #[test]
  fn images_from_pixels_live_while_built() {
    const RED: [u8; 16] = [255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255];
    struct Logo(usize);
    impl App for Logo {
      fn view(&mut self, ui: &mut Ui) {
        for _ in 0..self.0 {
          ui.rgba_image(2, 2, &RED, Style::new().w(px(20.0)).h(px(20.0)));
        }
      }
    }
    let mut h = crate::Harness::new(Logo(2), Size::new(40.0, 40.0), 1.0);
    h.frame();
    let images: Vec<_> = h.rt.ui.pictures.iter().map(|p| p.1).collect();
    assert_eq!(images.len(), 1, "the same pixels, one image");
    assert_eq!(h.rt.images.size(images[0]), Some((2, 2)));
    assert_eq!(
      (h.render().pixel(10.0, 30.0), h.render().pixel(30.0, 30.0)),
      ([255, 0, 0, 255], [0, 0, 0, 255])
    );

    // Built again: kept; not built: dropped.
    h.rt.redraw();
    h.frame();
    assert_eq!(h.rt.images.size(images[0]), Some((2, 2)));
    h.app_mut().0 = 0;
    h.rt.redraw();
    h.frame();
    assert_eq!((h.rt.images.size(images[0]), h.rt.ui.pictures.len()), (None, 0));
  }

  #[test]
  fn virtual_clock() {
    let mut rt = runtime();
    rt.advance(Duration::from_millis(16));
    assert_eq!(rt.now(), Duration::from_millis(16));
  }

  /// A count with a label built before its button, a box that a toggle moves down, a fade on
  /// the box's hover, a timer, and a line from a background thread.
  #[derive(Default)]
  struct Ui1 {
    count: i32,
    shift: bool,
    /// What the last view saw.
    saw_hover: bool,
    fade: f32,
    timer: bool,
    views: usize,
    rx: Option<mpsc::Receiver<String>>,
    line: String,
  }

  impl App for Ui1 {
    fn view(&mut self, ui: &mut Ui) {
      self.views += 1;
      if let Some(line) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
        self.line = line;
      }

      let sq = Style::new().w(px(40.0)).h(px(40.0));
      ui.id("count").text(self.count, crate::ui::TextStyle::new(crate::Font::ui(13.0), [1.0; 4]));
      if ui.id("plus").el(sq).clicked {
        self.count += 1;
      }

      if self.shift {
        ui.el(sq);
      }

      let b = Id::of("box");
      self.saw_hover = ui.hovered(b);
      self.fade = ui.animate(b, if self.saw_hover { 1.0 } else { 0.0 }, Transition::linear(4.0));
      ui.with_id(b).el(sq);
      if self.timer {
        ui.request_frame_in(Duration::from_millis(100));
        self.timer = false;
      }

      ui.id("line").text(&self.line, crate::ui::TextStyle::new(crate::Font::ui(13.0), [1.0; 4]));
    }
  }

  fn ui_runtime() -> Runtime<Ui1> {
    let clock = Clock::Virtual(Duration::ZERO);
    let clipboard = Box::new(MemClipboard::default());
    Runtime::new(Ui1::default(), Size::new(200.0, 200.0), 1.0, clock, clipboard)
  }

  fn text(rt: &Runtime<Ui1>, name: &str) -> String {
    let t = &rt.ui.last;
    let Kind::Text { range, .. } = t.get(Id::of(name)).unwrap().kind else { panic!() };
    t.str(range.0, range.1).to_owned()
  }

  fn click(rt: &mut Runtime<Ui1>, x: f32, y: f32) {
    let (pos, button, mods) = (Point::new(x, y), MouseButton::Left, Modifiers::empty());
    rt.event(Event::MouseDown { pos, button, clicks: 1, mods });
    rt.event(Event::MouseUp { pos, button, mods });
  }

  #[test]
  fn clicks_reach_the_view_before_the_next_frame() {
    let mut rt = ui_runtime();
    rt.frame();
    let r = rt.ui.last.rect(Id::of("plus")).unwrap();
    // Three clicks between frames: each is seen once, by a view run right away.
    for _ in 0..3 {
      click(&mut rt, r.x + 5.0, r.y + 5.0);
    }

    assert_eq!((rt.app.count, rt.app.views), (3, 4));
    assert!(rt.needs_frame());
    rt.frame();
    // The label built before the button shows the new count in the very next frame.
    assert_eq!((text(&rt, "count"), rt.app.count), ("3".into(), 3));
  }

  #[test]
  fn a_cancelled_press_does_not_click() {
    let mut rt = ui_runtime();
    rt.frame();
    let r = rt.ui.last.rect(Id::of("plus")).unwrap();
    let (pos, button, mods) =
      (Point::new(r.x + 5.0, r.y + 5.0), MouseButton::Left, Modifiers::empty());
    rt.event(Event::MouseDown { pos, button, clicks: 1, mods });
    // Another window took the capture: the release that follows, still over it, is not a click.
    rt.cancel_press();
    rt.event(Event::MouseUp { pos, button, mods });
    assert_eq!(rt.app.count, 0);
    click(&mut rt, pos.x, pos.y);
    assert_eq!(rt.app.count, 1);
  }

  #[test]
  fn hover_follows_the_new_layout_in_the_same_frame() {
    let mut rt = ui_runtime();
    rt.frame();
    let b = rt.ui.last.rect(Id::of("box")).unwrap();
    // Just under the box: nothing hovered.
    rt.event(Event::MouseMove {
      pos: Point::new(b.x + 5.0, b.y + b.h + 5.0),
      mods: Modifiers::empty(),
    });
    assert!(!rt.needs_frame());
    // The box moves under the pointer: the view runs again and sees the hover.
    rt.app.shift = true;
    rt.redraw();
    rt.frame();
    assert_eq!((rt.app.saw_hover, rt.ui_passes), (true, 2));
    // The fade starts at 0 and runs frames until it gets to 1 in 250 ms.
    assert_eq!(rt.app.fade, 0.0);
    assert!(rt.needs_frame());
    rt.advance(Duration::from_millis(125));
    rt.frame();
    assert_eq!((rt.app.fade, rt.ui_passes), (0.5, 1));
    rt.advance(Duration::from_millis(125));
    rt.frame();
    assert_eq!(rt.app.fade, 1.0);
    assert!(!rt.needs_frame());
  }

  #[test]
  fn hover_that_moves_its_element_away_settles() {
    /// A box that jumps down while hovered, out from under the pointer.
    struct Dodge;
    impl App for Dodge {
      fn view(&mut self, ui: &mut Ui) {
        let b = Id::of("dodge");
        let top = if ui.hovered(b) { 100.0 } else { 0.0 };
        ui.el(Style::new().h(px(top)));
        ui.with_id(b).el(Style::new().w(px(40.0)).h(px(40.0)));
      }
    }
    let clock = Clock::Virtual(Duration::ZERO);
    let mut rt =
      Runtime::new(Dodge, Size::new(200.0, 200.0), 1.0, clock, Box::new(MemClipboard::default()));
    rt.event(Event::MouseMove { pos: Point::new(5.0, 5.0), mods: Modifiers::empty() });
    let mut frames = 0;
    while rt.needs_frame() && frames < 10 {
      rt.frame();
      frames += 1;
    }

    assert!(frames <= 3 && !rt.needs_frame(), "{frames}");
  }

  #[test]
  fn timers_wake_when_due() {
    let mut rt = ui_runtime();
    rt.app.timer = true;
    rt.frame();
    assert_eq!(rt.due_in(), Some(Duration::from_millis(100)));
    rt.advance(Duration::from_millis(60));
    assert!(!rt.needs_frame());
    rt.advance(Duration::from_millis(40));
    assert!(rt.needs_frame());
    rt.frame();
    assert!(!rt.needs_frame() && rt.due_in().is_none());
  }

  /// A panel with bindings around a focusable item with its own; the app binds too.
  #[derive(Default)]
  struct Keyed {
    log: Vec<String>,
    item_takes: bool,
  }

  const ITEM_KEYS: &[Binding] = &[keys::bind("cmd+z", Command::Undo)];
  const PANEL_KEYS: &[Binding] = &[
    keys::bind("cmd+z", Command::Action("panel undo")),
    keys::bind("cmd+p", Command::Action("panel")),
  ];
  const APP_KEYS: &[Binding] =
    &[keys::bind("cmd+p", Command::Action("app")), keys::bind("cmd+n", Command::Action("new"))];

  impl App for Keyed {
    fn keymap(&self) -> &[Binding] {
      APP_KEYS
    }

    fn event(&mut self, _cx: &mut Cx, ev: &Event) -> bool {
      match ev {
        Event::Command(c @ Command::Action(_)) => self.log.push(format!("app {c:?}")),
        Event::Text(s) => self.log.push(format!("app {s}")),
        _ => return false,
      }

      true
    }

    fn view(&mut self, ui: &mut Ui) {
      let (panel, item) = (Id::of("panel"), Id::of("item"));
      ui.keymap(PANEL_KEYS).with_id(panel).col(Style::new(), |ui| {
        if let Some(c) = ui.command(item) {
          match self.item_takes {
            true => self.log.push(format!("item {c:?}")),
            false => ui.pass(item),
          }
        }

        let sq = Style::new().w(px(20.0)).h(px(20.0)).focusable();
        ui.keymap(ITEM_KEYS).with_id(item).el(sq);
      });
      if let Some(c) = ui.command(panel) {
        self.log.push(format!("panel {c:?}"));
      }
    }
  }

  #[test]
  fn bindings_go_from_the_focused_element_up_to_the_app() {
    let clock = Clock::Virtual(Duration::ZERO);
    let size = Size::new(100.0, 100.0);
    let mut rt =
      Runtime::new(Keyed::default(), size, 1.0, clock, Box::new(MemClipboard::default()));
    rt.frame();
    let key = |rt: &mut Runtime<Keyed>, c: char| {
      let k =
        KeyEvent { key: Key::Char(c), text: None, code: Code::KeyA, mods: CMD, repeat: false };
      rt.event(Event::KeyDown(k))
    };
    // No focus: only the app's bindings; a key nobody binds is unused.
    assert!(!key(&mut rt, 'z'));
    assert!(key(&mut rt, 'n'));
    assert!(rt.event(Event::Text("a".into())));
    // Focused: the item's binding, else (passed) the panel's, which also beats the app's.
    rt.ui.set_focus(Some(Id::of("item")));
    rt.app.item_takes = true;
    assert!(key(&mut rt, 'z'));
    rt.app.item_takes = false;
    assert!(key(&mut rt, 'z'));
    assert!(key(&mut rt, 'p'));
    assert!(key(&mut rt, 'n'));
    let log = [
      "app Action(\"new\")",
      "app a",
      "item Undo",
      "panel Action(\"panel undo\")",
      "panel Action(\"panel\")",
      "app Action(\"new\")",
    ];
    assert_eq!(rt.app.log, log);
    // Tab with nothing taking it moves focus; with one element it stays there.
    rt.ui.set_focus(None);
    assert!(rt.event(Event::Command(Command::InsertTab)));
    assert_eq!(rt.ui.focus(), Some(Id::of("item")));
    assert!(rt.event(Event::Command(Command::InsertBacktab)));
    assert_eq!(rt.ui.focus(), Some(Id::of("item")));
  }

  /// Counts its views, and the views run when each key came.
  #[derive(Default)]
  struct Views {
    views: usize,
    at_key: Vec<usize>,
  }

  impl App for Views {
    fn view(&mut self, _: &mut Ui) {
      self.views += 1;
    }

    fn event(&mut self, _: &mut Cx, ev: &Event) -> bool {
      if let Event::KeyDown(_) = ev {
        self.at_key.push(self.views);
      }

      false
    }
  }

  /// A wake lays the view out again before the next key, whether the platform asked for a
  /// frame since or not: the key meets what the view shows after the work it woke for.
  #[test]
  fn a_wake_refreshes_the_view_before_a_key() {
    let clock = Clock::Virtual(Duration::ZERO);
    let size = Size::new(100.0, 100.0);
    let mut rt =
      Runtime::new(Views::default(), size, 1.0, clock, Box::new(MemClipboard::default()));
    rt.frame();
    let k = KeyEvent {
      key: Key::Char('a'),
      text: None,
      code: Code::KeyA,
      mods: Modifiers::empty(),
      repeat: false,
    };
    rt.event(Event::KeyDown(k));
    assert_eq!(rt.app.at_key, [rt.app.views], "no wake: the last frame's view");

    rt.waker().wake();
    assert!(rt.needs_frame());
    let views = rt.app.views;
    rt.event(Event::KeyDown(k));
    assert!(rt.app.at_key[1] > views, "a wake the platform saw: laid out again");

    rt.frame();
    rt.waker().wake();
    let views = rt.app.views;
    rt.event(Event::KeyDown(k));
    assert!(rt.app.at_key[2] > views, "a wake it didn't see yet: laid out again");
    assert!(rt.needs_frame(), "the wake still draws");
  }

  /// An app that focuses its field on a letter and leaves the key unused.
  #[derive(Default)]
  struct Search;

  impl App for Search {
    fn view(&mut self, ui: &mut Ui) {
      ui.id("field").el(Style::new().w(px(20.0)).h(px(20.0)).focusable());
    }

    fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
      if let Event::KeyDown(k) = ev
        && k.is("a")
      {
        cx.focus(Some(Id::of("field")));
      }

      false
    }
  }

  /// A key that only moved the focus is unused, so the platform's text input still types it;
  /// the new focus draws.
  #[test]
  fn a_key_that_only_moves_focus_is_unused() {
    let clock = Clock::Virtual(Duration::ZERO);
    let size = Size::new(100.0, 100.0);
    let mut rt = Runtime::new(Search, size, 1.0, clock, Box::new(MemClipboard::default()));
    rt.frame();
    let k = KeyEvent {
      key: Key::Char('a'),
      text: None,
      code: Code::KeyA,
      mods: Modifiers::empty(),
      repeat: false,
    };
    assert!(!rt.event(Event::KeyDown(k)));
    assert_eq!(rt.ui.focus(), Some(Id::of("field")));
    assert!(rt.needs_frame());
  }

  /// A focusable item in a panel that binds keys only when the window is wide.
  #[derive(Default)]
  struct Wide(Vec<String>);

  impl App for Wide {
    fn view(&mut self, ui: &mut Ui) {
      let panel = Id::of("panel");
      if ui.size().w > 150.0 {
        ui.keymap(PANEL_KEYS);
      }

      ui.with_id(panel).col(Style::new(), |ui| {
        ui.id("item").el(Style::new().w(px(20.0)).h(px(20.0)).focusable());
      });
      if let Some(c) = ui.command(panel) {
        self.0.push(format!("{c:?}"));
      }
    }
  }

  #[test]
  fn a_key_after_a_resize_sees_its_layout() {
    let clock = Clock::Virtual(Duration::ZERO);
    let (size, clip) = (Size::new(100.0, 100.0), Box::new(MemClipboard::default()));
    let mut rt = Runtime::new(Wide::default(), size, 1.0, clock, clip);
    rt.frame();
    rt.ui.set_focus(Some(Id::of("item")));
    // Laid out wide, with the panel's bindings, before the key goes to it.
    rt.resize(Size::new(200.0, 100.0), 1.0);
    let k =
      KeyEvent { key: Key::Char('p'), text: None, code: Code::KeyA, mods: CMD, repeat: false };
    assert!(rt.event(Event::KeyDown(k)));
    assert_eq!(rt.app.0, ["Action(\"panel\")"]);
  }

  #[test]
  fn the_view_measures_text_as_the_layout_does() {
    #[derive(Default)]
    struct Measured(f32);
    impl App for Measured {
      fn view(&mut self, ui: &mut Ui) {
        let style = crate::ui::TextStyle::new(crate::Font::ui(20.0), [1.0; 4]);
        self.0 = ui.measure("1,234.5", style);
        ui.id("t").text("1,234.5", style.nowrap());
      }
    }
    let clock = Clock::Virtual(Duration::ZERO);
    let size = Size::new(200.0, 100.0);
    let mut rt =
      Runtime::new(Measured::default(), size, 2.0, clock, Box::new(MemClipboard::default()));
    rt.frame();
    assert!(rt.app.0 > 40.0, "{}", rt.app.0);
    // The layout's width, snapped to pixels.
    assert!((rt.ui.last.rect(Id::of("t")).unwrap().w - rt.app.0).abs() <= 0.5);
    // Outside a runtime there is nothing to measure with.
    let mut ui = Ui::new(UiState::default(), size, None, Duration::ZERO);
    assert_eq!(ui.measure("1", crate::ui::TextStyle::new(crate::Font::ui(20.0), [1.0; 4])), 0.0);
  }

  #[test]
  fn background_work_shows_after_a_wake() {
    let mut rt = ui_runtime();
    let (tx, rx) = mpsc::channel();
    rt.app.rx = Some(rx);
    rt.frame();
    assert!(!rt.needs_frame());
    let waker = rt.waker().clone();
    std::thread::spawn(move || {
      tx.send("loaded 42 rows".to_owned()).unwrap();
      waker.wake();
    })
    .join()
    .unwrap();
    assert!(rt.needs_frame());
    rt.frame();
    assert_eq!(text(&rt, "line"), "loaded 42 rows");
  }
}
