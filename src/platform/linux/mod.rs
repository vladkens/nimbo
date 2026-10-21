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
//! the text stack in [text] (fontconfig, FreeType, HarfBuzz). GTK 3 scales by whole numbers
//! only.

mod dl;
mod input;
mod keys;
pub mod text;

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_ulong, c_void};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use self::dl::{Lib, fns};
use self::input::Input;
use crate::Config;
use crate::app::{App, Appearance, Clipboard, Clock, Event, Runtime};
use crate::cpu::{Pixels, Renderer};
use crate::scene::Size;
use crate::ui::Cursor;

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
/// `CAIRO_FORMAT_RGB24` (32 bits, B G R x in memory, as the CPU renderer writes) and
/// `CAIRO_OPERATOR_SOURCE`.
const RGB24: c_int = 1;
const SOURCE: c_int = 1;
/// `GDK_FRAME_CLOCK_PHASE_LAYOUT`.
const LAYOUT: c_int = 1 << 3;
/// `GDK_WINDOW_STATE_WITHDRAWN | GDK_WINDOW_STATE_ICONIFIED`: not shown.
const HIDDEN: c_uint = 1 | 2;
/// `GDK_SELECTION_CLIPBOARD`, a predefined atom.
const CLIPBOARD: usize = 69;

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
  fn gdk_display_get_default() -> P;
  fn gdk_cursor_new_from_name(display: P, name: S) -> P;
  fn gdk_window_set_cursor(w: P, cursor: P);
  fn gdk_window_get_state(w: P) -> c_uint;
  fn gdk_frame_clock_request_phase(clock: P, phase: c_int);
  fn cairo_image_surface_create(format: c_int, w: c_int, h: c_int) -> P;
  fn cairo_image_surface_get_data(s: P) -> *mut u8;
  fn cairo_image_surface_get_stride(s: P) -> c_int;
  fn cairo_surface_flush(s: P);
  fn cairo_surface_mark_dirty(s: P);
  fn cairo_surface_set_device_scale(s: P, x: f64, y: f64);
  fn cairo_surface_destroy(s: P);
  fn cairo_set_source_surface(cr: P, s: P, x: f64, y: f64);
  fn cairo_set_operator(cr: P, op: c_int);
  fn cairo_paint(cr: P);
  fn gtk_init_check(argc: *mut c_int, argv: P) -> c_int;
  fn gtk_main();
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
  fn gtk_widget_destroy(w: P);
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

/// Connects `cb` to `obj`'s `signal`, with no data: the callbacks find the window in [WIN].
unsafe fn connect(obj: P, signal: &CStr, cb: Fp, flags: c_int) {
  unsafe { (gtk().g_signal_connect_data)(obj, signal.as_ptr(), cb, null_mut(), null(), flags) };
}

/// The OS's light or dark setting and accent: light, until the settings are read.
pub fn appearance() -> Appearance {
  Appearance::LIGHT
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

/// The window and everything its callbacks share, in [WIN].
struct Win {
  rt: Runtime<Box<dyn App>>,
  renderer: Renderer,
  window: P,
  area: P,
  /// The tile hashes of the frame in [SURFACE], and its size in pixels.
  tiles: Vec<u64>,
  shown: (usize, usize),
  /// A tick callback is on: frames are wanted.
  ticking: bool,
  /// The GLib timeout for the runtime's next timer, and when it fires.
  timer: Option<(c_uint, Instant)>,
  /// The user can resize it ([Config::resizable]) down to `min`, which a size the app asks for
  /// lowers.
  resizable: bool,
  min: Size,
  input: Input,
  cursor: Option<Cursor>,
  cursors: Vec<(Cursor, P)>,
  /// GTK painted our surface in this frame: logged after it.
  painted: bool,
  frames: u64,
  started: Instant,
  /// Logs every frame presented (`NIMBO_TRACE`), and every event (`NIMBO_TRACE=2`).
  trace: bool,
  trace_events: bool,
  quit: bool,
  destroyed: bool,
}

thread_local! {
  static WIN: RefCell<Option<Win>> = const { RefCell::new(None) };
  /// The frame shown, a cairo image surface: painted even while [WIN] is busy.
  static SURFACE: Cell<P> = const { Cell::new(null_mut()) };
}

/// Runs `f` on the window, then [Win::sync]; `none` when there is no window or it is busy (a
/// GTK call of ours ran a nested main loop, as a paste waiting for its owner does, and that
/// dispatched this callback). Input that came meanwhile runs after ([input::replay]).
fn with<R>(none: R, f: impl FnOnce(&mut Win) -> R) -> R {
  let r = WIN.with(|c| {
    let mut g = c.try_borrow_mut().ok()?;
    let w = g.as_mut()?;
    let r = f(w);
    w.sync();
    Some(r)
  });

  match r {
    Some(r) => {
      input::replay();
      r
    }
    None => none,
  }
}

/// Whether [WIN] is in use further up the stack.
fn busy() -> bool {
  WIN.with(|c| c.try_borrow_mut().is_err())
}

/// Opens the app's window and runs GTK's main loop until it closes or the app quits.
pub fn run(cfg: &Config, app: Box<dyn App>) {
  if let Err(e) = open(cfg, app) {
    eprintln!("nimbo: {e}");
    std::process::exit(1);
  }
}

fn open(cfg: &Config, app: Box<dyn App>) -> Res<()> {
  let started = Instant::now();
  let g = load().ok_or("GTK 3 is needed: libgtk-3.so.0 did not load")?;
  // GDK's X11 backend probes GLX as it opens the display, loading Mesa (and LLVM with
  // llvmpipe): about 34 MB more for nothing, every pixel being ours.
  if std::env::var_os("GDK_GL").is_none() {
    // SAFETY: before GTK and the renderer start their threads.
    unsafe { std::env::set_var("GDK_GL", "disable") };
  }

  let exe = std::env::current_exe().ok();
  let id = exe.as_ref().and_then(|p| p.file_stem()?.to_str()).unwrap_or("nimbo");
  let (id, title) = (CString::new(id)?, CString::new(cfg.title)?);
  let size = Size::new(cfg.size.w.round().max(1.0), cfg.size.h.round().max(1.0));
  let (w, h) = (size.w as c_int, size.h as c_int);
  let min = cfg.min();
  // SAFETY: GTK's calls, on the thread that runs its loop; each callback has its signal's
  // signature.
  unsafe {
    // The Wayland app id and the X11 WM_CLASS.
    (g.g_set_prgname)(id.as_ptr());
    if (g.gtk_init_check)(null_mut(), null_mut()) == FALSE {
      return Err("GTK cannot open a display (WAYLAND_DISPLAY or DISPLAY)".into());
    }

    let window = (g.gtk_window_new)(WINDOW_TOPLEVEL);
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
    connect(window, c"destroy", on_destroy as Fp, 0);
    connect(window, c"window-state-event", on_state as Fp, 0);
    connect(area, c"draw", on_draw as Fp, 0);
    connect(area, c"event", input::on_event as Fp, 0);

    let clip = Box::new(Clip((g.gtk_clipboard_get)(CLIPBOARD)));
    let mut rt = Runtime::new(app, size, 1.0, Clock::real(), clip);
    rt.event(Event::Appearance(appearance()));
    // A wake from any thread: an idle source on the main context, which GLib wakes.
    rt.waker().set_notify(Box::new(|| {
      (gtk().g_idle_add)(on_wake as Fp, null_mut());
    }));

    let trace = std::env::var("NIMBO_TRACE").ok();
    WIN.set(Some(Win {
      rt,
      renderer: Renderer::new(),
      window,
      area,
      tiles: Vec::new(),
      shown: (0, 0),
      ticking: false,
      timer: None,
      resizable: cfg.resizable,
      min,
      input: Input::new(),
      cursor: None,
      cursors: Vec::new(),
      painted: false,
      frames: 0,
      started,
      trace: trace.is_some(),
      trace_events: trace.as_deref() == Some("2"),
      quit: false,
      destroyed: false,
    }));

    // Shown, the window is realized: its frame clock and the area's GdkWindow exist.
    (g.gtk_widget_show_all)(window);
    let clock = (g.gtk_widget_get_frame_clock)(window);
    connect(clock, c"layout", on_layout as Fp, AFTER);
    connect(clock, c"after-paint", on_after_paint as Fp, 0);
    with((), |w| w.input.realized((g.gtk_widget_get_window)(area)));
    (g.gtk_main)();
  }

  let Some(win) = WIN.take() else { return Ok(()) };
  if win.trace {
    eprintln!("nimbo: closed after {} frames", win.frames);
  }

  // The last frame shown, as a PNG (`NIMBO_SHOT=<file>`): to check a window run.
  if let Some(path) = std::env::var_os("NIMBO_SHOT")
    && let Some(px) = pixels(win.shown)
  {
    crate::png::write(Path::new(&path), &px)?;
  }

  if !win.destroyed {
    unsafe { (g.gtk_widget_destroy)(win.window) };
  }

  Ok(())
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
    SURFACE.get().is_null() || Renderer::pixel_size(size, scale) != self.shown
  }

  /// Not minimized or withdrawn (as X11 tells; on Wayland the compositor holds the clock).
  fn visible(&self) -> bool {
    let g = gtk();
    let win = unsafe { (g.gtk_widget_get_window)(self.window) };
    !win.is_null() && unsafe { (g.gdk_window_get_state)(win) } & HIDDEN == 0
  }

  /// Feeds an event to the runtime; returns whether it was used.
  fn send(&mut self, ev: Event) -> bool {
    if self.trace_events {
      eprintln!("nimbo: {ev:?}");
    }

    let (size, scale) = self.size();
    self.rt.resize(size, scale);
    self.rt.event(ev)
  }

  /// After every callback: the size the app asked for, the cursor, quitting, and what runs the
  /// loop next: the tick callback while frames are wanted, else a timeout for the next timer.
  fn sync(&mut self) {
    let g = gtk();
    self.apply_resize();
    self.sync_cursor();
    if self.rt.quit_requested() && !self.quit {
      self.quit = true;
      unsafe { (g.gtk_main_quit)() };
    }

    let wanted = self.rt.needs_frame() && self.visible();
    if wanted && !self.ticking {
      self.ticking = true;
      unsafe { (g.gtk_widget_add_tick_callback)(self.area, on_tick as Fp, null_mut(), null()) };
    }

    let at = self.rt.due_in().filter(|_| !wanted).map(|d| Instant::now() + d);
    self.arm(at);
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
    let id = unsafe { (g.g_timeout_add)(ms as c_uint, on_timer as Fp, null_mut()) };
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
    if (w, h) != self.shown || SURFACE.get().is_null() {
      new_surface(w, h, scale);
      self.shown = (w, h);
      self.tiles.clear();
    }

    let rects = r.damage(&self.tiles);
    if !rects.is_empty() {
      let s = SURFACE.get();
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

/// A new surface of `w × h` pixels at `scale` as [SURFACE], the old one dropped.
fn new_surface(w: usize, h: usize, scale: f32) {
  let g = gtk();
  // SAFETY: a new surface, ours until the next one.
  unsafe {
    let s = (g.cairo_image_surface_create)(RGB24, w as c_int, h as c_int);
    (g.cairo_surface_set_device_scale)(s, scale as f64, scale as f64);
    let old = SURFACE.replace(s);
    if !old.is_null() {
      (g.cairo_surface_destroy)(old);
    }
  }
}

/// The pixels of the frame shown, `w × h`.
fn pixels((w, h): (usize, usize)) -> Option<Pixels> {
  let (g, s) = (gtk(), SURFACE.get());
  if s.is_null() {
    return None;
  }

  // SAFETY: the surface holds `stride × h` pixels.
  let (data, stride) = unsafe {
    (g.cairo_surface_flush)(s);
    let stride = (g.cairo_image_surface_get_stride)(s) as usize / 4;
    let data = (g.cairo_image_surface_get_data)(s).cast::<u32>();
    (std::slice::from_raw_parts(data, stride * h), stride)
  };
  let rows = data.chunks(stride).flat_map(|row| &row[..w]);
  let data = rows.flat_map(|p| (p | 0xff00_0000).to_le_bytes()).collect();
  Some(Pixels { w, h, data })
}

// MARK: Callbacks

/// The frame clock's update phase while frames are wanted: asks for the layout phase, after
/// which the frame is built ([on_layout]); removed once none are.
unsafe extern "C" fn on_tick(_: P, clock: P, _: P) -> c_int {
  with(TRUE, |w| {
    w.ticking = w.rt.needs_frame() && w.visible();
    if w.ticking {
      unsafe { (gtk().gdk_frame_clock_request_phase)(clock, LAYOUT) };
    }

    w.ticking as c_int
  })
}

/// After GTK's layout: the frame, at the size GTK just gave the area, when one is wanted or
/// that size is new.
unsafe extern "C" fn on_layout(_: P, _: P) {
  with((), |w| {
    if w.rt.needs_frame() || w.stale() {
      w.render(true);
    }
  });
}

/// GTK's paint: our surface where GTK repaints (the rects queued, or all of it after an expose
/// or a resize), built first if it is stale (the first frame).
unsafe extern "C" fn on_draw(_: P, cr: P, _: P) -> c_int {
  with((), |w| {
    if w.stale() {
      w.render(false);
    }

    w.painted = true;
  });

  let (g, s) = (gtk(), SURFACE.get());
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
unsafe extern "C" fn on_after_paint(_: P, _: P) {
  with((), |w| {
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
unsafe extern "C" fn on_timer(_: P) -> c_int {
  with(TRUE, |w| {
    w.timer = None;
    FALSE
  })
}

/// A wake from another thread; while the window is busy, the sync after the call that holds it
/// sees the wake.
unsafe extern "C" fn on_wake(_: P) -> c_int {
  with((), |_| {});
  FALSE
}

/// Minimized or shown again: frames stop or go on.
unsafe extern "C" fn on_state(_: P, _: P, _: P) -> c_int {
  with((), |_| {});
  FALSE
}

/// The window closed: the loop ends, unless it already did (the app quit).
unsafe extern "C" fn on_destroy(_: P, _: P) {
  let running = with(false, |w| {
    w.destroyed = true;
    !std::mem::replace(&mut w.quit, true)
  });

  if running {
    unsafe { (gtk().gtk_main_quit)() };
  }
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn perf_cores_count_ranges() {
    assert!(perf_cores().is_none_or(|n| n >= 1));
  }
}
