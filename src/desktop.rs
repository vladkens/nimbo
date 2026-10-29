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
//! The tray's icons go in `$XDG_DATA_HOME/icons` ([tray_icon]); [uninstall] removes them too.
//!
//! The entry runs `$APPIMAGE` when set, else the executable running now, and names the window's
//! class ([crate::AppInfo::id]) so the desktop matches the app's windows to it. Inside Flatpak
//! the package has its own: not for there.

#[cfg(any(all(test, unix), target_os = "linux"))]
use std::path::{Path, PathBuf};

use crate::platform::desktop as imp;
use crate::{AppInfo, Png};

/// What the launcher shows besides the app's name.
#[derive(Clone, Copy)]
pub struct Launcher<'a> {
  /// A tooltip's line: what the app does.
  pub comment: &'a str,
  /// The menu's categories by the Desktop Menu spec: `Utility`, `AudioVideo`, `Development`.
  pub categories: &'a [&'a str],
  /// The app's icon, square, its side one of the icon theme's sizes ([SIDES]), 256 for every
  /// size the desktop shows: `Png::icon(include_bytes!("icon.png"))`. Decoded only when [install]
  /// writes it, not on a launch that finds the entry current.
  pub icon: Png,
}

/// The icon sides the hicolor theme looks under.
pub const SIDES: [u32; 10] = [16, 22, 24, 32, 48, 64, 96, 128, 256, 512];

/// Writes the app's launcher entry and icon where they differ from what is there (first run, the
/// AppImage moved): `true` when it wrote. Nothing for an executable outside the home folder
/// (/usr, /opt, /snap): its package brings its own entry, which one here would outlive. Quiet: an
/// error is returned, never shown.
pub fn install(app: &AppInfo, launcher: &Launcher) -> Result<bool, String> {
  imp::install(app, launcher)
}

/// Removes the entry and its icons; what is missing already is fine.
pub fn uninstall(app: &AppInfo) -> Result<(), String> {
  imp::uninstall(app)
}

/// The executable `exe` is a package's: outside the home folder `home`, or with no home to tell.
#[cfg(any(all(test, unix), target_os = "linux"))]
pub(crate) fn packaged(exe: &Path, home: Option<&Path>) -> bool {
  !home.is_some_and(|h| h.is_absolute() && h.parent().is_some() && exe.starts_with(h))
}

/// Removes the launcher entry under the data folder `data`, if there is one; the icons stay, as
/// a running tray may use its own.
#[cfg(any(all(test, unix), target_os = "linux"))]
pub(crate) fn unlist(data: &Path, app: &AppInfo) -> Result<(), String> {
  let path = entry(data, app);
  match std::fs::remove_file(&path) {
    Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {e}", path.display())),
    _ => Ok(()),
  }
}

/// The launcher entry under the data folder `data`.
#[cfg(any(all(test, unix), target_os = "linux"))]
fn entry(data: &Path, app: &AppInfo) -> PathBuf {
  data.join(format!("applications/{}.desktop", app.id))
}

/// The start of the names of app `id`'s tray icons ([tray_icon]), with no dash: KIconLoader
/// looks for a name's shorter forms (up to a dash) in each theme first, and `<id>` alone is the
/// launcher's icon.
#[cfg(any(all(test, unix), target_os = "linux"))]
fn tray_icons(id: &str) -> String {
  format!("{}.tray", id.replace('-', "_"))
}

/// The file `name` is one of app `id`'s tray icons: its start, 16 hex digits, `-symbolic.svg`.
#[cfg(any(all(test, unix), target_os = "linux"))]
fn is_tray_icon(name: &str, id: &str) -> bool {
  let hash = name.strip_prefix(&tray_icons(id)).and_then(|n| n.strip_suffix("-symbolic.svg"));
  hash.is_some_and(|h| h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Writes a tray icon's `svg` for app `id` at the top of the data folder `data`'s `icons` (no
/// theme's: KIconLoader and GTK look there each time, in a theme only in the folders there were
/// as they started), named by its content, as panels keep an icon by its name, and `-symbolic`
/// for GNOME to colour it; with `clean` (the run's first) removes the app's others, as a host
/// may still ask for one of this run's. Its name, as `IconName` gives it.
#[cfg(any(all(test, unix), target_os = "linux"))]
pub(crate) fn tray_icon(data: &Path, id: &str, svg: &str, clean: bool) -> Result<String, String> {
  use std::hash::{Hash, Hasher};
  let mut hash = std::hash::DefaultHasher::new();
  svg.hash(&mut hash);
  let (prefix, icons) = (tray_icons(id), data.join("icons"));
  let name = format!("{prefix}{:016x}-symbolic", hash.finish());
  let file = icons.join(format!("{name}.svg"));
  if !file.exists() {
    crate::platform::write::file(&file, svg.as_bytes())?;
  }

  for e in std::fs::read_dir(&icons).into_iter().flatten().flatten().filter(|_| clean) {
    let n = e.file_name().to_string_lossy().into_owned();
    if is_tray_icon(&n, id) && n != format!("{name}.svg") {
      _ = std::fs::remove_file(e.path());
    }
  }

  Ok(name)
}

/// Dates the icon theme's folder `hicolor` now, as an icon in it changed: a cache older than it
/// is read no more (the Icon Theme spec's implementation notes), and a size folder that was
/// there keeps the theme's date.
#[cfg(any(all(test, unix), target_os = "linux"))]
fn date(hicolor: &Path) -> std::io::Result<()> {
  std::fs::File::open(hicolor)?.set_modified(std::time::SystemTime::now())
}

/// [install] under the data folder `data`, for the executable `exe`. The entry names the icon
/// file's hash: an entry the same, with an icon there, finds the icon current undecoded.
#[cfg(any(all(test, unix), target_os = "linux"))]
pub(crate) fn install_in(
  data: &Path,
  app: &AppInfo,
  l: &Launcher,
  exe: &str,
) -> Result<bool, String> {
  use std::hash::{Hash, Hasher};
  let mut hash = std::hash::DefaultHasher::new();
  l.icon.hash(&mut hash);
  let (stamp, id) = (format!("{:016x}", hash.finish()), app.id);
  let categories: String = l.categories.iter().map(|c| format!("{c};")).collect();
  // Startup notification: GLib hands a launch its activation token, which a second launch sends
  // to the instance ([crate::instance]) for its window to come to the front.
  let keys = [
    ("Comment", l.comment),
    ("Categories", &categories),
    ("StartupWMClass", id),
    ("StartupNotify", "true"),
    ("X-Nimbo-Icon", &stamp),
  ];
  let entry_text = crate::autostart::desktop_entry(app, exe, &keys).into_bytes();
  let hicolor = data.join("icons/hicolor");
  let icon_in = |d: std::fs::DirEntry| d.path().join(format!("apps/{id}.png")).exists();
  if std::fs::read(entry(data, app)).is_ok_and(|b| b == entry_text)
    && std::fs::read_dir(&hicolor).into_iter().flatten().flatten().any(icon_in)
  {
    return Ok(false);
  }

  let (side, h, px) = l.icon.rgba();
  if side != h || !SIDES.contains(&side) {
    return Err(format!("a {side} × {h} icon: square, its side one of {SIDES:?}"));
  }

  let icon = hicolor.join(format!("{side}x{side}/apps/{id}.png"));
  let png = crate::png::encode_rgba(side as usize, side as usize, px);
  // The icon first: the entry, once there, finds it.
  let mut wrote = false;
  for (path, bytes) in [(icon, png), (entry(data, app), entry_text)] {
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

/// [uninstall] under the data folder `data`: the entry, the icon in every size and the tray's.
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
  let tray = std::fs::read_dir(data.join("icons")).into_iter().flatten().flatten();
  let tray = tray
    .map(|e| e.path())
    .filter(|p| p.file_name().is_some_and(|n| is_tray_icon(&n.to_string_lossy(), app.id)));
  for path in icons.chain(tray).chain([entry(data, app)]) {
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
  /// A tray icon is named by its content, with one dash; the run's first removes the app's
  /// others, and no other app's.
  #[cfg(unix)]
  #[test]
  fn tray_icons_by_their_content() {
    use super::*;
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

  /// A package's executable gets no entry of the app's own: anywhere outside the home folder.
  #[cfg(unix)]
  #[test]
  fn none_for_a_package() {
    use super::*;
    let home = Some(Path::new("/home/me"));
    for exe in ["/usr/bin/app", "/opt/App/app", "/snap/app/1/app", "/home/meg/app"] {
      assert!(packaged(Path::new(exe), home), "{exe}");
    }

    assert!(!packaged(Path::new("/home/me/Apps/App.AppImage"), home));
    assert!(packaged(Path::new("/home/me/app"), None));
    assert!(packaged(Path::new("/app"), Some(Path::new("/"))));

    // What an earlier run wrote goes; once gone, that's no error.
    let data = std::env::temp_dir().join(format!("nimbo-unlist-{}", std::process::id()));
    let app =
      AppInfo { id: "com.example.unlist", name: "Unlist", presence: crate::Presence::Regular };
    std::fs::create_dir_all(data.join("applications")).unwrap();
    std::fs::write(entry(&data, &app), "").unwrap();
    assert_eq!(unlist(&data, &app), Ok(()));
    assert!(!entry(&data, &app).exists());
    assert_eq!(unlist(&data, &app), Ok(()));
    _ = std::fs::remove_dir_all(&data);
  }

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
    let png = |w, h, px: &[u8]| Png::new(crate::png::encode_rgba(w, h, px).leak());
    let l = Launcher {
      comment: "Counts\nC:\\things",
      categories: &["Utility", "Office"],
      icon: png(16, 16, &px),
    };
    assert_eq!(install_in(&data, &APP, &l, "/opt/Ex ample/app"), Ok(true));
    let text = std::fs::read_to_string(data.join("applications/com.example.app.desktop")).unwrap();
    let want = "[Desktop Entry]\nType=Application\nName=Example\nExec=\"/opt/Ex ample/app\"\n\
      Icon=com.example.app\nTerminal=false\nComment=Counts C:\\\\things\nCategories=Utility;Office;\n\
      StartupWMClass=com.example.app\nStartupNotify=true\nX-Nimbo-Icon=";
    let stamp = text.strip_prefix(want).and_then(|t| t.strip_suffix('\n')).unwrap_or_default();
    assert!(stamp.len() == 16 && stamp.bytes().all(|b| b.is_ascii_hexdigit()), "{text}");
    let icon = data.join("icons/hicolor/16x16/apps/com.example.app.png");
    assert_eq!(crate::png::decode(&std::fs::read(&icon).unwrap()), (16, 16, px.clone()));

    assert_eq!(install_in(&data, &APP, &l, "/opt/Ex ample/app"), Ok(false), "the same: kept");
    std::fs::remove_file(&icon).unwrap();
    assert_eq!(install_in(&data, &APP, &l, "/opt/Ex ample/app"), Ok(true), "the icon gone");
    assert!(icon.exists());
    // A new icon dates the theme's folder, which a cache is checked against.
    let hicolor = data.join("icons/hicolor");
    let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1);
    std::fs::File::open(&hicolor).unwrap().set_modified(old).unwrap();
    let px2 = vec![9; px.len()];
    assert_eq!(
      install_in(&data, &APP, &Launcher { icon: png(16, 16, &px2), ..l }, "/opt/Ex ample/app"),
      Ok(true)
    );
    assert!(std::fs::metadata(&hicolor).unwrap().modified().unwrap() > old);

    assert_eq!(install_in(&data, &APP, &l, "/home/me/app"), Ok(true), "moved");
    let text = std::fs::read_to_string(data.join("applications/com.example.app.desktop")).unwrap();
    assert!(text.contains("Exec=\"/home/me/app\"\n"), "{text}");

    for icon in [png(15, 15, &px[..900]), png(16, 8, &px[..512])] {
      assert!(install_in(&data, &APP, &Launcher { icon, ..l }, "/a").is_err());
    }

    std::fs::write(hicolor.join("icon-theme.cache"), b"").unwrap();
    let tray =
      data.join(format!("icons/{}.svg", tray_icon(&data, APP.id, "<svg/>", false).unwrap()));
    std::fs::File::open(&hicolor).unwrap().set_modified(old).unwrap();
    uninstall_in(&data, &APP).unwrap();
    assert!(std::fs::metadata(&hicolor).unwrap().modified().unwrap() > old, "dated");
    assert!(!data.join("applications/com.example.app.desktop").exists());
    assert!(!data.join("icons/hicolor/16x16/apps/com.example.app.png").exists());
    assert!(!tray.exists());
    uninstall_in(&data, &APP).unwrap();
    _ = std::fs::remove_dir_all(&data);
  }
}
