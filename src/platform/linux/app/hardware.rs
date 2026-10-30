// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Hardware on Linux ([crate::hardware]) and its changes ([crate::system]): no transport (ALSA
//! tells none); the lid from UPower over the system bus (`LidIsPresent`, `LidIsClosed`, their
//! `PropertiesChanged`); the audio inputs and the default input from PulseAudio's subscription
//! (`libpulse.so.0`, loaded at run time, on its threaded main loop), which PipeWire serves too
//! through `pipewire-pulse`. Constants from PulseAudio's `pulse/def.h` and `introspect.h`
//! (master, read 2026-10-30).

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, c_int, c_long, c_uint};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::super::dbus::{self, Gv};
use super::super::dl::{Lib, fns};
use super::super::{Fp, P, S};
use crate::hardware::Transport;
use crate::system::{Change, notice, refused};

const UPOWER: &str = "org.freedesktop.UPower";
const UPOWER_PATH: &str = "/org/freedesktop/UPower";
/// How long [lid_closed] waits for UPower.
const UPOWER_MS: c_int = 500;

pub fn transport(_id: &str) -> Transport {
  Transport::Unknown
}

/// UPower's lid: `None` without one, without UPower (never started by this read) or off the
/// bus. A synchronous call on the system bus: about a millisecond, at most half a second when
/// the bus hangs.
pub fn lid_closed() -> Option<bool> {
  let bus = dbus::system().ok()?;
  let props = dbus::properties(bus, UPOWER, UPOWER_PATH, UPOWER, UPOWER_MS).ok()?;
  let present = props.get("LidIsPresent").is_some_and(|v| v.bool());
  present.then(|| props.get("LidIsClosed").is_some_and(|v| v.bool()))
}

pub fn watch(kind: Change) -> Result<(), String> {
  match kind {
    Change::AudioInputs | Change::DefaultInput => watch_audio(),
    Change::Lid => watch_lid(),
  }
}

pub fn unwatch() {
  if let Some((id, name)) = LID.take()
    && let Ok(bus) = dbus::system()
  {
    dbus::unsubscribe(bus, id);
    dbus::unwatch(name);
  }

  unwatch_audio();
}

// MARK: Lid

thread_local! {
  /// UPower's subscription and its name's watch while the lid is watched.
  static LID: Cell<Option<(c_uint, c_uint)>> = const { Cell::new(None) };
}

/// UPower's property changes, from when it runs (none on a desktop, nothing to wait for), and
/// UPower itself coming or going: a new daemon starts without a signal, perhaps without the lid.
fn watch_lid() -> Result<(), String> {
  if LID.get().is_some() {
    return Ok(());
  }

  let bus = dbus::system()?;
  let iface = "org.freedesktop.DBus.Properties";
  let cb = on_upower as Fp;
  let path = Some(UPOWER_PATH);
  let id = dbus::subscribe(bus, UPOWER, iface, "PropertiesChanged", path, cb, (null_mut(), null()));
  let (came, went) = (upower_came as Fp, upower_went as Fp);
  let name = dbus::watch(bus, UPOWER, came, went, (null_mut(), null()));
  LID.set(Some((id, name)));
  Ok(())
}

/// UPower's name has an owner: as it is watched, and when a new daemon starts.
unsafe extern "C" fn upower_came(_: P, _: S, _: S, _: P) {
  notice(Change::Lid);
}

unsafe extern "C" fn upower_went(_: P, _: S, _: P) {
  notice(Change::Lid);
}

/// `PropertiesChanged(interface, changed, invalidated)`.
unsafe extern "C" fn on_upower(_: P, _: S, _: S, _: S, _: S, args: P, _: P) {
  if lid_told(&Gv::borrowed(args)) {
    notice(Change::Lid);
  }
}

/// Whether UPower's change is the lid's.
fn lid_told(args: &Gv) -> bool {
  let changed = args.child(1);
  args.child(0).str() == UPOWER
    && ["LidIsClosed", "LidIsPresent"].iter().any(|k| changed.get(k).is_some())
}

// MARK: Audio

fns!(Pulse {
  fn pa_threaded_mainloop_new() -> P;
  fn pa_threaded_mainloop_get_api(m: P) -> P;
  fn pa_threaded_mainloop_start(m: P) -> c_int;
  fn pa_threaded_mainloop_stop(m: P);
  fn pa_threaded_mainloop_free(m: P);
  fn pa_mainloop_api_once(api: P, f: Fp, data: P);
  fn pa_context_new(api: P, name: S) -> P;
  fn pa_context_unref(c: P);
  fn pa_context_connect(c: P, server: S, flags: c_int, spawn: P) -> c_int;
  fn pa_context_disconnect(c: P);
  fn pa_context_get_state(c: P) -> c_int;
  fn pa_context_set_state_callback(c: P, f: Fp, data: P);
  fn pa_context_set_subscribe_callback(c: P, f: Fp, data: P);
  fn pa_context_subscribe(c: P, mask: c_uint, f: Fp, data: P) -> P;
  fn pa_context_get_server_info(c: P, f: Fp, data: P) -> P;
  fn pa_operation_unref(o: P);
});

/// `PA_CONTEXT_NOAUTOSPAWN | PA_CONTEXT_NOFAIL`: no server is started for us, and with none the
/// context waits in `CONNECTING` for one to appear.
const FLAGS: c_int = 1 | 2;
/// `PA_CONTEXT_READY`, `PA_CONTEXT_FAILED`, `PA_CONTEXT_TERMINATED`.
const READY: c_int = 4;
const FAILED: c_int = 5;
const TERMINATED: c_int = 6;
/// `PA_SUBSCRIPTION_MASK_SOURCE | PA_SUBSCRIPTION_MASK_SERVER`.
const MASK: c_uint = 0x2 | 0x80;

/// `pa_server_info` up to the default source: four strings, `pa_sample_spec` (a format enum, a
/// rate, a channel count), then the default sink's and source's names.
#[repr(C)]
struct ServerInfo {
  _user: S,
  _host: S,
  _version: S,
  _name: S,
  _format: c_int,
  _rate: u32,
  _channels: u8,
  _sink: S,
  source: S,
}

/// What a subscription event (`pa_subscription_event_type_t`) tells.
#[derive(Debug, PartialEq)]
enum Told {
  /// A source came or went: the inputs.
  Source,
  /// The server changed: perhaps its default source.
  Server,
}

/// A source's `NEW` (0) or `REMOVE` (0x20), the server's anything; a source's `CHANGE`
/// (volume) and the rest nothing.
fn told(t: u32) -> Option<Told> {
  match (t & 0xf, t & 0x30) {
    (0x1, 0x00 | 0x20) => Some(Told::Source),
    (0x7, _) => Some(Told::Server),
    _ => None,
  }
}

/// The default source now `name`: whether it changed from one known before (the first only kept).
fn default_changed(known: &mut Option<String>, name: String) -> bool {
  let changed = known.as_ref().is_some_and(|k| *k != name);
  *known = Some(name);
  changed
}

fn pulse() -> Option<&'static Pulse> {
  static PULSE: OnceLock<Option<Pulse>> = OnceLock::new();
  PULSE.get_or_init(|| Pulse::load(&Lib::open(&[c"libpulse.so.0"])?)).as_ref()
}

/// The main loop's thread and API while watched, and its context (a new one after the server
/// dies, made on that thread).
static LOOP: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());
static API: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());
static CONTEXT: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());

/// The context reached `READY`: its failure (the server died) reconnects at once; one that
/// never was (a server named by `PULSE_SERVER` or client.conf that isn't there, which NOFAIL
/// doesn't wait for) tries again after [RETRY_S], not in a busy loop.
static READIED: AtomicBool = AtomicBool::new(false);
/// The subscription was refused: a later watch starts it all again.
static REFUSED: AtomicBool = AtomicBool::new(false);
const RETRY_S: u64 = 1;

/// `pa_mainloop_api` up to `time_free`, for a timer.
#[repr(C)]
struct Api {
  _userdata: P,
  _io: [Fp; 4],
  time_new: unsafe extern "C" fn(P, *const Timeval, Fp, P) -> P,
  _time_restart: Fp,
  time_free: unsafe extern "C" fn(P),
}

/// `struct timeval`, wall-clock time as `pa_mainloop` takes it.
#[repr(C)]
struct Timeval {
  sec: c_long,
  usec: c_long,
}

thread_local! {
  /// On pulse's thread: the default source as last read.
  static DEFAULT: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Pulse's main loop on its own thread, with a context connecting: `Err` without
/// `libpulse.so.0`, or when a named server refuses at once (the default one is waited for).
/// Watched already, it starts again only after a refused subscription.
fn watch_audio() -> Result<(), String> {
  if !LOOP.load(Ordering::Acquire).is_null() {
    if !REFUSED.swap(false, Ordering::AcqRel) {
      return Ok(());
    }

    unwatch_audio();
  }

  let p = pulse().ok_or("libpulse.so.0 not found")?;
  let m = unsafe { (p.pa_threaded_mainloop_new)() };
  if m.is_null() {
    return Err("pa_threaded_mainloop_new failed".into());
  }

  API.store(unsafe { (p.pa_threaded_mainloop_get_api)(m) }, Ordering::Release);
  let started = connect(p).and_then(|()| match unsafe { (p.pa_threaded_mainloop_start)(m) } {
    0.. => Ok(()),
    _ => Err("pa_threaded_mainloop_start failed".into()),
  });
  if let Err(e) = started {
    unwatch_context(p);
    unsafe { (p.pa_threaded_mainloop_free)(m) };
    return Err(e);
  }

  LOOP.store(m, Ordering::Release);
  Ok(())
}

/// A new context, connecting (before the loop runs, or on its thread).
fn connect(p: &Pulse) -> Result<(), String> {
  let api = API.load(Ordering::Acquire);
  let c = unsafe { (p.pa_context_new)(api, c"nimbo".as_ptr()) };
  if c.is_null() {
    return Err("pa_context_new failed".into());
  }

  unsafe { (p.pa_context_set_state_callback)(c, on_state as Fp, null_mut()) };
  CONTEXT.store(c, Ordering::Release);
  match unsafe { (p.pa_context_connect)(c, null(), FLAGS, null_mut()) } {
    0.. => Ok(()),
    e => Err(format!("pa_context_connect failed ({e})")),
  }
}

/// The context let go (the loop stopped, or on its thread).
fn unwatch_context(p: &Pulse) {
  let c = CONTEXT.swap(null_mut(), Ordering::AcqRel);
  if !c.is_null() {
    unsafe { (p.pa_context_set_state_callback)(c, null(), null_mut()) };
    unsafe { (p.pa_context_disconnect)(c) };
    unsafe { (p.pa_context_unref)(c) };
  }
}

/// Pulse's loop stopped first (its thread joined, so no timer or reconnect of it runs after),
/// then its context let go and the loop freed; nothing loaded when it never ran.
fn unwatch_audio() {
  let m = LOOP.swap(null_mut(), Ordering::AcqRel);
  if m.is_null() {
    return;
  }

  let Some(p) = pulse() else { return };
  unsafe { (p.pa_threaded_mainloop_stop)(m) };
  unwatch_context(p);
  unsafe { (p.pa_threaded_mainloop_free)(m) };
  READIED.store(false, Ordering::Release);
  REFUSED.store(false, Ordering::Release);
}

/// The context's state, on pulse's thread: ready, it subscribes, reads the default source and
/// tells both kinds, as the server's devices may differ from what the app read before (no
/// server then, or one restarted); failed, a new context is made outside this callback, at
/// once after a ready one ([READIED]), else after [RETRY_S].
unsafe extern "C" fn on_state(c: P, _: P) {
  let Some(p) = pulse() else { return };
  match unsafe { (p.pa_context_get_state)(c) } {
    READY => {
      unsafe { (p.pa_context_set_subscribe_callback)(c, on_event as Fp, null_mut()) };
      unref(p, unsafe { (p.pa_context_subscribe)(c, MASK, subscribed as Fp, null_mut()) });
      server_info(p, c);
      READIED.store(true, Ordering::Release);
      notice(Change::AudioInputs);
      notice(Change::DefaultInput);
    }
    FAILED | TERMINATED if CONTEXT.load(Ordering::Acquire) == c => {
      let api = API.load(Ordering::Acquire);
      if READIED.swap(false, Ordering::AcqRel) {
        unsafe { (p.pa_mainloop_api_once)(api, reconnect as Fp, null_mut()) };
        return;
      }

      let at = SystemTime::now() + Duration::from_secs(RETRY_S);
      let at = at.duration_since(UNIX_EPOCH).unwrap_or_default();
      let tv = Timeval { sec: at.as_secs() as c_long, usec: at.subsec_micros() as c_long };
      // SAFETY: the threaded main loop's API, whose table starts as `Api` declares.
      let a = unsafe { &*api.cast::<Api>() };
      unsafe { (a.time_new)(api, &tv, later as Fp, null_mut()) };
    }
    _ => {}
  }
}

/// The subscription's answer: refused (it never is, short of a broken server), neither audio
/// kind is watched any more, so a later watch tries again.
unsafe extern "C" fn subscribed(_: P, success: c_int, _: P) {
  if success == 0 {
    REFUSED.store(true, Ordering::Release);
    for kind in [Change::AudioInputs, Change::DefaultInput] {
      refused(kind, "PulseAudio refused the subscription");
    }
  }
}

/// Replaces the failed context with a new one, which waits for the default server to come back
/// (a named one that refuses fails it again, and [on_state] tries later).
unsafe extern "C" fn reconnect(_: P, _: P) {
  let Some(p) = pulse() else { return };
  unwatch_context(p);
  _ = connect(p);
}

/// [RETRY_S] passed after a context that never was ready failed: its timer let go, a new one.
unsafe extern "C" fn later(api: P, e: P, _: *const Timeval, _: P) {
  // SAFETY: the timer made in `on_state` on this API, freed once here.
  unsafe { ((*api.cast::<Api>()).time_free)(e) };
  unsafe { reconnect(api, null_mut()) };
}

/// Lets go of an operation's handle; null when the call failed (the context gone).
fn unref(p: &Pulse, op: P) {
  if !op.is_null() {
    unsafe { (p.pa_operation_unref)(op) };
  }
}

/// A subscription event, on pulse's thread.
unsafe extern "C" fn on_event(c: P, t: c_uint, _: u32, _: P) {
  let Some(p) = pulse() else { return };
  match told(t) {
    Some(Told::Source) => notice(Change::AudioInputs),
    Some(Told::Server) => server_info(p, c),
    None => {}
  }
}

fn server_info(p: &Pulse, c: P) {
  unref(p, unsafe { (p.pa_context_get_server_info)(c, on_info as Fp, null_mut()) });
}

/// The server's info: its default source compared with the last one read.
unsafe extern "C" fn on_info(_: P, info: *const ServerInfo, _: P) {
  // SAFETY: the server's info for the call, or null when it failed.
  let Some(info) = (unsafe { info.as_ref() }) else { return };
  let name = match info.source.is_null() {
    true => String::new(),
    false => unsafe { CStr::from_ptr(info.source) }.to_string_lossy().into_owned(),
  };

  if DEFAULT.with_borrow_mut(|d| default_changed(d, name)) {
    notice(Change::DefaultInput);
  }
}

#[cfg(test)]
mod tests {
  use super::super::super::dbus::V;
  use super::*;

  #[test]
  fn pulse_events() {
    let cases = [
      (0x01, Some(Told::Source)),
      (0x21, Some(Told::Source)),
      (0x11, None),
      (0x00, None),
      (0x20, None),
      (0x17, Some(Told::Server)),
    ];
    for (t, want) in cases {
      assert_eq!(told(t), want, "{t:#x}");
    }

    let mut known = None;
    assert!(!default_changed(&mut known, "mic".into()), "the first only kept");
    assert!(!default_changed(&mut known, "mic".into()));
    assert!(default_changed(&mut known, "headset".into()));
    // `pa_server_info`'s default source after four strings, the sample spec and the sink.
    assert_eq!(std::mem::offset_of!(ServerInfo, source), 56);
  }

  /// UPower's `PropertiesChanged`: the lid's when it names `LidIsClosed` or `LidIsPresent`.
  #[test]
  fn upower_changes() {
    // GIO comes with GTK, absent from a build host without it.
    if super::super::super::lib().is_none() {
      return;
    }

    let body = |iface: &str, key: &str| {
      let changed = V::dict(vec![(key, V::B(true))]);
      Gv::borrowed(V::Tuple(vec![V::s(iface), changed, V::Array("s", vec![])]).build())
    };
    assert!(lid_told(&body(UPOWER, "LidIsClosed")));
    assert!(!lid_told(&body(UPOWER, "OnBattery")));
    assert!(!lid_told(&body("org.freedesktop.UPower.Device", "LidIsClosed")));
  }
}
