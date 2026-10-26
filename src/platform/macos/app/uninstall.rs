// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What [crate::uninstall::system] undoes on macOS besides launch at login: the app's permissions
//! (TCC) and its defaults. Ported from wavo-app's `src/cli.rs` (vladkens, MIT).

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::AppInfo;
use crate::uninstall::{Step, remove_file};

pub fn steps(app: &AppInfo) -> Vec<Step> {
  let home = std::env::var_os("HOME").map(PathBuf::from);
  steps_with(app.id, home.as_deref(), run)
}

/// The steps for the app `id`, each command run by `run` (its path and arguments), the defaults'
/// file under `home`.
fn steps_with(
  id: &str,
  home: Option<&Path>,
  mut run: impl FnMut(&str, &[&str]) -> Result<(), String>,
) -> Vec<Step> {
  let permissions = run("/usr/bin/tccutil", &["reset", "All", id]);
  // Through cfprefsd, which caches them (the open and save panels' sizes and places); an app
  // that kept none has none to delete.
  let defaults = match run("/usr/bin/defaults", &["delete", id]) {
    Err(e) if e.contains("not found") => Ok(()),
    done => done,
  };
  // cfprefsd leaves the file behind, empty.
  let plist = home.map(|h| h.join(format!("Library/Preferences/{id}.plist")));
  let defaults = defaults.and(plist.map_or(Ok(()), |p| remove_file(&p)));
  vec![
    Step { what: "permissions reset", result: permissions },
    Step { what: "defaults removed", result: defaults },
  ]
}

/// Runs `cmd` to its end: `Err` with what it wrote to stderr when it failed.
fn run(cmd: &str, args: &[&str]) -> Result<(), String> {
  let out = Command::new(cmd).args(args).output().map_err(|e| format!("{cmd}: {e}"))?;
  let err = String::from_utf8_lossy(&out.stderr);
  match out.status.success() {
    true => Ok(()),
    false if err.trim().is_empty() => Err(format!("{cmd}: {}", out.status)),
    false => Err(format!("{cmd}: {}", err.trim())),
  }
}

/// A folder not empty yet: macOS can leave entries out of a folder's listing while it is
/// emptied (a delete once stopped "not empty" with 51 folders never visited).
pub fn busy(e: &io::Error) -> bool {
  e.kind() == io::ErrorKind::DirectoryNotEmpty
}

#[cfg(test)]
mod tests {
  use std::fs;

  use super::*;

  /// The commands, and the leftover plist gone; defaults the app never kept are no failure.
  #[test]
  fn permissions_and_defaults_are_reset() {
    let home = std::env::temp_dir().join(format!("nimbo-uninstall-{}", std::process::id()));
    let prefs = home.join("Library/Preferences");
    fs::create_dir_all(&prefs).unwrap();
    fs::write(prefs.join("com.example.a.plist"), "").unwrap();
    let mut ran = Vec::new();
    let mut run = |cmd: &str, args: &[&str]| {
      ran.push(format!("{cmd} {}", args.join(" ")));
      match cmd {
        "/usr/bin/defaults" => Err("Error: Domain 'com.example.a' not found.".into()),
        _ => Ok(()),
      }
    };
    let steps = steps_with("com.example.a", Some(&home), &mut run);
    let shown: Vec<String> = steps.iter().map(Step::to_string).collect();
    assert_eq!(shown, ["permissions reset", "defaults removed"]);
    assert_eq!(
      ran,
      ["/usr/bin/tccutil reset All com.example.a", "/usr/bin/defaults delete com.example.a"]
    );
    assert!(!prefs.join("com.example.a.plist").exists());

    let fail = |_: &str, _: &[&str]| Err("tccutil: No such bundle identifier".to_string());
    let steps = steps_with("com.example.a", Some(&home), fail);
    assert!(steps.iter().all(|s| s.result.is_err()));
    fs::remove_dir_all(&home).unwrap();
  }
}
