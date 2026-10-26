// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The launcher entry ([crate::desktop]) on macOS: none to write, the app bundle carries
//! the name and icon.
//! nothing to write.

pub fn install(_: &crate::AppInfo, _: &crate::desktop::Launcher) -> Result<bool, String> {
  Ok(false)
}

pub fn uninstall(_: &crate::AppInfo) -> Result<(), String> {
  Ok(())
}
