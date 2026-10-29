// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The launcher entry on Linux ([crate::desktop]): under `$XDG_DATA_HOME` (`~/.local/share`), for
//! the executable the autostart entry runs too.

use crate::AppInfo;
use crate::desktop::{Launcher, install_in, packaged, uninstall_in, unlist};

pub fn install(app: &AppInfo, launcher: &Launcher) -> Result<bool, String> {
  // Both resolved: the running exe is, and the home may be a link (/home to /var/home).
  let real = |p: std::path::PathBuf| std::fs::canonicalize(&p).unwrap_or(p);
  let exe = real(super::autostart::exe()?);
  let home = std::env::var_os("HOME").map(|h| real(h.into()));
  let data = crate::paths::data_dir("");
  // An entry of its own from an earlier run (the AppImage) would hide the package's. An AppImage
  // is never a package, wherever it is.
  if std::env::var_os("APPIMAGE").is_none() && packaged(&exe, home.as_deref()) {
    return unlist(&data, app).map(|()| false);
  }

  install_in(&data, app, launcher, &exe.to_string_lossy())
}

pub fn uninstall(app: &AppInfo) -> Result<(), String> {
  uninstall_in(&crate::paths::data_dir(""), app)
}
