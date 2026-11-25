// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Writes the files the app keeps in the user's folders on Linux (the launcher entry and icon,
//! the autostart entry, the portal's restore token, the tray's icons: [tray_icon]): mode 0644 in
//! folders it makes 0755, whole or not at all (a temporary file beside it, renamed over it), and
//! never by root into a folder of another user's (`sudo` keeps `HOME`), which would leave that user
//! files they can't change. Every Unix builds it for its tests ([crate::desktop]'s run on macOS
//! too).

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

/// The start of the names of app `id`'s tray icons ([tray_icon]), with no dash: KIconLoader
/// looks for a name's shorter forms (up to a dash) in each theme first, and `<id>` alone is the
/// launcher's icon.
fn tray_icons(id: &str) -> String {
  format!("{}.tray", id.replace('-', "_"))
}

/// The file `name` is one of app `id`'s tray icons: its start, 16 hex digits, `-symbolic.svg`.
pub(crate) fn is_tray_icon(name: &str, id: &str) -> bool {
  let hash = name.strip_prefix(&tray_icons(id)).and_then(|n| n.strip_suffix("-symbolic.svg"));
  hash.is_some_and(|h| h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Writes a tray icon's `svg` for app `id` at the top of the data folder `data`'s `icons` (no
/// theme's: KIconLoader and GTK look there each time, in a theme only in the folders there were
/// as they started), named by its content, as panels keep an icon by its name, and `-symbolic`
/// for GNOME to colour it; with `clean` (the run's first) removes the app's others, as a host
/// may still ask for one of this run's. Its name, as `IconName` gives it.
pub(crate) fn tray_icon(data: &Path, id: &str, svg: &str, clean: bool) -> Result<String, String> {
  use std::hash::{Hash, Hasher};
  let mut hash = std::hash::DefaultHasher::new();
  svg.hash(&mut hash);
  let (prefix, icons) = (tray_icons(id), data.join("icons"));
  let name = format!("{prefix}{:016x}-symbolic", hash.finish());
  let path = icons.join(format!("{name}.svg"));
  if !path.exists() {
    file(&path, svg.as_bytes())?;
  }

  for e in std::fs::read_dir(&icons).into_iter().flatten().flatten().filter(|_| clean) {
    let n = e.file_name().to_string_lossy().into_owned();
    if is_tray_icon(&n, id) && n != format!("{name}.svg") {
      _ = std::fs::remove_file(e.path());
    }
  }

  Ok(name)
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::*;

  /// A tray icon is named by its content, with one dash; the run's first removes the app's
  /// others, and no other app's.
  #[test]
  fn tray_icons_by_their_content() {
    let data = std::env::temp_dir().join(format!("nimbo-tray-icons-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&data);
    let id = "com.example.my-app";
    let a = tray_icon(&data, id, "<svg>a</svg>", false).unwrap();
    assert!(a.starts_with("com.example.my_app.tray") && a.ends_with("-symbolic"), "{a}");
    assert_eq!(a.matches('-').count(), 1);
    let file = |n: &str| data.join(format!("icons/{n}.svg"));
    assert_eq!(std::fs::read_to_string(file(&a)).unwrap(), "<svg>a</svg>");
    assert_eq!(tray_icon(&data, id, "<svg>a</svg>", false), Ok(a.clone()));

    let b = tray_icon(&data, id, "<svg>b</svg>", false).unwrap();
    assert!(file(&a).exists() && file(&b).exists(), "the run's are kept");
    let other = tray_icon(&data, "com.example.my-app.tray", "<svg>o</svg>", false).unwrap();
    std::fs::write(data.join("icons/other.svg"), "").unwrap();
    let c = tray_icon(&data, id, "<svg>c</svg>", true).unwrap();
    assert!(!file(&a).exists() && !file(&b).exists() && file(&c).exists());
    assert!(file(&other).exists() && file("other").exists(), "only the app's go");
    assert!(!is_tray_icon("com.example.my_app.tray0123-symbolic.svg", id));
    _ = std::fs::remove_dir_all(&data);
  }

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
