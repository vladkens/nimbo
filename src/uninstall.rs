// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Uninstalling an app: [system] undoes what nimbo set up for it in the OS, [remove] deletes its
//! folders and the app it runs from ([app_path]).
//!
//! | | what [system] undoes | [app_path] |
//! |---|---|---|
//! | macOS | launch at login, the app's permissions (`tccutil reset All <id>`), its defaults (`defaults delete <id>`, then the empty `~/Library/Preferences/<id>.plist` cfprefsd leaves) | the `.app` bundle |
//! | Windows | launch at login | none: Windows can't delete a running exe, its folder is left to the person |
//! | Linux | launch at login, the launcher entry ([crate::desktop]), the remote desktop portal's approval (`remote-desktop.token` in [crate::paths::config_dir]) | the AppImage (`$APPIMAGE`) |
//!
//! The app's own files are the app's to pick: an uninstall quits the running instance first
//! ([crate::instance::quit_and_wait]), then runs [system], then removes the app's folders
//! ([crate::paths]: the [outermost] of them, as one may hold another) and the app last, as the
//! uninstall may run from it. It runs in a process of its own (the app's `--uninstall` command,
//! say), not in the running app: there [crate::instance::quit_and_wait] would ask the app's own
//! main thread, blocked in the wait, to quit, and time out.
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use nimbo::{AppInfo, Presence, instance, paths, uninstall};
//!
//! const INFO: AppInfo =
//!   AppInfo { id: "com.example.app", name: "App", presence: Presence::Regular };
//! instance::quit_and_wait(&INFO, Duration::from_secs(5))?;
//! for step in uninstall::system(&INFO) {
//!   println!("{step}");
//! }
//!
//! let dirs = [paths::data_dir(INFO.id), paths::config_dir(INFO.id), paths::cache_dir(INFO.id)];
//! for path in uninstall::outermost(dirs).into_iter().chain(uninstall::app_path()) {
//!   if let Err(e) = uninstall::remove(&path) {
//!     println!("could not remove {}: {e}", path.display());
//!   }
//! }
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! Ported from wavo-app's `src/cli.rs` (vladkens, MIT, <https://github.com/vladkens/wavo-app>).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{fmt, fs, io, thread};

use crate::paths::{OS, Os};
use crate::platform::uninstall as imp;
use crate::{AppInfo, autostart};

/// One thing [system] undid, or why it couldn't; shown as `<what>` or `<what> failed: <why>`.
#[derive(Debug)]
pub struct Step {
  /// What was undone: `launch at login off`.
  pub what: &'static str,
  pub result: Result<(), String>,
}

impl fmt::Display for Step {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match &self.result {
      Ok(()) => f.write_str(self.what),
      Err(e) => write!(f, "{} failed: {e}", self.what),
    }
  }
}

/// Undoes what nimbo set up for the app in the OS (the table above), each step tried whatever
/// the one before did. Neither quits the app nor touches its folders.
pub fn system(app: &AppInfo) -> Vec<Step> {
  let off = autostart::state(app).and_then(|s| match s.wanted() {
    true => autostart::set(app, false),
    false => Ok(s),
  });
  let mut steps = vec![Step { what: "launch at login off", result: off.map(drop) }];
  steps.extend(imp::steps(app));
  steps
}

/// The app the running executable belongs to: macOS's `.app` bundle, Linux's AppImage; `None` for
/// a bare binary (a build) and on Windows, where a running exe can't be deleted: an uninstaller
/// tells the person to delete its folder.
pub fn app_path() -> Option<PathBuf> {
  app_of(OS, &std::env::current_exe().ok()?, &|k| std::env::var_os(k))
}

fn app_of(os: Os, exe: &Path, env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
  match os {
    // By its layout: `<name>.app/Contents/MacOS/<exe>`.
    Os::Mac => {
      let app = exe.parent().filter(|p| p.ends_with("Contents/MacOS"))?.parent()?.parent()?;
      app.extension().is_some_and(|e| e == "app").then(|| app.to_owned())
    }
    // The AppImage's runtime sets both; one inherited from another AppImage (a terminal that is
    // one) is not where this runs.
    Os::Linux => {
      let (image, mount) = (PathBuf::from(env("APPIMAGE")?), PathBuf::from(env("APPDIR")?));
      (exe.starts_with(mount) && image.is_file()).then_some(image)
    }
    Os::Windows => None,
  }
}

/// `paths` but those inside another or given before (Windows' cache folder is in its data
/// folder, macOS' config folder is its data folder), in their order.
pub fn outermost(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
  let paths: Vec<PathBuf> = paths.into_iter().collect();
  let inside = |i: usize, p: &PathBuf| {
    let mut others = paths.iter().enumerate().filter(|&(j, _)| j != i);
    others.any(|(j, q)| p.starts_with(q) && (p != q || j < i))
  };
  let keep = paths.iter().enumerate().filter(|&(i, p)| !inside(i, p));
  keep.map(|(_, p)| p.clone()).collect()
}

/// Deletes `path`, a folder with all in it or a file; `Ok` when it's not there. Tried again for
/// up to 5 s while the error may pass: macOS can leave entries out of a folder's listing while
/// it is emptied ("not empty"); on Windows the app that just quit may hold its files open a
/// moment after its lock is free (a sharing or lock violation, access denied while deleting).
pub fn remove(path: &Path) -> io::Result<()> {
  let deadline = Instant::now() + Duration::from_secs(5);
  loop {
    let done = match path.is_dir() {
      true => fs::remove_dir_all(path),
      false => fs::remove_file(path),
    };

    match done {
      Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
      Err(e) if imp::busy(&e) && Instant::now() < deadline => {
        thread::sleep(Duration::from_millis(50))
      }
      done => return done,
    }
  }
}

/// Deletes the file at `path` for a [Step]; `Ok` when it's not there.
#[cfg(not(windows))]
pub(crate) fn remove_file(path: &Path) -> Result<(), String> {
  match fs::remove_file(path) {
    Err(e) if e.kind() != io::ErrorKind::NotFound => Err(format!("{}: {e}", path.display())),
    _ => Ok(()),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn outermost_drops_paths_inside_others() {
    let paths = |s: &[&str]| s.iter().map(PathBuf::from).collect::<Vec<_>>();
    let given = paths(&["/a/data/cache", "/a/data", "/a/config", "/a/data", "/a/dat", "/b"]);
    assert_eq!(outermost(given), paths(&["/a/data", "/a/config", "/a/dat", "/b"]));
    assert_eq!(outermost(paths(&["/a", "/a/b", "/"])), paths(&["/"]));
    assert!(outermost(Vec::new()).is_empty());
  }

  #[test]
  fn the_app_is_the_bundle_or_the_appimage() {
    let none = |_: &str| None;
    let mac = |exe: &str| app_of(Os::Mac, Path::new(exe), &none);
    assert_eq!(mac("/Applications/A.app/Contents/MacOS/a"), Some("/Applications/A.app".into()));
    assert_eq!(mac("/Applications/A/Contents/MacOS/a"), None);
    assert_eq!(mac("/Users/me/a/target/debug/a"), None);
    assert_eq!(mac("/A.app/Contents/Resources/a"), None);
    assert_eq!(app_of(Os::Windows, Path::new("C:/A/a.exe"), &none), None);

    let dir = std::env::temp_dir().join(format!("nimbo-app-path-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let image = dir.join("A.AppImage");
    fs::write(&image, "").unwrap();
    let linux = |exe: &str, image: &Path| {
      let env = |k: &str| match k {
        "APPIMAGE" => Some(image.into()),
        "APPDIR" => Some("/tmp/.mount_A1".into()),
        _ => None,
      };
      app_of(Os::Linux, Path::new(exe), &env)
    };
    assert_eq!(linux("/tmp/.mount_A1/usr/bin/a", &image), Some(image.clone()));
    // Another AppImage's variables, inherited; one that is gone.
    assert_eq!(linux("/home/me/bin/a", &image), None);
    assert_eq!(linux("/tmp/.mount_A1/usr/bin/a", &dir.join("gone")), None);
    assert_eq!(app_of(Os::Linux, Path::new("/tmp/.mount_A1/usr/bin/a"), &none), None);
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn remove_takes_a_tree_a_file_and_nothing() {
    let dir = std::env::temp_dir().join(format!("nimbo-remove-{}", std::process::id()));
    fs::create_dir_all(dir.join("a/b/c")).unwrap();
    fs::write(dir.join("a/b/c/d.txt"), "d").unwrap();
    fs::write(dir.join("a/e.txt"), "e").unwrap();
    fs::write(dir.join("f.txt"), "f").unwrap();
    remove(&dir.join("f.txt")).unwrap();
    assert!(!dir.join("f.txt").exists());
    remove(&dir.join("a")).unwrap();
    assert!(!dir.join("a").exists());
    remove(&dir.join("a")).unwrap();
    remove(&dir).unwrap();
    assert!(!dir.exists());
  }

  #[test]
  fn steps_read_as_what_was_done() {
    let done = Step { what: "launch at login off", result: Ok(()) };
    let failed = Step { what: "permissions reset", result: Err("no such app".into()) };
    assert_eq!(done.to_string(), "launch at login off");
    assert_eq!(failed.to_string(), "permissions reset failed: no such app");
  }
}
