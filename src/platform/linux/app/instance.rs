// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! One instance on Unix ([crate::instance]), macOS's too: the lock file, and a Unix socket in the
//! user's runtime folder that takes one line per connection. Ported from wavo-app's `src/ctl.rs`
//! (vladkens, MIT, <https://github.com/vladkens/wavo-app>).

use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Instant;

use crate::instance::{MAX_LINE, READ_TIMEOUT, hash};

/// Whether the process `pid` runs: `kill(pid, 0)`, which sends nothing. Another user's is not ours.
pub fn alive(pid: u32) -> bool {
  unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
  }

  // SAFETY: signal 0 only checks; a pid of 0 or past i32 would name a group, never asked.
  i32::try_from(pid).is_ok_and(|pid| pid > 0 && unsafe { kill(pid, 0) } == 0)
}

/// The lock file at `path`, this user's alone.
pub fn lock_file(path: &Path) -> io::Result<File> {
  File::options().create(true).append(true).mode(0o600).open(path)
}

/// The lock and the socket, both let go on drop.
pub struct Server {
  /// Until [Server::serve] moves it to its thread.
  listener: Option<UnixListener>,
  path: PathBuf,
  stop: Arc<AtomicBool>,
  _lock: File,
}

impl Drop for Server {
  /// A connection wakes the thread from its wait, to end; the socket goes before the lock.
  fn drop(&mut self) {
    self.stop.store(true, Ordering::Release);
    if self.listener.is_none() {
      _ = UnixStream::connect(&self.path);
    }

    _ = fs::remove_file(&self.path);
  }
}

impl Server {
  /// The lock is ours: a socket file there is a dead instance's.
  pub fn bind(dir: &Path, lock: File) -> io::Result<Self> {
    let path = socket_path(dir)?;
    _ = fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    Ok(Self { listener: Some(listener), path, stop: Arc::default(), _lock: lock })
  }

  /// Calls `on` with each line, on a thread of its own; once.
  pub fn serve(&mut self, on: impl Fn(&str) + Send + 'static) -> io::Result<()> {
    let Some(listener) = self.listener.take() else { return Ok(()) };
    let stop = self.stop.clone();
    let serve = move || {
      for stream in listener.incoming() {
        if stop.load(Ordering::Acquire) {
          break;
        }

        if let Some(line) = stream.ok().and_then(read_line) {
          on(&line);
        }
      }
    };

    std::thread::Builder::new().name("instance".into()).spawn(serve).map(drop)
  }
}

/// The sender's line as it comes: until a line break, [MAX_LINE] bytes or its end, within
/// [READ_TIMEOUT] in all, so a sender that trickles bytes doesn't hold the instance.
fn read_line(mut stream: UnixStream) -> Option<String> {
  let (deadline, mut line) = (Instant::now() + READ_TIMEOUT, Vec::new());
  while !line.contains(&b'\n') && line.len() < MAX_LINE {
    let left = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero())?;
    // macOS refuses it (EINVAL) once the sender has hung up: then the read doesn't wait.
    _ = stream.set_read_timeout(Some(left));
    let mut buf = [0u8; MAX_LINE];
    match stream.read(&mut buf[..MAX_LINE - line.len()]).ok()? {
      0 => break,
      n => line.extend_from_slice(&buf[..n]),
    }
  }

  // The first line alone: what comes after is not the command's.
  line.truncate(line.iter().position(|&b| b == b'\n').map_or(line.len(), |i| i + 1));
  String::from_utf8(line).ok()
}

unsafe extern "C" {
  /// The user this process runs as; it can't fail.
  safe fn getuid() -> u32;
}

/// One socket per folder and user, by the folder's device and inode (a test's folder never meets
/// the app's), in the user's runtime folder (Linux `$XDG_RUNTIME_DIR`, macOS `$TMPDIR`, both the
/// user's alone); without it, or where the path would not fit `sun_path`'s 104 bytes (macOS), in
/// `/tmp/nimbo-<uid>` ([fallback]).
pub(crate) fn socket_path(dir: &Path) -> io::Result<PathBuf> {
  let name = socket_name(dir)?;
  runtime(&name).map_or_else(|| Ok(fallback(true)?.join(&name)), Ok)
}

fn socket_name(dir: &Path) -> io::Result<String> {
  let meta = fs::metadata(dir)?;
  let id = hash(&[meta.dev().to_le_bytes(), meta.ino().to_le_bytes()].concat());
  Ok(format!("nimbo-{}-{id:016x}.sock", getuid()))
}

fn runtime(name: &str) -> Option<PathBuf> {
  let var = if cfg!(target_os = "macos") { "TMPDIR" } else { "XDG_RUNTIME_DIR" };
  let base = std::env::var_os(var).map(PathBuf::from).filter(|b| b.is_absolute())?;
  Some(base.join(name)).filter(|p| p.as_os_str().len() < 104)
}

/// `/tmp/nimbo-<uid>` ([private]).
fn fallback(make: bool) -> io::Result<PathBuf> {
  private(PathBuf::from(format!("/tmp/nimbo-{}", getuid())), make)
}

/// The folder `dir`, made 0700 if `make`; refused unless it is a folder of this user's that no
/// one else may enter: another user could have made it first, to take the commands.
fn private(dir: PathBuf, make: bool) -> io::Result<PathBuf> {
  if make
    && let Err(e) = fs::DirBuilder::new().mode(0o700).create(&dir)
    && e.kind() != ErrorKind::AlreadyExists
  {
    return Err(e);
  }

  let meta = fs::symlink_metadata(&dir)?;
  if !meta.is_dir() || meta.uid() != getuid() || meta.mode() & 0o077 != 0 {
    let why = format!("{} is not this user's alone", dir.display());
    return Err(io::Error::new(ErrorKind::PermissionDenied, why));
  }

  Ok(dir)
}

/// Sends `line` to the instance for `dir`: in the runtime folder, or the fallback where the
/// instance had none (started from another session); the first one's error when neither takes
/// it. By `deadline`: a connection waits for room while the instance's queue is full (Linux), so
/// it runs on a thread of its own, left to end by itself past the deadline.
pub fn send(dir: &Path, line: &str, deadline: Instant) -> io::Result<()> {
  let name = socket_name(dir)?;
  let (tx, rx) = mpsc::channel();
  let connect = move || {
    let fallback = || UnixStream::connect(fallback(false)?.join(&name));
    _ = tx.send(match runtime(&name).map(UnixStream::connect) {
      Some(Ok(stream)) => Ok(stream),
      Some(Err(e)) => fallback().map_err(|_| e),
      None => fallback(),
    });
  };

  std::thread::Builder::new().name("instance-send".into()).spawn(connect)?;
  let left = deadline.saturating_duration_since(Instant::now());
  let late = |_| io::Error::new(ErrorKind::TimedOut, "the app did not take the connection");
  // A line this short fits the socket's buffer: the write doesn't wait.
  rx.recv_timeout(left).map_err(late)??.write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
  use super::*;

  unsafe extern "C" {
    safe fn listen(fd: i32, backlog: i32) -> i32;
  }

  /// A send gives up by its deadline while the instance's queue is full, where a connection
  /// waits for room (Linux; macOS refuses it).
  #[test]
  fn a_send_keeps_its_deadline() {
    use std::os::fd::AsRawFd;
    use std::time::Duration;

    let dir = std::env::temp_dir().join(format!("nimbo-full-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let server = Server::bind(&dir, lock_file(&dir.join("lock")).unwrap()).unwrap();
    // A queue of one, never accepted.
    assert_eq!(listen(server.listener.as_ref().unwrap().as_raw_fd(), 0), 0);
    for _ in 0..3 {
      let start = Instant::now();
      _ = send(&dir, "open\n", start + Duration::from_millis(200));
      assert!(start.elapsed() < Duration::from_secs(2));
    }

    drop(server);
    _ = fs::remove_dir_all(&dir);
  }

  /// The folder is made the user's alone; one others may enter, or a link, is refused.
  #[test]
  fn the_fallback_folder_is_the_users_alone() {
    let base = std::env::temp_dir().join(format!("nimbo-private-{}", std::process::id()));
    _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let dir = base.join("socks");
    assert_eq!(private(dir.clone(), true).unwrap(), dir);
    assert_eq!(fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);
    assert!(private(dir.clone(), false).is_ok(), "made before");

    let denied = |dir: &Path| private(dir.to_owned(), true).unwrap_err().kind();
    let link = base.join("link");
    std::os::unix::fs::symlink(&dir, &link).unwrap();
    assert_eq!(denied(&link), ErrorKind::PermissionDenied);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(denied(&dir), ErrorKind::PermissionDenied);
    assert_eq!(private(base.join("none"), false).unwrap_err().kind(), ErrorKind::NotFound);
    _ = fs::remove_dir_all(&base);
  }
}
