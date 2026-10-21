// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Linux platform: Wayland when the session has it, else X11 ([x11], for X sessions). [run]
//! picks it: `NIMBO_BACKEND=wayland|x11` forces one; otherwise Wayland when `WAYLAND_DISPLAY`
//! (or `WAYLAND_SOCKET`) is set and the connection works, else X11 on `DISPLAY`.
//!
//! On Wayland (pure-Rust `wayland-client`, no libwayland): an xdg-shell toplevel, CPU frames
//! presented through `wl_shm` buffers ([shm]) paced by frame callbacks, the scale from
//! fractional-scale-v1 (with a viewport), else the preferred buffer scale or the outputs'; the
//! objects' events in [dispatch]. One thread: the loop polls the display and a wake eventfd.
//!
//! No client-side decorations: sway and other wlroots compositors (and KDE) draw their own;
//! GNOME (Mutter) draws none for clients, so a window there has no title bar yet (CSD is to
//! do).

mod dispatch;
mod dl;
mod shm;
mod x11;

use std::os::fd::OwnedFd;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};
use wayland_client::backend::WaylandError;
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, EventQueue, QueueHandle};
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::WpFractionalScaleV1;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use wayland_protocols::xdg::shell::client::xdg_surface::XdgSurface;
use wayland_protocols::xdg::shell::client::xdg_wm_base::XdgWmBase;

use self::shm::Presenter;
use crate::Config;
use crate::app::{App, Appearance, Clock, Event, Runtime};
use crate::platform::points;
use crate::scene::Size;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

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

pub struct Win {
  qh: QueueHandle<Win>,
  rt: Runtime<Box<dyn App>>,
  surface: WlSurface,
  _xdg: XdgSurface,
  viewport: Option<WpViewport>,
  _fractional: Option<WpFractionalScaleV1>,
  present: Presenter,
  /// Logs every frame presented (`NIMBO_TRACE`).
  trace: bool,
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
  /// The toplevel is activated: the window has the focus.
  focused: bool,
  frame_pending: bool,
  next_tick: Option<Instant>,
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
  let outputs: Vec<Output> = globals.contents().with_list(|list| {
    let outputs = list.iter().filter(|g| g.interface == "wl_output");
    outputs.map(|g| Output::bind(globals.registry(), g.name, g.version, &qh)).collect()
  });

  let surface = compositor.create_surface(&qh, ());
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

  let mut rt = Runtime::new(app, cfg.size, 1.0, Clock::real());
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
  };

  // Requests wait in the connection's buffer for the socket to drain: no frame is drawn
  // meanwhile.
  let mut unsent = false;
  loop {
    queue.dispatch_pending(&mut win)?;
    if win.closed || win.rt.quit_requested() {
      break;
    }

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
    let flags = if unsent { PollFlags::IN | PollFlags::OUT } else { PollFlags::IN };
    let mut fds = [PollFd::new(&display, flags), PollFd::new(&*wake, PollFlags::IN)];
    match poll(&mut fds, timeout.as_ref()) {
      Ok(_) | Err(rustix::io::Errno::INTR) => {}
      Err(e) => return Err(e.into()),
    }

    let ready = !fds[0].revents().difference(PollFlags::OUT).is_empty();
    let woken = !fds[1].revents().is_empty();
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

  /// Until the loop must run again without input: the next paced tick when no frame callback
  /// is due.
  fn timeout(&mut self) -> Option<Duration> {
    let now = Instant::now();
    let paced = self.configured && !self.frame_pending && self.rt.needs_frame();
    let at = self.next_tick.filter(|_| paced);
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
  }
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
  fn perf_cores_count_ranges() {
    assert!(perf_cores().is_none_or(|n| n >= 1));
  }
}
