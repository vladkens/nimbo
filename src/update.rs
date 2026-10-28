// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Updating an app in place, as Sparkle does: [check] fetches a signed feed and [read]s it for a
//! newer [Release], [download] fetches its file beside the running app, [Release::verify]s and
//! unpacks it, [install] swaps it in. They block: an app runs them on a thread of its own and
//! posts the result back.
//!
//! The feed is one text file, `update.txt`, signed, hosted anywhere: on GitHub as a release asset,
//! `https://github.com/<owner>/<repo>/releases/latest/download/update.txt`. It is UTF-8, one record
//! per line, its fields split by single spaces: exactly one `version`, per platform, keyed by
//! `<OS>-<ARCH>` from [std::env::consts], the file's size in bytes, its SHA-512 in hex and its
//! URL, and last the signature:
//!
//! ```text
//! version 1.2.0
//! macos-aarch64 394788 <sha512 hex> https://…/App-macos.zip
//! windows-x86_64 285930 <sha512 hex> https://…/App-windows.zip
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
//! size and SHA-512 are the feed's; nothing is unpacked before. Redirects are followed by the
//! system (on Linux only to https, on Windows not from https to http, on macOS by the shared
//! session's defaults); the signature and the hash guard what comes. The package holds exactly one
//! entry, named as the app's own:
//!
//! | OS | package | unpacked with | swapped |
//! |---|---|---|---|
//! | macOS | a `.zip` of the `.app` (`ditto -c -k --keepParent`) | `/usr/bin/ditto` | the same bundle id, and signed so that it meets the running app's designated requirement; relaunched by LaunchServices |
//! | Windows | a `.zip` of the app's folder | `tar.exe` of System32 | each file beside its `<file>.nimbo-old`; a file a release drops stays (the running app's folder can't be renamed) |
//! | Linux | the AppImage | none | over `$APPIMAGE` |
//!
//! What it replaces moves aside to `.<name>.nimbo-old` on macOS and Linux. A failure undoes every
//! rename. The relaunched app gets the old one's arguments (on macOS not its environment). On
//! macOS the grants (microphone, Accessibility) survive an update only when every release is signed
//! with one stable identity: an ad-hoc signature's requirement is its own build, so [download]
//! refuses the next one.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{env, fmt};

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

/// A release's app downloaded, verified and unpacked beside the running one, for [install]; its
/// folder is removed on drop.
#[derive(Debug)]
pub struct Staged {
  /// The new app: the `.app` bundle, the folder holding the exe, the AppImage.
  new: PathBuf,
  /// The app it replaces ([target]).
  target: PathBuf,
  /// What the relaunch starts: the target, on Windows its exe.
  run: PathBuf,
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
/// differs) and unpacks it: held to the archive contract ([package]), on macOS to the running
/// bundle's id and designated requirement. Before a byte is fetched it fails without an app to
/// replace (`Unsupported`: a build on macOS and Linux) or one whose folder the user can't write
/// (`PermissionDenied`: `/Applications` for a standard user, Program Files, a translocated app).
/// On Windows only the archive names the app's folder, so a build's (`target\debug`) is refused
/// once unpacked (`Unsupported`).
///
/// `progress` gets the bytes done and the total, `release.size`, on the calling thread: at most
/// about 4 times a second while the file comes, then once with the total when it is verified.
pub fn download(release: &Release, mut progress: impl FnMut(u64, u64)) -> io::Result<Staged> {
  let exe = env::current_exe()?;
  let target = target(OS, &exe, &|k| env::var_os(k))?;
  let (dir, file) = fetch(release, beside(OS, &target), &mut progress)?;
  let into = dir.0.join("new");
  fs::create_dir(&into)?;
  imp::unpack(&file, &into)?;
  let new = package(OS, &into, &target, &exe)?;
  #[cfg(target_os = "macos")]
  imp::accepts(&new, &target)?;
  let run =
    exe.file_name().filter(|_| OS == Os::Windows).map_or(target.clone(), |n| target.join(n));
  Ok(Staged { new, target, run, dir })
}

/// Puts the [Staged] app in the running one's place and starts it: what it replaces renamed to
/// its [old] name (on Windows each file, as the folder of a running exe can't move), the new one
/// moved in, each rename undone on an error, also when the new app doesn't start; one that can't
/// be undone keeps the staged folder and the error says where the files are. The app must quit
/// promptly after `Ok`.
pub fn install(staged: Staged) -> io::Result<()> {
  let pid = std::process::id().to_string();
  put(OS, staged, &|a, b| fs::rename(a, b), &|app| imp::relaunch(app, &pid))
}

/// What the relaunch sets to this process's pid in the new one; a seam until the installer
/// replaces the relaunch.
pub(crate) const RELAUNCH: &str = "NIMBO_RELAUNCH";

/// [download]'s progress: the bytes done, the total.
type Progress<'a> = &'a mut dyn FnMut(u64, u64);

/// What starts the new app, [imp::relaunch], or one that fails in tests.
type Relaunch<'a> = &'a dyn Fn(&Path) -> io::Result<()>;

/// [install] on `os`, with `rename` and `relaunch`.
fn put(os: Os, staged: Staged, rename: Rename, relaunch: Relaunch) -> io::Result<()> {
  let (pairs, mut done) = (pairs(os, &staged.new, &staged.target)?, Vec::new());
  let started = swap(os, &pairs, rename, &mut done);
  let Err(e) = started.and_then(|()| relaunch(&staged.run)) else {
    return Ok(());
  };

  let Err(undone) = undo(&done, rename) else { return Err(e) };
  let (target, dir) = (staged.target.display().to_string(), staged.dir.keep());
  Err(io::Error::other(format!(
    "update: {e}, and undoing it failed ({undone}): {target} is left half-swapped, its old files \
     under their {OLD} names, the update kept in {}",
    dir.display()
  )))
}

/// Where [download] stages the update of `target`: beside it, on its volume, so the swap only
/// renames; in the folder it replaces on Windows.
fn beside(os: Os, target: &Path) -> &Path {
  if os == Os::Windows { target } else { target.parent().unwrap_or(target) }
}

/// The app an update replaces, of the running `exe`: the `.app` bundle on macOS and the AppImage
/// on Linux ([uninstall::app_path]'s rules), the exe's folder on Windows; `Unsupported` for none
/// (a build), `PermissionDenied` for a bundle macOS translocated (run from a read-only copy, as
/// it does for one downloaded and not moved).
fn target(os: Os, exe: &Path, env: Env) -> io::Result<PathBuf> {
  let target = match os {
    Os::Windows => exe.parent().map(Path::to_owned),
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
  let file = dir.0.join("download");
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

/// The app unpacked into `into`, held to the archive contract: exactly one entry, a `.app` folder
/// on macOS, the AppImage on Linux, on Windows a folder of the target's name (`Unsupported` if
/// not: a build's `target\debug`) holding a file of the running exe's name; `InvalidData` else.
fn package(os: Os, into: &Path, target: &Path, exe: &Path) -> io::Result<PathBuf> {
  let mut all = fs::read_dir(into)?.collect::<io::Result<Vec<_>>>()?;
  let one = all.pop().filter(|_| all.is_empty());
  let one = one.ok_or_else(|| bad("the archive holds more than one app, or none"))?;
  let (new, kind) = (one.path(), one.file_type()?);
  let app = match os {
    Os::Mac => kind.is_dir() && new.extension().is_some_and(|e| e == "app"),
    Os::Windows => kind.is_dir() && exe.file_name().is_some_and(|n| new.join(n).is_file()),
    Os::Linux => kind.is_file(),
  };

  // Windows matches names whatever their case.
  let (name, of) = (new.file_name().unwrap_or_default(), target.file_name().unwrap_or_default());
  match (app, os == Os::Windows && !name.eq_ignore_ascii_case(of)) {
    (false, _) => Err(bad(format!("{} is not the app of {}", new.display(), exe.display()))),
    (true, true) => Err(error(
      io::ErrorKind::Unsupported,
      format!("{} is not the app's folder", target.display()),
    )),
    (true, false) => Ok(new),
  }
}

/// What replaces what, `(new, target)`: the app on macOS and Linux; on Windows each file of the new
/// folder and its place in the target, a folder whole only where the target has none.
fn pairs(os: Os, new: &Path, target: &Path) -> io::Result<Pairs> {
  if os != Os::Windows {
    return Ok(vec![(new.to_owned(), target.to_owned())]);
  }

  let mut out = Vec::new();
  for e in fs::read_dir(new)? {
    let e = e?;
    let (from, to) = (e.path(), target.join(e.file_name()));
    match from.is_dir() && to.is_dir() {
      true => out.extend(pairs(os, &from, &to)?),
      false => out.push((from, to)),
    }
  }

  Ok(out)
}

/// Paths and where they go.
type Pairs = Vec<(PathBuf, PathBuf)>;
/// A rename, [fs::rename] or one that fails in tests.
type Rename<'a> = &'a dyn Fn(&Path, &Path) -> io::Result<()>;

/// Moves each new path over its target with `rename`, a target there first renamed to its [old]
/// name (a stale one removed), each tried again while it may pass ([uninstall::retry]); each
/// rename made goes to `done`, in order, for [undo].
fn swap(os: Os, pairs: &Pairs, rename: Rename, done: &mut Pairs) -> io::Result<()> {
  for (new, target) in pairs {
    if target.exists() {
      let old = old(os, target);
      uninstall::remove(&old)?;
      uninstall::retry(|| rename(target, &old))?;
      done.push((target.clone(), old));
    }

    uninstall::retry(|| rename(new, target))?;
    done.push((new.clone(), target.clone()));
  }

  Ok(())
}

/// Undoes the renames `done` made, newest first, each tried again while it may pass
/// ([uninstall::retry]); stops at one that fails, which the error names.
fn undo(done: &Pairs, rename: Rename) -> io::Result<()> {
  for (from, to) in done.iter().rev() {
    let back = uninstall::retry(|| rename(to, from));
    let what = |e| format!("{} back to {}: {e}", to.display(), from.display());
    back.map_err(|e| io::Error::new(e.kind(), what(e)))?;
  }

  Ok(())
}

/// Where an update moves what it replaces: `<name>.nimbo-old` beside it, hidden as
/// `.<name>.nimbo-old` on macOS and Linux; a suffix only nimbo makes, so cleanup removes no
/// user's `.old`.
fn old(os: Os, path: &Path) -> PathBuf {
  let mut name = OsString::from(if os == Os::Windows { "" } else { "." });
  name.push(path.file_name().unwrap_or_default());
  name.push(OLD);
  path.with_file_name(name)
}

/// The suffix of an [old] name.
const OLD: &str = ".nimbo-old";
/// A staging folder's name before `<pid>-<nanos>`; a kept one's before `kept-<pid>-<nanos>`.
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

  /// Its path, the folder kept and renamed to `.nimbo-update-kept-…`; the empty path left behind
  /// can't be removed.
  fn keep(mut self) -> PathBuf {
    let dir = std::mem::take(&mut self.0);
    let name = dir.file_name().and_then(OsStr::to_str).and_then(|n| n.strip_prefix(STAGING));
    let kept = dir.with_file_name(format!("{STAGING}kept-{}", name.unwrap_or_default()));
    if fs::rename(&dir, &kept).is_ok() { kept } else { dir }
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
  use std::cell::Cell;

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
    assert_eq!(target(Os::Windows, "C:/A/a.exe").unwrap(), Path::new("C:/A"));
    let moved = "/private/var/folders/x/T/AppTranslocation/1-2/d/A.app/Contents/MacOS/a";
    assert_eq!(target(Os::Mac, moved).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    for (os, exe) in [(Os::Mac, "/Users/me/a/target/debug/a"), (Os::Linux, "/usr/bin/a")] {
      assert_eq!(target(os, exe).unwrap_err().kind(), io::ErrorKind::Unsupported);
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

  #[test]
  fn an_archive_holds_exactly_one_app() {
    let root = dir("one");
    let files = [("mac/A.app/Contents/Info.plist", ""), ("two/A.app/x", ""), ("two/B.app/x", "")];
    write(&root, &files);
    write(&root, &[("file/A.app", ""), ("win/W/a.exe", ""), ("win/x", ""), ("linux/app", "")]);
    fs::create_dir(root.join("empty")).unwrap();
    let (target, exe) = (root.join("W"), root.join("W/a.exe"));
    let package = |os, into: &str| package(os, &root.join(into), &target, &exe);
    assert_eq!(package(Os::Mac, "mac").unwrap(), root.join("mac/A.app"));
    assert_eq!(package(Os::Linux, "linux").unwrap(), root.join("linux/app"));
    let bad = [(Os::Mac, "two"), (Os::Mac, "file"), (Os::Mac, "empty"), (Os::Windows, "win")];
    for (os, into) in bad.into_iter().chain([(Os::Windows, "linux"), (Os::Linux, "mac")]) {
      let e = package(os, into).unwrap_err();
      assert_eq!(e.kind(), io::ErrorKind::InvalidData, "{os:?} {into}: {e}");
    }

    fs::remove_dir_all(root).unwrap();
  }

  #[test]
  fn a_mismatched_name_is_refused() {
    let root = dir("name");
    write(&root, &[("ok/W/a.exe", ""), ("other/V/a.exe", ""), ("no-exe/W/b.exe", "")]);
    write(&root, &[("case/w/a.exe", "")]);
    let (target, exe) = (root.join("W"), root.join("W/a.exe"));
    let package = |into: &str| package(Os::Windows, &root.join(into), &target, &exe);
    assert_eq!(package("ok").unwrap(), root.join("ok/W"));
    assert_eq!(package("case").unwrap(), root.join("case/w"), "Windows ignores the case");
    assert_eq!(package("other").unwrap_err().kind(), io::ErrorKind::Unsupported);
    assert_eq!(package("no-exe").unwrap_err().kind(), io::ErrorKind::InvalidData);
    fs::remove_dir_all(root).unwrap();
  }

  #[test]
  fn the_swap_moves_the_new_app_in_and_the_old_aside() {
    let root = dir("swap");
    // A bundle and its new one, named otherwise; an AppImage; Windows' exe and DLL, a stale
    // `.nimbo-old`, a folder there and one new, with the update in the app's folder.
    write(&root, &[("A.app/Contents/MacOS/a", "1"), ("new/B.app/Contents/MacOS/a", "2")]);
    write(&root, &[("A.AppImage", "1"), ("new/app", "2")]);
    write(
      &root,
      &[("W/a.exe", "1"), ("W/a.dll", "1"), ("W/a.dll.nimbo-old", "0"), ("W/res/x", "1")],
    );
    write(&root, &[("W/.u/W/a.exe", "2"), ("W/.u/W/a.dll", "2"), ("W/.u/W/res/x", "2")]);
    write(&root, &[("W/.u/W/res/y", "2"), ("W/.u/W/lib/z", "2")]);
    let apps = [(Os::Mac, "new/B.app", "A.app"), (Os::Linux, "new/app", "A.AppImage")];
    for (os, new, target) in apps.into_iter().chain([(Os::Windows, "W/.u/W", "W")]) {
      let (new, target) = (root.join(new), root.join(target));
      let pairs = pairs(os, &new, &target).unwrap();
      swap(os, &pairs, &|a, b| fs::rename(a, b), &mut Vec::new()).unwrap();
    }

    let files = [("A.app/Contents/MacOS/a", "2"), (".A.app.nimbo-old/Contents/MacOS/a", "1")];
    holds(&root, &files, "on macOS");
    holds(
      &root,
      &[("A.AppImage", "2"), (".A.AppImage.nimbo-old", "1"), ("new/app", "")],
      "on Linux",
    );
    let files =
      [("W/a.exe", "2"), ("W/a.exe.nimbo-old", "1"), ("W/a.dll", "2"), ("W/a.dll.nimbo-old", "1")];
    holds(&root, &files, "on Windows");
    holds(
      &root,
      &[("W/res/x", "2"), ("W/res/x.nimbo-old", "1"), ("W/res/y", "2"), ("W/lib/z", "2")],
      "",
    );
    fs::remove_dir_all(root).unwrap();
  }

  /// The update of `target` by `new`, both under `root`, its folder there too.
  fn staged(root: &Path, new: &str, target: &str) -> Staged {
    let (new, target, dir) = (root.join(new), root.join(target), Folder::new(root).unwrap());
    Staged { run: target.clone(), new, target, dir }
  }

  /// The staging folders under `root`.
  fn staging(root: &Path) -> usize {
    let all = fs::read_dir(root).unwrap().flatten();
    all.filter(|e| e.file_name().to_string_lossy().starts_with(STAGING)).count()
  }

  #[test]
  fn a_failed_rename_or_relaunch_undoes_each_rename() {
    let root = dir("undo");
    let files = [("W/a.exe", "1"), ("W/a.dll", "1"), ("W/.u/W/a.exe", "2"), ("W/.u/W/a.dll", "2")];
    write(&root, &files);
    let relaunch = |_: &Path| Err(io::Error::other("busy"));
    // Four renames: the exe and the DLL each aside, then in; with none failing, the relaunch.
    for fail in [1, 2, 3, 4, 0] {
      let n = Cell::new(0);
      let rename = |a: &Path, b: &Path| {
        n.set(n.get() + 1);
        if n.get() == fail { Err(io::Error::other("busy")) } else { fs::rename(a, b) }
      };
      let e = put(Os::Windows, staged(&root, "W/.u/W", "W"), &rename, &relaunch).unwrap_err();
      assert_eq!(e.to_string(), "busy");
      holds(&root, &files, &format!("after step {fail} failed"));
      holds(&root, &[("W/a.exe.nimbo-old", ""), ("W/a.dll.nimbo-old", "")], "");
      assert_eq!(staging(&root), 0, "the folder is removed");
    }

    // The second file can't move aside, nor the first back: the folder stays, the error says so.
    let n = Cell::new(0);
    let rename = |a: &Path, b: &Path| {
      n.set(n.get() + 1);
      if n.get() >= 3 { Err(io::Error::other("held")) } else { fs::rename(a, b) }
    };
    let first = pairs(Os::Windows, &root.join("W/.u/W"), &root.join("W")).unwrap()[0].1.clone();
    let e = put(Os::Windows, staged(&root, "W/.u/W", "W"), &rename, &relaunch).unwrap_err();
    let read = |p: &Path| fs::read_to_string(p).unwrap();
    assert_eq!((read(&first), read(&old(Os::Windows, &first))), ("2".into(), "1".into()));
    assert_eq!(staging(&root), 1, "the folder is kept");
    let mut kept = fs::read_dir(&root).unwrap().flatten().map(|e| e.path());
    let kept = kept.find(|p| p.to_string_lossy().contains(".nimbo-update-kept-")).unwrap();
    assert!(e.to_string().contains("half-swapped"), "{e}");
    assert!(e.to_string().contains(&kept.display().to_string()), "{e}");
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
  fn a_bundle_unpacks_and_must_be_the_app_signed_alike() {
    let root = dir("bundle");
    let app = root.join("A.app");
    bundle(&app, "com.example.a", "1");
    let mut zip = std::process::Command::new("/usr/bin/ditto");
    zip.args(["-c", "-k", "--keepParent"]).arg(&app).arg(root.join("a.zip"));
    assert!(zip.status().unwrap().success());
    fs::create_dir(root.join("new")).unwrap();
    imp::unpack(&root.join("a.zip"), &root.join("new")).unwrap();
    let new = package(Os::Mac, &root.join("new"), &app, &app).unwrap();
    imp::accepts(&new, &app).unwrap();
    let not_zip = imp::unpack(&app.join("Contents/Info.plist"), &root).unwrap_err();
    assert_eq!(not_zip.kind(), io::ErrorKind::InvalidData);

    // Another build signed ad hoc meets no requirement but its own; another app is refused.
    bundle(&root.join("v2/A.app"), "com.example.a", "2");
    bundle(&root.join("b/A.app"), "com.example.b", "1");
    for other in ["v2/A.app", "b/A.app"] {
      let e = imp::accepts(&root.join(other), &app).unwrap_err();
      assert_eq!(e.kind(), io::ErrorKind::InvalidData, "{other}: {e}");
    }

    fs::remove_dir_all(root).unwrap();
  }

  #[cfg(target_os = "linux")]
  #[test]
  fn an_appimage_is_made_executable() {
    use std::os::unix::fs::PermissionsExt;
    let root = dir("appimage");
    write(&root, &[("download", "2")]);
    fs::create_dir(root.join("new")).unwrap();
    imp::unpack(&root.join("download"), &root.join("new")).unwrap();
    let new = package(Os::Linux, &root.join("new"), &root, &root).unwrap();
    assert_eq!(fs::metadata(new).unwrap().permissions().mode() & 0o777, 0o755);
    fs::remove_dir_all(root).unwrap();
  }

  #[cfg(windows)]
  #[test]
  fn a_zip_unpacks_with_tar() {
    let root = dir("tar");
    write(&root, &[("W/a.exe", "2"), ("W/res/x", "2")]);
    let tar = Path::new(&env::var_os("SystemRoot").unwrap()).join(r"System32\tar.exe");
    let mut zip = std::process::Command::new(tar);
    zip.arg("-a").arg("-cf").arg(root.join("a.zip")).arg("-C").arg(&root).arg("W");
    assert!(zip.status().unwrap().success());
    fs::create_dir(root.join("new")).unwrap();
    imp::unpack(&root.join("a.zip"), &root.join("new")).unwrap();
    let new = package(Os::Windows, &root.join("new"), &root.join("W"), &root.join("W/a.exe"));
    holds(&new.unwrap(), &[("a.exe", "2"), ("res/x", "2")], "unpacked");
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
    assert!(read_feed(FEED).unwrap().is_some());
    // The same lines ended in CRLF, the `sig` line too or not, or with no line end.
    let crlf = String::from_utf8(data.to_vec()).unwrap().replace('\n', "\r\n");
    for end in ["\r\n", "\n", ""] {
      let feed = format!("{crlf}sig {CRLF_SIG}{end}");
      assert!(read_feed(feed.as_bytes()).unwrap().is_some(), "{end:?}");
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
