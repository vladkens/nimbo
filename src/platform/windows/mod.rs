// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Windows platform: one Win32 window, per-monitor DPI aware (v2), its frames drawn by the
//! CPU renderer into a DIB section and copied to the window with BitBlt, only the tiles that
//! changed ([Dib]). The message loop sleeps in MsgWaitForMultipleObjectsEx until input, a wake
//! (the waker posts a message) or a high-resolution waitable timer: the runtime's next timer, or
//! while it animates the next frame slot at the refresh rate of the display it is on, followed as
//! it moves to another or the mode changes. No frames while the window is minimized; one when it
//! shows again. Input and cursors are in [input], keys in [keyboard]; then the clipboard, the
//! performance cores, and the DirectWrite text backend ([text]).

mod input;
mod keyboard;
pub mod text;

use std::cell::RefCell;
use std::mem::offset_of;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{
  CloseHandle, FILETIME, GlobalFree, HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::DataExchange::{
  CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{
  GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows_sys::Win32::System::SystemInformation::{
  GetLogicalProcessorInformationEx, GetSystemTimePreciseAsFileTime, PROCESSOR_RELATIONSHIP,
  RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};
use windows_sys::Win32::System::Threading::{
  CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, GetCurrentProcess,
  GetProcessTimes, INFINITE, SetWaitableTimer, TIMER_ALL_ACCESS,
};
use windows_sys::Win32::UI::HiDpi::{
  AdjustWindowRectExForDpi, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem,
  GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::Config;
use crate::app::{App, Appearance, Clipboard, Clock, Event, Mods, MouseButton, Runtime};
use crate::cpu::Renderer;
use crate::cpu::raster::IRect;
use crate::platform::{Pace, points};
use crate::scene::Size;
use crate::ui::Cursor;

/// Posted by the waker: something changed on another thread.
const WM_WAKE: u32 = WM_APP + 1;
/// Frames during a live resize or move, or while a menu is open: modal loops the message loop
/// doesn't run.
const TIMER_MODAL: usize = 1;
/// `CF_UNICODETEXT`.
const CF_TEXT_UTF16: u32 = 13;

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
  /// Minimized: frames wait until it shows again.
  hidden: bool,
  /// While animating, frames come a refresh of the display apart: the display it is on.
  pace: Pace,
  monitor: HMONITOR,
  /// How many modal loops run the messages (a live move or resize, a menu's), and their frame
  /// timer's period in milliseconds: a refresh while frames are due, the app's next timer when
  /// one is set, else none.
  modal: u8,
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
  /// The cursor last set.
  cursor: Cursor,
  /// The least content size in points ([Config::min]) and the window's style, for
  /// WM_GETMINMAXINFO.
  min: Size,
  style: WINDOW_STYLE,
}

thread_local! {
  static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
}

/// Runs `f` on the window, unless a call up the stack has it (a message sent while handling
/// another) or it is gone.
fn with<R>(f: impl FnOnce(&mut Window) -> R) -> Option<R> {
  WINDOW.with(|w| w.try_borrow_mut().ok()?.as_mut().map(f))
}

impl Window {
  /// The client area in pixels.
  fn client(&self) -> (i32, i32) {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetClientRect(self.hwnd, &mut r) };
    (r.right - r.left, r.bottom - r.top)
  }

  /// Builds a frame at the window's size and scale and presents it.
  fn render(&mut self) {
    self.pace.start(Instant::now());
    let (w, h) = self.client();
    if w <= 0 || h <= 0 {
      return;
    }

    let size = Size::new(points(w as u32, self.scale), points(h as u32, self.scale));
    self.rt.resize(size, self.scale);
    self.rt.frame();
    // The new tree's element under the pointer may want another cursor.
    self.sync_cursor(false);
    let r = &mut self.renderer;
    r.sync(&mut self.rt.text, &mut self.rt.images);
    r.prepare(&self.rt.scene, size, self.scale);
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
    let dc = unsafe { GetDC(self.hwnd) };
    for &rect in &rects {
      dib.blit(dc, rect);
    }

    unsafe { ReleaseDC(self.hwnd, dc) };
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
    let wait = match self.modal > 0 && !self.hidden {
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

  /// Wakes post nowhere from now on: the window is going.
  fn revoke(&self) {
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
  let res = handle(hwnd, msg, wp, lp);
  // A size the app asked for in the live move or resize loop (a timer's frame) is applied now,
  // not when the loop ends; outside the window's borrow, as SetWindowPos sends WM_SIZE.
  apply_resize(hwnd);
  with(|w| w.sync_modal());
  res
}

fn handle(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
  if let Some(r) = input::handle(msg, wp, lp) {
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
      let minimized = wp == SIZE_MINIMIZED as usize;
      with(|w| {
        let shown = w.hidden && !minimized;
        w.hidden = minimized;
        if shown {
          w.rt.redraw();
        }
        // Draw right away, so a live resize never shows a stretched or stale frame.
        if !minimized {
          w.render();
        }
      });
      0
    }
    WM_DPICHANGED => {
      let dpi = (wp & 0xFFFF) as f32;
      with(|w| w.scale = dpi / 96.0);
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
      with(|w| w.modal += 1);
      0
    }
    // The live move's or the menu's loop took the keys pressed or let go meanwhile: the
    // modifiers held now.
    WM_EXITSIZEMOVE | WM_EXITMENULOOP => {
      let focused = unsafe { GetFocus() } == hwnd;
      with(|w| {
        w.modal = w.modal.saturating_sub(1);
        w.send(Event::Mods(if focused { keyboard::mods_held() } else { Mods::NONE }));
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
      with(|w| {
        w.send(Event::Focus(true));
        w.send(Event::Mods(keyboard::mods_held()));
      });
      0
    }
    WM_KILLFOCUS => {
      with(|w| {
        w.send(Event::Mods(Mods::NONE));
        w.send(Event::Focus(false));
      });
      0
    }
    WM_DESTROY => {
      with(|w| w.revoke());
      unsafe { PostQuitMessage(0) };
      0
    }
    _ => def(),
  }
}

/// Opens the app's window and runs the Win32 message loop until it closes or the app quits.
pub fn run(cfg: &Config, app: Box<dyn App>) {
  let hwnd = create(cfg, app);
  unsafe { ShowWindow(hwnd, SW_SHOW) };

  run_loop(hwnd, &Timer::new());
  WINDOW.with(|w| w.borrow_mut().take());
}

/// Makes the app's window, hidden, and keeps it as this thread's [WINDOW].
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
  let style = match cfg.resizable {
    true => WS_OVERLAPPEDWINDOW,
    false => WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
  };

  let dpi = unsafe { GetDpiForSystem() };
  let scale = dpi as f32 / 96.0;
  let (w, h) = ((cfg.size.w * scale).round() as i32, (cfg.size.h * scale).round() as i32);
  let mut rc = RECT { left: 0, top: 0, right: w, bottom: h };
  unsafe { AdjustWindowRectExForDpi(&mut rc, style, 0, 0, dpi) };
  let (ww, wh) = (rc.right - rc.left, rc.bottom - rc.top);
  let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
  let (x, y) = (((sw - ww) / 2).max(0), ((sh - wh) / 2).max(0));
  let title = wide(cfg.title);
  let hwnd = unsafe {
    CreateWindowExW(
      0,
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

  let scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
  let mut rt = Runtime::new(app, cfg.size, scale, Clock::real(), Box::new(WinClipboard::new(hwnd)));
  rt.event(Event::Appearance(appearance()));
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
  let window = Window {
    hwnd,
    rt,
    renderer: Renderer::new(),
    dib: None,
    scale,
    hidden: false,
    pace: Pace::new(refresh(hwnd)),
    monitor: unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) },
    trace: std::env::var_os("NIMBO_TRACE").is_some(),
    frames: 0,
    tracking: false,
    clicks: input::Clicks::default(),
    held: Vec::new(),
    high: None,
    cursor: Cursor::Arrow,
    min: cfg.min(),
    style,
    modal: 0,
    armed: None,
    target,
  };
  WINDOW.with(|w| *w.borrow_mut() = Some(window));
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
  true
}

/// Draws the frame due and applies a size the app asked for, before it and in it; returns how
/// long the loop may sleep (`None`: until a message). A size asked for in the frame draws
/// another through WM_SIZE, and the sleep follows that one: what it wants (an animation, a
/// timer) would otherwise wait for the next message.
fn settle(hwnd: HWND) -> Option<Duration> {
  apply_resize(hwnd);
  let wait = with(|w| w.tick()).flatten();
  match apply_resize(hwnd) {
    true => with(|w| w.wait()).flatten(),
    false => wait,
  }
}

/// Handles messages until WM_QUIT; between them draws the frames due and sleeps until the next
/// message, wake or timer.
fn run_loop(hwnd: HWND, timer: &Timer) {
  let mut msg: MSG = unsafe { std::mem::zeroed() };
  loop {
    while unsafe { PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) } != 0 {
      if msg.message == WM_QUIT {
        return;
      }

      if input::key_message(&msg) {
        continue;
      }

      unsafe {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
      }
    }

    if with(|w| w.rt.quit_requested()).unwrap_or(true) {
      unsafe { DestroyWindow(hwnd) };
      continue;
    }

    let wait = settle(hwnd);
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
    if unsafe { OpenClipboard(self.owner as HWND) } == 0 {
      return None;
    }

    let mut out = None;
    let data = unsafe { GetClipboardData(CF_TEXT_UTF16) };
    let p = unsafe { GlobalLock(data) }.cast::<u16>();
    if !p.is_null() {
      // SAFETY: the locked allocation holds `GlobalSize` bytes; another process wrote them, so
      // the NUL that ends CF_UNICODETEXT is looked for only within them.
      let units = unsafe { std::slice::from_raw_parts(p, GlobalSize(data) / 2) };
      out = Some(utf16_text(units));
      unsafe { GlobalUnlock(data) };
    }

    unsafe { CloseClipboard() };
    out
  }

  fn write(&mut self, s: &str) {
    if unsafe { OpenClipboard(self.owner as HWND) } == 0 {
      return;
    }

    let text = wide(s);
    let mem = unsafe { GlobalAlloc(GMEM_MOVEABLE, text.len() * 2) };
    let p = unsafe { GlobalLock(mem) }.cast::<u16>();
    if !p.is_null() {
      unsafe {
        std::ptr::copy_nonoverlapping(text.as_ptr(), p, text.len());
        GlobalUnlock(mem);
        EmptyClipboard();
        // The clipboard owns the memory once it takes it.
        if SetClipboardData(CF_TEXT_UTF16, mem).is_null() {
          GlobalFree(mem);
        }
      }
    }

    unsafe { CloseClipboard() };
  }
}

/// The text of UTF-16 `units` up to the first NUL, or all of them without one.
fn utf16_text(units: &[u16]) -> String {
  let n = units.iter().position(|&u| u == 0).unwrap_or(units.len());
  String::from_utf16_lossy(&units[..n])
}

// MARK: System

/// The OS's light or dark setting and accent: light until ported. TODO: `AppsUseLightTheme`
/// (registry, re-read on `WM_SETTINGCHANGE` "ImmersiveColorSet"), the accent from `UISettings`
/// or `DwmGetColorizationColor`, a dark title bar with `DWMWA_USE_IMMERSIVE_DARK_MODE`.
pub fn appearance() -> Appearance {
  Appearance::LIGHT
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
    let hwnd = create(&cfg, Box::new(Still));
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
    WINDOW.with(|w| w.borrow_mut().take());
  }

  /// Shift let go during a live resize, whose loop takes its key message: the modifiers come
  /// again as it ends, none for a window without the focus. The window is never shown.
  #[test]
  fn a_live_resize_ends_with_the_modifiers_held() {
    #[derive(Default)]
    struct Seen(std::rc::Rc<std::cell::Cell<Option<Mods>>>);
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
    let hwnd = create(&Config::new("mods", 200.0, 100.0), Box::new(app));
    with(|w| w.send(Event::Mods(Mods::SHIFT)));
    unsafe { SendMessageW(hwnd, WM_ENTERSIZEMOVE, 0, 0) };
    unsafe { SendMessageW(hwnd, WM_EXITSIZEMOVE, 0, 0) };
    assert_eq!(seen.get(), Some(Mods::NONE));

    unsafe { DestroyWindow(hwnd) };
    WINDOW.with(|w| w.borrow_mut().take());
  }

  /// A live move's timer follows a new display mode: set at a second's interval, it ticks at the
  /// display's once WM_DISPLAYCHANGE comes. The window is never shown.
  #[test]
  fn a_new_display_mode_sets_the_interval() {
    let cfg = Config::new("retime", 200.0, 100.0);
    let hwnd = create(&cfg, Box::new(Busy));
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
    WINDOW.with(|w| w.borrow_mut().take());
  }

  /// An open menu (the system menu, a context menu) runs a modal loop too: an animation goes on
  /// under it, and under a live move started from it, until the last one ends. The window is
  /// never shown.
  #[test]
  fn a_menu_keeps_animating() {
    let cfg = Config::new("menu", 200.0, 100.0);
    let hwnd = create(&cfg, Box::new(Busy));
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
    WINDOW.with(|w| w.borrow_mut().take());
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
    let hwnd = create(&Config::new("focus", 200.0, 100.0), Box::new(Seen(seen.clone())));
    unsafe {
      SendMessageW(hwnd, WM_SETFOCUS, 0, 0);
      SendMessageW(hwnd, WM_KILLFOCUS, 0, 0);
    }
    assert_eq!(*seen.borrow(), [true, false]);

    unsafe { DestroyWindow(hwnd) };
    WINDOW.with(|w| w.borrow_mut().take());
  }

  /// A waker another thread keeps past the window's end posts to no window: its handle may be
  /// another's by then.
  #[test]
  fn a_waker_outliving_its_window_posts_nowhere() {
    struct Still;
    impl App for Still {}

    let hwnd = create(&Config::new("waker", 200.0, 100.0), Box::new(Still));
    let (waker, target) = with(|w| (w.rt.waker().clone(), w.target.clone())).unwrap();
    assert_eq!(*target.lock().unwrap(), hwnd as isize);

    unsafe { DestroyWindow(hwnd) };
    assert_eq!(*target.lock().unwrap(), 0);

    waker.wake();
    WINDOW.with(|w| w.borrow_mut().take());
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
    let hwnd = create(&cfg, Box::new(Later::default()));
    let ticks = || timer_ticks(hwnd, 120);
    unsafe { SendMessageW(hwnd, WM_ENTERSIZEMOVE, 0, 0) };
    assert!(ticks() > 0, "the first frame is due");
    assert!(ticks() > 0, "the timer draws");
    assert_eq!(ticks(), 0, "idle");
    unsafe {
      SendMessageW(hwnd, WM_EXITSIZEMOVE, 0, 0);
      DestroyWindow(hwnd);
    }
    WINDOW.with(|w| w.borrow_mut().take());
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
    let hwnd = create(&Config::new("grow", 200.0, 100.0), Box::new(Grow(grow.clone())));
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
    WINDOW.with(|w| w.borrow_mut().take());
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
