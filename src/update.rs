// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Updating an app in place, as Sparkle does: [check] fetches a signed feed and [read]s it for a
//! newer [Release], [download] fetches its file beside the running app, [Release::verify]s and
//! stages it, [install] hands it to an installer that puts it in once the app has quit, and
//! [finish], first in `main`, is that installer. They block: an app runs them on a thread of its
//! own and posts the result back.
//!
//! The feed is one text file, `update.txt`, signed, hosted anywhere: on GitHub as a release asset,
//! `https://github.com/<owner>/<repo>/releases/latest/download/update.txt`. It is UTF-8, one record
//! per line, its fields split by single spaces: exactly one `version`, per platform, keyed by
//! `<OS>-<ARCH>` from [std::env::consts], the file's size in bytes, its SHA-512 in hex and its
//! URL, and last the signature:
//!
//! ```text
//! version 1.2.0
//! macos-aarch64 394788 <sha512 hex> https://…/App-macos.dmg
//! windows-x86_64 285930 <sha512 hex> https://…/App-windows-x86_64-setup.exe
//! linux-x86_64 412204 <sha512 hex> https://…/App-x86_64.AppImage
//! sig <signature hex>
//! ```
//!
//! A line of another first word is skipped, so the format may grow; a first word repeated, or a
//! malformed `version` or line of this platform, is `InvalidData`. The download stops past the
//! size, and [download]'s progress counts to it. The feed may be 1 MiB.
//!
//! The signature is Ed25519 over every byte before the `sig` line, in 128 hex digits; that line is
//! the last and comes once, else the feed is `InvalidData`. [Feed::key] is the public key's 32
//! bytes in 64. Made with `openssl` 3, the key once and kept secret:
//!
//! ```sh
//! openssl genpkey -algorithm ed25519 -out update.pem
//! openssl pkey -in update.pem -pubout -outform DER | tail -c 32 | xxd -p -c 32  # Feed::key
//! echo "sig $(openssl pkeyutl -sign -inkey update.pem -rawin -in update.txt | xxd -p -c 64)" \
//!   >> update.txt
//! ```
//!
//! `scripts/update-feed.py` writes the feed with its `sig` line, and with `--public-key` prints
//! the key.
//!
//! Only a newer version is taken, over https (http on loopback only), and its file only when its
//! size and SHA-512 are the feed's; nothing is opened before. Redirects are followed by the system
//! (on Linux only to https, on Windows not from https to http, on macOS by the shared session's
//! defaults); the signature and the hash guard what comes. The file is the one people download
//! for the first install: the notarized `.dmg`, the per-user Inno Setup `setup.exe`, the AppImage.
//!
//! The app is never replaced while it runs. [install] starts the installer apart from the app (its
//! own process group on Unix; no console and out of the app's job on Windows), waits up to 10 s
//! for its sign and returns; the app then quits within 30 s. The installer waits for the old
//! process to end, puts the new version in (below), starts it with the old one's arguments in the
//! old one's working directory (on macOS LaunchServices starts it in `/`, as Finder does) and
//! removes the old copy and the staging folder. If that fails, it undoes the swap and starts the
//! old app again with its error in `NIMBO_UPDATE_ERROR`, which [finish] returns. What an installer
//! stopped midway left, the next [download] removes. The staging folder,
//! `.nimbo-update-<pid>-<nanos>`, is beside the app:
//!
//! | OS | the app | staged | checked | put in, and undone | started |
//! |---|---|---|---|---|---|
//! | macOS | the `.app` | the `.dmg` attached read-only inside the staging folder (`hdiutil`), its one `.app` at the top copied out with `ditto`, detached (forced if it must be) | the same bundle id, signed so that it meets the running app's designated requirement (Security.framework) | `renamex_np` with `RENAME_SWAP`, undone by the same; on a volume without it (HFS+) the app renamed to `.<name>.nimbo-old`, the new one renamed in | `open -n -g`, by LaunchServices: no focus taken, launchd's environment |
//! | Windows | the exe's folder in `%LOCALAPPDATA%\Programs` | the `setup.exe` as it came | the size and hash alone | the setup run `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CURRENTUSER` with no window, awaited; an exit code other than 0 is the error | the folder's exe of the running one's name, whatever the setup left |
//! | Linux | the AppImage | made executable | the size and hash alone | linked to `.<name>.nimbo-old`, the new one renamed over it in one step; undone by renaming the link back | the AppImage, without the installer's AppImage variables |
//!
//! On Windows the installer is a copy of the running exe, named as it is (Windows asks to elevate
//! an exe named `setup`, `update` or `install` that has no manifest), at `<staging>\<exe>`, and
//! stays there for the next [download] to remove. An exe outside `%LOCALAPPDATA%\Programs` (a
//! build) is `Unsupported`. On macOS the grants (microphone, Accessibility) survive an update only
//! when every release is signed with one stable identity: an ad-hoc signature's requirement is its
//! own build, so [download] refuses the next one.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{env, fmt, thread};

use ed25519_compact::sha512::Hash;
use ed25519_compact::{PublicKey, Signature};

use crate::paths::{OS, Os};
use crate::platform::{http, update as imp};
use crate::uninstall::{self, Env};

/// Where an app's updates come from: its feed's URL and the Ed25519 public key that signs it,
/// both from consts in the app.
#[derive(Clone, Copy, Debug)]
pub struct Feed<'a> {
  /// `update.txt`'s URL.
  pub url: &'a str,
  /// The public key's 32 bytes in 64 hex digits.
  pub key: &'a str,
}

/// A newer version the feed offers this platform, made by [check] and [read] alone.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Release {
  pub version: String,
  /// The file to download, https (or loopback).
  pub url: String,
  /// The file's size in bytes.
  pub size: u64,
  /// The file's SHA-512.
  pub sha512: [u8; 64],
}

impl Release {
  /// Checks the file downloaded for the release against its size and SHA-512: `InvalidData` if
  /// either differs.
  pub fn verify(&self, file: &Path) -> io::Result<()> {
    match fs::metadata(file)?.len() == self.size && sha512(file)? == self.sha512 {
      true => Ok(()),
      false => Err(bad(format!("{} is not the file of {}", file.display(), self.version))),
    }
  }
}

/// A release's app downloaded, verified and staged beside the running one, for [install]; its
/// folder is removed on drop, unless [install] handed it to the installer.
#[derive(Debug)]
pub struct Staged {
  /// The new app: the `.app` bundle copied from the image, the setup, the AppImage.
  new: PathBuf,
  /// The app it replaces ([target]).
  target: PathBuf,
  dir: Folder,
}

// What an app's thread sends back to its main one.
const _: () = {
  const fn send<T: Send>() {}
  send::<Staged>();
  send::<Release>();
  send::<io::Result<Option<Release>>>();
};

/// The release `feed` offers this platform when it is newer than `current`: the feed fetched over
/// the OS's HTTP, then [read].
pub fn check(feed: &Feed, current: &str) -> io::Result<Option<Release>> {
  if !secure(feed.url) {
    return Err(bad(format!("{} is not https", feed.url)));
  }

  let dir = Folder::new(&env::temp_dir())?;
  let to = dir.0.join("update.txt");
  http::get(feed.url, &to, 1 << 20, &mut |_| {})?;
  read(feed, &fs::read(to)?, current)
}

/// Downloads `release`'s file beside the running app, checks its SHA-512 (`InvalidData` if it
/// differs) and stages it: on macOS the one `.app` on the image (`InvalidData` for more or none),
/// held to the running bundle's id and designated requirement. Before a byte is fetched it fails
/// without an installed app to replace (`Unsupported`: a build, a Windows exe outside
/// `%LOCALAPPDATA%\Programs`) or one whose folder the user can't write (`PermissionDenied`:
/// `/Applications` for a standard user, `/opt`, a translocated app), and removes what an installer
/// stopped midway left ([tidy]).
///
/// `progress` gets the bytes done and the total, `release.size`, on the calling thread: at most
/// about 4 times a second while the file comes, then once with the total when it is verified.
pub fn download(release: &Release, mut progress: impl FnMut(u64, u64)) -> io::Result<Staged> {
  let exe = env::current_exe()?;
  let target = target(OS, &exe, &|k| env::var_os(k))?;
  tidy(&target, &imp::alive);
  let (dir, file) = fetch(release, target.parent().unwrap_or(&target), &mut progress)?;
  let new = imp::unpack(&file, &target)?;
  Ok(Staged { new, target, dir })
}

/// Starts the [Staged] app's installer, the new version's [finish] (on Windows this one's), and
/// returns once it runs; the app must then quit within 30 s. The installer waits for this process
/// to end, puts the new version in, starts it with this process's arguments and removes what is
/// left; an error of those comes from [finish] in the app it starts. `TimedOut` when the installer
/// gives no sign in 10 s, nothing swapped. The staging folder is the installer's from here on: on
/// an error the next [download] removes it.
pub fn install(staged: Staged) -> io::Result<()> {
  let Staged { new, target, dir } = staged;
  let (dir, exe) = (dir.keep(), env::current_exe()?);
  let name = exe.file_name().unwrap_or_default();
  let installer = match OS {
    Os::Mac => new.join("Contents/MacOS").join(name),
    // The setup replaces the exe: the installer runs from a copy of it, named as it is.
    Os::Windows => {
      let copy = dir.join(name);
      fs::copy(&exe, &copy)?;
      copy
    }
    Os::Linux => new.clone(),
  };

  let mut cmd = Command::new(installer);
  cmd.arg(INSTALL).arg(std::process::id().to_string()).arg(&new).arg(target).arg("--");
  cmd.args(env::args_os().skip(1)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
  handshake(&dir, READY, || {
    let mut child = imp::detach(&mut cmd)?;
    Ok(move || drop((child.kill(), child.wait())))
  })
}

/// The update's step at an app's launch, called first in `main`: before its arguments are parsed
/// and before any window or [crate::instance] call. Started by [install] (`--nimbo-install` the
/// first argument), it is the installer and never returns: it waits for the old app to end, puts
/// the new one in, starts it and exits (status 0, 1 on a failure, 2 for malformed arguments).
/// Otherwise the error of the installer that started this app, in [NOTE], which it removes from
/// the environment: so this runs before any thread does.
pub fn finish() -> Option<io::Error> {
  let Some(job) = job(&env::args_os().skip(1).collect::<Vec<_>>()) else {
    let note = env::var(NOTE).ok()?;
    // SAFETY: first in `main`, before any other thread runs.
    unsafe { env::remove_var(NOTE) };
    return Some(io::Error::other(note));
  };

  let Some(job) = job else { std::process::exit(2) };
  #[cfg(target_os = "macos")]
  let exchange = imp::exchange;
  #[cfg(not(target_os = "macos"))]
  let exchange = |_: &Path, _: &Path| -> io::Result<()> { Err(io::ErrorKind::Unsupported.into()) };
  #[cfg(windows)]
  let setup = imp::setup;
  #[cfg(not(windows))]
  let setup = |_: &Path| -> io::Result<i32> { Err(io::ErrorKind::Unsupported.into()) };
  let rename = |a: &Path, b: &Path| fs::rename(a, b);
  let (wait, start) = (&imp::wait_exit, &imp::start);
  let os = Seams { wait, exchange: &exchange, rename: &rename, setup: &setup, start };
  std::process::exit(apply(OS, &job, &os).map_or(1, |()| 0))
}

/// The environment variable that carries an installer's error to the app it starts.
pub(crate) const NOTE: &str = "NIMBO_UPDATE_ERROR";

/// The argument that makes [finish] the installer.
const INSTALL: &str = "--nimbo-install";
/// How long [install] waits for the installer's [MARK].
const READY: Duration = Duration::from_secs(10);
/// How long the installer waits for the old app to end.
const QUIT: Duration = Duration::from_secs(30);
/// The installer's mark in the staging folder: it runs.
const MARK: &str = "ready";

/// What an installer does: waits for `pid` to end, puts `new`, in the staging folder `dir`, in
/// `target`'s place and starts it with `args`.
#[derive(Debug, PartialEq)]
struct Job {
  pid: u32,
  new: PathBuf,
  target: PathBuf,
  dir: PathBuf,
  args: Vec<OsString>,
}

/// The installer's job `args` (the program's own skipped) ask: `None` for an app's own arguments,
/// `Some(None)` for malformed ones. They are [INSTALL] `<pid> <new> <target> -- <the app's>`,
/// `target` absolute, `new` inside a staging folder beside it.
fn job(args: &[OsString]) -> Option<Option<Job>> {
  let [flag, rest @ ..] = args else { return None };
  if flag != INSTALL {
    return None;
  }

  let [pid, new, target, dash, args @ ..] = rest else { return Some(None) };
  let pid = pid.to_str().and_then(|p| p.parse().ok());
  let (new, target) = (PathBuf::from(new), PathBuf::from(target));
  let inside =
    target.parent().filter(|_| target.is_absolute()).and_then(|p| new.strip_prefix(p).ok());
  let inside = inside.filter(|r| r.components().count() > 1);
  let inside = inside.filter(|r| r.components().all(|c| matches!(c, Component::Normal(_))));
  let dir =
    inside.and_then(|r| r.iter().next()).filter(|d| d.to_string_lossy().starts_with(STAGING));
  let dir = dir.map(|d| target.with_file_name(d));
  Some(match (pid, dir, dash == "--") {
    (Some(pid), Some(dir), true) => Some(Job { pid, new, target, dir, args: args.to_vec() }),
    _ => None,
  })
}

/// What the installer does to the system: the OS's, or a test's.
struct Seams<'a> {
  /// Whether the process ended within the time ([imp::wait_exit]).
  wait: &'a dyn Fn(u32, Duration) -> bool,
  /// Two paths exchanged in one step, `Unsupported` where they can't be: macOS' alone.
  exchange: Rename<'a>,
  rename: Rename<'a>,
  /// Windows' setup run to its end, its exit code ([imp::setup]).
  setup: &'a dyn Fn(&Path) -> io::Result<i32>,
  /// The app at a path started with arguments and an error for its [NOTE] ([imp::start]).
  start: Start<'a>,
}

/// An app started with arguments and an error for its [NOTE].
type Start<'a> = &'a dyn Fn(&Path, &[OsString], Option<&str>) -> io::Result<()>;

/// The installer's steps: its [MARK] written, the old app's end awaited (`TimedOut` past [QUIT],
/// nothing changed), the new app put in and started; on a failure the swap undone and the old app
/// started again with the error in its [NOTE]. What is left is removed either way.
fn apply(os: Os, job: &Job, s: &Seams) -> io::Result<()> {
  fs::write(job.dir.join(MARK), "")?;
  let done = match (s.wait)(job.pid, QUIT) {
    false => Err(error(io::ErrorKind::TimedOut, format!("the app didn't quit in {QUIT:?}"))),
    true => put(os, job, s).map_err(|e| {
      let e = error(e.kind(), format!("installing {}: {e}", job.target.display()));
      let _ = (s.start)(&job.target, &job.args, Some(&e.to_string()));
      e
    }),
  };

  let _ = uninstall::remove(&old(&job.target));
  if os != Os::Windows {
    let _ = uninstall::remove(&job.dir);
    return done;
  }

  // On Windows the installer runs from the staging folder, which can't go while it does: all but
  // its exe goes, and the next download removes that.
  let exe = env::current_exe().ok().and_then(|e| e.file_name().map(OsString::from));
  for e in fs::read_dir(&job.dir).into_iter().flatten().flatten() {
    if Some(e.file_name()) != exe {
      let _ = uninstall::remove(&e.path());
    }
  }

  done
}

/// The new app put in the target's place and started: Windows' setup run, an exit code other
/// than 0 an error; else swapped in, the swap undone if it doesn't start.
fn put(os: Os, job: &Job, s: &Seams) -> io::Result<()> {
  if os == Os::Windows {
    return match (s.setup)(&job.new)? {
      0 => (s.start)(&job.target, &job.args, None),
      code => Err(io::Error::other(format!("{} exited with {code}", job.new.display()))),
    };
  }

  let (rename, back) = swap(os, &job.new, &job.target, s)?;
  let undo = |e| also(e, back.iter().try_for_each(|(a, b)| uninstall::retry(|| rename(a, b))));
  (s.start)(&job.target, &job.args, None).map_err(undo)
}

/// Swaps the app `new` in for `target` whole: on macOS in one exchange, which left `new` holding
/// the old app; else (and on a volume with no exchange) `target` moved to its [old] name, on Linux
/// linked there so the rename over it replaces it in one step, and `new` renamed in, each tried
/// again while it may pass. A failing second step restores the target. What undoes it: the
/// exchange or the rename, and its steps in order.
fn swap<'a>(os: Os, new: &Path, target: &Path, s: &Seams<'a>) -> io::Result<Back<'a>> {
  let (new, target) = (new.to_owned(), target.to_owned());
  if os == Os::Mac {
    match (s.exchange)(&new, &target) {
      Err(e) if e.kind() == io::ErrorKind::Unsupported => {}
      done => return done.map(|()| (s.exchange, vec![(new, target)])),
    }
  }

  let old = old(&target);
  uninstall::remove(&old)?;
  match os {
    Os::Linux => fs::hard_link(&target, &old)?,
    _ => uninstall::retry(|| (s.rename)(&target, &old))?,
  }

  if let Err(e) = uninstall::retry(|| (s.rename)(&new, &target)) {
    // Linux' target is still there, its link removed with what is left.
    let back =
      if os == Os::Linux { Ok(()) } else { uninstall::retry(|| (s.rename)(&old, &target)) };
    return Err(also(e, back));
  }

  let back = match os {
    Os::Linux => vec![(old, target)],
    _ => vec![(target.clone(), new), (old, target)],
  };

  Ok((s.rename, back))
}

/// How a swap is undone: with what, the paths from and to in order.
type Back<'a> = (Rename<'a>, Vec<(PathBuf, PathBuf)>);
/// A rename, [fs::rename] or one that fails in tests.
type Rename<'a> = &'a dyn Fn(&Path, &Path) -> io::Result<()>;

/// The error `e`, saying so when undoing it failed too.
fn also(e: io::Error, undone: io::Result<()>) -> io::Error {
  match undone {
    Ok(()) => e,
    Err(u) => io::Error::new(e.kind(), format!("{e}, and undoing it failed: {u}")),
  }
}

/// Starts the installer with `spawn`, which gives what kills it, and waits up to `cap` for its
/// [MARK] in `dir`; `TimedOut`, the installer killed, when none comes.
fn handshake<K: FnOnce()>(
  dir: &Path,
  cap: Duration,
  spawn: impl FnOnce() -> io::Result<K>,
) -> io::Result<()> {
  let (kill, deadline) = (spawn()?, Instant::now() + cap);
  while !dir.join(MARK).exists() {
    if Instant::now() > deadline {
      kill();
      return Err(error(io::ErrorKind::TimedOut, format!("the installer gave no sign in {cap:?}")));
    }

    thread::sleep(Duration::from_millis(20));
  }

  Ok(())
}

/// Removes what an installer stopped midway left beside `target`: its [old] copy, and the staging
/// folders of processes that ended (by `alive`).
fn tidy(target: &Path, alive: &dyn Fn(u32) -> bool) {
  let all = target.parent().and_then(|p| fs::read_dir(p).ok()).into_iter().flatten().flatten();
  for e in all {
    let name = e.file_name();
    let pid = name.to_str().and_then(|n| n.strip_prefix(STAGING)?.split('-').next()?.parse().ok());
    if pid.is_some_and(|p| !alive(p)) {
      let _ = uninstall::remove(&e.path());
    }
  }

  let _ = uninstall::remove(&old(target));
}

/// The app an update replaces, of the running `exe`: the `.app` bundle on macOS and the AppImage
/// on Linux ([uninstall::app_path]'s rules), on Windows the exe's folder under
/// `%LOCALAPPDATA%\Programs`, where the per-user setup installs, whatever the case; `Unsupported`
/// for none (a build), `PermissionDenied` for a bundle macOS translocated (run from a read-only
/// copy, as it does for one downloaded and not moved).
fn target(os: Os, exe: &Path, env: Env) -> io::Result<PathBuf> {
  let folded =
    |p: &Path| -> Vec<_> { p.components().map(|c| c.as_os_str().to_ascii_lowercase()).collect() };
  let programs = env("LOCALAPPDATA").map(|l| folded(&Path::new(&l).join("Programs")));
  let under = |f: &Path| f.ancestors().skip(1).any(|a| Some(folded(a)) == programs);
  let target = match os {
    Os::Windows => exe.parent().filter(|f| under(f)).map(Path::to_owned),
    _ => uninstall::app_of(os, exe, env),
  };

  let what = format!("{} is not an installed app", exe.display());
  match target {
    None => Err(error(io::ErrorKind::Unsupported, what)),
    Some(t) if t.to_string_lossy().contains("/AppTranslocation/") => {
      Err(error(io::ErrorKind::PermissionDenied, format!("{what}: it runs translocated")))
    }
    Some(t) => Ok(t),
  }
}

/// [download]'s progress: the bytes done, the total.
type Progress<'a> = &'a mut dyn FnMut(u64, u64);

/// How often [download]'s progress is told at most.
const TICK: Duration = Duration::from_millis(250);

/// The release's file fetched over https (or loopback) and verified into a fresh folder under
/// `parent`, gone again on an error; one the user can't write fails first. `progress` as
/// [download]'s.
fn fetch(release: &Release, parent: &Path, progress: Progress) -> io::Result<(Folder, PathBuf)> {
  if !secure(&release.url) {
    return Err(bad(format!("{} is not https", release.url)));
  }

  let dir = Folder::new(parent)?;
  let file = dir.0.join(match OS {
    Os::Mac => "update.dmg",
    Os::Windows => "setup.exe",
    Os::Linux => "app.AppImage",
  });
  let (total, mut told) = (release.size, None::<Instant>);
  let mut tell = |done| {
    if 0 < done && done < total && told.is_none_or(|t| t.elapsed() >= TICK) {
      told = Some(Instant::now());
      progress(done, total);
    }
  };
  http::get(&release.url, &file, total, &mut tell)?;
  release.verify(&file)?;
  progress(total, total);
  Ok((dir, file))
}

/// Where a swap moves what it replaces: `.<name>.nimbo-old` beside it, a suffix only nimbo makes,
/// so cleanup removes no user's `.old`.
fn old(target: &Path) -> PathBuf {
  let mut name = OsString::from(".");
  name.push(target.file_name().unwrap_or_default());
  name.push(".nimbo-old");
  target.with_file_name(name)
}

/// A staging folder's name before `<pid>-<nanos>`, the pid of the app that made it.
const STAGING: &str = ".nimbo-update-";

/// A new folder that only the user reaches (0700 on Unix), removed with what it holds on drop.
/// Made with `create_dir`, so one already there (a link planted in a shared `/tmp`) is an error.
#[derive(Debug)]
struct Folder(PathBuf);

impl Folder {
  fn new(parent: &Path) -> io::Result<Self> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let dir = parent.join(format!("{STAGING}{}-{nanos}", std::process::id()));
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut made = fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut made, 0o700);
    made.create(&dir).map_err(|e| error(e.kind(), format!("{}: {e}", parent.display())))?;
    Ok(Self(dir))
  }

  /// Its path, the folder kept: the empty path left behind can't be removed.
  fn keep(mut self) -> PathBuf {
    std::mem::take(&mut self.0)
  }
}

impl Drop for Folder {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.0);
  }
}

/// The release a fetched feed offers this platform when it is newer than `current`, `text` the
/// feed's bytes. Its `sig` line is checked first (`InvalidData` if it is not the last line, comes
/// twice or is wrong), the feed read after; a version not newer, or none for this platform, is
/// `None`.
pub fn read(feed: &Feed, text: &[u8], current: &str) -> io::Result<Option<Release>> {
  let (data, sig) = signed(text)?;
  verify(feed.key, data, sig)?;
  release(data, current, &format!("{}-{}", env::consts::OS, env::consts::ARCH))
}

/// The feed `text` split at its last line, `sig <signature>`: the bytes before that line and the
/// signature's hex; `InvalidData` if the last line is not that or another line is too.
fn signed(text: &[u8]) -> io::Result<(&[u8], &str)> {
  let text = std::str::from_utf8(text).map_err(|_| bad("the feed is not UTF-8"))?;
  let end = text.strip_suffix('\n').unwrap_or(text);
  let start = end.rfind('\n').map_or(0, |i| i + 1);
  let (data, last) = (&text[..start], &end[start..]);
  let sig = last.strip_suffix('\r').unwrap_or(last).strip_prefix("sig ");
  match sig.filter(|_| !data.lines().any(|l| l.split(' ').next() == Some("sig"))) {
    Some(sig) => Ok((data.as_bytes(), sig)),
    None => Err(bad("the feed's last line is not its one `sig` line")),
  }
}

/// Checks `sig`, the hex Ed25519 signature of `data`, by `key`.
fn verify(key: &str, data: &[u8], sig: &str) -> io::Result<()> {
  let key = hex(key).map(PublicKey::new);
  let key = key.ok_or_else(|| error(io::ErrorKind::InvalidInput, "a bad key"))?;
  let sig = hex(sig).map(Signature::new);
  sig.and_then(|s| key.verify(data, &s).ok()).ok_or_else(|| bad("the feed's signature is wrong"))
}

/// The SHA-512 of the file at `path`, read in chunks.
fn sha512(path: &Path) -> io::Result<[u8; 64]> {
  let (mut file, mut hash, mut buf) = (File::open(path)?, Hash::new(), vec![0; 1 << 16]);
  loop {
    match file.read(&mut buf) {
      Ok(0) => return Ok(hash.finalize()),
      Ok(n) => hash.update(&buf[..n]),
      Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
      Err(e) => return Err(e),
    }
  }
}

/// The release in the feed `text` for `platform` when its version is newer than `current`: each
/// line's first word once, `version` one more, the platform's `<size> <sha512> <url>`; other
/// words skipped.
fn release(text: &[u8], current: &str, platform: &str) -> io::Result<Option<Release>> {
  let text = std::str::from_utf8(text).map_err(|_| bad("the feed is not UTF-8"))?;
  let (mut seen, mut version, mut file) = (Vec::new(), None, None);
  for line in text.lines() {
    let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
    if word.is_empty() || seen.contains(&word) {
      return Err(bad(format!("the feed's line {line:?} is empty or repeated")));
    }

    seen.push(word);
    match word {
      "version" => version = Some(rest),
      _ if word == platform => file = Some(rest),
      _ => {}
    }
  }

  let version = version.filter(|v| !v.is_empty() && !v.contains(' '));
  let version = version.ok_or_else(|| bad("the feed has no version, or a bad one"))?;
  match newer(version, current) {
    None => return Err(bad(format!("versions {version} and {current} don't compare"))),
    Some(false) => return Ok(None),
    Some(true) => {}
  }

  let Some(file) = file else { return Ok(None) };
  let what = || bad(format!("the feed's {platform} line is not <size> <sha512> <url>"));
  let [size, sha512, url] = file.split(' ').collect::<Vec<_>>()[..] else { return Err(what()) };
  let size = size.parse().ok().filter(|_| size.bytes().all(|b| b.is_ascii_digit()));
  let (Some(size), Some(sha512)) = (size, hex(sha512)) else { return Err(what()) };
  if !secure(url) {
    return Err(bad(format!("{url} is not https")));
  }

  Ok(Some(Release { version: version.into(), url: url.into(), size, sha512 }))
}

/// `url` is https, or http on [loopback] (tests, a VM serving from its own host).
fn secure(url: &str) -> bool {
  url.starts_with("https://") || loopback(url)
}

/// `url` is http to this machine: `127.0.0.1`, `localhost` or `[::1]`.
pub(crate) fn loopback(url: &str) -> bool {
  let Some(rest) = url.strip_prefix("http://") else { return false };
  let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
  let host = match host.rsplit_once(':') {
    Some((h, port)) if port.bytes().all(|b| b.is_ascii_digit()) => h,
    _ => host,
  };

  ["127.0.0.1", "localhost", "[::1]"].contains(&host)
}

/// A pre-release's dot part: numbers below words, as semver orders them.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Part<'a> {
  Num(u64),
  Word(&'a str),
}

/// Whether version `v` comes after `than`: numeric dot parts (`1.10` after `1.9`, `1.2` equal to
/// `1.2.0`), then a release after its pre-releases (`1.2.0-dev`); a leading `v` and `+build`
/// are ignored. `None` if one isn't a version.
fn newer(v: &str, than: &str) -> Option<bool> {
  fn key(v: &str) -> Option<(Vec<u64>, bool, Vec<Part<'_>>)> {
    let v = v.trim().trim_start_matches('v').split('+').next()?;
    let (core, pre) = v.split_once('-').map_or((v, None), |(c, p)| (c, Some(p)));
    let mut nums = core.split('.').map(|n| n.parse().ok()).collect::<Option<Vec<u64>>>()?;
    while nums.last() == Some(&0) {
      nums.pop();
    }

    let parts = pre.unwrap_or_default().split('.');
    Some((nums, pre.is_none(), parts.map(|p| p.parse().map_or(Part::Word(p), Part::Num)).collect()))
  }

  Some(key(v)? > key(than)?)
}

/// `N` bytes from exactly `2 * N` hex digits, of either case.
fn hex<const N: usize>(s: &str) -> Option<[u8; N]> {
  if s.len() != 2 * N || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
    return None;
  }

  let bytes = (0..N).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok());
  bytes.collect::<Option<Vec<u8>>>()?.try_into().ok()
}

/// An `InvalidData` error of the update.
pub(crate) fn bad(what: impl fmt::Display) -> io::Error {
  error(io::ErrorKind::InvalidData, what)
}

/// An error of the update of the kind `kind`.
fn error(kind: io::ErrorKind, what: impl fmt::Display) -> io::Error {
  io::Error::new(kind, format!("update: {what}"))
}

#[cfg(test)]
mod tests {
  use std::cell::{Cell, RefCell};

  use super::*;

  /// A feed signed with `openssl pkeyutl -sign -rawin` by the key whose public half is [KEY]; its
  /// `tags` line is a word nimbo doesn't know.
  const FEED: &[u8] = concat!(
    "version 1.2.0\n",
    "tags beta\n",
    "macos-aarch64 3 ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f https://example.com/app.zip\n",
    "linux-x86_64 3 ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f https://example.com/app.AppImage\n",
    "windows-x86_64 3 ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f https://example.com/app.zip\n",
    "test-http 3 ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f http://example.com/app.zip\n",
    "sig e965a7ea705bcf508b05a6c8ba1c74bf3737f91f0da936a7b5ce4b729a17f67dce041429b42008d440c5715c70fd1ae473e1e179b4b76b8641748b095442b105\n",
  )
  .as_bytes();
  /// The last 32 bytes of `openssl pkey -pubout -outform DER`, in hex.
  const KEY: &str = "6bf026c9174d547c1d68f3ec8267273426167628161897f3661a4677a6c24b6f";
  /// [FEED]'s lines before `sig` ended in CRLF, signed so.
  const CRLF_SIG: &str = "894a66f5a20e1013bef7720bdcfd2ecad93f289bd483adaca9f7ead1a450201e23f4282df9f4cc03c361ddac43db127f8be20ba9bb2a32779d9ffb61adc2e008";
  /// The SHA-512 of `abc` (FIPS 180-2).
  const ABC: &str = "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f";
  /// RFC 8032's test 1: the public key and the signature of the empty message.
  const RFC_KEY: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
  const RFC_SIG: &str = "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b";

  fn temp(name: &str, data: &[u8]) -> PathBuf {
    let path = env::temp_dir().join(format!("nimbo-update-{}-{name}", std::process::id()));
    fs::write(&path, data).unwrap();
    path
  }

  /// A new empty folder in the temp dir.
  fn dir(name: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("nimbo-update-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
  }

  /// Writes each `(path, text)` under `root`, with its folders.
  fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
      let path = root.join(path);
      fs::create_dir_all(path.parent().unwrap()).unwrap();
      fs::write(path, text).unwrap();
    }
  }

  /// Whether each `(path, text)` under `root` holds its text, `""` for one that is not there.
  fn holds(root: &Path, files: &[(&str, &str)], when: &str) {
    for (path, text) in files {
      assert_eq!(fs::read_to_string(root.join(path)).unwrap_or_default(), *text, "{path} {when}");
    }
  }

  #[test]
  fn the_target_is_the_installed_app() {
    let none = |_: &str| None;
    let target = |os, exe: &str| target(os, Path::new(exe), &none);
    let app = target(Os::Mac, "/Applications/A.app/Contents/MacOS/a").unwrap();
    assert_eq!(app, Path::new("/Applications/A.app"));
    let moved = "/private/var/folders/x/T/AppTranslocation/1-2/d/A.app/Contents/MacOS/a";
    assert_eq!(target(Os::Mac, moved).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    let windows = "C:/Users/Me/AppData/Local/Programs/W/a.exe";
    for (os, exe) in [(Os::Mac, "/Users/me/a/target/debug/a"), (Os::Linux, "/usr/bin/a")] {
      assert_eq!(target(os, exe).unwrap_err().kind(), io::ErrorKind::Unsupported);
    }

    assert_eq!(target(Os::Windows, windows).unwrap_err().kind(), io::ErrorKind::Unsupported);
  }

  #[test]
  fn a_windows_app_is_one_the_setup_installed() {
    let local = |k: &str| (k == "LOCALAPPDATA").then(|| "C:/Users/Me/AppData/Local".into());
    let target = |exe: &str| target(Os::Windows, Path::new(exe), &local);
    let programs = "C:/Users/Me/AppData/Local/Programs";
    assert_eq!(target(&format!("{programs}/W/a.exe")).unwrap(), Path::new(programs).join("W"));
    let case = "c:/users/me/appdata/local/PROGRAMS/W";
    assert_eq!(target(&format!("{case}/a.exe")).unwrap(), Path::new(case), "whatever the case");
    assert!(target(&format!("{programs}/V/W/a.exe")).is_ok(), "a folder deeper");
    for exe in [
      format!("{programs}/a.exe"),
      format!("{programs}X/W/a.exe"),
      "C:/Users/Me/src/w/target/release/a.exe".into(),
      "C:/Program Files/W/a.exe".into(),
    ] {
      assert_eq!(target(&exe).unwrap_err().kind(), io::ErrorKind::Unsupported, "{exe}");
    }
  }

  #[cfg(unix)]
  #[test]
  fn a_read_only_folder_is_refused_before_downloading() {
    use std::os::unix::fs::PermissionsExt;
    let root = dir("read-only");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o555)).unwrap();
    let url = "http://127.0.0.1:9/a".into();
    let r = Release { version: "2".into(), url, size: 1, sha512: [0; 64] };
    let e = fetch(&r, &root, &mut |_, _| {}).unwrap_err();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(e.kind(), io::ErrorKind::PermissionDenied, "{e}");
    fs::remove_dir(root).unwrap();
  }

  /// macOS' exchange as three renames, through a name of its own.
  fn exchange(a: &Path, b: &Path) -> io::Result<()> {
    let via = a.with_extension("via");
    fs::rename(a, &via).and_then(|()| fs::rename(b, a)).and_then(|()| fs::rename(&via, b))
  }

  fn rename(a: &Path, b: &Path) -> io::Result<()> {
    fs::rename(a, b)
  }

  fn unsupported(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
  }

  /// The OS's renames, the exchange, an app that quits, a setup that installs, an app that starts.
  const SEAMS: Seams = Seams {
    wait: &|_, _| true,
    exchange: &exchange,
    rename: &rename,
    setup: &|_| Ok(0),
    start: &|_, _, _| Ok(()),
  };

  /// What a start was given: the app, its arguments, the note.
  type Started = RefCell<Vec<(PathBuf, Vec<OsString>, Option<String>)>>;

  /// A start that records what it was given in `started`, failing the first time if `fail`.
  fn starts(
    started: &Started,
    fail: bool,
  ) -> impl Fn(&Path, &[OsString], Option<&str>) -> io::Result<()> {
    move |app, args, note| {
      started.borrow_mut().push((app.to_owned(), args.to_vec(), note.map(String::from)));
      match fail && started.borrow().len() == 1 {
        true => Err(io::Error::other("no start")),
        false => Ok(()),
      }
    }
  }

  /// The installer's arguments of `pid`, `new` and `target` under `root`, the app's `-x`.
  fn args(root: &Path, pid: &str, new: &str, target: &str) -> Vec<OsString> {
    let (new, target) = (root.join(new).into_os_string(), root.join(target).into_os_string());
    vec![INSTALL.into(), pid.into(), new, target, "--".into(), "-x".into()]
  }

  /// The installer's job of `new` and `target` under `root`.
  fn install_job(root: &Path, new: &str, target: &str) -> Job {
    job(&args(root, "7", new, target)).unwrap().unwrap()
  }

  /// A macOS and a Linux app under `root` and their new versions in a staging folder: a bundle
  /// holding a file `gone` that the new version drops, and an AppImage.
  fn apps(root: &Path) -> [(Os, &'static str, &'static str); 2] {
    write(root, &[("A.app/Contents/MacOS/a", "1"), ("A.app/gone", "1"), ("A.AppImage", "1")]);
    write(
      root,
      &[(".nimbo-update-9/new/A.app/Contents/MacOS/a", "2"), (".nimbo-update-9/app", "2")],
    );
    [
      (Os::Mac, ".nimbo-update-9/new/A.app", "A.app"),
      (Os::Linux, ".nimbo-update-9/app", "A.AppImage"),
    ]
  }

  /// Whether `os`'s app under `root` is the new version (`new`) or the old.
  fn installed(root: &Path, os: Os, new: bool, when: &str) {
    let (v, gone) = if new { ("2", "") } else { ("1", "1") };
    let files = match os {
      Os::Linux => vec![("A.AppImage", v)],
      _ => vec![("A.app/Contents/MacOS/a", v), ("A.app/gone", gone)],
    };

    holds(root, &files, &format!("{os:?} {when}"));
  }

  #[test]
  fn each_os_swaps_the_whole_app() {
    let root = dir("swap");
    for (os, new, target) in apps(&root) {
      swap(os, &root.join(new), &root.join(target), &SEAMS).unwrap();
      installed(&root, os, true, "swapped");
    }

    let files = [
      (".nimbo-update-9/new/A.app/gone", "1"),
      (".A.AppImage.nimbo-old", "1"),
      (".nimbo-update-9/app", ""),
    ];
    holds(&root, &files, "the old apps aside");
    fs::remove_dir_all(root).unwrap();
  }

  #[test]
  fn macos_renames_twice_without_the_exchange() {
    let root = dir("fallback");
    let [(os, new, target), _] = apps(&root);
    let seams = Seams { exchange: &unsupported, ..SEAMS };
    let (_, back) = swap(os, &root.join(new), &root.join(target), &seams).unwrap();
    holds(&root, &[("A.app/gone", ""), (".A.app.nimbo-old/gone", "1")], "renamed");
    assert_eq!(back.len(), 2, "both renames undone");
    fs::remove_dir_all(root).unwrap();
  }

  #[test]
  fn a_failing_second_rename_restores_the_target() {
    let root = dir("second");
    for (os, new, target) in apps(&root) {
      // Linux links first, then renames once.
      let n = Cell::new(0);
      let fail = |a: &Path, b: &Path| {
        n.set(n.get() + 1);
        if n.get() == 2 || os == Os::Linux {
          Err(io::Error::other("busy"))
        } else {
          fs::rename(a, b)
        }
      };
      let seams = Seams { exchange: &unsupported, rename: &fail, ..SEAMS };
      let e = swap(os, &root.join(new), &root.join(target), &seams).map(drop).unwrap_err();
      assert_eq!(e.to_string(), "busy", "{os:?}");
      installed(&root, os, false, "restored");
    }

    holds(&root, &[(".nimbo-update-9/app", "2")], "");
    fs::remove_dir_all(root).unwrap();
  }

  #[test]
  fn a_failing_start_undoes_the_swap_and_starts_the_old_app() {
    for i in 0..2 {
      let root = dir(&format!("start-{i}"));
      let (os, new, target) = apps(&root)[i];
      let started = Started::default();
      let job = install_job(&root, new, target);
      let e = apply(os, &job, &Seams { start: &starts(&started, true), ..SEAMS }).unwrap_err();
      assert!(e.to_string().contains("no start"), "{e}");
      installed(&root, os, false, "undone");
      let start = |note| (job.target.clone(), vec![OsString::from("-x")], note);
      assert_eq!(*started.borrow(), [start(None), start(Some(e.to_string()))], "the note");
      assert!(!old(&job.target).exists() && !job.dir.exists(), "{os:?}: what is left");
      fs::remove_dir_all(root).unwrap();
    }
  }

  #[cfg(unix)]
  #[test]
  fn an_app_in_a_read_only_folder_still_gets_its_error() {
    use std::os::unix::fs::PermissionsExt;
    let root = dir("read-only-install");
    for (os, new, target) in apps(&root) {
      let (started, job) = (Started::default(), install_job(&root, new, target));
      fs::set_permissions(&root, fs::Permissions::from_mode(0o555)).unwrap();
      let e = apply(os, &job, &Seams { start: &starts(&started, false), ..SEAMS }).unwrap_err();
      fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
      assert_eq!(e.kind(), io::ErrorKind::PermissionDenied, "{os:?}: {e}");
      assert_eq!(started.borrow()[0].2, Some(e.to_string()), "{os:?}");
      installed(&root, os, false, "unchanged");
    }

    fs::remove_dir_all(root).unwrap();
  }

  #[test]
  fn an_install_waits_for_the_app_then_swaps_and_starts_it() {
    for i in 0..2 {
      let root = dir(&format!("install-{i}"));
      let (os, new, target) = apps(&root)[i];
      let (job, waited, started) =
        (install_job(&root, new, target), Cell::new(None), Started::default());
      let wait = |pid, cap| {
        waited.set(Some((pid, cap, job.dir.join(MARK).exists())));
        true
      };
      apply(os, &job, &Seams { wait: &wait, start: &starts(&started, false), ..SEAMS }).unwrap();
      assert_eq!(waited.get(), Some((7, QUIT, true)), "the mark written first");
      installed(&root, os, true, "installed");
      assert_eq!(*started.borrow(), [(job.target.clone(), vec![OsString::from("-x")], None)]);
      assert!(!old(&job.target).exists() && !job.dir.exists(), "{os:?}: what is left");
      fs::remove_dir_all(root).unwrap();
    }
  }

  #[test]
  fn windows_runs_the_setup_then_starts_what_it_left() {
    // The installer's exe, which stays for the next download.
    let exe = env::current_exe().unwrap().file_name().unwrap().to_owned();
    for (i, code) in [Some(0), Some(2), None].into_iter().enumerate() {
      let root = dir(&format!("setup-{i}"));
      write(
        &root,
        &[("W/a.exe", "1"), (".nimbo-update-9/setup.exe", "2"), (".nimbo-update-9/ready", "")],
      );
      fs::write(root.join(".nimbo-update-9").join(&exe), "1").unwrap();
      let (job, ran, started) = (
        install_job(&root, ".nimbo-update-9/setup.exe", "W"),
        RefCell::new(Vec::new()),
        Started::default(),
      );
      let setup = |exe: &Path| {
        ran.borrow_mut().push(exe.to_owned());
        code.ok_or_else(|| io::Error::other("no setup"))
      };
      let done = apply(
        Os::Windows,
        &job,
        &Seams { setup: &setup, start: &starts(&started, false), ..SEAMS },
      );
      assert_eq!(*ran.borrow(), std::slice::from_ref(&job.new));
      let note = done.err().map(|e| e.to_string());
      let why = ["", "setup.exe exited with 2", "no setup"][i];
      assert!(note.as_ref().map_or(i == 0, |n| i > 0 && n.contains(why)), "{note:?}");
      assert_eq!(*started.borrow(), [(job.target.clone(), vec![OsString::from("-x")], note)]);
      let left =
        fs::read_dir(&job.dir).unwrap().flatten().map(|e| e.file_name()).collect::<Vec<_>>();
      assert_eq!(left, std::slice::from_ref(&exe), "all but the installer's exe goes");
      fs::remove_dir_all(root).unwrap();
    }
  }

  #[test]
  fn an_app_that_doesnt_quit_changes_nothing() {
    let root = dir("stays");
    let [(os, new, target), _] = apps(&root);
    let (job, started) = (install_job(&root, new, target), Started::default());
    let seams = Seams { wait: &|_, _| false, start: &starts(&started, false), ..SEAMS };
    assert_eq!(apply(os, &job, &seams).unwrap_err().kind(), io::ErrorKind::TimedOut);
    installed(&root, os, false, "after the wait");
    assert!(started.borrow().is_empty() && !job.dir.exists());
    fs::remove_dir_all(root).unwrap();
  }

  #[test]
  fn the_installers_arguments() {
    let root = env::temp_dir();
    let job = install_job(&root, ".nimbo-update-1-2/new/A.app", "A.app");
    assert_eq!((job.pid, &job.new), (7, &root.join(".nimbo-update-1-2/new/A.app")));
    assert_eq!((&job.dir, &job.args[..]), (&root.join(".nimbo-update-1-2"), &["-x".into()][..]));
    assert_eq!(super::job(&["-x".into()]), None, "an app's own");
    assert_eq!(super::job(&[]), None);
    let new = ".nimbo-update-1/new/A.app";
    let mut no_dash = args(&root, "7", new, "A.app");
    no_dash[4] = "-x".into();
    let relative = [INSTALL, "7", new, "A.app", "--"].map(OsString::from).to_vec();
    for args in [
      args(&root, "x7", new, "A.app"),
      args(&root, "-7", new, "A.app"),
      args(&root, "7", "other/new/A.app", "A.app"),
      args(&root, "7", ".nimbo-update-1", "A.app"),
      args(&root, "7", ".nimbo-update-1/../A.app", "A.app"),
      args(&root, "7", new, "A.app")[..4].to_vec(),
      no_dash,
      relative,
    ] {
      assert_eq!(super::job(&args), Some(None), "{args:?}");
    }
  }

  #[test]
  fn download_removes_what_an_ended_installer_left() {
    let root = dir("tidy");
    write(&root, &[(".nimbo-update-1-5/x", ""), (".nimbo-update-2-5/x", "")]);
    write(&root, &[(".A.app.nimbo-old/x", ""), ("other/x", "")]);
    tidy(&root.join("A.app"), &|pid| pid == 2);
    let names = [".nimbo-update-1-5", ".nimbo-update-2-5", ".A.app.nimbo-old", "other"];
    assert_eq!(names.map(|n| root.join(n).exists()), [false, true, false, true]);
    fs::remove_dir_all(root).unwrap();
  }

  #[test]
  fn install_waits_for_the_installers_mark() {
    let (root, cap, killed) = (dir("mark"), Duration::from_millis(100), Cell::new(false));
    let e = handshake(&root, cap, || Ok(|| killed.set(true))).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::TimedOut);
    assert!(killed.get() && root.exists(), "killed, the staging folder kept");
    killed.set(false);
    let mark = root.join(MARK);
    handshake(&root, READY, || {
      thread::spawn(move || fs::write(mark, ""));
      Ok(|| killed.set(true))
    })
    .unwrap();
    assert!(!killed.get(), "a running installer is left alone");
    fs::remove_dir_all(root).unwrap();
  }

  /// A bundle of the id `id` at `app`, signed ad hoc; `build` tells two builds apart.
  #[cfg(target_os = "macos")]
  fn bundle(app: &Path, id: &str, build: &str) {
    let plist = format!(
      "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key>\
       <string>{id}</string><key>CFBundleExecutable</key><string>a</string>\
       <key>CFBundleVersion</key><string>{build}</string></dict></plist>"
    );
    write(app, &[("Contents/Info.plist", &plist)]);
    fs::create_dir(app.join("Contents/MacOS")).unwrap();
    fs::copy("/usr/bin/true", app.join("Contents/MacOS/a")).unwrap();
    let mut sign = std::process::Command::new("/usr/bin/codesign");
    assert!(sign.args(["-s", "-", "-f"]).arg(app).output().unwrap().status.success());
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn a_bundle_must_be_the_app_signed_alike() {
    let root = dir("bundle");
    let app = root.join("A.app");
    bundle(&app, "com.example.a", "1");
    // Another build signed ad hoc meets no requirement but its own; another app is refused.
    bundle(&root.join("v2/A.app"), "com.example.a", "2");
    bundle(&root.join("b/A.app"), "com.example.b", "1");
    for other in ["v2/A.app", "b/A.app"] {
      let e = imp::accepts(&root.join(other), &app).unwrap_err();
      assert_eq!(e.kind(), io::ErrorKind::InvalidData, "{other}: {e}");
    }

    // The exchange on the temp dir's APFS: each path holds the other's bundle.
    let plist = |p: &str| fs::read_to_string(root.join(p).join("Contents/Info.plist")).unwrap();
    let (v1, v2) = (plist("A.app"), plist("v2/A.app"));
    imp::exchange(&app, &root.join("v2/A.app")).unwrap();
    assert_eq!((plist("A.app"), plist("v2/A.app")), (v2, v1));
    fs::remove_dir_all(root).unwrap();
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn a_disk_image_gives_its_one_app() {
    use std::process::Command;
    let root = dir("dmg");
    let app = root.join("A.app");
    bundle(&app, "com.example.a", "1");
    // A staging folder `name` with `update.dmg` made of `apps`, copies of the app, and a file.
    let image = |name: &str, apps: &[&str]| {
      let src = root.join(name).join("src");
      write(&src, &[("readme", "")]);
      for a in apps {
        assert!(
          Command::new("/usr/bin/ditto").arg(&app).arg(src.join(a)).status().unwrap().success()
        );
      }

      let dmg = root.join(name).join("update.dmg");
      let mut make = Command::new("/usr/bin/hdiutil");
      make.args(["create", "-quiet", "-format", "UDZO", "-srcfolder"]).arg(src).arg(&dmg);
      assert!(make.status().unwrap().success());
      dmg
    };

    let new = imp::unpack(&image("one", &["A.app"]), &app).unwrap();
    assert_eq!(new, root.join("one/new/A.app"));
    let plist = |p: &Path| fs::read(p.join("Contents/Info.plist")).unwrap();
    assert_eq!(plist(&new), plist(&app), "copied out");
    imp::accepts(&new, &app).unwrap();
    for (name, apps) in [("one", &[][..]), ("two", &["A.app", "B.app"]), ("none", &[])] {
      if name != "one" {
        let e = imp::unpack(&image(name, apps), &app).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData, "{name}: {e}");
      }

      assert!(!root.join(name).join("volume/readme").exists(), "{name}: detached");
    }

    let not_image = imp::unpack(&app.join("Contents/Info.plist"), &app).unwrap_err();
    assert_eq!(not_image.kind(), io::ErrorKind::InvalidData);
    fs::remove_dir_all(root).unwrap();
  }

  #[cfg(target_os = "linux")]
  #[test]
  fn an_appimage_is_made_executable() {
    use std::os::unix::fs::PermissionsExt;
    let root = dir("appimage");
    write(&root, &[("app.AppImage", "2")]);
    let new = imp::unpack(&root.join("app.AppImage"), &root).unwrap();
    assert_eq!(fs::metadata(new).unwrap().permissions().mode() & 0o777, 0o755);
    fs::remove_dir_all(root).unwrap();
  }

  /// A server on loopback, its URL: `/update.txt` a 302 to `/feed/update.txt`, which with `/abc`
  /// it serves; `/bad.txt` a feed changed after it was signed; anything else a 404.
  fn serve() -> String {
    use std::io::Write;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let mut bad = FEED.to_vec();
    bad[20] ^= 1;
    std::thread::spawn(move || {
      for mut s in listener.incoming().flatten() {
        let (mut req, mut buf) = (Vec::new(), [0; 1024]);
        while !req.ends_with(b"\r\n\r\n") {
          match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => req.extend_from_slice(&buf[..n]),
          }
        }

        let req = String::from_utf8_lossy(&req);
        let (status, body): (_, &[u8]) = match req.split(' ').nth(1).unwrap_or_default() {
          "/update.txt" => ("302 Found\r\nLocation: /feed/update.txt", b""),
          "/feed/update.txt" => ("200 OK", FEED),
          "/bad.txt" => ("200 OK", bad.as_slice()),
          "/abc" => ("200 OK", b"abc"),
          _ => ("404 Not Found", b"none"),
        };

        let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n", body.len());
        let _ = s.write_all(head.as_bytes()).and_then(|_| s.write_all(body));
      }
    });

    url
  }

  #[test]
  fn get_follows_a_redirect_and_fails_on_a_404() {
    let (url, to) = (serve(), temp("get", b""));
    let mut seen = Vec::new();
    http::get(&format!("{url}/update.txt"), &to, FEED.len() as u64, &mut |n| seen.push(n)).unwrap();
    assert_eq!(fs::read(&to).unwrap(), FEED);
    assert!(seen.is_sorted() && seen.last() == Some(&(FEED.len() as u64)), "{seen:?}");
    let none = &mut |_| {};
    let missing = http::get(&format!("{url}/none"), &to, 1 << 10, none).unwrap_err();
    assert_eq!(missing.kind(), io::ErrorKind::NotFound, "{missing}");
    let big = http::get(&format!("{url}/update.txt"), &to, FEED.len() as u64 - 1, none);
    let big = big.unwrap_err();
    assert_eq!(big.kind(), io::ErrorKind::InvalidData, "{big}");
    let _ = fs::remove_file(to);
  }

  #[test]
  fn check_and_download_over_http() {
    let url = serve();
    let feed = Feed { url: &format!("{url}/update.txt"), key: KEY };
    let platform = format!("{}-{}", env::consts::OS, env::consts::ARCH);
    assert_eq!(check(&feed, "1.0").unwrap(), release(FEED, "1.0", &platform).unwrap());
    assert_eq!(check(&feed, "1.2.0").unwrap(), None);
    let feed = Feed { url: &format!("{url}/none.txt"), key: KEY };
    assert_eq!(check(&feed, "1.0").unwrap_err().kind(), io::ErrorKind::NotFound);
    let feed = Feed { url: "http://example.com/update.txt", key: KEY };
    assert_eq!(check(&feed, "1.0").unwrap_err().kind(), io::ErrorKind::InvalidData);
    let feed = Feed { url: &format!("{url}/bad.txt"), key: KEY };
    assert_eq!(check(&feed, "1.0").unwrap_err().kind(), io::ErrorKind::InvalidData);

    let parent = dir("parent");
    let mut r = Release {
      version: "2".into(),
      url: format!("{url}/abc"),
      size: 3,
      sha512: hex(ABC).unwrap(),
    };
    let mut seen = Vec::new();
    let (staged, file) = fetch(&r, &parent, &mut |done, total| seen.push((done, total))).unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"abc");
    assert_eq!(seen, [(3, 3)], "only the end within a tick");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      let mode = fs::metadata(&staged.0).unwrap().permissions().mode();
      assert_eq!(mode & 0o777, 0o700);
    }

    drop(staged);
    // Shorter than the feed says, longer, another file, not https.
    let changes: [fn(&mut Release); 4] = [
      |r| r.size = 4,
      |r| r.size = 2,
      |r| r.sha512[0] ^= 1,
      |r| r.url = "http://example.com/abc".into(),
    ];
    for change in changes {
      let mut r = r.clone();
      change(&mut r);
      let e = fetch(&r, &parent, &mut |_, _| {}).unwrap_err();
      assert_eq!(e.kind(), io::ErrorKind::InvalidData, "{r:?}");
    }

    r.url = format!("{url}/none");
    assert_eq!(fetch(&r, &parent, &mut |_, _| {}).unwrap_err().kind(), io::ErrorKind::NotFound);
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 0, "no folder is left");
    fs::remove_dir(parent).unwrap();
  }

  #[test]
  fn a_feed_gives_its_release() {
    let r = release(FEED, "1.1.9", "macos-aarch64").unwrap().unwrap();
    assert_eq!((r.version.as_str(), r.url.as_str()), ("1.2.0", "https://example.com/app.zip"));
    assert_eq!((r.size, r.sha512), (3, hex(ABC).unwrap()));
    let crlf = format!("version 2\r\np 1 {ABC} https://a\r\n");
    assert_eq!(release(crlf.as_bytes(), "1", "p").unwrap().unwrap().url, "https://a", "CRLF");
  }

  #[test]
  fn no_file_for_the_platform_is_no_update() {
    assert_eq!(release(FEED, "1.0", "plan9-mips").unwrap(), None);
  }

  #[test]
  fn a_malformed_feed_is_refused() {
    let line = |file: &str| format!("version 2\np {file}\n");
    let files =
      ["1 00 https://a", "-1 {ABC} https://a", "+1 {ABC} https://a", "1.5 {ABC} https://a"];
    let more = ["{ABC} https://a", "1 {ABC}", "1 {ABC} https://a x", "1  {ABC} https://a", ""];
    for file in files.into_iter().chain(more) {
      let feed = line(&file.replace("{ABC}", ABC));
      let e = release(feed.as_bytes(), "1", "p").unwrap_err();
      assert_eq!(e.kind(), io::ErrorKind::InvalidData, "{file}: {e}");
    }

    let ok = line(&format!("1 {ABC} https://a"));
    for feed in [
      "".to_owned(),
      "p 1 x\n".to_owned(),
      "version\n".to_owned(),
      "version 2 3\n".to_owned(),
      format!("{ok}version 3\n"),
      format!("{ok}p 1 {ABC} https://b\n"),
      format!("{ok}\n"),
      format!(" {ok}"),
    ] {
      assert!(release(feed.as_bytes(), "1", "p").is_err(), "{feed:?}");
    }

    assert!(release(b"version \xff\n", "1", "p").is_err(), "not UTF-8");
    let other = format!("{ok}q 1 x\nwhatever comes later\n");
    assert!(release(other.as_bytes(), "1", "p").unwrap().is_some(), "other lines are skipped");
  }

  #[test]
  fn hex_of_either_case() {
    assert_eq!(hex("00fF7a"), Some([0, 255, 122]));
    for s in ["00ff", "00ff7a0", "00ff7a00", "+0ff7a", "00ffxx", "00ffé", ""] {
      assert_eq!(hex::<3>(s), None, "{s}");
    }
  }

  #[test]
  fn versions_order() {
    let newer = |a, b| newer(a, b).unwrap();
    assert!(newer("1.10", "1.9"));
    assert!(newer("1.2.0", "1.2.0-dev"));
    assert!(newer("v2.0", "1.99.99"));
    assert!(newer("1.2.0-rc.10", "1.2.0-rc.9"));
    assert!(newer("1.2.0-rc", "1.2.0-1"));
    assert!(!newer("1.2.0-dev", "1.2.0"));
    assert!(!newer("1.2", "1.2.0"), "equal is no update");
    assert!(!newer("1.2.0+5", "1.2.0"));
    assert!(!newer("1.1.9", "1.2.0"), "older is no update");
    assert_eq!(super::newer("1.x", "1.0"), None);
    assert_eq!(release(FEED, "1.2.0", "macos-aarch64").unwrap(), None);
    assert_eq!(release(FEED, "1.3", "macos-aarch64").unwrap(), None);
    assert!(release(FEED, "", "macos-aarch64").is_err());
  }

  #[test]
  fn http_is_only_for_loopback() {
    assert!(release(FEED, "1.0", "test-http").is_err());
    for url in ["http://127.0.0.1:8000/a.zip", "http://localhost/a", "http://[::1]:80", "https://a"]
    {
      assert!(secure(url), "{url}");
    }

    for url in ["http://example.com/a", "http://127.0.0.1.evil.com/", "http://127.0.0.1@evil.com/"]
    {
      assert!(!secure(url), "{url}");
    }

    assert!(!secure("ftp://localhost/a"));
  }

  #[test]
  fn rfc_8032_test_1_verifies() {
    verify(RFC_KEY, b"", RFC_SIG).unwrap();
    assert!(verify(RFC_KEY, b"x", RFC_SIG).is_err());
  }

  /// [FEED] read with [KEY].
  fn read_feed(text: &[u8]) -> io::Result<Option<Release>> {
    read(&Feed { url: "https://example.com/update.txt", key: KEY }, text, "1.0")
  }

  #[test]
  fn an_openssl_signed_feed_verifies() {
    let (data, sig) = signed(FEED).unwrap();
    assert_eq!((data.len(), sig.len()), (FEED.len() - 133, 128));
    verify(KEY, data, sig).unwrap();
    // A release only where the feed has a line for this platform (not on ARM Windows or Linux).
    read_feed(FEED).unwrap();
    // The same lines ended in CRLF, the `sig` line too or not, or with no line end.
    let crlf = String::from_utf8(data.to_vec()).unwrap().replace('\n', "\r\n");
    for end in ["\r\n", "\n", ""] {
      let feed = format!("{crlf}sig {CRLF_SIG}{end}");
      assert_eq!(read_feed(feed.as_bytes()).unwrap(), read_feed(FEED).unwrap(), "{end:?}");
    }
  }

  #[test]
  fn the_sig_line_is_last_and_once() {
    let (data, sig) = signed(FEED).unwrap();
    let data = std::str::from_utf8(data).unwrap();
    for feed in [
      data.to_owned(),
      format!("{data}sig {sig}\n\n"),
      format!("{data}sig {sig}\nsig {sig}\n"),
      format!("sig {sig}\n{data}sig {sig}\n"),
      format!("{data}sig\nsig {sig}\n"),
      format!("{data}sig {sig}\nversion 1.3\n"),
      format!("{data}sig  {sig}\n"),
      format!("{data}sig {sig} \n"),
    ] {
      let e = read_feed(feed.as_bytes()).unwrap_err();
      assert_eq!(e.kind(), io::ErrorKind::InvalidData, "{feed:?}: {e}");
    }
  }

  #[test]
  fn a_changed_feed_or_another_key_is_refused() {
    let (data, sig) = signed(FEED).unwrap();
    // Each byte the signature covers: a line's text, its line end, the line before `sig`.
    for at in [0, 20, data.len() - 2, data.len() - 1] {
      let mut feed = FEED.to_vec();
      feed[at] ^= 1;
      assert_eq!(read_feed(&feed).unwrap_err().kind(), io::ErrorKind::InvalidData, "{at}");
    }

    let kind = |r: io::Result<()>| r.unwrap_err().kind();
    assert_eq!(kind(verify(RFC_KEY, data, sig)), io::ErrorKind::InvalidData);
    assert_eq!(kind(verify(KEY, data, "not hex!")), io::ErrorKind::InvalidData);
    assert_eq!(kind(verify("AAAA", data, sig)), io::ErrorKind::InvalidInput);
    // The signature is checked before the feed is read.
    let e = read_feed(format!("{{\nsig {sig}\n").as_bytes()).unwrap_err();
    assert!(e.to_string().contains("signature"), "{e}");
  }

  #[test]
  fn sha512_of_a_file() {
    let path = temp("abc", b"abc");
    assert_eq!(sha512(&path).unwrap(), hex(ABC).unwrap());
    let r = release(FEED, "1.0", "linux-x86_64").unwrap().unwrap();
    r.verify(&path).unwrap();
    let other = temp("abd", b"abd");
    assert_eq!(r.verify(&other).unwrap_err().kind(), io::ErrorKind::InvalidData);
    let _ = (fs::remove_file(path), fs::remove_file(other));
  }
}
