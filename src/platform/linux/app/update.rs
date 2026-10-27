// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The update's steps on Linux, which unpacks nothing: the AppImage downloaded is the app, made
//! executable, then started in the old one's place.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::{fs, io};

use crate::instance::RELAUNCH;

/// Makes the AppImage downloaded as `file` executable and moves it into the folder `into`.
pub fn unpack(file: &Path, into: &Path) -> io::Result<()> {
  fs::set_permissions(file, fs::Permissions::from_mode(0o755))?;
  fs::rename(file, into.join("app.AppImage"))
}

/// Starts the AppImage `app` with [RELAUNCH] set to `var`, not waiting for it.
pub fn relaunch(app: &Path, var: &str) -> io::Result<()> {
  Command::new(app).env(RELAUNCH, var).spawn().map(drop)
}
