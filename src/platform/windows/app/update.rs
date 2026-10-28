// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The update's process calls on Windows: the release's `.zip` unpacked by the system's `tar`,
//! the installer started apart from the app and waiting for it to end, the new exe started.

use std::ffi::OsString;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;
use std::{env, io};

use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::System::Threading::{
  CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW, DETACHED_PROCESS, OpenProcess, PROCESS_SYNCHRONIZE,
  WaitForSingleObject,
};

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

/// Whether the process `pid` runs.
pub fn alive(pid: u32) -> bool {
  !wait_exit(pid, Duration::ZERO)
}

/// Whether the process `pid` ended within `cap`: its handle signalled, or none to open (gone, or
/// another user's).
pub fn wait_exit(pid: u32, cap: Duration) -> bool {
  // SAFETY: a handle of ours, closed on drop; a pid that is gone gives none.
  let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
  if process.is_null() {
    return true;
  }

  let process = unsafe { OwnedHandle::from_raw_handle(process) };
  let ms = u32::try_from(cap.as_millis()).unwrap_or(u32::MAX - 1);
  // SAFETY: the handle is open.
  unsafe { WaitForSingleObject(process.as_raw_handle(), ms) == WAIT_OBJECT_0 }
}

/// Spawns `cmd` with no console, out of the app's job where the job lets it, so the app's end
/// doesn't end it too.
pub fn detach(cmd: &mut Command) -> io::Result<Child> {
  let flags = DETACHED_PROCESS | CREATE_NO_WINDOW;
  match cmd.creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB).spawn() {
    Err(_) => cmd.creation_flags(flags).spawn(),
    child => child,
  }
}

/// Starts the app's folder `app` by its exe, named as this one's, with `args`, not waiting for it.
/// The installer has no console to share, so a console app would get a window of its own, in
/// Windows Terminal or conhost: it gets a console with none (a windowed app is unaffected).
pub fn start(app: &Path, args: &[OsString]) -> io::Result<()> {
  let exe = app.join(env::current_exe()?.file_name().unwrap_or_default());
  Command::new(exe).args(args).creation_flags(CREATE_NO_WINDOW).spawn().map(drop)
}
