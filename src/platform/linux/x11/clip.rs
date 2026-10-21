// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The CLIPBOARD selection, plain text (ICCCM): copying owns the selection and answers other
//! clients' `SelectionRequest`s with UTF8_STRING (or STRING) in their property; pasting
//! converts the owner's selection into a property of our window and reads it, INCR transfers
//! too, waiting up to a second (events meanwhile are kept for the loop, in order). Our own
//! selection is read without a round trip. Text larger than one request is refused to others
//! (no INCR sending); PRIMARY is not served.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
  AtomEnum, ConnectionExt as _, EventMask, PropMode, Property, SelectionClearEvent,
  SelectionNotifyEvent, SelectionRequestEvent,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{CURRENT_TIME, NONE};

use super::{Atoms, property, selection_owner, wait_event};

/// How long a paste waits for the owner, and for each INCR chunk.
const WAIT: Duration = Duration::from_secs(1);

pub struct Clip {
  pub x: Rc<RustConnection>,
  pub win: u32,
  pub atoms: Atoms,
  /// Events read while waiting for the owner, for the loop.
  pub stash: Rc<RefCell<VecDeque<Event>>>,
  /// The server time of the last key or button press, which taking the selection uses.
  pub time: u32,
  /// Our selection's text, while we own it.
  pub ours: Option<String>,
}

impl Clip {
  /// Another client asks for our selection: the text (or the targets) in its property, then a
  /// `SelectionNotify`, with no property when refused.
  pub fn request(&self, ev: &SelectionRequestEvent) {
    let a = &self.atoms;
    // An obsolete client gives no property: the target's name is used.
    let prop = if ev.property == NONE { ev.target } else { ev.property };
    let ok = match (&self.ours, ev.selection == a.CLIPBOARD) {
      (Some(text), true) => self.answer(ev.requestor, prop, ev.target, text),
      _ => false,
    };

    let out = SelectionNotifyEvent {
      response_type: x11rb::protocol::xproto::SELECTION_NOTIFY_EVENT,
      sequence: 0,
      time: ev.time,
      requestor: ev.requestor,
      selection: ev.selection,
      target: ev.target,
      property: if ok { prop } else { NONE },
    };
    let _ = self.x.send_event(false, ev.requestor, EventMask::NO_EVENT, out);
    let _ = self.x.flush();
  }

  fn answer(&self, win: u32, prop: u32, target: u32, text: &str) -> bool {
    let (a, string) = (&self.atoms, AtomEnum::STRING.into());
    if target == a.TARGETS {
      let list = [a.TARGETS, a.UTF8_STRING, a.MIME, a.TEXT, string];
      let _ = self.x.change_property32(PropMode::REPLACE, win, prop, AtomEnum::ATOM, &list);
      return true;
    }

    let (ty, bytes) = match target {
      t if t == a.UTF8_STRING || t == a.MIME || t == a.TEXT => {
        (a.UTF8_STRING, text.as_bytes().to_vec())
      }
      // Latin-1, `?` for the rest.
      t if t == string => (string, text.chars().map(|c| u8::try_from(c).unwrap_or(b'?')).collect()),
      _ => return false,
    };

    if bytes.len() + 64 > self.x.maximum_request_bytes() {
      return false;
    }

    let _ = self.x.change_property8(PropMode::REPLACE, win, prop, ty, &bytes);
    true
  }

  /// `SelectionClear`: another client took the selection. One read after we took it back (a
  /// copy handled before it) is stale, so the server says who owns it now.
  pub fn cleared(&mut self, ev: &SelectionClearEvent) {
    let a = &self.atoms;
    if ev.selection == a.CLIPBOARD && selection_owner(&self.x, a.CLIPBOARD) != self.win {
      self.ours = None;
    }
  }

  /// Takes our window's paste property, deleting it: its type and bytes.
  fn take(&self) -> Option<(u32, Vec<u8>)> {
    let r = property(&self.x, self.win, self.atoms.NIMBO_CLIPBOARD, AtomEnum::ANY, true)?;
    (r.type_ != NONE).then_some((r.type_, r.value))
  }

  /// Converts the owner's selection to `target` and reads it.
  fn convert(&self, target: u32) -> Option<Vec<u8>> {
    let (x, a, win) = (&self.x, self.atoms, self.win);
    x.convert_selection(win, a.CLIPBOARD, target, a.NIMBO_CLIPBOARD, CURRENT_TIME).ok()?;
    let mut stash = self.stash.borrow_mut();
    // This conversion's answer: a late one to an earlier conversion (UTF8_STRING, given up on
    // for STRING) has another target.
    let is_ours = |ev: &Event| match ev {
      Event::SelectionNotify(n) => {
        n.requestor == win && n.selection == a.CLIPBOARD && n.target == target
      }
      _ => false,
    };
    let Event::SelectionNotify(n) = wait_event(x, WAIT, &mut stash, is_ours)? else { return None };
    if n.property == NONE {
      return None;
    }

    let (ty, data) = self.take()?;
    if ty != a.INCR {
      return Some(data);
    }

    // INCR: each chunk comes as a new value of the property, which we delete; an empty one
    // ends it.
    x.flush().ok()?;
    let mut out = Vec::new();
    loop {
      let chunk = |ev: &Event| match ev {
        Event::PropertyNotify(p) => {
          p.window == win && p.atom == a.NIMBO_CLIPBOARD && p.state == Property::NEW_VALUE
        }
        _ => false,
      };
      wait_event(x, WAIT, &mut stash, chunk)?;
      let (_, data) = self.take()?;
      if data.is_empty() {
        return Some(out);
      }

      out.extend_from_slice(&data);
      x.flush().ok()?;
    }
  }
}

/// The [crate::Clipboard] the runtime holds, sharing [Clip] with the event loop.
pub struct Clipboard(pub Rc<RefCell<Clip>>);

impl crate::Clipboard for Clipboard {
  fn read(&self) -> Option<String> {
    let c = self.0.borrow();
    if let Some(text) = &c.ours {
      return Some(text.clone());
    }

    if selection_owner(&c.x, c.atoms.CLIPBOARD) == NONE {
      return None;
    }

    match c.convert(c.atoms.UTF8_STRING) {
      Some(bytes) => String::from_utf8(bytes).ok(),
      // STRING is Latin-1.
      None => Some(c.convert(AtomEnum::STRING.into())?.into_iter().map(char::from).collect()),
    }
  }

  fn write(&mut self, s: &str) {
    let mut c = self.0.borrow_mut();
    let _ = c.x.set_selection_owner(c.win, c.atoms.CLIPBOARD, c.time);
    c.ours = (selection_owner(&c.x, c.atoms.CLIPBOARD) == c.win).then(|| s.to_owned());
  }
}

#[cfg(test)]
mod tests {
  use x11rb::protocol::xproto::{CreateWindowAux, WindowClass};

  use super::*;
  use crate::Clipboard as _;

  /// A client of its own with an 8 × 8 window, never mapped.
  fn client() -> (Rc<RustConnection>, u32) {
    let (x, n) = x11rb::connect(None).unwrap();
    let (root, visual) = (x.setup().roots[n].root, x.setup().roots[n].root_visual);
    let win = x.generate_id().unwrap();
    let (class, aux) = (WindowClass::INPUT_OUTPUT, CreateWindowAux::new());
    x.create_window(0, win, root, 0, 0, 8, 8, 0, class, visual, &aux).unwrap();
    (Rc::new(x), win)
  }

  /// Another client takes the selection, and we copy again before its `SelectionClear` is
  /// handled: the stale clear leaves the new text ours. A selection of the test's own, not the
  /// desktop's clipboard. Skipped without `DISPLAY`.
  #[test]
  fn a_stale_clear_keeps_the_new_copy() {
    if std::env::var_os("DISPLAY").is_none() {
      eprintln!("no DISPLAY: skipped");
      return;
    }

    let (x, win) = client();
    // Another client: the server sends no clear when the owner stays the same client.
    let (y, other) = client();
    let mut atoms = Atoms::new(&*x).unwrap().reply().unwrap();
    let own = x.intern_atom(false, b"NIMBO_TEST_SELECTION").unwrap().reply().unwrap().atom;
    atoms.CLIPBOARD = own;
    let stash = Rc::new(RefCell::new(VecDeque::new()));
    let clip = Clip { x: x.clone(), win, atoms, stash: stash.clone(), time: 0, ours: None };
    let clip = Rc::new(RefCell::new(clip));
    let mut board = Clipboard(clip.clone());
    let take = || {
      y.set_selection_owner(other, atoms.CLIPBOARD, CURRENT_TIME).unwrap();
      assert_eq!(selection_owner(&y, atoms.CLIPBOARD), other);
    };
    let clear = || {
      let ev =
        wait_event(&x, WAIT, &mut stash.borrow_mut(), |e| matches!(e, Event::SelectionClear(_)));
      let Some(Event::SelectionClear(e)) = ev else { panic!("no SelectionClear") };
      clip.borrow_mut().cleared(&e);
    };

    board.write("old");
    take();
    board.write("new");
    clear();
    assert_eq!(board.read().as_deref(), Some("new"));

    // A clear while another client owns it is not stale.
    take();
    clear();
    assert!(clip.borrow().ours.is_none());
  }
}
