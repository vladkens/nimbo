// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A paste on Wayland through the XDG desktop portals, ported from lalo's `src/linux/remote.rs`
//! and `src/linux/portal.rs` (same author, MIT): Wayland lets no app type into another, nor set
//! the clipboard without the keyboard focus. A `org.freedesktop.portal.RemoteDesktop` session
//! with the keyboard (<https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html>,
//! as of 2026-10) presses a shortcut's keys ([post]: `NotifyKeyboardKeycode`, evdev's codes of
//! the keys in the layout GDK has), the modifiers pressed and released around the key. Its
//! clipboard (`org.freedesktop.portal.Clipboard`, `RequestClipboard` before `Start`) serves what
//! [super::paste] offers or restores (`SetSelection` with the types; each `SelectionTransfer`
//! answered with the bytes written to `SelectionWrite`'s file descriptor, then
//! `SelectionWriteDone`) and reads a snapshot (`SelectionRead` of the types the owner's
//! `SelectionOwnerChanged` gave, else of text).
//!
//! The session starts at the first paste (its snapshot, offer or keys), each request's
//! `Response` waited for in a nested main loop: the desktop asks the user the first time only,
//! as the permission persists (`persist_mode` 2) and the `restore_token` each start gives, kept
//! in the app's config folder ([crate::paths::config_dir]), lets the next one start without
//! asking. It lasts as long as the app (GNOME shows it in the top bar); closed by the desktop,
//! the next paste starts another, and one refused is tried again only after [RETRY].

use std::cell::{Cell, RefCell};
use std::ffi::{c_int, c_uint, c_ulong};
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::PathBuf;
use std::ptr::null_mut;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use keyboard_types::{Code, Modifiers};

use super::super::dbus::{self, Gv, PORTAL, PORTAL_PATH as PATH, V, portal, request};
use super::super::dl::fns;
use super::super::{Fp, P, S, gtk, keys, lib};
use super::{paste, post};
use crate::clipboard::Snapshot;
use crate::keys::{Shortcut, strokes};

const REMOTE: &str = "org.freedesktop.portal.RemoteDesktop";
const CLIPBOARD: &str = "org.freedesktop.portal.Clipboard";
/// The keyboard, of the device types.
const KEYBOARD: u32 = 1;
const MS: c_int = 2000;
/// How long a request may take (the user's answer to the desktop's question), how soon a
/// session that failed is tried again, and how long the owner may take to give a type.
const WAIT: Duration = Duration::from_secs(60);
const RETRY: Duration = Duration::from_secs(2);
const READ: Duration = Duration::from_millis(500);

fns!(Keymap {
  fn gdk_keymap_get_for_display(display: P) -> P;
  fn gdk_keymap_get_entries_for_keyval(map: P, sym: c_uint, keys: *mut *mut [c_int; 3],
    n: *mut c_int) -> c_int;
});

/// A started session with the keyboard, and the clipboard where the desktop gave it.
pub(super) struct State {
  bus: P,
  session: RefCell<String>,
  clipboard: Cell<bool>,
  signals: RefCell<Vec<c_uint>>,
  /// The types of the clipboard's owner, once another than the app took it.
  owner: RefCell<Option<Vec<String>>>,
}

thread_local! {
  static SESSION: RefCell<Option<Rc<State>>> = const { RefCell::new(None) };
  /// When the last session failed, and why: in the future while one starts.
  static FAILED: RefCell<(Option<Instant>, String)> = const { RefCell::new((None, String::new())) };
}

/// The session, started at the first call; `Err` with why there is none.
fn session() -> Result<Rc<State>, String> {
  if let Some(s) = SESSION.with_borrow(Clone::clone) {
    return Ok(s);
  }

  let (at, why) = FAILED.with_borrow(Clone::clone);
  if at.is_some_and(|t| t.elapsed() < RETRY) {
    return Err(why);
  }

  // A paste in the nested main loop meanwhile starts none of its own.
  let asking = "the desktop still asks whether the app may type".to_string();
  FAILED.set((Some(Instant::now() + WAIT), asking));
  let s = start().inspect_err(|e| FAILED.set((Some(Instant::now()), e.clone())))?;
  FAILED.set((None, String::new()));
  SESSION.set(Some(s.clone()));
  Ok(s)
}

/// The session when it has the clipboard (Wayland only).
pub(super) fn clipboard() -> Option<Rc<State>> {
  let on = super::wayland() && super::shell::running();
  on.then(session).and_then(Result::ok).filter(|s| s.clipboard.get())
}

/// A session on a connection of its own, its signals followed from the start.
fn start() -> Result<Rc<State>, String> {
  let (session, clipboard) = (RefCell::default(), Cell::new(false));
  let (signals, owner) = (RefCell::default(), RefCell::default());
  let s =
    Rc::new(State { bus: portal(super::shell::info().id)?, session, clipboard, signals, owner });
  let follow = [
    (CLIPBOARD, "SelectionOwnerChanged", on_owner as Fp),
    (CLIPBOARD, "SelectionTransfer", on_transfer as Fp),
    ("org.freedesktop.portal.Session", "Closed", on_closed as Fp),
  ];
  for (iface, signal, cb) in follow {
    let sub = dbus::subscribe(s.bus, PORTAL, iface, signal, None, cb, dbus::weak(&s));
    s.signals.borrow_mut().push(sub);
  }

  // The portal gone (it crashed) closes no session: no signal would tell the clipboard changed.
  let (bus, gone) = ("org.freedesktop.DBus", on_portal_gone as Fp);
  let sub = dbus::subscribe(s.bus, bus, bus, "NameOwnerChanged", None, gone, dbus::weak(&s));
  s.signals.borrow_mut().push(sub);

  let made = ask(&s, "CreateSession", vec![], vec![])?.get("session_handle").map(|v| v.str());
  *s.session.borrow_mut() = made.ok_or("the portal made no session")?;
  // `persist_mode` 2: until the user revokes it.
  let mut opts = vec![("types", V::U(KEYBOARD)), ("persist_mode", V::U(2))];
  if let Some(token) = std::fs::read_to_string(token_file()).ok().filter(|t| !t.trim().is_empty()) {
    opts.push(("restore_token", V::S(token.trim().into())));
  }

  ask(&s, "SelectDevices", vec![path(&s)], opts)?;
  // Before `Start`, as the portal wants; a desktop without the Clipboard portal has none.
  let args = V::Tuple(vec![path(&s), V::dict(vec![])]);
  let _ = dbus::call(s.bus, PORTAL, PATH, CLIPBOARD, "RequestClipboard", args, MS);
  let results = ask(&s, "Start", vec![path(&s), V::s("")], vec![])?;
  if results.get("devices").is_none_or(|v| v.u32() & KEYBOARD == 0) {
    return Err("the desktop gave no keyboard".into());
  }

  s.clipboard.set(results.get("clipboard_enabled").is_some_and(|v| v.bool()));
  // A token is good for one start: each gives the next.
  let (file, token) = (token_file(), results.get("restore_token").map(|v| v.str()));
  let kept = file.parent().map_or(Ok(()), std::fs::create_dir_all);
  if let Err(e) = kept.and_then(|_| std::fs::write(&file, token.unwrap_or_default())) {
    eprintln!("nimbo: the remote desktop's restore token: {e}");
  }

  Ok(s)
}

impl Drop for State {
  fn drop(&mut self) {
    for sub in self.signals.take() {
      dbus::unsubscribe(self.bus, sub);
    }

    let (session, iface) = (self.session.take(), "org.freedesktop.portal.Session");
    if !session.is_empty() {
      let _ = dbus::call(self.bus, PORTAL, &session, iface, "Close", V::Tuple(vec![]), MS);
    }

    dbus::close(self.bus);
  }
}

/// Calls RemoteDesktop's `method` and waits for its `Response` in a nested main loop: its
/// results, or why there are none.
fn ask(s: &State, method: &str, args: Vec<V>, opts: Vec<(&str, V)>) -> Result<Gv, String> {
  // The type [on_response] takes it as.
  let got = Rc::new(RefCell::new(None::<(u32, Gv)>));
  let sub = request(s.bus, REMOTE, method, args, opts, on_response as Fp, dbus::weak(&got))?;
  let deadline = Instant::now() + WAIT;
  while got.borrow().is_none() && Instant::now() < deadline {
    turn();
  }

  dbus::unsubscribe(s.bus, sub);
  match got.take() {
    Some((0, results)) => Ok(results),
    Some((1, _)) => Err(format!("{method}: the user did not allow the app to type")),
    Some((code, _)) => Err(format!("{method}: the desktop refused it (response {code})")),
    None => Err(format!("{method}: no answer from the desktop")),
  }
}

/// Calls the session's `method`; failed, the session is forgotten (the portal may have restarted
/// without closing it), so the next paste starts another.
fn call(s: &State, iface: &str, method: &str, args: Vec<V>) -> Result<Gv, String> {
  let done = dbus::call(s.bus, PORTAL, PATH, iface, method, V::Tuple(args), MS);
  done.inspect_err(|_| forget()).map_err(|e| format!("{method}: {e}"))
}

/// No session: what it served goes with it.
fn forget() {
  SESSION.set(None);
  paste::lost();
}

/// The session's own path, first of each call's arguments.
fn path(s: &State) -> V {
  V::O(s.session.borrow().clone())
}

/// A session runs, or the desktop's approval is saved for the next one to start without asking.
pub(super) fn granted() -> bool {
  let saved = std::fs::read_to_string(token_file()).is_ok_and(|t| !t.trim().is_empty());
  SESSION.with_borrow(Option::is_some) || saved
}

fn token_file() -> PathBuf {
  crate::paths::config_dir(super::shell::info().id).join("remote-desktop.token")
}

// MARK: Keys

/// Presses shortcut `k` into the app in focus: the modifiers, the key (Shift added at its second
/// level), the modifiers let go. Wayland tells no app which keys are held: none, as the portal's
/// keyboard is a device of its own.
pub(super) fn post(k: Shortcut) -> Result<(), String> {
  let s = session()?;
  let (key, shift) = post::keysym(k.key).and_then(layout).ok_or("the keymap has no such key")?;
  let mods = if shift { k.mods | Modifiers::SHIFT } else { k.mods };
  for (code, down) in strokes(mods, key, evdev, |_| false, None).ok_or("no modifier key")? {
    let args = vec![path(&s), V::dict(vec![]), V::I(code as i32), V::U(down.into())];
    call(&s, REMOTE, "NotifyKeyboardKeycode", args)?;
  }

  Ok(())
}

/// The evdev code of the key at `code`.
pub(super) fn evdev(code: Code) -> Option<u32> {
  (1..256).find(|&k| keys::code(k) == code)
}

/// The evdev code of the key that types keysym `sym` in the layout GDK has (its lowest group,
/// the first or second level), and whether it takes Shift.
fn layout(sym: c_ulong) -> Option<(u32, bool)> {
  static KEYMAP: OnceLock<Option<Keymap>> = OnceLock::new();
  let k = KEYMAP.get_or_init(|| Keymap::load(&lib()?)).as_ref()?;
  let (g, mut entries, mut n) = (gtk(), null_mut(), 0);
  let map = unsafe { (k.gdk_keymap_get_for_display)((g.gdk_display_get_default)()) };
  let get = k.gdk_keymap_get_entries_for_keyval;
  if unsafe { get(map, sym as c_uint, &mut entries, &mut n) } == 0 {
    return None;
  }

  // SAFETY: GDK's `n` GdkKeymapKeys (keycode, group, level), freed after.
  let all = unsafe { std::slice::from_raw_parts(entries, n.max(0) as usize) };
  let best = all.iter().filter(|e| e[2] <= 1 && e[0] > 8).min_by_key(|e| (e[1], e[2]));
  let found = best.map(|e| (e[0] as u32 - 8, e[2] == 1));
  unsafe { (g.g_free)(entries.cast()) };
  found
}

// MARK: Clipboard

/// Takes the clipboard with `types`, their bytes from [paste::read] at each transfer.
pub(super) fn offer(s: &State, types: Vec<String>) -> bool {
  let types = V::Array("s", types.into_iter().map(V::S).collect());
  call(s, CLIPBOARD, "SetSelection", vec![path(s), V::dict(vec![("mime_types", types)])]).is_ok()
}

/// What the clipboard holds: what the app serves, else each of the owner's types read (text
/// when they are not known).
pub(super) fn snapshot(s: &State) -> Snapshot {
  let (mut snapshot, served) = (Snapshot::default(), paste::served());
  let text = || ["text/plain;charset=utf-8", "text/plain"].map(String::from).to_vec();
  let types =
    if served.is_some() { Vec::new() } else { s.owner.borrow().clone().unwrap_or_else(text) };
  let mut item = served.unwrap_or_default();
  for kind in types.into_iter().filter(|t| !paste::META.contains(&t.as_str())) {
    match read(s, &kind) {
      Some(bytes) => item.push((kind, bytes)),
      None => snapshot.missing += 1,
    }
  }

  snapshot.items.extend(Some(item).filter(|i| !i.is_empty()));
  snapshot
}

/// The owner's bytes of `kind`, read as they come while the main loop turns (the owner may be
/// the app, GTK serving it there); `None` unless they ended within [READ]. The descriptor closes
/// either way, which ends the transfer.
fn read(s: &State, kind: &str) -> Option<Vec<u8>> {
  let args = V::Tuple(vec![path(s), V::s(kind)]);
  let fd = dbus::call_fd(s.bus, PORTAL, PATH, CLIPBOARD, "SelectionRead", args, MS).ok()?;
  let (mut file, mut bytes, mut buf) = (file(fd, true), Vec::new(), [0; 16384]);
  let deadline = Instant::now() + READ;
  while Instant::now() < deadline {
    match file.read(&mut buf) {
      Ok(0) => return Some(bytes),
      Ok(n) => bytes.extend_from_slice(&buf[..n]),
      Err(e) if e.kind() == ErrorKind::WouldBlock => turn(),
      Err(_) => return None,
    }
  }

  None
}

/// One turn of the main loop, or a short sleep when nothing was waiting.
fn turn() {
  if unsafe { (gtk().g_main_context_iteration)(null_mut(), 0) } == 0 {
    std::thread::sleep(Duration::from_millis(2));
  }
}

unsafe extern "C" {
  fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
}

/// A transfer's descriptor as a file, `nonblock` or not, whichever way the portal gave it.
fn file(fd: OwnedFd, nonblock: bool) -> File {
  // `F_GETFL`, `F_SETFL` and `O_NONBLOCK`.
  let (get, set, flag) = (3, 4, 0o4000);
  let flags = unsafe { fcntl(fd.as_raw_fd(), get) } & !flag;
  unsafe { fcntl(fd.as_raw_fd(), set, if nonblock { flags | flag } else { flags }) };
  File::from(fd)
}

// MARK: Callbacks

/// A request's `Response(u code, a{sv} results)`, for [ask].
unsafe extern "C" fn on_response(_: P, _: S, _: S, _: S, _: S, args: P, data: P) {
  if let Some(got) = unsafe { dbus::upgrade::<RefCell<Option<(u32, Gv)>>>(data) } {
    let args = Gv::borrowed(args);
    *got.borrow_mut() = Some((args.child(0).u32(), args.child(1)));
  }
}

/// A session's `Closed`: what it served goes, and the next paste starts another.
unsafe extern "C" fn on_closed(_: P, _: S, path: S, _: S, _: S, _: P, data: P) {
  let Some(s) = (unsafe { dbus::upgrade::<State>(data) }) else { return };
  if dbus::text(path) == *s.session.borrow() {
    forget();
  }
}

/// `NameOwnerChanged(s name, s old, s new)`: the portal left the bus, and the session with it.
unsafe extern "C" fn on_portal_gone(_: P, _: S, _: S, _: S, _: S, args: P, data: P) {
  let args = Gv::borrowed(args);
  let ours = unsafe { dbus::upgrade::<State>(data) }.is_some();
  if ours && args.child(0).str() == PORTAL && args.child(2).str().is_empty() {
    forget();
  }
}

/// `SelectionOwnerChanged(o session, a{sv})`: another owner's types; what the app served goes.
unsafe extern "C" fn on_owner(_: P, _: S, _: S, _: S, _: S, args: P, data: P) {
  let Some(s) = (unsafe { dbus::upgrade::<State>(data) }) else { return };
  let (args, ours) = (Gv::borrowed(args), s.session.borrow().clone());
  let opts = args.child(1);
  if args.child(0).str() != ours || opts.get("session_is_owner").is_some_and(|v| v.bool()) {
    return;
  }

  // `as`, which GNOME sends in a tuple.
  let types = opts.get("mime_types").map(|v| if v.ty() == "(as)" { v.child(0) } else { v });
  let types = types.map(|v| (0..v.len()).map(|i| v.child(i).str()).collect());
  *s.owner.borrow_mut() = Some(types.unwrap_or_default());
  paste::lost();
}

/// `SelectionTransfer(o session, s type, u serial)`: an app reads a type of what the app serves,
/// written on a thread of its own (a reader that stops reading never stops the app).
unsafe extern "C" fn on_transfer(_: P, _: S, _: S, _: S, _: S, args: P, data: P) {
  let Some(s) = (unsafe { dbus::upgrade::<State>(data) }) else { return };
  let (args, session) = (Gv::borrowed(args), s.session.borrow().clone());
  let (kind, serial) = (args.child(1).str(), args.child(2).u32());
  if args.child(0).str() != session {
    return;
  }

  let bytes = paste::read(None, |_, t| t == kind);
  let args = V::Tuple(vec![path(&s), V::U(serial)]);
  let write = || dbus::call_fd(s.bus, PORTAL, PATH, CLIPBOARD, "SelectionWrite", args, MS).ok();
  let fd = bytes.as_ref().and_then(|_| write());
  let bus = dbus::retain(s.bus);
  std::thread::spawn(move || {
    let done = fd.zip(bytes).is_some_and(|(fd, b)| file(fd, false).write_all(&b).is_ok());
    let args = V::Tuple(vec![V::O(session), V::U(serial), V::B(done)]);
    let _ = dbus::call(bus as P, PORTAL, PATH, CLIPBOARD, "SelectionWriteDone", args, MS);
    dbus::release(bus);
  });
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The keys by evdev code (<linux/input-event-codes.h>), and a shortcut's presses: Ctrl+V is
  /// the left Ctrl around V (47 on the US layout).
  #[test]
  fn evdev_codes_of_keys() {
    let codes = [Code::ControlLeft, Code::ShiftLeft, Code::AltLeft, Code::MetaLeft, Code::KeyV];
    assert_eq!(codes.map(evdev), [Some(29), Some(42), Some(56), Some(125), Some(47)]);
    let ctrl_v = strokes(Modifiers::CONTROL, 47, evdev, |_| false, None);
    assert_eq!(ctrl_v, Some(vec![(29, true), (47, true), (47, false), (29, false)]));
    let shift = strokes(Modifiers::CONTROL | Modifiers::SHIFT, 47, evdev, |_| false, None);
    assert_eq!(shift.map(|s| s.len()), Some(6));
  }

  /// The calls' arguments as the portal's spec types them.
  #[test]
  fn portal_arguments() {
    let session = V::O("/org/freedesktop/portal/desktop/session/1_42/t".into());
    let key = V::Tuple(vec![session.clone(), V::dict(vec![]), V::I(47), V::U(1)]);
    assert_eq!(key.ty(), "(oa{sv}iu)");
    let types = V::Array("s", vec![V::s("text/plain")]);
    let set = V::Tuple(vec![session.clone(), V::dict(vec![("mime_types", types)])]);
    assert_eq!(set.ty(), "(oa{sv})");
    let opts = V::dict(vec![("types", V::U(KEYBOARD)), ("persist_mode", V::U(2))]);
    assert_eq!(V::Tuple(vec![session.clone(), opts]).ty(), "(oa{sv})");
    assert_eq!(V::Tuple(vec![session, V::U(7), V::B(true)]).ty(), "(oub)");
  }
}
