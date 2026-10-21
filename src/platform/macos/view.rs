// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The NSView subclass: a layer we draw with the CPU renderer (IOSurfaces as its contents),
//! NSEvents converted to [Event], and a display link that runs only while the
//! runtime needs frames (input, animations); a timer the app asked for sleeps on the main queue
//! until it is due and draws its frame there, leaving the display link paused. While the window is
//! hidden (minimized, covered, on another space) frames wait until it shows again. Keys nobody used
//! go through `interpretKeyEvents:`, which calls back `insertText:` and `doCommandBySelector:` (see
//! [crate::keys]); the pointer's cursor follows the runtime's.

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchTime, MainThreadBound};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, Sel};
use objc2::{
  AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use objc2_app_kit::{
  NSCursor, NSEvent, NSEventModifierFlags, NSEventPhase, NSResponder, NSTrackingArea,
  NSTrackingAreaOptions, NSView, NSWindow, NSWindowOcclusionState,
};
use objc2_foundation::{
  NSArray, NSAttributedString, NSObject, NSPoint, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize,
  NSString,
};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange, CALayer, kCAGravityTopLeft};

use super::keyboard;
use super::surface::Presenter;
use crate::app::{App, Event, Mods, MouseButton, Phase, Runtime, WHEEL_LINE};
use crate::hotkey::ModKey;
use crate::keys::{self, Command, Key, KeyEvent};
use crate::scene::{Point, Size};
use crate::ui::Cursor;

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
  /// Timestamp of the last key event offered through `performKeyEquivalent:`, which AppKit
  /// sends again as `keyDown:` when neither we nor the menu used it.
  key_equivalent: Cell<f64>,
  /// When the scheduled timer wake fires, on the runtime's clock.
  timer: Cell<Option<Duration>>,
  /// `interpretKeyEvents:` runs for a key with Cmd: what it would type is dropped.
  cmd_key: Cell<bool>,
  /// The cursor set while the pointer is in the view; none outside it.
  cursor: Cell<Option<Cursor>>,
  /// The display link was invalidated (the view left its window): a new one starts when it
  /// moves to a window again.
  stopped: Cell<bool>,
  /// The buttons pressed in the view and not let go yet, a bit per button number: AppKit sends
  /// each release to the view that got the press, while `pressedMouseButtons` is the mouse's
  /// state now, ahead of the events queued and with presses in other windows.
  held: Cell<u64>,
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

    #[unsafe(method(acceptsFirstResponder))]
    fn accepts_first_responder(&self) -> bool {
      true
    }

    #[unsafe(method(acceptsFirstMouse:))]
    fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
      true
    }

    #[unsafe(method(setFrameSize:))]
    fn set_frame_size(&self, size: NSSize) {
      let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
      // Draw right away, inside the resize transaction, so the content never stretches; then
      // the display link runs while that frame wants more (a paused one would stall them).
      self.render();
      self.after_frame();
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
      // `render` picks up the new scale and rebuilds the glyph cache at it.
      self.wake();
    }

    #[unsafe(method(mouseMoved:))]
    fn mouse_moved(&self, event: &NSEvent) {
      self.moved(event);
    }

    /// The pointer came in, maybe without moving (the window moved or resized under it): a
    /// move there, so the hover and the cursor follow.
    #[unsafe(method(mouseEntered:))]
    fn mouse_entered(&self, event: &NSEvent) {
      self.moved(event);
    }

    #[unsafe(method(mouseDragged:))]
    fn mouse_dragged(&self, event: &NSEvent) {
      self.moved(event);
    }

    #[unsafe(method(rightMouseDragged:))]
    fn right_mouse_dragged(&self, event: &NSEvent) {
      self.moved(event);
    }

    #[unsafe(method(otherMouseDragged:))]
    fn other_mouse_dragged(&self, event: &NSEvent) {
      self.moved(event);
    }

    #[unsafe(method(mouseExited:))]
    fn mouse_exited(&self, _event: &NSEvent) {
      self.send(Event::MouseExit);
    }

    #[unsafe(method(mouseDown:))]
    fn mouse_down(&self, event: &NSEvent) {
      self.button(event, true);
    }

    #[unsafe(method(mouseUp:))]
    fn mouse_up(&self, event: &NSEvent) {
      self.button(event, false);
    }

    #[unsafe(method(rightMouseDown:))]
    fn right_mouse_down(&self, event: &NSEvent) {
      self.button(event, true);
    }

    #[unsafe(method(rightMouseUp:))]
    fn right_mouse_up(&self, event: &NSEvent) {
      self.button(event, false);
    }

    #[unsafe(method(otherMouseDown:))]
    fn other_mouse_down(&self, event: &NSEvent) {
      self.button(event, true);
    }

    #[unsafe(method(otherMouseUp:))]
    fn other_mouse_up(&self, event: &NSEvent) {
      self.button(event, false);
    }

    #[unsafe(method(scrollWheel:))]
    fn scroll_wheel(&self, event: &NSEvent) {
      let precise = event.hasPreciseScrollingDeltas();
      let k = if precise { 1.0 } else { WHEEL_LINE };
      let delta = Point::new(event.scrollingDeltaX() as f32 * k, event.scrollingDeltaY() as f32 * k);
      self.send(Event::Scroll {
        pos: self.location(event),
        delta,
        precise,
        phase: phase(event.phase()),
        momentum: phase(event.momentumPhase()),
        mods: mods(event),
      });
    }

    #[unsafe(method(magnifyWithEvent:))]
    fn magnify(&self, event: &NSEvent) {
      self.send(Event::Magnify {
        pos: self.location(event),
        delta: event.magnification() as f32,
        phase: phase(event.phase()),
        mods: mods(event),
      });
    }

    #[unsafe(method(performKeyEquivalent:))]
    fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
      self.key_equivalent(event)
    }

    #[unsafe(method(keyDown:))]
    fn key_down(&self, event: &NSEvent) {
      // A key equivalent nobody used comes back here: it was offered already.
      let offered = event.timestamp() == self.ivars().key_equivalent.get();
      if offered || !self.send(Event::KeyDown(key_event(event))) {
        self.interpret(event);
      }
    }

    #[unsafe(method(keyUp:))]
    fn key_up(&self, event: &NSEvent) {
      self.send(Event::KeyUp(KeyEvent { repeat: false, ..key_event(event) }));
    }

    /// Text from `interpretKeyEvents:`.
    #[unsafe(method(insertText:))]
    fn insert_text(&self, text: &AnyObject) {
      if let Some(s) = ns_text(text) {
        self.typed(s);
      }
    }

    /// A command from `interpretKeyEvents:` (`moveWordLeft:`); unknown ones are dropped
    /// quietly, where NSResponder would beep.
    #[unsafe(method(doCommandBySelector:))]
    fn do_command(&self, sel: Sel) {
      if let Some(cmd) = sel.name().to_str().ok().and_then(Command::from_selector) {
        self.send(Event::Command(cmd));
      }
    }

    #[unsafe(method(cursorUpdate:))]
    fn cursor_update(&self, _event: &NSEvent) {
      let state = self.ivars().state.try_borrow();
      let c = state.ok().and_then(|s| Some(s.as_ref()?.rt.cursor())).unwrap_or(Cursor::Arrow);
      ns_cursor(c).set();
      self.ivars().cursor.set(Some(c));
    }

    #[unsafe(method(flagsChanged:))]
    fn flags_changed(&self, event: &NSEvent) {
      self.send(Event::Mods(mods(event)));
      // The device-dependent flags (`NX_DEVICE*KEYMASK`) tell whether the key went down.
      if let Some(key) = ModKey::of_mac(event.keyCode()) {
        let down = event.modifierFlags().0 as u64 & key.mac_flag() != 0;
        self.send(Event::Modifier { key, down });
      }
    }

    #[unsafe(method(tick:))]
    fn tick(&self, _link: &CADisplayLink) {
      let pause = {
        let mut state = self.ivars().state.borrow_mut();
        let Some(st) = state.as_mut() else { return };
        let idle = !st.rt.needs_frame() || !self.visible();
        if idle {
          // Nothing changed since the last frame: stop until the next event, wake or timer. Hidden:
          // until the window shows again.
          st.link.setPaused(true);
          if self.visible() {
            self.schedule(st);
          }
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

    // Mouse moves and exits for the visible rect, which AppKit keeps up to date, in an inactive
    // app too; `cursorUpdate:` in an area of its own, as AppKit sends none with `ActiveAlways`.
    let opts = NSTrackingAreaOptions::MouseEnteredAndExited
      | NSTrackingAreaOptions::MouseMoved
      | NSTrackingAreaOptions::ActiveAlways
      | NSTrackingAreaOptions::InVisibleRect;
    let cursors = NSTrackingAreaOptions::CursorUpdate
      | NSTrackingAreaOptions::ActiveInActiveApp
      | NSTrackingAreaOptions::InVisibleRect;
    for opts in [opts, cursors] {
      let area = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
          NSTrackingArea::alloc(),
          NSRect::ZERO,
          opts,
          Some(&view),
          None,
        )
      };
      view.addTrackingArea(&area);
    }

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

  /// The window became key or resigned: modifiers that changed while another window had the
  /// keys never came here, so none while away, and those held now on return.
  pub fn focus(&self, on: bool) {
    if on {
      self.send(Event::Focus(true));
      self.send(Event::Mods(flags_mods(NSEvent::modifierFlags_class())));
    } else {
      self.send(Event::Mods(Mods::NONE));
      self.send(Event::Focus(false));
    }
  }

  /// Feeds an event to the runtime and restarts the display link if it needs a frame.
  pub fn send(&self, ev: Event) -> bool {
    let (used, quit) = {
      let mut state = self.ivars().state.borrow_mut();
      let Some(st) = state.as_mut() else { return false };
      let used = st.rt.event(ev);
      match st.rt.needs_frame() {
        true => st.link.setPaused(false),
        false => self.schedule(st),
      }

      self.sync_cursor(st);
      (used, st.rt.quit_requested())
    };
    if quit {
      super::shell::quit();
    }

    self.apply_resize();
    used
  }

  /// Resizes the window's content to the size the app asked for ([Runtime::take_resize]),
  /// keeping its top edge where it is, as preferences windows do when their tab changes. The
  /// least content size comes down with it; a fixed window stays fixed for the user. Outside the
  /// runtime's borrow: the new frame size draws at once ([Self::render]).
  fn apply_resize(&self) {
    let want = self.ivars().state.borrow_mut().as_mut().and_then(|st| st.rt.take_resize());
    let (Some(size), Some(window)) = (want, self.window()) else { return };
    let old = window.frame();
    let content = window.contentRectForFrameRect(old);
    let rect = NSRect::new(content.origin, NSSize::new(size.w as f64, size.h as f64));
    let mut frame = window.frameRectForContentRect(rect);
    frame.origin.y = old.origin.y + old.size.height - frame.size.height;
    let min = window.contentMinSize();
    let (w, h) = (min.width.min(size.w as f64), min.height.min(size.h as f64));
    window.setContentMinSize(NSSize::new(w, h));
    window.setFrame_display(frame, true);
  }

  /// Typed text, unless it came with Cmd or is a control character.
  fn typed(&self, s: String) {
    if !self.ivars().cmd_key.get() && keys::printable(&s) {
      self.send(Event::Text(s));
    }
  }

  fn moved(&self, event: &NSEvent) {
    self.send(Event::MouseMove { pos: self.location(event), mods: mods(event) });
  }

  /// Offers a key with Cmd (or another key equivalent) to the app before the menu; AppKit
  /// asks every view of the key window.
  fn key_equivalent(&self, event: &NSEvent) -> bool {
    if !self.window().is_some_and(|w| w.isKeyWindow()) {
      return false;
    }

    self.ivars().key_equivalent.set(event.timestamp());
    self.send(Event::KeyDown(key_event(event)))
  }

  /// Lets AppKit's text input turn a key nobody used into text or a command.
  fn interpret(&self, event: &NSEvent) {
    let cmd = event.modifierFlags().contains(NSEventModifierFlags::Command);
    self.ivars().cmd_key.set(cmd);
    self.interpretKeyEvents(&NSArray::from_slice(&[event]));
    self.ivars().cmd_key.set(false);
  }

  /// Shows the runtime's cursor when the pointer is in the view and it isn't the one shown: one
  /// asked for while the pointer was outside shows as it comes in.
  fn sync_cursor(&self, st: &State) {
    let want = st.rt.mouse.map(|_| st.rt.cursor());
    if let Some(c) = cursor_change(self.ivars().cursor.replace(want), want) {
      ns_cursor(c).set();
    }
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

  /// Sleeps until the runtime's next timer, unless an earlier wake is already set.
  fn schedule(&self, st: &mut State) {
    let Some(left) = st.rt.due_in() else { return };
    let at = st.rt.now() + left;
    if self.ivars().timer.get().is_some_and(|t| t <= at) {
      return;
    }

    self.ivars().timer.set(Some(at));
    let weak = self.weak();
    let when = DispatchTime::try_from(left).unwrap_or(DispatchTime::NOW);
    let _ = DispatchQueue::main().after(when, move || {
      let mtm = MainThreadMarker::new().unwrap();
      let Some(view) = weak.get(mtm).load() else { return };
      // An earlier wake superseded this one: that one runs and sets the next.
      if view.ivars().timer.get() != Some(at) {
        return;
      }

      view.ivars().timer.set(None);
      // A timer frame draws at once, without waking the display link: unpausing it for one
      // frame cost ~100 wakeups a second at 10 Hz. Hidden, it waits for
      // the window to show ([Self::occlusion_changed]).
      if !view.visible() {
        return;
      }

      let due = {
        let mut state = view.ivars().state.borrow_mut();
        let Some(st) = state.as_mut() else { return };
        match st.rt.needs_frame() {
          true => st.link.isPaused(),
          false => {
            view.schedule(st);
            false
          }
        }
      };
      if due {
        view.render();
        view.after_frame();
      }
    });
  }

  /// After a frame: the display link runs on while the runtime needs frames (animations, drags),
  /// else it stops at once, not a refresh later, and the view sleeps until the next timer.
  fn after_frame(&self) {
    {
      let mut state = self.ivars().state.borrow_mut();
      let Some(st) = state.as_mut() else { return };
      match st.rt.needs_frame() {
        true => st.link.setPaused(false),
        false => {
          st.link.setPaused(true);
          self.schedule(st);
        }
      }
    }
    self.apply_resize();
  }

  fn button(&self, event: &NSEvent, down: bool) {
    let (pos, mods) = (self.location(event), mods(event));
    let button = match event.buttonNumber() {
      0 => MouseButton::Left,
      1 => MouseButton::Right,
      2 => MouseButton::Middle,
      n => MouseButton::Other(n as u8),
    };

    let held = buttons(self.ivars().held.get(), event.buttonNumber(), down);
    self.ivars().held.set(held);
    if down {
      self.send(Event::MouseDown { pos, button, clicks: event.clickCount() as u32, mods });
      return;
    }

    self.send(Event::MouseUp { pos, button, mods });
    // The last button of a drag let go outside: AppKit sent the exit during the drag, and the
    // release put the pointer back in.
    let bounds = self.bounds().size;
    if released_outside(pos, Size::new(bounds.width as f32, bounds.height as f32), held) {
      self.send(Event::MouseExit);
    }
  }

  fn location(&self, event: &NSEvent) -> Point {
    let p: NSPoint = self.convertPoint_fromView(event.locationInWindow(), None);
    Point::new(p.x as f32, p.y as f32)
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
    st.presenter.renderer.sync(&mut st.rt.text, &mut st.rt.images);
    st.presenter.present(&st.rt.scene, size, scale);
    // Every surface busy: the frame is drawn again at the next tick.
    if st.presenter.starved {
      st.rt.redraw();
    }

    self.sync_cursor(st);
  }
}

// MARK: NSEvent conversion

/// The string of an NSString or NSAttributedString, as text input passes either.
fn ns_text(text: &AnyObject) -> Option<String> {
  match (text.downcast_ref::<NSAttributedString>(), text.downcast_ref::<NSString>()) {
    (Some(a), _) => Some(a.string().to_string()),
    (None, Some(s)) => Some(s.to_string()),
    (None, None) => None,
  }
}

fn mods(event: &NSEvent) -> Mods {
  flags_mods(event.modifierFlags())
}

fn flags_mods(f: NSEventModifierFlags) -> Mods {
  Mods {
    shift: f.contains(NSEventModifierFlags::Shift),
    ctrl: f.contains(NSEventModifierFlags::Control),
    alt: f.contains(NSEventModifierFlags::Option),
    cmd: f.contains(NSEventModifierFlags::Command),
  }
}

fn phase(p: NSEventPhase) -> Phase {
  match p {
    NSEventPhase::MayBegin => Phase::MayBegin,
    NSEventPhase::Began => Phase::Began,
    NSEventPhase::Changed | NSEventPhase::Stationary => Phase::Changed,
    NSEventPhase::Ended => Phase::Ended,
    NSEventPhase::Cancelled => Phase::Cancelled,
    _ => Phase::None,
  }
}

/// A key event: named keys by virtual key code, the rest by the character they type. With Cmd
/// or Ctrl a non-Latin character is the ASCII-capable layout's, so shortcuts work on a Russian
/// layout as AppKit's menus do; with Cmd on a layout that maps keys anew under it ("Dvorak –
/// QWERTY ⌘"), that mapping's.
fn key_event(event: &NSEvent) -> KeyEvent {
  let (code, mods) = (event.keyCode(), mods(event));
  let first = |s: Option<Retained<NSString>>| s.and_then(|s| s.to_string().chars().next());
  let cmd = mods.cmd.then(|| keyboard::cmd_char(code, mods.shift)).flatten();
  let key = Key::named(code).unwrap_or_else(|| match first(event.charactersIgnoringModifiers()) {
    Some(_) if let Some(c) = cmd => Key::Char(c),
    Some(c) if (mods.cmd || mods.ctrl) && !c.is_ascii() => {
      keyboard::ascii_char(code, mods.shift).map_or(Key::Char(c), Key::Char)
    }
    Some(c) if !c.is_control() => Key::Char(c),
    _ => Key::Other(code),
  });
  let text = match key {
    Key::Char(_) if !(mods.cmd || mods.ctrl) => {
      let s = event.characters().map(|s| s.to_string()).unwrap_or_default();
      let mut cs = s.chars();
      match (cs.next(), cs.next()) {
        (Some(c), None) if keys::printable(&s) => Some(c),
        _ => None,
      }
    }
    _ => None,
  };

  KeyEvent { key, text, code, mods, repeat: event.isARepeat() }
}

/// The cursor to set, going from the one shown in the view to the one wanted there (none: the
/// pointer is outside).
fn cursor_change(shown: Option<Cursor>, want: Option<Cursor>) -> Option<Cursor> {
  want.filter(|_| want != shown)
}

/// The system cursor for ours.
fn ns_cursor(c: Cursor) -> Retained<NSCursor> {
  match c {
    Cursor::Inherit | Cursor::Arrow => NSCursor::arrowCursor(),
    Cursor::Text => NSCursor::IBeamCursor(),
    Cursor::Pointer => NSCursor::pointingHandCursor(),
    // `columnResizeCursor` and `rowResizeCursor` replace these from macOS 15 only.
    #[allow(deprecated)]
    Cursor::ResizeX => NSCursor::resizeLeftRightCursor(),
    #[allow(deprecated)]
    Cursor::ResizeY => NSCursor::resizeUpDownCursor(),
    Cursor::Crosshair => NSCursor::crosshairCursor(),
    Cursor::Grab => NSCursor::openHandCursor(),
    Cursor::Grabbing => NSCursor::closedHandCursor(),
    Cursor::NotAllowed => NSCursor::operationNotAllowedCursor(),
  }
}

/// The buttons `held` after button `number` went `down` or up; a number past the mask's bits
/// counts as none.
fn buttons(held: u64, number: isize, down: bool) -> u64 {
  let bit = u32::try_from(number).ok().and_then(|n| 1u64.checked_shl(n)).unwrap_or(0);
  if down { held | bit } else { held & !bit }
}

/// Whether a release at `pos` ends a drag outside the view of `size`: no button `held` any
/// more, and the pointer off it.
fn released_outside(pos: Point, size: Size, held: u64) -> bool {
  let inside = (0.0..size.w).contains(&pos.x) && (0.0..size.h).contains(&pos.y);
  held == 0 && !inside
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The last button let go outside the view takes the pointer out; one let go inside, or with
  /// another still held, doesn't.
  #[test]
  fn a_drag_released_outside_exits() {
    let size = Size::new(100.0, 80.0);
    assert!(released_outside(Point::new(120.0, 40.0), size, 0));
    assert!(released_outside(Point::new(50.0, -1.0), size, 0));
    assert!(!released_outside(Point::new(50.0, 40.0), size, 0));
    assert!(!released_outside(Point::new(120.0, 40.0), size, 0b10));
  }

  /// The buttons held are the view's own presses: the left one let go is the last even while
  /// the right one is down in another window, the right one pressed here holds the exit back,
  /// and a button number past the mask is ignored.
  #[test]
  fn held_buttons_are_the_views_presses() {
    let left = buttons(0, 0, true);
    assert_eq!(buttons(left, 0, false), 0);
    let both = buttons(left, 1, true);
    assert_eq!(buttons(both, 0, false), 0b10);
    assert_eq!(buttons(both, 70, true), both);
    assert_eq!(buttons(both, -1, false), both);
  }

  /// A crosshair asked for while the pointer is outside: set as it comes in, not again while it
  /// stays, and again after it left and came back.
  #[test]
  fn a_cursor_asked_for_outside_shows_as_the_pointer_enters() {
    let cross = Some(Cursor::Crosshair);
    assert_eq!(cursor_change(None, None), None);
    assert_eq!(cursor_change(None, cross), cross);
    assert_eq!(cursor_change(cross, cross), None);
    assert_eq!(cursor_change(cross, None), None);
    assert_eq!(cursor_change(cross, Some(Cursor::Arrow)), Some(Cursor::Arrow));
  }
}
