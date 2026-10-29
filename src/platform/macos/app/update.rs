// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The update's process calls on macOS: the release's `.dmg` attached by `hdiutil` and its one
//! bundle copied out with `ditto`, held to the running one's id and designated requirement by
//! Security.framework, the two exchanged in one step, the new one opened by LaunchServices in the
//! old one's place.

use std::ffi::{CString, OsStr, OsString, c_char};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr::{self, NonNull};
use std::{fs, io};

use objc2_core_foundation::{CFRetained, CFURL, Type};
use objc2_foundation::{NSBundle, NSString};
use objc2_security::{SecCSFlags, SecCode, SecStaticCode, kSecCSCheckAllArchitectures};

pub use crate::platform::process::{alive, detach, wait_exit};
use crate::update::{NOTE, bad};

/// Copies the one bundle at the top of the disk image `file` into `file`'s folder, as
/// `<folder>/new/<name>.app`, once it [accepts] it in place of `app`: the image attached read-only
/// at `<folder>/volume`, unseen by Finder, and detached again, forced if it must be. `InvalidData`
/// for an image with more than one `.app` at its top, or none.
pub fn unpack(file: &Path, app: &Path) -> io::Result<PathBuf> {
  let (dir, hdiutil) = (file.parent().unwrap_or(file), "/usr/bin/hdiutil");
  let volume = dir.join("volume");
  let attach = ["attach", "-nobrowse", "-noautoopen", "-readonly", "-mountpoint"].map(OsStr::new);
  run(hdiutil, &[&attach[..], &[volume.as_os_str(), file.as_os_str()]].concat())?;
  let new = copy(&volume, dir, app);
  let detach =
    |force: &[&OsStr]| run(hdiutil, &[&["detach".as_ref(), volume.as_os_str()], force].concat());
  let detached = detach(&[]).or_else(|_| detach(&["-force".as_ref()]));
  new.and_then(|new| detached.map(|()| new))
}

/// The one bundle at the top of `volume`, accepted for `app` and copied into `dir` by `ditto`.
fn copy(volume: &Path, dir: &Path, app: &Path) -> io::Result<PathBuf> {
  let all = fs::read_dir(volume)?.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir()));
  let mut apps = all.map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "app"));
  let (Some(one), None) = (apps.next(), apps.next()) else {
    return Err(bad("the image holds more than one app, or none"));
  };

  accepts(&one, app)?;
  let new = dir.join("new").join(one.file_name().unwrap_or_default());
  run("/usr/bin/ditto", &[one.as_os_str(), new.as_os_str()])?;
  Ok(new)
}

/// Whether the bundle `new` may replace the bundle `app`: the same bundle id, and signed so that
/// it meets `app`'s designated requirement, checked for every architecture as Sparkle's
/// `SUCodeSigningVerifier` does (an ad-hoc app's is its own cdhash, which no other build meets);
/// `InvalidData` with Security's reason if not.
pub fn accepts(new: &Path, app: &Path) -> io::Result<()> {
  let id = |p: &Path| {
    let bundle = NSBundle::bundleWithPath(&NSString::from_str(&p.to_string_lossy()));
    bundle?.bundleIdentifier().map(|id| id.to_string())
  };
  if id(new).is_none() || id(new) != id(app) {
    return Err(bad(format!("{} is not the app of {}", new.display(), app.display())));
  }

  let (code, mut req, mut err) = (static_code(app)?, ptr::null_mut(), ptr::null_mut());
  // SAFETY: the code lives through the call, `req` an out pointer it sets on success.
  let status = unsafe {
    SecCode::copy_designated_requirement(&code, SecCSFlags::DefaultFlags, NonNull::from(&mut req))
  };
  let req = owned(app, status, req)?;
  let flags = SecCSFlags(kSecCSCheckAllArchitectures);
  // SAFETY: the code and requirement live through the call, `err` an out pointer.
  let status = unsafe { static_code(new)?.check_validity_with_errors(flags, Some(&req), &mut err) };
  if status == 0 {
    return Ok(());
  }

  // SAFETY: an error the call made, ours to release.
  let why = NonNull::new(err).and_then(|e| unsafe { CFRetained::from_raw(e) }.description());
  let why = why.map_or(format!("status {status}"), |d| d.to_string());
  Err(bad(format!("{} doesn't meet {}'s requirement: {why}", new.display(), app.display())))
}

/// The signed code of the bundle at `path`.
fn static_code(path: &Path) -> io::Result<CFRetained<SecStaticCode>> {
  let url = CFURL::from_directory_path(path);
  let url = url.ok_or_else(|| bad(format!("{} is not a path", path.display())))?;
  let mut code = ptr::null();
  // SAFETY: the URL lives through the call, `code` an out pointer it sets on success.
  let status = unsafe {
    SecStaticCode::create_with_path(&url, SecCSFlags::DefaultFlags, NonNull::from(&mut code))
  };
  owned(path, status, code.cast_mut())
}

/// What a Security call of `status` made at `made`, ours to release; `InvalidData` if it failed.
fn owned<T: Type>(path: &Path, status: i32, made: *mut T) -> io::Result<CFRetained<T>> {
  let made = NonNull::new(made).filter(|_| status == 0);
  // SAFETY: an object the call made with a retain count of one.
  let made = made.map(|m| unsafe { CFRetained::from_raw(m) });
  made.ok_or_else(|| bad(format!("{}: Security's status {status}", path.display())))
}

/// Exchanges the paths `a` and `b` in one step, `renamex_np` with `RENAME_SWAP`, as Sparkle's
/// `SUFileManager` does on APFS; `Unsupported` on a volume without it (HFS+).
pub fn exchange(a: &Path, b: &Path) -> io::Result<()> {
  unsafe extern "C" {
    fn renamex_np(from: *const c_char, to: *const c_char, flags: u32) -> i32;
  }

  const RENAME_SWAP: u32 = 2;
  const ENOTSUP: i32 = 45;
  let path = |p: &Path| CString::new(p.as_os_str().as_bytes()).map_err(io::Error::from);
  let (a, b) = (path(a)?, path(b)?);
  // SAFETY: two C strings that live through the call.
  if unsafe { renamex_np(a.as_ptr(), b.as_ptr(), RENAME_SWAP) } == 0 {
    return Ok(());
  }

  match io::Error::last_os_error() {
    e if e.raw_os_error() == Some(ENOTSUP) => Err(io::ErrorKind::Unsupported.into()),
    e => Err(e),
  }
}

/// Opens the bundle `app` as a new instance with `args` and `note` in its [NOTE] without taking
/// the focus, by `open -n -g`: started by launchd, it is its own responsible process for TCC and
/// gets launchd's environment, not this process's, and `/` as its working directory.
/// `InvalidData` with `open`'s error if LaunchServices fails.
pub fn start(app: &Path, args: &[OsString], note: Option<&str>) -> io::Result<()> {
  let env = note.map(|n| OsString::from(format!("{NOTE}={n}")));
  let env = env.iter().flat_map(|e| ["--env".as_ref(), e.as_os_str()]);
  let head =
    ["-n", "-g"].map(OsStr::new).into_iter().chain(env).chain([app.as_os_str(), "--args".as_ref()]);
  run("/usr/bin/open", &head.chain(args.iter().map(OsString::as_os_str)).collect::<Vec<_>>())
}

/// Runs `cmd` to its end; `InvalidData` with its stderr if it failed.
fn run(cmd: &str, args: &[&OsStr]) -> io::Result<()> {
  let out = Command::new(cmd).args(args).output()?;
  match out.status.success() {
    true => Ok(()),
    false => Err(bad(format!("{cmd}: {}", String::from_utf8_lossy(&out.stderr).trim()))),
  }
}
