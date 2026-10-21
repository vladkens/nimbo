// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Linux platform on X11, for sessions without Wayland (X11 desktops, remote displays):
//! the protocol through x11rb's pure-Rust connection, one top-level window the window manager
//! decorates, CPU frames through MIT-SHM or the socket ([present]) paced at [REFRESH], the scale
//! from `Xft.dpi` in the resources ([settings]; the size hints follow it), the keyboard through
//! xkbcommon ([super::xkb]: the server's keymap with libxkbcommon-x11 over a connection of
//! libxcb's own, else its rules names; the state from each event's modifiers and group), the
//! server's key repeat (detectable: presses alone), cursors from the theme or the cursor
//! font ([cursor]), the CLIPBOARD selection ([clip]), light or dark from XSETTINGS'
//! `Net/ThemeName`, its manager followed as it starts, quits or restarts. One thread: the loop
//! polls the connection and a wake eventfd with the next timer as the timeout.
//!
//! Not yet: input methods (XIM or IBus: typing goes through xkbcommon's compose only), smooth
//! and touchpad scrolling (XInput2: wheels come as clicks), pinch, `_NET_WM_SYNC_REQUEST`
//! (the window manager doesn't wait for our frame as it resizes), PRIMARY, the display's
//! refresh rate (RandR), per-monitor scale.

mod clip;
mod cursor;
mod present;
mod settings;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::CString;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};
use x11rb::connection::Connection;
use x11rb::cursor::Handle as CursorTheme;
use x11rb::properties::WmSizeHints;
use x11rb::protocol::Event as XEvent;
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
  AtomEnum, ChangeWindowAttributesAux, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux,
  EventMask, GetPropertyReply, Gravity, Mapping, NotifyDetail, NotifyMode, PropMode, Visibility,
  WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, NONE};

use self::clip::{Clip, Clipboard};
use self::cursor::Cursors;
use self::present::{Presenter, STUCK};
use self::settings::{appearance, dpi_scale, resources};
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
/// Buttons 1–5 held, in an event's state.
const BUTTONS_MASK: u16 = 0x1f00;

x11rb::atom_manager! {
  /// The atoms the window and the clipboard use, interned in one round trip; pasted text
  /// arrives in `NIMBO_CLIPBOARD`.
  pub Atoms: AtomsCookie {
    WM_PROTOCOLS,
    WM_DELETE_WINDOW,
    _NET_WM_PING,
    _NET_WM_NAME,
    _NET_WM_PID,
    WM_CLIENT_MACHINE,
    _XSETTINGS_SETTINGS,
    _XKB_RULES_NAMES,
    MANAGER,
    CLIPBOARD,
    TARGETS,
    UTF8_STRING,
    TEXT,
    MIME: b"text/plain;charset=utf-8",
    INCR,
    NIMBO_CLIPBOARD,
  }
}

struct Win {
  x: Rc<RustConnection>,
  win: u32,
  root: u32,
  atoms: Atoms,
  /// `_XSETTINGS_S<screen>`, the XSETTINGS manager's selection.
  xsettings_sel: u32,
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
  last_click: Option<Click>,
  kb: Option<Keyboard>,
  /// The keyboard was set up ([Win::setup_keyboard]).
  kb_ready: bool,
  mods: Mods,
  /// Keys down (by X keycode); with detectable autorepeat a press of one is a repeat.
  held: [u64; 4],
  detectable: bool,
  /// XKB's state events come ([Win::setup_keyboard]): they keep the modifiers
  /// ([Keyboard::state_notify]).
  xkb_state: bool,
  /// The keyboard state wasn't read from the server (a keymap from the rules names): events'
  /// core states set it until the server's state events follow.
  kb_fresh: bool,
  /// Modifier keys that went down or up, held for the state event the server sends after
  /// them: [Event::Modifier] comes after the [Event::Mods] it changed.
  mod_keys: Vec<(ModKey, bool)>,
  /// A keymap change came (`MappingNotify`, XKB's): the keymap is read again after the batch.
  reload_keymap: bool,
  focused: bool,
  clip: Rc<RefCell<Clip>>,
  /// Events read ahead (a paste waiting for its owner, a key release looking for its repeat).
  stash: Rc<RefCell<VecDeque<XEvent>>>,
  /// The connection failed (a read or a write): the loop ends.
  broken: bool,
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
  let (x, n) = x11rb::connect(None).map_err(|e| {
    let d = std::env::var("DISPLAY").unwrap_or_default();
    format!("can't connect to the X server (DISPLAY={d:?}): {e}")
  })?;
  run_on(Rc::new(x), n, cfg, app, started)
}

fn run_on(
  x: Rc<RustConnection>,
  n: usize,
  cfg: &Config,
  app: Box<dyn App>,
  started: Instant,
) -> Res<()> {
  let sc = &x.setup().roots[n];
  let (root, visual, depth) = (sc.root, sc.root_visual, sc.root_depth);
  if !matches!(depth, 24 | 32) {
    return Err(format!("the X screen has depth {depth}, 24 or 32 needed").into());
  }

  let atoms = Atoms::new(&*x)?;
  let xsettings_sel = x.intern_atom(false, format!("_XSETTINGS_S{n}").as_bytes())?;
  let (atoms, xsettings_sel) = (atoms.reply()?, xsettings_sel.reply()?.atom);

  let db = resources(&x);
  let scale = dpi_scale(&db);
  let px = (px(cfg.size.w, scale), px(cfg.size.h, scale));
  let events = EventMask::KEY_PRESS
    | EventMask::KEY_RELEASE
    | EventMask::BUTTON_PRESS
    | EventMask::BUTTON_RELEASE
    | EventMask::ENTER_WINDOW
    | EventMask::LEAVE_WINDOW
    | EventMask::POINTER_MOTION
    | EventMask::EXPOSURE
    | EventMask::VISIBILITY_CHANGE
    | EventMask::STRUCTURE_NOTIFY
    | EventMask::FOCUS_CHANGE
    | EventMask::PROPERTY_CHANGE;
  // No background (nothing is cleared before our frame), the content kept at the top left as
  // it resizes.
  let aux = CreateWindowAux::new()
    .background_pixmap(NONE)
    .bit_gravity(Gravity::NORTH_WEST)
    .event_mask(events);
  let win = x.generate_id()?;
  let (w, h, class) = (px.0 as u16, px.1 as u16, WindowClass::INPUT_OUTPUT);
  x.create_window(COPY_DEPTH_FROM_PARENT, win, root, 0, 0, w, h, 0, class, visual, &aux)?;

  // Title, class, protocols, the process for `_NET_WM_PING`, sizes.
  let title = cfg.title;
  let latin1: Vec<u8> = title.chars().map(|c| u8::try_from(c).unwrap_or(b'?')).collect();
  let (replace, string) = (PropMode::REPLACE, AtomEnum::STRING);
  x.change_property8(replace, win, AtomEnum::WM_NAME, string, &latin1)?;
  x.change_property8(replace, win, atoms._NET_WM_NAME, atoms.UTF8_STRING, title.as_bytes())?;
  let exe = std::env::current_exe().ok();
  let id = exe.as_ref().and_then(|p| p.file_stem()?.to_str()).unwrap_or("nimbo");
  let class = format!("{id}\0{}\0", capitalized(id));
  x.change_property8(replace, win, AtomEnum::WM_CLASS, string, class.as_bytes())?;
  let protocols = [atoms.WM_DELETE_WINDOW, atoms._NET_WM_PING];
  x.change_property32(replace, win, atoms.WM_PROTOCOLS, AtomEnum::ATOM, &protocols)?;
  x.change_property32(replace, win, atoms._NET_WM_PID, AtomEnum::CARDINAL, &[std::process::id()])?;
  if let Ok(host) = std::fs::read_to_string("/proc/sys/kernel/hostname") {
    x.change_property8(replace, win, atoms.WM_CLIENT_MACHINE, string, host.trim().as_bytes())?;
  }

  let (min, max) = match cfg.resizable {
    true => (cfg.min(), None),
    false => (cfg.size, Some(cfg.size)),
  };

  set_size_hints(&x, win, min, max, scale);

  // Changes of the resources (Xft.dpi), a new XSETTINGS manager (`MANAGER` on the root), and the
  // manager's settings and end.
  watch(&x, root);
  let xs_owner = selection_owner(&x, xsettings_sel);
  if xs_owner != NONE {
    watch(&x, xs_owner);
  }

  let stash = Rc::new(RefCell::new(VecDeque::new()));
  let clip = Rc::new(RefCell::new(Clip {
    x: x.clone(),
    win,
    atoms,
    stash: stash.clone(),
    time: 0,
    ours: None,
  }));
  let mut rt = Runtime::new(app, cfg.size, scale, Clock::real(), Box::new(Clipboard(clip.clone())));
  rt.event(Event::Appearance(appearance(&x, xs_owner, atoms._XSETTINGS_SETTINGS)));

  // A wake from any thread writes the eventfd the loop polls.
  let wake: Arc<OwnedFd> = Arc::new(eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)?);
  let notify = wake.clone();
  rt.waker().set_notify(Box::new(move || {
    let _ = rustix::io::write(&*notify, &1u64.to_ne_bytes());
  }));

  // Every round trip before the map: after it they wait for the window manager's work.
  let theme = CursorTheme::new(&*x, n, &db).ok().and_then(|c| c.reply().ok());
  let mut present = Presenter::new(x.clone(), win, depth)?;
  present.reserve(px.0 as usize, px.1 as usize);
  let mut win = Win {
    x: x.clone(),
    win,
    root,
    atoms,
    xsettings_sel,
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
    cursors: Cursors::new(theme),
    mouse: Point::new(0.0, 0.0),
    inside: false,
    last_click: None,
    kb: None,
    kb_ready: false,
    mods: Mods::NONE,
    held: [0; 4],
    detectable: false,
    xkb_state: false,
    kb_fresh: true,
    mod_keys: Vec::new(),
    reload_keymap: false,
    focused: false,
    clip,
    stash,
    broken: false,
    xsettings: xs_owner,
    min,
    max,
  };
  if win.trace {
    let shm = if win.present.shm() { "MIT-SHM" } else { "PutImage" };
    let cursors = if win.cursors.themed() { "themed" } else { "font" };
    eprintln!("nimbo: x11, scale {scale}, frames by {shm}, {cursors} cursors");
  }
  // The window manager maps it while we draw the first frame, which the first expose puts;
  // the keyboard (8–10 ms: the server's keymap, the compose table) comes after that frame.
  x.map_window(win.win)?;
  x.flush()?;

  loop {
    while let Some(ev) = win.next_event() {
      win.handle(ev);
    }

    // A modifier key that left the state as it was (the other Shift) has no state event.
    win.modifier_keys();

    win.apply_keymap();

    if win.broken {
      return Err("the X connection broke".into());
    }

    if win.closed || win.rt.quit_requested() {
      break;
    }

    win.apply_resize();
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
      // A size the frame asked for, before the loop sleeps.
      win.apply_resize();
    }

    if !win.kb_ready && win.frames > 0 {
      win.setup_keyboard();
    }

    win.broken |= x.flush().is_err();

    // Events read while waiting for a reply, or stashed after the loop drained them (a paste in
    // the view), aren't on the socket any more: back at the front, so they keep their order.
    if let Some(ev) = win.next_event() {
      win.stash.borrow_mut().push_front(ev);
      continue;
    }

    let timeout = win.timeout().map(timespec);
    let mut fds = [PollFd::new(x.stream(), PollFlags::IN), PollFd::new(&*wake, PollFlags::IN)];
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
  x.destroy_window(win.win)?;
  x.flush()?;
  Ok(())
}

fn timespec(d: Duration) -> Timespec {
  Timespec { tv_sec: d.as_secs() as _, tv_nsec: d.subsec_nanos() as _ }
}

/// Waits up to `limit` for an event `want` accepts; the others go to `stash` in order. None
/// when none came or the connection broke.
fn wait_event(
  x: &RustConnection,
  limit: Duration,
  stash: &mut VecDeque<XEvent>,
  want: impl Fn(&XEvent) -> bool,
) -> Option<XEvent> {
  let end = Instant::now() + limit;
  x.flush().ok()?;
  loop {
    while let Some(ev) = x.poll_for_event().ok()? {
      match want(&ev) {
        true => return Some(ev),
        false => stash.push_back(ev),
      }
    }

    let left = end.checked_duration_since(Instant::now()).filter(|d| !d.is_zero())?;
    let mut fds = [PollFd::new(x.stream(), PollFlags::IN)];
    let _ = poll(&mut fds, Some(&timespec(left)));
  }
}

/// A window's property of type `ty` (any with `AtomEnum::ANY`), whole; deleted after it is read
/// when `delete`. None when the request fails.
fn property(
  x: &RustConnection,
  win: u32,
  prop: u32,
  ty: impl Into<u32>,
  delete: bool,
) -> Option<GetPropertyReply> {
  x.get_property(delete, win, prop, ty, 0, u32::MAX / 4).ok()?.reply().ok()
}

/// A selection's owner; none when it has none or the request fails.
fn selection_owner(x: &RustConnection, selection: u32) -> u32 {
  let r = x.get_selection_owner(selection).ok().and_then(|c| c.reply().ok());
  r.map_or(NONE, |r| r.owner)
}

/// Selects a window's property changes and structure events, one that isn't ours (the root:
/// the resources, a new XSETTINGS manager; the manager's window: its settings and its end).
fn watch(x: &RustConnection, w: u32) {
  let events = EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY;
  let _ = x.change_window_attributes(w, &ChangeWindowAttributesAux::new().event_mask(events));
}

/// The keyboard: the server's keymap through libxkbcommon-x11, else one from the server's
/// rules names (`_XKB_RULES_NAMES`: rules, model, layout, variant, options).
fn keyboard(x: &RustConnection, root: u32, rules: u32) -> Option<Keyboard> {
  if let Some(kb) = Keyboard::from_x11() {
    return Some(kb);
  }

  let names = property(x, root, rules, AtomEnum::STRING, false).map(|r| r.value);
  let names = names.unwrap_or_default();
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
fn set_size_hints(x: &RustConnection, win: u32, min: Size, max: Option<Size>, scale: f32) {
  let size = |s: Size| (px(s.w, scale) as i32, px(s.h, scale) as i32);
  let hints =
    WmSizeHints { min_size: Some(size(min)), max_size: max.map(size), ..Default::default() };
  let _ = hints.set_normal_hints(x, win);
}

/// XKB for the connection, so core events carry the layout group; false without it.
fn use_xkb(x: &RustConnection) -> bool {
  let r = x.xkb_use_extension(1, 0).ok().and_then(|c| c.reply().ok());
  r.is_some_and(|r| r.supported)
}

/// Asks the server (XKB set up) to repeat a held key as presses alone, without the release
/// before each: a press of a key that is down is a repeat. False when it can't.
fn detectable_repeat(x: &RustConnection) -> bool {
  let (kbd, flag, none) =
    (xkb::ID::USE_CORE_KBD.into(), xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT, 0u32);
  let r = x.xkb_per_client_flags(kbd, flag, flag, none.into(), none.into(), none.into());
  r.ok().and_then(|c| c.reply().ok()).is_some_and(|r| r.value.contains(flag))
}

/// Asks the server for XKB's keymap events (`XkbNewKeyboardNotify`, `XkbMapNotify`: an
/// XKB-aware client gets no core `MappingNotify` when the keymap changes, `setxkbmap`) and its
/// state events (`XkbStateNotify`, [Keyboard::state_notify]).
fn select_events(x: &RustConnection) {
  let ev =
    xkb::EventType::NEW_KEYBOARD_NOTIFY | xkb::EventType::MAP_NOTIFY | xkb::EventType::STATE_NOTIFY;
  let parts = xkb::MapPart::from(0xffu16);
  let (kbd, aux) = (xkb::ID::USE_CORE_KBD.into(), xkb::SelectEventsAux::new());
  let _ = x.xkb_select_events(kbd, 0u16.into(), ev, parts, parts, &aux);
}

impl Win {
  /// The next event read ahead, else from the connection, without waiting.
  fn next_event(&mut self) -> Option<XEvent> {
    if let Some(ev) = self.stash.borrow_mut().pop_front() {
      return Some(ev);
    }

    let ev = self.x.poll_for_event();
    self.broken |= ev.is_err();
    ev.ok().flatten()
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

  /// Until the loop must run again without input: the next tick while a frame is wanted, else
  /// the next timer; none while the server reads the last frame (its completion wakes us), nor
  /// for a timer while the window is unmapped or covered (`MapNotify` or `VisibilityNotify` draws
  /// again).
  fn timeout(&mut self) -> Option<Duration> {
    let now = Instant::now();
    if self.present.busy() {
      return Some(STUCK);
    }

    match self.visible() && self.rt.needs_frame() {
      true => Some(self.pace.left(now)),
      false if self.visible() => self.rt.due_in(),
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
    self.present.renderer.sync(&mut self.rt.text, &mut self.rt.images);
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

  /// Asks the server for the size the app asked for ([Runtime::take_resize]); a fixed window
  /// gets it as its least and largest size too. The size changes when `ConfigureNotify` says
  /// so: a window manager may refuse it (a tiled window).
  fn apply_resize(&mut self) {
    let Some(size) = self.rt.take_resize() else { return };
    match self.max {
      None if self.min.w > size.w || self.min.h > size.h => {
        self.min = Size::new(self.min.w.min(size.w), self.min.h.min(size.h));
        set_size_hints(&self.x, self.win, self.min, None, self.scale);
      }
      None => {}
      Some(_) => {
        (self.min, self.max) = (size, Some(size));
        set_size_hints(&self.x, self.win, size, Some(size), self.scale);
      }
    }

    let (w, h) = (px(size.w, self.scale), px(size.h, self.scale));
    let _ = self.x.configure_window(self.win, &ConfigureWindowAux::new().width(w).height(h));
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
    let _ = self.x.change_window_attributes(self.win, &ChangeWindowAttributesAux::new().cursor(id));
  }

  fn point(&self, x: i16, y: i16) -> Point {
    Point::new(x as f32 / self.scale, y as f32 / self.scale)
  }

  /// The keyboard: XKB for the connection, its keymap, detectable autorepeat, the keymap
  /// events.
  fn setup_keyboard(&mut self) {
    self.kb_ready = true;
    let xkb = use_xkb(&self.x);
    if !xkb {
      eprintln!("nimbo: no XKB extension: the layout group is not followed");
    }

    self.kb = keyboard(&self.x, self.root, self.atoms._XKB_RULES_NAMES);
    if self.kb.is_none() {
      eprintln!("nimbo: no keyboard: libxkbcommon.so.0 is missing or has no keymap");
    }

    // Modifiers held as the window opened.
    self.kb_fresh = !self.kb.as_ref().is_some_and(|kb| kb.from_server);
    self.sync_mods();

    self.detectable = xkb && detectable_repeat(&self.x);
    if xkb {
      select_events(&self.x);
    }

    self.xkb_state = xkb;
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
      && (self.kb_fresh || !self.xkb_state)
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
      && let Some(kb) = keyboard(&self.x, self.root, self.atoms._XKB_RULES_NAMES)
    {
      self.kb_fresh = !kb.from_server;
      self.kb = Some(kb);
      self.sync_mods();
    }
  }

  fn handle(&mut self, ev: XEvent) {
    if matches!(ev, XEvent::KeyPress(_) | XEvent::KeyRelease(_)) {
      if !self.kb_ready {
        self.setup_keyboard();
      }

      self.apply_keymap();
    }

    // A button's press, the focus coming.
    let on = matches!(ev, XEvent::ButtonPress(_) | XEvent::FocusIn(_));
    match ev {
      XEvent::KeyPress(e) => {
        self.clip.borrow_mut().time = e.time;
        let (i, bit) = (e.detail as usize / 64, 1u64 << (e.detail % 64));
        let repeat = self.detectable && self.held[i] & bit != 0;
        self.held[i] |= bit;
        self.key(e.detail, e.state.into(), true, repeat);
      }
      XEvent::KeyRelease(e) => {
        self.held[e.detail as usize / 64] &= !(1u64 << (e.detail % 64));
        // Without detectable autorepeat the server repeats a held key as a release and a press
        // at the same time; the press may still be on its way (a read can end between them).
        let all = |_: &XEvent| true;
        if !self.detectable
          && let Some(next) = self
            .next_event()
            .or_else(|| wait_event(&self.x, PAIR_WAIT, &mut self.stash.borrow_mut(), all))
        {
          if let XEvent::KeyPress(n) = &next
            && n.detail == e.detail
            && n.time == e.time
          {
            self.key(n.detail, n.state.into(), true, true);
            return;
          }

          self.stash.borrow_mut().push_front(next);
        }

        self.key(e.detail, e.state.into(), false, false);
      }
      XEvent::ButtonPress(e) | XEvent::ButtonRelease(e) => {
        let down = on;
        if down {
          self.clip.borrow_mut().time = e.time;
        }

        self.core_state(e.state.into());
        self.mouse = self.point(e.event_x, e.event_y);
        self.button(e.detail, down, e.state.into(), e.time);
      }
      XEvent::MotionNotify(e) => {
        self.core_state(e.state.into());
        self.mouse = self.point(e.event_x, e.event_y);
        let (pos, mods) = (self.mouse, self.kb_mods());
        self.send(Event::MouseMove { pos, mods });
      }
      XEvent::EnterNotify(e) => {
        self.core_state(e.state.into());
        self.inside = true;
        self.mouse = self.point(e.event_x, e.event_y);
        let (pos, mods) = (self.mouse, self.kb_mods());
        self.send(Event::MouseMove { pos, mods });
      }
      XEvent::LeaveNotify(e) => {
        if e.detail == NotifyDetail::INFERIOR {
          return;
        }

        self.inside = false;
        // A drag keeps the pointer (an implicit grab): it exits when the button is released.
        if u16::from(e.state) & BUTTONS_MASK == 0 {
          self.send(Event::MouseExit);
        }
      }
      XEvent::FocusIn(e) | XEvent::FocusOut(e) => {
        let grab = matches!(e.mode, NotifyMode::GRAB | NotifyMode::UNGRAB);
        if grab || matches!(e.detail, NotifyDetail::POINTER | NotifyDetail::INFERIOR) {
          return;
        }

        self.focus(on);
      }
      XEvent::Expose(e) => {
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
      XEvent::ConfigureNotify(e) => {
        if e.window == self.win && (e.width as u32, e.height as u32) != self.px {
          self.px = (e.width as u32, e.height as u32);
          let (w, h) = (points(e.width as u32, self.scale), points(e.height as u32, self.scale));
          self.size = Size::new(w, h);
          self.force = true;
          self.rt.redraw();
        }
      }
      // The root's and the XSETTINGS manager's structure events come too: only ours count.
      XEvent::MapNotify(e) if e.window == self.win => {
        self.mapped = true;
        self.force = true;
        self.rt.redraw();
      }
      XEvent::UnmapNotify(e) if e.window == self.win => self.mapped = false,
      // Covered frames wait; uncovered, the frame due is drawn and the exposes put back the rest.
      XEvent::VisibilityNotify(e) if e.window == self.win => {
        self.obscured = e.state == Visibility::FULLY_OBSCURED;
      }
      XEvent::DestroyNotify(e) => {
        if e.window == self.win {
          self.closed = true;
        } else if e.window != NONE && e.window == self.xsettings {
          // The manager quit: none until another announces itself.
          self.watch_xsettings();
        }
      }
      XEvent::ClientMessage(mut e) => {
        let data = e.data.as_data32();
        // A manager took the XSETTINGS selection (it started, or replaced the last).
        if e.type_ == self.atoms.MANAGER && data[1] == self.xsettings_sel {
          self.watch_xsettings();
          return;
        }

        if e.type_ != self.atoms.WM_PROTOCOLS {
          return;
        }

        if data[0] == self.atoms.WM_DELETE_WINDOW {
          self.closed = true;
        } else if data[0] == self.atoms._NET_WM_PING {
          // Back to the root: the window manager sees we're alive.
          e.window = self.root;
          let mask = EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT;
          let _ = self.x.send_event(false, self.root, mask, e);
        }
      }
      XEvent::PropertyNotify(e) => {
        if e.window == self.root && e.atom == u32::from(AtomEnum::RESOURCE_MANAGER) {
          let scale = dpi_scale(&resources(&self.x));
          if scale != self.scale {
            self.scale = scale;
            // The hints are in pixels: the same points need others at the new scale.
            set_size_hints(&self.x, self.win, self.min, self.max, scale);
            self.size = Size::new(points(self.px.0, scale), points(self.px.1, scale));
            self.force = true;
            self.rt.redraw();
          }
        } else if e.window == self.xsettings && e.atom == self.atoms._XSETTINGS_SETTINGS {
          let a = appearance(&self.x, self.xsettings, self.atoms._XSETTINGS_SETTINGS);
          self.send(Event::Appearance(a));
        }
      }
      XEvent::SelectionRequest(e) => self.clip.borrow().request(&e),
      XEvent::SelectionClear(e) => self.clip.borrow_mut().cleared(&e),
      XEvent::MappingNotify(e) => self.reload_keymap |= e.request == Mapping::KEYBOARD,
      XEvent::XkbNewKeyboardNotify(_) | XEvent::XkbMapNotify(_) => self.reload_keymap = true,
      XEvent::XkbStateNotify(e) => {
        if let Some(kb) = &mut self.kb {
          let mods = [e.base_mods, e.latched_mods, e.locked_mods].map(u32::from);
          let groups =
            [e.base_group as i32 as u32, e.latched_group as i32 as u32, e.locked_group.into()];
          kb.state_notify(mods, groups);
        }

        self.kb_fresh = false;
        self.sync_mods();
        self.modifier_keys();
      }
      XEvent::ShmCompletion(e) => self.present.completed(e.shmseg),
      XEvent::Error(e) if self.trace => {
        let (kind, major, minor) = (e.error_kind, e.major_opcode, e.minor_opcode);
        eprintln!("nimbo: X error {kind:?} (request {major}.{minor})");
      }
      _ => {}
    }
  }

  /// Watches the XSETTINGS selection's owner now (none when it has none) and sends the
  /// appearance it sets.
  fn watch_xsettings(&mut self) {
    let owner = selection_owner(&self.x, self.xsettings_sel);
    if owner != NONE && owner != self.xsettings {
      watch(&self.x, owner);
    }

    self.xsettings = owner;
    let a = appearance(&self.x, owner, self.atoms._XSETTINGS_SETTINGS);
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
    let pointer = self.x.query_pointer(self.root).ok().and_then(|c| c.reply().ok());
    match pointer {
      Some(p) => self.core_state(p.mask.into()),
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
    let own = self.kb_fresh || !self.xkb_state;
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
    self.send(Event::MouseDown { pos, button, clicks, mods });
  }
}
