// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The update's process calls on macOS: the release's `.zip` unpacked with `ditto`, the new
//! bundle held to the running one's id and designated requirement with `codesign`, then opened
//! by LaunchServices in the old one's place.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2_app_kit::{NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{NSBundle, NSDictionary, NSError, NSString, NSURL};

use crate::instance::RELAUNCH;
use crate::update::bad;

/// Unpacks the `.zip` `file` into the folder `into`, as `ditto -c -k --keepParent` packed it.
pub fn unpack(file: &Path, into: &Path) -> io::Result<()> {
  run("/usr/bin/ditto", &["-x".as_ref(), "-k".as_ref(), file.as_os_str(), into.as_os_str()])
    .map(drop)
}

/// Whether the bundle `new` may replace the bundle `app`: the same bundle id, and signed so that
/// it meets `app`'s designated requirement (an ad-hoc app's is its own cdhash, which no other
/// build meets); `InvalidData` if not.
pub fn accepts(new: &Path, app: &Path) -> io::Result<()> {
  let id = |p: &Path| {
    let bundle = NSBundle::bundleWithPath(&NSString::from_str(&p.to_string_lossy()));
    bundle?.bundleIdentifier().map(|id| id.to_string())
  };
  if id(new).is_none() || id(new) != id(app) {
    return Err(bad(format!("{} is not the app of {}", new.display(), app.display())));
  }

  let req = run("/usr/bin/codesign", &["-d".as_ref(), "-r-".as_ref(), app.as_os_str()])?;
  // Implicit, as an ad-hoc signature's is, it comes as a comment: `# designated => cdhash H"…"`.
  let req = req.trim().trim_start_matches("# ").strip_prefix("designated => ");
  let req = req.ok_or_else(|| bad(format!("{} has no requirement", app.display())))?;
  let req = format!("-R={req}");
  run("/usr/bin/codesign", &["-v".as_ref(), req.as_ref(), new.as_os_str()]).map(drop)
}

/// Runs `cmd` to its end, what it wrote to stdout; `InvalidData` with its stderr if it failed.
fn run(cmd: &str, args: &[&OsStr]) -> io::Result<String> {
  let out = Command::new(cmd).args(args).output()?;
  match out.status.success() {
    true => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
    false => Err(bad(format!("{cmd}: {}", String::from_utf8_lossy(&out.stderr).trim()))),
  }
}

/// Opens the bundle `app` as a new instance beside the running one, with [RELAUNCH] set to `var`
/// and without taking the focus: started by launchd, it is its own responsible process for TCC.
/// Back with LaunchServices' error, or once a new process of the bundle's id runs: its answer
/// waits until the app has finished launching, which the new one, waiting in [crate::instance]
/// for this one to end, would only do after LaunchServices gives up (30 s). `TimedOut` when
/// neither comes in 35 s, so the swap is undone.
pub fn relaunch(app: &Path, var: &str) -> io::Result<()> {
  let path = NSString::from_str(&app.to_string_lossy());
  let id = NSBundle::bundleWithPath(&path).and_then(|b| b.bundleIdentifier());
  let apps = || {
    let apps = id.as_deref().map(NSRunningApplication::runningApplicationsWithBundleIdentifier);
    apps.map_or(Vec::new(), |a| a.to_vec())
  };
  let before = apps();
  let config = NSWorkspaceOpenConfiguration::configuration();
  config.setCreatesNewApplicationInstance(true);
  config.setActivates(false);
  let (k, v) = (NSString::from_str(RELAUNCH), NSString::from_str(var));
  config.setEnvironment(&NSDictionary::from_slices(&[&*k], &[&*v]));
  let (tx, rx) = mpsc::channel();
  let done = RcBlock::new(move |_: *mut NSRunningApplication, err: *mut NSError| {
    // SAFETY: null or an error that lives through the call.
    let _ = tx.send(unsafe { err.as_ref() }.map(|e| e.localizedDescription().to_string()));
  });

  let url = NSURL::fileURLWithPath(&path);
  let ws = NSWorkspace::sharedWorkspace();
  ws.openApplicationAtURL_configuration_completionHandler(&url, &config, Some(&done));
  let deadline = Instant::now() + Duration::from_secs(35);
  loop {
    match rx.recv_timeout(Duration::from_millis(50)) {
      Ok(None) => return Ok(()),
      Ok(Some(e)) => return Err(io::Error::other(format!("{}: {e}", app.display()))),
      Err(RecvTimeoutError::Timeout) if apps().iter().any(|a| !before.contains(a)) => return Ok(()),
      Err(RecvTimeoutError::Timeout) if Instant::now() > deadline => {
        let e = format!("{}: LaunchServices started nothing in 35 s", app.display());
        return Err(io::Error::new(io::ErrorKind::TimedOut, e));
      }
      Err(RecvTimeoutError::Timeout) => {}
      Err(RecvTimeoutError::Disconnected) => {
        return Err(io::Error::other(format!("{}: LaunchServices never answered", app.display())));
      }
    }
  }
}
