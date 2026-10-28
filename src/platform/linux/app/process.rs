// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The update's processes on every Unix: whether one runs, the installer's wait for the app that
//! started it, and the installer started apart from that app.

use std::os::unix::process::{CommandExt, parent_id};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

/// Whether the process `pid` runs: `kill(pid, 0)`, which sends nothing. Another user's is not ours.
pub fn alive(pid: u32) -> bool {
  unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
  }

  // SAFETY: signal 0 only checks; a pid of 0 or past i32 would name a group, never asked.
  i32::try_from(pid).is_ok_and(|pid| pid > 0 && unsafe { kill(pid, 0) } == 0)
}

/// Whether the process `pid`, this one's parent, ended within `cap`: this one is reparented then,
/// and no other process can become its parent, so a reused pid can't fool the wait.
pub fn wait_exit(pid: u32, cap: Duration) -> bool {
  let deadline = Instant::now() + cap;
  while parent_id() == pid && Instant::now() < deadline {
    thread::sleep(Duration::from_millis(20));
  }

  parent_id() != pid
}

/// Spawns `cmd` in a process group of its own, so the terminal's hangup that ends the app doesn't
/// reach it.
pub fn detach(cmd: &mut Command) -> std::io::Result<Child> {
  cmd.process_group(0).spawn()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_wait_ends_with_the_parent_or_the_cap() {
    let start = Instant::now();
    assert!(wait_exit(parent_id().wrapping_add(1), Duration::from_secs(5)), "not the parent");
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(!wait_exit(parent_id(), Duration::from_millis(60)), "the parent runs");
    assert!(start.elapsed() >= Duration::from_millis(60));
    assert!(alive(std::process::id()) && !alive(0));
  }
}
