// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The CPU renderer's window path on X11: frames drawn into one buffer, a memfd shared with the
//! server through MIT-SHM (`ShmAttachFd`) and put with `ShmPutImage`, only the tiles that changed.
//! The last put of a frame and every expose's put ask for a completion, and nothing is drawn into
//! the buffer until all of them came (the server reads it as it handles a put). A completion that
//! never comes (the put failed) retires the segment for a new one ([Presenter::unstick]), never
//! taken as read. Without MIT-SHM 1.2 (a remote display), the changed rects go through the socket
//! with `PutImage`. The server keeps the window's content; an expose puts the buffer again, no
//! redraw.

use std::os::fd::OwnedFd;
use std::time::{Duration, Instant};

use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};

use super::xcb::X;
use crate::cpu::raster::IRect;
use crate::cpu::{Pixels, Renderer};
use crate::scene::{Scene, Size};

/// Waiting this long for a completion retires the segment: the put failed, or the server
/// stalls.
pub const STUCK: Duration = Duration::from_millis(500);

/// A shared segment: the memfd mapped here and attached on the server.
struct Seg {
  id: u32,
  ptr: *mut u32,
  len: usize,
}

enum Mem {
  Shm(Seg),
  Local(Vec<u32>),
}

pub struct Presenter {
  pub renderer: Renderer,
  x: X,
  win: u32,
  gc: u32,
  depth: u8,
  /// MIT-SHM's completion event code; `None` once it failed or without it.
  shm: Option<u8>,
  mem: Option<Mem>,
  w: usize,
  h: usize,
  /// The tile hashes of the frame the buffer holds.
  tiles: Vec<u64>,
  /// Puts whose completion hasn't come: the server may still read the buffer.
  reads: u32,
  /// Since when the oldest of them waits.
  since: Option<Instant>,
}

impl Presenter {
  pub fn new(x: X, win: u32, depth: u8) -> Self {
    let shm = x.shm();
    let gc = x.create_gc(win);
    let (mem, w, h, tiles) = (None, 0, 0, Vec::new());
    let (reads, since) = (0, None);
    Self { renderer: Renderer::new(), x, win, gc, depth, shm, mem, w, h, tiles, reads, since }
  }

  /// Makes the buffer for frames of `w` × `h` pixels ahead of the first (its segment's attach
  /// is a round trip).
  pub fn reserve(&mut self, w: usize, h: usize) {
    if (w, h) != (self.w, self.h) || self.mem.is_none() {
      self.alloc(w, h);
    }
  }

  /// `ShmCompletion`'s response type, while presents use MIT-SHM.
  pub fn completion(&self) -> Option<u8> {
    self.shm
  }

  /// The server may still read the buffer: nothing is drawn into it.
  pub fn busy(&self) -> bool {
    self.reads > 0
  }

  /// A completion came: once every put's has, the buffer is ours again. Those of a retired
  /// segment don't count.
  pub fn completed(&mut self, seg: u32) {
    if let Some(Mem::Shm(s)) = &self.mem
      && s.id == seg
    {
      self.reads = self.reads.saturating_sub(1);
      if self.reads == 0 {
        self.since = None;
      }
    }
  }

  /// A completion overdue by [STUCK] at `now`: the segment is retired (detached after the puts
  /// before it; the server keeps its own mapping while it reads) and a new one takes its place,
  /// so nothing the server may read is written. Returns whether it was.
  pub fn unstick(&mut self, now: Instant) -> bool {
    if !self.since.is_some_and(|t| now.duration_since(t) >= STUCK) {
      return false;
    }

    self.alloc(self.w, self.h);
    true
  }

  /// Draws the scene's changed tiles into the buffer and puts them in the window; nothing when
  /// the frame equals the one shown. Returns whether anything was put.
  pub fn present(&mut self, scene: &Scene, size: Size, scale: f32) -> bool {
    if size.w <= 0.0 || size.h <= 0.0 {
      return false;
    }

    let r = &mut self.renderer;
    r.prepare(scene, size, scale);
    let (w, h) = r.size();
    self.reserve(w, h);
    let rects = self.renderer.damage(&self.tiles);
    if rects.is_empty() || self.mem.is_none() {
      return false;
    }

    let px = match self.mem.as_mut() {
      Some(Mem::Shm(s)) => unsafe { std::slice::from_raw_parts_mut(s.ptr, w * h) },
      Some(Mem::Local(v)) => v.as_mut_slice(),
      None => return false,
    };

    self.renderer.draw(scene, px, w, &rects);
    self.tiles.clear();
    self.tiles.extend_from_slice(self.renderer.tiles());
    for (i, rc) in rects.iter().enumerate() {
      self.put(*rc, i + 1 == rects.len());
    }

    true
  }

  /// Puts the buffer's pixels in `rc` again (an expose), counted as a read like a frame's;
  /// false when there is no frame to put.
  pub fn expose(&mut self, rc: IRect) -> bool {
    if self.mem.is_none() || self.tiles.is_empty() {
      return false;
    }

    let rc = rc.intersect(IRect::new(0, 0, self.w as i32, self.h as i32));
    if !rc.is_empty() {
      self.put(rc, true);
    }

    true
  }

  /// One rect of the buffer to the window; `notify` asks for a completion, which the buffer
  /// waits for (the server handles requests in order: one for the last put covers those before).
  fn put(&mut self, rc: IRect, notify: bool) {
    let (x0, y0) = (rc.x0 as usize, rc.y0 as usize);
    let (rw, rh) = ((rc.x1 - rc.x0) as usize, (rc.y1 - rc.y0) as usize);
    match &self.mem {
      Some(Mem::Shm(s)) => {
        let rect = [x0 as u16, y0 as u16, rw as u16, rh as u16];
        let total = (self.w as u16, self.h as u16);
        self.x.shm_put_image(self.win, self.gc, total, rect, self.depth, notify, s.id);
        if notify {
          self.reads += 1;
          self.since.get_or_insert_with(Instant::now);
        }
      }
      Some(Mem::Local(v)) => {
        // Bands of whole rows that fit a request (its header is 24 bytes).
        let rows = ((self.x.max_request() - 64) / (rw * 4)).max(1);
        let mut y = y0;
        while y < y0 + rh {
          let n = rows.min(y0 + rh - y);
          let band: Vec<u8> = (y..y + n)
            .flat_map(|row| &v[row * self.w + x0..row * self.w + x0 + rw])
            .flat_map(|p| p.to_ne_bytes())
            .collect();
          self.x.put_image(
            self.win, self.gc, x0 as i16, y as i16, rw as u16, n as u16, self.depth, &band,
          );
          y += n;
        }
      }
      None => {}
    }
  }

  /// A buffer of `w` × `h` pixels: a new shared segment, else memory of our own.
  fn alloc(&mut self, w: usize, h: usize) {
    self.free();
    (self.w, self.h) = (w, h);
    self.tiles.clear();
    if w == 0 || h == 0 {
      return;
    }

    if self.shm.is_some() {
      match self.segment(w * h * 4) {
        Some(s) => {
          self.mem = Some(Mem::Shm(s));
          return;
        }
        None => {
          eprintln!("nimbo: MIT-SHM unavailable, frames go through the socket");
          self.shm = None;
        }
      }
    }

    self.mem = Some(Mem::Local(vec![0; w * h]));
  }

  fn segment(&self, len: usize) -> Option<Seg> {
    let fd: OwnedFd = memfd_create("nimbo-x11", MemfdFlags::CLOEXEC).ok()?;
    ftruncate(&fd, len as u64).ok()?;
    let flags = ProtFlags::READ | ProtFlags::WRITE;
    // SAFETY: a fresh shared mapping of the whole memfd, unmapped in `free`.
    let ptr = unsafe { mmap(std::ptr::null_mut(), len, flags, MapFlags::SHARED, &fd, 0) }.ok()?;
    let id = self.x.id();
    // The server maps its own copy of the fd; XCB closes the one it sends.
    match self.x.shm_attach(id, fd) {
      true => Some(Seg { id, ptr: ptr.cast(), len }),
      false => {
        let _ = unsafe { munmap(ptr, len) };
        None
      }
    }
  }

  /// Lets the buffer go: a segment is detached after the puts before it (the server handles
  /// requests in order) and unmapped here; the server keeps its own mapping until then.
  fn free(&mut self) {
    if let Some(Mem::Shm(s)) = self.mem.take() {
      self.x.shm_detach(s.id);
      let _ = unsafe { munmap(s.ptr.cast(), s.len) };
    }

    (self.reads, self.since) = (0, None);
  }

  /// The pixels of the frame shown.
  pub fn shown(&self) -> Option<Pixels> {
    let px = match self.mem.as_ref()? {
      Mem::Shm(s) => unsafe { std::slice::from_raw_parts(s.ptr, self.w * self.h) },
      Mem::Local(v) => v.as_slice(),
    };

    let data = px.iter().flat_map(|p| (p | 0xff00_0000).to_le_bytes()).collect();
    Some(Pixels { w: self.w, h: self.h, data })
  }
}

impl Drop for Presenter {
  fn drop(&mut self) {
    self.free();
  }
}

#[cfg(test)]
mod tests {
  use super::super::xcb::{Screen, ShmCompletionEv};
  use super::*;
  use crate::scene::{Quad, Rect};

  fn scene(x: f32) -> Scene {
    let mut s = Scene::default();
    s.quad(Quad::new(Rect::new(0.0, 0.0, 64.0, 48.0), [0.1, 0.2, 0.3, 1.0]));
    s.quad(Quad::new(Rect::new(x, 8.0, 16.0, 16.0), [0.9, 0.5, 0.1, 1.0]));
    s
  }

  /// Hands the completions that came to the presenter until it isn't busy; false after a second.
  fn drain(x: &X, p: &mut Presenter) -> bool {
    let first = p.completion().unwrap();
    let until = Instant::now() + Duration::from_secs(1);
    while p.busy() && Instant::now() < until {
      x.flush();
      match x.poll_event() {
        Some(ev) if ev.kind() == first => p.completed(ev.get::<ShmCompletionEv>().shmseg),
        Some(_) => {}
        None => std::thread::sleep(Duration::from_millis(1)),
      }
    }

    !p.busy()
  }

  /// On a display with MIT-SHM (Xvfb), an unmapped window: every put a frame or an expose makes
  /// is waited for, and an overdue one retires the segment. Skipped without `DISPLAY`.
  #[test]
  fn shared_buffer_waits_for_every_read() {
    if std::env::var_os("DISPLAY").is_none() {
      eprintln!("no DISPLAY: skipped");
      return;
    }

    let (x, screen, _) = X::connect().unwrap();
    // SAFETY: the setup's screen, valid while the connection is open.
    let sc: &Screen = unsafe { &*screen };
    let win = x.create_window(sc, 64, 48, 0);
    let mut p = Presenter::new(x, win, sc.root_depth);
    if p.completion().is_none() {
      eprintln!("no MIT-SHM: skipped");
      return;
    }

    let size = Size::new(64.0, 48.0);
    let seg = |p: &Presenter| match &p.mem {
      Some(Mem::Shm(s)) => Some(s.id),
      _ => None,
    };
    assert!(p.present(&scene(4.0), size, 1.0));
    if seg(&p).is_none() {
      eprintln!("no shared segment (a remote display): skipped");
      return;
    }

    assert!(p.busy(), "a frame's put is waited for");
    // An expose's put too; one completion leaves the buffer busy.
    assert!(p.expose(IRect::new(0, 0, 32, 32)));
    assert_eq!(p.reads, 2);
    assert!(drain(&x, &mut p));

    // An expose alone is waited for as well.
    assert!(p.expose(IRect::new(0, 0, 64, 48)));
    assert!(p.busy());
    assert!(drain(&x, &mut p));

    // No completion read for STUCK: the segment is retired for a new one, not taken as read.
    assert!(p.present(&scene(20.0), size, 1.0));
    let old = seg(&p).unwrap();
    assert!(!p.unstick(Instant::now()));
    assert!(p.unstick(Instant::now() + STUCK));
    assert!(seg(&p).is_some_and(|s| s != old) && !p.busy());
    // The old segment's completion doesn't count for the new one.
    assert!(p.present(&scene(30.0), size, 1.0));
    p.completed(old);
    assert!(p.busy());
    assert!(drain(&x, &mut p));

    drop(p);
    x.destroy_window(win);
    x.flush();
    x.disconnect();
  }
}
