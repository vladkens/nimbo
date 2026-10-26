// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Writes the files the app keeps in the user's folders on Linux (the launcher entry and icon,
//! the autostart entry, the portal's restore token): mode 0644 in folders it makes 0755, whole or
//! not at all (a temporary file beside it, renamed over it), and never by root into a folder of
//! another user's (`sudo` keeps `HOME`), which would leave that user files they can't change.
//! Every Unix builds it for its tests ([crate::desktop]'s run on macOS too).

use std::fs::{OpenOptions, Permissions, create_dir_all, remove_file, rename, set_permissions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

unsafe extern "C" {
  fn geteuid() -> u32;
}

/// Writes `bytes` to `path`, making its folders: `Err` with why it did not.
pub(crate) fn file(path: &Path, bytes: &[u8]) -> Result<(), String> {
  file_as(unsafe { geteuid() }, path, bytes)
}

/// [file] by the effective user `uid`.
fn file_as(uid: u32, path: &Path, bytes: &[u8]) -> Result<(), String> {
  let err = |e: std::io::Error| format!("{}: {e}", path.display());
  let (dir, name) = (path.parent().unwrap_or(Path::new(".")), path.file_name().unwrap_or_default());
  // The nearest folder there is, and those above it: root writes only where root owns them all.
  let theirs = dir.ancestors().find(|a| a.metadata().is_ok_and(|m| m.uid() != 0));
  if let Some(home) = theirs.filter(|_| uid == 0) {
    return Err(format!(
      "{}: not written as root in {}, another user's",
      path.display(),
      home.display()
    ));
  }

  let made: Vec<&Path> = dir.ancestors().take_while(|a| !a.exists()).collect();
  create_dir_all(dir).map_err(err)?;
  for d in made {
    set_permissions(d, Permissions::from_mode(0o755)).map_err(err)?;
  }

  // One of its own for each write, made new: never a file (or link) that was there.
  static NEXT: AtomicU32 = AtomicU32::new(0);
  let n = NEXT.fetch_add(1, Ordering::Relaxed);
  let tmp = dir.join(format!(".{}.{}-{n}.tmp", name.to_string_lossy(), std::process::id()));
  let mut f =
    OpenOptions::new().write(true).create_new(true).mode(0o644).open(&tmp).map_err(err)?;
  // The mode the umask may have narrowed, set again.
  let done = (f.write_all(bytes).and_then(|_| f.sync_all()))
    .and_then(|_| set_permissions(&tmp, Permissions::from_mode(0o644)))
    .and_then(|_| rename(&tmp, path));
  if done.is_err() {
    _ = remove_file(&tmp);
  }

  done.map_err(err)
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::*;

  /// A folder of its own under the system's temp one, emptied first.
  fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nimbo-write-{}-{name}", std::process::id()));
    _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).unwrap().mode() & 0o777
  }

  #[test]
  fn files_0644_in_folders_0755() {
    let dir = temp("modes");
    let path = dir.join("a/b/entry.desktop");
    file_as(1000, &path, b"one").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"one");
    assert_eq!([mode(&path), mode(&dir.join("a")), mode(&dir.join("a/b"))], [0o644, 0o755, 0o755]);
    _ = std::fs::remove_dir_all(&dir);
  }

  /// A rewrite is a new file renamed over the old one, nothing left beside it.
  #[test]
  fn rewrites_whole() {
    let dir = temp("rewrite");
    let path = dir.join("token");
    file_as(1000, &path, b"first").unwrap();
    let before = std::fs::metadata(&path).unwrap().ino();
    file_as(1000, &path, b"second").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"second");
    assert_ne!(std::fs::metadata(&path).unwrap().ino(), before, "renamed over");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    _ = std::fs::remove_dir_all(&dir);
  }

  /// Root writes nowhere under a folder another user owns, as the test's own temp folder is
  /// (unless the tests run as root).
  #[test]
  fn root_skips_another_users_home() {
    let dir = temp("root");
    let path = dir.join("autostart/app.desktop");
    if std::fs::metadata(&dir).unwrap().uid() != 0 {
      let e = file_as(0, &path, b"x").unwrap_err();
      assert!(e.contains("not written as root"), "{e}");
      assert!(!dir.join("autostart").exists());
    }

    file_as(1000, &path, b"x").unwrap();
    _ = std::fs::remove_dir_all(&dir);
  }
}
