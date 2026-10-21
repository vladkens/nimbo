// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Linux platform on X11, for sessions without Wayland (X11 desktops, remote displays):
//! XCB loaded at run time ([xcb]), one top-level window the window manager
//! decorates, CPU frames through MIT-SHM or the socket ([present]) paced at [REFRESH], the scale
//! from `Xft.dpi` in the resources (the size hints follow it), light or dark from XSETTINGS'
//! `Net/ThemeName`, its manager followed as it starts, quits or restarts. No frames while the
//! window is unmapped or fully covered (`VisibilityNotify`; under a compositing manager it never
//! is). One thread: the loop polls the connection and a wake eventfd.
//!
//! Not yet: `_NET_WM_SYNC_REQUEST` (the window manager doesn't wait for our frame as it
//! resizes), the display's refresh rate (RandR), per-monitor scale, pixel formats other than
//! 32-bit `0xRRGGBB` (a BGR VNC server: refused at the start, [present::check_format]).

mod present;
mod xcb;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};

use self::present::{Presenter, STUCK};
use self::xcb::*;
use super::Res;
use crate::Config;
use crate::app::{App, Appearance, Clock, Event, Runtime};
use crate::cpu::raster::IRect;
use crate::platform::{Pace, points};
use crate::scene::Size;

/// The frame interval while the runtime animates: 60 Hz (the display's rate isn't read).
const REFRESH: Duration = Duration::from_micros(16_667);

/// `WM_NORMAL_HINTS` flags (ICCCM): the least and largest sizes are set.
const P_MIN_SIZE: u32 = 1 << 4;
const P_MAX_SIZE: u32 = 1 << 5;
/// `FocusIn` / `LeaveNotify` details and modes to skip: the pointer's own focus, a child's,
/// and the keyboard grabs of the window manager.
const NOTIFY_INFERIOR: u8 = 2;
const NOTIFY_POINTER: u8 = 5;
const NOTIFY_MODE_GRAB: u8 = 1;
const NOTIFY_MODE_UNGRAB: u8 = 2;
/// What the XSETTINGS manager's window is watched for: its settings, and its destruction.
const XSETTINGS_EVENTS: u32 = EM_PROPERTY_CHANGE | EM_STRUCTURE_NOTIFY;

struct Atoms {
  protocols: u32,
  delete: u32,
  ping: u32,
  net_name: u32,
  pid: u32,
  machine: u32,
  xsettings: u32,
  xsettings_settings: u32,
  manager: u32,
}

struct Win {
  x: X,
  win: u32,
  root: u32,
  atoms: Atoms,
  rt: Runtime<Box<dyn App>>,
  present: Presenter,
  scale: f32,
  /// The content size in points, and in pixels as the server last configured it.
  size: Size,
  px: (u32, u32),
  mapped: bool,
  /// Nothing of the window shows: other windows cover it all.
  obscured: bool,
  closed: bool,
  /// A new size: drawn at once, not at the next tick.
  force: bool,
  /// Frames a [REFRESH] apart while the runtime animates.
  pace: Pace,
  /// Logs every frame presented (`NIMBO_TRACE`), and every event (`NIMBO_TRACE=2`).
  trace: bool,
  trace_events: bool,
  frames: u64,
  started: Instant,
  /// The first expose came (the first frame shows).
  exposed: bool,

  focused: bool,
  /// Events read ahead.
  stash: Rc<RefCell<VecDeque<Ev>>>,
  /// The XSETTINGS manager's window, watched for changes of the theme and for its end.
  xsettings: u32,
  /// The least content size in points, and the largest of a fixed window: the size hints at
  /// each scale.
  min: Size,
  max: Option<Size>,
}

/// Opens the app's window on `$DISPLAY` and runs the loop until it closes or quits.
pub fn run(cfg: &Config, app: Box<dyn App>) -> Res<()> {
  let started = Instant::now();
  let (x, screen, n) = X::connect()?;
  let res = run_on(x, screen, n, cfg, app, started);
  x.disconnect();
  res
}

fn run_on(
  x: X,
  screen: *mut Screen,
  n: i32,
  cfg: &Config,
  app: Box<dyn App>,
  started: Instant,
) -> Res<()> {
  // SAFETY: the setup's screen, valid while the connection is open.
  let sc = unsafe { &*screen };
  if !matches!(sc.root_depth, 24 | 32) {
    return Err(format!("the X screen has depth {}, 24 or 32 needed", sc.root_depth).into());
  }

  let (setup, at) = x.setup(screen);
  present::check_format(setup, at)?;

  let xs_name = format!("_XSETTINGS_S{n}");
  let [protocols, delete, ping, net_name, pid, machine, xsettings, xs_settings] = x.atoms([
    "WM_PROTOCOLS",
    "WM_DELETE_WINDOW",
    "_NET_WM_PING",
    "_NET_WM_NAME",
    "_NET_WM_PID",
    "WM_CLIENT_MACHINE",
    xs_name.as_str(),
    "_XSETTINGS_SETTINGS",
  ]);
  let [utf8, manager] = x.atoms(["UTF8_STRING", "MANAGER"]);
  let atoms = Atoms {
    protocols,
    delete,
    ping,
    net_name,
    pid,
    machine,
    xsettings,
    xsettings_settings: xs_settings,
    manager,
  };

  let scale = dpi_scale(&x, sc.root);
  let px = (px(cfg.size.w, scale), px(cfg.size.h, scale));
  let events =
    EM_EXPOSURE | EM_VISIBILITY_CHANGE | EM_STRUCTURE_NOTIFY | EM_FOCUS_CHANGE | EM_PROPERTY_CHANGE;
  let win = x.create_window(sc, px.0 as u16, px.1 as u16, events);

  // Title, class, protocols, the process for `_NET_WM_PING`, sizes.
  let title = cfg.title;
  let latin1: Vec<u8> = title.chars().map(|c| u8::try_from(c).unwrap_or(b'?')).collect();
  x.set_property(win, ATOM_WM_NAME, ATOM_STRING, 8, &latin1);
  x.set_property(win, atoms.net_name, utf8, 8, title.as_bytes());
  let exe = std::env::current_exe().ok();
  let id = exe.as_ref().and_then(|p| p.file_stem()?.to_str()).unwrap_or("nimbo");
  let class = format!("{id}\0{}\0", capitalized(id));
  x.set_property(win, ATOM_WM_CLASS, ATOM_STRING, 8, class.as_bytes());
  x.set_property32(win, atoms.protocols, ATOM_ATOM, &[atoms.delete, atoms.ping]);
  x.set_property32(win, atoms.pid, ATOM_CARDINAL, &[std::process::id()]);
  if let Ok(host) = std::fs::read_to_string("/proc/sys/kernel/hostname") {
    x.set_property(win, atoms.machine, ATOM_STRING, 8, host.trim().as_bytes());
  }

  let (min, max) = match cfg.resizable {
    true => (cfg.min(), None),
    false => (cfg.size, Some(cfg.size)),
  };

  set_size_hints(&x, win, min, max, scale);

  // Changes of the resources (Xft.dpi), a new XSETTINGS manager (`MANAGER` on the root), and the
  // manager's settings and end.
  x.select_events(sc.root, EM_PROPERTY_CHANGE | EM_STRUCTURE_NOTIFY);
  let xs_owner = x.selection_owner(atoms.xsettings);
  if xs_owner != 0 {
    x.select_events(xs_owner, XSETTINGS_EVENTS);
  }

  let stash = Rc::new(RefCell::new(VecDeque::new()));
  let mut rt = Runtime::new(app, cfg.size, scale, Clock::real());
  rt.event(Event::Appearance(appearance(&x, xs_owner, xs_settings)));

  // A wake from any thread writes the eventfd the loop polls.
  let wake: Arc<OwnedFd> = Arc::new(eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)?);
  let notify = wake.clone();
  rt.waker().set_notify(Box::new(move || {
    let _ = rustix::io::write(&*notify, &1u64.to_ne_bytes());
  }));

  // Every round trip before the map: after it they wait for the window manager's work.
  let mut present = Presenter::new(x, win, sc.root_depth);
  present.reserve(px.0 as usize, px.1 as usize);
  let mut win = Win {
    x,
    win,
    root: sc.root,
    atoms,
    rt,
    present,
    scale,
    size: Size::new(points(px.0, scale), points(px.1, scale)),
    px,
    mapped: false,
    obscured: false,
    closed: false,
    force: false,
    pace: Pace::new(REFRESH),
    trace: std::env::var_os("NIMBO_TRACE").is_some(),
    trace_events: std::env::var("NIMBO_TRACE").is_ok_and(|v| v == "2"),
    frames: 0,
    started,
    exposed: false,
    focused: false,
    stash,
    xsettings: xs_owner,
    min,
    max,
  };
  if win.trace {
    let shm = if win.present.completion().is_some() { "MIT-SHM" } else { "PutImage" };
    eprintln!("nimbo: x11, scale {scale}, frames by {shm}");
  }
  // The window manager maps it while we draw the first frame, which the first expose puts.
  x.map_window(win.win);
  x.flush();

  loop {
    while let Some(ev) = win.next_event() {
      win.handle(ev);
    }

    if x.has_error() {
      return Err("the X connection broke".into());
    }

    if win.closed || win.rt.quit_requested() {
      break;
    }

    let now = Instant::now();
    if win.present.unstick(now) {
      // The put likely failed: the whole frame again, into the new segment.
      win.rt.redraw();
      if win.trace {
        eprintln!("nimbo: no MIT-SHM completion in {STUCK:?}, a new segment");
      }
    }

    if win.frame_due(now) {
      win.render();
    }

    x.flush();

    // Events XCB read while waiting for a reply aren't on the socket any more.
    if let Some(ev) = x.queued_event() {
      win.stash.borrow_mut().push_back(ev);
      continue;
    }

    let timeout =
      win.timeout().map(|d| Timespec { tv_sec: d.as_secs() as _, tv_nsec: d.subsec_nanos() as _ });
    let fd = x.fd();
    let mut fds = [PollFd::new(&fd, PollFlags::IN), PollFd::new(&*wake, PollFlags::IN)];
    match poll(&mut fds, timeout.as_ref()) {
      Ok(_) | Err(rustix::io::Errno::INTR) => {}
      Err(e) => return Err(e.into()),
    }

    if !fds[1].revents().is_empty() {
      let mut buf = [0u8; 8];
      let _ = rustix::io::read(&*wake, &mut buf);
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

  drop(win.present);
  x.destroy_window(win.win);
  x.flush();
  Ok(())
}

/// `Xft.dpi` in the root window's resources over 96, at least 1.
fn dpi_scale(x: &X, root: u32) -> f32 {
  let res = x.property(root, ATOM_RESOURCE_MANAGER, ATOM_STRING, false);
  let text = res.map(|p| String::from_utf8_lossy(&p.data).into_owned()).unwrap_or_default();
  resource(&text, "Xft.dpi")
    .and_then(|v| v.parse::<f32>().ok())
    .map_or(1.0, |d| (d / 96.0).max(1.0))
}

/// A resource's value in `xrdb` text (`name:\tvalue` lines).
fn resource<'a>(text: &'a str, name: &str) -> Option<&'a str> {
  text.lines().find_map(|l| {
    let (k, v) = l.split_once(':')?;
    (k.trim() == name).then(|| v.trim())
  })
}

fn px(points: f32, scale: f32) -> u32 {
  (points * scale).round().max(1.0) as u32
}

fn capitalized(s: &str) -> String {
  let mut cs = s.chars();
  cs.next().map(|c| c.to_uppercase().chain(cs).collect()).unwrap_or_default()
}

/// `WM_NORMAL_HINTS`: the least size, and the largest for a fixed window.
fn set_size_hints(x: &X, win: u32, min: Size, max: Option<Size>, scale: f32) {
  let mut h = [0u32; 18];
  h[0] = P_MIN_SIZE | max.map_or(0, |_| P_MAX_SIZE);
  (h[5], h[6]) = (px(min.w, scale), px(min.h, scale));
  if let Some(m) = max {
    (h[7], h[8]) = (px(m.w, scale), px(m.h, scale));
  }

  x.set_property32(win, ATOM_WM_NORMAL_HINTS, ATOM_WM_SIZE_HINTS, &h);
}

/// Dark when the XSETTINGS manager's `Net/ThemeName` is a dark theme (`Adwaita-dark`,
/// `Breeze-Dark`); light without one.
fn appearance(x: &X, owner: u32, prop: u32) -> Appearance {
  let data = match owner {
    0 => None,
    w => x.property(w, prop, prop, false).map(|p| p.data),
  };

  let theme = data.and_then(|d| xsettings_string(&d, "Net/ThemeName"));
  match theme {
    Some(t) if t.to_lowercase().contains("dark") => Appearance::DARK,
    _ => Appearance::LIGHT,
  }
}

/// A string setting in `_XSETTINGS_SETTINGS` (the XSETTINGS spec: a byte order, a serial, the
/// count, then each setting's type, name, serial and value, 4-byte aligned).
fn xsettings_string(d: &[u8], name: &str) -> Option<String> {
  let big = *d.first()? == 1;
  let u16_at = |i: usize| -> Option<u16> {
    let b = d.get(i..i + 2)?;
    Some(if big { u16::from_be_bytes([b[0], b[1]]) } else { u16::from_le_bytes([b[0], b[1]]) })
  };
  let u32_at = |i: usize| -> Option<u32> {
    let b: [u8; 4] = d.get(i..i + 4)?.try_into().ok()?;
    Some(if big { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
  };
  let pad = |n: usize| (n + 3) & !3;

  let count = u32_at(8)?;
  let mut i = 12;
  for _ in 0..count {
    let ty = *d.get(i)?;
    let len = u16_at(i + 2)? as usize;
    let key = d.get(i + 4..i + 4 + len)?;
    i += 4 + pad(len) + 4;
    match ty {
      // Integer.
      0 => i += 4,
      // String.
      1 => {
        let n = u32_at(i)? as usize;
        let value = d.get(i + 4..i + 4 + n)?;
        if key == name.as_bytes() {
          return String::from_utf8(value.to_vec()).ok();
        }

        i += 4 + pad(n);
      }
      // Colour.
      2 => i += 8,
      _ => return None,
    }
  }

  None
}

impl Win {
  fn next_event(&self) -> Option<Ev> {
    let stashed = self.stash.borrow_mut().pop_front();
    stashed.or_else(|| self.x.poll_event())
  }

  /// Whether the window shows: mapped, and not fully covered.
  fn visible(&self) -> bool {
    self.mapped && !self.obscured
  }

  /// A frame is drawn while the runtime wants one, the window shows (the first one before),
  /// the server has read the last one, and a tick passed since it started (at once after a
  /// resize).
  fn frame_due(&mut self, now: Instant) -> bool {
    let hidden = !self.visible() && self.frames > 0;
    if hidden || self.present.busy() || !self.rt.needs_frame() {
      return false;
    }

    self.force || self.pace.left(now).is_zero()
  }

  /// Until the loop must run again without input: the next tick while a frame is wanted; none
  /// while the server reads the last frame (its completion wakes us).
  fn timeout(&mut self) -> Option<Duration> {
    let now = Instant::now();
    if self.present.busy() {
      return Some(STUCK);
    }

    match self.visible() && self.rt.needs_frame() {
      true => Some(self.pace.left(now)),
      false => None,
    }
  }

  /// Builds a frame and puts it in the window if it changed.
  fn render(&mut self) {
    let t0 = Instant::now();
    self.pace.start(t0);
    let (scale, size) = (self.scale, self.size);
    self.rt.resize(size, scale);
    self.rt.frame();
    self.present.renderer.sync(&mut self.rt.images);
    let shown = self.present.present(&self.rt.scene, size, scale);
    self.force = false;
    if shown {
      self.frames += 1;
      if self.trace {
        let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        let since = self.started.elapsed().as_secs_f64() * 1e3;
        let (w, h) = self.present.renderer.size();
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        eprintln!(
          "nimbo: presented {:.3} (+{since:.1} ms) {w}x{h} in {ms:.2} ms",
          t.as_secs_f64() * 1e3
        );
      }
    }
  }

  /// Feeds an event to the runtime; returns whether it was used.
  fn send(&mut self, ev: Event) -> bool {
    if self.trace_events {
      eprintln!("nimbo: {ev:?}");
    }

    self.rt.event(ev)
  }

  fn handle(&mut self, ev: Ev) {
    match ev.kind() {
      FOCUS_IN | FOCUS_OUT => {
        let e = ev.get::<FocusEv>();
        let grab = matches!(e.mode, NOTIFY_MODE_GRAB | NOTIFY_MODE_UNGRAB);
        if grab || e.detail == NOTIFY_POINTER || e.detail == NOTIFY_INFERIOR {
          return;
        }

        self.focus(ev.kind() == FOCUS_IN);
      }
      EXPOSE => {
        let e = ev.get::<ExposeEv>();
        let (x, y) = (e.x as i32, e.y as i32);
        let rc = IRect::new(x, y, x + e.width as i32, y + e.height as i32);
        if !self.present.expose(rc) {
          self.rt.redraw();
        } else if self.trace && !self.exposed {
          let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
          let since = self.started.elapsed().as_secs_f64() * 1e3;
          eprintln!("nimbo: exposed {:.3} (+{since:.1} ms)", t.as_secs_f64() * 1e3);
        }

        self.exposed = true;
      }
      CONFIGURE_NOTIFY => {
        let e = ev.get::<ConfigureEv>();
        if e.window == self.win && (e.width as u32, e.height as u32) != self.px {
          self.px = (e.width as u32, e.height as u32);
          let (w, h) = (points(e.width as u32, self.scale), points(e.height as u32, self.scale));
          self.size = Size::new(w, h);
          self.force = true;
          self.rt.redraw();
        }
      }
      // The root's and the XSETTINGS manager's structure events come too: only ours count.
      MAP_NOTIFY if ev.get::<WindowEv>().window == self.win => {
        self.mapped = true;
        self.force = true;
        self.rt.redraw();
      }
      UNMAP_NOTIFY if ev.get::<WindowEv>().window == self.win => self.mapped = false,
      // Covered frames wait; uncovered, the frame due is drawn and the exposes put back the rest.
      VISIBILITY_NOTIFY => {
        let e = ev.get::<VisibilityEv>();
        if e.window == self.win {
          self.obscured = e.state == VISIBILITY_FULLY_OBSCURED;
        }
      }
      DESTROY_NOTIFY => {
        let w = ev.get::<WindowEv>().window;
        if w == self.win {
          self.closed = true;
        } else if w != 0 && w == self.xsettings {
          // The manager quit: none until another announces itself.
          self.watch_xsettings();
        }
      }
      CLIENT_MESSAGE => {
        let e = ev.get::<ClientMessageEv>();
        // A manager took the XSETTINGS selection (it started, or replaced the last).
        if e.ty == self.atoms.manager && e.data[1] == self.atoms.xsettings {
          self.watch_xsettings();
          return;
        }

        if e.ty != self.atoms.protocols {
          return;
        }

        if e.data[0] == self.atoms.delete {
          self.closed = true;
        } else if e.data[0] == self.atoms.ping {
          // Back to the root: the window manager sees we're alive.
          let mut out = ev.bytes();
          out[4..8].copy_from_slice(&self.root.to_ne_bytes());
          self.x.send_event(self.root, EM_SUBSTRUCTURE_NOTIFY | EM_SUBSTRUCTURE_REDIRECT, &out);
        }
      }
      PROPERTY_NOTIFY => {
        let e = ev.get::<PropertyEv>();
        if e.window == self.root && e.atom == ATOM_RESOURCE_MANAGER {
          let scale = dpi_scale(&self.x, self.root);
          if scale != self.scale {
            self.scale = scale;
            // The hints are in pixels: the same points need others at the new scale.
            set_size_hints(&self.x, self.win, self.min, self.max, scale);
            self.size = Size::new(points(self.px.0, scale), points(self.px.1, scale));
            self.force = true;
            self.rt.redraw();
          }
        } else if e.window == self.xsettings && e.atom == self.atoms.xsettings_settings {
          let a = appearance(&self.x, self.xsettings, self.atoms.xsettings_settings);
          self.send(Event::Appearance(a));
        }
      }
      0 if self.trace => {
        let b = ev.bytes();
        eprintln!(
          "nimbo: X error {} (request {}.{})",
          b[1],
          b[10],
          u16::from_ne_bytes([b[8], b[9]])
        );
      }
      k if Some(k) == self.present.completion() => {
        self.present.completed(ev.get::<ShmCompletionEv>().shmseg);
      }
      _ => {}
    }
  }

  /// Watches the XSETTINGS selection's owner now (none when it has none) and sends the
  /// appearance it sets.
  fn watch_xsettings(&mut self) {
    let owner = self.x.selection_owner(self.atoms.xsettings);
    if owner != 0 && owner != self.xsettings {
      self.x.select_events(owner, XSETTINGS_EVENTS);
    }

    self.xsettings = owner;
    let a = appearance(&self.x, owner, self.atoms.xsettings_settings);
    self.send(Event::Appearance(a));
  }

  fn focus(&mut self, on: bool) {
    if on == self.focused {
      return;
    }

    self.focused = on;
    self.send(Event::Focus(on));
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn resources_give_the_dpi() {
    let text = "Xcursor.theme:\tbreeze_cursors\nXft.dpi:\t144\nXft.hinting:\t1\n";
    assert_eq!(resource(text, "Xft.dpi"), Some("144"));
    assert_eq!(resource(text, "Xft.rgba"), None);
  }

  #[test]
  fn xsettings_strings_are_found() {
    // Little-endian: an integer `Net/DoubleClickTime`, then the string `Net/ThemeName`.
    let mut d = vec![0, 0, 0, 0];
    d.extend(7u32.to_le_bytes());
    d.extend(2u32.to_le_bytes());
    let name = b"Net/DoubleClickTime";
    d.extend([0, 0]);
    d.extend((name.len() as u16).to_le_bytes());
    d.extend(name);
    d.push(0);
    d.extend(0u32.to_le_bytes());
    d.extend(400i32.to_le_bytes());
    let (name, value) = (b"Net/ThemeName", b"Breeze-Dark");
    d.extend([1, 0]);
    d.extend((name.len() as u16).to_le_bytes());
    d.extend(name);
    d.extend([0, 0, 0]);
    d.extend(0u32.to_le_bytes());
    d.extend((value.len() as u32).to_le_bytes());
    d.extend(value);
    d.push(0);
    assert_eq!(xsettings_string(&d, "Net/ThemeName").as_deref(), Some("Breeze-Dark"));
    assert_eq!(xsettings_string(&d, "Gtk/FontName"), None);
  }
}
