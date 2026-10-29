// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The update's steps on Linux, which unpacks nothing: the AppImage downloaded is the app, made
//! executable, then started in the old one's place.

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{fs, io};

pub use crate::platform::process::{alive, detach, wait_exit};

/// Makes the AppImage downloaded as `file` executable: the new app, for any target.
pub fn unpack(file: &Path, _: &Path) -> io::Result<PathBuf> {
  fs::set_permissions(file, fs::Permissions::from_mode(0o755)).map(|()| file.to_owned())
}

/// Starts the AppImage `app` with `args` and `note` in its [crate::update::NOTE], not waiting for
/// it, without the installer's AppImage variables, which the runtime sets again for its own.
pub fn start(app: &Path, args: &[OsString], note: Option<&str>) -> io::Result<()> {
  let mut cmd = Command::new(app);
  let cmd = ["APPIMAGE", "APPDIR", "ARGV0", "OWD"].iter().fold(&mut cmd, |c, k| c.env_remove(k));
  let cmd = crate::platform::process::closed(cmd);
  cmd.envs(note.map(|n| (crate::update::NOTE, n))).args(args).spawn().map(drop)
}
