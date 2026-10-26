// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! One instance on Unix ([crate::instance]), macOS's too: the lock file, and a Unix socket in the
//! user's runtime folder that takes one line per connection. Ported from wavo-app's `src/ctl.rs`
//! (vladkens, MIT, <https://github.com/vladkens/wavo-app>).

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, ErrorKind, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::instance::{MAX_LINE, READ_TIMEOUT, hash};

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

        let Ok(stream) = stream else { continue };
        _ = stream.set_read_timeout(Some(READ_TIMEOUT));
        let mut line = String::new();
        if BufReader::new(stream.take(MAX_LINE as u64)).read_line(&mut line).is_ok() {
          on(&line);
        }
      }
    };

    std::thread::Builder::new().name("instance".into()).spawn(serve).map(drop)
  }
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
/// it. A line this short never blocks: no `deadline`.
pub fn send(dir: &Path, line: &str, _deadline: Instant) -> io::Result<()> {
  let name = socket_name(dir)?;
  let fallback = || UnixStream::connect(fallback(false)?.join(&name));
  let mut stream = match runtime(&name).map(UnixStream::connect) {
    Some(Ok(stream)) => stream,
    Some(Err(e)) => fallback().map_err(|_| e)?,
    None => fallback()?,
  };

  stream.write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
  use super::*;

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
