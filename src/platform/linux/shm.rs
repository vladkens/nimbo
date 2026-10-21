// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The CPU renderer's window path: frames drawn straight into `wl_shm` buffers on memfds. A
//! buffer the compositor still holds (not yet released) is left alone (a second, at most a
//! third, is made); each keeps the tile hashes of the frame it holds, so only the tiles that
//! changed since are drawn into it, and only their rects are damaged (`damage_buffer`).

use std::os::fd::{AsFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_shm::{Format, WlShm};
use wayland_client::protocol::wl_surface::WlSurface;

use super::Win;
use crate::cpu::{Pixels, Renderer};
use crate::scene::{Scene, Size};

/// Buffers at most: one shown, one drawn, one more while the compositor holds both.
const BUFFERS: usize = 3;

/// Set while the compositor holds the buffer: from its attach to `wl_buffer.release`.
pub type Busy = Arc<AtomicBool>;

struct Buf {
  _fd: OwnedFd,
  buffer: WlBuffer,
  ptr: *mut u32,
  len: usize,
  w: usize,
  h: usize,
  busy: Busy,
  /// The tile hashes of the frame it holds.
  tiles: Vec<u64>,
}

impl Buf {
  fn new(shm: &WlShm, w: usize, h: usize, qh: &QueueHandle<Win>) -> Option<Self> {
    let len = w * h * 4;
    let fd = memfd_create("nimbo", MemfdFlags::CLOEXEC).ok()?;
    ftruncate(&fd, len as u64).ok()?;
    let flags = ProtFlags::READ | ProtFlags::WRITE;
    // SAFETY: a fresh shared mapping of the whole memfd, unmapped on drop.
    let ptr = unsafe { mmap(std::ptr::null_mut(), len, flags, MapFlags::SHARED, &fd, 0) }.ok()?;
    let pool = shm.create_pool(fd.as_fd(), len as i32, qh, ());
    let busy = Busy::default();
    let (wi, hi) = (w as i32, h as i32);
    let buffer = pool.create_buffer(0, wi, hi, wi * 4, Format::Xrgb8888, qh, busy.clone());
    // The buffer keeps the pool's memory.
    pool.destroy();
    Some(Self { _fd: fd, buffer, ptr: ptr.cast(), len, w, h, busy, tiles: Vec::new() })
  }

  fn busy(&self) -> bool {
    self.busy.load(Ordering::Acquire)
  }
}

impl Drop for Buf {
  fn drop(&mut self) {
    self.buffer.destroy();
    // SAFETY: the mapping made in `new`; the compositor has its own.
    let _ = unsafe { munmap(self.ptr.cast(), self.len) };
  }
}

pub struct Presenter {
  pub renderer: Renderer,
  shm: WlShm,
  bufs: Vec<Buf>,
  /// The buffer the surface shows.
  front: Option<usize>,
  /// A frame waits for a buffer to be released.
  pub starved: bool,
}

impl Presenter {
  pub fn new(shm: WlShm) -> Self {
    Self { renderer: Renderer::new(), shm, bufs: Vec::new(), front: None, starved: false }
  }

  /// Draws the scene into a buffer the compositor doesn't hold, attaches it with the changed
  /// rects damaged; nothing when the frame equals the one shown. The caller commits. Returns
  /// whether a buffer was attached.
  pub fn present(
    &mut self,
    scene: &Scene,
    size: Size,
    scale: f32,
    surface: &WlSurface,
    qh: &QueueHandle<Win>,
  ) -> bool {
    self.starved = false;
    if size.w <= 0.0 || size.h <= 0.0 {
      return false;
    }

    let r = &mut self.renderer;
    r.prepare(scene, size, scale);
    let (w, h) = r.size();
    if self.bufs.first().is_some_and(|b| (b.w, b.h) != (w, h)) {
      self.bufs.clear();
      self.front = None;
    }
    if let Some(f) = self.front
      && r.damage(&self.bufs[f].tiles).is_empty()
    {
      return false;
    }

    let free = (0..self.bufs.len()).find(|&i| Some(i) != self.front && !self.bufs[i].busy());
    let i = match free {
      Some(i) => i,
      None if self.bufs.len() < BUFFERS => match Buf::new(&self.shm, w, h, qh) {
        Some(b) => {
          self.bufs.push(b);
          self.bufs.len() - 1
        }
        None => return false,
      },
      None => {
        self.starved = true;
        return false;
      }
    };

    // What changed since the frame shown: the compositor's damage; `b`'s own rects (since its
    // older frame) are what is drawn, and damaged too.
    let shown = self.front.map(|f| r.damage(&self.bufs[f].tiles));
    let b = &mut self.bufs[i];
    let rects = r.damage(&b.tiles);
    // SAFETY: the mapping holds w × h pixels, which the compositor doesn't read until attached.
    let px = unsafe { std::slice::from_raw_parts_mut(b.ptr, w * h) };
    r.draw(scene, px, w, &rects);
    b.tiles.clear();
    b.tiles.extend_from_slice(r.tiles());
    surface.attach(Some(&b.buffer), 0, 0);
    for rc in rects.iter().chain(shown.iter().flatten()) {
      surface.damage_buffer(rc.x0, rc.y0, rc.x1 - rc.x0, rc.y1 - rc.y0);
    }
    b.busy.store(true, Ordering::Release);
    self.front = Some(i);
    true
  }

  /// The pixels of the buffer shown.
  pub fn shown(&self) -> Option<Pixels> {
    let b = &self.bufs[self.front?];
    // SAFETY: the mapping holds w × h pixels.
    let px = unsafe { std::slice::from_raw_parts(b.ptr, b.w * b.h) };
    let data = px.iter().flat_map(|p| (p | 0xff00_0000).to_le_bytes()).collect();
    Some(Pixels { w: b.w, h: b.h, data })
  }
}
