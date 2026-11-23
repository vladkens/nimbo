// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The main thread's loop as the services see it: work posted to run there after a delay, run
//! only in the loop it was posted to, so work posted after a loop ended never runs in a later
//! one. The shell's [crate::Proxy] runs on it, and the shell hands in how work is posted
//! (the platform's, whose loop it runs).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Runs work on the main thread after a delay, from any thread: the platform's.
pub(crate) type Post = fn(Duration, Box<dyn FnOnce() + Send>);

/// The loop running now, by number (0: none).
static LOOP: AtomicU64 = AtomicU64::new(0);
static LOOPS: AtomicU64 = AtomicU64::new(0);
static POST: OnceLock<Post> = OnceLock::new();

/// Runs `run`, the main thread's loop, as the loop work posted meanwhile belongs to; `post`
/// posts to it.
pub(crate) fn running(post: Post, run: impl FnOnce()) {
  POST.get_or_init(|| post);
  LOOP.store(LOOPS.fetch_add(1, Ordering::Relaxed) + 1, Ordering::Release);
  run();
  LOOP.store(0, Ordering::Release);
}

/// Runs `f` on the main thread after `delay`, in the loop running now; with none, never.
pub(crate) fn after(delay: Duration, f: impl FnOnce() + Send + 'static) {
  let (n, Some(post)) = (LOOP.load(Ordering::Acquire), POST.get()) else { return };
  if n == 0 {
    return;
  }

  post(
    delay,
    Box::new(move || {
      if LOOP.load(Ordering::Acquire) == n {
        f();
      }
    }),
  );
}
