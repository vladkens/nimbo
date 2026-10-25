// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Input on the window: the mouse (moves, buttons with click counts, leaving through
//! TrackMouseEvent), the wheel and precision touchpads, keys ([keyboard]: a key nobody used
//! goes on to TranslateMessage, whose WM_CHAR is typed text, and to [keyboard::standard] for
//! its command), the IMM32 input method while a text field that takes it has focus (the
//! composition drawn in the field as marked text, the candidate window under the caret), and
//! cursors.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::Input::Ime::{
  CANDIDATEFORM, CFS_EXCLUDE, CFS_POINT, COMPOSITIONFORM, CPS_CANCEL, CPS_COMPLETE, GCS_COMPSTR,
  GCS_CURSORPOS, GCS_RESULTSTR, IACE_DEFAULT, IME_COMPOSITION_STRING, ISC_SHOWUICOMPOSITIONWINDOW,
  ImmAssociateContextEx, ImmGetCompositionStringW, ImmGetContext, ImmNotifyIME, ImmReleaseContext,
  ImmSetCandidateWindow, ImmSetCompositionWindow, NI_COMPOSITIONSTR,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
  GetDoubleClickTime, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
  VK_PROCESSKEY, VK_RETURN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::{Window, keyboard, with};
use crate::app::{Event, Ime, MouseButton, Phase, WHEEL_LINE};
use crate::keys::KeyEvent;
use crate::scene::{Point, Rect};
use crate::ui::Cursor;

/// `WM_MOUSELEAVE` (CommCtrl.h, in windows-sys' Controls).
const WM_MOUSELEAVE: u32 = 0x2A3;
/// One wheel notch.
const NOTCH: f32 = 120.0;
/// The wheel scrolls a screen per notch (WinUser.h, not in windows-sys).
const WHEEL_PAGESCROLL: u32 = u32::MAX;

/// The last press, to count clicks: its button, time (ms), position (pixels) and count.
#[derive(Default)]
pub(super) struct Clicks {
  button: Option<MouseButton>,
  time: u32,
  at: (i32, i32),
  count: u32,
}

/// Points a notch scrolls: the system's `lines` (or characters), or a `page` (the client's
/// extent) for "one screen at a time".
fn notch_step(lines: u32, page: f32) -> f32 {
  match lines {
    WHEEL_PAGESCROLL => page,
    n => n.min(100) as f32 * WHEEL_LINE,
  }
}

fn xy(lp: LPARAM) -> (i32, i32) {
  ((lp & 0xFFFF) as i16 as i32, ((lp >> 16) & 0xFFFF) as i16 as i32)
}

impl Window {
  pub(super) fn point(&self, (x, y): (i32, i32)) -> Point {
    Point::new(x as f32 / self.scale, y as f32 / self.scale)
  }

  /// Whether client pixel `(x, y)` is in the client area.
  fn inside(&self, (x, y): (i32, i32)) -> bool {
    let (w, h) = self.client();
    (0..w).contains(&x) && (0..h).contains(&y)
  }

  /// Feeds an event to the runtime, then shows its cursor and turns the input method on or
  /// off with the field's focus.
  pub(super) fn send(&mut self, ev: Event) -> bool {
    let used = self.rt.event(ev);
    self.sync_cursor(false);
    self.sync_ime();
    used
  }

  /// Sets the runtime's cursor when it changed and the pointer is in the window, or when
  /// `force`d (WM_SETCURSOR: the pointer is over the client area, maybe before its first move).
  pub(super) fn sync_cursor(&mut self, force: bool) {
    let c = self.rt.cursor();
    let changed = c != std::mem::replace(&mut self.cursor, c);
    if force || (changed && self.rt.mouse.is_some()) {
      let id = match c {
        Cursor::Inherit | Cursor::Arrow => IDC_ARROW,
        Cursor::Text => IDC_IBEAM,
        Cursor::Pointer | Cursor::Grab => IDC_HAND,
        Cursor::ResizeX => IDC_SIZEWE,
        Cursor::ResizeY => IDC_SIZENS,
        Cursor::Crosshair => IDC_CROSS,
        Cursor::Grabbing => IDC_SIZEALL,
        Cursor::NotAllowed => IDC_NO,
      };

      unsafe { SetCursor(LoadCursorW(null_mut(), id)) };
    }
  }

  // MARK: Mouse

  fn mouse_move(&mut self, lp: LPARAM) {
    if !self.tracking {
      let mut t = TRACKMOUSEEVENT {
        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: self.hwnd,
        dwHoverTime: 0,
      };
      self.tracking = unsafe { TrackMouseEvent(&mut t) } != 0;
    }

    let pos = self.point(xy(lp));
    self.send(Event::MouseMove { pos, mods: keyboard::mods() });
  }

  fn button(&mut self, button: MouseButton, down: bool, wp: WPARAM, lp: LPARAM) {
    let (at, mods) = (xy(lp), keyboard::mods());
    let pos = self.point(at);
    if !down {
      // MK_* buttons still held after this one went up.
      let last = wp & 0x73 == 0;
      self.held.retain(|&b| b != button);
      if last {
        self.held.clear();
        unsafe { ReleaseCapture() };
      }

      self.send(Event::MouseUp { pos, button, mods });
      // The last button of a drag that left the window: its leave may have come while captured.
      if last && !self.inside(at) {
        self.tracking = false;
        self.send(Event::MouseExit);
      }

      return;
    }

    unsafe { SetCapture(self.hwnd) };
    if !self.held.contains(&button) {
      self.held.push(button);
    }

    let time = unsafe { GetMessageTime() } as u32;
    let c = &mut self.clicks;
    let (dx, dy) = unsafe { (GetSystemMetrics(SM_CXDOUBLECLK), GetSystemMetrics(SM_CYDOUBLECLK)) };
    let near = (at.0 - c.at.0).abs() <= dx / 2 && (at.1 - c.at.1).abs() <= dy / 2;
    let again = c.button == Some(button)
      && time.wrapping_sub(c.time) <= unsafe { GetDoubleClickTime() }
      && near;
    *c = Clicks { button: Some(button), time, at, count: if again { c.count + 1 } else { 1 } };
    let clicks = c.count;
    self.send(Event::MouseDown { pos, button, clicks, mods });
  }

  /// The capture went to another window (another app took the mouse mid-drag): no button-up
  /// will come, so the buttons held are let go here, where the pointer is now, without a click.
  fn capture_lost(&mut self, to: HWND) {
    if to == self.hwnd || self.held.is_empty() {
      return;
    }

    let mut p = POINT { x: 0, y: 0 };
    unsafe {
      GetCursorPos(&mut p);
      ScreenToClient(self.hwnd, &mut p);
    }

    // Not the user's release: no click, though the app's drags end with it.
    self.rt.cancel_press();
    let (pos, mods) = (self.point((p.x, p.y)), keyboard::mods());
    for button in std::mem::take(&mut self.held) {
      self.send(Event::MouseUp { pos, button, mods });
    }

    if !self.inside((p.x, p.y)) {
      self.tracking = false;
      self.send(Event::MouseExit);
    }
  }

  /// A wheel or touchpad scroll: notches of 120 scroll the system's lines (or characters) per
  /// notch, or a screen; a precision touchpad sends fractions of one.
  fn wheel(&mut self, wp: WPARAM, lp: LPARAM, horizontal: bool) {
    let notches = (wp >> 16) as u16 as i16;
    let (x, y) = xy(lp);
    let mut p = POINT { x, y };
    unsafe { ScreenToClient(self.hwnd, &mut p) };
    let action = if horizontal { SPI_GETWHEELSCROLLCHARS } else { SPI_GETWHEELSCROLLLINES };
    let mut lines = 3u32;
    unsafe { SystemParametersInfoW(action, 0, (&raw mut lines).cast(), 0) };
    let (w, h) = self.client();
    let page = if horizontal { w } else { h } as f32 / self.scale;
    let k = notches as f32 / NOTCH * notch_step(lines, page);
    // Content follows the wheel: up moves it down; a tilt right moves it left.
    let delta = if horizontal { Point::new(-k, 0.0) } else { Point::new(0.0, k) };
    self.send(Event::Scroll {
      pos: self.point((p.x, p.y)),
      delta,
      precise: notches % NOTCH as i16 != 0,
      phase: Phase::None,
      momentum: Phase::None,
      mods: keyboard::mods(),
    });
  }

  // MARK: Keys

  /// A key press from the queue, before TranslateMessage: returns whether the app used it (so
  /// it types nothing). A modifier changes [Event::Mods]; while an input method composes, keys
  /// are its own.
  fn key_down(&mut self, wp: WPARAM, lp: LPARAM) -> bool {
    let vk = wp as u16;
    if vk == VK_PROCESSKEY {
      return false;
    }

    if keyboard::is_modifier(vk) {
      self.send(Event::Mods(keyboard::mods()));
      self.modifier(vk, lp, true);
      return false;
    }

    let k = key(vk, lp);
    self.send(Event::KeyDown(k))
  }

  /// A key nobody used: its command, as Windows' text controls bind it.
  fn unused_key(&mut self, wp: WPARAM, lp: LPARAM) {
    let vk = wp as u16;
    if vk == VK_PROCESSKEY || keyboard::is_modifier(vk) {
      return;
    }

    if let Some(cmd) = keyboard::standard(&key(vk, lp)) {
      self.send(Event::Command(cmd));
    }
  }

  fn key_up(&mut self, wp: WPARAM, lp: LPARAM) {
    let vk = wp as u16;
    match keyboard::is_modifier(vk) {
      true => {
        self.send(Event::Mods(keyboard::mods()));
        self.modifier(vk, lp, false);
      }
      false => _ = self.send(Event::KeyUp(KeyEvent { repeat: false, ..key(vk, lp) })),
    };
  }

  /// A modifier key going down (not its autorepeat) or up, by its side.
  fn modifier(&mut self, vk: u16, lp: LPARAM, down: bool) {
    let (scan, extended, repeat) =
      (((lp >> 16) & 0xFF) as u16, lp & (1 << 24) != 0, lp & (1 << 30) != 0);
    if let Some(key) = keyboard::mod_key(vk, scan, extended).filter(|_| !(down && repeat)) {
      self.send(Event::Modifier { key, down });
    }
  }

  /// A UTF-16 unit of typed text; a high surrogate waits for its low one.
  fn char(&mut self, unit: u16) {
    let units = match (self.high.take(), unit) {
      (_, 0xD800..=0xDBFF) => {
        self.high = Some(unit);
        return;
      }
      (Some(h), 0xDC00..=0xDFFF) => vec![h, unit],
      _ => vec![unit],
    };

    let s = String::from_utf16_lossy(&units);
    if crate::keys::printable(&s) {
      self.send(Event::Text(s));
    }
  }

  // MARK: Input method

  /// The input method only while a field that takes it has focus: without it keys stay plain.
  /// A composition the field ended by itself (the focus moved on, a command) is cancelled in
  /// the input method too: the field kept it as typed.
  pub(super) fn sync_ime(&mut self) {
    let (on, marked) = (self.rt.ui.text_input().and(self.rt.ui.focus()), self.composing());
    let ended = std::mem::replace(&mut self.ime_marked, marked) && !marked && !self.ime_call;
    // Its messages find the window busy and go.
    if on == self.ime {
      if ended {
        ime_notify(self.hwnd, CPS_CANCEL);
      }

      // The composition moved (the field scrolled): its windows follow.
      let at = self.rt.ui.text_input().filter(|_| marked).and_then(|t| t.caret_rect());
      if at.is_some() && at != self.ime_at {
        self.place_ime();
      }

      return;
    }

    if self.ime.is_some() {
      ime_notify(self.hwnd, CPS_CANCEL);
    }

    match (self.ime.is_some(), on.is_some()) {
      (false, true) => unsafe { _ = ImmAssociateContextEx(self.hwnd, null_mut(), IACE_DEFAULT) },
      (true, false) => ime_off(self.hwnd),
      _ => {}
    }

    self.ime = on;
  }

  /// Whether an input method has marked text in the focused field.
  fn composing(&self) -> bool {
    self.rt.ui.text_input().is_some_and(|t| t.marked().is_some())
  }

  /// Puts the composition and candidate windows at the caret, the candidates under it.
  fn place_ime(&mut self) {
    let Some(rect) = self.rt.text_input().and_then(|t| t.caret_rect()) else { return };
    self.ime_at = Some(rect);
    let s = self.scale;
    let px = |r: Rect| RECT {
      left: (r.x * s) as i32,
      top: (r.y * s) as i32,
      right: ((r.x + r.w.max(1.0)) * s) as i32,
      bottom: ((r.y + r.h) * s) as i32,
    };
    let area = px(rect);
    let at = POINT { x: area.left, y: area.bottom };
    let cx = unsafe { ImmGetContext(self.hwnd) };
    if cx.is_null() {
      return;
    }

    let comp = COMPOSITIONFORM { dwStyle: CFS_POINT, ptCurrentPos: at, rcArea: area };
    let cand = CANDIDATEFORM { dwIndex: 0, dwStyle: CFS_EXCLUDE, ptCurrentPos: at, rcArea: area };
    unsafe {
      ImmSetCompositionWindow(cx, &comp);
      ImmSetCandidateWindow(cx, &cand);
      ImmReleaseContext(self.hwnd, cx);
    }
  }

  /// The composition changed: committed text, then the text being composed as marked text with
  /// the input method's caret. Both in one message are an end and a new start.
  fn composition(&mut self, lp: LPARAM) {
    self.ime_call = true;
    self.compose(lp);
    self.ime_call = false;
    self.place_ime();
  }

  /// [Self::composition]'s work, its events the input method's own.
  fn compose(&mut self, lp: LPARAM) {
    let flags = lp as u32;
    let cx = unsafe { ImmGetContext(self.hwnd) };
    if cx.is_null() {
      return;
    }

    let get = |kind: IME_COMPOSITION_STRING| {
      let n = unsafe { ImmGetCompositionStringW(cx, kind, null_mut(), 0) };
      let mut buf = vec![0u16; n.max(0) as usize / 2];
      let bytes = (buf.len() * 2) as u32;
      unsafe { ImmGetCompositionStringW(cx, kind, buf.as_mut_ptr().cast(), bytes) };
      String::from_utf16_lossy(&buf)
    };
    let result = (flags & GCS_RESULTSTR != 0).then(|| get(GCS_RESULTSTR));
    // A move of the input method's caret alone comes without the string: read it again.
    let comp = (flags & (GCS_COMPSTR | GCS_CURSORPOS) != 0).then(|| {
      let text = get(GCS_COMPSTR);
      let n = text.encode_utf16().count();
      let at = match flags & GCS_CURSORPOS != 0 {
        true => unsafe { ImmGetCompositionStringW(cx, GCS_CURSORPOS, null_mut(), 0) },
        false => n as i32,
      };

      (text, (at.max(0) as usize).min(n))
    });
    unsafe { ImmReleaseContext(self.hwnd, cx) };
    if let Some(text) = result {
      self.send(Event::Ime(Ime::Commit { text, replace: None }));
    }

    if let Some((text, at)) = comp.filter(|(t, _)| !t.is_empty() || self.composing()) {
      self.send(Event::Ime(Ime::Mark { text, sel: at..at, replace: None }));
    }
  }

  /// Composing ended: marked text left over (a cancel) goes.
  fn end_composition(&mut self) {
    if self.composing() {
      self.ime_call = true;
      self.send(Event::Ime(Ime::Mark { text: String::new(), sel: 0..0, replace: None }));
      self.ime_call = false;
    }
  }
}

/// Tells the input method to complete or cancel its composition (`CPS_COMPLETE`, `CPS_CANCEL`).
/// Outside the window's borrow when what it sends back matters: its messages come at once.
fn ime_notify(hwnd: HWND, action: u32) {
  let cx = unsafe { ImmGetContext(hwnd) };
  if !cx.is_null() {
    unsafe {
      ImmNotifyIME(cx, NI_COMPOSITIONSTR, action, 0);
      ImmReleaseContext(hwnd, cx);
    }
  }
}

/// Whether the window's field has a composition under way.
fn composing() -> bool {
  with(|w| w.composing()).unwrap_or(false)
}

/// Takes the input method off the window: keys stay plain.
pub(super) fn ime_off(hwnd: HWND) {
  unsafe { ImmAssociateContextEx(hwnd, null_mut(), 0) };
}

fn key(vk: u16, lp: LPARAM) -> KeyEvent {
  let scan = ((lp >> 16) & 0xFF) as u16;
  let (extended, repeat) = (lp & (1 << 24) != 0, lp & (1 << 30) != 0);
  keyboard::key_event(vk, scan, extended, repeat)
}

/// Keys go to the app before TranslateMessage, so a key it used types nothing: returns true
/// when `msg` was handled here and must not go on.
pub(super) fn key_message(msg: &MSG) -> bool {
  let (wp, lp) = (msg.wParam, msg.lParam);
  match msg.message {
    // Enter that reaches us while a composition is pending only completes it; its result comes
    // back at once as WM_IME_COMPOSITION.
    WM_KEYDOWN if wp as u16 == VK_RETURN && composing() => {
      ime_notify(msg.hwnd, CPS_COMPLETE);
      true
    }
    WM_KEYDOWN | WM_SYSKEYDOWN => {
      if with(|w| w.key_down(wp, lp)).unwrap_or(false) {
        return true;
      }

      unsafe { TranslateMessage(msg) };
      with(|w| w.unused_key(wp, lp));
      unsafe { DispatchMessageW(msg) };
      true
    }
    WM_KEYUP | WM_SYSKEYUP => {
      with(|w| w.key_up(wp, lp));
      false
    }
    _ => false,
  }
}

/// The window's input messages; `None` for others.
pub(super) fn handle(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
  let button = |b| move |w: &mut Window, down| w.button(b, down, wp, lp);
  // A click ends a composition as typed; the input method lets go of it ([Window::sync_ime]).
  let downs = [WM_LBUTTONDOWN, WM_RBUTTONDOWN, WM_MBUTTONDOWN, WM_XBUTTONDOWN];
  if downs.contains(&msg) && composing() {
    with(|w| w.send(Event::Ime(Ime::Unmark)));
  }

  match msg {
    WM_MOUSEMOVE => _ = with(|w| w.mouse_move(lp)),
    WM_MOUSELEAVE => {
      _ = with(|w| {
        w.tracking = false;
        w.send(Event::MouseExit);
      })
    }
    WM_LBUTTONDOWN | WM_LBUTTONUP => {
      _ = with(|w| button(MouseButton::Left)(w, msg == WM_LBUTTONDOWN))
    }
    WM_RBUTTONDOWN | WM_RBUTTONUP => {
      _ = with(|w| button(MouseButton::Right)(w, msg == WM_RBUTTONDOWN))
    }
    WM_MBUTTONDOWN | WM_MBUTTONUP => {
      _ = with(|w| button(MouseButton::Middle)(w, msg == WM_MBUTTONDOWN))
    }
    WM_XBUTTONDOWN | WM_XBUTTONUP => {
      // XBUTTON1 and 2: AppKit's buttons 3 and 4.
      let b = MouseButton::Other(2 + ((wp >> 16) & 0xFFFF) as u8);
      _ = with(|w| button(b)(w, msg == WM_XBUTTONDOWN));
      return Some(1);
    }
    WM_MOUSEWHEEL | WM_MOUSEHWHEEL => _ = with(|w| w.wheel(wp, lp, msg == WM_MOUSEHWHEEL)),
    WM_CAPTURECHANGED => _ = with(|w| w.capture_lost(lp as HWND)),
    WM_SETCURSOR if (lp & 0xFFFF) as u32 == HTCLIENT => {
      return with(|w| w.sync_cursor(true)).map(|_| 1);
    }
    WM_CHAR => _ = with(|w| w.char(wp as u16)),
    // Alt with a key: no menu to open, no beep; Alt-Space still opens the window menu.
    WM_SYSCHAR if wp != ' ' as usize => {}
    WM_SYSCOMMAND if wp & 0xFFF0 == SC_KEYMENU as usize && lp == 0 => {}
    WM_IME_SETCONTEXT => {
      // The composition shows in the field, not in the input method's window.
      let lp = lp & !(ISC_SHOWUICOMPOSITIONWINDOW as LPARAM);
      return Some(unsafe { DefWindowProcW(hwnd, msg, wp, lp) });
    }
    WM_IME_STARTCOMPOSITION => _ = with(|w| w.place_ime()),
    WM_IME_COMPOSITION => _ = with(|w| w.composition(lp)),
    WM_IME_ENDCOMPOSITION => _ = with(|w| w.end_composition()),
    _ => return None,
  }

  Some(0)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_notch_scrolls_lines_or_a_page() {
    assert_eq!(notch_step(3, 400.0), 3.0 * WHEEL_LINE);
    assert_eq!(notch_step(500, 400.0), 100.0 * WHEEL_LINE);
    assert_eq!(notch_step(WHEEL_PAGESCROLL, 400.0), 400.0);
  }
}
