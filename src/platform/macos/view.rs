// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The NSView subclass: a layer we draw with the CPU renderer (IOSurfaces as its contents),
//! NSEvents converted to [Event], and a display link that runs only while the runtime needs frames
//! (input, animations); a timer the app asked for sleeps on the main queue until it is due (a
//! refresh after the last frame at the soonest) and draws its frame there, leaving the display link
//! paused. While the window is hidden (minimized, covered, on another space) frames wait until it
//! shows again. Keys nobody used go through `interpretKeyEvents:`, which calls back `insertText:`
//! and `doCommandBySelector:` (see [crate::keys]); the pointer's cursor follows the runtime's.
//!
//! The view is an `NSTextInputClient`: while a text field that takes input methods has focus
//! ([Runtime::text_input]), it has an input context, so input methods compose marked text in the
//! field ([Event::Ime]) and ask where its text is, placing their candidates under the caret;
//! while they compose, keys go to them first. With no such field the view has no input context
//! and keys stay plain. Assistive clients see its nodes, not the view itself ([a11y]).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{
  _dispatch_source_type_timer, DispatchObject, DispatchQueue, DispatchRetained, DispatchSource,
  DispatchTime, MainThreadBound,
};
use keyboard_types::Modifiers;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{
  AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
  sel,
};
use objc2_app_kit::{
  NSDragOperation, NSDraggingInfo, NSEvent, NSEventModifierFlags, NSPasteboardTypeFileURL,
  NSResponder, NSTextInputClient, NSTextInputContext, NSTrackingArea, NSTrackingAreaOptions,
  NSView, NSWindow, NSWindowOcclusionState, NSWindowStyleMask,
};
use objc2_foundation::{
  NSArray, NSAttributedString, NSAttributedStringKey, NSNotFound, NSObject, NSObjectProtocol,
  NSPoint, NSRange, NSRangePointer, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize, NSString,
  NSUInteger, NSUserDefaults, ns_string,
};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange, CALayer, kCAGravityTopLeft};

use super::input::*;
use super::surface::Presenter;
use super::{a11y, keyboard, overlay};
use crate::app::{App, Event, Ime, Runtime, WHEEL_LINE};
use crate::keys::{self, Command, KeyEvent};
use crate::scene::{Point, Rect, Size};
use crate::shell::Anchor;
use crate::text::edit::utf16_len;
use crate::ui::{Cursor, Id, TextInput, UiState};

type WeakView = Arc<MainThreadBound<Weak<View>>>;

pub struct State {
  rt: Runtime<Box<dyn App>>,
  /// The CPU renderer's frames, presented as the layer's contents.
  presenter: Presenter,
  link: Retained<CADisplayLink>,
  /// The timer wake: one, set again for each new deadline.
  wake: Wake,
}

#[derive(Default)]
pub struct Ivars {
  state: RefCell<Option<State>>,
  /// Events that came while the runtime was busy up the stack, sent in order before the next.
  waiting: RefCell<VecDeque<Event>>,
  /// Timestamp of the last key event offered through `performKeyEquivalent:`, which AppKit
  /// sends again as `keyDown:` when neither we nor the menu used it.
  key_equivalent: Cell<f64>,
  /// When the scheduled timer wake fires, on the runtime's clock.
  timer: Cell<Option<Duration>>,
  /// When the last frame started, on the runtime's clock: a timer frame comes a refresh later at
  /// the soonest.
  last_frame: Cell<Option<Duration>>,
  /// `interpretKeyEvents:` runs for a key with Cmd: what it would type is dropped.
  cmd_key: Cell<bool>,
  /// The cursor set while the pointer is in the view; none outside it.
  cursor: Cell<Option<Cursor>>,
  /// The display link was invalidated (the view left its window): a new one starts when it
  /// moves to a window again.
  stopped: Cell<bool>,
  /// A wake that came while the runtime was busy up the stack (a nested run loop: a panel, a
  /// permission prompt, Finder revealing a file), done once it is free ([View::deliver],
  /// [View::after_frame]).
  rewake: Cell<bool>,
  /// The buttons pressed in the view and not let go yet, a bit per button number: AppKit sends
  /// each release to the view that got the press, while `pressedMouseButtons` is the mouse's
  /// state now, ahead of the events queued and with presses in other windows.
  held: Cell<u64>,
  /// The field input methods work for and whether it had marked text, as of the last event or
  /// frame; whether the input method's own call is under way.
  ime_field: Cell<Option<Id>>,
  ime_marked: Cell<bool>,
  ime_call: Cell<bool>,
  /// Where the composition was as the input method last saw it: when the text moves under it
  /// (scrolled), its windows are told.
  ime_at: Cell<Option<Rect>>,
  /// An overlay's anchor ([crate::Config::overlay]): a new size goes back to it.
  overlay: Cell<Option<Anchor>>,
  /// The last secondary press, which pops up the context menu it asks for ([View::show_menu]).
  press: RefCell<Option<Retained<NSEvent>>>,
  /// What assistive clients see, from their first query ([a11y]).
  pub(super) a11y: RefCell<Option<Rc<a11y::Shared>>>,
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

    /// AppKit never moves the window from the view: the app says where a press does
    /// ([View::drag_window]).
    #[unsafe(method(mouseDownCanMoveWindow))]
    fn mouse_down_can_move_window(&self) -> bool {
      false
    }

    #[unsafe(method(draggingEntered:))]
    fn dragging_entered(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
      self.drag(info, false)
    }

    #[unsafe(method(draggingUpdated:))]
    fn dragging_updated(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
      self.drag(info, false)
    }

    #[unsafe(method(draggingExited:))]
    fn dragging_exited(&self, _info: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
      self.send(Event::DragExit);
    }

    #[unsafe(method(performDragOperation:))]
    fn perform_drag(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
      self.drag(info, true) != NSDragOperation::None
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
        if let Ok(Some(st)) = self.ivars().state.try_borrow_mut().as_deref_mut() {
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
      if self.drag_window(event) {
        return;
      }

      // A click ends composing: the marked text stays as typed, the input method lets go.
      if self.composing() {
        self.send(Event::Ime(Ime::Unmark));
      }

      // A Control-click is the secondary click: its press is the context menu's event.
      if mods(event).contains(Modifiers::CONTROL) {
        self.ivars().press.replace(Some(event.retain()));
      }

      self.button(event, true);
    }

    #[unsafe(method(mouseUp:))]
    fn mouse_up(&self, event: &NSEvent) {
      self.button(event, false);
    }

    #[unsafe(method(rightMouseDown:))]
    fn right_mouse_down(&self, event: &NSEvent) {
      self.ivars().press.replace(Some(event.retain()));
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
      // While an input method composes, keys are its own first.
      let offered = event.timestamp() == self.ivars().key_equivalent.get();
      if offered || self.composing() || !self.send(Event::KeyDown(key_event(event))) {
        self.interpret(event);
      }
    }

    #[unsafe(method(keyUp:))]
    fn key_up(&self, event: &NSEvent) {
      self.send(Event::KeyUp(KeyEvent { repeat: false, ..key_event(event) }));
    }

    /// Text from `interpretKeyEvents:` with no input context (no field focused).
    #[unsafe(method(insertText:))]
    fn insert_text(&self, text: &AnyObject) {
      if let Some(s) = ns_text(text) {
        self.typed(s);
      }
    }

    /// An input context only while a field that takes input methods has focus: without one,
    /// input methods leave the keys alone.
    #[unsafe(method_id(inputContext))]
    fn input_context(&self) -> Option<Retained<NSTextInputContext>> {
      match self.input(|_| ()).is_some() {
        true => unsafe { msg_send![super(self), inputContext] },
        false => None,
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
      let key = keyboard::code(event.keyCode());
      if let Some(flag) = keyboard::device_flag(key) {
        let down = event.modifierFlags().0 as u64 & flag != 0;
        self.send(Event::Modifier { key, down });
      }
    }

    #[unsafe(method(tick:))]
    fn tick(&self, _link: &CADisplayLink) {
      let pause = {
        // Busy up the stack (a nested run loop): this refresh is skipped, the link runs on.
        let Ok(mut state) = self.ivars().state.try_borrow_mut() else { return };
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

    #[unsafe(method_id(accessibilityChildren))]
    fn accessibility_children(&self) -> Option<Retained<NSArray>> {
      a11y::children(self)
    }

    #[unsafe(method_id(accessibilityFocusedUIElement))]
    fn accessibility_focused(&self) -> Option<Retained<AnyObject>> {
      a11y::focused(self)
    }

    #[unsafe(method_id(accessibilityHitTest:))]
    fn accessibility_hit_test(&self, p: NSPoint) -> Option<Retained<AnyObject>> {
      a11y::hit_test(self, p).or_else(|| unsafe { msg_send![super(self), accessibilityHitTest: p] })
    }
  }

  unsafe impl NSTextInputClient for View {
    /// Typed or committed text: typing (no range, nothing marked) as [Event::Text], an input
    /// method's commit as [Ime::Commit].
    #[unsafe(method(insertText:replacementRange:))]
    fn insert_text_replacing(&self, text: &AnyObject, range: NSRange) {
      let Some(s) = ns_text(text) else { return };
      match to_range(range) {
        None if !self.composing() => self.typed(s),
        replace => self.send_ime(Ime::Commit { text: s, replace }),
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

    #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
    fn set_marked_text(&self, text: &AnyObject, sel: NSRange, range: NSRange) {
      let Some(s) = ns_text(text) else { return };
      let n = utf16_len(&s);
      let sel = to_range(sel).map_or(n..n, |r| r.start.min(n)..r.end.min(n));
      self.send_ime(Ime::Mark { text: s, sel, replace: to_range(range) });
    }

    #[unsafe(method(unmarkText))]
    fn unmark_text(&self) {
      self.send_ime(Ime::Unmark);
    }

    #[unsafe(method(selectedRange))]
    fn selected_range(&self) -> NSRange {
      self.input(|t| ns_range(t.selected())).unwrap_or(NOT_FOUND)
    }

    #[unsafe(method(markedRange))]
    fn marked_range(&self) -> NSRange {
      self.input(|t| t.marked().map(ns_range)).flatten().unwrap_or(NOT_FOUND)
    }

    #[unsafe(method(hasMarkedText))]
    fn has_marked_text(&self) -> bool {
      self.composing()
    }

    #[unsafe(method_id(attributedSubstringForProposedRange:actualRange:))]
    fn attributed_substring(
      &self,
      range: NSRange,
      actual: NSRangePointer,
    ) -> Option<Retained<NSAttributedString>> {
      self.substring(range, actual)
    }

    #[unsafe(method_id(validAttributesForMarkedText))]
    fn valid_attributes(&self) -> Retained<NSArray<NSAttributedStringKey>> {
      NSArray::new()
    }

    /// Where the range's first line is on screen: the candidate window goes under it.
    #[unsafe(method(firstRectForCharacterRange:actualRange:))]
    fn first_rect(&self, range: NSRange, actual: NSRangePointer) -> NSRect {
      let r = to_range(range).unwrap_or(0..0);
      let Some((rect, got)) = self.input(|t| t.rect(r)).flatten() else { return NSRect::ZERO };
      write_range(actual, got);
      self.to_screen(rect)
    }

    #[unsafe(method(characterIndexForPoint:))]
    fn character_index(&self, p: NSPoint) -> NSUInteger {
      let p = self.window().map_or(p, |w| w.convertPointFromScreen(p));
      let p = self.convertPoint_fromView(p, None);
      let at = self.input(|t| t.index(Point::new(p.x as f32, p.y as f32))).flatten();
      at.unwrap_or(NSNotFound as NSUInteger)
    }
  }
);

impl View {
  /// The view of a window, or of an overlay at `overlay`: the frame's alpha shows through.
  pub fn new(
    mtm: MainThreadMarker,
    frame: NSRect,
    rt: Runtime<Box<dyn App>>,
    overlay: Option<Anchor>,
  ) -> Retained<Self> {
    let ivars = Ivars { overlay: Cell::new(overlay), ..Default::default() };
    let this = Self::alloc(mtm).set_ivars(ivars);
    let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
    let layer = CALayer::new();
    layer.setContentsGravity(unsafe { kCAGravityTopLeft });
    view.setLayer(Some(&layer));
    view.setWantsLayer(true);
    view.registerForDraggedTypes(&NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]));
    let mut presenter = Presenter::new(layer);
    presenter.trace = std::env::var_os("NIMBO_TRACE").is_some();
    presenter.renderer.transparent = overlay.is_some();

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
    let weak = view.weak();
    let wake = Wake::new(DispatchQueue::main(), move || {
      let mtm = MainThreadMarker::new().unwrap();
      if let Some(view) = weak.get(mtm).load() {
        view.timer_fired();
      }
    });
    *view.ivars().state.borrow_mut() = Some(State { rt, presenter, link, wake });
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

    a11y::close(self);
  }

  /// Runs `f` on the runtime unless a call up the stack has it, then redraws: an app changed
  /// from outside its window ([crate::Window::with]).
  pub fn try_runtime<R>(&self, f: impl FnOnce(&mut Runtime<Box<dyn App>>) -> R) -> Option<R> {
    let mut state = self.ivars().state.try_borrow_mut().ok()?;
    let st = state.as_mut()?;
    let out = f(&mut st.rt);
    st.rt.redraw();
    st.link.setPaused(false);
    Some(out)
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
      self.send(Event::Mods(Modifiers::empty()));
      self.send(Event::Focus(false));
    }
  }

  /// Feeds an event to the runtime and restarts the display link if it needs a frame.
  pub fn send(&self, ev: Event) -> bool {
    if self.busy() {
      // The runtime is busy up the stack (an app changed through `Window::with` fronts another
      // window, and AppKit tells this one it lost the keys): sent once it is done, in order.
      let mut waiting = self.ivars().waiting.borrow_mut();
      if waiting.is_empty() {
        let weak = self.weak();
        DispatchQueue::main().exec_async(move || {
          let mtm = MainThreadMarker::new().unwrap();
          if let Some(view) = weak.get(mtm).load() {
            view.flush();
          }
        });
      }

      waiting.push_back(ev);
      return false;
    }

    self.flush();
    self.deliver(ev)
  }

  /// Sends the events that waited, unless the runtime is still busy.
  fn flush(&self) {
    while self.ivars().state.try_borrow_mut().is_ok() {
      let Some(ev) = self.ivars().waiting.borrow_mut().pop_front() else { return };
      self.deliver(ev);
    }
  }

  fn deliver(&self, ev: Event) -> bool {
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
    self.sync_ime();
    if quit {
      super::shell::quit();
    }

    self.apply_resize();
    self.apply_file_dialog();
    self.show_menu();
    if self.ivars().rewake.take() {
      self.wake();
    }

    used
  }

  /// Shows the file panel the app asked for ([crate::Runtime::take_file_dialog]).
  fn apply_file_dialog(&self) {
    let d = match self.ivars().state.try_borrow_mut().as_deref_mut() {
      Ok(Some(st)) => st.rt.take_file_dialog(),
      _ => None,
    };

    if let (Some(d), Some(window)) = (d, self.window()) {
      super::files::show(self, &window, d);
    }
  }

  /// Pops up the context menu the app asked for ([Runtime::take_context_menu]) and sends the
  /// answer once it closes: frames go on meanwhile (the display link runs in every mode). The
  /// menu took the press's release: the app gets it after the menu.
  fn show_menu(&self) {
    let want = match self.ivars().state.try_borrow_mut().as_deref_mut() {
      Ok(Some(st)) => st.rt.take_context_menu(),
      _ => None,
    };

    let Some((on, at, menu)) = want else { return };
    // The press at the menu's point; one at another is no context for it.
    let press = self.ivars().press.take().filter(|e| {
      let p = self.location(e);
      (p.x - at.x).abs().max((p.y - at.y).abs()) < 1.0
    });
    // The window lives while its menu is open, even if the app closes it meanwhile.
    let _window = self.window();
    let item = super::app::menu::popup(self, press.as_deref(), at, &menu);
    // Any button the menu took the release of: the right one, the left of a Control-click.
    let held = self.ivars().held.get();
    let gone = held & !(NSEvent::pressedMouseButtons() as u64);
    if gone != 0 {
      let p = self.window().map_or(NSPoint::ZERO, |w| w.mouseLocationOutsideOfEventStream());
      let p = self.convertPoint_fromView(p, None);
      let (pos, mods) =
        (Point::new(p.x as f32, p.y as f32), flags_mods(NSEvent::modifierFlags_class()));
      self.ivars().held.set(held & !gone);
      for n in (0..64).filter(|n| gone >> n & 1 != 0) {
        self.send(Event::MouseUp { pos, button: button(n), mods });
      }
    }

    self.send(Event::Menu { on, item });
  }

  /// Resizes the window's content to the size the app asked for ([Runtime::take_resize]),
  /// keeping its top edge where it is, as preferences windows do when their tab changes, or an
  /// overlay at its anchor on its screen. The
  /// least content size comes down with it; a fixed window stays fixed for the user. Outside the
  /// runtime's borrow: the new frame size draws at once ([Self::render]).
  fn apply_resize(&self) {
    // Out of its window, the size waits for it: asked again, the same size would be nothing new.
    let Some(window) = self.window() else { return };
    let want = match self.ivars().state.try_borrow_mut().as_deref_mut() {
      Ok(Some(st)) => st.rt.take_resize(),
      _ => None,
    };

    let Some(size) = want else { return };
    let old = window.frame();
    let content = window.contentRectForFrameRect(old);
    let rect = NSRect::new(content.origin, NSSize::new(size.w as f64, size.h as f64));
    let mut frame = window.frameRectForContentRect(rect);
    frame.origin.y = old.origin.y + old.size.height - frame.size.height;
    let min = window.contentMinSize();
    let (w, h) = (min.width.min(size.w as f64), min.height.min(size.h as f64));
    window.setContentMinSize(NSSize::new(w, h));
    window.setFrame_display(frame, true);
    if let Some(anchor) = self.ivars().overlay.get() {
      overlay::place(&window, anchor, false);
    }
  }

  /// Lets `f` look at the focused text field, as an input method asks; `None` when no field
  /// that takes input methods has focus, or the runtime is busy further up the stack.
  fn input<R>(&self, f: impl FnOnce(&TextInput) -> R) -> Option<R> {
    let mut state = self.ivars().state.try_borrow_mut().ok()?;
    state.as_mut()?.rt.text_input().map(|t| f(&t))
  }

  fn substring(
    &self,
    range: NSRange,
    actual: NSRangePointer,
  ) -> Option<Retained<NSAttributedString>> {
    let (s, got) = self.input(|t| to_range(range).map(|r| t.substring(r))).flatten()?;
    write_range(actual, got);
    Some(NSAttributedString::from_nsstring(&NSString::from_str(&s)))
  }

  /// Whether an input method has marked text in the focused field.
  fn composing(&self) -> bool {
    let state = self.ivars().state.try_borrow();
    let st = state.as_ref().ok().and_then(|s| s.as_ref());
    st.and_then(|st| st.rt.ui.text_input()).is_some_and(|t| t.marked().is_some())
  }

  /// Whether the runtime is busy up the stack: an event sent now waits ([View::send]).
  pub(super) fn busy(&self) -> bool {
    self.ivars().state.try_borrow_mut().is_err()
  }

  /// Lets `f` look at the view's last frame, unless the runtime is busy up the stack.
  pub(super) fn with_ui<R>(&self, f: impl FnOnce(&UiState) -> R) -> Option<R> {
    Some(f(&self.ivars().state.try_borrow().ok()?.as_ref()?.rt.ui))
  }

  /// A rect of the view in screen coordinates (window coordinates without a window).
  pub(super) fn to_screen(&self, r: Rect) -> NSRect {
    let rect =
      NSRect::new(NSPoint::new(r.x as f64, r.y as f64), NSSize::new(r.w as f64, r.h as f64));
    let rect = self.convertRect_toView(rect, None);
    self.window().map_or(rect, |w| w.convertRectToScreen(rect))
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

  /// Asks for a frame; busy up the stack, once the runtime is free ([Ivars::rewake]).
  pub(super) fn wake(&self) {
    match self.ivars().state.try_borrow_mut().as_deref_mut() {
      Ok(Some(st)) => {
        st.rt.redraw();
        st.link.setPaused(false);
      }
      Ok(None) => {}
      Err(_) => self.ivars().rewake.set(true),
    }
  }

  fn weak(&self) -> WeakView {
    Arc::new(MainThreadBound::new(Weak::new(self), self.mtm()))
  }

  /// Sleeps until the runtime's next timer, unless an earlier wake is already set.
  fn schedule(&self, st: &mut State) {
    let Some(left) = st.rt.due_in() else { return };
    let since = self.ivars().last_frame.get().map(|t| st.rt.now().saturating_sub(t));
    let left = paced(left, since, refresh(&st.link));
    let at = st.rt.now() + left;
    if self.ivars().timer.get().is_some_and(|t| t <= at) {
      return;
    }

    self.ivars().timer.set(Some(at));
    st.wake.arm(left);
  }

  /// The timer wake's deadline came: its frame draws at once, without waking the display link
  /// (unpausing it for one frame cost ~100 wakeups a second at 10 Hz). Hidden, it waits for the
  /// window to show ([Self::occlusion_changed]).
  fn timer_fired(&self) {
    self.ivars().timer.set(None);
    if !self.visible() {
      return;
    }

    let due = {
      let Ok(mut state) = self.ivars().state.try_borrow_mut() else {
        self.ivars().rewake.set(true);
        return;
      };
      let Some(st) = state.as_mut() else { return };
      match st.rt.needs_frame() {
        true => st.link.isPaused(),
        false => {
          self.schedule(st);
          false
        }
      }
    };
    if due {
      self.render();
      self.after_frame();
    }
  }

  /// After a frame: the display link runs on while the runtime needs frames (animations, drags),
  /// else it stops at once, not a refresh later, and the view sleeps until the next timer.
  fn after_frame(&self) {
    {
      let Ok(mut state) = self.ivars().state.try_borrow_mut() else { return };
      let Some(st) = state.as_mut() else { return };
      match st.rt.needs_frame() {
        true => st.link.setPaused(false),
        false => {
          st.link.setPaused(true);
          self.schedule(st);
        }
      }
    }
    a11y::sync(self);
    self.sync_ime();
    self.apply_resize();
    self.apply_file_dialog();
    self.show_menu();
    if self.ivars().rewake.take() {
      self.wake();
    }
  }

  /// Draws the first frame before the window shows and takes the size the app asked for in it
  /// ([crate::Ui::fit_window_height]): a window fitted to its content opens at its size.
  pub fn prepare(&self) {
    self.render();
    self.after_frame();
  }

  /// Lets the input method's composition go when the field ended it by itself (a click, a
  /// command, the focus moved on, by an event or the view): the field kept it as typed.
  fn sync_ime(&self) {
    let (now, at) = {
      let Ok(state) = self.ivars().state.try_borrow() else { return };
      let ui = state.as_ref().map(|st| &st.rt.ui);
      let t = ui.and_then(|ui| ui.text_input().map(|t| (ui.focus(), t)));
      let at = t.as_ref().filter(|(_, t)| t.marked().is_some()).and_then(|(_, t)| t.caret_rect());
      (t.map_or((None, false), |(f, t)| (f, t.marked().is_some())), at)
    };
    let (iv, own) = (self.ivars(), self.ivars().ime_call.get());
    let (was, had) = (iv.ime_field.replace(now.0), iv.ime_marked.replace(now.1));
    let moved = was.is_some_and(|f| Some(f) != now.0);
    let shifted = at.is_some() && iv.ime_at.replace(at) != at;
    let cx: Option<Retained<NSTextInputContext>> = match moved || had && !now.1 && !own {
      true => unsafe { msg_send![super(self), inputContext] },
      false if shifted => self.inputContext(),
      false => None,
    };

    match (cx, shifted && !moved) {
      (Some(cx), true) => cx.invalidateCharacterCoordinates(),
      (Some(cx), false) => cx.discardMarkedText(),
      (None, _) => {}
    }
  }

  /// Sends an input method's own event: its end is no reason to discard its composition.
  fn send_ime(&self, ime: Ime) {
    self.ivars().ime_call.set(true);
    self.send(Event::Ime(ime));
    self.ivars().ime_call.set(false);
  }

  fn button(&self, event: &NSEvent, down: bool) {
    let (pos, mods) = (self.location(event), mods(event));
    let button = button(event.buttonNumber());
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

  /// A press where the app draws its title bar ([crate::Ui::drags_window]) moves the window; a
  /// double-click there does what System Settings asks of a title bar's
  /// (`AppleActionOnDoubleClick`): minimize, nothing, fill (macOS 15's private `_zoomFill:`, as
  /// Firefox calls it) or zoom, which a window of a fixed size can't. The app never sees the
  /// press.
  fn drag_window(&self, event: &NSEvent) -> bool {
    let pos = self.location(event);
    let drags =
      matches!(self.ivars().state.try_borrow().as_deref(), Ok(Some(st)) if st.rt.drags_window(pos));
    let Some(window) = self.window().filter(|_| drags) else { return false };
    let key = ns_string!("AppleActionOnDoubleClick");
    let action = NSUserDefaults::standardUserDefaults().stringForKey(key).map(|s| s.to_string());
    let fill = sel!(_zoomFill:);
    match (event.clickCount(), action.as_deref()) {
      (1, _) => window.performWindowDragWithEvent(event),
      (_, Some("Minimize")) => window.miniaturize(None),
      (_, Some("None")) => {}
      _ if !window.styleMask().contains(NSWindowStyleMask::Resizable) => {}
      (_, Some("Fill")) if window.respondsToSelector(fill) => unsafe {
        let _: () = msg_send![&window, _zoomFill: None::<&AnyObject>];
      },
      _ => window.zoom(None),
    }

    true
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
    // Busy up the stack (resized during a frame): drawn again once the runtime is free.
    let Ok(mut state) = self.ivars().state.try_borrow_mut() else {
      self.ivars().rewake.set(true);
      return;
    };
    let Some(st) = state.as_mut() else { return };
    self.ivars().last_frame.set(Some(st.rt.now()));
    // A wake set before this frame is let go: [Self::after_frame] sets the next, a refresh after
    // this one at the soonest.
    self.ivars().timer.set(None);
    st.wake.disarm();
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

/// How long a timer due in `left` waits, `since` the last frame started: until a refresh after
/// it at the soonest, as the display link would pace it (a timer every 2 ms draws at the
/// display's rate, not 500 times a second).
fn paced(left: Duration, since: Option<Duration>, refresh: Duration) -> Duration {
  since.map_or(left, |s| left.max(refresh.saturating_sub(s)))
}

/// The display's refresh interval from its link (a 120 Hz one's until it has run).
fn refresh(link: &CADisplayLink) -> Duration {
  let d = link.duration();
  let d = if d > 0.0 { d.clamp(1.0 / 120.0, 1.0 / 24.0) } else { 1.0 / 120.0 };
  Duration::from_secs_f64(d)
}

/// One timer on a queue, set again for each new deadline: it fires only at the last one, and
/// not at all once disarmed. `dispatch_after` blocks can't be taken back: each superseded one
/// would still wake the process when its time came.
struct Wake(DispatchRetained<DispatchSource>);

impl Wake {
  fn new(queue: &DispatchQueue, fired: impl Fn() + 'static) -> Self {
    let ty = (&raw const _dispatch_source_type_timer).cast_mut();
    // SAFETY: a timer source takes no handle or mask.
    let source = unsafe { DispatchSource::new(ty, 0, 0, Some(queue)) };
    let block = RcBlock::new(fired);
    // SAFETY: dispatch copies the block; it runs on `queue`, the thread `fired` was made for.
    unsafe { source.set_event_handler_with_block(RcBlock::as_ptr(&block)) };
    let wake = Self(source);
    wake.disarm();
    wake.0.activate();
    wake
  }

  /// Fires once, `after` from now (with dispatch_after's leeway: a tenth, within 1 ms and 60 s),
  /// instead of at any deadline set before.
  fn arm(&self, after: Duration) {
    let at = DispatchTime::try_from(after).unwrap_or(DispatchTime::NOW);
    let leeway = (after / 10).clamp(Duration::from_millis(1), Duration::from_secs(60));
    self.0.set_timer(at, DispatchTime::FOREVER.0, leeway.as_nanos() as u64);
  }

  fn disarm(&self) {
    self.0.set_timer(DispatchTime::FOREVER, DispatchTime::FOREVER.0, 0);
  }
}

impl Drop for Wake {
  fn drop(&mut self) {
    self.0.cancel();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn timer_frames_come_a_refresh_apart() {
    let ms = Duration::from_millis;
    let hz60 = Duration::from_micros(16_667);
    // Due in 2 ms, 1 ms after the last frame: at the next refresh.
    assert_eq!(paced(ms(2), Some(ms(1)), hz60), Duration::from_micros(15_667));
    // Due later than a refresh, or long after the last frame: when due.
    assert_eq!(paced(ms(100), Some(ms(1)), hz60), ms(100));
    assert_eq!(paced(ms(2), Some(ms(500)), hz60), ms(2));
    assert_eq!(paced(ms(2), None, hz60), ms(2));
  }

  /// A timer wake set again fires only at its last deadline, and not at all once disarmed: a
  /// superseded deadline wakes nothing.
  #[test]
  fn a_wake_set_again_fires_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let fired = Arc::new(AtomicUsize::new(0));
    let n = fired.clone();
    let queue = DispatchQueue::new("nimbo.wake", None);
    let wake = Wake::new(&queue, move || {
      n.fetch_add(1, Ordering::SeqCst);
    });
    let ms = Duration::from_millis;
    for t in [20, 40, 60] {
      wake.arm(ms(t));
    }

    std::thread::sleep(ms(250));
    assert_eq!(fired.load(Ordering::SeqCst), 1);

    wake.arm(ms(20));
    wake.disarm();
    std::thread::sleep(ms(100));
    assert_eq!(fired.load(Ordering::SeqCst), 1);
  }
}
