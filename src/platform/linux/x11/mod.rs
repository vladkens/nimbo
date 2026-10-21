// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Linux platform on X11, for sessions without Wayland (X11 desktops, remote displays):
//! XCB loaded at run time ([xcb]), one top-level window the window manager
//! decorates, CPU frames through MIT-SHM or the socket ([present]) paced at [REFRESH], the scale
//! from `Xft.dpi` in the resources ([settings]; the size hints follow it), the keyboard through
//! xkbcommon ([super::xkb]: the server's keymap with libxkbcommon-x11, else its rules names; the
//! state from each event's modifiers and group), the server's key repeat (detectable: presses
//! alone), cursors from the theme (libxcb-cursor) or the cursor font, the CLIPBOARD selection
//! ([clip]), light or dark from XSETTINGS' `Net/ThemeName`, its manager followed as it starts,
//! quits or restarts. No frames while the window is unmapped or fully covered
//! (`VisibilityNotify`; under a compositing manager it never is). One thread: the loop polls the
//! connection and a wake eventfd.
//!
//! Not yet: input methods (XIM or IBus: typing goes through xkbcommon's compose only), smooth
//! and touchpad scrolling (XInput2: wheels come as clicks), pinch, `_NET_WM_SYNC_REQUEST`
//! (the window manager doesn't wait for our frame as it resizes), PRIMARY, the display's
//! refresh rate (RandR), per-monitor scale, pixel formats other than 32-bit `0xRRGGBB` (a BGR
//! VNC server: refused at the start, [present::check_format]).

mod clip;
mod present;
mod settings;
mod xcb;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::{CStr, CString};
use std::os::fd::OwnedFd;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};

use self::clip::{Atoms as ClipAtoms, Clip, Clipboard};
use self::present::{Presenter, STUCK};
use self::settings::{appearance, dpi_scale};
use self::xcb::*;
use super::xkb::{Keyboard, Typed};
use super::{Click, Res, clicks};
use crate::Config;
use crate::app::{App, Clock, Event, Mods, MouseButton, Phase, Runtime, WHEEL_LINE};
use crate::cpu::raster::IRect;
use crate::hotkey::ModKey;
use crate::platform::{Pace, points};
use crate::scene::{Point, Size};
use crate::ui::Cursor;

/// The frame interval while the runtime animates: 60 Hz (the display's rate isn't read).
const REFRESH: Duration = Duration::from_micros(16_667);

/// How long a key release waits for the press that would make it a repeat, without detectable
/// autorepeat: the server sends both at once.
const PAIR_WAIT: Duration = Duration::from_millis(2);
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
/// Buttons 1–5 held, in an event's state.
const BUTTONS_MASK: u16 = 0x1f00;
const MAPPING_KEYBOARD: u8 = 1;

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
  rules: u32,
}

/// Cursors made on first use: the theme's, else the cursor font's.
struct Cursors {
  theme: Option<CursorContext>,
  font: u32,
  made: Vec<(Cursor, u32)>,
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

  cursor: Cursor,
  cursors: Cursors,
  mouse: Point,
  inside: bool,
  /// Buttons down: the pointer grab that brings their release ends if the window is unmapped.
  pressed: Vec<MouseButton>,
  last_click: Option<Click>,
  kb: Option<Keyboard>,
  /// The keyboard was set up ([Win::setup_keyboard]).
  kb_ready: bool,
  mods: Mods,
  /// Keys down (by X keycode); with detectable autorepeat a press of one is a repeat.
  held: [u64; 4],
  detectable: bool,
  /// XKB's event code, for its keymap events (the keymap is read again after a batch of them)
  /// and state events, which keep the modifiers once they come ([Keyboard::state_notify]).
  xkb_event: Option<u8>,
  /// The keyboard state wasn't read from the server (a keymap from the rules names): events'
  /// core states set it until the server's state events follow.
  kb_fresh: bool,
  /// Modifier keys that went down or up, held for the state event the server sends after
  /// them: [Event::Modifier] comes after the [Event::Mods] it changed.
  mod_keys: Vec<(ModKey, bool)>,
  reload_keymap: bool,
  focused: bool,
  clip: Rc<RefCell<Clip>>,
  /// Events read ahead (a paste waiting for its owner, a key release looking for its repeat).
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
  let [protocols, delete, ping, net_name, pid, machine, xsettings, xs_settings, rules] = x.atoms([
    "WM_PROTOCOLS",
    "WM_DELETE_WINDOW",
    "_NET_WM_PING",
    "_NET_WM_NAME",
    "_NET_WM_PID",
    "WM_CLIENT_MACHINE",
    xs_name.as_str(),
    "_XSETTINGS_SETTINGS",
    "_XKB_RULES_NAMES",
  ]);
  let [clipboard, targets, utf8, text, mime, incr, prop, manager] = x.atoms([
    "CLIPBOARD",
    "TARGETS",
    "UTF8_STRING",
    "TEXT",
    "text/plain;charset=utf-8",
    "INCR",
    "NIMBO_CLIPBOARD",
    "MANAGER",
  ]);
  let clip_atoms = ClipAtoms { clipboard, targets, utf8, text, mime, incr, prop };
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
    rules,
  };

  let scale = dpi_scale(&x, sc.root);
  let px = (px(cfg.size.w, scale), px(cfg.size.h, scale));
  let events = EM_KEY_PRESS
    | EM_KEY_RELEASE
    | EM_BUTTON_PRESS
    | EM_BUTTON_RELEASE
    | EM_ENTER_WINDOW
    | EM_LEAVE_WINDOW
    | EM_POINTER_MOTION
    | EM_EXPOSURE
    | EM_VISIBILITY_CHANGE
    | EM_STRUCTURE_NOTIFY
    | EM_FOCUS_CHANGE
    | EM_PROPERTY_CHANGE;
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
  let clip = Rc::new(RefCell::new(Clip {
    x,
    win,
    atoms: clip_atoms,
    stash: stash.clone(),
    time: 0,
    ours: None,
  }));
  let mut rt = Runtime::new(app, cfg.size, scale, Clock::real(), Box::new(Clipboard(clip.clone())));
  rt.event(Event::Appearance(appearance(&x, xs_owner, xs_settings)));

  // A wake from any thread writes the eventfd the loop polls.
  let wake: Arc<OwnedFd> = Arc::new(eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)?);
  let notify = wake.clone();
  rt.waker().set_notify(Box::new(move || {
    let _ = rustix::io::write(&*notify, &1u64.to_ne_bytes());
  }));

  // Every round trip before the map: after it they wait for the window manager's work.
  let theme = x.cursor_context(screen);
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
    cursor: Cursor::Arrow,
    cursors: Cursors { theme, font: 0, made: Vec::new() },
    mouse: Point::new(0.0, 0.0),
    inside: false,
    last_click: None,
    kb: None,
    kb_ready: false,
    mods: Mods::NONE,
    held: [0; 4],
    pressed: Vec::new(),
    detectable: false,
    xkb_event: None,
    kb_fresh: true,
    mod_keys: Vec::new(),
    reload_keymap: false,
    focused: false,
    clip,
    stash,
    xsettings: xs_owner,
    min,
    max,
  };
  if win.trace {
    let shm = if win.present.completion().is_some() { "MIT-SHM" } else { "PutImage" };
    let cursors = if win.cursors.theme.is_some() { "themed" } else { "font" };
    eprintln!("nimbo: x11, scale {scale}, frames by {shm}, {cursors} cursors");
  }
  // The window manager maps it while we draw the first frame, which the first expose puts;
  // the keyboard (8–10 ms: the server's keymap, the compose table) comes after that frame.
  x.map_window(win.win);
  x.flush();

  loop {
    while let Some(ev) = win.next_event() {
      win.handle(ev);
    }

    // A modifier key that left the state as it was (the other Shift) has no state event.
    win.modifier_keys();

    win.apply_keymap();

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

    if !win.kb_ready && win.frames > 0 {
      win.setup_keyboard();
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

/// The keyboard: the server's keymap through libxkbcommon-x11, else one from the server's
/// rules names (`_XKB_RULES_NAMES`: rules, model, layout, variant, options) with the extension
/// set up through libxcb-xkb.
fn keyboard(x: &X, root: u32, rules: u32) -> Option<Keyboard> {
  if let Some(kb) = Keyboard::from_x11(x.raw()) {
    return Some(kb);
  }

  if !Keyboard::use_xkb_extension(x.raw()) {
    eprintln!("nimbo: no XKB extension: the layout group is not followed");
  }

  let names = x.property(root, rules, ATOM_STRING, false).map(|p| p.data).unwrap_or_default();
  let parts: Vec<CString> =
    names.split(|&b| b == 0).take(5).map(|s| CString::new(s).unwrap_or_default()).collect();
  let get = |i: usize| parts.get(i).filter(|s| !s.is_empty()).map(|s| s.as_c_str());
  Keyboard::from_names([get(0), get(1), get(2), get(3), get(4)])
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

    self.sync_cursor();
  }

  /// Feeds an event to the runtime; returns whether it was used.
  fn send(&mut self, ev: Event) -> bool {
    if self.trace_events {
      eprintln!("nimbo: {ev:?}");
    }

    // A ConfigureNotify in the same batch already counts for the input after it.
    self.rt.resize(self.size, self.scale);
    let used = self.rt.event(ev);
    self.sync_cursor();
    used
  }

  fn sync_cursor(&mut self) {
    let c = self.rt.cursor();
    if c == self.cursor {
      return;
    }

    self.cursor = c;
    let id = self.cursors.get(&self.x, c);
    self.x.set_cursor(self.win, id);
  }

  fn point(&self, x: i16, y: i16) -> Point {
    Point::new(x as f32 / self.scale, y as f32 / self.scale)
  }

  /// The keyboard: its keymap, detectable autorepeat, the keymap events.
  fn setup_keyboard(&mut self) {
    let x = self.x.raw();
    self.kb_ready = true;
    self.kb = keyboard(&self.x, self.root, self.atoms.rules);
    if self.kb.is_none() {
      eprintln!("nimbo: no keyboard: libxkbcommon.so.0 is missing or has no keymap");
    }

    // Modifiers held as the window opened.
    self.kb_fresh = !self.kb.as_ref().is_some_and(|kb| kb.from_server);
    self.sync_mods();

    self.detectable = Keyboard::detectable_repeat(x);
    let ev = self.x.extension_event("XKEYBOARD");
    self.xkb_event = ev.filter(|_| Keyboard::select_events(x));
    if self.trace {
      let repeat = if self.detectable { "detectable" } else { "paired" };
      let keymap = if Keyboard::has_x11() { "the server's" } else { "the rules names'" };
      eprintln!("nimbo: {keymap} keymap, {repeat} repeat");
    }
  }

  /// Sends the modifier keys held back for their state event.
  fn modifier_keys(&mut self) {
    for (key, down) in std::mem::take(&mut self.mod_keys) {
      self.send(Event::Modifier { key, down });
    }
  }

  /// An event's modifiers and group, as the state before it: the keyboard's state, unless
  /// the server's state events keep it (or its state came from the server).
  fn core_state(&mut self, state: u16) {
    if let Some(kb) = &mut self.kb
      && (self.kb_fresh || self.xkb_event.is_none())
    {
      kb.set_core_state(state);
    }

    self.sync_mods();
  }

  /// The keyboard's modifiers, as the last event's state set them.
  fn kb_mods(&self) -> Mods {
    self.kb.as_ref().map_or(Mods::NONE, |kb| kb.mods())
  }

  /// Sends [Event::Mods] when they changed; none while the window lacks the focus (their key
  /// presses go to another window, the pointer's events still carry them).
  fn sync_mods(&mut self) {
    let mods = if self.focused { self.kb_mods() } else { Mods::NONE };
    if mods != self.mods {
      self.mods = mods;
      self.send(Event::Mods(mods));
    }
  }

  /// A keymap change seen (`MappingNotify`, XKB's notify): the new one, before the next key is
  /// read with it; changes in a row are read once.
  fn apply_keymap(&mut self) {
    if std::mem::take(&mut self.reload_keymap)
      && let Some(kb) = keyboard(&self.x, self.root, self.atoms.rules)
    {
      self.kb_fresh = !kb.from_server;
      self.kb = Some(kb);
      self.sync_mods();
    }
  }

  fn handle(&mut self, ev: Ev) {
    if matches!(ev.kind(), KEY_PRESS | KEY_RELEASE) {
      if !self.kb_ready {
        self.setup_keyboard();
      }

      self.apply_keymap();
    }

    match ev.kind() {
      KEY_PRESS => {
        let e = ev.get::<InputEv>();
        self.clip.borrow_mut().time = e.time;
        let (i, bit) = (e.detail as usize / 64, 1u64 << (e.detail % 64));
        let repeat = self.detectable && self.held[i] & bit != 0;
        self.held[i] |= bit;
        self.key(e.detail, e.state, true, repeat);
      }
      KEY_RELEASE => {
        let e = ev.get::<InputEv>();
        self.held[e.detail as usize / 64] &= !(1u64 << (e.detail % 64));
        // Without detectable autorepeat the server repeats a held key as a release and a press
        // at the same time; the press may still be on its way (a read can end between them).
        let all = |_: &Ev| true;
        if !self.detectable
          && let Some(next) = self
            .next_event()
            .or_else(|| self.x.wait_event(PAIR_WAIT, &mut self.stash.borrow_mut(), all))
        {
          let n = next.get::<InputEv>();
          if next.kind() == KEY_PRESS && n.detail == e.detail && n.time == e.time {
            self.key(n.detail, n.state, true, true);
            return;
          }

          self.stash.borrow_mut().push_front(next);
        }

        self.key(e.detail, e.state, false, false);
      }
      BUTTON_PRESS | BUTTON_RELEASE => {
        let e = ev.get::<InputEv>();
        let down = ev.kind() == BUTTON_PRESS;
        if down {
          self.clip.borrow_mut().time = e.time;
        }

        self.core_state(e.state);
        self.mouse = self.point(e.event_x, e.event_y);
        self.button(e.detail, down, e.state, e.time);
      }
      MOTION_NOTIFY => {
        let e = ev.get::<InputEv>();
        self.core_state(e.state);
        self.mouse = self.point(e.event_x, e.event_y);
        let (pos, mods) = (self.mouse, self.kb_mods());
        self.send(Event::MouseMove { pos, mods });
      }
      ENTER_NOTIFY => {
        let e = ev.get::<CrossingEv>();
        self.core_state(e.state);
        self.inside = true;
        self.mouse = self.point(e.event_x, e.event_y);
        let (pos, mods) = (self.mouse, self.kb_mods());
        self.send(Event::MouseMove { pos, mods });
      }
      LEAVE_NOTIFY => {
        let e = ev.get::<CrossingEv>();
        if e.detail == NOTIFY_INFERIOR {
          return;
        }

        self.inside = false;
        // A drag keeps the pointer (an implicit grab): it exits when the button is released.
        if e.state & BUTTONS_MASK == 0 {
          self.send(Event::MouseExit);
        }
      }
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
      UNMAP_NOTIFY if ev.get::<WindowEv>().window == self.win => {
        self.mapped = false;
        self.let_go();
      }
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
      SELECTION_REQUEST => self.clip.borrow().request(ev.get::<SelectionRequestEv>()),
      SELECTION_CLEAR => self.clip.borrow_mut().cleared(ev.get::<SelectionClearEv>()),
      MAPPING_NOTIFY => self.reload_keymap |= ev.get::<MappingEv>().request == MAPPING_KEYBOARD,
      // XkbNewKeyboardNotify, XkbMapNotify, XkbStateNotify (the XKB event type is the second
      // byte).
      k if Some(k) == self.xkb_event => match ev.bytes()[1] {
        0 | 1 => self.reload_keymap = true,
        2 => {
          if let Some(kb) = &mut self.kb {
            kb.state_notify(&ev.bytes());
          }

          self.kb_fresh = false;
          self.sync_mods();
          self.modifier_keys();
        }
        _ => {}
      },
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
    self.held = [0; 4];
    if !on {
      self.sync_mods();
      self.send(Event::Focus(false));
      return;
    }

    // Modifiers pressed or let go in another window: those held now.
    self.send(Event::Focus(true));
    match self.x.pointer_state(self.root) {
      Some(state) => self.core_state(state),
      None => self.sync_mods(),
    }
  }

  // MARK: Keys

  /// A key pressed or released (X keycode: evdev + 8, `state` before it): to the app as a key
  /// event, then, unused, through the keyboard's compose state and the Linux editing keys into
  /// text or a command.
  fn key(&mut self, keycode: u8, state: u16, down: bool, repeat: bool) {
    let key = (keycode as u32).saturating_sub(8);
    let Some(kb) = &mut self.kb else { return };
    // Without the server's state events, the state from the event's and the key's own change.
    let own = self.kb_fresh || self.xkb_event.is_none();
    if own {
      kb.set_core_state(state);
    }

    let (k, utf8) = kb.key(key, repeat);
    let modifier = kb.modifier(key, k.code);
    if own && !repeat {
      kb.update_key(key, down);
    }

    self.sync_mods();

    // Modifier keys reach the app as [Event::Mods] and, by their side, [Event::Modifier], as on
    // macOS (`flagsChanged:`).
    if let Some(side) = modifier {
      match side.filter(|_| !repeat) {
        Some(key) if own => _ = self.send(Event::Modifier { key, down }),
        Some(key) => self.mod_keys.push((key, down)),
        None => {}
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

  // MARK: Pointer

  /// Buttons 1–3 click, 4–7 scroll a wheel click up, down, left, right, 8 and 9 are back and
  /// forward (as evdev's side and extra buttons on Wayland).
  fn button(&mut self, detail: u8, down: bool, state: u16, time: u32) {
    let (pos, mods) = (self.mouse, self.kb_mods());
    let wheel = match detail {
      4 => Some(Point::new(0.0, WHEEL_LINE)),
      5 => Some(Point::new(0.0, -WHEEL_LINE)),
      6 => Some(Point::new(WHEEL_LINE, 0.0)),
      7 => Some(Point::new(-WHEEL_LINE, 0.0)),
      _ => None,
    };

    if let Some(delta) = wheel {
      if down {
        let (precise, phase, momentum) = (false, Phase::None, Phase::None);
        self.send(Event::Scroll { pos, delta, precise, phase, momentum, mods });
      }

      return;
    }

    let button = match detail {
      1 => MouseButton::Left,
      2 => MouseButton::Middle,
      3 => MouseButton::Right,
      8 => MouseButton::Other(3),
      9 => MouseButton::Other(4),
      b => MouseButton::Other(b),
    };

    if !down {
      self.pressed.retain(|&b| b != button);
      self.send(Event::MouseUp { pos, button, mods });
      // The last button of a drag that left the window.
      let this = if (1..=5).contains(&detail) { 0x80 << detail } else { 0 };
      let held = state & BUTTONS_MASK & !this;
      if !self.inside && held == 0 {
        self.send(Event::MouseExit);
      }

      return;
    }

    let clicks = clicks(self.last_click, button, time, pos);
    self.last_click = Some((button, time, pos, clicks));
    if !self.pressed.contains(&button) {
      self.pressed.push(button);
    }

    self.send(Event::MouseDown { pos, button, clicks, mods });
  }

  /// The window was unmapped (minimized, hidden): the server ended the pointer's grab with it,
  /// so the buttons held let go where the pointer was last seen and it is out of the window.
  fn let_go(&mut self) {
    let (pos, mods) = (self.mouse, self.kb_mods());
    let held = std::mem::take(&mut self.pressed);
    let out = self.inside || !held.is_empty();
    for button in held {
      self.send(Event::MouseUp { pos, button, mods });
    }

    if out {
      self.inside = false;
      self.send(Event::MouseExit);
    }
  }
}

impl Cursors {
  /// The X cursor for ours: the theme's by its CSS name or the legacy one, else the cursor
  /// font's glyph; the arrow is the parent's (the desktop's) without a theme.
  fn get(&mut self, x: &X, c: Cursor) -> u32 {
    if let Some(&(_, id)) = self.made.iter().find(|m| m.0 == c) {
      return id;
    }

    let (names, glyph): (&[&CStr], u16) = match c {
      Cursor::Inherit | Cursor::Arrow => (&[c"default", c"left_ptr"], 68),
      Cursor::Text => (&[c"text", c"xterm"], 152),
      Cursor::Pointer => (&[c"pointer", c"hand2"], 60),
      Cursor::ResizeX => (&[c"col-resize", c"sb_h_double_arrow"], 108),
      Cursor::ResizeY => (&[c"row-resize", c"sb_v_double_arrow"], 116),
      Cursor::Crosshair => (&[c"crosshair"], 34),
      Cursor::Grab => (&[c"grab", c"openhand", c"hand1"], 58),
      Cursor::Grabbing => (&[c"grabbing", c"closedhand", c"fleur"], 52),
      Cursor::NotAllowed => (&[c"not-allowed", c"crossed_circle"], 0),
    };

    let id = match (&self.theme, c) {
      (Some(t), _) => t.load(names).unwrap_or_else(|| x.font_cursor(&mut self.font, glyph)),
      (None, Cursor::Inherit | Cursor::Arrow) => 0,
      (None, _) => x.font_cursor(&mut self.font, glyph),
    };

    self.made.push((c, id));
    id
  }
}
