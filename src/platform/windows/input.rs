// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Input on the window: the mouse (moves, buttons with click counts, leaving through
//! TrackMouseEvent), the wheel and precision touchpads, keys ([keyboard]: a key nobody used
//! goes on to TranslateMessage, whose WM_CHAR is typed text, and to [keyboard::standard] for
//! its command), and cursors.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
  GetDoubleClickTime, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
  VK_PROCESSKEY,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::{Window, keyboard, with};
use crate::app::{Event, MouseButton, Phase, WHEEL_LINE};
use crate::keys::KeyEvent;
use crate::scene::Point;
use crate::ui::Cursor;

/// `WM_MOUSELEAVE` (CommCtrl.h, in windows-sys' Controls).
const WM_MOUSELEAVE: u32 = 0x2A3;
/// One wheel notch.
const NOTCH: f32 = 120.0;

/// The last press, to count clicks: its button, time (ms), position (pixels) and count.
#[derive(Default)]
pub(super) struct Clicks {
  button: Option<MouseButton>,
  time: u32,
  at: (i32, i32),
  count: u32,
}

fn xy(lp: LPARAM) -> (i32, i32) {
  ((lp & 0xFFFF) as i16 as i32, ((lp >> 16) & 0xFFFF) as i16 as i32)
}

impl Window {
  fn point(&self, (x, y): (i32, i32)) -> Point {
    Point::new(x as f32 / self.scale, y as f32 / self.scale)
  }

  /// Whether client pixel `(x, y)` is in the client area.
  fn inside(&self, (x, y): (i32, i32)) -> bool {
    let (w, h) = self.client();
    (0..w).contains(&x) && (0..h).contains(&y)
  }

  /// Feeds an event to the runtime, then shows its cursor.
  pub(super) fn send(&mut self, ev: Event) -> bool {
    let used = self.rt.event(ev);
    self.sync_cursor(false);
    used
  }

  /// Sets the runtime's cursor when it changed and the pointer is in the window, or when
  /// `force`d (WM_SETCURSOR: the pointer is over the client area, maybe before its first move).
  fn sync_cursor(&mut self, force: bool) {
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
      if last {
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

  /// A wheel or touchpad scroll: notches of 120 scroll the system's lines (or characters) per
  /// notch; a precision touchpad sends fractions of one.
  fn wheel(&mut self, wp: WPARAM, lp: LPARAM, horizontal: bool) {
    let notches = (wp >> 16) as u16 as i16;
    let (x, y) = xy(lp);
    let mut p = POINT { x, y };
    unsafe { ScreenToClient(self.hwnd, &mut p) };
    let action = if horizontal { SPI_GETWHEELSCROLLCHARS } else { SPI_GETWHEELSCROLLLINES };
    let mut lines = 3u32;
    unsafe { SystemParametersInfoW(action, 0, (&raw mut lines).cast(), 0) };
    let k = notches as f32 / NOTCH * lines.min(100) as f32 * WHEEL_LINE;
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
pub(super) fn handle(msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
  let button = |b| move |w: &mut Window, down| w.button(b, down, wp, lp);
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
    WM_SETCURSOR if (lp & 0xFFFF) as u32 == HTCLIENT => {
      return with(|w| w.sync_cursor(true)).map(|_| 1);
    }
    WM_CHAR => _ = with(|w| w.char(wp as u16)),
    // Alt with a key: no menu to open, no beep; Alt-Space still opens the window menu.
    WM_SYSCHAR if wp != ' ' as usize => {}
    WM_SYSCOMMAND if wp & 0xFFF0 == SC_KEYMENU as usize && lp == 0 => {}
    _ => return None,
  }

  Some(0)
}
