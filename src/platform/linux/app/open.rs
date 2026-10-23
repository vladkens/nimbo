// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Opening links and files on Linux: `xdg-open`, which every desktop has (and which goes through
//! the OpenURI portal inside Flatpak), its exit awaited on a thread of its own; an item shown
//! selected through the file managers' `org.freedesktop.FileManager1` (Nautilus, Dolphin, Nemo,
//! Caja, Thunar), else its folder opened.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use super::super::dbus::{self, V};
use crate::open::file_uri;

pub fn url(url: &str) -> Result<(), String> {
  xdg_open(url.as_ref())
}

pub fn path(path: &Path) -> Result<(), String> {
  xdg_open(path.as_os_str())
}

/// On a thread of its own: the file manager may have to start first, and the call waits for it.
pub fn reveal(path: &Path) -> Result<(), String> {
  let (path, uri) = (path.to_owned(), file_uri(path));
  std::thread::spawn(move || {
    let items = V::Array("s", vec![V::S(uri)]);
    let args = V::Tuple(vec![items, V::s("")]);
    let (name, object) = ("org.freedesktop.FileManager1", "/org/freedesktop/FileManager1");
    let call = |bus| dbus::call(bus, name, object, name, "ShowItems", args, 5000);
    if dbus::session().and_then(call).is_err()
      && let Some(dir) = path.parent()
    {
      let _ = xdg_open(dir.as_os_str());
    }
  });
  Ok(())
}

pub fn file_manager() -> &'static str {
  by_desktop(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default())
}

/// The file manager of desktop `names` (`XDG_CURRENT_DESKTOP`, such as `ubuntu:GNOME`), by the
/// name it shows: "Files" for GNOME's and those not known.
fn by_desktop(names: &str) -> &'static str {
  let known = names.split(':').find_map(|d| match d.to_ascii_lowercase().as_str() {
    "kde" => Some("Dolphin"),
    "xfce" => Some("Thunar"),
    "x-cinnamon" | "cinnamon" => Some("Nemo"),
    "mate" => Some("Caja"),
    "lxqt" => Some("PCManFM-Qt"),
    _ => None,
  });
  known.unwrap_or("Files")
}

fn xdg_open(what: &OsStr) -> Result<(), String> {
  let mut child =
    Command::new("xdg-open").arg(what).spawn().map_err(|e| format!("xdg-open: {e}"))?;
  std::thread::spawn(move || child.wait());
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn file_managers_by_desktop() {
    let names = ["ubuntu:GNOME", "KDE", "XFCE", "X-Cinnamon", "MATE", "LXQt", "sway", ""];
    let want = ["Files", "Dolphin", "Thunar", "Nemo", "Caja", "PCManFM-Qt", "Files", "Files"];
    assert_eq!(names.map(by_desktop), want);
  }
}
