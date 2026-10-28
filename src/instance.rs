// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! One instance of the app: the first launch holds a lock in its folder while it runs and takes
//! one-line commands from other processes (a second launch's [OPEN], an uninstaller's [QUIT],
//! the app's own); a second launch finds it, sends what it wanted and ends.
//!
//! | | the lock | where commands come |
//! |---|---|---|
//! | macOS | `instance.lock` in the folder | a Unix socket, `$TMPDIR/nimbo-<hash>.sock` |
//! | Linux | the same | a Unix socket, `$XDG_RUNTIME_DIR/nimbo-<hash>.sock` |
//! | Windows | the same | a named pipe, `\\.\pipe\nimbo-<hash>`, for this user only |
//!
//! The folder is the app's data folder ([crate::paths::data_dir]), or one given to the `*_in`
//! variants (a dev build, a test). The OS lets the lock go however the process ends, and a dead
//! instance's socket is replaced. A launch claims before the loop and hands the instance to
//! [Main::serve], which keeps it while the loop runs:
//!
//! ```no_run
//! use nimbo::instance::{self, Claim};
//! use nimbo::{AppInfo, Presence};
//!
//! const INFO: AppInfo =
//!   AppInfo { id: "com.example.app", name: "App", presence: Presence::Regular };
//! let instance = match instance::claim(&INFO) {
//!   Ok(Claim::Primary(instance)) => Some(instance),
//!   Ok(Claim::Running) => return instance::send(&INFO, instance::OPEN).unwrap_or_default(),
//!   // An update's relaunch that the old instance never let go of: it still runs.
//!   Ok(Claim::Replaced) => return,
//!   Err(_) => None,
//! };
//!
//! nimbo::app(INFO, |main| {
//!   // A second launch's "open" runs this, as a reopen from the Dock does.
//!   main.on_reopen(|_main| { /* show the settings */ });
//!   // "quit" quits after the handler; the app's own commands come here.
//!   if let Some(instance) = instance {
//!     _ = main.serve(instance, |_main, cmd| eprintln!("command: {}", cmd.name));
//!   }
//! });
//! ```
//!
//! Ported from wavo-app's `src/ctl.rs` and `quit_and_wait` in `src/cli.rs` (vladkens, MIT,
//! <https://github.com/vladkens/wavo-app>).

use std::fs::{self, TryLockError};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{env, thread};

use crate::AppInfo;
#[cfg(doc)]
use crate::Main;
use crate::platform::instance as imp;

/// Shows the app: what a second launch sends. [Main::serve] runs [Main::on_reopen]'s hook for it.
pub const OPEN: &str = "open";
/// Quits the app, for [quit_and_wait]: [Main::serve] runs [Main::quit] after the app's handler.
pub const QUIT: &str = "quit";

/// The lock's file in the instance's folder.
const LOCK: &str = "instance.lock";
/// How long [send_in] may take: the instance may still be starting, and on Windows the pipe
/// may serve another sender, then the instance reads the line.
const SEND_WAIT: Duration = Duration::from_secs(3);
/// How long a sender tries while the lock is held but nothing takes the command: an instance
/// that started at the same time has yet to listen.
const STARTING: Duration = Duration::from_secs(1);
/// How long a sender may take to send its line.
pub(crate) const READ_TIMEOUT: Duration = Duration::from_secs(1);
/// The longest line taken: a command and an activation token.
pub(crate) const MAX_LINE: usize = 1024;
/// What an update's relaunch ([crate::update::install]) starts the new process with:
/// `<old pid>:<the hex key of the old instance's folder>:<a nonce>` ([relaunch]), for [claim_in]
/// to wait.
pub(crate) const RELAUNCH: &str = "NIMBO_RELAUNCH";
/// The one-shot token beside the lock: [RELAUNCH]'s value, written by [relaunch], taken by the
/// first [claim_in] that has it, so that the new process's children don't wait as it did.
const TOKEN: &str = "relaunch";
/// The longest a relaunched process waits for the one it replaces.
const RELAUNCH_WAIT: Duration = Duration::from_secs(60);
/// How long a relaunched process still tries the lock once the old one has ended: Windows may
/// let it go late.
const RELAUNCH_LATE: Duration = Duration::from_secs(3);
/// The folder this process is the instance for, `None` before it is.
static CLAIMED: Mutex<Option<PathBuf>> = Mutex::new(None);

/// What [claim] found.
pub enum Claim {
  /// This process is the app's instance until it is dropped: hand it to [Main::serve].
  Primary(Instance),
  /// Another process is: [send] it what this launch wanted.
  Running,
  /// This process is an update's relaunch ([RELAUNCH] and its token) and the instance it replaces
  /// never let go: it ran on past 60 s, or ended with the lock still held 3 s later. The app ends
  /// quietly, sending nothing.
  Replaced,
}

/// The app's one instance: its lock and the place commands come to, let go when dropped.
pub struct Instance(imp::Server);

impl Instance {
  /// Calls `on` with each command, on a thread of its own; once.
  pub(crate) fn listen(&mut self, on: impl Fn(Command) + Send + 'static) -> io::Result<()> {
    self.0.serve(move |line| {
      if let Some(cmd) = Command::parse(line) {
        on(cmd);
      }
    })
  }
}

/// A command from another process, sent as one line: `<name>`, or `<name>\t<token>`, neither
/// with a tab or a line break in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
  /// What it asks: [OPEN], [QUIT] or the app's own.
  pub name: String,
  /// The desktop's activation token the sender was started with (`XDG_ACTIVATION_TOKEN` on
  /// Wayland, `DESKTOP_STARTUP_ID` on X11), which lets a window come to the front for it: while
  /// [Main::serve]'s handler runs, [Main::open] and [crate::Window::front] use it (Linux).
  pub token: Option<String>,
}

impl Command {
  /// `name` sent by this process, with the token it was started with.
  fn new(name: &str) -> Self {
    let env = |k| std::env::var(k).ok().filter(|t: &String| !t.is_empty());
    let token = env("XDG_ACTIVATION_TOKEN").or_else(|| env("DESKTOP_STARTUP_ID"));
    Self { name: name.into(), token }
  }

  /// The line sent; `None` when the name can't be one. A token that can't be, or makes the line
  /// too long, is left out.
  fn line(&self) -> Option<String> {
    let plain = |s: &str| !s.trim().is_empty() && !s.contains(['\t', '\n', '\r']);
    if !plain(&self.name) || self.name.len() >= MAX_LINE {
      return None;
    }

    let fits = |t: &&str| plain(t) && self.name.len() + t.len() + 2 <= MAX_LINE;
    Some(match self.token.as_deref().filter(fits) {
      Some(token) => format!("{}\t{token}\n", self.name),
      None => format!("{}\n", self.name),
    })
  }

  /// The first line of what was read, its line break optional.
  fn parse(read: &str) -> Option<Self> {
    let line = read.lines().next().unwrap_or_default();
    let (name, token) = line.split_once('\t').map_or((line, None), |(n, t)| (n, Some(t)));
    let token = token.filter(|t| !t.is_empty() && !t.contains('\t')).map(String::from);
    let name = name.trim();
    (!name.is_empty()).then(|| Self { name: name.into(), token })
  }
}

/// Becomes the app's one instance unless another process is: [claim_in] its data folder
/// ([crate::paths::data_dir]), made if missing.
pub fn claim(app: &AppInfo) -> io::Result<Claim> {
  let dir = crate::paths::data_dir(app.id);
  fs::create_dir_all(&dir)?;
  claim_in(&dir)
}

/// Becomes the instance for the folder `dir`, which exists, unless another process is: the lock
/// `instance.lock` in it, and the place commands come to. Relaunched by an update of the app
/// ([RELAUNCH], while the token `relaunch` in `dir` holds its value), it first waits up to 60 s for
/// the old process to end and let the lock go ([Claim::Replaced] if it doesn't), and removes the
/// token; once the instance, it removes what an update left beside the app
/// ([crate::update::tidy]).
pub fn claim_in(dir: &Path) -> io::Result<Claim> {
  claim_until(dir, env::var(RELAUNCH).ok().as_deref(), RELAUNCH_WAIT, RELAUNCH_LATE)
}

/// [claim_in], waiting up to `most` while the process the [RELAUNCH] value `wait` names (its pid
/// and the [key] of its folder) runs and holds the lock, and `late` once it has ended, when that
/// folder is `dir` and its token holds `wait`; another's is not waited for.
fn claim_until(
  dir: &Path,
  wait: Option<&str>,
  most: Duration,
  late: Duration,
) -> io::Result<Claim> {
  let (lock, mut until, mut ended) =
    (imp::lock_file(&dir.join(LOCK))?, Instant::now() + most, false);
  let token = dir.join(TOKEN);
  let mine = |v: &&str| fs::read_to_string(&token).is_ok_and(|t| t == *v);
  let wait = wait.filter(mine).and_then(relaunched);
  let wait = wait.filter(|&(_, k)| k == key(dir)).map(|(pid, _)| pid);
  let replaced = loop {
    // Looked at before the lock is tried: the process lets it go as it ends.
    if let Some(pid) = wait
      && !ended
      && !imp::alive(pid)
    {
      (ended, until) = (true, Instant::now() + late);
    }

    match lock.try_lock() {
      Ok(()) => break false,
      Err(TryLockError::WouldBlock) if wait.is_none() => return Ok(Claim::Running),
      Err(TryLockError::WouldBlock) if Instant::now() >= until => break true,
      Err(TryLockError::WouldBlock) => thread::sleep(Duration::from_millis(20)),
      Err(TryLockError::Error(e)) => return Err(e),
    }
  };

  // Taken: the new process's children inherit the value, not the wait.
  if wait.is_some() {
    _ = fs::remove_file(&token);
  }

  if replaced {
    return Ok(Claim::Replaced);
  }

  let instance = Instance(imp::Server::bind(dir, lock)?);
  *CLAIMED.lock().unwrap_or_else(PoisonError::into_inner) = Some(dir.to_owned());
  crate::update::tidy(wait);
  Ok(Claim::Primary(instance))
}

/// [RELAUNCH]'s value for a process this one starts: its pid, the [key] of the folder it is the
/// instance for and the time in nanoseconds, also written to the folder's token; with no folder
/// the key is 0, waited for by none.
pub(crate) fn relaunch() -> io::Result<String> {
  let dir = CLAIMED.lock().unwrap_or_else(PoisonError::into_inner).clone();
  let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
  let v = format!("{}:{:016x}:{nonce}", std::process::id(), dir.as_deref().map_or(0, key));
  if let Some(dir) = dir {
    fs::write(dir.join(TOKEN), &v)?;
  }

  Ok(v)
}

/// The pid and folder key in [RELAUNCH]'s value.
fn relaunched(v: &str) -> Option<(u32, u64)> {
  let mut parts = v.split(':');
  let (pid, key, _nonce) = (parts.next()?, parts.next()?, parts.next()?);
  Some((pid.parse().ok()?, u64::from_str_radix(key, 16).ok()?))
}

/// The folder `dir` by its whole path, the same in each process: [hash] of it canonical.
pub(crate) fn key(dir: &Path) -> u64 {
  hash(dir.canonicalize().unwrap_or_else(|_| dir.to_owned()).as_os_str().as_encoded_bytes())
}

/// Sends the command `name` to the app's instance: [send_in] its data folder.
pub fn send(app: &AppInfo, name: &str) -> io::Result<()> {
  send_in(&crate::paths::data_dir(app.id), name)
}

/// Sends the command `name` to the instance for `dir`, with this process's activation token
/// ([Command::token]), within 3 s: an instance that holds the lock but doesn't listen yet (it
/// started at the same time) is given a second; `Err` when none runs. On Windows it waits until
/// the instance has read it, and lets it come to the front: this process, in front, allows it.
pub fn send_in(dir: &Path, name: &str) -> io::Result<()> {
  deliver(dir, &Command::new(name), Instant::now() + SEND_WAIT)
}

/// Sends `cmd` by `deadline`, tried again for up to [STARTING] while nothing takes it but an
/// instance holds the lock.
fn deliver(dir: &Path, cmd: &Command, deadline: Instant) -> io::Result<()> {
  let bad = || io::Error::new(ErrorKind::InvalidInput, "a command is one line without tabs");
  let line = cmd.line().ok_or_else(bad)?;
  let retry = deadline.min(Instant::now() + STARTING);
  loop {
    match imp::send(dir, &line, deadline) {
      Err(e)
        if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused)
          && Instant::now() < retry
          && running(dir).unwrap_or(false) =>
      {
        thread::sleep(Duration::from_millis(20));
      }
      sent => return sent,
    }
  }
}

/// Asks the app's instance to quit ([QUIT]) and waits up to `timeout` for its lock:
/// [quit_and_wait_in] its data folder.
pub fn quit_and_wait(app: &AppInfo, timeout: Duration) -> io::Result<()> {
  quit_and_wait_in(&crate::paths::data_dir(app.id), timeout)
}

/// Asks the instance for `dir` to quit ([QUIT], which [Main::serve] answers with [Main::quit]) and
/// waits up to `timeout` for it to let its lock go: `Ok` once none runs (at once when none did),
/// `Err` of [ErrorKind::TimedOut] while one still does; sending counts in the `timeout`. Before an
/// uninstall removes its files, from a process other than the app's.
pub fn quit_and_wait_in(dir: &Path, timeout: Duration) -> io::Result<()> {
  let deadline = Instant::now() + timeout;
  let mut asked = false;
  while running(dir)? {
    if Instant::now() >= deadline {
      return Err(io::Error::new(ErrorKind::TimedOut, "the app is still running"));
    }

    if !asked {
      asked = true;
      _ = deliver(dir, &Command::new(QUIT), deadline);
    }

    thread::sleep(Duration::from_millis(20));
  }

  Ok(())
}

/// Whether an instance holds the lock in `dir`: tried, and let go at once.
fn running(dir: &Path) -> io::Result<bool> {
  let lock = match imp::lock_file(&dir.join(LOCK)) {
    Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
    lock => lock?,
  };

  match lock.try_lock() {
    Ok(()) => Ok(false),
    Err(TryLockError::WouldBlock) => Ok(true),
    Err(TryLockError::Error(e)) => Err(e),
  }
}

/// FNV-1a, the same in every build: the instance's socket or pipe by its folder.
pub(crate) fn hash(bytes: &[u8]) -> u64 {
  bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

/// A line read as it comes, polled: `peek` the bytes waiting (`None` once the sender is gone),
/// `read` takes them; until a line break, [MAX_LINE] bytes or the sender's end, `None` after
/// [READ_TIMEOUT] with nothing more waiting. Windows' pipe, where a blocking read would wait for
/// a sender that sends nothing.
#[cfg(any(test, windows))]
pub(crate) fn read_polled(
  mut peek: impl FnMut() -> Option<usize>,
  mut read: impl FnMut(&mut [u8]) -> Option<usize>,
  timeout: Duration,
) -> Option<String> {
  let (deadline, mut line) = (Instant::now() + timeout, Vec::new());
  while !line.contains(&b'\n') && line.len() < MAX_LINE {
    let Some(waiting) = peek() else { break };
    if waiting == 0 {
      if Instant::now() >= deadline {
        return None;
      }

      thread::sleep(Duration::from_millis(5));
      continue;
    }

    let mut buf = [0u8; MAX_LINE];
    let Some(n) = read(&mut buf[..waiting.min(MAX_LINE - line.len())]) else { break };
    line.extend_from_slice(&buf[..n]);
  }

  // The first line alone: what comes after is not the command's.
  line.truncate(line.iter().position(|&b| b == b'\n').map_or(line.len(), |i| i + 1));
  String::from_utf8(line).ok()
}

#[cfg(test)]
mod tests {
  use std::cell::RefCell;
  use std::collections::VecDeque;
  use std::path::PathBuf;
  use std::sync::mpsc;

  use super::*;

  /// A folder of its own under the system's temp one, removed when dropped.
  struct Dir(PathBuf);

  impl Dir {
    fn new(name: &str) -> Self {
      let dir = std::env::temp_dir().join(format!("nimbo-instance-{}-{name}", std::process::id()));
      _ = fs::remove_dir_all(&dir);
      fs::create_dir_all(&dir).unwrap();
      Self(dir)
    }
  }

  impl Drop for Dir {
    fn drop(&mut self) {
      _ = fs::remove_dir_all(&self.0);
    }
  }

  /// The instance for `dir`, its commands sent to the receiver.
  fn serve(dir: &Path) -> (Instance, mpsc::Receiver<Command>) {
    let Ok(Claim::Primary(mut instance)) = claim_in(dir) else {
      panic!("{} is taken", dir.display())
    };
    let (tx, rx) = mpsc::channel();
    instance.listen(move |cmd| _ = tx.send(cmd)).unwrap();
    (instance, rx)
  }

  fn recv(rx: &mpsc::Receiver<Command>) -> Command {
    rx.recv_timeout(Duration::from_secs(2)).unwrap()
  }

  #[test]
  fn a_second_claim_finds_the_first() {
    let dir = Dir::new("claim");
    let Ok(Claim::Primary(first)) = claim_in(&dir.0) else { panic!("not the first") };
    assert!(matches!(claim_in(&dir.0).unwrap(), Claim::Running));
    assert!(running(&dir.0).unwrap());

    drop(first);
    assert!(!running(&dir.0).unwrap(), "the lock is free");
    assert!(matches!(claim_in(&dir.0).unwrap(), Claim::Primary(_)));
  }

  #[test]
  fn commands_come_with_their_token() {
    let dir = Dir::new("send");
    let (instance, rx) = serve(&dir.0);
    let open =
      Command { name: OPEN.into(), token: Some("gnome-shell/App/1234-1-host_TIME42".into()) };
    deliver(&dir.0, &open, Instant::now() + SEND_WAIT).unwrap();
    assert_eq!(recv(&rx), open);
    send_in(&dir.0, "show settings").unwrap();
    assert_eq!(recv(&rx).name, "show settings");
    assert_eq!(send_in(&dir.0, "a\tb").unwrap_err().kind(), ErrorKind::InvalidInput);

    drop(instance);
    assert!(send_in(&dir.0, QUIT).is_err(), "gone with the instance");
  }

  #[cfg(unix)]
  #[test]
  fn a_dead_instances_socket_is_replaced() {
    let dir = Dir::new("dead");
    let path = imp::socket_path(&dir.0).unwrap();
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    assert!(path.exists());
    assert!(send_in(&dir.0, QUIT).is_err(), "nobody listens");

    let (_instance, rx) = serve(&dir.0);
    send_in(&dir.0, QUIT).unwrap();
    assert_eq!(recv(&rx).name, QUIT);
  }

  /// A sender that trickles bytes without a line is let go after [READ_TIMEOUT]: the next one
  /// gets through.
  #[cfg(unix)]
  #[test]
  fn a_slow_sender_does_not_hold_the_instance() {
    use std::io::Write;

    let dir = Dir::new("slow");
    let (_instance, rx) = serve(&dir.0);
    let path = imp::socket_path(&dir.0).unwrap();
    let slow = thread::spawn(move || {
      let mut s = std::os::unix::net::UnixStream::connect(path).unwrap();
      for _ in 0..6 {
        if s.write_all(b"x").is_err() {
          break;
        }

        thread::sleep(Duration::from_millis(300));
      }
    });
    thread::sleep(Duration::from_millis(50));
    send_in(&dir.0, OPEN).unwrap();
    assert_eq!(recv(&rx).name, OPEN, "the slow one dropped");
    slow.join().unwrap();
  }

  /// It waits for the instance to quit, at once when none runs, and gives up after the timeout.
  #[test]
  fn quit_and_wait_returns_once_the_lock_is_free() {
    let dir = Dir::new("quit");
    quit_and_wait_in(&dir.0, Duration::ZERO).unwrap();
    quit_and_wait_in(&dir.0.join("none"), Duration::ZERO).unwrap();

    let (instance, rx) = serve(&dir.0);
    let took = Instant::now();
    let err = quit_and_wait_in(&dir.0, Duration::from_millis(100)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::TimedOut);
    let took = took.elapsed();
    assert!(took >= Duration::from_millis(100) && took < Duration::from_millis(600), "{took:?}");
    assert_eq!(recv(&rx).name, QUIT);

    // The app quits on it, from its loop.
    let app = thread::spawn(move || {
      assert_eq!(recv(&rx).name, QUIT);
      drop(instance);
    });
    quit_and_wait_in(&dir.0, Duration::from_secs(5)).unwrap();
    app.join().unwrap();
    assert!(matches!(claim_in(&dir.0).unwrap(), Claim::Primary(_)));
  }

  /// A second launch that finds the lock taken before the first listens (both started at once)
  /// waits for it.
  #[test]
  fn a_send_waits_for_an_instance_that_is_starting() {
    let dir = Dir::new("starting");
    let lock = imp::lock_file(&dir.0.join(LOCK)).unwrap();
    lock.try_lock().unwrap();
    let path = dir.0.clone();
    let first = thread::spawn(move || {
      thread::sleep(Duration::from_millis(200));
      let mut instance = Instance(imp::Server::bind(&path, lock).unwrap());
      let (tx, rx) = mpsc::channel();
      instance.listen(move |cmd| _ = tx.send(cmd)).unwrap();
      (instance, rx)
    });
    send_in(&dir.0, OPEN).unwrap();
    let (_instance, rx) = first.join().unwrap();
    assert_eq!(recv(&rx).name, OPEN);
  }

  #[test]
  fn lines_round_trip() {
    let cmd = |name: &str, token: Option<&str>| Command {
      name: name.into(),
      token: token.map(String::from),
    };
    let line = |c: &Command| c.line();
    for c in
      [cmd("open", None), cmd("open", Some("x11-123_TIME9")), cmd("show settings", Some("t"))]
    {
      let l = line(&c).unwrap();
      assert!(l.ends_with('\n') && l.matches('\n').count() == 1, "{l:?}");
      assert_eq!(Command::parse(&l), Some(c.clone()));
    }

    assert_eq!(line(&cmd("open", Some("t"))).unwrap(), "open\tt\n");
    assert_eq!(Command::parse("open\r\n"), Some(cmd("open", None)), "a Windows line");
    assert_eq!(Command::parse(" quit "), Some(cmd("quit", None)), "no line break");
    assert_eq!(Command::parse("open\t"), Some(cmd("open", None)));
    assert_eq!(Command::parse("open\tt\nmore\n"), Some(cmd("open", Some("t"))), "the first line");
    for bad in ["", "\n", "\tt\n", " \n"] {
      assert_eq!(Command::parse(bad), None, "{bad:?}");
    }

    for bad in ["", " ", "a\tb", "a\nb", "a\rb", &"x".repeat(MAX_LINE)] {
      assert_eq!(line(&cmd(bad, None)), None, "{bad:?}");
    }

    // A token that can't be sent is left out.
    let long = "t".repeat(MAX_LINE);
    for token in ["a\tb", "a\nb", "", long.as_str()] {
      assert_eq!(line(&cmd("open", Some(token))).unwrap(), "open\n", "{token:?}");
    }
  }

  /// Windows' pipe as a fake: the line in pieces as they come, a sender that sends nothing, one
  /// that goes without a line break, one that sends too much.
  #[test]
  fn polled_lines() {
    let polled = |pieces: &[&str], gone: bool| {
      let queue: VecDeque<Vec<u8>> = pieces.iter().map(|p| p.as_bytes().to_vec()).collect();
      let queue = RefCell::new(queue);
      let peek = || match queue.borrow().front() {
        Some(p) => Some(p.len()),
        None => (!gone).then_some(0),
      };
      let read = |buf: &mut [u8]| {
        let mut q = queue.borrow_mut();
        let p = q.front_mut()?;
        let n = buf.len().min(p.len());
        buf[..n].copy_from_slice(&p[..n]);
        p.drain(..n);
        if p.is_empty() {
          q.pop_front();
        }

        Some(n)
      };
      read_polled(peek, read, Duration::from_millis(30))
    };

    assert_eq!(polled(&["op", "en\tt", "ok\n"], false).as_deref(), Some("open\ttok\n"));
    assert_eq!(polled(&["quit"], true).as_deref(), Some("quit"), "the sender's end ends it");
    assert_eq!(polled(&["open\nmore"], true).as_deref(), Some("open\n"), "the first line");
    assert_eq!(polled(&[], false), None, "nothing sent within the time");
    assert_eq!(polled(&["qu"], false), None, "half a line within the time");
    let long = "x".repeat(MAX_LINE + 10);
    assert_eq!(polled(&[&long], false).map(|l| l.len()), Some(MAX_LINE));
  }

  /// [RELAUNCH]'s value for the process `pid` and the folder key `k`, its token written in `dir`.
  fn token(dir: &Path, pid: u32, k: u64) -> String {
    let v = format!("{pid}:{k:016x}:1");
    fs::write(dir.join(TOKEN), &v).unwrap();
    v
  }

  /// A relaunch waits for the instance of its folder to let go, not for another folder's, and
  /// takes the token; a value without its token, as a relaunch's children inherit it, isn't waited
  /// on.
  #[test]
  fn a_relaunch_waits_for_the_old_instance() {
    let dir = Dir::new("relaunch");
    let Ok(Claim::Primary(first)) = claim_in(&dir.0) else { panic!("not the first") };
    let (me, k) = (std::process::id(), key(&dir.0));
    let claim = |wait: &str, most, late| claim_until(&dir.0, Some(wait), most, late).unwrap();
    let (short, long) = (Duration::from_millis(100), Duration::from_secs(5));
    let took = Instant::now();
    let other = token(&dir.0, me, k ^ 1);
    assert!(matches!(claim(&other, long, long), Claim::Running), "another folder's");
    let old = token(&dir.0, me, k);
    fs::write(dir.0.join(TOKEN), format!("{me}:{k:016x}:2")).unwrap();
    assert!(matches!(claim(&old, long, long), Claim::Running), "another token");
    fs::remove_file(dir.0.join(TOKEN)).unwrap();
    assert!(matches!(claim(&old, long, long), Claim::Running), "no token");
    assert!(took.elapsed() < short, "{:?}", took.elapsed());

    let wait = token(&dir.0, me, k);
    let quit = thread::spawn(move || {
      thread::sleep(Duration::from_millis(200));
      drop(first);
    });
    assert!(matches!(claim(&wait, long, long), Claim::Primary(_)));
    assert!(took.elapsed() >= Duration::from_millis(200));
    assert!(!dir.0.join(TOKEN).exists(), "the token is taken");
    quit.join().unwrap();
  }

  /// A relaunch whose old instance ran on, or ended with its lock still held, is
  /// [Claim::Replaced] once its time is up, its token taken; a lock let go late (Windows) is still
  /// taken.
  #[test]
  fn a_relaunch_gives_up_on_an_instance_that_never_lets_go() {
    let dir = Dir::new("replaced");
    let Ok(Claim::Primary(first)) = claim_in(&dir.0) else { panic!("not the first") };
    // No process of its own: one spawned here would hold other tests' locks for a moment.
    let (me, ended, k) = (std::process::id(), i32::MAX as u32, key(&dir.0));
    let claim = |pid, most, late| claim_until(&dir.0, Some(&token(&dir.0, pid, k)), most, late);
    let (short, long) = (Duration::from_millis(100), Duration::from_secs(5));
    for (pid, most, late) in [(me, short, long), (ended, long, short)] {
      let took = Instant::now();
      assert!(matches!(claim(pid, most, late).unwrap(), Claim::Replaced), "{pid}");
      let took = took.elapsed();
      assert!(took >= short && took < Duration::from_secs(2), "{pid}: {took:?}");
      assert!(!dir.0.join(TOKEN).exists(), "{pid}: the token is taken");
    }

    let quit = thread::spawn(move || {
      thread::sleep(Duration::from_millis(200));
      drop(first);
    });
    assert!(matches!(claim(ended, short, long).unwrap(), Claim::Primary(_)), "the late lock");
    quit.join().unwrap();
  }

  #[test]
  fn relaunch_values_parse() {
    assert_eq!(relaunched("12:00ab:345"), Some((12, 0xab)));
    for bad in ["", "12", "12:", ":ab", "x:ab:1", "12:xy:1", "-1:ab:1", "12:00ab"] {
      assert_eq!(relaunched(bad), None, "{bad:?}");
    }
  }

  #[test]
  fn hashes_as_fnv_1a() {
    assert_eq!((hash(b""), hash(b"a")), (0xcbf2_9ce4_8422_2325, 0xaf63_dc4c_8601_ec8c));
  }
}
