// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Linux platform on Wayland (pure-Rust `wayland-client`, no libwayland): an xdg-shell
//! toplevel, CPU frames presented through `wl_shm` buffers ([shm]) paced by frame callbacks, the
//! scale from fractional-scale-v1 (with a viewport), else the preferred buffer scale or the
//! outputs'; the objects' events in [dispatch]. One thread: the loop polls the display and a wake
//! eventfd.
//!
//! No client-side decorations: sway and other wlroots compositors (and KDE) draw their own;
//! GNOME (Mutter) draws none for clients, so a window there has no title bar yet (CSD is to
//! do).

mod dispatch;
mod shm;

use std::os::fd::OwnedFd;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::WlOutput;
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
use crate::scene::Size;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

/// Frames paced by a timer while the runtime animates without a visible change (no commit, so
/// no frame callback): 120 Hz.
const TICK: Duration = Duration::from_micros(8333);

/// Opens the app's window and runs the event loop until it closes or quits.
pub fn run(cfg: &Config, app: Box<dyn App>) {
  let res = Connection::connect_to_env().map_err(Into::into).and_then(|c| run_loop(c, cfg, app));
  if let Err(e) = res {
    eprintln!("nimbo: {e}");
    std::process::exit(1);
  }
}

/// The OS's light or dark setting and accent: light until ported. TODO: the XDG portal's
/// `org.freedesktop.appearance` `color-scheme` and `accent-color` (`Settings.Read`, the
/// `SettingChanged` signal); light without a portal.
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
  outputs: Vec<(WlOutput, i32)>,
  entered: Vec<WlOutput>,
  /// The scale and size the surface was last set up for (buffer scale or viewport).
  set_scale: f32,
  set_size: Size,
  configured: bool,
  /// A configure was acked: the next commit carries it.
  acked: bool,
  closed: bool,
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
  let outputs: Vec<(WlOutput, i32)> = globals.contents().with_list(|list| {
    let outputs = list.iter().filter(|g| g.interface == "wl_output");
    outputs.map(|g| (globals.registry().bind(g.name, g.version.min(4), &qh, ()), 1)).collect()
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
  match cfg.resizable {
    true => top.set_min_size(cfg.min().w.ceil() as i32, cfg.min().h.ceil() as i32),
    false => {
      let (w, h) = (cfg.size.w as i32, cfg.size.h as i32);
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
    size: cfg.size,
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
    frame_pending: false,
    next_tick: None,
  };

  loop {
    queue.dispatch_pending(&mut win)?;
    if win.closed || win.rt.quit_requested() {
      break;
    }

    let now = Instant::now();
    if win.configured
      && !win.frame_pending
      && win.next_tick.is_none_or(|t| t <= now)
      && win.rt.needs_frame()
    {
      win.render();
    } else if win.acked {
      win.acked = false;
      win.surface.commit();
    }
    queue.flush()?;

    let Some(guard) = queue.prepare_read() else { continue };
    let timeout =
      win.timeout().map(|d| Timespec { tv_sec: d.as_secs() as _, tv_nsec: d.subsec_nanos() as _ });
    let display = guard.connection_fd();
    let mut fds = [PollFd::new(&display, PollFlags::IN), PollFd::new(&*wake, PollFlags::IN)];
    match poll(&mut fds, timeout.as_ref()) {
      Ok(_) | Err(rustix::io::Errno::INTR) => {}
      Err(e) => return Err(e.into()),
    }
    let (ready, woken) = (!fds[0].revents().is_empty(), !fds[1].revents().is_empty());
    if woken {
      let mut buf = [0u8; 8];
      let _ = rustix::io::read(&*wake, &mut buf);
    }
    match ready {
      true => match guard.read() {
        Ok(_) => {}
        Err(wayland_client::backend::WaylandError::Io(e))
          if e.kind() == std::io::ErrorKind::WouldBlock => {}
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
    let on = self.entered.iter().filter_map(|o| self.outputs.iter().find(|x| x.0 == *o));
    let on = on.map(|x| x.1).max();
    let any = self.outputs.iter().map(|x| x.1).max();
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
    let (scale, size) = (self.scale(), self.size);
    if scale != self.set_scale || size != self.set_size {
      match (&self.viewport, self.scale120.is_some()) {
        (Some(v), true) => {
          self.surface.set_buffer_scale(1);
          v.set_destination(size.w as i32, size.h as i32);
        }
        _ => self.surface.set_buffer_scale(scale as i32),
      }
      (self.set_scale, self.set_size) = (scale, size);
    }

    self.rt.resize(size, scale);
    self.rt.frame();
    self.present.renderer.sync(&mut self.rt.images);
    let shown = self.present.present(&self.rt.scene, size, scale, &self.surface, &self.qh);
    self.next_tick = None;
    if shown {
      self.surface.frame(&self.qh, ());
      self.frame_pending = true;
      self.frames += 1;
      if self.trace {
        let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        let since = self.started.elapsed().as_secs_f64() * 1e3;
        let (w, h) = self.present.renderer.size();
        eprintln!("nimbo: presented {:.3} (+{since:.1} ms) {w}x{h}", t.as_secs_f64() * 1e3);
      }
    } else if self.rt.needs_frame() {
      self.next_tick = Some(Instant::now() + TICK);
    }
    if shown || self.acked {
      self.acked = false;
      self.surface.commit();
    }
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
