// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Windows platform: Win32 windows, per-monitor DPI aware (v2), each one's frames drawn by
//! the CPU renderer into a DIB section and copied to the window with BitBlt, only the tiles that
//! changed ([Dib]). The message loop sleeps in MsgWaitForMultipleObjectsEx until input, a wake
//! (the waker posts a message) or a high-resolution waitable timer: the earliest of the windows'
//! next timers, or while one animates its next frame slot at the refresh rate of the display it
//! is on, followed as it moves to another or the mode changes. No frames while a window is
//! minimized; one when it shows again. The windows are kept by handle; a message's handler finds
//! its own through [CURRENT] ([with]). Input and cursors are in [input], keys in [keyboard],
//! light or dark and the accent in [settings], the app shell, tray, hotkeys and launch at login
//! in [app], file dialogs and drops in [files]; then the clipboard, the performance cores, and
//! the DirectWrite text backend ([text]).

mod app;
mod files;
mod input;
mod keyboard;
mod settings;
pub mod text;

use std::cell::{Cell, RefCell};
use std::mem::offset_of;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use app::paste::Open;
pub use app::{autostart, focus, hotkey, open, paste, post, shell, tray};
use keyboard_types::Modifiers;
pub use settings::appearance;
use windows_sys::Win32::Foundation::{
  CloseHandle, FILETIME, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::DataExchange::EmptyClipboard;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Ole::RevokeDragDrop;
use windows_sys::Win32::System::SystemInformation::{
  GetLogicalProcessorInformationEx, GetSystemTimePreciseAsFileTime, PROCESSOR_RELATIONSHIP,
  RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};
use windows_sys::Win32::System::Threading::{
  CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, GetCurrentProcess,
  GetProcessTimes, INFINITE, SetWaitableTimer, TIMER_ALL_ACCESS,
};
use windows_sys::Win32::UI::HiDpi::{
  AdjustWindowRectExForDpi, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor,
  GetDpiForSystem, GetDpiForWindow, MDT_EFFECTIVE_DPI, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::app::{App, Appearance, Clipboard, Clock, Event, MouseButton, Runtime};
use crate::cpu::Renderer;
use crate::cpu::raster::IRect;
use crate::platform::{Pace, points};
use crate::scene::Size;
use crate::ui::Cursor;
use crate::{Anchor, Config};

/// Posted by the waker: something changed on another thread.
const WM_WAKE: u32 = WM_APP + 1;
/// Frames during a live resize or move, or while a menu is open: modal loops the message loop
/// doesn't run.
const TIMER_MODAL: usize = 1;
/// `CF_UNICODETEXT`.
const CF_TEXT_UTF16: u32 = 13;

/// `s` as a NUL-terminated UTF-16 string, for the `W` calls.
fn wide(s: &str) -> Vec<u16> {
  s.encode_utf16().chain([0]).collect()
}

// MARK: Present

/// A top-down 32-bit DIB section the CPU renderer draws into, selected into a memory DC to copy
/// from; it keeps the tile hashes of the frame it holds.
struct Dib {
  dc: HDC,
  bmp: HBITMAP,
  bits: *mut u32,
  w: usize,
  h: usize,
  tiles: Vec<u64>,
}

impl Dib {
  fn new(w: usize, h: usize) -> Option<Self> {
    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
    info.bmiHeader = BITMAPINFOHEADER {
      biSize: size_of::<BITMAPINFOHEADER>() as u32,
      biWidth: w as i32,
      biHeight: -(h as i32),
      biPlanes: 1,
      biBitCount: 32,
      biCompression: BI_RGB,
      ..unsafe { std::mem::zeroed() }
    };
    let mut bits = null_mut();
    let bmp =
      unsafe { CreateDIBSection(null_mut(), &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0) };
    if bmp.is_null() {
      return None;
    }

    let dc = unsafe { CreateCompatibleDC(null_mut()) };
    unsafe { SelectObject(dc, bmp) };
    Some(Self { dc, bmp, bits: bits.cast(), w, h, tiles: Vec::new() })
  }

  /// The pixels to draw into, once GDI is done with them: it may batch the last frame's
  /// `BitBlt` reads from the section (`GdiFlush`, as `CreateDIBSection` asks before the bits are
  /// touched).
  fn pixels(&mut self) -> &mut [u32] {
    unsafe { GdiFlush() };
    // SAFETY: the section holds `w * h` pixels, written only through this borrow.
    unsafe { std::slice::from_raw_parts_mut(self.bits, self.w * self.h) }
  }

  fn blit(&self, dst: HDC, r: IRect) {
    let (w, h) = (r.x1 - r.x0, r.y1 - r.y0);
    unsafe { BitBlt(dst, r.x0, r.y0, w, h, self.dc, r.x0, r.y0, SRCCOPY) };
  }

  /// Shows the whole frame in layered window `hwnd`, blended by its alpha (premultiplied, as
  /// `AC_SRC_ALPHA` takes it): an overlay's.
  fn show_layered(&self, hwnd: HWND) {
    let size = SIZE { cx: self.w as i32, cy: self.h as i32 };
    let blend = BLENDFUNCTION {
      BlendOp: AC_SRC_OVER as u8,
      BlendFlags: 0,
      SourceConstantAlpha: 255,
      AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let at = POINT { x: 0, y: 0 };
    unsafe {
      UpdateLayeredWindow(hwnd, null_mut(), null(), &size, self.dc, &at, 0, &blend, ULW_ALPHA)
    };
  }
}

impl Drop for Dib {
  fn drop(&mut self) {
    unsafe {
      DeleteDC(self.dc);
      DeleteObject(self.bmp);
    }
  }
}

/// A high-resolution waitable timer, closed when dropped; null where the OS has none (before
/// Windows 10 1803), and the loop falls back to the wait's timeout.
struct Timer(HANDLE);

impl Timer {
  fn new() -> Self {
    let flags = CREATE_WAITABLE_TIMER_HIGH_RESOLUTION;
    Self(unsafe { CreateWaitableTimerExW(null(), null(), flags, TIMER_ALL_ACCESS) })
  }
}

impl Drop for Timer {
  fn drop(&mut self) {
    if !self.0.is_null() {
      unsafe { CloseHandle(self.0) };
    }
  }
}

// MARK: Window

struct Window {
  hwnd: HWND,
  rt: Runtime<Box<dyn App>>,
  renderer: Renderer,
  dib: Option<Dib>,
  scale: f32,
  /// Minimized or hidden: frames wait until it shows again.
  hidden: bool,
  /// An overlay ([Config::overlay]): a layered window, placed by [place].
  overlay: Option<Anchor>,
  /// While animating, frames come a refresh of the display apart: the display it is on.
  pace: Pace,
  monitor: HMONITOR,
  /// The frame timer's period in milliseconds while a modal loop runs ([MODAL]): a refresh
  /// while frames are due, the app's next timer when one is set, else none.
  armed: Option<u32>,
  /// The window a wake posts to: none once it is destroyed, so a waker kept by another thread
  /// never posts to a window that took the handle over.
  target: Arc<Mutex<isize>>,
  /// Logs each presented frame (`NIMBO_TRACE`): `nimbo: presented <unix ms>` on stderr, and the
  /// first one's time since the process started.
  trace: bool,
  frames: u64,
  /// The pointer is tracked for WM_MOUSELEAVE.
  tracking: bool,
  clicks: input::Clicks,
  /// The buttons held, in the window's capture.
  held: Vec<MouseButton>,
  /// A high surrogate from WM_CHAR, waiting for its low one.
  high: Option<u16>,
  /// The text field the input method is on for (off without one), whether it had marked text,
  /// and whether the input method's own message is under way.
  ime: Option<crate::ui::Id>,
  ime_marked: bool,
  ime_call: bool,
  /// Where the composition and candidate windows were put last.
  ime_at: Option<crate::scene::Rect>,
  /// The cursor last set.
  cursor: Cursor,
  /// The least content size in points ([Config::min]) and the window's style, for
  /// WM_GETMINMAXINFO.
  min: Size,
  style: WINDOW_STYLE,
  /// The system's light or dark mode and accent last sent.
  look: Appearance,
  /// Takes the files dragged onto the window while held.
  _drops: Box<files::Target>,
}

thread_local! {
  /// The open windows by handle; each borrowed only while its message is handled.
  static WINDOWS: RefCell<Vec<(HWND, Rc<RefCell<Window>>)>> = const { RefCell::new(Vec::new()) };
  /// The window whose message is being handled: [with]'s.
  static CURRENT: Cell<HWND> = const { Cell::new(null_mut()) };
  /// The modal loops running the thread's messages (a live move or resize, a menu's, the tray's
  /// menu): every window's frames come from its timer then ([Window::sync_modal]).
  static MODAL: Cell<u8> = const { Cell::new(0) };
  /// Focus changes that came while their window was busy up the stack (an app changed through
  /// `Window::with` fronts another window): sent in order once it is free ([with_window]).
  static WAITING: RefCell<Vec<(HWND, Event)>> = const { RefCell::new(Vec::new()) };
}

/// Runs `f` on the current window ([CURRENT]), unless a call up the stack has it (a message sent
/// while handling another) or it is gone.
fn with<R>(f: impl FnOnce(&mut Window) -> R) -> Option<R> {
  with_window(CURRENT.get(), f)
}

/// Runs `f` on window `hwnd`, as [with], then sends what waited for it.
fn with_window<R>(hwnd: HWND, f: impl FnOnce(&mut Window) -> R) -> Option<R> {
  let w = WINDOWS.with_borrow(|ws| ws.iter().find(|(h, _)| *h == hwnd).map(|(_, w)| w.clone()))?;
  let r = f(&mut *w.try_borrow_mut().ok()?);
  while let Ok(mut win) = w.try_borrow_mut() {
    let next = WAITING.with_borrow_mut(|q| Some(q.remove(q.iter().position(|e| e.0 == hwnd)?)));
    let Some((_, ev)) = next else { break };
    win.send(ev);
  }

  Some(r)
}

/// Sends `events` to the current window, or keeps them for it while it is busy.
fn send_or_wait(events: [Event; 2]) {
  let hwnd = CURRENT.get();
  let mut events = Some(events);
  with(|w| events.take().map(|e| e.map(|ev| w.send(ev))));
  if let Some(events) = events {
    WAITING.with_borrow_mut(|q| q.extend(events.map(|ev| (hwnd, ev))));
  }
}

/// Window `hwnd` shows (again): its frames start.
pub(crate) fn shown(hwnd: HWND) {
  with_window(hwnd, |w| {
    if std::mem::replace(&mut w.hidden, false) {
      w.rt.redraw();
    }
  });
}

/// A modal loop starts (`delta` 1) or ends (-1): every window's frame timer follows.
pub(crate) fn modal(delta: i8) {
  MODAL.set(MODAL.get().saturating_add_signed(delta));
  for h in handles() {
    with_window(h, |w| w.sync_modal());
  }
}

/// Runs `f` with `hwnd` the [CURRENT] window, then the one before again.
fn current<R>(hwnd: HWND, f: impl FnOnce() -> R) -> R {
  let before = CURRENT.replace(hwnd);
  let r = f();
  CURRENT.set(before);
  r
}

/// The open windows' handles.
fn handles() -> Vec<HWND> {
  WINDOWS.with_borrow(|ws| ws.iter().map(|(h, _)| *h).collect())
}

impl Window {
  /// The client area in pixels.
  fn client(&self) -> (i32, i32) {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetClientRect(self.hwnd, &mut r) };
    (r.right - r.left, r.bottom - r.top)
  }

  /// Gives the runtime the window's size and scale; false while it has no area (minimized).
  fn sync_size(&mut self) -> bool {
    let (w, h) = self.client();
    if w <= 0 || h <= 0 {
      return false;
    }

    let size = Size::new(points(w as u32, self.scale), points(h as u32, self.scale));
    self.rt.resize(size, self.scale);
    true
  }

  /// Builds a frame at the window's size and scale and presents it.
  fn render(&mut self) {
    self.pace.start(Instant::now());
    if !self.sync_size() {
      return;
    }

    self.rt.frame();
    // The new tree's element under the pointer may want another cursor, and its focus (a field
    // focused as it shows) the input method.
    self.sync_cursor(false);
    self.sync_ime();
    let r = &mut self.renderer;
    r.sync(&mut self.rt.text, &mut self.rt.images);
    r.prepare(&self.rt.scene, self.rt.size, self.scale);
    let (pw, ph) = r.size();
    if self.dib.as_ref().is_none_or(|d| (d.w, d.h) != (pw, ph)) {
      self.dib = Dib::new(pw, ph);
    }

    let Some(dib) = &mut self.dib else { return };
    let rects = r.damage(&dib.tiles);
    if rects.is_empty() {
      return;
    }

    r.draw(&self.rt.scene, dib.pixels(), pw, &rects);
    dib.tiles.clear();
    dib.tiles.extend_from_slice(r.tiles());
    if self.overlay.is_some() {
      dib.show_layered(self.hwnd);
    } else {
      let dc = unsafe { GetDC(self.hwnd) };
      for &rect in &rects {
        dib.blit(dc, rect);
      }

      unsafe { ReleaseDC(self.hwnd, dc) };
    }

    self.frames += 1;
    if self.trace {
      let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
      eprintln!("nimbo: presented {:.3}", t.as_secs_f64() * 1e3);
      if self.frames == 1 {
        eprintln!("nimbo: first frame {:.1} ms after start", since_start());
      }
    }
  }

  /// Runs the live move or resize's frame timer while frames are due or a timer is set, and
  /// stops it when idle; after each message, so a wake or an input that needs a frame starts it
  /// again.
  fn sync_modal(&mut self) {
    // Every refresh while a frame is due, else at the app's next timer.
    let wait = match MODAL.get() > 0 && !self.hidden {
      false => None,
      true if self.rt.needs_frame() => Some(self.pace.interval),
      true => self.rt.due_in(),
    };

    let ms = wait.map(|d| (d.as_millis() as u32).max(1));
    if ms == self.armed {
      return;
    }

    match ms {
      Some(ms) => unsafe {
        SetTimer(self.hwnd, TIMER_MODAL, ms, None);
      },
      None => unsafe {
        KillTimer(self.hwnd, TIMER_MODAL);
      },
    }

    self.armed = ms;
  }

  /// Follows the refresh rate of the window's display: another monitor, or another mode. The
  /// live move's timer, when it runs, goes on at the new interval ([Window::sync_modal] after
  /// the message).
  fn retime(&mut self) {
    self.monitor = unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST) };
    self.pace.interval = refresh(self.hwnd);
  }

  /// Follows the system's light or dark mode and accent: the title bar and the app, when they
  /// changed.
  fn restyle(&mut self) {
    let look = appearance();
    if look == self.look {
      return;
    }

    if look.dark != self.look.dark {
      settings::title_bar(self.hwnd, look.dark);
    }

    self.look = look;
    self.send(Event::Appearance(look));
  }

  /// Wakes post nowhere from now on, and drops come no more: the window is going.
  fn revoke(&self) {
    unsafe { RevokeDragDrop(self.hwnd) };
    if let Ok(mut hwnd) = self.target.lock() {
      *hwnd = 0;
    }
  }

  /// Draws a frame if one is due; returns how long the loop may sleep (`None`: until a
  /// message).
  fn tick(&mut self) -> Option<Duration> {
    if self.hidden {
      return None;
    }

    if self.rt.needs_frame() {
      let left = self.pace.left(Instant::now());
      if !left.is_zero() {
        return Some(left);
      }

      self.render();
    }

    self.wait()
  }

  /// How long the loop may sleep after the last frame (`None`: until a message): the rest of
  /// the interval while frames are wanted (the time spent drawing is part of it), else until the
  /// app's next timer.
  fn wait(&mut self) -> Option<Duration> {
    if self.hidden {
      return None;
    }

    match self.rt.needs_frame() {
      true => Some(self.pace.left(Instant::now())),
      false => self.rt.due_in(),
    }
  }
}

impl Drop for Window {
  fn drop(&mut self) {
    self.revoke();
  }
}

/// Milliseconds since the process was created, loader time included.
fn since_start() -> f64 {
  let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
  let (mut created, mut e, mut k, mut u, mut now) = (z, z, z, z, z);
  unsafe {
    GetProcessTimes(GetCurrentProcess(), &mut created, &mut e, &mut k, &mut u);
    GetSystemTimePreciseAsFileTime(&mut now);
  }
  let t = |f: FILETIME| (f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64;
  t(now).saturating_sub(t(created)) as f64 / 1e4
}

/// The refresh interval of the display the window is on (the one with most of it), 60 Hz when
/// unknown.
fn refresh(hwnd: HWND) -> Duration {
  let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
  let mut info: MONITORINFOEXW = unsafe { std::mem::zeroed() };
  info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
  let mut mode: DEVMODEW = unsafe { std::mem::zeroed() };
  mode.dmSize = size_of::<DEVMODEW>() as u16;
  let found = unsafe { GetMonitorInfoW(monitor, (&raw mut info).cast()) } != 0
    && unsafe { EnumDisplaySettingsW(info.szDevice.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode) }
      != 0;
  // 0 and 1 are the hardware's default rate.
  let hz = match mode.dmDisplayFrequency {
    hz if found && hz > 1 => hz.min(240),
    _ => 60,
  };

  Duration::from_secs_f64(1.0 / hz as f64)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
  current(hwnd, || {
    let res = handle(hwnd, msg, wp, lp);
    // A size the app asked for in the live move or resize loop (a timer's frame) is applied
    // now, not when the loop ends; outside the window's borrow, as SetWindowPos sends WM_SIZE.
    // So is a file dialog asked for, which runs the messages until it closes.
    apply_resize(hwnd);
    files::apply(hwnd);
    with(|w| w.sync_modal());
    res
  })
}

fn handle(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
  if let Some(r) = input::handle(hwnd, msg, wp, lp) {
    return r;
  }

  let def = || unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
  match msg {
    WM_WAKE => {
      with(|w| w.rt.redraw());
      0
    }
    WM_PAINT => {
      // Shows the frame held; the loop draws a new one when the runtime needs it.
      let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
      let dc = unsafe { BeginPaint(hwnd, &mut ps) };
      let shown = with(|w| {
        let dib = w.dib.as_ref()?;
        dib.blit(dc, IRect::new(0, 0, dib.w as i32, dib.h as i32));
        Some(())
      });
      if shown.flatten().is_none() {
        with(|w| w.rt.redraw());
      }

      unsafe { EndPaint(hwnd, &ps) };
      0
    }
    WM_PRINTCLIENT => {
      // PrintWindow: the frame held, into its DC.
      with(|w| {
        if let Some(dib) = &w.dib {
          dib.blit(wp as HDC, IRect::new(0, 0, dib.w as i32, dib.h as i32));
        }
      });
      0
    }
    WM_ERASEBKGND => 1,
    WM_GETMINMAXINFO => {
      // The least window size: the content's at the window's DPI, plus the frame. Sent before
      // the window is stored too, when it has none yet.
      let Some((min, style)) = with(|w| (w.min, w.style)) else { return def() };
      let dpi = unsafe { GetDpiForWindow(hwnd) };
      let k = dpi as f32 / 96.0;
      let (w, h) = ((min.w * k).ceil() as i32, (min.h * k).ceil() as i32);
      let mut rc = RECT { left: 0, top: 0, right: w, bottom: h };
      unsafe { AdjustWindowRectExForDpi(&mut rc, style, 0, 0, dpi) };
      // SAFETY: lParam points to the MINMAXINFO to fill.
      let info = unsafe { &mut *(lp as *mut MINMAXINFO) };
      info.ptMinTrackSize.x = rc.right - rc.left;
      info.ptMinTrackSize.y = rc.bottom - rc.top;
      0
    }
    WM_SIZE => {
      // Minimized, or resized while hidden ([crate::Window::hide]): no frames until it shows.
      let off = wp == SIZE_MINIMIZED as usize || unsafe { IsWindowVisible(hwnd) } == 0;
      with(|w| {
        let shown = w.hidden && !off;
        w.hidden = off;
        if shown {
          w.rt.redraw();
        }
        // Draw right away, so a live resize never shows a stretched or stale frame; hidden, the
        // runtime still takes the size, which placing an overlay reads.
        match off {
          false => w.render(),
          true => _ = w.sync_size(),
        }
      });
      0
    }
    WM_DPICHANGED => {
      let dpi = (wp & 0xFFFF) as f32;
      let overlay = with(|w| {
        w.scale = dpi / 96.0;
        w.overlay
      });
      // An overlay keeps its size in points at its anchor, on the monitor it is on now.
      if let Some(anchor) = overlay.flatten() {
        place(hwnd, anchor, false);
        with(|w| w.retime());
        return 0;
      }

      // SAFETY: lParam is the suggested window rect.
      let r = unsafe { &*(lp as *const RECT) };
      let flags = SWP_NOZORDER | SWP_NOACTIVATE;
      let (x, y, cx, cy) = (r.left, r.top, r.right - r.left, r.bottom - r.top);
      unsafe { SetWindowPos(hwnd, null_mut(), x, y, cx, cy, flags) };
      with(|w| w.retime());
      0
    }
    WM_DISPLAYCHANGE => {
      with(|w| w.retime());
      def()
    }
    WM_MOVE => {
      with(|w| {
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        if monitor != w.monitor {
          w.retime();
        }
      });
      0
    }
    WM_ENTERSIZEMOVE | WM_ENTERMENULOOP => {
      modal(1);
      0
    }
    // The live move's or the menu's loop took the keys pressed or let go meanwhile: the
    // modifiers held now.
    WM_EXITSIZEMOVE | WM_EXITMENULOOP => {
      let focused = unsafe { GetFocus() } == hwnd;
      modal(-1);
      with(|w| {
        w.send(Event::Mods(if focused { keyboard::mods_held() } else { Modifiers::empty() }));
      });
      0
    }
    WM_TIMER if wp == TIMER_MODAL => {
      with(|w| {
        if w.rt.needs_frame() {
          w.render();
        }
      });
      0
    }
    // Modifiers that went up or down in another window never reach this one: none held while
    // away, those held now on return.
    WM_SETFOCUS => {
      send_or_wait([Event::Focus(true), Event::Mods(keyboard::mods_held())]);
      0
    }
    WM_KILLFOCUS => {
      send_or_wait([Event::Mods(Modifiers::empty()), Event::Focus(false)]);
      0
    }
    m if settings::changed(m, lp) => {
      with(|w| w.restyle());
      def()
    }
    WM_DESTROY => {
      with(|w| w.revoke());
      // Dropped here unless a call up the stack holds it, outside the list's borrow (the app's
      // drop may open or close windows); its close callback runs after.
      let gone = WINDOWS.with_borrow_mut(|ws| {
        let i = ws.iter().position(|(h, _)| *h == hwnd)?;
        Some(ws.remove(i))
      });
      drop(gone);
      app::shell::destroyed(hwnd);
      0
    }
    _ => def(),
  }
}

/// Makes a window running `app`, hidden, and keeps it in [WINDOWS].
fn create(cfg: &Config, app: Box<dyn App>) -> HWND {
  unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
  let hinstance = unsafe { GetModuleHandleW(null()) };
  let class = wide("nimbo");
  let wc = WNDCLASSW {
    lpfnWndProc: Some(wndproc),
    hInstance: hinstance,
    hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
    lpszClassName: class.as_ptr(),
    ..unsafe { std::mem::zeroed() }
  };
  unsafe { RegisterClassW(&wc) };

  // The content area at the system DPI, centred on the primary screen; a window on another
  // monitor gets WM_DPICHANGED.
  // An overlay: no frame, blended by its alpha, the mouse going through, never activated, out
  // of the taskbar and Alt+Tab, over other windows.
  let (style, ex) = match (cfg.overlay, cfg.resizable) {
    (Some(_), _) => {
      let ex = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
      (WS_POPUP, ex | WS_EX_TOPMOST)
    }
    (None, true) => (WS_OVERLAPPEDWINDOW, 0),
    (None, false) => (WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX, 0),
  };

  let dpi = unsafe { GetDpiForSystem() };
  let scale = dpi as f32 / 96.0;
  let (w, h) = ((cfg.size.w * scale).round() as i32, (cfg.size.h * scale).round() as i32);
  let mut rc = RECT { left: 0, top: 0, right: w, bottom: h };
  unsafe { AdjustWindowRectExForDpi(&mut rc, style, 0, ex, dpi) };
  let (ww, wh) = (rc.right - rc.left, rc.bottom - rc.top);
  let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
  let (x, y) = (((sw - ww) / 2).max(0), ((sh - wh) / 2).max(0));
  let title = wide(cfg.title);
  let hwnd = unsafe {
    CreateWindowExW(
      ex,
      class.as_ptr(),
      title.as_ptr(),
      style,
      x,
      y,
      ww,
      wh,
      null_mut(),
      null_mut(),
      hinstance,
      null(),
    )
  };
  assert!(!hwnd.is_null(), "nimbo: CreateWindowExW failed");
  // No input method until a text field takes it.
  input::ime_off(hwnd);
  let drops = files::register(hwnd);

  let scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
  let mut rt = Runtime::new(app, cfg.size, scale, Clock::real(), Box::new(WinClipboard::new(hwnd)));
  // The title bar's mode before the window shows, so it never flashes light.
  let look = appearance();
  settings::title_bar(hwnd, look.dark);
  rt.event(Event::Appearance(look));
  // A wake from any thread posts a message; the window handle crosses threads as a number, held
  // while posting so the window's end waits for a post under way.
  let target = Arc::new(Mutex::new(hwnd as isize));
  let notify = target.clone();
  rt.waker().set_notify(Box::new(move || {
    if let Ok(hwnd) = notify.lock()
      && *hwnd != 0
    {
      unsafe { PostMessageW(*hwnd as HWND, WM_WAKE, 0, 0) };
    }
  }));
  let mut renderer = Renderer::new();
  renderer.transparent = cfg.overlay.is_some();
  let window = Window {
    hwnd,
    rt,
    renderer,
    dib: None,
    scale,
    // Opened hidden ([Config::hidden]): no frames until it shows ([shown]).
    hidden: cfg.hidden,
    overlay: cfg.overlay,
    pace: Pace::new(refresh(hwnd)),
    monitor: unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) },
    trace: std::env::var_os("NIMBO_TRACE").is_some(),
    frames: 0,
    tracking: false,
    clicks: input::Clicks::default(),
    held: Vec::new(),
    high: None,
    ime: None,
    ime_marked: false,
    ime_call: false,
    ime_at: None,
    cursor: Cursor::Arrow,
    min: cfg.min(),
    style,
    look,
    _drops: drops,
    armed: None,
    target,
  };
  WINDOWS.with_borrow_mut(|ws| ws.push((hwnd, Rc::new(RefCell::new(window)))));
  hwnd
}

/// Resizes the client area to the size the app asked for ([Runtime::take_resize]) at the
/// window's DPI, keeping its top-left corner; the least size comes down with it first, as
/// WM_GETMINMAXINFO holds SetWindowPos to it too. Outside the window's borrow: WM_SIZE draws.
fn apply_resize(hwnd: HWND) -> bool {
  let want = with(|w| {
    let size = w.rt.take_resize()?;
    w.min = Size::new(w.min.w.min(size.w), w.min.h.min(size.h));
    Some((size, w.style))
  });
  let Some((size, style)) = want.flatten() else { return false };
  let dpi = unsafe { GetDpiForWindow(hwnd) };
  let k = dpi as f32 / 96.0;
  let (w, h) = ((size.w * k).round() as i32, (size.h * k).round() as i32);
  let mut rc = RECT { left: 0, top: 0, right: w, bottom: h };
  unsafe { AdjustWindowRectExForDpi(&mut rc, style, 0, 0, dpi) };
  let flags = SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE;
  let (cx, cy) = (rc.right - rc.left, rc.bottom - rc.top);
  unsafe { SetWindowPos(hwnd, null_mut(), 0, 0, cx, cy, flags) };
  if let Some(anchor) = with(|w| w.overlay).flatten() {
    place(hwnd, anchor, false);
  }

  true
}

/// Shows overlay `hwnd` at `anchor` on the monitor with the pointer, over other windows,
/// activating nothing.
pub(crate) fn show_overlay(hwnd: HWND, anchor: Anchor) {
  place(hwnd, anchor, true);
  shown(hwnd);
  unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
}

/// Moves overlay `hwnd` to `anchor` in the work area (no taskbar) of the monitor with the
/// pointer, or of its own unless `pointer`, at its size in points (the last frame's, or the one
/// it asked for) for that monitor's DPI; topmost again.
fn place(hwnd: HWND, anchor: Anchor, pointer: bool) {
  let monitor = match pointer {
    true => {
      let mut p = POINT { x: 0, y: 0 };
      unsafe { GetCursorPos(&mut p) };
      unsafe { MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST) }
    }
    false => unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) },
  };

  let mut info =
    MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..unsafe { std::mem::zeroed() } };
  let (mut dpi, mut _y) = (96, 96);
  unsafe {
    GetMonitorInfoW(monitor, &mut info);
    GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _y);
  }

  let (r, k) = (info.rcWork, dpi as f32 / 96.0);
  // Drawn at that monitor's scale from the move's WM_SIZE on, before or without WM_DPICHANGED
  // (a hidden window): a stale one would turn its size in points into another.
  let Some(size) = with_window(hwnd, |w| {
    w.scale = k;
    w.rt.size
  }) else {
    return;
  };
  let area = Size::new((r.right - r.left) as f32 / k, (r.bottom - r.top) as f32 / k);
  let at = anchor.offset(area, size);
  let px = |v: f32| (v * k).round() as i32;
  let (x, y) = (r.left + px(at.x), r.top + px(at.y));
  unsafe { SetWindowPos(hwnd, HWND_TOPMOST, x, y, px(size.w), px(size.h), SWP_NOACTIVATE) };
}

/// Draws the frame due and applies a size the app asked for, before it and in it, then a file
/// dialog; returns how long the loop may sleep (`None`: until a message). A size asked for in
/// the frame draws another through WM_SIZE, as a dialog's answer wants one, and the sleep
/// follows that: what it wants (an animation, a timer) would otherwise wait for the next message.
fn settle(hwnd: HWND) -> Option<Duration> {
  apply_resize(hwnd);
  let wait = with(|w| w.tick()).flatten();
  match apply_resize(hwnd) | files::apply(hwnd) {
    true => with(|w| w.wait()).flatten(),
    false => wait,
  }
}

/// Handles messages until WM_QUIT; between them draws each window's frame due and sleeps until
/// the next message, wake or timer.
fn run_loop(timer: &Timer) {
  let mut msg: MSG = unsafe { std::mem::zeroed() };
  loop {
    while unsafe { PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) } != 0 {
      if msg.message == WM_QUIT {
        return;
      }

      let root = unsafe { GetAncestor(msg.hwnd, GA_ROOT) };
      if current(root, || input::key_message(&msg)) {
        continue;
      }

      unsafe {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
      }
    }

    let windows = handles();
    if windows.iter().any(|&h| with_window(h, |w| w.rt.quit_requested()).unwrap_or(false)) {
      app::shell::quit();
      continue;
    }

    let waits = windows.iter().map(|&h| current(h, || settle(h)));
    let wait = waits.flatten().min();
    let n = match wait {
      // Relative due times are negative, in 100 ns units.
      Some(d) if !timer.0.is_null() => {
        let due = -((d.as_nanos() / 100).max(1) as i64);
        unsafe { SetWaitableTimer(timer.0, &due, 0, None, null(), 0) };
        1
      }
      _ => 0,
    };

    let ms = match (wait, n) {
      (Some(d), 0) => d.as_millis().max(1) as u32,
      _ => INFINITE,
    };

    unsafe { MsgWaitForMultipleObjectsEx(n, &timer.0, ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE) };
  }
}

// MARK: Clipboard

/// The system clipboard, plain text (`CF_UNICODETEXT`), opened for the window: one opened for
/// no window has no owner once emptied, and `SetClipboardData` fails then (its docs).
pub struct WinClipboard {
  /// The window's handle, as a number (it never leaves the thread).
  owner: isize,
}

impl WinClipboard {
  fn new(owner: HWND) -> Self {
    Self { owner: owner as isize }
  }
}

impl Clipboard for WinClipboard {
  fn read(&self) -> Option<String> {
    let _open = Open::by(self.owner as HWND)?;
    let bytes = paste::bytes(CF_TEXT_UTF16)?;
    // Another process wrote it: the NUL that ends CF_UNICODETEXT is looked for only within it.
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|u| u16::from_ne_bytes(*u)).collect();
    Some(utf16_text(&units))
  }

  fn write(&mut self, s: &str) {
    if let Some(_open) = Open::by(self.owner as HWND) {
      unsafe { EmptyClipboard() };
      paste::set(CF_TEXT_UTF16, &paste::utf16_bytes(&wide(s)));
    }
  }
}

/// The system clipboard outside a window, through the app's hidden window; `None` while no app
/// runs: a clipboard opened for no window can't be written.
pub fn clipboard() -> Option<impl Clipboard> {
  let host = app::shell::host();
  (!host.is_null()).then(|| WinClipboard::new(host))
}

/// The text of UTF-16 `units` up to the first NUL, or all of them without one.
fn utf16_text(units: &[u16]) -> String {
  let n = units.iter().position(|&u| u == 0).unwrap_or(units.len());
  String::from_utf16_lossy(&units[..n])
}

/// The cores of the highest efficiency class (a hybrid CPU's performance cores, every core of
/// another), at most 4: past that, more threads cost CPU time for little wall time
/// (as measured on a hybrid CPU).
pub fn perf_cores() -> Option<usize> {
  let mut len = 0u32;
  unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, null_mut(), &mut len) };
  let mut buf = vec![0u64; (len as usize).div_ceil(8)];
  let p = buf.as_mut_ptr().cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>();
  if unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, p, &mut len) } == 0 {
    return None;
  }

  // Variable-sized records, each `Size` bytes: a core's is shorter than the whole union, so
  // only its fields are read, from the bytes, never through a reference to the full record.
  // SAFETY: the call filled `len` bytes of `buf`, which holds at least that many.
  let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), len as usize) };
  let size_at = offset_of!(SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX, Size);
  let class_at = offset_of!(SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX, Anonymous)
    + offset_of!(PROCESSOR_RELATIONSHIP, EfficiencyClass);
  let mut classes = Vec::new();
  let mut at = 0;
  while let (Some(size), Some(&class)) =
    (bytes.get(at + size_at..at + size_at + 4), bytes.get(at + class_at))
  {
    classes.push(class);
    at += u32::from_ne_bytes(size.try_into().ok()?).max(1) as usize;
  }

  let top = *classes.iter().max()?;
  Some(classes.iter().filter(|&&c| c == top).count().min(4))
}

#[cfg(test)]
mod tests {
  use windows_sys::Win32::System::DataExchange::GetClipboardOwner;

  use super::*;

  /// The modal loop's timer messages handled after waiting `ms`: how many came.
  fn timer_ticks(hwnd: HWND, ms: u64) -> usize {
    std::thread::sleep(Duration::from_millis(ms));
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    let mut n = 0;
    while unsafe { PeekMessageW(&mut msg, hwnd, WM_TIMER, WM_TIMER, PM_REMOVE) } != 0 {
      unsafe { DispatchMessageW(&msg) };
      n += 1;
    }

    n
  }

  /// A window made as the shell makes it, hidden, and the current one for [with].
  fn open(cfg: &Config, app: Box<dyn App>) -> HWND {
    let hwnd = create(cfg, app);
    CURRENT.set(hwnd);
    hwnd
  }

  struct Busy;
  impl App for Busy {
    fn paint(&mut self, f: &mut crate::Frame) {
      f.animate();
    }
  }

  #[test]
  fn perf_cores_are_found() {
    assert!(perf_cores().is_some_and(|n| (1..=4).contains(&n)));
  }

  /// It empties the user's clipboard (every format, not only text), so it runs only when asked:
  /// `cargo test -- --ignored clipboard_round_trips` on a machine whose clipboard can go.
  #[test]
  #[ignore = "empties the system clipboard"]
  fn clipboard_round_trips() {
    // A message-only window owns it, as the app's window does: never shown.
    let class = wide("STATIC");
    let hwnd = unsafe {
      CreateWindowExW(
        0,
        class.as_ptr(),
        null(),
        0,
        0,
        0,
        0,
        0,
        HWND_MESSAGE,
        null_mut(),
        null_mut(),
        null(),
      )
    };
    assert!(!hwnd.is_null());
    let mut c = WinClipboard::new(hwnd);
    c.write("nimbo ✓ 👍");
    assert_eq!(c.read().as_deref(), Some("nimbo ✓ 👍"));
    assert_eq!(unsafe { GetClipboardOwner() }, hwnd, "the window owns what it wrote");
    unsafe { DestroyWindow(hwnd) };
  }

  #[test]
  fn clipboard_text_ends_within_its_memory() {
    let ab: Vec<u16> = "ab".encode_utf16().collect();
    assert_eq!(utf16_text(&ab), "ab", "no NUL: the allocation's end");
    assert_eq!(utf16_text(&[0x61, 0, 0x62]), "a");
    assert_eq!(utf16_text(&[]), "");
  }

  #[test]
  fn dib_holds_pixels() {
    let mut d = Dib::new(3, 2).unwrap();
    d.pixels().fill(0xff00ff00);
    assert_eq!(d.pixels().len(), 6);
  }

  /// Holding the title bar of a still window: the live move's timer stops once no frame is
  /// due, and a wake starts it again. The window is never shown.
  #[test]
  fn a_live_move_ticks_only_while_frames_are_due() {
    struct Still;
    impl App for Still {}

    let cfg = Config::new("modal", 200.0, 100.0);
    let hwnd = open(&cfg, Box::new(Still));
    let ticks = || timer_ticks(hwnd, 120);

    unsafe { SendMessageW(hwnd, WM_ENTERSIZEMOVE, 0, 0) };
    assert!(ticks() > 0, "the first frame is due");
    assert_eq!(ticks(), 0, "idle");

    unsafe { SendMessageW(hwnd, WM_WAKE, 0, 0) };
    assert!(ticks() > 0, "a wake draws");
    assert_eq!(ticks(), 0, "idle again");

    unsafe {
      SendMessageW(hwnd, WM_EXITSIZEMOVE, 0, 0);
      DestroyWindow(hwnd);
    }
  }

  /// Shift let go during a live resize, whose loop takes its key message: the modifiers come
  /// again as it ends, none for a window without the focus. The window is never shown.
  #[test]
  fn a_live_resize_ends_with_the_modifiers_held() {
    #[derive(Default)]
    struct Seen(std::rc::Rc<std::cell::Cell<Option<Modifiers>>>);
    impl App for Seen {
      fn event(&mut self, _: &mut crate::app::Cx, ev: &Event) -> bool {
        if let Event::Mods(m) = ev {
          self.0.set(Some(*m));
        }

        false
      }
    }

    let app = Seen::default();
    let seen = app.0.clone();
    let hwnd = open(&Config::new("mods", 200.0, 100.0), Box::new(app));
    with(|w| w.send(Event::Mods(Modifiers::SHIFT)));
    unsafe { SendMessageW(hwnd, WM_ENTERSIZEMOVE, 0, 0) };
    unsafe { SendMessageW(hwnd, WM_EXITSIZEMOVE, 0, 0) };
    assert_eq!(seen.get(), Some(Modifiers::empty()));

    unsafe { DestroyWindow(hwnd) };
  }

  /// A live move's timer follows a new display mode: set at a second's interval, it ticks at the
  /// display's once WM_DISPLAYCHANGE comes. The window is never shown.
  #[test]
  fn a_new_display_mode_sets_the_interval() {
    let cfg = Config::new("retime", 200.0, 100.0);
    let hwnd = open(&cfg, Box::new(Busy));
    with(|w| w.pace.interval = Duration::from_secs(1));
    unsafe { SendMessageW(hwnd, WM_ENTERSIZEMOVE, 0, 0) };
    let ticks = || timer_ticks(hwnd, 150);
    assert_eq!(ticks(), 0, "a second apart");

    unsafe { SendMessageW(hwnd, WM_DISPLAYCHANGE, 32, 0) };
    assert_eq!(with(|w| w.pace.interval), Some(refresh(hwnd)));
    assert!(ticks() > 0, "a refresh apart");

    unsafe {
      SendMessageW(hwnd, WM_EXITSIZEMOVE, 0, 0);
      DestroyWindow(hwnd);
    }
  }

  /// An open menu (the system menu, a context menu) runs a modal loop too: an animation goes on
  /// under it, and under a live move started from it, until the last one ends. The window is
  /// never shown.
  #[test]
  fn a_menu_keeps_animating() {
    let cfg = Config::new("menu", 200.0, 100.0);
    let hwnd = open(&cfg, Box::new(Busy));
    assert_eq!(timer_ticks(hwnd, 120), 0, "the message loop draws");

    unsafe { SendMessageW(hwnd, WM_ENTERMENULOOP, 0, 0) };
    assert!(timer_ticks(hwnd, 120) > 0, "a menu");

    unsafe {
      SendMessageW(hwnd, WM_ENTERSIZEMOVE, 0, 0);
      SendMessageW(hwnd, WM_EXITSIZEMOVE, 0, 0);
    }
    assert!(timer_ticks(hwnd, 120) > 0, "the menu still open");

    unsafe { SendMessageW(hwnd, WM_EXITMENULOOP, 0, 0) };
    timer_ticks(hwnd, 0);
    assert_eq!(timer_ticks(hwnd, 120), 0, "closed");

    unsafe { DestroyWindow(hwnd) };
  }

  /// An overlay: a layered, click-through, never activated tool window, drawn transparent.
  #[test]
  fn an_overlay_is_layered_and_transparent() {
    let cfg = Config::new("hud", 200.0, 60.0).overlay(Anchor::Bottom(40.0));
    let hwnd = open(&cfg, Box::new(Busy));
    let ex = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    let want = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
    assert_eq!(ex & want, want);
    assert_eq!(with_window(hwnd, |w| w.renderer.transparent), Some(true));
    unsafe { DestroyWindow(hwnd) };
  }

  /// The keyboard focus coming and going reaches the app.
  #[test]
  fn focus_reaches_the_app() {
    use std::rc::Rc;

    struct Seen(Rc<RefCell<Vec<bool>>>);
    impl App for Seen {
      fn event(&mut self, _: &mut crate::app::Cx, ev: &Event) -> bool {
        if let Event::Focus(on) = ev {
          self.0.borrow_mut().push(*on);
        }

        false
      }
    }

    let seen = Rc::new(RefCell::new(Vec::new()));
    let hwnd = open(&Config::new("focus", 200.0, 100.0), Box::new(Seen(seen.clone())));
    unsafe {
      SendMessageW(hwnd, WM_SETFOCUS, 0, 0);
      SendMessageW(hwnd, WM_KILLFOCUS, 0, 0);
    }
    assert_eq!(*seen.borrow(), [true, false]);

    // Sent while the window is busy up the stack (an app's `with` fronting another window):
    // kept, then sent in order once it is free, never a double borrow.
    with_window(hwnd, |_| unsafe {
      SendMessageW(hwnd, WM_SETFOCUS, 0, 0);
      SendMessageW(hwnd, WM_KILLFOCUS, 0, 0);
    });
    assert_eq!(*seen.borrow(), [true, false, true, false]);

    unsafe { DestroyWindow(hwnd) };
  }

  /// The app hears the system's appearance before the first frame, and again on a colour-set
  /// change only when it differs from the one sent last. The window is never shown.
  #[test]
  fn a_color_set_change_sends_the_new_appearance() {
    use std::rc::Rc;

    struct Seen(Rc<RefCell<Vec<Appearance>>>);
    impl App for Seen {
      fn event(&mut self, _: &mut crate::app::Cx, ev: &Event) -> bool {
        if let Event::Appearance(a) = ev {
          self.0.borrow_mut().push(*a);
        }

        false
      }
    }

    let seen = Rc::new(RefCell::new(Vec::new()));
    let hwnd = open(&Config::new("look", 200.0, 100.0), Box::new(Seen(seen.clone())));
    let now = appearance();
    assert_eq!(*seen.borrow(), [now]);

    let set = wide("ImmersiveColorSet");
    let change = || unsafe { SendMessageW(hwnd, WM_SETTINGCHANGE, 0, set.as_ptr() as LPARAM) };
    change();
    assert_eq!(seen.borrow().len(), 1, "unchanged: nothing sent");

    // As if the window last saw the other mode.
    let other = Appearance { dark: !now.dark, ..now };
    with(|w| {
      w.look = other;
      w.send(Event::Appearance(other));
    });
    change();
    assert_eq!(*seen.borrow(), [now, other, now]);
    assert_eq!(with(|w| w.look), Some(now));

    unsafe { DestroyWindow(hwnd) };
  }

  /// A waker another thread keeps past the window's end posts to no window: its handle may be
  /// another's by then.
  #[test]
  fn a_waker_outliving_its_window_posts_nowhere() {
    struct Still;
    impl App for Still {}

    let hwnd = open(&Config::new("waker", 200.0, 100.0), Box::new(Still));
    let (waker, target) = with(|w| (w.rt.waker().clone(), w.target.clone())).unwrap();
    assert_eq!(*target.lock().unwrap(), hwnd as isize);

    unsafe { DestroyWindow(hwnd) };
    assert_eq!(*target.lock().unwrap(), 0);

    waker.wake();
  }

  /// An app's timer comes due while the title bar is held still: the live move's timer
  /// draws it, then stops.
  #[test]
  fn a_live_move_draws_a_timer_when_due() {
    #[derive(Default)]
    struct Later(bool);
    impl App for Later {
      fn view(&mut self, ui: &mut crate::Ui) {
        if !std::mem::replace(&mut self.0, true) {
          ui.request_frame_in(Duration::from_millis(60));
        }
      }
    }

    let cfg = Config::new("modal timer", 200.0, 100.0);
    let hwnd = open(&cfg, Box::new(Later::default()));
    let ticks = || timer_ticks(hwnd, 120);
    unsafe { SendMessageW(hwnd, WM_ENTERSIZEMOVE, 0, 0) };
    assert!(ticks() > 0, "the first frame is due");
    assert!(ticks() > 0, "the timer draws");
    assert_eq!(ticks(), 0, "idle");
    unsafe {
      SendMessageW(hwnd, WM_EXITSIZEMOVE, 0, 0);
      DestroyWindow(hwnd);
    }
  }

  /// A size the app asks for in a frame draws another through WM_SIZE: the loop's sleep follows
  /// that one, so the timer it sets wakes the loop, not the next message. The window is never
  /// shown.
  #[test]
  fn a_frame_after_an_asked_size_keeps_its_timer() {
    use std::cell::Cell;
    use std::rc::Rc;

    struct Grow(Rc<Cell<bool>>);
    impl App for Grow {
      fn view(&mut self, ui: &mut crate::Ui) {
        if !self.0.get() {
          return;
        }

        match ui.size().w > 250.0 {
          true => ui.request_frame_in(Duration::from_millis(50)),
          false => ui.set_window_size(300.0, 200.0),
        }
      }
    }

    let grow = Rc::new(Cell::new(false));
    let hwnd = open(&Config::new("grow", 200.0, 100.0), Box::new(Grow(grow.clone())));
    for _ in 0..10 {
      if settle(hwnd).is_none() {
        break;
      }

      std::thread::sleep(Duration::from_millis(20));
    }

    assert_eq!(settle(hwnd), None, "idle");

    // A refresh later: the frame is due at once.
    std::thread::sleep(Duration::from_millis(50));
    grow.set(true);
    with(|w| w.rt.redraw());
    let wait = settle(hwnd);
    assert!(with(|w| w.rt.size.w).is_some_and(|w| w > 250.0), "resized");
    assert!(wait.is_some_and(|d| d <= Duration::from_millis(50)), "{wait:?}");

    unsafe { DestroyWindow(hwnd) };
  }

  #[test]
  fn timer_closes_its_handle() {
    use windows_sys::Win32::Foundation::GetHandleInformation;

    let timer = Timer::new();
    let (h, mut flags) = (timer.0, 0);
    assert!(!h.is_null());
    assert_ne!(unsafe { GetHandleInformation(h, &mut flags) }, 0);

    drop(timer);
    assert_eq!(unsafe { GetHandleInformation(h, &mut flags) }, 0);
  }
}
