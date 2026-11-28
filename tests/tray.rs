// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The tray with nothing on screen: the detached tray's scenario (`tests/tray/detached.rs`)
//! where it needs no main thread, everywhere but macOS; and an icon read from an SVG file's path
//! data as the app gives it. The messenger's tray menu is `tests/messenger.rs`'s.

#[cfg(not(target_os = "macos"))]
#[path = "tray/detached.rs"]
mod detached;

use nimbo::tray::TrayIcon;

#[cfg(not(target_os = "macos"))]
#[test]
fn detached_tray() {
  detached::run();
}

/// A ring, filled even-odd.
const RING: &str = "M9 1a8 8 0 1 0 0 16A8 8 0 1 0 9 1Zm0 4a4 4 0 1 1 0 8 4 4 0 1 1 0-8Z";

/// The icon's path data reads from an icon file of it, its size and fill rule with it.
#[test]
fn an_icon_reads_from_an_svg_file() {
  let file = format!(r#"<svg viewBox="0 0 18 18"><path fill-rule="evenodd" d="{RING}"/></svg>"#);
  let icon = TrayIcon::Path { size: 18.0, d: RING.into(), even_odd: true };
  assert_eq!(TrayIcon::from_svg(&file).unwrap(), icon);
}
