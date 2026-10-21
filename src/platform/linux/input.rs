// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! GDK's input events on the area into [Event]s: the pointer (moves, buttons with click
//! counts, the exit, a broken grab), scrolling (a touchpad's smooth deltas with phases, a
//! wheel's in lines), a touchpad's pinch (Wayland only, in GTK 3), focus and keys ([keys]: their
//! repeats are presses of a key already down, the Latin key of a shortcut from GDK's keymap). A
//! key nobody used goes through a GtkIMContextSimple, GTK's dead keys and compose table, into
//! [Event::Text], or through the PC editing keys into [Event::Command]; an input method's context
//! comes with text fields.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::{CStr, c_char, c_int, c_uint};
use std::ptr::null_mut;
use std::sync::OnceLock;

use super::dl::fns;
use super::keys::{self, kvk};
use super::{Fp, P, TRUE, Win, busy, connect, lib, with};
use crate::app::{Event, Mods, MouseButton, Phase, WHEEL_LINE};
use crate::scene::Point;

/// The events the area asks for (GdkEventMask): motion, buttons, keys, crossing, focus,
/// scrolling (smooth too) and touchpad gestures.
pub const EVENTS: c_int = 1 << 2
  | 1 << 8
  | 1 << 9
  | 1 << 10
  | 1 << 11
  | 1 << 12
  | 1 << 13
  | 1 << 14
  | 1 << 21
  | 1 << 23
  | 1 << 24;

// GdkEventType.
const MOTION: c_int = 3;
const BUTTON_PRESS: c_int = 4;
const BUTTON_2PRESS: c_int = 5;
const BUTTON_3PRESS: c_int = 6;
const BUTTON_RELEASE: c_int = 7;
const KEY_PRESS: c_int = 8;
const KEY_RELEASE: c_int = 9;
const ENTER: c_int = 10;
const LEAVE: c_int = 11;
const FOCUS: c_int = 12;
const SCROLL: c_int = 31;
const GRAB_BROKEN: c_int = 35;
const PINCH: c_int = 42;
/// `GDK_NOTIFY_INFERIOR`: a crossing into a child window, still ours.
const INFERIOR: c_int = 2;
/// `GDK_SOURCE_TOUCHPAD`, `GDK_SOURCE_TRACKPOINT`.
const TOUCHPAD: c_int = 6;
const TRACKPOINT: c_int = 7;
/// Points per unit of a touchpad's smooth delta: GDK's Wayland backend gives the compositor's
/// pixels over 10 (so a wheel's click is 1.5 under sway, which sends libinput's 15, as in every
/// GTK 3 app there).
const PRECISE: f32 = 10.0;

/// The longest gap between clicks that counts them as one double (triple) click, in
/// milliseconds, and how far apart they may be: GTK's defaults (400 ms, 5 px).
const CLICK_TIME: u32 = 400;
const CLICK_DIST: f32 = 5.0;

/// `GdkKeymapKey`.
#[repr(C)]
pub struct KeymapKey {
  keycode: c_uint,
  group: c_int,
  level: c_int,
}

/// `GdkEventCrossing`, up to its detail.
#[repr(C)]
struct Crossing {
  kind: c_int,
  window: P,
  send_event: i8,
  subwindow: P,
  time: u32,
  x: f64,
  y: f64,
  x_root: f64,
  y_root: f64,
  mode: c_int,
  detail: c_int,
}

/// `GdkEventFocus`.
#[repr(C)]
struct Focus {
  kind: c_int,
  window: P,
  send_event: i8,
  focus_in: i16,
}

/// `GdkEventTouchpadPinch`, up to its scale.
#[repr(C)]
struct Pinch {
  kind: c_int,
  window: P,
  send_event: i8,
  phase: i8,
  fingers: i8,
  time: u32,
  x: f64,
  y: f64,
  dx: f64,
  dy: f64,
  angle: f64,
  scale: f64,
}

fns!(Gdk {
  fn gdk_event_get_event_type(ev: P) -> c_int;
  fn gdk_event_get_coords(ev: P, x: *mut f64, y: *mut f64) -> c_int;
  fn gdk_event_get_button(ev: P, button: *mut c_uint) -> c_int;
  fn gdk_event_get_state(ev: P, state: *mut c_uint) -> c_int;
  fn gdk_event_get_time(ev: P) -> u32;
  fn gdk_event_get_keyval(ev: P, keyval: *mut c_uint) -> c_int;
  fn gdk_event_get_keycode(ev: P, code: *mut u16) -> c_int;
  fn gdk_event_get_scroll_direction(ev: P, dir: *mut c_int) -> c_int;
  fn gdk_event_get_scroll_deltas(ev: P, x: *mut f64, y: *mut f64) -> c_int;
  fn gdk_event_is_scroll_stop_event(ev: P) -> c_int;
  fn gdk_event_get_source_device(ev: P) -> P;
  fn gdk_device_get_source(device: P) -> c_int;
  fn gdk_event_copy(ev: P) -> P;
  fn gdk_event_free(ev: P);
  fn gdk_keyval_to_unicode(keyval: c_uint) -> u32;
  fn gdk_display_get_default() -> P;
  fn gdk_keymap_get_for_display(display: P) -> P;
  fn gdk_keymap_get_modifier_state(keymap: P) -> c_uint;
  fn gdk_keymap_get_entries_for_keycode(keymap: P, code: c_uint, keys: *mut *mut KeymapKey,
    keyvals: *mut *mut c_uint, n: *mut c_int) -> c_int;
  fn gtk_im_context_simple_new() -> P;
  fn gtk_im_context_set_client_window(im: P, window: P);
  fn gtk_im_context_filter_keypress(im: P, ev: P) -> c_int;
  fn gtk_im_context_focus_in(im: P);
  fn gtk_im_context_focus_out(im: P);
  fn gtk_im_context_reset(im: P);
  fn g_free(p: P);
  fn g_object_unref(obj: P);
});

fn gdk() -> &'static Gdk {
  static GDK: OnceLock<Option<Gdk>> = OnceLock::new();
  GDK.get_or_init(|| Gdk::load(&lib()?)).as_ref().expect("GTK 3 has GDK's functions")
}

thread_local! {
  /// Events that came while the window was busy, copied, to run after it in order; set while
  /// they run, so a callback among them adds to the queue rather than running it again.
  static STASH: RefCell<VecDeque<P>> = const { RefCell::new(VecDeque::new()) };
  static REPLAYING: Cell<bool> = const { Cell::new(false) };
  /// Text the input context committed during a key press.
  static TYPED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The last press: its button, the event's time (milliseconds, wrapping), place and click count.
type Click = (MouseButton, u32, Point, u32);

/// The pointer and keyboard as the events left them.
pub struct Input {
  mods: Mods,
  mouse: Point,
  /// The buttons down, and whether the pointer left meanwhile: it exits as the last goes up.
  buttons: Vec<MouseButton>,
  outside: bool,
  last_click: Option<Click>,
  /// A touchpad scroll is under way.
  scrolling: bool,
  /// A pinch's scale since it began, at the last update.
  pinch: f64,
  /// The keys down (hardware code and keyval): a press of one is a repeat.
  held: Vec<(u16, u32)>,
  /// Dead keys and compose sequences ([TYPED]).
  im: P,
  /// The window has the keyboard focus: the keymap's modifiers are ours.
  focused: bool,
}

impl Input {
  pub fn new() -> Self {
    let g = gdk();
    let im = unsafe { (g.gtk_im_context_simple_new)() };
    unsafe {
      connect(im, c"commit", on_commit as Fp, 0);
      connect(keymap(), c"state-changed", on_keymap as Fp, 0);
    }

    Self {
      mods: Mods::NONE,
      mouse: Point::new(0.0, 0.0),
      buttons: Vec::new(),
      outside: false,
      last_click: None,
      scrolling: false,
      pinch: 1.0,
      held: Vec::new(),
      im,
      focused: false,
    }
  }

  /// The area has its GdkWindow, which the input context works for.
  pub fn realized(&mut self, window: P) {
    unsafe { (gdk().gtk_im_context_set_client_window)(self.im, window) };
  }
}

/// The area's `event` signal: every input event, before GTK's own handlers. While the window
/// is busy, a copy waits for [replay].
pub unsafe extern "C" fn on_event(_: P, ev: P, _: P) -> c_int {
  match with(None, |w| Some(w.event(ev))) {
    Some(used) => used as c_int,
    None if busy() => {
      STASH.with_borrow_mut(|s| s.push_back(unsafe { (gdk().gdk_event_copy)(ev) }));
      TRUE
    }
    None => 0,
  }
}

impl Drop for Input {
  fn drop(&mut self) {
    unsafe { (gdk().g_object_unref)(self.im) };
  }
}

/// Runs the events that waited for the window, oldest first, unless they already run further
/// up the stack.
pub fn replay() {
  if REPLAYING.replace(true) {
    return;
  }

  while let Some(ev) = STASH.with_borrow_mut(|s| s.pop_front()) {
    unsafe {
      on_event(null_mut(), ev, null_mut());
      (gdk().gdk_event_free)(ev);
    }
  }

  REPLAYING.set(false);
}

/// The default display's keymap.
fn keymap() -> P {
  let g = gdk();
  unsafe { (g.gdk_keymap_get_for_display)((g.gdk_display_get_default)()) }
}

/// The keyboard's modifiers changed (after a key, or as the window takes the focus while some
/// are held): they are the window's while it has the focus.
unsafe extern "C" fn on_keymap(_: P, _: P) {
  with((), |w| {
    if w.input.focused {
      let state = unsafe { (gdk().gdk_keymap_get_modifier_state)(keymap()) };
      w.mods(keys::mods(state));
    }
  });
}

unsafe extern "C" fn on_commit(_: P, s: *const c_char, _: P) {
  let text = unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned();
  TYPED.with_borrow_mut(|t| t.push(text));
}

impl Win {
  /// One GDK event; whether it was one of ours.
  fn event(&mut self, ev: P) -> bool {
    let g = gdk();
    let (mut x, mut y, mut state) = (0.0, 0.0, 0);
    let kind = unsafe {
      (g.gdk_event_get_coords)(ev, &mut x, &mut y);
      (g.gdk_event_get_state)(ev, &mut state);
      (g.gdk_event_get_event_type)(ev)
    };
    let (pos, mods) = (Point::new(x as f32, y as f32), keys::mods(state));
    match kind {
      MOTION | ENTER => {
        self.input.mouse = pos;
        self.input.outside &= kind == MOTION;
        self.send(Event::MouseMove { pos, mods });
      }
      LEAVE => {
        // SAFETY: a crossing event.
        if unsafe { &*ev.cast::<Crossing>() }.detail != INFERIOR {
          self.input.outside = true;
          if self.input.buttons.is_empty() {
            self.send(Event::MouseExit);
          }
        }
      }
      BUTTON_PRESS | BUTTON_RELEASE => self.button(ev, kind == BUTTON_PRESS, pos, mods),
      // GTK's own double and triple clicks come after the presses, which count them here.
      BUTTON_2PRESS | BUTTON_3PRESS => {}
      GRAB_BROKEN => self.let_go(),
      SCROLL => self.scroll(ev, pos, mods),
      // SAFETY: a pinch event.
      PINCH => self.pinch(unsafe { &*ev.cast::<Pinch>() }, mods),
      KEY_PRESS | KEY_RELEASE => self.key(ev, kind == KEY_PRESS, state),
      // SAFETY: a focus event.
      FOCUS => self.focus(unsafe { &*ev.cast::<Focus>() }.focus_in != 0),
      _ => return false,
    }

    true
  }

  fn button(&mut self, ev: P, down: bool, pos: Point, mods: Mods) {
    let mut b = 0;
    unsafe { (gdk().gdk_event_get_button)(ev, &mut b) };
    // 8 and 9 are back and forward, AppKit's buttons 3 and 4.
    let button = match b {
      1 => MouseButton::Left,
      2 => MouseButton::Middle,
      3 => MouseButton::Right,
      8 | 9 => MouseButton::Other(b as u8 - 5),
      b => MouseButton::Other(b.min(255) as u8),
    };

    let i = &mut self.input;
    i.buttons.retain(|&x| x != button);
    if !down {
      let exit = i.outside && i.buttons.is_empty();
      self.send(Event::MouseUp { pos, button, mods });
      if exit {
        self.send(Event::MouseExit);
      }

      return;
    }

    i.buttons.push(button);
    let time = unsafe { (gdk().gdk_event_get_time)(ev) };
    let clicks = clicks(i.last_click, button, time, pos);
    i.last_click = Some((button, time, pos, clicks));
    self.send(Event::MouseDown { pos, button, clicks, mods });
  }

  /// Another client took the pointer while buttons were held: the press ends without a click,
  /// the buttons go up, and the pointer exits if it was out.
  fn let_go(&mut self) {
    let (pos, mods) = (self.input.mouse, self.input.mods);
    let held = std::mem::take(&mut self.input.buttons);
    if !held.is_empty() {
      self.rt.cancel_press();
    }

    for button in held {
      self.send(Event::MouseUp { pos, button, mods });
    }

    if self.input.outside {
      self.send(Event::MouseExit);
    }
  }

  fn scroll(&mut self, ev: P, pos: Point, mods: Mods) {
    let g = gdk();
    let (mut dir, mut dx, mut dy) = (0, 0.0, 0.0);
    // Smooth (GDK's unit is a wheel's click), else a click up, down, left or right.
    let precise = match unsafe { (g.gdk_event_get_scroll_deltas)(ev, &mut dx, &mut dy) } {
      0 => {
        unsafe { (g.gdk_event_get_scroll_direction)(ev, &mut dir) };
        (dx, dy) = [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)][dir.clamp(0, 3) as usize];
        false
      }
      _ => unsafe {
        let source = (g.gdk_device_get_source)((g.gdk_event_get_source_device)(ev));
        matches!(source, TOUCHPAD | TRACKPOINT)
      },
    };

    let stop = unsafe { (g.gdk_event_is_scroll_stop_event)(ev) } != 0;
    let phase = phase(&mut self.input.scrolling, precise, stop);
    let unit = if precise { PRECISE } else { WHEEL_LINE };
    let delta = Point::new(-dx as f32 * unit, -dy as f32 * unit);
    self.send(Event::Scroll { pos, delta, precise, phase, momentum: Phase::None, mods });
  }

  fn pinch(&mut self, p: &Pinch, mods: Mods) {
    let pos = Point::new(p.x as f32, p.y as f32);
    // GdkTouchpadGesturePhase: begin, update, end, cancel. The scale is the gesture's since it
    // began; the delta is the change since the last update, as a fraction of it (×1.5 then ×2
    // is +50%, then +33%).
    let (delta, phase) = match p.phase {
      0 => {
        self.input.pinch = 1.0;
        (0.0, Phase::Began)
      }
      1 => {
        let last = std::mem::replace(&mut self.input.pinch, p.scale);
        let d = if last > 0.0 { p.scale / last - 1.0 } else { 0.0 };
        (d as f32, Phase::Changed)
      }
      2 => (0.0, Phase::Ended),
      _ => (0.0, Phase::Cancelled),
    };

    self.send(Event::Magnify { pos, delta, phase, mods });
  }

  /// The window took or lost the focus: taking it, the modifiers held now are ours; losing it,
  /// no key or modifier stays down.
  fn focus(&mut self, on: bool) {
    let (g, im) = (gdk(), self.input.im);
    self.input.focused = on;
    let state = unsafe { (g.gdk_keymap_get_modifier_state)(keymap()) };
    match on {
      true => unsafe { (g.gtk_im_context_focus_in)(im) },
      false => unsafe {
        self.input.held.clear();
        (g.gtk_im_context_reset)(im);
        (g.gtk_im_context_focus_out)(im);
      },
    }

    self.mods(if on { keys::mods(state) } else { Mods::NONE });
    self.send(Event::Focus(on));
  }

  /// Sends the modifiers when they changed.
  fn mods(&mut self, m: Mods) {
    if m != self.input.mods {
      self.input.mods = m;
      self.send(Event::Mods(m));
    }
  }

  /// A key down or up, with the modifiers before it in `state`. A modifier key changes them
  /// ([Event::Mods]) and, by its side, sends [Event::Modifier]; another goes to the app, then,
  /// unused, through the input context's compose into text, else the editing keys' commands.
  fn key(&mut self, ev: P, down: bool, state: c_uint) {
    let g = gdk();
    let (mut sym, mut hw) = (0, 0u16);
    unsafe {
      (g.gdk_event_get_keyval)(ev, &mut sym);
      (g.gdk_event_get_keycode)(ev, &mut hw);
    }

    let held = &mut self.input.held;
    let repeat = down && held.iter().any(|&(h, _)| h == hw);
    held.retain(|&(h, _)| h != hw);
    if down {
      held.push((hw, sym));
    }

    if let Some(side) = keys::modifier(sym) {
      let syms: Vec<u32> = held.iter().map(|&(_, s)| s).collect();
      self.mods(keys::after(keys::mods(state), sym, down, &syms));
      if let Some(key) = side.filter(|_| !repeat) {
        self.send(Event::Modifier { key, down });
      }

      return;
    }

    let mods = keys::mods(state);
    self.mods(mods);
    let code = kvk(hw.saturating_sub(8) as u32).unwrap_or(0xffff);
    let ch = char::from_u32(unsafe { (g.gdk_keyval_to_unicode)(sym) }).filter(|&c| c != '\0');
    let k = keys::key_event(sym, ch, code, mods, repeat, || latin(hw, mods.shift));
    if !down {
      self.send(Event::KeyUp(k));
      return;
    }

    if self.send(Event::KeyDown(k)) {
      return;
    }

    // A shortcut types nothing and ends a compose sequence: GTK's context leaves Super out.
    if keys::types(mods) {
      TYPED.take();
      if unsafe { (g.gtk_im_context_filter_keypress)(self.input.im, ev) } != 0 {
        for text in TYPED.take().into_iter().filter(|t| crate::keys::printable(t)) {
          self.send(Event::Text(text));
        }

        return;
      }
    } else {
      unsafe { (g.gtk_im_context_reset)(self.input.im) };
    }

    if let Some(c) = crate::keys::pc(&k) {
      self.send(Event::Command(c));
    }
  }
}

/// The ASCII character hardware key `hw` types on the first layout that has one, at the Shift
/// level or not: shortcuts on a Russian layout match their Latin keys.
fn latin(hw: u16, shift: bool) -> Option<char> {
  let g = gdk();
  let (mut entries, mut vals, mut n) = (null_mut(), null_mut(), 0);
  let ok = unsafe {
    (g.gdk_keymap_get_entries_for_keycode)(keymap(), hw as c_uint, &mut entries, &mut vals, &mut n)
  };
  if ok == 0 || n <= 0 {
    return None;
  }

  // SAFETY: GDK's two arrays of `n`, ours to free.
  let found = unsafe {
    let keys = std::slice::from_raw_parts(entries, n as usize);
    let vals = std::slice::from_raw_parts(vals, n as usize);
    keys.iter().zip(vals).find_map(|(k, &v)| {
      let c = char::from_u32((g.gdk_keyval_to_unicode)(v))?;
      (k.level == shift as c_int && c.is_ascii_graphic()).then_some(c)
    })
  };
  unsafe {
    (g.g_free)(entries.cast());
    (g.g_free)(vals.cast());
  }

  found
}

/// A press's click count after the `last` one: one more for the same button within
/// [CLICK_TIME] and [CLICK_DIST], else 1. By the events' own times, not when they are handled:
/// presses queued behind a slow paste don't make a double click.
fn clicks(last: Option<Click>, button: MouseButton, time: u32, pos: Point) -> u32 {
  match last {
    Some((b, t, p, n))
      if b == button
        && time.wrapping_sub(t) < CLICK_TIME
        && (p.x - pos.x).abs() < CLICK_DIST
        && (p.y - pos.y).abs() < CLICK_DIST =>
    {
      n + 1
    }
    _ => 1,
  }
}

/// A scroll's phase: a touchpad's begins, changes and ends with its stop event; a wheel's has
/// none.
fn phase(scrolling: &mut bool, precise: bool, stop: bool) -> Phase {
  let was = std::mem::replace(scrolling, precise && !stop);
  match (precise, was, stop) {
    (false, ..) => Phase::None,
    (true, true, true) => Phase::Ended,
    (true, false, true) => Phase::None,
    (true, false, false) => Phase::Began,
    (true, true, false) => Phase::Changed,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Presses 600 ms apart are two clicks even when handled together; a second within 400 ms is
  /// a double click, across the clock's wrap too.
  #[test]
  fn clicks_count_by_the_events_time() {
    let (left, at) = (MouseButton::Left, Point::new(10.0, 10.0));
    let first = Some((left, 1000, at, 1));
    assert_eq!(clicks(first, left, 1600, at), 1);
    assert_eq!(clicks(first, left, 1300, at), 2);
    assert_eq!(clicks(Some((left, u32::MAX - 100, at, 2)), left, 100, at), 3);
    assert_eq!(clicks(first, MouseButton::Right, 1300, at), 1);
    assert_eq!(clicks(first, left, 1300, Point::new(20.0, 10.0)), 1);
  }

  #[test]
  fn a_touchpad_scroll_ends_with_its_stop() {
    let mut on = false;
    assert_eq!(phase(&mut on, true, false), Phase::Began);
    assert_eq!(phase(&mut on, true, false), Phase::Changed);
    assert_eq!(phase(&mut on, true, true), Phase::Ended);
    assert!(!on);
    assert_eq!(phase(&mut on, true, true), Phase::None, "a stop alone");
    // A wheel: no phases, and it ends a touchpad scroll.
    assert_eq!(phase(&mut on, true, false), Phase::Began);
    assert_eq!(phase(&mut on, false, false), Phase::None);
    assert!(!on);
  }
}
