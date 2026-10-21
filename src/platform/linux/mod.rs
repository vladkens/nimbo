// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Linux platform: Wayland when the session has it, else X11 ([x11], for X sessions). [run]
//! picks it: `NIMBO_BACKEND=wayland|x11` forces one; otherwise Wayland when `WAYLAND_DISPLAY`
//! (or `WAYLAND_SOCKET`) is set and the connection works, else X11 on `DISPLAY`. The keyboard
//! ([xkb]) is shared.
//!
//! On Wayland (pure-Rust `wayland-client`, no libwayland): an xdg-shell toplevel, CPU frames
//! presented through `wl_shm` buffers ([shm]) paced by frame callbacks, the scale from
//! fractional-scale-v1 (with a viewport), else the preferred buffer scale or the outputs'; input
//! from `wl_seat` (pointer, axis, pinch gestures, keyboard through xkbcommon in [xkb]),
//! cursor-shape-v1 for cursors (else the Xcursor theme's, [cursor]), the clipboard on
//! `wl_data_device` ([clip]); the objects' events in [dispatch]. One thread: the loop polls the
//! display and a wake eventfd with the next key repeat as the timeout.
//!
//! No client-side decorations: sway and other wlroots compositors (and KDE) draw their own;
//! GNOME (Mutter) draws none for clients, so a window there has no title bar yet (CSD is to
//! do).

mod clip;
mod cursor;
mod dispatch;
mod dl;
mod shm;
mod x11;
mod xkb;

use std::cell::RefCell;
use std::ffi::CStr;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};
use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};
use wayland_client::backend::WaylandError;
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_data_device_manager::WlDataDeviceManager;
use wayland_client::protocol::wl_keyboard::WlKeyboard;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_pointer::{AxisSource, WlPointer};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, EventQueue, QueueHandle};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
  Shape, WpCursorShapeDeviceV1,
};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::WpFractionalScaleV1;
use wayland_protocols::wp::pointer_gestures::zv1::client::zwp_pointer_gesture_pinch_v1::ZwpPointerGesturePinchV1;
use wayland_protocols::wp::pointer_gestures::zv1::client::zwp_pointer_gestures_v1::ZwpPointerGesturesV1;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use wayland_protocols::xdg::shell::client::xdg_surface::XdgSurface;
use wayland_protocols::xdg::shell::client::xdg_wm_base::XdgWmBase;

use self::clip::{Clip, Clipboard, Transfers};
use self::cursor::Cursors;
use self::shm::Presenter;
use self::xkb::{Keyboard, Typed};
use crate::Config;
use crate::app::{App, Appearance, Clock, Event, Mods, MouseButton, Phase, Runtime, WHEEL_LINE};
use crate::hotkey::ModKey;
use crate::platform::points;
use crate::scene::{Point, Size};
use crate::ui::Cursor;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

/// The longest gap between clicks that counts them as one double (triple) click, and how far
/// apart they may be: GTK's defaults (400 ms, 5 px).
const CLICK_TIME: Duration = Duration::from_millis(400);
const CLICK_DIST: f32 = 5.0;
/// Frames paced by a timer while a frame waits for a buffer (all held by the compositor, or a
/// configure's ack that needs one of its size): 120 Hz.
const TICK: Duration = Duration::from_micros(8333);

/// Opens the app's window and runs the event loop until it closes or quits, on Wayland or X11.
pub fn run(cfg: &Config, app: Box<dyn App>) {
  let forced = std::env::var("NIMBO_BACKEND").ok();
  let wayland =
    std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("WAYLAND_SOCKET").is_some();
  let res = match forced.as_deref() {
    Some("x11") => x11::run(cfg, app),
    Some("wayland") => {
      Connection::connect_to_env().map_err(Into::into).and_then(|c| run_loop(c, cfg, app))
    }
    Some(b) => Err(format!("NIMBO_BACKEND={b:?}: wayland or x11").into()),
    None if wayland => match Connection::connect_to_env() {
      Ok(c) => run_loop(c, cfg, app),
      Err(e) if std::env::var_os("DISPLAY").is_some() => {
        eprintln!("nimbo: Wayland: {e}; X11 instead");
        x11::run(cfg, app)
      }
      Err(e) => Err(e.into()),
    },
    None => x11::run(cfg, app),
  };

  if let Err(e) = res {
    eprintln!("nimbo: {e}");
    std::process::exit(1);
  }
}

/// The OS's light or dark setting and accent: light until ported (X11 windows follow
/// XSETTINGS' theme name, [x11]). TODO: the XDG portal's `org.freedesktop.appearance`
/// `color-scheme` and `accent-color` (`Settings.Read`, the `SettingChanged` signal); light
/// without a portal.
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

// MARK: Window state

/// A `wl_output`: its registry name (to see it go) and its scale.
pub struct Output {
  name: u32,
  obj: WlOutput,
  scale: i32,
}

/// A held key that repeats: its evdev code and when it fires next.
struct Repeat {
  key: u32,
  at: Instant,
}

impl Repeat {
  /// The repeat after `key` went down or up: a key that repeats takes over at `at`; one that
  /// doesn't (a modifier) leaves the held key repeating, with the new modifiers; the held key's
  /// release ends it.
  fn after(held: Option<Repeat>, key: u32, down: bool, repeats: bool, at: Instant) -> Option<Self> {
    match (down, held) {
      (true, _) if repeats => Some(Repeat { key, at }),
      (false, Some(r)) if r.key == key => None,
      (_, held) => held,
    }
  }
}

/// The pointer's axis events of one `wl_pointer.frame`; index 0 vertical, 1 horizontal.
#[derive(Default)]
struct Axes {
  source: Option<AxisSource>,
  value: [f64; 2],
  /// Wheel clicks in 120ths (`axis_value120`, or `axis_discrete` × 120).
  v120: [i32; 2],
  /// The axes that stopped (`axis_stop`).
  stop: [bool; 2],
  any: bool,
}

impl Axes {
  /// Whether this frame is a touchpad's (a finger, or continuous), and its phase, given the
  /// axes a finger scroll moves (`scrolling`), which it updates; `None` when it has nothing to
  /// report. A frame with no source (an `axis_stop` alone) goes on with the scroll under way;
  /// the scroll ends when every axis it moved stopped, not the first.
  fn phase(&self, scrolling: &mut [bool; 2]) -> Option<(bool, Phase)> {
    let was = scrolling.contains(&true);
    let precise = match self.source {
      Some(s) => matches!(s, AxisSource::Finger | AxisSource::Continuous),
      None => was,
    };

    if !precise {
      *scrolling = [false; 2];
      return Some((false, Phase::None));
    }

    for (i, on) in scrolling.iter_mut().enumerate() {
      *on = (*on || self.value[i] != 0.0) && !self.stop[i];
    }

    let phase = match (was, scrolling.contains(&true)) {
      (false, true) => Phase::Began,
      (true, true) => Phase::Changed,
      (true, false) => Phase::Ended,
      (false, false) => return None,
    };

    Some((true, phase))
  }
}

pub struct Win {
  qh: QueueHandle<Win>,
  rt: Runtime<Box<dyn App>>,
  surface: WlSurface,
  _xdg: XdgSurface,
  viewport: Option<WpViewport>,
  _fractional: Option<WpFractionalScaleV1>,
  present: Presenter,
  /// Logs every frame presented (`NIMBO_TRACE`), and every event with whether it was used
  /// (`NIMBO_TRACE=2`).
  trace: bool,
  trace_events: bool,
  frames: u64,
  started: Instant,

  /// Logical size (points), from the last configure.
  size: Size,
  pending_size: Option<Size>,
  /// Fractional scale in 120ths, when the compositor sends one.
  scale120: Option<u32>,
  /// `wl_surface.preferred_buffer_scale` (v6).
  preferred: Option<i32>,
  /// Every output, followed as they come and go (the registry), and those the surface is on.
  outputs: Vec<Output>,
  entered: Vec<WlOutput>,
  /// The scale and size the surface was last set up for (buffer scale or viewport).
  set_scale: f32,
  set_size: Size,
  configured: bool,
  /// A configure was acked: the next commit carries it, with a buffer of its size. Its frame is
  /// drawn at once, not paced.
  acked: bool,
  closed: bool,
  /// The window has the focus (the toplevel is activated, or the keyboard is ours).
  focused: bool,
  frame_pending: bool,
  next_tick: Option<Instant>,

  pointer: Option<WlPointer>,
  keyboard: Option<WlKeyboard>,
  shapes: Option<WpCursorShapeManagerV1>,
  shape_device: Option<WpCursorShapeDeviceV1>,
  /// The theme's cursors on a surface of ours, without cursor-shape-v1.
  cursors: Option<Cursors>,
  gestures: Option<ZwpPointerGesturesV1>,
  _pinch: Option<ZwpPointerGesturePinchV1>,
  pinch_scale: f64,
  /// The pointer's enter serial while it is over the surface (cursors need it).
  enter_serial: Option<u32>,
  cursor: Cursor,
  /// The scale of the theme's image last shown: another needs the image again.
  cursor_scale: i32,
  mouse: Point,
  last_click: Option<(MouseButton, Instant, Point, u32)>,
  axes: Axes,
  /// The axes a finger scroll in progress moves: the next frame continues it.
  scrolling: [bool; 2],

  kb: Option<Keyboard>,
  mods: Mods,
  /// Keys per second and the delay before the first repeat, ms.
  repeat_info: (i32, i32),
  repeat: Option<Repeat>,
  /// Modifier keys that went down or up, held for the `modifiers` event the compositor sends
  /// after them: [Event::Modifier] comes after the [Event::Mods] it changed.
  mod_keys: Vec<(ModKey, bool)>,
  clip: Rc<RefCell<Clip>>,
}

fn run_loop(conn: Connection, cfg: &Config, app: Box<dyn App>) -> Res<()> {
  let started = Instant::now();
  let (globals, mut queue): (_, EventQueue<Win>) = registry_queue_init(&conn)?;
  let qh = queue.handle();
  let compositor: WlCompositor = globals.bind(&qh, 4..=6, ())?;
  let wm: XdgWmBase = globals.bind(&qh, 1..=6, ())?;
  let shm: WlShm = globals.bind(&qh, 1..=1, ())?;
  let viewporter: Option<WpViewporter> = globals.bind(&qh, 1..=1, ()).ok();
  let fractions: Option<WpFractionalScaleManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
  let seat: Option<WlSeat> = globals.bind(&qh, 1..=8, ()).ok();
  let data: Option<WlDataDeviceManager> = globals.bind(&qh, 1..=3, ()).ok();
  let shapes: Option<WpCursorShapeManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
  let gestures: Option<ZwpPointerGesturesV1> = globals.bind(&qh, 1..=3, ()).ok();
  let outputs: Vec<Output> = globals.contents().with_list(|list| {
    let outputs = list.iter().filter(|g| g.interface == "wl_output");
    outputs.map(|g| Output::bind(globals.registry(), g.name, g.version, &qh)).collect()
  });

  let surface = compositor.create_surface(&qh, ());
  let cursors = shapes.is_none().then(|| Cursors::new(&compositor, shm.clone(), &qh));
  let (viewport, fractional) = match (&viewporter, &fractions) {
    (Some(v), Some(f)) => {
      (Some(v.get_viewport(&surface, &qh, ())), Some(f.get_fractional_scale(&surface, &qh, ())))
    }
    _ => (None, None),
  };

  let xdg = wm.get_xdg_surface(&surface, &qh, ());
  let top = xdg.get_toplevel(&qh, ());
  top.set_title(cfg.title.into());
  let exe = std::env::current_exe().ok();
  let app_id = exe.as_ref().and_then(|p| p.file_stem()?.to_str()).unwrap_or("nimbo");
  top.set_app_id(app_id.into());
  let size = whole(cfg.size);
  match cfg.resizable {
    true => top.set_min_size(cfg.min().w.ceil() as i32, cfg.min().h.ceil() as i32),
    false => {
      let (w, h) = (size.w as i32, size.h as i32);
      top.set_min_size(w, h);
      top.set_max_size(w, h);
    }
  }

  surface.commit();

  let device = match (&data, &seat) {
    (Some(m), Some(s)) => Some(m.get_data_device(s, &qh, ())),
    _ => None,
  };

  let clip = Rc::new(RefCell::new(Clip {
    conn: conn.clone(),
    qh: qh.clone(),
    manager: data,
    device,
    offers: Vec::new(),
    selection: None,
    source: None,
    serial: 0,
    transfers: Transfers::default(),
  }));
  let mut rt = Runtime::new(app, cfg.size, 1.0, Clock::real(), Box::new(Clipboard(clip.clone())));
  rt.event(Event::Appearance(appearance()));

  // A wake from any thread writes the eventfd the loop polls.
  let wake: Arc<OwnedFd> = Arc::new(eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)?);
  let notify = wake.clone();
  rt.waker().set_notify(Box::new(move || {
    let _ = rustix::io::write(&*notify, &1u64.to_ne_bytes());
  }));

  let mut win = Win {
    qh: qh.clone(),
    rt,
    surface,
    _xdg: xdg,
    viewport,
    _fractional: fractional,
    present: Presenter::new(shm),
    trace: std::env::var_os("NIMBO_TRACE").is_some(),
    trace_events: std::env::var("NIMBO_TRACE").is_ok_and(|v| v == "2"),
    frames: 0,
    started,
    size,
    pending_size: None,
    scale120: None,
    preferred: None,
    outputs,
    entered: Vec::new(),
    set_scale: 0.0,
    set_size: Size::new(0.0, 0.0),
    configured: false,
    acked: false,
    closed: false,
    focused: false,
    frame_pending: false,
    next_tick: None,
    pointer: None,
    keyboard: None,
    shapes,
    shape_device: None,
    cursors,
    gestures,
    _pinch: None,
    pinch_scale: 1.0,
    enter_serial: None,
    cursor: Cursor::Arrow,
    cursor_scale: 0,
    mouse: Point::new(0.0, 0.0),
    last_click: None,
    axes: Axes::default(),
    scrolling: [false; 2],
    kb: None,
    mods: Mods::NONE,
    repeat_info: (25, 600),
    repeat: None,
    mod_keys: Vec::new(),
    clip,
  };

  // Requests wait in the connection's buffer for the socket to drain: no frame is drawn
  // meanwhile.
  let mut unsent = false;
  loop {
    queue.dispatch_pending(&mut win)?;
    // A modifier key that left the modifiers as they were (the other Shift) has no `modifiers`.
    win.modifier_keys();
    if win.closed || win.rt.quit_requested() {
      break;
    }

    win.key_repeat();

    // A configure's frame comes at once: its ack is committed with a buffer of its size.
    let now = Instant::now();
    let paced = !win.frame_pending && win.next_tick.is_none_or(|t| t <= now);
    if win.configured && win.rt.needs_frame() && (paced || win.acked) && !unsent {
      win.render();
    }

    let was = std::mem::replace(&mut unsent, backed_up(queue.flush())?);
    if was && !unsent {
      // Drained: the frame that waited for it is drawn before the loop sleeps.
      continue;
    }

    let Some(guard) = queue.prepare_read() else { continue };
    // Requests the socket couldn't take yet go out once it is writable again; no tick until
    // then.
    let timeout = win.timeout().filter(|_| !unsent);
    let timeout =
      timeout.map(|d| Timespec { tv_sec: d.as_secs() as _, tv_nsec: d.subsec_nanos() as _ });
    let display = guard.connection_fd();
    let clip = win.clip.borrow();
    // The display, a wake, and the pipes our copied text goes into.
    let flags = if unsent { PollFlags::IN | PollFlags::OUT } else { PollFlags::IN };
    let mut fds = vec![PollFd::new(&display, flags), PollFd::new(&*wake, PollFlags::IN)];
    fds.extend(clip.transfers.fds().map(|fd| PollFd::from_borrowed_fd(fd, PollFlags::OUT)));
    match poll(&mut fds, timeout.as_ref()) {
      Ok(_) | Err(rustix::io::Errno::INTR) => {}
      Err(e) => return Err(e.into()),
    }

    let ready = !fds[0].revents().difference(PollFlags::OUT).is_empty();
    let woken = !fds[1].revents().is_empty();
    drop(fds);
    drop(clip);
    win.clip.borrow_mut().transfers.pump(Instant::now());
    if woken {
      let mut buf = [0u8; 8];
      let _ = rustix::io::read(&*wake, &mut buf);
    }

    match ready {
      true => match guard.read() {
        Ok(_) => {}
        Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
        Err(e) => return Err(e.into()),
      },
      false => drop(guard),
    }
  }

  if win.trace {
    eprintln!("nimbo: closed after {} frames", win.frames);
  }
  // The last frame shown, as a PNG (`NIMBO_SHOT=<file>`): to check a window run.
  if let Some(path) = std::env::var_os("NIMBO_SHOT")
    && let Some(px) = win.present.shown()
  {
    crate::png::write(Path::new(&path), &px)?;
  }
  Ok(())
}

impl Win {
  /// The scale frames are drawn at: fractional, else the preferred buffer scale, else the
  /// largest of the outputs the surface is on; before it is on one (the first frame), the
  /// largest of all.
  fn scale(&self) -> f32 {
    if let Some(n) = self.scale120
      && self.viewport.is_some()
    {
      return n as f32 / 120.0;
    }
    let on = self.entered.iter().filter_map(|o| self.outputs.iter().find(|x| x.obj == *o));
    let on = on.map(|x| x.scale).max();
    let any = self.outputs.iter().map(|x| x.scale).max();
    self.preferred.or(on).or(any).unwrap_or(1).max(1) as f32
  }

  /// Until the loop must run again without input: the next key repeat, the next idle reader of
  /// our text let go, and the next paced tick when no frame callback is due.
  fn timeout(&mut self) -> Option<Duration> {
    let now = Instant::now();
    let repeat = self.repeat.as_ref().map(|r| r.at);
    let transfer = self.clip.borrow().transfers.deadline();
    let mut at = match (repeat, transfer) {
      (Some(a), Some(b)) => Some(a.min(b)),
      (a, b) => a.or(b),
    };

    if self.configured && !self.frame_pending {
      let next = match self.rt.needs_frame() {
        true => self.next_tick,
        false => None,
      };

      at = match (at, next) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
      };
    }

    at.map(|t| t.saturating_duration_since(now))
  }

  /// Builds a frame and presents it if it changed; a presented frame asks for a frame callback,
  /// which paces the next.
  fn render(&mut self) {
    let t0 = Instant::now();
    let (scale, logical) = (self.scale(), self.size);
    if scale != self.set_scale || logical != self.set_size {
      match (&self.viewport, self.scale120.is_some()) {
        (Some(v), true) => {
          self.surface.set_buffer_scale(1);
          v.set_destination(logical.w as i32, logical.h as i32);
        }
        _ => self.surface.set_buffer_scale(scale as i32),
      }

      (self.set_scale, self.set_size) = (scale, logical);
    }

    // Drawn at the points that fill the buffer's whole pixels.
    let (w, h) = buffer_px(logical, scale);
    let size = Size::new(points(w, scale), points(h, scale));
    self.rt.resize(size, scale);
    self.rt.frame();
    self.present.renderer.sync(&mut self.rt.images);
    let shown = self.present.present(&self.rt.scene, size, scale, &self.surface, &self.qh);
    let fits = self.present.fits();
    self.next_tick = None;
    // An unchanged frame that animates on asks for a callback too, committed with no buffer: the
    // compositor paces it, and sends none while the window is hidden. One waiting for a buffer
    // ticks instead.
    let waits = self.present.starved || (self.acked && !fits);
    let callback = shown || (self.rt.needs_frame() && !waits);
    if callback {
      self.surface.frame(&self.qh, ());
      self.frame_pending = true;
    } else if self.rt.needs_frame() {
      // From the frame's start: the time spent building it is part of the tick.
      self.next_tick = Some(t0 + TICK);
    }

    if shown {
      self.frames += 1;
      if self.trace {
        let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        let since = self.started.elapsed().as_secs_f64() * 1e3;
        let (w, h) = self.present.renderer.size();
        eprintln!("nimbo: presented {:.3} (+{since:.1} ms) {w}x{h}", t.as_secs_f64() * 1e3);
      }
    }

    // An ack goes with the buffer of its size: a new one, or the one shown when that has it (an
    // unchanged frame, or one waiting for a buffer at the same size).
    if callback || (self.acked && fits) {
      self.acked = false;
      self.surface.commit();
    } else if self.acked && self.trace {
      eprintln!("nimbo: a configure waits for a buffer of its size");
    }

    self.sync_cursor();
  }

  /// Feeds an event to the runtime; returns whether it was used.
  fn send(&mut self, ev: Event) -> bool {
    if self.trace_events {
      eprintln!("nimbo: {ev:?}");
    }

    // A configure in the same batch already counts for the input after it.
    self.rt.resize(self.size, self.scale());
    let used = self.rt.event(ev);
    self.sync_cursor();
    used
  }

  /// The window took or lost the focus (the keyboard's, or the toplevel's activated state):
  /// the app hears of each change once.
  fn focus(&mut self, on: bool) {
    if on != self.focused {
      self.focused = on;
      self.send(Event::Focus(on));
    }
  }

  fn sync_cursor(&mut self) {
    let c = self.rt.cursor();
    let rescaled = self.cursors.is_some() && self.scale().ceil() as i32 != self.cursor_scale;
    if c == self.cursor && !rescaled {
      return;
    }

    self.cursor = c;
    if let Some(serial) = self.enter_serial {
      self.show_cursor(serial);
    }
  }

  /// Shows the cursor for the pointer's enter `serial`: by its shape, else the theme's image.
  fn show_cursor(&mut self, serial: u32) {
    let scale = self.scale().ceil() as i32;
    if let Some(dev) = &self.shape_device {
      dev.set_shape(serial, shape(self.cursor));
    } else if let (Some(cursors), Some(pointer)) = (&mut self.cursors, &self.pointer) {
      cursors.set(pointer, serial, self.cursor, scale, &self.qh);
      self.cursor_scale = scale;
    }
  }

  // MARK: Keys

  /// A key pressed or released (evdev code): to the app as a key event, then, unused, through
  /// the keyboard's compose state and the Linux editing keys into text or a command.
  fn key(&mut self, key: u32, down: bool, repeat: bool) {
    let Some(kb) = &self.kb else { return };
    let (k, utf8) = kb.key(key, repeat);
    // Modifier keys reach the app as [Event::Mods] (from the compositor's modifiers) and, by
    // their side, [Event::Modifier], as on macOS (`flagsChanged:`).
    if let Some(side) = kb.modifier(key, k.code) {
      if let Some(key) = side.filter(|_| !repeat) {
        self.mod_keys.push((key, down));
      }

      return;
    }

    if !down {
      self.send(Event::KeyUp(k));
      return;
    }

    if self.send(Event::KeyDown(k)) {
      return;
    }

    let typed = self.kb.as_mut().map_or(Typed::None, |kb| kb.interpret(key, &k, &utf8));
    match typed {
      Typed::Text(s) => _ = self.send(Event::Text(s)),
      Typed::Command(c) => _ = self.send(Event::Command(c)),
      Typed::None => {}
    }
  }

  /// Sends the modifier keys held back for their `modifiers` event.
  fn modifier_keys(&mut self) {
    for (key, down) in std::mem::take(&mut self.mod_keys) {
      self.send(Event::Modifier { key, down });
    }
  }

  /// Fires the held key's repeat when it is due.
  fn key_repeat(&mut self) {
    let Some(r) = &mut self.repeat else { return };
    let now = Instant::now();
    if r.at > now {
      return;
    }

    r.at = now + Duration::from_secs(1) / self.repeat_info.0.max(1) as u32;
    let key = r.key;
    self.key(key, true, true);
  }

  // MARK: Pointer

  fn button(&mut self, button: u32, down: bool) {
    // <linux/input-event-codes.h>: BTN_LEFT, BTN_RIGHT, BTN_MIDDLE.
    let button = match button {
      0x110 => MouseButton::Left,
      0x111 => MouseButton::Right,
      0x112 => MouseButton::Middle,
      b => MouseButton::Other(b.saturating_sub(0x110).min(255) as u8),
    };

    let (pos, mods) = (self.mouse, self.mods);
    if !down {
      self.send(Event::MouseUp { pos, button, mods });
      return;
    }

    let now = Instant::now();
    let clicks = match self.last_click {
      Some((b, t, p, n))
        if b == button
          && now - t < CLICK_TIME
          && (p.x - pos.x).abs() < CLICK_DIST
          && (p.y - pos.y).abs() < CLICK_DIST =>
      {
        n + 1
      }
      _ => 1,
    };

    self.last_click = Some((button, now, pos, clicks));
    self.send(Event::MouseDown { pos, button, clicks, mods });
  }

  /// `wl_pointer.frame`: the axis events since the last one as one scroll.
  fn scroll(&mut self) {
    let a = std::mem::take(&mut self.axes);
    if !a.any {
      return;
    }

    let Some((precise, phase)) = a.phase(&mut self.scrolling) else { return };
    let (pos, mods) = (self.mouse, self.mods);
    let delta = match precise {
      true => Point::new(-a.value[1] as f32, -a.value[0] as f32),
      false => {
        // A wheel click is 15 units of `axis` in libinput when no click count came.
        let lines = |i: usize| match a.v120[i] {
          0 => -a.value[i] as f32 / 15.0,
          v => -v as f32 / 120.0,
        };
        Point::new(lines(1) * WHEEL_LINE, lines(0) * WHEEL_LINE)
      }
    };

    self.send(Event::Scroll { pos, delta, precise, phase, momentum: Phase::None, mods });
  }
}

/// The cursor-shape-v1 shape for ours.
fn shape(c: Cursor) -> Shape {
  match c {
    Cursor::Inherit | Cursor::Arrow => Shape::Default,
    Cursor::Text => Shape::Text,
    Cursor::Pointer => Shape::Pointer,
    Cursor::ResizeX => Shape::ColResize,
    Cursor::ResizeY => Shape::RowResize,
    Cursor::Crosshair => Shape::Crosshair,
    Cursor::Grab => Shape::Grab,
    Cursor::Grabbing => Shape::Grabbing,
    Cursor::NotAllowed => Shape::NotAllowed,
  }
}

/// The keymap text the compositor shares through `fd`.
fn keymap(fd: OwnedFd, size: usize) -> Option<Keyboard> {
  // SAFETY: a private read-only mapping of the keymap the compositor sent, unmapped below.
  let flags = (ProtFlags::READ, MapFlags::PRIVATE);
  let ptr = unsafe { mmap(std::ptr::null_mut(), size, flags.0, flags.1, &fd, 0) }.ok()?;
  let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), size) };
  let kb = CStr::from_bytes_until_nul(bytes).ok().and_then(Keyboard::from_string);
  let _ = unsafe { munmap(ptr, size) };
  kb
}

/// Whether a flush left requests waiting for the socket to drain: `WouldBlock` while the
/// compositor isn't reading is backpressure, not an error, and they stay buffered.
fn backed_up(flushed: Result<(), WaylandError>) -> Res<bool> {
  match flushed {
    Ok(()) => Ok(false),
    Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(true),
    Err(e) => Err(e.into()),
  }
}

/// The buffer's pixels for `size` points at `scale`, rounded half away from zero as
/// fractional-scale-v1 asks: 481 points at 1.5 are 722 pixels, where the renderer's cut-down
/// size would make 721 that the viewport stretches.
fn buffer_px(size: Size, scale: f32) -> (u32, u32) {
  let px = |p: f32| (p * scale).round().max(1.0) as u32;
  (px(size.w), px(size.h))
}

/// `s` in whole surface coordinates, as configures give them: at an integer buffer scale a
/// fractional size would make a buffer that isn't a multiple of it (`invalid_size`).
fn whole(s: Size) -> Size {
  Size::new(s.w.round().max(1.0), s.h.round().max(1.0))
}

impl Output {
  fn bind(registry: &WlRegistry, name: u32, version: u32, qh: &QueueHandle<Win>) -> Self {
    Self { name, obj: registry.bind(name, version.min(4), qh, ()), scale: 1 }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn sizes_are_whole() {
    assert_eq!(whole(Size::new(480.5, 320.4)), Size::new(481.0, 320.0));
    assert_eq!(whole(Size::new(0.2, 64.0)), Size::new(1.0, 64.0));
  }

  #[test]
  fn fractional_buffers_round() {
    assert_eq!(buffer_px(Size::new(481.0, 320.0), 1.5), (722, 480));
    assert_eq!(buffer_px(Size::new(481.0, 321.0), 1.25), (601, 401));
    assert_eq!(buffer_px(Size::new(480.0, 320.0), 2.0), (960, 640));

    // The renderer draws that many.
    let (w, h) = buffer_px(Size::new(481.0, 333.0), 1.75);
    let size = Size::new(points(w, 1.75), points(h, 1.75));
    let px = crate::cpu::Renderer::pixel_size(size, 1.75);
    assert_eq!(px, (w as usize, h as usize));
  }

  #[test]
  fn a_full_socket_waits() {
    let err = |k: std::io::ErrorKind| Err(WaylandError::Io(k.into()));
    assert!(!backed_up(Ok(())).unwrap());
    assert!(backed_up(err(std::io::ErrorKind::WouldBlock)).unwrap());
    assert!(backed_up(err(std::io::ErrorKind::BrokenPipe)).is_err());
  }

  #[test]
  fn a_finger_scroll_ends_when_every_axis_stops() {
    let finger = Some(AxisSource::Finger);
    let frame =
      |source, value: [f64; 2], stop| Axes { source, value, stop, any: true, ..Default::default() };
    let mut on = [false; 2];
    // Diagonal, then the vertical axis stops while the horizontal goes on, then that stops too.
    assert_eq!(frame(finger, [1.0, 1.0], [false; 2]).phase(&mut on), Some((true, Phase::Began)));
    assert_eq!(
      frame(finger, [0.0, 2.0], [true, false]).phase(&mut on),
      Some((true, Phase::Changed))
    );
    assert_eq!(on, [false, true]);
    // A stop with no source still ends the touchpad's scroll.
    assert_eq!(frame(None, [0.0; 2], [false, true]).phase(&mut on), Some((true, Phase::Ended)));
    assert_eq!(on, [false; 2]);
    // A wheel: no phases.
    let wheel = Some(AxisSource::Wheel);
    assert_eq!(frame(wheel, [10.0, 0.0], [false; 2]).phase(&mut on), Some((false, Phase::None)));
    assert_eq!(frame(None, [0.0; 2], [false; 2]).phase(&mut on), Some((false, Phase::None)));
  }

  #[test]
  fn a_modifier_keeps_the_held_key_repeating() {
    const LEFT: u32 = 105;
    const SHIFT: u32 = 42;
    const A: u32 = 30;
    let t = Instant::now();
    let held = Repeat::after(None, LEFT, true, true, t);
    let held = Repeat::after(held, SHIFT, true, false, t);
    assert_eq!(held.as_ref().map(|r| r.key), Some(LEFT), "Shift down");
    let held = Repeat::after(held, SHIFT, false, false, t);
    assert_eq!(held.as_ref().map(|r| r.key), Some(LEFT), "Shift up");
    let held = Repeat::after(held, A, true, true, t);
    assert_eq!(held.as_ref().map(|r| r.key), Some(A), "another key takes over");
    let held = Repeat::after(held, LEFT, false, true, t);
    assert_eq!(held.as_ref().map(|r| r.key), Some(A), "not the held key's release");
    assert!(Repeat::after(held, A, false, true, t).is_none());
  }

  #[test]
  fn perf_cores_count_ranges() {
    assert!(perf_cores().is_none_or(|n| n >= 1));
  }
}
