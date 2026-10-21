// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The clipboard on `wl_data_device`: plain UTF-8 text. Copying makes a data source that
//! answers `send` with our text, written into the reader's pipe as it takes it ([Transfers]:
//! the loop polls the pipes, it never waits on one); pasting reads the selection's offer through
//! a pipe (our own selection without a round trip).

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
  /// Offers announced (`data_offer`) and the types each has, until a selection picks one.
  pub offers: Vec<(WlDataOffer, Vec<String>)>,
  /// The selection's offer, someone else's.
  pub selection: Option<(WlDataOffer, Vec<String>)>,
  /// Our selection: its source and text.
  pub source: Option<(WlDataSource, String)>,
  /// The serial of the last key or button press, which `set_selection` needs.
  pub serial: u32,
  /// Our text on its way to readers.
  pub transfers: Transfers,
}

impl Clip {
  /// `wl_data_device.selection`: the new selection's offer, or none.
  pub fn select(&mut self, offer: Option<WlDataOffer>) {
    if let Some((old, _)) = self.selection.take() {
      old.destroy();
    }

    let picked = offer.and_then(|o| self.offers.iter().position(|(x, _)| *x == o));
    self.selection = picked.map(|i| self.offers.swap_remove(i));
    for (o, _) in self.offers.drain(..) {
      o.destroy();
    }
  }

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

    let (offer, types) = c.selection.as_ref()?;
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
