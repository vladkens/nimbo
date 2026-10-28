// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The update's steps on Linux, which unpacks nothing: the AppImage downloaded is the app, made
//! executable, then started in the old one's place.

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::{fs, io};

pub use crate::platform::process::{alive, detach, wait_exit};

/// Makes the AppImage downloaded as `file` executable and moves it into the folder `into`.
pub fn unpack(file: &Path, into: &Path) -> io::Result<()> {
  fs::set_permissions(file, fs::Permissions::from_mode(0o755))?;
  fs::rename(file, into.join("app.AppImage"))
}

/// Starts the AppImage `app` with `args`, not waiting for it, without the installer's AppImage
/// variables, which the runtime sets again for its own.
pub fn start(app: &Path, args: &[OsString]) -> io::Result<()> {
  let vars = ["APPIMAGE", "APPDIR", "ARGV0", "OWD"];
  vars.iter().fold(&mut Command::new(app), |c, k| c.env_remove(k)).args(args).spawn().map(drop)
}
