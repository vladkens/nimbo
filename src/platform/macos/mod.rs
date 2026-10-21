// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The macOS platform: NSApplication and its windows ([shell]), each window's content our NSView
//! ([view]: display link, occlusion, present through [surface]) and the performance cores.

mod view;

mod shell;
mod surface;

use crate::Config;
use crate::app::App;

/// Opens the app's window and runs the AppKit event loop until it closes or the app quits: the
/// shell with one window.
pub fn run(cfg: &Config, app: Box<dyn App>) {
  shell::run(cfg.title, || {
    let id = shell::open(cfg, app);
    shell::on_close(id, Box::new(shell::quit));
  });
}

/// The performance cores of an Apple Silicon Mac (`hw.perflevel0.physicalcpu`).
pub fn perf_cores() -> Option<usize> {
  use std::ffi::{c_char, c_int, c_void};

  // libSystem, <sys/sysctl.h>.
  unsafe extern "C" {
    fn sysctlbyname(
      name: *const c_char,
      old: *mut c_void,
      old_len: *mut usize,
      new: *mut c_void,
      new_len: usize,
    ) -> c_int;
  }

  let (mut n, mut len) = (0i32, size_of::<i32>());
  // SAFETY: a NUL-terminated name and an int-sized output buffer with its length.
  let rc = unsafe {
    sysctlbyname(
      c"hw.perflevel0.physicalcpu".as_ptr(),
      (&raw mut n).cast(),
      &mut len,
      std::ptr::null_mut(),
      0,
    )
  };
  (rc == 0 && n > 0).then_some(n as usize)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn perf_cores_are_found() {
    assert!(perf_cores().is_some_and(|n| n >= 1));
  }
}
