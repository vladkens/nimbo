// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The main thread's loop as the services see it: work posted to run there after a delay, run
//! only in the loop it was posted to, so work posted after a loop ended never runs in a later
//! one. The shell's [crate::Proxy] runs on it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::platform::shell as imp;

/// The loop running now, by number (0: none).
static LOOP: AtomicU64 = AtomicU64::new(0);
static LOOPS: AtomicU64 = AtomicU64::new(0);

/// Runs `run`, the main thread's loop, as the loop work posted meanwhile belongs to.
pub(crate) fn running(run: impl FnOnce()) {
  LOOP.store(LOOPS.fetch_add(1, Ordering::Relaxed) + 1, Ordering::Release);
  run();
  LOOP.store(0, Ordering::Release);
}

/// Runs `f` on the main thread after `delay`, in the loop running now; with none, never.
pub(crate) fn after(delay: Duration, f: impl FnOnce() + Send + 'static) {
  let n = LOOP.load(Ordering::Acquire);
  if n == 0 {
    return;
  }

  imp::post(
    delay,
    Box::new(move || {
      if LOOP.load(Ordering::Acquire) == n {
        f();
      }
    }),
  );
}
