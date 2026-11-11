// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Images decoded by gdk-pixbuf ([crate::Bitmap]), loaded at run time as GTK is: a
//! `GdkPixbufLoader` fed the file, which its `size-prepared` signal shrinks (a JPEG decodes at a
//! fraction of its full size, other formats in full and shrunk after), its first frame turned
//! upright by `gdk_pixbuf_apply_embedded_orientation`, then premultiplied. The formats are the
//! loaders the system has (WebP and HEIC from the distribution's `webp-pixbuf-loader` and
//! `libheif`). The loader is used by one thread at a time and gdk-pixbuf locks the loaders that
//! aren't thread-safe: any thread.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr::null_mut;
use std::sync::OnceLock;

use super::dl::{Lib, fns};
use crate::bitmap::{MAX_PIXELS, premultiply, size, zeroed};

type P = *mut c_void;

fns!(Pixbuf {
  fn gdk_pixbuf_loader_new() -> P;
  fn gdk_pixbuf_loader_write(l: P, buf: *const u8, len: usize, err: *mut *mut GError) -> c_int;
  fn gdk_pixbuf_loader_close(l: P, err: *mut *mut GError) -> c_int;
  fn gdk_pixbuf_loader_get_pixbuf(l: P) -> P;
  fn gdk_pixbuf_loader_set_size(l: P, w: c_int, h: c_int);
  fn gdk_pixbuf_apply_embedded_orientation(pb: P) -> P;
  fn gdk_pixbuf_get_width(pb: P) -> c_int;
  fn gdk_pixbuf_get_height(pb: P) -> c_int;
  fn gdk_pixbuf_get_rowstride(pb: P) -> c_int;
  fn gdk_pixbuf_get_n_channels(pb: P) -> c_int;
  fn gdk_pixbuf_get_bits_per_sample(pb: P) -> c_int;
  fn gdk_pixbuf_read_pixels(pb: P) -> *const u8;
  fn g_signal_connect_data(o: P, s: *const c_char, cb: P, data: P, destroy: P, flags: c_int) -> u64;
  fn g_object_unref(o: P);
  fn g_error_free(e: *mut GError);
});

/// GLib's `GError`.
#[repr(C)]
struct GError {
  domain: u32,
  code: c_int,
  message: *const c_char,
}

fn pixbuf() -> Option<&'static Pixbuf> {
  static PIXBUF: OnceLock<Option<Pixbuf>> = OnceLock::new();
  PIXBUF.get_or_init(|| Pixbuf::load(&Lib::open(&[c"libgdk_pixbuf-2.0.so.0"])?)).as_ref()
}

pub fn decode(file: &[u8], side: u32) -> Result<(u32, u32, Vec<u8>), String> {
  let f = pixbuf().ok_or("gdk-pixbuf is not installed")?;
  let jpeg = file.starts_with(&[0xff, 0xd8, 0xff]);
  let mut ask = Ask { side, jpeg, refused: None };
  // SAFETY: GLib's calls with the loader alive until its unref; `ask` outlives the signal, which
  // comes during the writes; the pixbuf is the loader's (borrowed) or an owned turned copy.
  unsafe {
    let loader = (f.gdk_pixbuf_loader_new)();
    let data = (&raw mut ask).cast();
    (f.g_signal_connect_data)(
      loader,
      c"size-prepared".as_ptr(),
      prepared as P,
      data,
      null_mut(),
      0,
    );
    let mut err = null_mut();
    let read = (f.gdk_pixbuf_loader_write)(loader, file.as_ptr(), file.len(), &mut err) != 0;
    // Closed in any case, its error kept only when the write had none.
    let closed = (f.gdk_pixbuf_loader_close)(loader, if read { &mut err } else { null_mut() }) != 0;
    let pb = (f.gdk_pixbuf_loader_get_pixbuf)(loader);
    let image = if read && closed && !pb.is_null() {
      // A new pixbuf, null without memory for it (the loader's is unref'd with the loader).
      let upright = (f.gdk_pixbuf_apply_embedded_orientation)(pb);
      Some(match upright.is_null() {
        true => Err("no memory to turn the image".into()),
        false => {
          let image = pixels(f, upright);
          (f.g_object_unref)(upright);
          image
        }
      })
    } else {
      None
    };

    (f.g_object_unref)(loader);
    let why =
      (!err.is_null()).then(|| CStr::from_ptr((*err).message).to_string_lossy().into_owned());
    if !err.is_null() {
      (f.g_error_free)(err);
    }

    match (ask.refused, image) {
      (Some(e), _) => Err(e),
      (None, Some(image)) => image,
      (None, None) => Err(why.unwrap_or_else(|| "not an image the system reads".into())),
    }
  }
}

/// What the loader's `size-prepared` is asked: the side, whether the file is a JPEG, and why the
/// image was refused.
struct Ask {
  side: u32,
  jpeg: bool,
  refused: Option<String>,
}

/// `size-prepared`: the image is `w` × `h` (its header read, nothing decoded); shrinks it to the
/// side asked, or refuses it over the limits with a size of 0, which stops the loader. Only the
/// JPEG loader decodes smaller (by 1/2, 1/4 or 1/8, `scale_denom` in gdk-pixbuf's `io-jpeg.c`);
/// the others decode the full picture and the loader shrinks it after, so its full size is held
/// to [MAX_PIXELS] too.
unsafe extern "C" fn prepared(loader: P, w: c_int, h: c_int, data: P) {
  let ask = unsafe { &mut *data.cast::<Ask>() };
  let (w, h) = (w.max(0) as u32, h.max(0) as u32);
  let fits = size(w, h, ask.side).and_then(|(fw, fh)| {
    let mut d = 1;
    while ask.jpeg && d < 8 && w.div_ceil(d * 2) >= fw && h.div_ceil(d * 2) >= fh {
      d *= 2;
    }

    match w.div_ceil(d) as u64 * h.div_ceil(d) as u64 {
      n if n > MAX_PIXELS => Err(format!("a {w} × {h} picture is too large")),
      _ => Ok((fw, fh)),
    }
  });
  let (fw, fh) = fits.unwrap_or_else(|e| {
    ask.refused = Some(e);
    (0, 0)
  });
  if (fw, fh) != (w, h) {
    let f = pixbuf().expect("loaded before its loader");
    unsafe { (f.gdk_pixbuf_loader_set_size)(loader, fw as c_int, fh as c_int) };
  }
}

/// `pb`'s size and premultiplied RGBA pixels: 8-bit RGB or RGBA, as gdk-pixbuf's loaders give.
unsafe fn pixels(f: &Pixbuf, pb: P) -> Result<(u32, u32, Vec<u8>), String> {
  let (w, h) = unsafe { ((f.gdk_pixbuf_get_width)(pb), (f.gdk_pixbuf_get_height)(pb)) };
  let (n, bits) =
    unsafe { ((f.gdk_pixbuf_get_n_channels)(pb), (f.gdk_pixbuf_get_bits_per_sample)(pb)) };
  let stride = unsafe { (f.gdk_pixbuf_get_rowstride)(pb) } as usize;
  let src = unsafe { (f.gdk_pixbuf_read_pixels)(pb) };
  if w <= 0 || h <= 0 || bits != 8 || !(n == 3 || n == 4) || src.is_null() {
    return Err("an image of a kind not read".into());
  }

  let mut px = zeroed(w as u32, h as u32)?;
  let (w, n) = (w as usize, n as usize);
  for (y, to) in px.chunks_exact_mut(w * 4).enumerate() {
    // SAFETY: each row is `w * n` bytes at its stride; the last one isn't padded.
    let row = unsafe { std::slice::from_raw_parts(src.add(y * stride), w * n) };
    for (p, q) in row.chunks_exact(n).zip(to.as_chunks_mut::<4>().0) {
      q.copy_from_slice(&[p[0], p[1], p[2], if n == 4 { p[3] } else { 255 }]);
    }
  }

  premultiply(&mut px);
  Ok((w as u32, h as u32, px))
}
