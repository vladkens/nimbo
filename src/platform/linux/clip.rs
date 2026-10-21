// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The clipboard on `wl_data_device`: plain UTF-8 text. Copying makes a data source that
//! answers `send` with our text; pasting reads the selection's offer through a pipe (our own
//! selection without a round trip).

use std::cell::RefCell;
use std::io::{Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::rc::Rc;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::pipe::{PipeFlags, pipe_with};
use wayland_client::protocol::wl_data_device::WlDataDevice;
use wayland_client::protocol::wl_data_device_manager::WlDataDeviceManager;
use wayland_client::protocol::wl_data_offer::WlDataOffer;
use wayland_client::protocol::wl_data_source::WlDataSource;
use wayland_client::{Connection, QueueHandle};

use super::Win;

/// Text types, preferred first.
const MIMES: [&str; 3] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];

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

  /// `wl_data_source.send`: writes our text to `fd`.
  pub fn send(&self, source: &WlDataSource, fd: OwnedFd) {
    if let Some((s, text)) = &self.source
      && s == source
    {
      let _ = std::fs::File::from(fd).write_all(text.as_bytes());
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
