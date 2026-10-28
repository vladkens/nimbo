// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! One instance on Windows ([crate::instance]): the lock file, and a named pipe,
//! `\\.\pipe\nimbo-<hash of the folder>`, of one instance that serves one sender at a time, local
//! ones only, open to this user and the system alone. A sender waits until the instance hangs
//! up: it has read the line then. Ported from wavo-app's `src/ctl.rs` (vladkens, MIT,
//! <https://github.com/vladkens/wavo-app>).

use std::fs::File;
use std::io::{self, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
  ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
  ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
  FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION,
};
use windows_sys::Win32::System::Pipes::{
  ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
  PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, PeekNamedPipe,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};

use crate::instance::{READ_TIMEOUT, hash, read_polled};

/// How long the instance waits for its thread to end.
const BUSY_WAIT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(5);
/// Full access for the pipe's owner (this user) and the system, none for anyone else: the
/// default security lets every local user read, and so hold the one instance.
const SDDL: &str = "D:P(A;;GA;;;OW)(A;;GA;;;SY)";

/// The lock file at `path`, with write access: LockFileEx refuses an append-only handle.
pub fn lock_file(path: &Path) -> io::Result<File> {
  File::options().create(true).write(true).truncate(false).open(path)
}

/// The lock and the pipe; on drop the thread ends before the pipe goes, then the lock.
pub struct Server {
  pipe: Arc<OwnedHandle>,
  path: PathBuf,
  stop: Arc<AtomicBool>,
  thread: Option<JoinHandle<()>>,
  _lock: File,
}

impl Drop for Server {
  /// A sender wakes the thread from `ConnectNamedPipe`; one that does not end is left, keeping
  /// the pipe until the process ends.
  fn drop(&mut self) {
    self.stop.store(true, Ordering::Release);
    let Some(thread) = self.thread.take() else { return };
    let deadline = Instant::now() + BUSY_WAIT;
    while !thread.is_finished() && Instant::now() < deadline {
      _ = client(&self.path, deadline);
      thread::sleep(Duration::from_millis(10));
    }

    if thread.is_finished() {
      _ = thread.join();
    }
  }
}

impl Server {
  /// The first instance of the pipe: a pipe of that name made by another process fails it.
  pub fn bind(dir: &Path, lock: File) -> io::Result<Self> {
    let path = pipe_path(dir);
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mode = PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;
    let open = PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE;
    let sddl: Vec<u16> = SDDL.encode_utf16().chain(Some(0)).collect();
    let mut sd = null_mut();
    // SAFETY: a NUL-terminated string; the descriptor is freed below.
    let made = unsafe {
      ConvertStringSecurityDescriptorToSecurityDescriptorW(
        sddl.as_ptr(),
        SDDL_REVISION_1,
        &mut sd,
        null_mut(),
      )
    };
    if made == 0 {
      return Err(io::Error::last_os_error());
    }

    let size = size_of::<SECURITY_ATTRIBUTES>() as u32;
    let security =
      SECURITY_ATTRIBUTES { nLength: size, lpSecurityDescriptor: sd, bInheritHandle: 0 };
    // SAFETY: a NUL-terminated name and a valid descriptor, which the pipe copies.
    let pipe = unsafe { CreateNamedPipeW(name.as_ptr(), open, mode, 1, 0, 0, 0, &security) };
    let error = io::Error::last_os_error();
    // SAFETY: the descriptor made above, used no more.
    unsafe { LocalFree(sd) };
    if pipe == INVALID_HANDLE_VALUE {
      return Err(error);
    }

    // SAFETY: the handle just made, owned from here.
    let pipe = Arc::new(unsafe { OwnedHandle::from_raw_handle(pipe) });
    Ok(Self { pipe, path, stop: Arc::default(), thread: None, _lock: lock })
  }

  /// Calls `on` with each line, on a thread of its own; once.
  pub fn serve(&mut self, on: impl Fn(&str) + Send + 'static) -> io::Result<()> {
    if self.thread.is_some() {
      return Ok(());
    }

    let (pipe, stop) = (self.pipe.clone(), self.stop.clone());
    let serve = move || {
      let pipe = pipe.as_raw_handle();
      while !stop.load(Ordering::Acquire) {
        // SAFETY: our pipe; blocks until a sender connects.
        let connected = unsafe { ConnectNamedPipe(pipe, null_mut()) } != 0
          || io::Error::last_os_error().raw_os_error() == Some(ERROR_PIPE_CONNECTED as i32);
        if connected
          && !stop.load(Ordering::Acquire)
          && let Some(line) = read_line(pipe)
        {
          on(&line);
        }

        // SAFETY: our pipe; the sender's reads end.
        unsafe { DisconnectNamedPipe(pipe) };
      }
    };

    self.thread = Some(thread::Builder::new().name("instance".into()).spawn(serve)?);
    Ok(())
  }
}

/// The sender's line, read as it comes within [READ_TIMEOUT] ([read_polled]).
fn read_line(pipe: HANDLE) -> Option<String> {
  let read = |buf: &mut [u8]| {
    let mut n = 0;
    // SAFETY: at most `buf.len()` bytes, no more than are waiting: it does not block.
    let ok = unsafe { ReadFile(pipe, buf.as_mut_ptr(), buf.len() as u32, &mut n, null_mut()) };
    (ok != 0).then_some(n as usize)
  };
  read_polled(|| available(pipe), read, READ_TIMEOUT)
}

/// The bytes waiting in `pipe`; `None` once the other end is gone.
fn available(pipe: HANDLE) -> Option<usize> {
  let mut n = 0;
  // SAFETY: a pipe; only the count is asked for.
  let ok = unsafe { PeekNamedPipe(pipe, null_mut(), 0, null_mut(), &mut n, null_mut()) };
  (ok != 0).then_some(n as usize)
}

/// One pipe per folder, by its path's hash, so a test's folder never meets the app's.
fn pipe_path(dir: &Path) -> PathBuf {
  let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_owned());
  let id = hash(dir.as_os_str().as_encoded_bytes());
  PathBuf::from(format!(r"\\.\pipe\nimbo-{id:016x}"))
}

/// Connects, waiting until `deadline` while the pipe serves another sender. The instance may only
/// identify the sender, not act as it: a pipe of that name made by another process gets no rights
/// of ours.
fn client(path: &Path, deadline: Instant) -> io::Result<File> {
  loop {
    let mut options = File::options();
    options.read(true).write(true).security_qos_flags(SECURITY_IDENTIFICATION);
    match options.open(path) {
      Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) && Instant::now() < deadline => {
        thread::sleep(Duration::from_millis(10));
      }
      result => return result,
    }
  }
}

/// Sends `line` to the instance for `dir`, back once it has read it, or `TimedOut` at `deadline`.
pub fn send(dir: &Path, line: &str, deadline: Instant) -> io::Result<()> {
  // The instance runs in the background: its window comes to the front only if the process in
  // front (this one) lets it.
  // SAFETY: no preconditions.
  unsafe { AllowSetForegroundWindow(ASFW_ANY) };
  let mut pipe = client(&pipe_path(dir), deadline)?;
  pipe.write_all(line.as_bytes())?;
  // The instance hangs up once it has the line, within its READ_TIMEOUT.
  while available(pipe.as_raw_handle()).is_some() {
    if Instant::now() >= deadline {
      return Err(io::Error::new(io::ErrorKind::TimedOut, "the app did not take the command"));
    }

    thread::sleep(POLL);
  }

  Ok(())
}
