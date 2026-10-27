// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The update's process calls on Windows: the release's `.zip` unpacked by the system's `tar`,
//! the new exe started in the old one's place.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, io};

use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::instance::RELAUNCH;
use crate::update::bad;

/// Unpacks the `.zip` `file` into the folder `into` with `%SystemRoot%\System32\tar.exe`, never
/// one from `PATH`, and with no console window.
pub fn unpack(file: &Path, into: &Path) -> io::Result<()> {
  let root = env::var_os("SystemRoot").map_or(PathBuf::from(r"C:\Windows"), PathBuf::from);
  let mut tar = Command::new(root.join(r"System32\tar.exe"));
  let out = tar.arg("-xf").arg(file).arg("-C").arg(into).creation_flags(CREATE_NO_WINDOW);
  let out = out.output()?;
  let err = || bad(format!("tar: {}", String::from_utf8_lossy(&out.stderr).trim()));
  out.status.success().then_some(()).ok_or_else(err)
}

/// Starts the exe `app` with [RELAUNCH] set to `var`, not waiting for it.
pub fn relaunch(app: &Path, var: &str) -> io::Result<()> {
  Command::new(app).env(RELAUNCH, var).spawn().map(drop)
}
