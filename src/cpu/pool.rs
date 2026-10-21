// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The threads that help the calling thread draw a big frame: made on the first frame that asks
//! for them, then parked on a condvar between frames, so an idle window has no wakeups from them
//! and a frame pays a wake instead of a thread spawn. One pool per process; a renderer that finds
//! it busy (another renderer drawing on another thread) draws alone.

use std::panic::{self, AssertUnwindSafe};
use std::sync::{Condvar, Mutex, OnceLock};

/// A task lent to the helpers for one [Pool::run], which doesn't return before every helper that
/// took it is done with it.
#[derive(Clone, Copy)]
struct Task(*const (dyn Fn() + Sync));

// SAFETY: the task is `Sync` and outlives every use of it (see [Pool::run]).
unsafe impl Send for Task {}

#[derive(Default)]
struct State {
  task: Option<Task>,
  /// Helpers still to start the task.
  open: usize,
  /// Helpers running it.
  busy: usize,
  panicked: bool,
  /// Helper threads made so far.
  threads: usize,
}

#[derive(Default)]
pub struct Pool {
  /// Held for a whole [Pool::run]: one task at a time.
  gate: Mutex<()>,
  state: Mutex<State>,
  start: Condvar,
  done: Condvar,
}

impl Pool {
  /// The process's pool, its threads made only when a run asks for them.
  pub fn get() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(Pool::default)
  }

  /// Runs `f` on the calling thread and on `helpers` pool threads at once, returning when all of
  /// them have returned; `f` shares the work out itself (a queue). Panics if any of them did.
  pub fn run(&'static self, helpers: usize, f: &(dyn Fn() + Sync)) {
    if helpers == 0 {
      return f();
    }

    let Ok(_gate) = self.gate.try_lock() else { return f() };

    let mut st = self.state.lock().unwrap();
    for n in st.threads..helpers {
      let spawned =
        std::thread::Builder::new().name(format!("nimbo-draw-{n}")).spawn(|| self.work());
      if spawned.is_err() {
        break;
      }

      st.threads += 1;
    }
    // SAFETY: only the lifetime is erased; the task is cleared below, after every helper that
    // started it is done, before `f` can go out of scope.
    let task: &'static (dyn Fn() + Sync) = unsafe { std::mem::transmute(f) };
    st.task = Some(Task(task));
    st.open = helpers.min(st.threads);
    for _ in 0..st.open {
      self.start.notify_one();
    }

    drop(st);

    let mine = panic::catch_unwind(AssertUnwindSafe(f));
    let mut st = self.state.lock().unwrap();
    // Helpers that haven't woken yet would find the work taken: they stay asleep.
    st.open = 0;
    while st.busy > 0 {
      st = self.done.wait(st).unwrap();
    }

    st.task = None;
    let theirs = std::mem::take(&mut st.panicked);
    drop(st);

    if let Err(e) = mine {
      panic::resume_unwind(e);
    }

    assert!(!theirs, "a drawing thread panicked");
  }

  /// A helper: waits for a task, runs it, reports back, forever.
  fn work(&self) {
    let mut st = self.state.lock().unwrap();
    loop {
      if st.open == 0 {
        st = self.start.wait(st).unwrap();
        continue;
      }

      st.open -= 1;
      st.busy += 1;
      let Some(Task(task)) = st.task else { unreachable!("open without a task") };
      drop(st);

      // SAFETY: `run` keeps the task alive until `busy` is back to 0.
      let ok = panic::catch_unwind(AssertUnwindSafe(|| unsafe { (*task)() })).is_ok();
      st = self.state.lock().unwrap();
      st.busy -= 1;
      st.panicked |= !ok;
      if st.busy == 0 {
        self.done.notify_one();
      }
    }
  }
}

/// Threads to draw a big frame with: the performance cores, as the platform tells. On an M2
/// all 8 cores (4 of them efficiency ones) draw a full frame a fifth faster than the 4
/// performance cores, for a quarter more CPU time.
pub fn threads() -> usize {
  // Unknown, no more than 4: past that the cores are likely efficiency ones or busy.
  let cores = || std::thread::available_parallelism().map_or(1, |n| n.get().min(4));
  // The platform's count of performance cores comes with the platforms.
  cores()
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicUsize, Ordering};

  use super::*;

  #[test]
  fn helpers_share_the_work() {
    let next = AtomicUsize::new(0);
    let sum = AtomicUsize::new(0);
    for _ in 0..50 {
      next.store(0, Ordering::Relaxed);
      sum.store(0, Ordering::Relaxed);
      Pool::get().run(3, &|| {
        loop {
          let i = next.fetch_add(1, Ordering::Relaxed);
          if i >= 1000 {
            break;
          }

          sum.fetch_add(i, Ordering::Relaxed);
        }
      });
      assert_eq!(sum.load(Ordering::Relaxed), 999 * 1000 / 2);
    }
  }

  #[test]
  fn a_helper_panic_reaches_the_caller() {
    let pool: &'static Pool = Box::leak(Box::default());
    let main = std::thread::current().id();
    let started = AtomicUsize::new(0);
    let got = panic::catch_unwind(|| {
      pool.run(2, &|| {
        if std::thread::current().id() != main {
          started.fetch_add(1, Ordering::Relaxed);
          panic!("helper");
        }
        // Waits for a helper to take the task, at most a second.
        for _ in 0..1000 {
          if started.load(Ordering::Relaxed) > 0 {
            break;
          }

          std::thread::sleep(std::time::Duration::from_millis(1));
        }
      })
    });
    assert!(got.is_err());
    // The pool still works.
    let n = AtomicUsize::new(0);
    pool.run(2, &|| _ = n.fetch_add(1, Ordering::Relaxed));
    assert!((1..=3).contains(&n.load(Ordering::Relaxed)));
  }
}
