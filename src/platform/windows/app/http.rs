// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A file fetched over HTTP on Windows: WinHTTP with the system's proxy
//! (`WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY`), which follows redirects but not from https to http.

use std::ffi::c_void;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Networking::WinHttp::{
  URL_COMPONENTS, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
  WINHTTP_INTERNET_SCHEME_HTTPS, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
  WinHttpCloseHandle, WinHttpConnect, WinHttpCrackUrl, WinHttpOpen, WinHttpOpenRequest,
  WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
};
use windows_sys::core::BOOL;

use super::super::wide;

/// A WinHTTP handle, closed on drop.
struct Handle(*mut c_void);

impl Drop for Handle {
  fn drop(&mut self) {
    // SAFETY: a handle WinHTTP gave, closed once.
    unsafe { WinHttpCloseHandle(self.0) };
  }
}

fn handle(h: *mut c_void) -> io::Result<Handle> {
  if h.is_null() { Err(io::Error::last_os_error()) } else { Ok(Handle(h)) }
}

fn ok(done: BOOL) -> io::Result<()> {
  if done == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

pub fn get(url: &str, to: &Path) -> io::Result<()> {
  let w = wide(url);
  let mut parts = URL_COMPONENTS {
    dwStructSize: size_of::<URL_COMPONENTS>() as u32,
    dwHostNameLength: u32::MAX,
    dwUrlPathLength: u32::MAX,
    dwExtraInfoLength: u32::MAX,
    ..Default::default()
  };
  // SAFETY: a NUL-terminated URL; the parts point into it, the query right after the path.
  ok(unsafe { WinHttpCrackUrl(w.as_ptr(), 0, 0, &mut parts) })?;
  let part = |p: *const u16, n: u32| {
    // SAFETY: a part WinHttpCrackUrl found in `w`, null when missing.
    let s =
      if p.is_null() { &[][..] } else { unsafe { std::slice::from_raw_parts(p, n as usize) } };
    s.iter().copied().chain([0]).collect::<Vec<u16>>()
  };
  let host = part(parts.lpszHostName, parts.dwHostNameLength);
  let path = part(parts.lpszUrlPath, parts.dwUrlPathLength + parts.dwExtraInfoLength);
  let secure = if parts.nScheme == WINHTTP_INTERNET_SCHEME_HTTPS { WINHTTP_FLAG_SECURE } else { 0 };

  let (agent, verb, proxy) = (wide("nimbo"), wide("GET"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY);
  let (mut code, mut len) = (0u32, 4u32);
  // SAFETY: NUL-terminated strings, null for the defaults; a handle dropped after its children
  // (locals drop in reverse); the status into a u32 of its length.
  unsafe {
    let session = handle(WinHttpOpen(agent.as_ptr(), proxy, null(), null(), 0))?;
    let connect = handle(WinHttpConnect(session.0, host.as_ptr(), parts.nPort, 0))?;
    let req =
      WinHttpOpenRequest(connect.0, verb.as_ptr(), path.as_ptr(), null(), null(), null(), secure);
    let req = handle(req)?;
    ok(WinHttpSendRequest(req.0, null(), 0, null(), 0, 0, 0))?;
    ok(WinHttpReceiveResponse(req.0, null_mut()))?;
    let query = WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER;
    ok(WinHttpQueryHeaders(req.0, query, null(), (&raw mut code).cast(), &mut len, null_mut()))?;
    crate::platform::status(url, code)?;
    read(&req, to)
  }
}

/// The body of the request `req` into a new file at `to`.
fn read(req: &Handle, to: &Path) -> io::Result<()> {
  let (mut file, mut buf) = (File::create(to)?, vec![0u8; 1 << 16]);
  loop {
    let mut n = 0;
    // SAFETY: a buffer of its length.
    ok(unsafe { WinHttpReadData(req.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut n) })?;
    if n == 0 {
      return file.sync_all();
    }

    file.write_all(&buf[..n as usize])?;
  }
}
