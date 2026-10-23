// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Opening what lives outside the app: a link or a file in the app the OS picks for it ([url],
//! [path]), and a file or folder shown in the file manager ([reveal]), whose name a "Show in …"
//! button reads from [file_manager]. None of them waits for the other app to start.
//!
//! | | [url], [path] | [reveal] | [file_manager] |
//! |---|---|---|---|
//! | macOS | `NSWorkspace` `openURL:` | Finder, the item selected | Finder |
//! | Windows | `ShellExecuteW` | `explorer /select,` | Explorer |
//! | Linux | `xdg-open` | `org.freedesktop.FileManager1.ShowItems`, else its folder through `xdg-open` | by the desktop: Files (GNOME), Dolphin (KDE), … |

use std::path::Path;

use crate::platform::open as imp;

/// Opens `url` (`https://…`, `mailto:…`, a settings pane's scheme) in the app the OS has for it;
/// `Err` when it is no URL or nothing could be started.
pub fn url(url: &str) -> Result<(), String> {
  if !url.contains(':') {
    return Err(format!("{url:?} is not a URL"));
  }

  imp::url(url)
}

/// Opens the file or folder at `path` in its app: a folder in the file manager, a document in
/// its editor.
pub fn path(path: &Path) -> Result<(), String> {
  imp::path(&absolute(path)?)
}

/// Shows the file or folder at `path` in the file manager, selected in its folder where the file
/// manager can (Finder, Explorer, the file managers of GNOME and KDE); else its folder opens.
pub fn reveal(path: &Path) -> Result<(), String> {
  let path = absolute(path)?;
  if !path.exists() {
    return Err(format!("{} does not exist", path.display()));
  }

  imp::reveal(&path)
}

/// The file manager's name as the user knows it, for a button: `format!("Show in {}",
/// open::file_manager())`. On Linux the desktop's own (`XDG_CURRENT_DESKTOP`), "Files" for
/// GNOME and the ones not known.
pub fn file_manager() -> &'static str {
  imp::file_manager()
}

fn absolute(path: &Path) -> Result<std::path::PathBuf, String> {
  std::path::absolute(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// `path` as a `file://` URI, each byte outside the unreserved ones and `/` escaped (RFC 3986):
/// what the file managers' D-Bus interface takes.
#[cfg(any(all(test, unix), target_os = "linux"))]
pub(crate) fn file_uri(path: &Path) -> String {
  use std::os::unix::ffi::OsStrExt;

  let mut out = String::from("file://");
  for &b in path.as_os_str().as_bytes() {
    match b {
      b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
        out.push(b as char)
      }
      _ => out.push_str(&format!("%{b:02X}")),
    }
  }

  out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[cfg(unix)]
  #[test]
  fn file_uris_escape_what_a_uri_cannot_hold() {
    let uri = file_uri(Path::new("/home/me/My Files/100% тест#1"));
    assert_eq!(uri, "file:///home/me/My%20Files/100%25%20%D1%82%D0%B5%D1%81%D1%82%231");
  }

  #[test]
  fn what_is_no_url_is_refused() {
    assert!(url("example.com").is_err());
    assert!(reveal(Path::new("/no/such/place/at/all")).is_err());
  }

  #[test]
  fn the_file_manager_has_a_name() {
    let want = if cfg!(target_os = "macos") { "Finder" } else { "Explorer" };
    if !cfg!(target_os = "linux") {
      assert_eq!(file_manager(), want);
    }

    assert!(!file_manager().is_empty());
  }
}
