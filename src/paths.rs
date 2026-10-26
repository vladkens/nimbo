// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Where an app keeps its files, per user, by its id ([crate::AppInfo::id], reverse DNS), as
//! each OS lays them out:
//!
//! | | macOS | Windows | Linux |
//! |---|---|---|---|
//! | [data_dir] | `~/Library/Application Support/<id>` | `%LOCALAPPDATA%\<id>` | `$XDG_DATA_HOME/<id>` (`~/.local/share`) |
//! | [config_dir] | `~/Library/Application Support/<id>` | `%APPDATA%\<id>` | `$XDG_CONFIG_HOME/<id>` (`~/.config`) |
//! | [cache_dir] | `~/Library/Caches/<id>` | `%LOCALAPPDATA%\<id>\cache` | `$XDG_CACHE_HOME/<id>` (`~/.cache`) |
//! | [downloads] | `~/Downloads` | the Known Folder | `XDG_DOWNLOAD_DIR` in `user-dirs.dirs`, else `~/Downloads` |
//!
//! Nothing is created: the app's folders come from the environment alone, [downloads] reads
//! the user's settings too and is only a folder that exists. [display] writes a path as the user
//! reads it: `~/…` under the home folder on macOS and Linux.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

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

/// The user's Downloads folder, where an app saves what the user fetched, or `None` when there's
/// none on disk. macOS: `~/Downloads`. Windows: the Known Folder, wherever the user moved it.
/// Linux: `XDG_DOWNLOAD_DIR` from `$XDG_CONFIG_HOME/user-dirs.dirs` (`~/.config`), as
/// xdg-user-dirs names it in the session's language (`~/Загрузки`), else `~/Downloads`; none
/// where it is the home folder itself (xdg-user-dirs' way to turn a folder off).
pub fn downloads() -> Option<PathBuf> {
  #[cfg(windows)]
  let dir = crate::platform::downloads();
  #[cfg(not(windows))]
  let dir = downloads_in(OS, &|k| std::env::var_os(k), &|p| std::fs::read_to_string(p).ok());
  dir.filter(|d| d.is_dir())
}

/// `path` as the user reads it: under the home folder `~/…` on macOS and Linux (`~` itself for
/// it), as a shell writes it; whole on Windows, which has no such form.
pub fn display(path: &Path) -> String {
  display_in(OS, path, std::env::var_os("HOME").map(PathBuf::from).as_deref())
}

fn display_in(os: Os, path: &Path, home: Option<&Path>) -> String {
  let home = home.filter(|h| os != Os::Windows && h.is_absolute() && h.parent().is_some());
  match home.and_then(|h| path.strip_prefix(h).ok()) {
    Some(rest) if rest.as_os_str().is_empty() => "~".into(),
    Some(rest) => format!("~/{}", rest.display()),
    None => path.display().to_string(),
  }
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

/// [downloads] off Windows, with the environment and the files `read` gives, before it is checked
/// on disk.
#[cfg(not(windows))]
fn downloads_in(
  os: Os,
  env: &dyn Fn(&str) -> Option<OsString>,
  read: &dyn Fn(&Path) -> Option<String>,
) -> Option<PathBuf> {
  let home = env("HOME").map(PathBuf::from).filter(|h| h.is_absolute())?;
  let file = dir(os, Kind::Config, "user-dirs.dirs", env);
  let set = (os == Os::Linux).then(|| read(&file)).flatten();
  let dir = set.and_then(|t| user_dir(&t, "XDG_DOWNLOAD_DIR", &home));
  Some(dir.unwrap_or_else(|| home.join("Downloads"))).filter(|d| *d != home)
}

/// The folder `key` names in `text`, a `user-dirs.dirs`
/// (<https://www.freedesktop.org/wiki/Software/xdg-user-dirs/>, as of 2026-10): `KEY="$HOME/yyy"`
/// or `KEY="/yyy"`, shell-escaped in double quotes; the last line wins, as for the shell that
/// sources it. Another form is skipped, as the spec supports no other.
#[cfg(not(windows))]
fn user_dir(text: &str, key: &str, home: &Path) -> Option<PathBuf> {
  let unescape = |s: &str| {
    let (mut out, mut chars) = (String::new(), s.chars());
    while let Some(c) = chars.next() {
      out.push(if c == '\\' { chars.next().unwrap_or(c) } else { c });
    }

    out
  };

  let mut found = None;
  for line in text.lines() {
    let value = line.trim().strip_prefix(key).and_then(|v| v.strip_prefix("=\""));
    let Some(v) = value.and_then(|v| v.strip_suffix('"')) else { continue };
    found = match v.strip_prefix("$HOME") {
      Some(rest) if rest.is_empty() || rest.starts_with('/') => {
        Some(home.join(unescape(rest.trim_start_matches('/'))))
      }
      _ if v.starts_with('/') => Some(PathBuf::from(unescape(v))),
      _ => found,
    };
  }

  found
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

  /// Under the home folder as `~/…`, whole elsewhere, on Windows, or with a home of `/`.
  #[cfg(unix)]
  #[test]
  fn home_as_a_tilde() {
    let (p, home) = (Path::new, Some(Path::new("/Users/me")));
    let shown = |os, path| display_in(os, p(path), home);
    assert_eq!(shown(Os::Mac, "/Users/me/.cache/hub"), "~/.cache/hub");
    assert_eq!(shown(Os::Linux, "/Users/me"), "~");
    assert_eq!(shown(Os::Linux, "/Users/me/"), "~");
    assert_eq!(shown(Os::Mac, "/Users/meg/x"), "/Users/meg/x", "a component, not a prefix");
    assert_eq!(shown(Os::Windows, "/Users/me/x"), "/Users/me/x");
    assert_eq!(display_in(Os::Mac, p("/etc/x"), Some(p("/"))), "/etc/x");
    assert_eq!(display_in(Os::Mac, p("/Users/me/x"), None), "/Users/me/x");
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

  /// A `user-dirs.dirs` as xdg-user-dirs writes it, localized, escaped, or turned off.
  #[cfg(unix)]
  #[test]
  fn downloads_from_user_dirs() {
    let home = Path::new("/home/me");
    let dir = |text: &str| user_dir(text, "XDG_DOWNLOAD_DIR", home);
    let file = "# written by xdg-user-dirs-update\nXDG_DESKTOP_DIR=\"$HOME/Рабочий стол\"\n\
      XDG_DOWNLOAD_DIR=\"$HOME/Загрузки\"\n";
    assert_eq!(dir(file), Some(home.join("Загрузки")));
    assert_eq!(
      dir(r#"XDG_DOWNLOAD_DIR="/data/my \"dl\"""#),
      Some(PathBuf::from(r#"/data/my "dl""#))
    );
    assert_eq!(dir("XDG_DOWNLOAD_DIR=\"$HOME/\""), Some(home.into()));
    assert_eq!(
      dir("XDG_DOWNLOAD_DIR=\"$HOME/a\"\nXDG_DOWNLOAD_DIR=\"$HOME/b\""),
      Some(home.join("b"))
    );
    for skipped in [
      "",
      "# XDG_DOWNLOAD_DIR=\"/x\"",
      "XDG_DOWNLOAD_DIRS=\"/x\"",
      "XDG_DOWNLOAD_DIR=x",
      "XDG_DOWNLOAD_DIR=\"$HOMEx\"",
      "XDG_DOWNLOAD_DIR=\"rel\"",
    ] {
      assert_eq!(dir(skipped), None, "{skipped}");
    }

    // The file under XDG_CONFIG_HOME, read on Linux only; ~/Downloads without the entry, none
    // where the entry is the home folder.
    let env = |k: &str| match k {
      "HOME" => Some(OsString::from("/home/me")),
      "XDG_CONFIG_HOME" => Some(OsString::from("/cfg")),
      _ => None,
    };
    let read = |p: &Path| (p == Path::new("/cfg/user-dirs.dirs")).then(|| file.to_string());
    assert_eq!(downloads_in(Os::Linux, &env, &read), Some(home.join("Загрузки")));
    assert_eq!(downloads_in(Os::Mac, &env, &read), Some(home.join("Downloads")));
    assert_eq!(downloads_in(Os::Linux, &env, &|_| None), Some(home.join("Downloads")));
    let off = |_: &Path| Some("XDG_DOWNLOAD_DIR=\"$HOME\"".to_string());
    assert_eq!(downloads_in(Os::Linux, &env, &off), None);
    assert_eq!(downloads_in(Os::Linux, &|_| None, &read), None, "no home");
  }
}
