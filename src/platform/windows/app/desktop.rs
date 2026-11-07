// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The launcher entry ([crate::desktop]) on Windows: none to write, the executable carries
//! the name and icon.

use crate::AppInfo;
use crate::desktop::Launcher;

pub fn install(_: &AppInfo, _: &Launcher) -> Result<bool, String> {
  Ok(false)
}

pub fn uninstall(_: &AppInfo) -> Result<(), String> {
  Ok(())
}
