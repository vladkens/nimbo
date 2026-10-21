// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The clipboard on `wl_data_device`: plain UTF-8 text. Copying makes a data source that
//! answers `send` with our text, written into the reader's pipe as it takes it ([Transfers]:
//! the loop polls the pipes, it never waits on one); pasting reads the selection's offer through
//! a pipe (our own selection without a round trip). The device's offers are kept until the
//! event that uses them, a drag's while it is over the window ([Offers]).

use std::cell::RefCell;
use std::io::Read;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::rc::Rc;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use rustix::io::Errno;
use rustix::pipe::{PipeFlags, pipe_with};
use wayland_client::protocol::wl_data_device::WlDataDevice;
use wayland_client::protocol::wl_data_device_manager::WlDataDeviceManager;
use wayland_client::protocol::wl_data_offer::WlDataOffer;
use wayland_client::protocol::wl_data_source::WlDataSource;
use wayland_client::{Connection, QueueHandle};

use super::Win;

/// Text types, preferred first.
const MIMES: [&str; 3] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];
/// A reader that takes none of our text for this long is let go.
const IDLE: Duration = Duration::from_secs(1);

pub struct Clip {
  pub conn: Connection,
  pub qh: QueueHandle<Win>,
  pub manager: Option<WlDataDeviceManager>,
  pub device: Option<WlDataDevice>,
  pub offers: Offers<WlDataOffer>,
  /// Our selection: its source and text.
  pub source: Option<(WlDataSource, String)>,
  /// The serial of the last key or button press, which `set_selection` needs.
  pub serial: u32,
  /// Our text on its way to readers.
  pub transfers: Transfers,
}

/// A data offer as [Offers] keeps it: one goes away with `destroy`.
pub trait Offer: PartialEq {
  fn destroy(&self);
}

impl Offer for WlDataOffer {
  fn destroy(&self) {
    WlDataOffer::destroy(self);
  }
}

/// The data device's offers, each destroyed once the event it came for is over: the
/// selection's until the next selection, a drag's until it leaves or drops (we take no drops).
pub struct Offers<O> {
  /// Announced (`data_offer`) with the types each has, until a selection or a drag takes one.
  pending: Vec<(O, Vec<String>)>,
  /// The selection's offer, someone else's.
  pub selection: Option<(O, Vec<String>)>,
  /// The offer of a drag over the window.
  drag: Option<O>,
}

impl<O> Default for Offers<O> {
  fn default() -> Self {
    Self { pending: Vec::new(), selection: None, drag: None }
  }
}

impl<O: Offer> Offers<O> {
  /// `wl_data_device.data_offer`: a new offer, its types to come.
  pub fn announce(&mut self, offer: O) {
    self.pending.push((offer, Vec::new()));
  }

  /// `wl_data_offer.offer`: one of its types.
  pub fn add_type(&mut self, offer: &O, mime: String) {
    if let Some((_, types)) = self.pending.iter_mut().find(|(o, _)| o == offer) {
      types.push(mime);
    }
  }

  /// `wl_data_device.selection`: the new selection's offer, or none; the others announced
  /// before it go.
  pub fn select(&mut self, offer: Option<O>) {
    if let Some((old, _)) = self.selection.take() {
      old.destroy();
    }

    let picked = offer.and_then(|o| self.pending.iter().position(|(x, _)| *x == o));
    self.selection = picked.map(|i| self.pending.swap_remove(i));
    for (o, _) in self.pending.drain(..) {
      o.destroy();
    }
  }

  /// `wl_data_device.enter`: a drag came over the window, with its offer (none for a drag
  /// without data).
  pub fn drag_enter(&mut self, offer: Option<O>) {
    self.drag_end();
    if let Some(o) = offer {
      self.pending.retain(|(x, _)| *x != o);
      self.drag = Some(o);
    }
  }

  /// `wl_data_device.leave` or `drop`: the drag's offer is done with.
  pub fn drag_end(&mut self) {
    if let Some(o) = self.drag.take() {
      o.destroy();
    }
  }
}

impl Clip {
  /// `wl_data_source.send`: our text to `fd`, as much as it takes now, the rest as it reads.
  pub fn send(&mut self, source: &WlDataSource, fd: OwnedFd) {
    if let Some((s, text)) = &self.source
      && s == source
    {
      self.transfers.add(fd, text.as_bytes().into(), Instant::now());
    }
  }

  /// `wl_data_source.cancelled`: another client took the selection.
  pub fn cancelled(&mut self, source: &WlDataSource) {
    if self.source.as_ref().is_some_and(|(s, _)| s == source) {
      self.source = None;
    }

    source.destroy();
  }
}

/// Bytes going to readers' pipes, made non-blocking: [Transfers::pump] writes what each takes
/// and lets go of those done, closed, or idle for [IDLE]. A reader that stops can't stall the
/// loop, and they don't pile up.
#[derive(Default)]
pub struct Transfers(Vec<Transfer>);

struct Transfer {
  fd: OwnedFd,
  data: Rc<[u8]>,
  at: usize,
  until: Instant,
}

impl Transfers {
  pub fn add(&mut self, fd: OwnedFd, data: Rc<[u8]>, now: Instant) {
    let nonblocking = fcntl_getfl(&fd).and_then(|f| fcntl_setfl(&fd, f | OFlags::NONBLOCK));
    if nonblocking.is_ok() {
      self.0.push(Transfer { fd, data, at: 0, until: now + IDLE });
      self.pump(now);
    }
  }

  /// Writes what the readers take now; drops the transfers done, failed or idle at `now`.
  pub fn pump(&mut self, now: Instant) {
    self.0.retain_mut(|t| {
      while t.at < t.data.len() {
        match rustix::io::write(&t.fd, &t.data[t.at..]) {
          Ok(n) => (t.at, t.until) = (t.at + n, now + IDLE),
          Err(Errno::INTR) => {}
          Err(Errno::AGAIN) => return now < t.until,
          Err(_) => return false,
        }
      }

      false
    });
  }

  /// The pipes to poll for room.
  pub fn fds(&self) -> impl Iterator<Item = BorrowedFd<'_>> {
    self.0.iter().map(|t| t.fd.as_fd())
  }

  /// When the next idle reader is let go.
  pub fn deadline(&self) -> Option<Instant> {
    self.0.iter().map(|t| t.until).min()
  }
}

/// The [crate::Clipboard] the runtime holds, sharing [Clip] with the event loop.
pub struct Clipboard(pub Rc<RefCell<Clip>>);

impl crate::Clipboard for Clipboard {
  fn read(&self) -> Option<String> {
    let c = self.0.borrow();
    if let Some((_, text)) = &c.source {
      return Some(text.clone());
    }

    let (offer, types) = c.offers.selection.as_ref()?;
    let mime = MIMES.iter().find(|m| types.iter().any(|t| t == *m))?;
    let (rd, wr) = pipe_with(PipeFlags::CLOEXEC).ok()?;
    offer.receive(mime.to_string(), wr.as_fd());
    drop(wr);
    c.conn.flush().ok()?;
    // The owner writes and closes its end; give up after a second of silence.
    let mut out = Vec::new();
    let mut file = std::fs::File::from(rd);
    let mut buf = [0u8; 8192];
    loop {
      let mut fds = [PollFd::new(&file, PollFlags::IN)];
      let ts = Timespec { tv_sec: 1, tv_nsec: 0 };
      if poll(&mut fds, Some(&ts)).ok()? == 0 {
        return None;
      }

      match file.read(&mut buf).ok()? {
        0 => break,
        n => out.extend_from_slice(&buf[..n]),
      }
    }

    String::from_utf8(out).ok()
  }

  fn write(&mut self, s: &str) {
    let mut c = self.0.borrow_mut();
    let (Some(manager), Some(device)) = (c.manager.clone(), c.device.clone()) else { return };
    let source = manager.create_data_source(&c.qh, ());
    for m in MIMES {
      source.offer(m.into());
    }

    device.set_selection(Some(&source), c.serial);
    if let Some((old, _)) = c.source.replace((source, s.to_owned())) {
      old.destroy();
    }

    let _ = c.conn.flush();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn pipe() -> (std::fs::File, OwnedFd) {
    let (rd, wr) = pipe_with(PipeFlags::CLOEXEC).unwrap();
    (std::fs::File::from(rd), wr)
  }

  /// An offer that counts its `destroy` calls.
  #[derive(Debug)]
  struct Fake(u32, Rc<std::cell::Cell<u32>>);

  impl PartialEq for Fake {
    fn eq(&self, o: &Self) -> bool {
      self.0 == o.0
    }
  }

  impl Offer for Fake {
    fn destroy(&self) {
      self.1.set(self.1.get() + 1);
    }
  }

  /// Drags across the window, each with its offer: every one goes when it leaves or drops,
  /// none waits for a selection; the selection's stays.
  #[test]
  fn drag_offers_go_when_the_drag_leaves() {
    let gone = Rc::new(std::cell::Cell::new(0));
    let offer = |id| Fake(id, gone.clone());
    let mut o = Offers::default();
    o.announce(offer(1));
    o.add_type(&offer(1), "text/plain".into());
    o.select(Some(offer(1)));
    for id in 2..12 {
      o.announce(offer(id));
      o.add_type(&offer(id), "text/uri-list".into());
      o.drag_enter(Some(offer(id)));
      if id % 2 == 0 {
        o.drag_end();
      }
    }

    o.drag_end();
    assert_eq!(gone.get(), 10, "every drag's offer destroyed");
    assert!(o.pending.is_empty() && o.drag.is_none());
    assert_eq!(o.selection.as_ref().map(|(s, t)| (s.0, t.len())), Some((1, 1)));
  }

  #[test]
  fn transfers_never_wait_for_a_reader() {
    // More than a pipe holds: the rest waits for the reader, without blocking the loop.
    let data: Rc<[u8]> = (0..1 << 20).map(|i| i as u8).collect::<Vec<_>>().into();
    let (mut rd, wr) = pipe();
    let mut t = Transfers::default();
    let now = Instant::now();
    t.add(wr, data.clone(), now);
    assert_eq!(t.0.len(), 1, "a full pipe leaves the rest for later");
    let mut got = Vec::new();
    let mut buf = vec![0; 1 << 16];
    while got.len() < data.len() {
      let n = rd.read(&mut buf).unwrap();
      got.extend_from_slice(&buf[..n]);
      t.pump(now);
    }

    assert!(got == *data && t.0.is_empty(), "all written, then let go");

    // A reader that stops is let go after IDLE; one that closes at once.
    let (_rd, wr) = pipe();
    t.add(wr, data.clone(), now);
    t.pump(now + IDLE / 2);
    assert_eq!(t.0.len(), 1);
    assert_eq!(t.deadline(), Some(now + IDLE));
    t.pump(now + IDLE);
    assert!(t.0.is_empty());
    let (rd, wr) = pipe();
    t.add(wr, data, now);
    drop(rd);
    t.pump(now);
    assert!(t.0.is_empty());
  }
}
