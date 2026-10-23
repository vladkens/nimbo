// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The hotkey on Linux, through the XDG desktop portal's `org.freedesktop.portal.GlobalShortcuts`
//! (<https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html>,
//! as of 2026-10): a session ([Hotkeys::start]: `CreateSession`) binds the hotkey
//! (`BindShortcuts`, with the chord as its `preferred_trigger` in the XDG shortcuts spec's form,
//! `CTRL+ALT+space`), the desktop asks the user, who may pick another; its `Activated` and
//! `Deactivated` signals are the press and the release, handed to the "hotkey" thread.
//!
//! A session binds once, so another hotkey closes it and makes a new one. On a connection of
//! its own, the app first registers its id with `org.freedesktop.host.portal.Registry`
//! (xdg-desktop-portal 1.19+), which the backend keeps the shortcut under. KDE Plasma 6 and GNOME
//! 48+ have the portal; without it (X11 desktops, older GNOME) there are no hotkeys, and a
//! modifier key alone is no shortcut the spec can write. Nor can it take Escape for a while
//! ([crate::hotkey::Hotkeys::escape]): that is reported, not done.

use std::cell::{Cell, RefCell};
use std::ffi::c_uint;
use std::ptr::null_mut;
use std::rc::Rc;
use std::sync::mpsc::{Sender, channel};
use std::time::Instant;

use keyboard_types::NamedKey;

use super::super::dbus::{self, Gv, V};
use super::super::{Fp, P, S};
use crate::Modifiers;
use crate::hotkey::{Edge, Listener, OnEdge, Status};
use crate::keys::{Hotkey, Key, Shortcut};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const SHORTCUTS: &str = "org.freedesktop.portal.GlobalShortcuts";
const REQUEST: &str = "org.freedesktop.portal.Request";
/// The one shortcut the session binds.
const ID: &str = "hotkey";
const MS: i32 = 2000;

struct State {
  tx: Sender<Edge>,
  /// The portal's connection, once [Hotkeys::start] made it.
  bus: Cell<P>,
  hotkey: Cell<Option<Shortcut>>,
  started: Cell<bool>,
  listener: Cell<Listener>,
  error: RefCell<Option<String>>,
  session: RefCell<Option<String>>,
  /// The `Response` subscription of the request under way, and the signals' subscriptions.
  request: Cell<c_uint>,
  signals: RefCell<Vec<c_uint>>,
  /// Requests made, for their tokens.
  tokens: Cell<u32>,
  events: Cell<u64>,
  /// [Hotkeys::escape] is on: only reported, the portal can't take Escape.
  escape: Cell<bool>,
}

pub struct Hotkeys(Rc<State>);

impl Hotkeys {
  /// Starts the "hotkey" thread that hands the edges to `on`.
  pub fn new(on: OnEdge) -> Self {
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new().name("hotkey".into()).spawn(move || {
      for edge in rx {
        on(edge);
      }
    });
    if let Err(e) = spawned {
      eprintln!("nimbo: no hotkey thread: {e}");
    }

    Self(Rc::new(State {
      tx,
      bus: Cell::new(null_mut()),
      hotkey: Cell::new(None),
      started: Cell::new(false),
      listener: Cell::new(Listener::Off),
      error: RefCell::new(None),
      session: RefCell::new(None),
      request: Cell::new(0),
      signals: RefCell::default(),
      tokens: Cell::new(0),
      events: Cell::new(0),
      escape: Cell::new(false),
    }))
  }

  pub fn set(&mut self, hotkey: Option<Hotkey>) -> Result<(), String> {
    let s = &self.0;
    let shortcut = match hotkey {
      None => None,
      Some(Hotkey::Modifier(_)) => {
        self.rebind(None);
        return Err("a modifier key alone is not supported by the GlobalShortcuts portal".into());
      }
      Some(Hotkey::Shortcut(k)) => match trigger(&k) {
        Some(_) => Some(k),
        None => {
          self.rebind(None);
          return Err(format!("{k} has no key name the portal takes"));
        }
      },
    };

    if s.hotkey.get() != shortcut {
      self.rebind(shortcut);
    }

    Ok(())
  }

  /// Takes `hotkey`: the session that bound another closes, and once started a new one binds it.
  fn rebind(&self, hotkey: Option<Shortcut>) {
    let s = &self.0;
    s.hotkey.set(hotkey);
    close_session(s);
    if s.started.get() && !s.bus.get().is_null() && hotkey.is_some() {
      create_session(s);
    }
  }

  pub fn start(&mut self) -> Result<(), String> {
    let s = &self.0;
    if matches!(s.listener.get(), Listener::Active | Listener::Starting) {
      return Ok(());
    }

    s.started.set(true);
    if s.bus.get().is_null() {
      match connect(s) {
        Ok(bus) => s.bus.set(bus),
        Err(e) => {
          fail(s, e);
          return Ok(());
        }
      }
    }

    if s.hotkey.get().is_some() {
      create_session(s);
    }

    Ok(())
  }

  /// The portal binds shortcuts the user approved, never a key taken for a while: Escape stays
  /// the focused app's, and the status says so.
  pub fn escape(&mut self, on: bool) {
    self.0.escape.set(on);
  }

  pub fn status(&self, out: &mut Status) {
    let s = &self.0;
    out.listener = s.listener.get();
    out.listener_error = s.error.borrow().clone();
    out.counts.listener = s.events.get();
    if s.escape.get() {
      out.escape_error = Some("the GlobalShortcuts portal can't take Escape".into());
    }
  }
}

impl Drop for Hotkeys {
  fn drop(&mut self) {
    let s = &self.0;
    close_session(s);
    for id in s.signals.take() {
      dbus::unsubscribe(s.bus.get(), id);
    }

    dbus::close(s.bus.replace(std::ptr::null_mut()));
  }
}

/// A connection of the app's own to the portal, its id registered and the signals followed;
/// `Err` without a portal that has global shortcuts.
fn connect(s: &Rc<State>) -> Result<P, String> {
  let bus = dbus::private_session()?;
  let app = super::shell::info();
  let args = V::Tuple(vec![V::s(app.id), V::dict(Vec::new())]);
  let registry = "org.freedesktop.host.portal.Registry";
  // Older portals have no registry: they take the id from the process's systemd scope.
  let _ = dbus::call(bus, PORTAL, PATH, registry, "Register", args, MS);
  let args = V::Tuple(vec![V::s(SHORTCUTS), V::s("version")]);
  if let Err(e) = dbus::call(bus, PORTAL, PATH, "org.freedesktop.DBus.Properties", "Get", args, MS)
  {
    dbus::close(bus);
    return Err(format!(
      "no GlobalShortcuts portal (KDE Plasma 6 and GNOME 48+ have one; not X11 desktops): {e}"
    ));
  }

  let mut signals = s.signals.borrow_mut();
  for (signal, cb) in [("Activated", on_activated as Fp), ("Deactivated", on_deactivated as Fp)] {
    signals.push(dbus::subscribe(bus, PORTAL, SHORTCUTS, signal, Some(PATH), cb, dbus::weak(s)));
  }

  Ok(bus)
}

fn fail(s: &State, why: String) {
  *s.error.borrow_mut() = Some(why);
  s.listener.set(Listener::Failed);
}

/// A token for a request, and the request's object path it gives (the portal's spec: the
/// sender's unique name without the colon, dots as underscores).
fn token(s: &State) -> (String, String) {
  let n = s.tokens.get() + 1;
  s.tokens.set(n);
  let token = format!("nimbo{}_{n}", std::process::id());
  let sender = dbus::unique_name(s.bus.get()).trim_start_matches(':').replace('.', "_");
  let path = format!("{PATH}/request/{sender}/{token}");
  (token, path)
}

/// Calls `method` with `args` and its options last: a fresh `handle_token` (and for a session
/// its `session_handle_token`); `done` gets the request's `Response` (its code, 0 for success,
/// and results).
fn request(s: &Rc<State>, method: &str, mut args: Vec<V>, done: Fp) {
  let (token, path) = token(s);
  let mut opts = vec![("handle_token", V::S(token.clone()))];
  if method == "CreateSession" {
    opts.push(("session_handle_token", V::S(token)));
  }

  args.push(V::dict(opts));
  // Subscribed first: the response may come before the call returns.
  let sub =
    dbus::subscribe(s.bus.get(), PORTAL, REQUEST, "Response", Some(&path), done, dbus::weak(s));
  s.request.set(sub);
  s.listener.set(Listener::Starting);
  if let Err(e) = dbus::call(s.bus.get(), PORTAL, PATH, SHORTCUTS, method, V::Tuple(args), MS) {
    end_request(s);
    fail(s, format!("{method}: {e}"));
  }
}

fn end_request(s: &State) {
  let sub = s.request.replace(0);
  if sub != 0 {
    dbus::unsubscribe(s.bus.get(), sub);
  }
}

fn create_session(s: &Rc<State>) {
  request(s, "CreateSession", Vec::new(), on_session as Fp);
}

fn close_session(s: &State) {
  end_request(s);
  if let Some(session) = s.session.take() {
    let iface = "org.freedesktop.portal.Session";
    let _ = dbus::call(s.bus.get(), PORTAL, &session, iface, "Close", V::Tuple(Vec::new()), MS);
  }

  if s.listener.get() != Listener::Failed {
    s.listener.set(Listener::Off);
  }
}

/// Binds the hotkey in the session made.
fn bind(s: &Rc<State>) {
  let (Some(session), Some(k)) = (s.session.borrow().clone(), s.hotkey.get()) else { return };
  let app = super::shell::info();
  let mut opts = vec![("description", V::s(app.name))];
  if let Some(t) = trigger(&k) {
    opts.push(("preferred_trigger", V::S(t)));
  }

  let shortcuts = V::Array("(sa{sv})", vec![V::Tuple(vec![V::s(ID), V::dict(opts)])]);
  request(s, "BindShortcuts", vec![V::O(session), shortcuts, V::s("")], on_bound as Fp);
}

/// The chord in the XDG shortcuts spec's form: `CTRL`, `ALT`, `SHIFT`, `LOGO`, then the key's
/// keysym name, joined by `+`.
pub fn trigger(k: &Shortcut) -> Option<String> {
  let mods = [
    (Modifiers::CONTROL, "CTRL"),
    (Modifiers::ALT, "ALT"),
    (Modifiers::SHIFT, "SHIFT"),
    (Modifiers::META, "LOGO"),
  ];
  let held = mods.iter().filter(|m| k.mods.contains(m.0)).map(|m| format!("{}+", m.1));
  Some(held.collect::<String>() + &keysym(k.key)?)
}

/// The xkb keysym name of `key` (`xkbcommon-keysyms.h` without `XKB_KEY_`): a letter or digit
/// itself, `space`, `minus`, `Return`, `Page_Up`, `F12`.
pub fn keysym(key: Key) -> Option<String> {
  let name = match key {
    Key::Char(c) if c.is_ascii_alphanumeric() => return Some(c.to_ascii_lowercase().to_string()),
    Key::Char(c) => match c {
      ' ' => "space",
      '-' => "minus",
      '=' => "equal",
      '[' => "bracketleft",
      ']' => "bracketright",
      '\\' => "backslash",
      ';' => "semicolon",
      '\'' => "apostrophe",
      ',' => "comma",
      '.' => "period",
      '/' => "slash",
      '`' => "grave",
      _ => return None,
    },
    Key::Named(k) => match k {
      NamedKey::Enter => "Return",
      NamedKey::Tab => "Tab",
      NamedKey::Backspace => "BackSpace",
      NamedKey::Delete => "Delete",
      NamedKey::Escape => "Escape",
      NamedKey::ArrowUp => "Up",
      NamedKey::ArrowDown => "Down",
      NamedKey::ArrowLeft => "Left",
      NamedKey::ArrowRight => "Right",
      NamedKey::Home => "Home",
      NamedKey::End => "End",
      NamedKey::PageUp => "Page_Up",
      NamedKey::PageDown => "Page_Down",
      NamedKey::Insert => "Insert",
      k => {
        let name = k.to_string();
        let f = name.strip_prefix('F').is_some_and(|n| n.parse::<u8>().is_ok());
        return f.then_some(name);
      }
    },
  };

  Some(name.into())
}

// MARK: Callbacks

/// A request's `Response(u code, a{sv} results)`: the state and both, the request over.
unsafe fn response(args: P, data: P) -> Option<(Rc<State>, u32, Gv)> {
  let s = unsafe { dbus::upgrade::<State>(data) }?;
  end_request(&s);
  let args = Gv::borrowed(args);
  Some((s, args.child(0).u32(), args.child(1)))
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn on_session(_: P, _: S, _: S, _: S, _: S, args: P, data: P) {
  let Some((s, code, results)) = (unsafe { response(args, data) }) else { return };
  let session = results.get("session_handle").map(|v| v.str()).filter(|h| !h.is_empty());
  match (code, session) {
    (0, Some(h)) => {
      *s.session.borrow_mut() = Some(h);
      bind(&s);
    }
    (code, _) => fail(&s, format!("the portal made no session (response {code})")),
  }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn on_bound(_: P, _: S, _: S, _: S, _: S, args: P, data: P) {
  let Some((s, code, results)) = (unsafe { response(args, data) }) else { return };
  let bound = results.get("shortcuts").is_none_or(|v| v.len() > 0);
  match (code, bound) {
    (0, true) => {
      s.error.take();
      s.listener.set(Listener::Active);
    }
    (0, false) => fail(&s, "the shortcut was not bound".into()),
    (code, _) => fail(&s, format!("the user did not bind the shortcut (response {code})")),
  }
}

/// `Activated` or `Deactivated(o session, s id, t timestamp, a{sv})` of our session's hotkey.
unsafe fn edge(args: P, data: P, down: bool) {
  let Some(s) = (unsafe { dbus::upgrade::<State>(data) }) else { return };
  let args = Gv::borrowed(args);
  let ours =
    s.session.borrow().as_deref() == Some(&args.child(0).str()) && args.child(1).str() == ID;
  if !ours {
    return;
  }

  s.events.set(s.events.get() + 1);
  let at = Instant::now();
  let _ = s.tx.send(if down { Edge::Down { at, repeat: false } } else { Edge::Up { at } });
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn on_activated(_: P, _: S, _: S, _: S, _: S, args: P, data: P) {
  unsafe { edge(args, data, true) };
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn on_deactivated(_: P, _: S, _: S, _: S, _: S, args: P, data: P) {
  unsafe { edge(args, data, false) };
}

#[cfg(test)]
mod tests {
  use super::*;

  fn chord(text: &str) -> Shortcut {
    let Ok(Hotkey::Shortcut(k)) = Hotkey::parse(text) else { panic!("{text}") };
    k
  }

  /// No session, no bus: the status alone says Escape is not taken.
  #[test]
  fn escape_is_reported_as_not_taken() {
    let mut h = crate::hotkey::Hotkeys::new(|_| {});
    h.escape(true);
    let s = h.status();
    assert!(!s.escape);
    assert!(s.escape_error.is_some_and(|e| e.contains("can't take Escape")));
    h.escape(false);
    assert_eq!((h.status().escape, h.status().escape_error), (false, None));
  }

  #[test]
  fn triggers_in_the_shortcuts_spec_form() {
    assert_eq!(trigger(&chord("ctrl+option+space")).as_deref(), Some("CTRL+ALT+space"));
    assert_eq!(trigger(&chord("cmd+shift+F12")).as_deref(), Some("SHIFT+LOGO+F12"));
    assert_eq!(trigger(&chord("ctrl+pagedown")).as_deref(), Some("CTRL+Page_Down"));
    assert_eq!(trigger(&chord("alt+'")).as_deref(), Some("ALT+apostrophe"));
    assert_eq!(trigger(&chord("ctrl+K")).as_deref(), Some("CTRL+k"));
    assert_eq!(keysym(Key::Named(NamedKey::Backspace)).as_deref(), Some("BackSpace"));
    assert_eq!(keysym(Key::Char('!')), None);
  }
}
