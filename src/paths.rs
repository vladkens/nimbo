// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Where an app keeps its files, per user, by its id ([crate::AppInfo::id], reverse DNS), as
//! each OS lays them out:
//!
//! | | macOS | Windows | Linux |
//! |---|---|---|---|
//! | [data_dir] | `~/Library/Application Support/<id>` | `%LOCALAPPDATA%\<id>` | `$XDG_DATA_HOME/<id>` (`~/.local/share`) |
//! | [config_dir] | `~/Library/Application Support/<id>` | `%APPDATA%\<id>` | `$XDG_CONFIG_HOME/<id>` (`~/.config`) |
//! | [cache_dir] | `~/Library/Caches/<id>` | `%LOCALAPPDATA%\<id>\cache` | `$XDG_CACHE_HOME/<id>` (`~/.cache`) |
//!
//! Only the environment is read: nothing is created.

use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Os {
  Mac,
  Windows,
  Linux,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
  Data,
  Config,
  Cache,
}

const OS: Os = match () {
  _ if cfg!(target_os = "macos") => Os::Mac,
  _ if cfg!(windows) => Os::Windows,
  _ => Os::Linux,
};

/// The app's data: databases, downloads, what the user would miss.
pub fn data_dir(id: &str) -> PathBuf {
  dir(OS, Kind::Data, id, &|k| std::env::var_os(k))
}

/// The app's settings.
pub fn config_dir(id: &str) -> PathBuf {
  dir(OS, Kind::Config, id, &|k| std::env::var_os(k))
}

/// What the app can make again; the OS may clear it.
pub fn cache_dir(id: &str) -> PathBuf {
  dir(OS, Kind::Cache, id, &|k| std::env::var_os(k))
}

fn dir(os: Os, kind: Kind, id: &str, env: &dyn Fn(&str) -> Option<OsString>) -> PathBuf {
  // Set and absolute, as the XDG spec asks of its variables.
  let var = |k: &str| env(k).map(PathBuf::from).filter(|p| p.is_absolute());
  let home = var(if os == Os::Windows { "USERPROFILE" } else { "HOME" });
  let home = || home.clone().unwrap_or_else(|| PathBuf::from("/"));
  match (os, kind) {
    (Os::Mac, Kind::Data | Kind::Config) => home().join("Library/Application Support").join(id),
    (Os::Mac, Kind::Cache) => home().join("Library/Caches").join(id),
    (Os::Windows, Kind::Config) => {
      var("APPDATA").unwrap_or_else(|| home().join("AppData").join("Roaming")).join(id)
    }
    (Os::Windows, kind) => {
      let local = var("LOCALAPPDATA").unwrap_or_else(|| home().join("AppData").join("Local"));
      match kind {
        Kind::Cache => local.join(id).join("cache"),
        _ => local.join(id),
      }
    }
    (Os::Linux, kind) => {
      let (name, fallback) = match kind {
        Kind::Data => ("XDG_DATA_HOME", ".local/share"),
        Kind::Config => ("XDG_CONFIG_HOME", ".config"),
        Kind::Cache => ("XDG_CACHE_HOME", ".cache"),
      };

      var(name).unwrap_or_else(|| home().join(fallback)).join(id)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Unix fixtures: `/Users/me` is a relative path on Windows.
  #[cfg(unix)]
  #[test]
  fn per_os_and_kind() {
    let env = |vars: &'static [(&'static str, &'static str)]| {
      move |k: &str| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
    };
    let mac = env(&[("HOME", "/Users/me")]);
    let id = "com.example.app";
    let got = |os, kind, e: &dyn Fn(&str) -> Option<OsString>| dir(os, kind, id, e);
    let p = PathBuf::from;
    assert_eq!(
      got(Os::Mac, Kind::Data, &mac),
      p("/Users/me/Library/Application Support/com.example.app")
    );
    assert_eq!(got(Os::Mac, Kind::Config, &mac), got(Os::Mac, Kind::Data, &mac));
    assert_eq!(got(Os::Mac, Kind::Cache, &mac), p("/Users/me/Library/Caches/com.example.app"));

    let linux =
      env(&[("HOME", "/home/me"), ("XDG_CONFIG_HOME", "/cfg"), ("XDG_CACHE_HOME", "rel")]);
    assert_eq!(got(Os::Linux, Kind::Data, &linux), p("/home/me/.local/share/com.example.app"));
    assert_eq!(got(Os::Linux, Kind::Config, &linux), p("/cfg/com.example.app"));
    assert_eq!(
      got(Os::Linux, Kind::Cache, &linux),
      p("/home/me/.cache/com.example.app"),
      "relative: ignored"
    );

    // Windows paths parse as relative on Unix: only the fallback from no variables is checked.
    let none = env(&[]);
    assert_eq!(got(Os::Windows, Kind::Data, &none), p("/AppData/Local/com.example.app"));
    assert_eq!(got(Os::Windows, Kind::Config, &none), p("/AppData/Roaming/com.example.app"));
    assert_eq!(got(Os::Windows, Kind::Cache, &none), p("/AppData/Local/com.example.app/cache"));
    assert!(data_dir(id).ends_with(id));
  }

  #[cfg(windows)]
  #[test]
  fn windows_by_its_variables() {
    let env = |k: &str| match k {
      "APPDATA" => Some(OsString::from(r"C:\Users\me\AppData\Roaming")),
      "LOCALAPPDATA" => Some(OsString::from(r"D:\Local")),
      _ => None,
    };
    let id = "com.example.app";
    let config = dir(Os::Windows, Kind::Config, id, &env);
    assert_eq!(config, PathBuf::from(r"C:\Users\me\AppData\Roaming\com.example.app"));
    assert_eq!(
      dir(Os::Windows, Kind::Cache, id, &env),
      PathBuf::from(r"D:\Local\com.example.app\cache")
    );
  }
}
