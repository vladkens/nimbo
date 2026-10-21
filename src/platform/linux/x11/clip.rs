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

use super::xcb::{
  ATOM_ATOM, ATOM_STRING, Ev, PROPERTY_NOTIFY, PropertyEv, SELECTION_NOTIFY, SelectionClearEv,
  SelectionNotifyEv, SelectionRequestEv, X,
};

/// How long a paste waits for the owner, and for each INCR chunk.
const WAIT: Duration = Duration::from_secs(1);

/// The atoms selections need.
#[derive(Clone, Copy)]
pub struct Atoms {
  pub clipboard: u32,
  pub targets: u32,
  pub utf8: u32,
  pub text: u32,
  pub mime: u32,
  pub incr: u32,
  /// The property pasted text arrives in.
  pub prop: u32,
}

pub struct Clip {
  pub x: X,
  pub win: u32,
  pub atoms: Atoms,
  /// Events read while waiting for the owner, for the loop.
  pub stash: Rc<RefCell<VecDeque<Ev>>>,
  /// The server time of the last key or button press, which taking the selection uses.
  pub time: u32,
  /// Our selection's text, while we own it.
  pub ours: Option<String>,
}

impl Clip {
  /// Another client asks for our selection: the text (or the targets) in its property, then a
  /// `SelectionNotify`, with no property when refused.
  pub fn request(&self, ev: &SelectionRequestEv) {
    let a = &self.atoms;
    // An obsolete client gives no property: the target's name is used.
    let prop = if ev.property == 0 { ev.target } else { ev.property };
    let ok = match (&self.ours, ev.selection == a.clipboard) {
      (Some(text), true) => self.answer(ev.requestor, prop, ev.target, text),
      _ => false,
    };

    let mut out = [0u8; 32];
    out[0] = SELECTION_NOTIFY;
    let fields = [ev.time, ev.requestor, ev.selection, ev.target, if ok { prop } else { 0 }];
    for (i, v) in fields.iter().enumerate() {
      out[4 + i * 4..8 + i * 4].copy_from_slice(&v.to_ne_bytes());
    }

    self.x.send_event(ev.requestor, 0, &out);
    self.x.flush();
  }

  fn answer(&self, win: u32, prop: u32, target: u32, text: &str) -> bool {
    let a = &self.atoms;
    if target == a.targets {
      let list = [a.targets, a.utf8, a.mime, a.text, ATOM_STRING];
      self.x.set_property32(win, prop, ATOM_ATOM, &list);
      return true;
    }

    let (ty, bytes) = match target {
      t if t == a.utf8 || t == a.mime || t == a.text => (a.utf8, text.as_bytes().to_vec()),
      // Latin-1, `?` for the rest.
      ATOM_STRING => (ATOM_STRING, text.chars().map(|c| u8::try_from(c).unwrap_or(b'?')).collect()),
      _ => return false,
    };

    if bytes.len() + 64 > self.x.max_request() {
      return false;
    }

    self.x.set_property(win, prop, ty, 8, &bytes);
    true
  }

  /// `SelectionClear`: another client took the selection.
  pub fn cleared(&mut self, ev: &SelectionClearEv) {
    if ev.selection == self.atoms.clipboard {
      self.ours = None;
    }
  }

  /// Converts the owner's selection to `target` and reads it.
  fn convert(&self, target: u32) -> Option<Vec<u8>> {
    let (x, a, win) = (self.x, self.atoms, self.win);
    x.convert_selection(win, a.clipboard, target, a.prop);
    let mut stash = self.stash.borrow_mut();
    let is_ours = |ev: &Ev| {
      let n = ev.get::<SelectionNotifyEv>();
      ev.kind() == SELECTION_NOTIFY && n.requestor == win && n.selection == a.clipboard
    };
    let ev = x.wait_event(WAIT, &mut stash, is_ours)?;
    if ev.get::<SelectionNotifyEv>().property == 0 {
      return None;
    }

    let p = x.property(win, a.prop, 0, true)?;
    if p.ty != a.incr {
      return Some(p.data);
    }

    // INCR: each chunk comes as a new value of the property, which we delete; an empty one
    // ends it.
    x.flush();
    let mut out = Vec::new();
    loop {
      let chunk = |ev: &Ev| {
        let p = ev.get::<PropertyEv>();
        ev.kind() == PROPERTY_NOTIFY && p.window == win && p.atom == a.prop && p.state == 0
      };
      x.wait_event(WAIT, &mut stash, chunk)?;
      let p = x.property(win, a.prop, 0, true)?;
      if p.data.is_empty() {
        return Some(out);
      }

      out.extend_from_slice(&p.data);
      x.flush();
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

    if c.x.selection_owner(c.atoms.clipboard) == 0 {
      return None;
    }

    match c.convert(c.atoms.utf8) {
      Some(bytes) => String::from_utf8(bytes).ok(),
      // STRING is Latin-1.
      None => Some(c.convert(ATOM_STRING)?.into_iter().map(char::from).collect()),
    }
  }

  fn write(&mut self, s: &str) {
    let mut c = self.0.borrow_mut();
    c.x.set_selection_owner(c.win, c.atoms.clipboard, c.time);
    c.ours = (c.x.selection_owner(c.atoms.clipboard) == c.win).then(|| s.to_owned());
  }
}
