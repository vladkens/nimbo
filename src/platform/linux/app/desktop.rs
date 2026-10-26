// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The launcher entry on Linux ([crate::desktop]): under `$XDG_DATA_HOME` (`~/.local/share`), for
//! the executable the autostart entry runs too.

use crate::AppInfo;
use crate::desktop::{Launcher, install_in, uninstall_in};

pub fn install(app: &AppInfo, launcher: &Launcher) -> Result<bool, String> {
  let exe = super::autostart::exe()?;
  install_in(&crate::paths::data_dir(""), app, launcher, &exe.to_string_lossy())
}

pub fn uninstall(app: &AppInfo) -> Result<(), String> {
  uninstall_in(&crate::paths::data_dir(""), app)
}
