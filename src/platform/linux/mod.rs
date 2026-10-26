// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Linux platform on GTK 3, loaded at run time as Sublime Text does ([dl]: no build-time C,
//! no binding crates). GTK opens the window on Wayland or X11 (GDK picks, `GDK_BACKEND` forces
//! one) with the title bar the desktop draws, or GTK's own where it draws none (GNOME on
//! Wayland); every pixel inside is ours. One GtkDrawingArea fills the window: the CPU frame is
//! drawn into a cairo image surface where its tiles changed, and only those rects go to GTK
//! (`queue_draw_area`), which paints them in its frame.
//!
//! Frames follow the GdkFrameClock: while the runtime wants frames a tick callback asks for the
//! layout phase, and the frame is built after GTK's layout, at the size it just gave the area,
//! so a live resize never stretches. Nothing runs while idle or minimized: timers are GLib
//! timeouts, a wake from another thread an idle source. Input is in [input] (keys in [keys]),
//! light or dark in [settings], the text stack in [text] (fontconfig, FreeType, HarfBuzz),
//! overlays in [overlay], file panels and drops in [files].
//! GTK 3 scales by whole numbers only.
//!
//! Windows open and close on demand while GTK's main loop runs ([app] has the loop, the tray,
//! the hotkeys and launch at login): each callback carries its window's id, set as the current
//! window for [with] while it runs.

mod app;
mod dbus;
mod dl;
mod files;
mod input;
mod keys;
mod overlay;
mod settings;
pub mod text;

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_ulong, c_void};
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub use self::app::{activity, autostart, focus, hotkey, open, paste, post, shell, tray};
use self::dl::{Lib, fns};
use self::input::Input;
pub use self::settings::appearance;
use crate::app::{App, Clipboard, Clock, Event, Runtime};
use crate::cpu::Renderer;
use crate::scene::Size;
use crate::ui::Cursor;
use crate::{Anchor, Config};

type P = *mut c_void;
type S = *const c_char;
/// A callback: an `extern "C" fn` of the signal's real signature, cast.
type Fp = *const c_void;
type Res<T> = Result<T, Box<dyn std::error::Error>>;

const TRUE: c_int = 1;
const FALSE: c_int = 0;
/// `G_CONNECT_AFTER`: the handler runs after the signal's other ones.
const AFTER: c_int = 1;
const WINDOW_TOPLEVEL: c_int = 0;
/// `CAIRO_FORMAT_ARGB32` and `RGB24` (32 bits, B G R A or x in memory, premultiplied, as the
/// CPU renderer writes) and `CAIRO_OPERATOR_SOURCE`.
const ARGB32: c_int = 0;
const RGB24: c_int = 1;
const SOURCE: c_int = 1;
/// `GDK_FRAME_CLOCK_PHASE_LAYOUT`.
const LAYOUT: c_int = 1 << 3;
/// `GDK_WINDOW_STATE_WITHDRAWN | GDK_WINDOW_STATE_ICONIFIED`: not shown.
const HIDDEN: c_uint = 1 | 2;
/// `GDK_SELECTION_CLIPBOARD`, a predefined atom.
const CLIPBOARD: usize = 69;
/// Frames while animating come at most this often, in µs (120 Hz), give or take [JITTER_US]
/// of the clock's jitter, which the next deadline makes up.
const FRAME_US: i64 = 8_333;
const JITTER_US: i64 = 1_500;

// GTK 3.24's headers; GTK 3's ABI is frozen. A symbol looked up in `libgtk-3.so.0` is found in
// what it links too: GDK, GLib, GObject, GIO, cairo.
fns!(Gtk {
  fn g_set_prgname(name: S);
  fn g_free(p: P);
  fn g_signal_connect_data(obj: P, signal: S, cb: Fp, data: P, free: Fp, flags: c_int)
    -> c_ulong;
  fn g_idle_add(f: Fp, data: P) -> c_uint;
  fn g_timeout_add(ms: c_uint, f: Fp, data: P) -> c_uint;
  fn g_source_remove(id: c_uint) -> c_int;
  fn g_unix_fd_add_full(priority: c_int, fd: c_int, cond: c_uint, f: Fp, data: P, free: Fp)
    -> c_uint;
  fn g_main_context_iteration(context: P, block: c_int) -> c_int;
  fn g_object_unref(obj: P);
  fn gdk_display_get_default() -> P;
  fn gdk_cursor_new_from_name(display: P, name: S) -> P;
  fn gdk_window_set_cursor(w: P, cursor: P);
  fn gdk_window_get_state(w: P) -> c_uint;
  fn gdk_frame_clock_request_phase(clock: P, phase: c_int);
  fn gdk_frame_clock_get_frame_time(clock: P) -> i64;
  fn cairo_image_surface_create(format: c_int, w: c_int, h: c_int) -> P;
  fn cairo_image_surface_get_data(s: P) -> *mut u8;
  fn cairo_image_surface_get_stride(s: P) -> c_int;
  fn cairo_surface_status(s: P) -> c_int;
  fn cairo_surface_flush(s: P);
  fn cairo_surface_mark_dirty(s: P);
  fn cairo_surface_set_device_scale(s: P, x: f64, y: f64);
  fn cairo_surface_destroy(s: P);
  fn cairo_set_source_surface(cr: P, s: P, x: f64, y: f64);
  fn cairo_set_operator(cr: P, op: c_int);
  fn cairo_paint(cr: P);
  fn gtk_init_check(argc: *mut c_int, argv: P) -> c_int;
  fn gtk_main();
  fn gtk_main_level() -> c_uint;
  fn gtk_main_quit();
  fn gtk_window_new(kind: c_int) -> P;
  fn gtk_window_set_title(w: P, title: S);
  fn gtk_window_set_default_size(w: P, width: c_int, height: c_int);
  fn gtk_window_set_resizable(w: P, on: c_int);
  fn gtk_window_resize(w: P, width: c_int, height: c_int);
  fn gtk_drawing_area_new() -> P;
  fn gtk_container_add(c: P, w: P);
  fn gtk_widget_set_size_request(w: P, width: c_int, height: c_int);
  fn gtk_widget_set_can_focus(w: P, on: c_int);
  fn gtk_widget_grab_focus(w: P);
  fn gtk_widget_add_events(w: P, mask: c_int);
  fn gtk_widget_show_all(w: P);
  fn gtk_window_present(w: P);
  fn gtk_window_is_active(w: P) -> c_int;
  fn gtk_widget_destroy(w: P);
  fn gtk_dialog_run(d: P) -> c_int;
  fn gtk_widget_hide(w: P);
  fn gtk_widget_queue_draw_area(w: P, x: c_int, y: c_int, width: c_int, height: c_int);
  fn gtk_widget_add_tick_callback(w: P, cb: Fp, data: P, free: Fp) -> c_uint;
  fn gtk_widget_get_allocated_width(w: P) -> c_int;
  fn gtk_widget_get_allocated_height(w: P) -> c_int;
  fn gtk_widget_get_scale_factor(w: P) -> c_int;
  fn gtk_widget_get_window(w: P) -> P;
  fn gtk_widget_get_frame_clock(w: P) -> P;
  fn gtk_clipboard_get(selection: usize) -> P;
  fn gtk_clipboard_set_text(c: P, text: S, len: c_int);
  fn gtk_clipboard_wait_for_text(c: P) -> *mut c_char;
  fn gtk_get_current_event() -> P;
  fn gdk_event_get_event_type(ev: P) -> c_int;
  fn gdk_event_free(ev: P);
  fn gtk_drag_dest_set(w: P, flags: c_int, targets: P, n: c_int, actions: c_int);
  fn gtk_drag_dest_find_target(w: P, ctx: P, list: P) -> P;
  fn gtk_drag_get_data(w: P, ctx: P, target: P, time: u32);
  fn gtk_drag_finish(ctx: P, ok: c_int, delete: c_int, time: u32);
  fn gdk_drag_status(ctx: P, action: c_int, time: u32);
  fn gtk_selection_data_get_data_with_length(sel: P, len: *mut c_int) -> *const u8;
});

/// `libgtk-3.so.0`, opened by each module for its own functions (the same handle each time).
fn lib() -> Option<Lib> {
  Lib::open(&[c"libgtk-3.so.0"])
}

fn load() -> Option<&'static Gtk> {
  static GTK: OnceLock<Option<Gtk>> = OnceLock::new();
  GTK.get_or_init(|| Gtk::load(&lib()?)).as_ref()
}

/// GTK's functions, once [run] loaded them.
fn gtk() -> &'static Gtk {
  load().expect("GTK is loaded before its callbacks run")
}

/// Connects `cb` to `obj`'s `signal`, with no data: [with] finds the window the event is for.
unsafe fn connect(obj: P, signal: &CStr, cb: Fp, flags: c_int) {
  unsafe { connect_id(obj, signal, cb, flags, 0) };
}

/// Connects `cb` to `obj`'s `signal` with window `id` as its data ([as_window]).
unsafe fn connect_id(obj: P, signal: &CStr, cb: Fp, flags: c_int, id: u64) {
  let data = id as usize as P;
  unsafe { (gtk().g_signal_connect_data)(obj, signal.as_ptr(), cb, data, null(), flags) };
}

/// The performance cores: Intel hybrid CPUs list theirs in `/sys/devices/cpu_core/cpus`
/// (`0-7`); elsewhere `None`, all cores alike.
pub fn perf_cores() -> Option<usize> {
  let list = std::fs::read_to_string("/sys/devices/cpu_core/cpus").ok()?;
  let n = list
    .trim()
    .split(',')
    .filter_map(|r| match r.split_once('-') {
      Some((a, b)) => Some(b.parse::<usize>().ok()? + 1 - a.parse::<usize>().ok()?),
      None => r.parse::<usize>().ok().map(|_| 1),
    })
    .sum();
  (n > 0).then_some(n)
}

// MARK: Window

/// A window and everything its callbacks share, in [WINS].
struct Win {
  id: u64,
  rt: Runtime<Box<dyn App>>,
  renderer: Renderer,
  window: P,
  area: P,
  /// The frame shown, a cairo image surface, also in the window's [Slot]: painted even while
  /// the window is busy.
  surface: Rc<Cell<P>>,
  /// The tile hashes of the frame in `surface`, its size in pixels and its scale.
  tiles: Vec<u64>,
  shown: (usize, usize),
  shown_scale: f32,
  /// A tick callback is on: frames are wanted. The frame clock's time the next frame is due
  /// ([paced]), µs.
  ticking: bool,
  next_tick: i64,
  /// The GLib timeout for the runtime's next timer, and when it fires.
  timer: Option<(c_uint, Instant)>,
  /// The user can resize it ([Config::resizable]) down to `min`, which a size the app asks for
  /// lowers.
  resizable: bool,
  min: Size,
  overlay: Option<Anchor>,
  input: Input,
  cursor: Option<Cursor>,
  cursors: Vec<(Cursor, P)>,
  /// GTK painted our surface in this frame: logged after it.
  painted: bool,
  frames: u64,
  started: Instant,
  /// Logs every frame presented (`NIMBO_TRACE`).
  trace: bool,
}

impl Drop for Win {
  /// Lets go of what GTK gave the window: its timeout, cursors and the last frame's surface.
  fn drop(&mut self) {
    let g = gtk();
    self.arm(None);
    for (_, c) in self.cursors.drain(..).filter(|(_, c)| !c.is_null()) {
      unsafe { (g.g_object_unref)(c) };
    }

    let s = self.surface.replace(null_mut());
    if !s.is_null() {
      unsafe { (g.cairo_surface_destroy)(s) };
    }
  }
}

/// An open window: its GTK window and area, its state, and the frame it shows.
struct Slot {
  id: u64,
  window: P,
  area: P,
  win: Rc<RefCell<Win>>,
  surface: Rc<Cell<P>>,
  /// Shown once: its frame clock is followed and its input set up.
  shown: bool,
  overlay: Option<Anchor>,
}

thread_local! {
  static WINS: RefCell<Vec<Slot>> = const { RefCell::new(Vec::new()) };
  /// Closed windows still in use up the stack ([close]).
  static DYING: RefCell<Vec<Slot>> = const { RefCell::new(Vec::new()) };
  /// The window the callback running is for; 0 outside one (an idle source of input's).
  static CURRENT: Cell<u64> = const { Cell::new(0) };
  static NEXT: Cell<u64> = const { Cell::new(0) };
  /// The loop ends (the app quit), set without a window.
  static QUITTING: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f` with window `id` as the current one ([with]).
fn as_window<R>(id: u64, f: impl FnOnce() -> R) -> R {
  let prev = CURRENT.replace(id);
  let r = f();
  CURRENT.set(prev);
  r
}

/// The window id a callback got as its data.
fn id_of(data: P) -> u64 {
  data as usize as u64
}

/// [with] the window a callback's `data` names as the current one.
fn with_data<R>(data: P, none: R, f: impl FnOnce(&mut Win) -> R) -> R {
  as_window(id_of(data), || with(none, f))
}

/// The current window: the callback's, or outside one the active window (else the first).
fn current() -> Option<Rc<RefCell<Win>>> {
  let g = gtk();
  WINS.with_borrow(|ws| {
    let slot = match CURRENT.get() {
      0 => ws.iter().find(|s| unsafe { (g.gtk_window_is_active)(s.window) } != 0).or(ws.first()),
      id => ws.iter().find(|s| s.id == id),
    };

    slot.map(|s| s.win.clone())
  })
}

/// Runs `f` on the current window, then [Win::sync]; `none` when there is none or it is busy (a
/// GTK call of ours ran a nested main loop, as a paste waiting for its owner does, and that
/// dispatched this callback). Input that came meanwhile runs after ([input::replay]).
fn with<R>(none: R, f: impl FnOnce(&mut Win) -> R) -> R {
  let Some(win) = current() else { return none };
  let Ok(mut w) = win.try_borrow_mut() else { return none };
  let r = f(&mut w);
  w.sync();
  drop(w);
  input::replay();
  r
}

/// Runs `f` on every window not busy; whether one was.
fn each(mut f: impl FnMut(&mut Win)) -> bool {
  let all: Vec<_> = WINS.with_borrow(|ws| ws.iter().map(|s| (s.id, s.win.clone())).collect());
  let mut missed = false;
  for (id, win) in all {
    as_window(id, || match win.try_borrow_mut() {
      Ok(mut w) => {
        f(&mut w);
        w.sync();
      }
      Err(_) => missed = true,
    });
  }

  missed
}

/// Whether the current window is in use further up the stack.
fn busy() -> bool {
  current().is_some_and(|w| w.try_borrow_mut().is_err())
}

/// Whether window `id` is in use up the stack (none is when it is gone).
fn window_busy(id: u64) -> bool {
  slot(id, |s| s.win.try_borrow_mut().is_err()).unwrap_or(false)
}

/// Window `id`'s slot field `f`.
fn slot<R>(id: u64, f: impl FnOnce(&mut Slot) -> R) -> Option<R> {
  WINS.with_borrow_mut(|ws| ws.iter_mut().find(|s| s.id == id).map(f))
}

/// Ends the main loop once, or keeps it from starting.
fn quit() {
  let g = gtk();
  if !QUITTING.replace(true) && unsafe { (g.gtk_main_level)() } > 0 {
    unsafe { (g.gtk_main_quit)() };
  }
}

/// Loads GTK and opens the display, before any window: `id` is the Wayland app id and the X11
/// `WM_CLASS`; then follows the OS's light or dark.
fn init(id: &str) -> Res<()> {
  let g = load().ok_or("GTK 3 is needed: libgtk-3.so.0 did not load")?;
  QUITTING.set(false);
  // GDK's X11 backend probes GLX as it opens the display, loading Mesa (and LLVM with
  // llvmpipe): about 34 MB more for nothing, every pixel being ours.
  if std::env::var_os("GDK_GL").is_none() {
    // SAFETY: before GTK, GIO and the renderer start their threads; an app that reads the
    // environment from threads of its own sets `GDK_GL` itself before `run` (then left alone).
    unsafe { std::env::set_var("GDK_GL", "disable") };
  }

  let id = CString::new(id)?;
  // SAFETY: GTK's calls, on the thread that runs its loop.
  unsafe {
    (g.g_set_prgname)(id.as_ptr());
    if (g.gtk_init_check)(null_mut(), null_mut()) == FALSE {
      return Err("GTK cannot open a display (WAYLAND_DISPLAY or DISPLAY)".into());
    }
  }

  settings::watch();
  Ok(())
}

/// Runs GTK's main loop until [quit], unless it was asked for already.
fn main_loop() {
  if !QUITTING.get() {
    unsafe { (gtk().gtk_main)() };
  }
}

/// Opens a window running `app`, shown unless `cfg.hidden`; its id.
fn open(cfg: &Config, app: Box<dyn App>) -> Result<u64, std::ffi::NulError> {
  let started = Instant::now();
  let g = gtk();
  let id = NEXT.get() + 1;
  NEXT.set(id);
  let title = CString::new(cfg.title)?;
  let size = Size::new(cfg.size.w.round().max(1.0), cfg.size.h.round().max(1.0));
  let (w, h) = (size.w as c_int, size.h as c_int);
  let min = cfg.min();
  // SAFETY: GTK's calls, on the thread that runs its loop; each callback has its signal's
  // signature.
  let (window, area) = unsafe {
    let window = (g.gtk_window_new)(cfg.overlay.map_or(WINDOW_TOPLEVEL, |_| overlay::kind()));
    if let Some(anchor) = cfg.overlay {
      overlay::setup(window, anchor);
    }

    let area = (g.gtk_drawing_area_new)();
    (g.gtk_window_set_title)(window, title.as_ptr());
    match cfg.resizable {
      true => {
        (g.gtk_window_set_default_size)(window, w, h);
        (g.gtk_widget_set_size_request)(area, min.w.ceil() as c_int, min.h.ceil() as c_int);
      }
      false => {
        (g.gtk_window_set_resizable)(window, FALSE);
        (g.gtk_widget_set_size_request)(area, w, h);
      }
    }

    (g.gtk_widget_set_can_focus)(area, TRUE);
    (g.gtk_widget_add_events)(area, input::EVENTS);
    (g.gtk_container_add)(window, area);
    (g.gtk_widget_grab_focus)(area);
    connect_id(window, c"delete-event", on_delete as Fp, 0, id);
    connect_id(window, c"window-state-event", on_state as Fp, 0, id);
    connect_id(area, c"draw", on_draw as Fp, 0, id);
    connect_id(area, c"event", on_event as Fp, 0, id);
    files::dest(area, id);
    (window, area)
  };

  let clip = Box::new(Clip(unsafe { (g.gtk_clipboard_get)(CLIPBOARD) }));
  let mut rt = Runtime::new(app, size, 1.0, Clock::real(), clip);
  let mut renderer = Renderer::new();
  renderer.transparent = cfg.overlay.is_some();
  rt.event(Event::Appearance(appearance()));
  // A wake from any thread: an idle source on the main context, which GLib wakes.
  rt.waker().set_notify(Box::new(move || {
    unsafe { (gtk().g_idle_add)(on_wake as Fp, id as usize as P) };
  }));

  let surface = Rc::new(Cell::new(null_mut()));
  let win = Win {
    id,
    rt,
    renderer,
    window,
    area,
    surface: surface.clone(),
    tiles: Vec::new(),
    shown: (0, 0),
    shown_scale: 0.0,
    ticking: false,
    next_tick: 0,
    timer: None,
    resizable: cfg.resizable,
    min,
    overlay: cfg.overlay,
    input: Input::new(id),
    cursor: None,
    cursors: Vec::new(),
    painted: false,
    frames: 0,
    started,
    trace: std::env::var_os("NIMBO_TRACE").is_some(),
  };
  let win = Rc::new(RefCell::new(win));
  WINS.with_borrow_mut(|ws| {
    ws.push(Slot { id, window, area, win, surface, shown: false, overlay: cfg.overlay })
  });
  if !cfg.hidden {
    front(id);
  }

  Ok(id)
}

/// Shows window `id` in front, an overlay at its anchor ([overlay]); the first time, follows its
/// frame clock and sets its input up (shown, the window is realized: its frame clock and the
/// area's GdkWindow exist).
fn front(id: u64) {
  let g = gtk();
  let Some((window, area, first, overlay, size)) = slot(id, |s| {
    let first = !std::mem::replace(&mut s.shown, true);
    (s.window, s.area, first, s.overlay, s.win.try_borrow().map(|w| w.rt.size))
  }) else {
    return;
  };
  if let (Some(anchor), Ok(size)) = (overlay, size) {
    overlay::place(window, size, anchor, true);
  }

  // SAFETY: GTK's calls on a window of ours.
  unsafe {
    if first {
      (g.gtk_widget_show_all)(window);
      let clock = (g.gtk_widget_get_frame_clock)(window);
      connect_id(clock, c"layout", on_layout as Fp, AFTER, id);
      connect_id(clock, c"after-paint", on_after_paint as Fp, 0, id);
      as_window(id, || with((), |w| w.input.realized((g.gtk_widget_get_window)(area))));
      // Shown for the first time, it comes in front as any new window: presenting it as well
      // has Wayland compositors (sway) place it twice, the pointer losing it.
      return;
    }

    match overlay {
      Some(_) => {
        (g.gtk_widget_show_all)(window);
        overlay::raise(window);
      }
      None => (g.gtk_window_present)(window),
    }
  }
}

/// Hides window `id`: withdrawn, it draws no frames until it shows again.
fn hide(id: u64) {
  if let Some(window) = slot(id, |s| s.window) {
    unsafe { (gtk().gtk_widget_hide)(window) };
  }
}

/// Window `id` goes: hidden at once; its runtime and GTK window once no call up the stack has
/// them (`defer`: a callback further up, or one waiting in a nested main loop for a paste,
/// never meets them gone; [on_destroy]) or at once.
fn close(id: u64, defer: bool) -> bool {
  let Some(s) = WINS.with_borrow_mut(|ws| Some(ws.remove(ws.iter().position(|s| s.id == id)?)))
  else {
    return false;
  };

  if let Ok(w) = s.win.try_borrow()
    && w.trace
  {
    eprintln!("nimbo: closed after {} frames", w.frames);
  }

  let g = gtk();
  match defer {
    true => {
      unsafe { (g.gtk_widget_hide)(s.window) };
      DYING.with_borrow_mut(|d| d.push(s));
      unsafe { (g.g_idle_add)(on_destroy as Fp, null_mut()) };
    }
    false => {
      let window = s.window;
      drop(s);
      unsafe { (g.gtk_widget_destroy)(window) };
    }
  }

  true
}

impl Win {
  /// The area's size in points, and the scale.
  fn size(&self) -> (Size, f32) {
    let g = gtk();
    let (w, h, scale) = unsafe {
      let w = (g.gtk_widget_get_allocated_width)(self.area);
      let h = (g.gtk_widget_get_allocated_height)(self.area);
      (w, h, (g.gtk_widget_get_scale_factor)(self.area).max(1))
    };

    (Size::new(w.max(1) as f32, h.max(1) as f32), scale as f32)
  }

  /// The surface shown is not of the area's size and scale, or there is none.
  fn stale(&self) -> bool {
    let (size, scale) = self.size();
    let px = Renderer::pixel_size(size, scale);
    self.surface.get().is_null() || (px, scale) != (self.shown, self.shown_scale)
  }

  /// Not minimized or withdrawn (as X11 tells; on Wayland the compositor holds the clock).
  fn visible(&self) -> bool {
    let g = gtk();
    let win = unsafe { (g.gtk_widget_get_window)(self.window) };
    !win.is_null() && unsafe { (g.gdk_window_get_state)(win) } & HIDDEN == 0
  }

  /// Feeds an event to the runtime; returns whether it was used.
  fn send(&mut self, ev: Event) -> bool {
    let (size, scale) = self.size();
    self.rt.resize(size, scale);
    self.rt.event(ev)
  }

  /// After every callback: the size and the file panel the app asked for ([files]), the
  /// cursor, quitting, and what runs the loop next ([next]).
  fn sync(&mut self) {
    let g = gtk();
    self.drain_ime();
    self.sync_ime();
    self.apply_resize();
    if let Some(d) = self.rt.take_file_dialog()
      && let Err(e) = files::show(self.id, self.window, &d)
    {
      eprintln!("nimbo: {e}");
      self.send(Event::FileDialog(None));
    }

    self.sync_cursor();
    if self.rt.quit_requested() {
      quit();
    }

    let visible = self.visible();
    let (tick, timer) = next(visible, visible && self.rt.needs_frame(), self.rt.due_in());
    if tick && !self.ticking {
      self.ticking = true;
      let (area, data) = (self.area, self.id as usize as P);
      unsafe { (g.gtk_widget_add_tick_callback)(area, on_tick as Fp, data, null()) };
    }

    self.arm(timer.map(|d| Instant::now() + d));
  }

  /// Sets the timeout to fire `at`, unless it already does within a millisecond.
  fn arm(&mut self, at: Option<Instant>) {
    let near = |a: Instant, b: Instant| a.max(b) - a.min(b) < Duration::from_millis(1);
    if let (Some((_, t)), Some(a)) = (self.timer, at)
      && near(t, a)
    {
      return;
    }

    let g = gtk();
    if let Some((id, _)) = self.timer.take() {
      unsafe { (g.g_source_remove)(id) };
    }

    let Some(at) = at else { return };
    let ms = at.saturating_duration_since(Instant::now()).as_micros().div_ceil(1000);
    let data = self.id as usize as P;
    let id = unsafe { (g.g_timeout_add)(ms as c_uint, on_timer as Fp, data) };
    self.timer = Some((id, at));
  }

  /// Builds a frame at the area's size and draws the tiles that changed into the surface;
  /// `queue` hands their rects to GTK for its paint (not from the paint itself).
  fn render(&mut self, queue: bool) {
    let g = gtk();
    let (size, scale) = self.size();
    self.rt.resize(size, scale);
    self.rt.frame();
    let r = &mut self.renderer;
    r.sync(&mut self.rt.text, &mut self.rt.images);
    r.prepare(&self.rt.scene, size, scale);
    let (w, h) = r.size();
    if ((w, h), scale) != (self.shown, self.shown_scale) || self.surface.get().is_null() {
      // Out of memory: the frame waits for the next.
      if !new_surface(&self.surface, w, h, scale, r.transparent) {
        return;
      }

      (self.shown, self.shown_scale) = ((w, h), scale);
      self.tiles.clear();
    }

    let rects = r.damage(&self.tiles);
    if !rects.is_empty() {
      let s = self.surface.get();
      // SAFETY: our surface's pixels, `stride × h` of them, B G R x as the renderer writes.
      unsafe {
        (g.cairo_surface_flush)(s);
        let stride = (g.cairo_image_surface_get_stride)(s) as usize / 4;
        let data = (g.cairo_image_surface_get_data)(s).cast::<u32>();
        r.draw(&self.rt.scene, std::slice::from_raw_parts_mut(data, stride * h), stride, &rects);
        (g.cairo_surface_mark_dirty)(s);
      }

      self.tiles.clear();
      self.tiles.extend_from_slice(r.tiles());
    }

    if queue {
      for rc in &rects {
        let (x, y) = ((rc.x0 as f32 / scale) as c_int, (rc.y0 as f32 / scale) as c_int);
        let x1 = (rc.x1 as f32 / scale).ceil() as c_int;
        let y1 = (rc.y1 as f32 / scale).ceil() as c_int;
        unsafe { (g.gtk_widget_queue_draw_area)(self.area, x, y, x1 - x, y1 - y) };
      }
    }

    self.apply_resize();
  }

  /// Takes the size the app asked for ([Runtime::take_resize]): a resizable window is resized
  /// to it (GTK leaves its own title bar out of the size), its least size lowered with it; a
  /// fixed one gets it as the area's size, which GTK fits the window to.
  fn apply_resize(&mut self) {
    let Some(size) = self.rt.take_resize() else { return };
    let g = gtk();
    let (w, h) = (size.w.round().max(1.0) as c_int, size.h.round().max(1.0) as c_int);
    unsafe {
      match self.resizable {
        true => {
          if self.min.w > size.w || self.min.h > size.h {
            self.min = Size::new(self.min.w.min(size.w), self.min.h.min(size.h));
            let (mw, mh) = (self.min.w.ceil() as c_int, self.min.h.ceil() as c_int);
            (g.gtk_widget_set_size_request)(self.area, mw, mh);
          }

          (g.gtk_window_resize)(self.window, w, h);
        }
        false => (g.gtk_widget_set_size_request)(self.area, w, h),
      }
    }

    if let Some(anchor) = self.overlay {
      overlay::place(self.window, size, anchor, false);
    }

    self.rt.redraw();
  }

  /// Shows the runtime's cursor over the area: the theme's, by its CSS name.
  fn sync_cursor(&mut self) {
    let c = self.rt.cursor();
    let g = gtk();
    let win = unsafe { (g.gtk_widget_get_window)(self.area) };
    if self.cursor == Some(c) || win.is_null() {
      return;
    }

    self.cursor = Some(c);
    let cursor = match self.cursors.iter().find(|(k, _)| *k == c) {
      Some((_, p)) => *p,
      None => {
        let p = unsafe { (g.gdk_cursor_new_from_name)((g.gdk_display_get_default)(), name(c)) };
        self.cursors.push((c, p));
        p
      }
    };

    unsafe { (g.gdk_window_set_cursor)(win, cursor) };
  }
}

/// A cursor's CSS name, as GDK finds it in the theme.
fn name(c: Cursor) -> S {
  let n = match c {
    Cursor::Inherit | Cursor::Arrow => c"default",
    Cursor::Text => c"text",
    Cursor::Pointer => c"pointer",
    Cursor::ResizeX => c"col-resize",
    Cursor::ResizeY => c"row-resize",
    Cursor::Crosshair => c"crosshair",
    Cursor::Grab => c"grab",
    Cursor::Grabbing => c"grabbing",
    Cursor::NotAllowed => c"not-allowed",
  };

  n.as_ptr()
}

/// A new surface of `w × h` pixels at `scale` in `cell`, with `alpha` or not, the old one
/// dropped; false when cairo couldn't make one (its error surface has no pixels).
fn new_surface(cell: &Cell<P>, w: usize, h: usize, scale: f32, alpha: bool) -> bool {
  let g = gtk();
  // SAFETY: a new surface, ours until the next one.
  unsafe {
    let format = if alpha { ARGB32 } else { RGB24 };
    let s = (g.cairo_image_surface_create)(format, w as c_int, h as c_int);
    if (g.cairo_surface_status)(s) != 0 {
      (g.cairo_surface_destroy)(s);
      return false;
    }

    (g.cairo_surface_set_device_scale)(s, scale as f64, scale as f64);
    let old = cell.replace(s);
    if !old.is_null() {
      (g.cairo_surface_destroy)(old);
    }
  }

  true
}

/// Whether a frame at `now` (µs) keeps to 120 Hz: due at `next` give or take the clock's
/// jitter, the next one due a frame later, so frames slightly early on a faster display are
/// made up by one left out; after a pause the schedule starts again from `now`.
fn paced(now: i64, next: &mut i64) -> bool {
  if now < *next - JITTER_US {
    return false;
  }

  *next = if now > *next + FRAME_US { now + FRAME_US } else { *next + FRAME_US };
  true
}

/// What runs the loop next: the tick while frames are `wanted`, else a timeout for the next
/// timer; neither while the window is hidden, where a timer that came due couldn't draw (it
/// would fire again at once) and the window's showing again syncs.
fn next(visible: bool, wanted: bool, due_in: Option<Duration>) -> (bool, Option<Duration>) {
  (wanted, due_in.filter(|_| visible && !wanted))
}

// MARK: Callbacks

/// The frame clock's update phase while frames are wanted: asks for the layout phase, after
/// which the frame is built ([on_layout]), at most at 120 Hz ([paced]); removed once none are.
unsafe extern "C" fn on_tick(_: P, clock: P, data: P) -> c_int {
  with_data(data, TRUE, |w| {
    let g = gtk();
    w.ticking = w.rt.needs_frame() && w.visible();
    let now = unsafe { (g.gdk_frame_clock_get_frame_time)(clock) };
    if w.ticking && paced(now, &mut w.next_tick) {
      unsafe { (g.gdk_frame_clock_request_phase)(clock, LAYOUT) };
    }

    w.ticking as c_int
  })
}

/// After GTK's layout: the frame, at the size GTK just gave the area, when one is wanted or
/// that size is new.
unsafe extern "C" fn on_layout(_: P, data: P) {
  with_data(data, (), |w| {
    if w.rt.needs_frame() || w.stale() {
      w.render(true);
    }
  });
}

/// GTK's paint: our surface where GTK repaints (the rects queued, or all of it after an expose
/// or a resize), built first if it is stale (the first frame).
unsafe extern "C" fn on_draw(_: P, cr: P, data: P) -> c_int {
  with_data(data, (), |w| {
    if w.stale() {
      w.render(false);
    }

    w.painted = true;
  });

  let (g, s) = (gtk(), slot(id_of(data), |s| s.surface.get()).unwrap_or(null_mut()));
  if s.is_null() {
    return FALSE;
  }

  // SAFETY: GTK's context for this paint, clipped to what it repaints; our surface.
  unsafe {
    (g.cairo_set_operator)(cr, SOURCE);
    (g.cairo_set_source_surface)(cr, s, 0.0, 0.0);
    (g.cairo_paint)(cr);
  }

  TRUE
}

/// After GDK painted (and on Wayland committed) the frame: counted, and logged with
/// `NIMBO_TRACE` as `nimbo: presented <unix ms> (+<ms since start>) <w>x<h>`.
unsafe extern "C" fn on_after_paint(_: P, data: P) {
  with_data(data, (), |w| {
    if !std::mem::take(&mut w.painted) {
      return;
    }

    w.frames += 1;
    if w.trace {
      let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
      let since = w.started.elapsed().as_secs_f64() * 1e3;
      let (pw, ph) = w.shown;
      eprintln!("nimbo: presented {:.3} (+{since:.1} ms) {pw}x{ph}", t.as_secs_f64() * 1e3);
    }
  });
}

/// The runtime's next timer is due; while the window is busy, it fires again after its delay.
unsafe extern "C" fn on_timer(data: P) -> c_int {
  as_window(id_of(data), || {
    with(busy() as c_int, |w| {
      w.timer = None;
      FALSE
    })
  })
}

/// A wake from another thread; while the window is busy, the sync after the call that holds it
/// sees the wake.
unsafe extern "C" fn on_wake(data: P) -> c_int {
  with_data(data, (), |_| {});
  FALSE
}

/// Minimized or shown again: frames stop or go on.
unsafe extern "C" fn on_state(_: P, _: P, data: P) -> c_int {
  with_data(data, (), |_| {});
  FALSE
}

/// The area's input, to [input] as the window's.
unsafe extern "C" fn on_event(area: P, ev: P, data: P) -> c_int {
  as_window(id_of(data), || unsafe { input::on_event(area, ev, null_mut()) })
}

/// The window's close button: it closes through the shell (its close callback runs), destroyed
/// after this callback.
unsafe extern "C" fn on_delete(_: P, _: P, data: P) -> c_int {
  shell::close(id_of(data));
  TRUE
}

/// The closed windows still waiting go now: the loop has ended, nothing holds them.
fn reap() {
  for s in DYING.take() {
    let window = s.window;
    drop(s);
    unsafe { (gtk().gtk_widget_destroy)(window) };
  }
}

/// Closed windows go once nothing up the stack holds them: their runtime, then their GTK
/// window; the rest are tried again a moment later.
unsafe extern "C" fn on_destroy(_: P) -> c_int {
  let free = |s: &Slot| Rc::strong_count(&s.win) == 1 && s.win.try_borrow_mut().is_ok();
  let done: Vec<Slot> = DYING.with_borrow_mut(|d| d.extract_if(.., |s| free(s)).collect());
  for s in done {
    let window = s.window;
    drop(s);
    unsafe { (gtk().gtk_widget_destroy)(window) };
  }

  if DYING.with_borrow(|d| !d.is_empty()) {
    unsafe { (gtk().g_timeout_add)(50, on_destroy as Fp, null_mut()) };
  }

  FALSE
}

// MARK: Clipboard

/// The CLIPBOARD selection as GTK keeps it: GTK serves our text to readers; a paste waits for
/// the owner in a nested main loop, whose callbacks find the window busy.
struct Clip(P);

impl Clipboard for Clip {
  fn read(&self) -> Option<String> {
    let g = gtk();
    let s = unsafe { (g.gtk_clipboard_wait_for_text)(self.0) };
    if s.is_null() {
      return None;
    }

    let text = unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned();
    unsafe { (g.g_free)(s.cast()) };
    Some(text)
  }

  fn write(&mut self, s: &str) {
    let len = s.len().min(c_int::MAX as usize) as c_int;
    unsafe { (gtk().gtk_clipboard_set_text)(self.0, s.as_ptr().cast(), len) };
  }
}

/// The system clipboard outside a window, GTK's CLIPBOARD selection: on the main thread while
/// the app runs (GTK initialized), `None` elsewhere.
pub fn clipboard() -> Option<impl Clipboard> {
  if !app::shell::running() {
    return None;
  }

  Some(Clip(unsafe { (load()?.gtk_clipboard_get)(CLIPBOARD) }))
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Hidden, a timer that came due waits for the window to show (no timeout firing at once
  /// again and again); shown, frames tick, else the timer wakes the loop.
  #[test]
  fn nothing_runs_while_hidden() {
    let (due, later) = (Some(Duration::ZERO), Some(Duration::from_millis(500)));
    assert_eq!(next(false, false, due), (false, None));
    assert_eq!(next(true, true, due), (true, None));
    assert_eq!(next(true, false, later), (false, later));
    assert_eq!(next(true, false, None), (false, None));
  }

  /// A second of a display's ticks: 120 Hz keeps every one (with jitter too), faster ones at
  /// most 120.
  #[test]
  fn frames_keep_to_120_hz() {
    let frames = |hz: i64, jitter: i64| {
      let mut next = 0;
      let ticks = (0..hz).map(|i| i * 1_000_000 / hz + if i % 2 == 0 { jitter } else { -jitter });
      ticks.filter(|&t| paced(t, &mut next)).count()
    };
    assert_eq!(frames(120, 0), 120);
    assert_eq!(frames(120, 800), 120);
    assert_eq!(frames(60, 0), 60);
    for hz in [125, 144, 165, 240, 280] {
      assert!((100..=121).contains(&frames(hz, 0)), "{hz} Hz: {}", frames(hz, 0));
    }
  }

  #[test]
  fn perf_cores_count_ranges() {
    assert!(perf_cores().is_none_or(|n| n >= 1));
  }
}
