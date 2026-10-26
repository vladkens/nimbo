// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app's entry in the desktop's launcher on Linux, as a package writes it at install: what
//! the dock, the window's icon on Wayland and the portals' dialogs find the app by. An app that
//! ships without a package (an AppImage, a bare binary) calls [install] at start.
//!
//! | | what [install] writes |
//! |---|---|
//! | macOS, Windows | nothing: the app bundle or the exe carries its name and icon |
//! | Linux | `$XDG_DATA_HOME/applications/<id>.desktop` (`~/.local/share` without it) and its icon, `$XDG_DATA_HOME/icons/hicolor/<side>x<side>/apps/<id>.png` |
//!
//! The entry runs `$APPIMAGE` when set, else the executable running now, and names the window's
//! class ([crate::AppInfo::id]) so the desktop matches the app's windows to it. Inside Flatpak
//! the package has its own: not for there.

#[cfg(any(all(test, unix), target_os = "linux"))]
use std::path::{Path, PathBuf};

use crate::AppInfo;
use crate::platform::desktop as imp;

/// What the launcher shows besides the app's name.
#[derive(Clone, Copy, Debug)]
pub struct Launcher<'a> {
  /// A tooltip's line: what the app does.
  pub comment: &'a str,
  /// The menu's categories by the Desktop Menu spec: `Utility`, `AudioVideo`, `Development`.
  pub categories: &'a [&'a str],
  /// `side` × `side` pixels of straight RGBA, rows top-down ([crate::Config::icon]); `side` one
  /// of the icon theme's sizes ([SIDES]), 256 for every size the desktop shows.
  pub icon: (u32, &'a [u8]),
}

/// The icon sides the hicolor theme looks under.
pub const SIDES: [u32; 10] = [16, 22, 24, 32, 48, 64, 96, 128, 256, 512];

/// Writes the app's launcher entry and icon where they differ from what is there (first run, the
/// AppImage moved): `true` when it wrote. Quiet: an error is returned, never shown.
pub fn install(app: &AppInfo, launcher: &Launcher) -> Result<bool, String> {
  imp::install(app, launcher)
}

/// Removes the entry and its icons; what is missing already is fine.
pub fn uninstall(app: &AppInfo) -> Result<(), String> {
  imp::uninstall(app)
}

/// The launcher entry under the data folder `data`.
#[cfg(any(all(test, unix), target_os = "linux"))]
fn entry(data: &Path, app: &AppInfo) -> PathBuf {
  data.join(format!("applications/{}.desktop", app.id))
}

/// Dates the icon theme's folder `hicolor` now, as an icon in it changed: a cache older than it
/// is read no more (the Icon Theme spec's implementation notes), and a size folder that was
/// there keeps the theme's date.
#[cfg(any(all(test, unix), target_os = "linux"))]
fn date(hicolor: &Path) -> std::io::Result<()> {
  std::fs::File::open(hicolor)?.set_modified(std::time::SystemTime::now())
}

/// [install] under the data folder `data`, for the executable `exe`.
#[cfg(any(all(test, unix), target_os = "linux"))]
pub(crate) fn install_in(
  data: &Path,
  app: &AppInfo,
  l: &Launcher,
  exe: &str,
) -> Result<bool, String> {
  let ((side, px), id) = (l.icon, app.id);
  if !SIDES.contains(&side) || px.len() != side as usize * side as usize * 4 {
    return Err(format!("an icon is side² RGBA pixels, the side one of {SIDES:?}"));
  }

  let categories: String = l.categories.iter().map(|c| format!("{c};")).collect();
  let keys = [("Comment", l.comment), ("Categories", &categories), ("StartupWMClass", id)];
  let entry_text = crate::autostart::desktop_entry(app, exe, &keys);
  let hicolor = data.join("icons/hicolor");
  let icon = hicolor.join(format!("{side}x{side}/apps/{id}.png"));
  let png = crate::png::encode_rgba(side as usize, side as usize, px);
  // The icon first: the entry, once there, finds it.
  let mut wrote = false;
  for (path, bytes) in [(icon, png), (entry(data, app), entry_text.into_bytes())] {
    if std::fs::read(&path).is_ok_and(|b| b == bytes) {
      continue;
    }

    crate::platform::write::file(&path, &bytes)?;
    if path.starts_with(&hicolor) {
      date(&hicolor).map_err(|e| format!("{}: {e}", hicolor.display()))?;
    }

    wrote = true;
  }

  Ok(wrote)
}

/// [uninstall] under the data folder `data`: the entry, and the icon in every size.
#[cfg(any(all(test, unix), target_os = "linux"))]
pub(crate) fn uninstall_in(data: &Path, app: &AppInfo) -> Result<(), String> {
  let hicolor = data.join("icons/hicolor");
  let err = |e: std::io::Error| format!("{}: {e}", hicolor.display());
  let dirs: Vec<std::fs::DirEntry> = match std::fs::read_dir(&hicolor) {
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
    dirs => dirs.map_err(err)?.collect::<Result<_, _>>().map_err(err)?,
  };
  // The size folders; the theme's cache and index are files beside them.
  let sizes = dirs.iter().map(|d| d.path()).filter(|p| p.is_dir());
  let icons = sizes.map(|p| p.join(format!("apps/{}.png", app.id)));
  for path in icons.chain([entry(data, app)]) {
    match std::fs::remove_file(&path) {
      Ok(()) if path.starts_with(&hicolor) => date(&hicolor).map_err(err)?,
      Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
        return Err(format!("{}: {e}", path.display()));
      }
      _ => {}
    }
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  /// Unix: Windows opens no folder as a file, to date it.
  #[cfg(unix)]
  #[test]
  fn installs_once_and_again_when_moved() {
    use super::*;
    const APP: AppInfo =
      AppInfo { id: "com.example.app", name: "Example", presence: crate::Presence::Regular };
    // A folder of its own under the system's temp one, emptied first.
    let data = std::env::temp_dir().join(format!("nimbo-desktop-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&data);
    let px: Vec<u8> = (0..16 * 16 * 4).map(|i| (i * 7) as u8).collect();
    let l = Launcher {
      comment: "Counts\nC:\\things",
      categories: &["Utility", "Office"],
      icon: (16, &px),
    };
    assert_eq!(install_in(&data, &APP, &l, "/opt/Ex ample/app"), Ok(true));
    let text = std::fs::read_to_string(data.join("applications/com.example.app.desktop")).unwrap();
    let want = "[Desktop Entry]\nType=Application\nName=Example\nExec=\"/opt/Ex ample/app\"\n\
      Icon=com.example.app\nTerminal=false\nComment=Counts C:\\\\things\nCategories=Utility;Office;\n\
      StartupWMClass=com.example.app\n";
    assert_eq!(text, want);
    let png = std::fs::read(data.join("icons/hicolor/16x16/apps/com.example.app.png")).unwrap();
    assert_eq!(crate::png::tests::decode(&png), (16, 16, px.clone()));

    assert_eq!(install_in(&data, &APP, &l, "/opt/Ex ample/app"), Ok(false), "the same: kept");
    // A new icon dates the theme's folder, which a cache is checked against.
    let hicolor = data.join("icons/hicolor");
    let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1);
    std::fs::File::open(&hicolor).unwrap().set_modified(old).unwrap();
    let px2 = vec![9; px.len()];
    assert_eq!(
      install_in(&data, &APP, &Launcher { icon: (16, &px2), ..l }, "/opt/Ex ample/app"),
      Ok(true)
    );
    assert!(std::fs::metadata(&hicolor).unwrap().modified().unwrap() > old);

    assert_eq!(install_in(&data, &APP, &l, "/home/me/app"), Ok(true), "moved");
    let text = std::fs::read_to_string(data.join("applications/com.example.app.desktop")).unwrap();
    assert!(text.contains("Exec=\"/home/me/app\"\n"), "{text}");

    assert!(install_in(&data, &APP, &Launcher { icon: (15, &px[..900]), ..l }, "/a").is_err());
    assert!(install_in(&data, &APP, &Launcher { icon: (16, &px[..4]), ..l }, "/a").is_err());

    std::fs::write(hicolor.join("icon-theme.cache"), b"").unwrap();
    std::fs::File::open(&hicolor).unwrap().set_modified(old).unwrap();
    uninstall_in(&data, &APP).unwrap();
    assert!(std::fs::metadata(&hicolor).unwrap().modified().unwrap() > old, "dated");
    assert!(!data.join("applications/com.example.app.desktop").exists());
    assert!(!data.join("icons/hicolor/16x16/apps/com.example.app.png").exists());
    uninstall_in(&data, &APP).unwrap();
    _ = std::fs::remove_dir_all(&data);
  }
}
