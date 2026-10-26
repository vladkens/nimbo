// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! One instance on Unix ([crate::instance]), macOS's too: the lock file, and a Unix socket in the
//! user's runtime folder that takes one line per connection. Ported from wavo-app's `src/ctl.rs`
//! (vladkens, MIT, <https://github.com/vladkens/wavo-app>).

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

/// One socket per folder, by its device and inode (a test's folder never meets the app's), in
/// the user's runtime folder (Linux `$XDG_RUNTIME_DIR`, macOS `$TMPDIR`, both the user's alone);
/// in `/tmp` without it or where the path would not fit `sun_path`'s 104 bytes (macOS).
pub(crate) fn socket_path(dir: &Path) -> io::Result<PathBuf> {
  let meta = fs::metadata(dir)?;
  let id = hash(&[meta.dev().to_le_bytes(), meta.ino().to_le_bytes()].concat());
  let name = format!("nimbo-{id:016x}.sock");
  let var = if cfg!(target_os = "macos") { "TMPDIR" } else { "XDG_RUNTIME_DIR" };
  let base = std::env::var_os(var).map(PathBuf::from).filter(|b| b.is_absolute());
  let path = base.map(|b| b.join(&name)).filter(|p| p.as_os_str().len() < 104);
  Ok(path.unwrap_or_else(|| Path::new("/tmp").join(name)))
}

/// Sends `line` to the instance for `dir`.
pub fn send(dir: &Path, line: &str) -> io::Result<()> {
  UnixStream::connect(socket_path(dir)?)?.write_all(line.as_bytes())
}
