// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The hotkey on X11: a passive grab of its chord on the root window (`XGrabKey`) on an X
//! connection of its own (`libX11` loaded at run time), ported from lalo's `src/linux/hotkey.rs`
//! (same author, MIT; on x11rb there). X matches a grab's modifiers exactly, so the chord is
//! grabbed four times, with and without Caps Lock (`LockMask`) and Num Lock (`Mod2Mask`); another
//! client's grab of it is a BadAccess, caught ([super::trap]) and returned by [Grab::set], the
//! grabs made undone. XKB's detectable autorepeat has a held key repeat as presses with no
//! releases between: the first press is the edge down, the next ones its repeats, the release the
//! edge up. While [Grab::escape] is on, Escape (alone or with Shift) is grabbed the same way, one
//! [Edge::Escape] per press. GLib watches the connection's socket: the main loop reads the events
//! as they come and never waits on X.

use std::cell::Cell;
use std::ffi::{c_int, c_long, c_uint, c_ulong};
use std::ptr::null;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::time::Instant;

use super::super::{Fp, P, dbus, gtk};
use super::{X11, post, trap, x11};
use crate::Modifiers;
use crate::hotkey::Edge;
use crate::keys::{Key, Shortcut};

/// The lock modifiers each grab is made with and without: `LockMask` and `Mod2Mask`.
const LOCKS: [c_uint; 4] = [0, 2, 16, 2 | 16];
/// `ShiftMask`, `ControlMask`, `Mod1Mask` (Alt) and `Mod4Mask` (Super): what a chord holds.
const MODS: [(Modifiers, c_uint); 4] =
  [(Modifiers::SHIFT, 1), (Modifiers::CONTROL, 4), (Modifiers::ALT, 8), (Modifiers::META, 64)];
const HELD: c_uint = 1 | 4 | 8 | 64;
/// `KeyPress` and `KeyRelease`.
const PRESS: c_uint = 2;
const RELEASE: c_uint = 3;

/// The keys grabbed and those held, which key events match into edges.
#[derive(Default)]
struct Keys {
  /// The chord's keycode and modifier mask.
  chord: Cell<Option<(u8, c_uint)>>,
  /// Escape's keycode, while it is grabbed.
  escape: Cell<Option<u8>>,
  /// The chord's key while it is down: its release counts though the chord changed meanwhile
  /// (paused while a recorder records).
  down: Cell<Option<u8>>,
  escape_down: Cell<bool>,
}

impl Keys {
  /// The edge of key event `kind` of `code` with modifier `state`: the chord's press (a repeat
  /// while it is held) and its release, or Escape's press, once while it is held.
  fn edge(&self, kind: c_uint, code: u8, state: c_uint, at: Instant) -> Option<Edge> {
    let chord = self.chord.get().is_some_and(|(c, m)| c == code && state & HELD == m);
    let escape = self.escape.get() == Some(code);
    match kind {
      PRESS if chord => {
        Some(Edge::Down { at, repeat: self.down.replace(Some(code)) == Some(code) })
      }
      PRESS if escape => (!self.escape_down.replace(true)).then_some(Edge::Escape),
      RELEASE if self.down.get() == Some(code) => {
        self.down.set(None);
        Some(Edge::Up { at })
      }
      RELEASE if escape => {
        self.escape_down.set(false);
        None
      }
      _ => None,
    }
  }
}

struct Inner {
  x: &'static X11,
  d: P,
  root: c_ulong,
  /// GLib's watch of the connection.
  source: Cell<c_uint>,
  tx: Sender<Edge>,
  keys: Keys,
  events: Cell<u64>,
}

pub struct Grab(Rc<Inner>);

impl Grab {
  /// A connection of its own, read in GLib's main loop, with no key grabbed; `Err` without X.
  pub fn open(tx: Sender<Edge>) -> Result<Self, String> {
    let x = x11().ok_or("no libX11")?;
    let d = unsafe { (x.XOpenDisplay)(null()) };
    if d.is_null() {
      return Err("no X display".into());
    }

    // Without XKB a held key repeats as releases and presses: each comes as a new press.
    let mut supported = 0;
    unsafe { (x.XkbSetDetectableAutoRepeat)(d, 1, &mut supported) };
    let root = unsafe { (x.XDefaultRootWindow)(d) };
    let (source, keys, events) = (Cell::new(0), Keys::default(), Cell::new(0));
    let s = Rc::new(Inner { x, d, root, source, tx, keys, events });
    let ((data, free), fd) = (dbus::weak(&s), unsafe { (x.XConnectionNumber)(d) });
    // `G_IO_IN`: the socket has data.
    s.source.set(unsafe { (gtk().g_unix_fd_add_full)(0, fd, 1, on_ready as Fp, data, free) });
    Ok(Self(s))
  }

  /// The edges seen.
  pub fn events(&self) -> u64 {
    self.0.events.get()
  }

  /// Grabs `chord` in place of the one before; `Err` (none grabbed) when the keyboard has no key
  /// for it or another app grabbed it.
  pub fn set(&self, chord: Option<Shortcut>) -> Result<(), String> {
    let s = &self.0;
    if let Some((code, mods)) = s.keys.chord.take() {
      s.grab(code, &[mods], false)?;
    }

    let Some(k) = chord else { return Ok(()) };
    let key = match k.key {
      Key::Char(c) => Key::Char(c.to_lowercase().next().unwrap_or(c)),
      key => key,
    };

    let code = post::keysym(key).map_or(0, |sym| unsafe { (s.x.XKeysymToKeycode)(s.d, sym) });
    if code == 0 {
      return Err(format!("no key on this keyboard types {k}"));
    }

    let mods = mask(k.mods);
    s.grab(code, &[mods], true)?;
    s.keys.chord.set(Some((code, mods)));
    Ok(())
  }

  /// Grabs Escape alone and with Shift (`ShiftMask`), or lets it go; `Err` when another app
  /// grabbed it.
  pub fn escape(&self, on: bool) -> Result<(), String> {
    let s = &self.0;
    s.keys.escape_down.set(false);
    if let Some(code) = s.keys.escape.take() {
      s.grab(code, &[0, 1], false)?;
    }

    let code = unsafe { (s.x.XKeysymToKeycode)(s.d, 0xff1b) };
    if on && code != 0 {
      s.grab(code, &[0, 1], true)?;
      s.keys.escape.set(Some(code));
    }

    Ok(())
  }
}

impl Drop for Grab {
  fn drop(&mut self) {
    let _ = (self.set(None), self.escape(false));
    let s = &self.0;
    unsafe {
      (gtk().g_source_remove)(s.source.get());
      (s.x.XCloseDisplay)(s.d);
    }
  }
}

impl Inner {
  /// Grabs `code` with each of `mods`, with and without each lock, or lets them go; grabs
  /// refused are undone.
  fn grab(&self, code: u8, mods: &[c_uint], on: bool) -> Result<(), String> {
    let (x, d, root, kc) = (self.x, self.d, self.root, c_int::from(code));
    // X keeps one grab of a key and mask for the app: what the chord or Escape (Shift's mask 1
    // or none) still holds stays.
    let (chord, escape) = (self.keys.chord.get(), self.keys.escape.get());
    let held = |m: c_uint| chord == Some((code, m)) || escape == Some(code) && m <= 1;
    let mods: Vec<_> = mods.iter().copied().filter(|&m| on || !held(m)).collect();
    let ((), error) = trap(x, d, || {
      for m in mods.iter().flat_map(|m| LOCKS.map(|lock| m | lock)) {
        // `GrabModeAsync` (1): the keyboard and the pointer go on as before.
        match on {
          true => unsafe { (x.XGrabKey)(d, kc, m, root, 0, 1, 1) },
          false => unsafe { (x.XUngrabKey)(d, kc, m, root) },
        };
      }
    });
    // What the sync read meanwhile waits in Xlib's queue: the socket won't say.
    self.drain();
    if error != 0 && on {
      self.grab(code, &mods, false)?;
    }

    match error {
      0 => Ok(()),
      // `BadAccess`.
      10 => Err("another app has grabbed the key".into()),
      e => Err(format!("X error {e} grabbing the key")),
    }
  }

  /// Reads the events queued: the edges go to the "hotkey" thread.
  fn drain(&self) {
    let mut event = [0 as c_long; 24];
    while unsafe { (self.x.XPending)(self.d) } > 0 {
      unsafe { (self.x.XNextEvent)(self.d, &mut event) };
      // An `XKeyEvent` (LP64): its type the first int, `state` and `keycode` at bytes 80 and 84.
      let e = unsafe { &*event.as_ptr().cast::<[c_uint; 48]>() };
      if let Some(edge) = self.keys.edge(e[0], e[21] as u8, e[20], Instant::now()) {
        self.events.set(self.events.get() + 1);
        let _ = self.tx.send(edge);
      }
    }
  }
}

/// The X modifier mask of `mods`.
fn mask(mods: Modifiers) -> c_uint {
  MODS.iter().filter(|m| mods.contains(m.0)).fold(0, |all, m| all | m.1)
}

/// `GUnixFDSourceFunc`: the connection has data.
unsafe extern "C" fn on_ready(_fd: c_int, _cond: c_uint, data: P) -> c_int {
  if let Some(s) = unsafe { dbus::upgrade::<Inner>(data) } {
    s.drain();
  }

  1
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Ctrl+Alt+Space at keycode 65: presses with Caps or Num Lock on match, a repeat while it is
  /// held; its release counts with the modifiers let go first. Escape (9) once per press, its
  /// release no edge, and pressed while the chord is held, the chord's release still comes.
  #[test]
  fn key_events_into_edges() {
    let (keys, at) = (Keys::default(), Instant::now());
    keys.chord.set(Some((65, 4 | 8)));
    keys.escape.set(Some(9));
    let edge = |kind, code, state| keys.edge(kind, code, state, at);
    assert_eq!(edge(PRESS, 65, 4), None, "Ctrl alone");
    assert_eq!(edge(PRESS, 65, 4 | 8 | 2 | 16), Some(Edge::Down { at, repeat: false }));
    assert_eq!(edge(PRESS, 65, 4 | 8), Some(Edge::Down { at, repeat: true }));
    assert_eq!(edge(RELEASE, 65, 0), Some(Edge::Up { at }));
    assert_eq!(edge(RELEASE, 65, 0), None);
    assert_eq!(edge(PRESS, 9, 1), Some(Edge::Escape));
    assert_eq!(edge(PRESS, 9, 1), None, "held");
    assert_eq!(edge(RELEASE, 9, 0), None);
    assert_eq!(edge(PRESS, 65, 4 | 8), Some(Edge::Down { at, repeat: false }));
    assert_eq!(edge(PRESS, 9, 2), Some(Edge::Escape), "with Caps Lock");
    assert_eq!(edge(RELEASE, 65, 4 | 8), Some(Edge::Up { at }));
  }

  /// The grab's masks: each chord's modifiers, with the four lock variants.
  #[test]
  fn masks_with_the_locks() {
    assert_eq!(mask(Modifiers::CONTROL | Modifiers::ALT), 4 | 8);
    assert_eq!(mask(Modifiers::META | Modifiers::SHIFT), 64 | 1);
    assert_eq!(LOCKS.map(|l| 4 | l), [4, 4 | 2, 4 | 16, 4 | 2 | 16]);
  }
}
