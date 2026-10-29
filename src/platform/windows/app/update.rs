// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The update's process calls on Windows: the installer started apart from the app and waiting
//! for it to end, the release's NSIS setup run silently, the installed exe started.

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

use crate::update::NOTE;

/// The setup downloaded as `file`, run as it came: the new app, for any target.
pub fn unpack(file: &Path, _: &Path) -> io::Result<PathBuf> {
  Ok(file.to_owned())
}

/// Runs the per-user NSIS `exe` silently (`/S`, into `%LOCALAPPDATA%\Programs\<name>`) with
/// no window, to its end; its exit code (NSIS': 0 installed, 1 cancelled, 2 aborted by the script).
pub fn setup(exe: &Path) -> io::Result<i32> {
  let done = Command::new(exe).arg("/S").creation_flags(CREATE_NO_WINDOW).status()?;
  Ok(done.code().unwrap_or(-1))
}

/// Whether the process `pid` runs.
pub fn alive(pid: u32) -> bool {
  !wait_exit(pid)(pid, Duration::ZERO)
}

/// The wait for the process `pid` to end, its handle opened now so that a pid reused later can't
/// fool it: whether it ended within the time, its handle signalled, or there was none to open
/// (gone, or another user's).
pub fn wait_exit(pid: u32) -> impl Fn(u32, Duration) -> bool {
  // SAFETY: a handle of ours, closed on drop; a pid that is gone gives none.
  let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
  let process = (!process.is_null()).then(|| unsafe { OwnedHandle::from_raw_handle(process) });
  move |_, cap| {
    let ms = u32::try_from(cap.as_millis()).unwrap_or(u32::MAX - 1);
    // SAFETY: the handle is open.
    let ended = |p: &OwnedHandle| unsafe { WaitForSingleObject(p.as_raw_handle(), ms) };
    process.as_ref().is_none_or(|p| ended(p) == WAIT_OBJECT_0)
  }
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

/// Starts the app's folder `app` by its exe, named as this one's, with `args` and `note` in its
/// [NOTE] (none inherited), not waiting for it. The installer has no console to share, so a console
/// app would get a window of its own, in Windows Terminal or conhost: it gets a console with none
/// (a windowed app is unaffected).
pub fn start(app: &Path, args: &[OsString], note: Option<&str>) -> io::Result<()> {
  let exe = app.join(env::current_exe()?.file_name().unwrap_or_default());
  let mut cmd = Command::new(exe);
  cmd.env_remove(NOTE).envs(note.map(|n| (NOTE, n))).args(args).creation_flags(CREATE_NO_WINDOW);
  cmd.spawn().map(drop)
}
