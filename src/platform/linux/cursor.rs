// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Cursors on compositors without cursor-shape-v1: the Xcursor theme's images (`XCURSOR_THEME`,
//! else `default`, and the themes it inherits; `XCURSOR_SIZE`, else 24), as libXcursor finds
//! them, drawn into `wl_shm` buffers on a surface of ours that `wl_pointer.set_cursor` shows.
//! Each cursor is read once per scale. No theme: the compositor's cursor stays.

use std::os::fd::AsFd;
use std::path::{Path, PathBuf};

use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_client::protocol::wl_shm::{Format, WlShm};
use wayland_client::protocol::wl_surface::WlSurface;

use super::Win;
use super::shm::Busy;
use crate::ui::Cursor;

/// Xcursor's image chunk type (<X11/Xcursor/Xcursor.h>, `XCURSOR_IMAGE_TYPE`).
const IMAGE: u32 = 0xfffd_0002;

/// A cursor image: premultiplied ARGB rows, as Xcursor stores it and `wl_shm`'s ARGB8888 takes.
#[derive(Debug, PartialEq)]
pub struct Image {
  pub w: u32,
  pub h: u32,
  pub hot: (u32, u32),
  pub px: Vec<u32>,
}

/// The theme's names for a cursor: the CSS name, then the legacy X ones.
fn names(c: Cursor) -> &'static [&'static str] {
  match c {
    Cursor::Inherit | Cursor::Arrow => &["default", "left_ptr"],
    Cursor::Text => &["text", "xterm"],
    Cursor::Pointer => &["pointer", "hand2"],
    Cursor::ResizeX => &["col-resize", "sb_h_double_arrow"],
    Cursor::ResizeY => &["row-resize", "sb_v_double_arrow"],
    Cursor::Crosshair => &["crosshair"],
    Cursor::Grab => &["grab", "openhand", "hand1"],
    Cursor::Grabbing => &["grabbing", "closedhand", "fleur"],
    Cursor::NotAllowed => &["not-allowed", "crossed_circle"],
  }
}

/// A cursor's buffer and hotspot in points; `None` in [Cursors] when the theme has none.
type Made = (WlBuffer, (i32, i32));

pub struct Cursors {
  surface: WlSurface,
  shm: WlShm,
  theme: String,
  size: u32,
  /// Where themes are looked for.
  dirs: Vec<PathBuf>,
  /// Each cursor made, at a scale.
  made: Vec<(Cursor, i32, Option<Made>)>,
}

impl Cursors {
  pub fn new(compositor: &WlCompositor, shm: WlShm, qh: &QueueHandle<Win>) -> Self {
    let theme = std::env::var("XCURSOR_THEME").ok().filter(|t| !t.is_empty());
    let size = std::env::var("XCURSOR_SIZE").ok().and_then(|s| s.parse().ok());
    Self {
      surface: compositor.create_surface(qh, ()),
      shm,
      theme: theme.unwrap_or_else(|| "default".into()),
      size: size.filter(|&s| s > 0).unwrap_or(24),
      dirs: dirs(),
      made: Vec::new(),
    }
  }

  /// Shows cursor `c` at integer `scale` for the pointer's enter `serial`.
  pub fn set(
    &mut self,
    pointer: &WlPointer,
    serial: u32,
    c: Cursor,
    scale: i32,
    qh: &QueueHandle<Win>,
  ) {
    let i = match self.made.iter().position(|m| m.0 == c && m.1 == scale) {
      Some(i) => i,
      None => {
        let path = find(&self.dirs, &self.theme, names(c));
        let img = path.and_then(|p| parse(&std::fs::read(p).ok()?, self.size * scale as u32));
        let buf = img.and_then(|img| {
          let hot = (img.hot.0 as i32 / scale, img.hot.1 as i32 / scale);
          Some((buffer(&self.shm, &img, qh)?, hot))
        });
        self.made.push((c, scale, buf));
        self.made.len() - 1
      }
    };

    let Some((buf, hot)) = &self.made[i].2 else { return };
    self.surface.set_buffer_scale(scale);
    self.surface.attach(Some(buf), 0, 0);
    self.surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
    self.surface.commit();
    pointer.set_cursor(serial, Some(&self.surface), hot.0, hot.1);
  }
}

impl Drop for Cursors {
  fn drop(&mut self) {
    for (_, _, buf) in self.made.drain(..) {
      if let Some((b, _)) = buf {
        b.destroy();
      }
    }

    self.surface.destroy();
  }
}

/// A `wl_shm` buffer holding `img`; the buffer keeps the memory after the pool and mapping go.
fn buffer(shm: &WlShm, img: &Image, qh: &QueueHandle<Win>) -> Option<WlBuffer> {
  let len = img.px.len() * 4;
  let fd = memfd_create("nimbo-cursor", MemfdFlags::CLOEXEC).ok()?;
  ftruncate(&fd, len as u64).ok()?;
  let flags = ProtFlags::READ | ProtFlags::WRITE;
  // SAFETY: a fresh shared mapping of the whole memfd, unmapped below.
  let ptr = unsafe { mmap(std::ptr::null_mut(), len, flags, MapFlags::SHARED, &fd, 0) }.ok()?;
  unsafe { std::ptr::copy_nonoverlapping(img.px.as_ptr(), ptr.cast(), img.px.len()) };
  let _ = unsafe { munmap(ptr, len) };
  let pool = shm.create_pool(fd.as_fd(), len as i32, qh, ());
  let (w, h) = (img.w as i32, img.h as i32);
  let buf = pool.create_buffer(0, w, h, w * 4, Format::Argb8888, qh, Busy::default());
  pool.destroy();
  Some(buf)
}

/// Where themes are, as libXcursor looks: `XCURSOR_PATH`, else the XDG data dirs' `icons`,
/// `~/.icons` and `/usr/share/pixmaps`.
fn dirs() -> Vec<PathBuf> {
  if let Ok(p) = std::env::var("XCURSOR_PATH") {
    return std::env::split_paths(&p).collect();
  }

  let home = std::env::var_os("HOME").map(PathBuf::from);
  let data_home = std::env::var_os("XDG_DATA_HOME")
    .map(PathBuf::from)
    .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
  let data =
    std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
  let mut out: Vec<PathBuf> = data_home.into_iter().map(|d| d.join("icons")).collect();
  out.extend(home.map(|h| h.join(".icons")));
  out.extend(std::env::split_paths(&data).map(|d| d.join("icons")));
  out.push("/usr/share/pixmaps".into());
  out
}

/// The file of the first of `names` in `theme`, else in the themes it inherits (`Inherits=` in
/// its `index.theme`), depth first.
fn find(dirs: &[PathBuf], theme: &str, names: &[&str]) -> Option<PathBuf> {
  let mut seen = Vec::new();
  find_in(dirs, theme, names, &mut seen)
}

fn find_in(
  dirs: &[PathBuf],
  theme: &str,
  names: &[&str],
  seen: &mut Vec<String>,
) -> Option<PathBuf> {
  if seen.iter().any(|t| t == theme) || seen.len() > 16 {
    return None;
  }

  seen.push(theme.to_owned());
  let file = names
    .iter()
    .find_map(|n| dirs.iter().map(|d| d.join(theme).join("cursors").join(n)).find(|p| p.is_file()));
  if file.is_some() {
    return file;
  }

  let parents: Vec<String> =
    dirs.iter().filter_map(|d| inherits(&d.join(theme))).flatten().collect();
  parents.iter().find_map(|t| find_in(dirs, t, names, seen))
}

/// The themes a theme dir's `index.theme` inherits.
fn inherits(dir: &Path) -> Option<Vec<String>> {
  let text = std::fs::read_to_string(dir.join("index.theme")).ok()?;
  let line =
    text.lines().find_map(|l| l.trim().strip_prefix("Inherits")?.trim().strip_prefix('='))?;
  Some(line.split([',', ';']).map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()).collect())
}

/// The image of an Xcursor file whose nominal size is nearest `size` (its first frame).
/// The format: "Xcur", the header's length, a version, the table of contents' length, then
/// (type, subtype, position) entries; an image chunk is a 36-byte header (its length, type,
/// nominal size, version, width, height, hotspot, delay) and the pixels, little-endian.
pub fn parse(d: &[u8], size: u32) -> Option<Image> {
  let u32_at = |i: usize| Some(u32::from_le_bytes(d.get(i..i + 4)?.try_into().ok()?));
  if d.get(..4)? != b"Xcur" {
    return None;
  }

  let (header, n) = (u32_at(4)? as usize, u32_at(12)? as usize);
  let toc = (0..n.min(4096)).filter_map(|i| {
    let at = header + i * 12;
    Some((u32_at(at)?, u32_at(at + 4)?, u32_at(at + 8)? as usize))
  });
  let images: Vec<(u32, usize)> = toc.filter(|t| t.0 == IMAGE).map(|t| (t.1, t.2)).collect();
  let best = images.iter().map(|i| i.0).min_by_key(|&s| s.abs_diff(size))?;
  let &(_, at) = images.iter().find(|i| i.0 == best)?;

  let (w, h) = (u32_at(at + 16)?, u32_at(at + 20)?);
  if w == 0 || h == 0 || w > 1024 || h > 1024 {
    return None;
  }

  let hot = (u32_at(at + 24)?.min(w - 1), u32_at(at + 28)?.min(h - 1));
  let start = at + u32_at(at)? as usize;
  let bytes = d.get(start..start + (w * h * 4) as usize)?;
  let px = bytes.as_chunks::<4>().0.iter().map(|&b| u32::from_le_bytes(b)).collect();
  Some(Image { w, h, hot, px })
}

#[cfg(test)]
mod tests {
  use super::*;

  /// An Xcursor file with one image per (nominal size, side, hotspot, colour).
  fn xcursor(images: &[(u32, u32, (u32, u32), u32)]) -> Vec<u8> {
    let mut d = b"Xcur".to_vec();
    let header = 16u32;
    d.extend(header.to_le_bytes());
    d.extend(0x1_0000u32.to_le_bytes());
    d.extend((images.len() as u32).to_le_bytes());
    let mut at = header as usize + images.len() * 12;
    let mut chunks = Vec::new();
    for &(size, side, hot, colour) in images {
      d.extend(IMAGE.to_le_bytes());
      d.extend(size.to_le_bytes());
      d.extend((at as u32).to_le_bytes());
      let mut c = Vec::new();
      for v in [36, IMAGE, size, 1, side, side, hot.0, hot.1, 50] {
        c.extend(u32::to_le_bytes(v));
      }

      c.extend((0..side * side).flat_map(|_| colour.to_le_bytes()));
      at += c.len();
      chunks.extend(c);
    }

    d.extend(chunks);
    d
  }

  #[test]
  fn the_nearest_size_is_read() {
    let d = xcursor(&[(24, 24, (4, 2), 0xff00_00ff), (48, 48, (8, 4), 0xff00_ff00)]);
    let img = parse(&d, 24).unwrap();
    assert_eq!((img.w, img.h, img.hot, img.px.len()), (24, 24, (4, 2), 24 * 24));
    assert!(img.px.iter().all(|&p| p == 0xff00_00ff));
    let img = parse(&d, 40).unwrap();
    assert_eq!((img.w, img.hot, img.px[0]), (48, (8, 4), 0xff00_ff00));
    assert_eq!(parse(&d[..d.len() - 1], 48), None, "cut short");
    assert_eq!(parse(b"nope", 24), None);
  }

  #[test]
  fn themes_inherit() {
    let root = std::env::temp_dir().join(format!("nimbo-cursors-{}", std::process::id()));
    let dir = |p: &str| {
      let d = root.join(p);
      std::fs::create_dir_all(&d).unwrap();
      d
    };
    std::fs::write(dir("default").join("index.theme"), "[Icon Theme]\nInherits=Mine\n").unwrap();
    let cursors = dir("Mine/cursors");
    std::fs::write(cursors.join("left_ptr"), xcursor(&[(24, 2, (0, 0), 0)])).unwrap();
    std::fs::write(dir("Mine").join("index.theme"), "Inherits=default\n").unwrap();
    let dirs = [root.join("none"), root.clone()];
    assert_eq!(find(&dirs, "default", names(Cursor::Arrow)), Some(cursors.join("left_ptr")));
    assert_eq!(find(&dirs, "default", names(Cursor::Text)), None, "a cycle ends");
    std::fs::remove_dir_all(&root).unwrap();
  }
}
