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

/// Whether the server takes the frames' pixels as they are: 32 bits each, `0xRRGGBB` in the
/// root visual's masks, least significant byte first (`setup` is the connection setup, the
/// screen at `screen_at`). Others (a BGR VNC server, a big-endian display) aren't converted.
pub fn check_format(setup: &[u8], screen_at: usize) -> Result<(), String> {
  let u16_at = |i: usize| Some(u16::from_ne_bytes(setup.get(i..i + 2)?.try_into().ok()?));
  let u32_at = |i: usize| Some(u32::from_ne_bytes(setup.get(i..i + 4)?.try_into().ok()?));
  let bad = || "a malformed X connection setup".to_string();

  // The setup: the image byte order at 30, then after the vendor string 8-byte pixel formats
  // (depth, bits per pixel).
  let lsb_first = *setup.get(30).ok_or_else(bad)? == 0;
  let vendor = u16_at(24).ok_or_else(bad)? as usize;
  let formats = 40 + vendor.next_multiple_of(4);
  let n = *setup.get(29).ok_or_else(bad)? as usize;
  // The screen: the root visual at 32, its depth at 38, then its depths (8 bytes each) with
  // their visuals (24 bytes: the id, then the red, green and blue masks at 8).
  let visual = u32_at(screen_at + 32).ok_or_else(bad)?;
  let depth = *setup.get(screen_at + 38).ok_or_else(bad)?;
  let bpp = (0..n).find_map(|i| {
    let f = setup.get(formats + 8 * i..formats + 8 * i + 2)?;
    (f[0] == depth).then_some(f[1])
  });

  let mut masks = None;
  let mut at = screen_at + 40;
  for _ in 0..*setup.get(screen_at + 39).ok_or_else(bad)? {
    let visuals = u16_at(at + 2).ok_or_else(bad)? as usize;
    for v in (0..visuals).map(|i| at + 8 + 24 * i) {
      if u32_at(v) == Some(visual) {
        masks = Some([u32_at(v + 8), u32_at(v + 12), u32_at(v + 16)]);
      }
    }

    at += 8 + 24 * visuals;
  }

  let rgb = [Some(0xff0000), Some(0xff00), Some(0xff)];
  if lsb_first && bpp == Some(32) && masks == Some(rgb) {
    return Ok(());
  }

  let [r, g, b] = masks.unwrap_or_default().map(Option::unwrap_or_default);
  let order = if lsb_first { "LSB first" } else { "MSB first" };
  Err(format!(
    "the X screen's pixels are {} bits, red {r:#x} green {g:#x} blue {b:#x}, {order}: only \
     32-bit 0xRRGGBB, LSB first, is drawn",
    bpp.unwrap_or(0)
  ))
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

  /// A connection setup with one pixel format and one screen of one depth with one visual, the
  /// root's; the screen's offset.
  fn setup(lsb_first: bool, bpp: u8, masks: [u32; 3]) -> (Vec<u8>, usize) {
    let mut d = vec![0u8; 40];
    d[24..26].copy_from_slice(&3u16.to_ne_bytes());
    (d[28], d[29], d[30]) = (1, 1, if lsb_first { 0 } else { 1 });
    d.extend(b"Xvf\0");
    d.extend([24, bpp, 32, 0, 0, 0, 0, 0]);
    let at = d.len();
    let mut screen = [0u8; 40];
    screen[32..36].copy_from_slice(&0x21u32.to_ne_bytes());
    (screen[38], screen[39]) = (24, 1);
    d.extend(screen);
    d.extend([24, 0]);
    d.extend(1u16.to_ne_bytes());
    d.extend([0; 4]);
    d.extend(0x21u32.to_ne_bytes());
    d.extend([4, 8, 0, 1]);
    for m in masks {
      d.extend(m.to_ne_bytes());
    }

    d.extend([0; 4]);
    (d, at)
  }

  #[test]
  fn only_rgb_pixels_are_taken() {
    let ok = |(d, at): (Vec<u8>, usize)| check_format(&d, at);
    let rgb = [0xff0000, 0xff00, 0xff];
    assert_eq!(ok(setup(true, 32, rgb)), Ok(()));

    let bgr = ok(setup(true, 32, [0xff, 0xff00, 0xff0000])).unwrap_err();
    assert!(bgr.contains("red 0xff green 0xff00 blue 0xff0000"), "{bgr}");
    assert!(ok(setup(false, 32, rgb)).unwrap_err().contains("MSB first"));
    assert!(ok(setup(true, 24, rgb)).unwrap_err().contains("are 24 bits"));

    let (d, at) = setup(true, 32, rgb);
    assert!(check_format(&d[..d.len() - 8], at).is_err(), "cut short");
  }

  /// The display's own setup passes (Xvfb, a desktop's server). Skipped without `DISPLAY`.
  #[test]
  fn the_display_takes_our_pixels() {
    if std::env::var_os("DISPLAY").is_none() {
      eprintln!("no DISPLAY: skipped");
      return;
    }

    let (x, screen, _) = X::connect().unwrap();
    let (setup, at) = x.setup(screen);
    assert_eq!(check_format(setup, at), Ok(()));
    x.disconnect();
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
