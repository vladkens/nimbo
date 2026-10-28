// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A file fetched over HTTP on Linux: libcurl's easy API, loaded at run time (`libcurl.so.4`,
//! else Debian's GnuTLS build), its write callback into the file, redirects only to https (http
//! too from loopback) ([libcurl-easy](https://curl.se/libcurl/c/libcurl-easy.html), as of
//! 2026-10).

use std::any::Any;
use std::ffi::{CStr, CString, c_int, c_long, c_void};
use std::fs::File;
use std::io::{self, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::OnceLock;

use super::super::dl::{Lib, fns};
use super::super::{Fp, P, S};

// <curl/curl.h>: options are a type's base (long 0, object 10000, function 20000) plus a number.
const URL: c_int = 10002;
const WRITEDATA: c_int = 10001;
const WRITEFUNCTION: c_int = 20011;
const FOLLOWLOCATION: c_int = 52;
const MAXREDIRS: c_int = 68;
/// Since 7.85; before it the bit mask of `REDIR_PROTOCOLS` (Ubuntu 22.04's 7.81).
const REDIR_PROTOCOLS_STR: c_int = 10319;
const REDIR_PROTOCOLS: c_int = 182;
const NOSIGNAL: c_int = 99;
/// Below a byte a second for a minute, the transfer stops.
const LOW_SPEED_LIMIT: c_int = 19;
const LOW_SPEED_TIME: c_int = 20;
/// `CURLINFO_LONG + 2`.
const RESPONSE_CODE: c_int = 0x200002;

fns!(Easy {
  fn curl_global_init(flags: c_long) -> c_int;
  fn curl_easy_init() -> P;
  fn curl_easy_perform(h: P) -> c_int;
  fn curl_easy_cleanup(h: P);
  fn curl_easy_strerror(code: c_int) -> S;
});

/// The easy API, with `curl_easy_setopt` and `curl_easy_getinfo`, which `fns!` can't declare.
struct Curl {
  f: Easy,
  setopt: unsafe extern "C" fn(P, c_int, ...) -> c_int,
  getinfo: unsafe extern "C" fn(P, c_int, ...) -> c_int,
}

fn curl() -> Option<&'static Curl> {
  static CURL: OnceLock<Option<Curl>> = OnceLock::new();
  let load = || {
    let lib = Lib::open(&[c"libcurl.so.4", c"libcurl-gnutls.so.4"])?;
    // SAFETY: curl.h's declarations.
    let (setopt, getinfo) =
      unsafe { (lib.sym("curl_easy_setopt\0")?, lib.sym("curl_easy_getinfo\0")?) };
    let f = Easy::load(&lib)?;
    // SAFETY: once, before any handle; `CURL_GLOBAL_DEFAULT`.
    (unsafe { (f.curl_global_init)(3) } == 0).then_some(Curl { f, setopt, getinfo })
  };
  CURL.get_or_init(load).as_ref()
}

/// The file a transfer writes to, the error that stopped it (a bare `InvalidData` past the
/// bytes it may take), those bytes, what is told the bytes left after each write, and its panic.
type Out<'a> = (File, Option<io::Error>, u64, &'a mut dyn FnMut(u64), Option<Box<dyn Any + Send>>);

/// curl's `CURLOPT_WRITEFUNCTION`: `size × n` bytes at `data`, all taken or the transfer stops.
unsafe extern "C" fn write(data: *const u8, size: usize, n: usize, out: P) -> usize {
  // SAFETY: curl passes the `Out` set as `CURLOPT_WRITEDATA` and that many bytes.
  let ((file, err, left, tell, panicked), data) =
    unsafe { (&mut *out.cast::<Out>(), std::slice::from_raw_parts(data, size * n)) };
  let fits = left.checked_sub(data.len() as u64).ok_or(io::ErrorKind::InvalidData.into());
  // Called no more once it took nothing.
  *err = fits.and_then(|l| file.write_all(data).map(|()| *left = l)).err();
  // A panic can't unwind through curl: it stops the transfer and goes on past it.
  *panicked = panic::catch_unwind(AssertUnwindSafe(|| tell(*left))).err();
  if err.is_some() || panicked.is_some() { 0 } else { size * n }
}

pub fn get(url: &str, to: &Path, max: u64, progress: &mut dyn FnMut(u64)) -> io::Result<()> {
  let none = || io::Error::new(io::ErrorKind::Unsupported, "libcurl.so.4 is not installed");
  let c = curl().ok_or_else(none)?;
  let c_url = CString::new(url).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, url))?;
  let tell = &mut |left| progress(max - left);
  let (mut out, mut code): (Out, c_long) = ((File::create(to)?, None, max, tell, None), 0);
  // SAFETY: a handle of its own, its options of their documented types, cleaned up before `out`
  // and the URL go.
  let done = unsafe {
    let h = (c.f.curl_easy_init)();
    if h.is_null() {
      return Err(io::Error::other("curl_easy_init failed"));
    }

    (c.setopt)(h, URL, c_url.as_ptr());
    (c.setopt)(h, FOLLOWLOCATION, 1 as c_long);
    (c.setopt)(h, MAXREDIRS, 10 as c_long);
    // `CURLPROTO_HTTPS` 2, with `CURLPROTO_HTTP` 1 from loopback.
    let http = crate::update::loopback(url);
    let names = if http { c"http,https" } else { c"https" };
    if (c.setopt)(h, REDIR_PROTOCOLS_STR, names.as_ptr()) != 0 {
      (c.setopt)(h, REDIR_PROTOCOLS, 2 | http as c_long);
    }

    (c.setopt)(h, NOSIGNAL, 1 as c_long);
    (c.setopt)(h, LOW_SPEED_LIMIT, 1 as c_long);
    (c.setopt)(h, LOW_SPEED_TIME, 60 as c_long);
    (c.setopt)(h, WRITEFUNCTION, write as Fp);
    (c.setopt)(h, WRITEDATA, (&raw mut out).cast::<c_void>());
    let done = (c.f.curl_easy_perform)(h);
    (c.getinfo)(h, RESPONSE_CODE, &raw mut code);
    (c.f.curl_easy_cleanup)(h);
    done
  };

  if let Some(p) = out.4 {
    panic::resume_unwind(p);
  }

  // A 404's page may be past `max`: the status tells first.
  if code != 0 {
    crate::platform::status(url, code as u32)?;
  }

  let big = out.1.as_ref().is_some_and(|e| e.kind() == io::ErrorKind::InvalidData);
  out.1.map_or(Ok(()), |e| Err(if big { crate::platform::too_big(url, max) } else { e }))?;

  if done != 0 {
    // SAFETY: curl's static message for its code.
    let why = unsafe { CStr::from_ptr((c.f.curl_easy_strerror)(done)) };
    return Err(io::Error::other(format!("{url}: {}", why.to_string_lossy())));
  }

  crate::platform::status(url, code as u32)?;
  out.0.sync_all()
}
