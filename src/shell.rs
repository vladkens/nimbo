// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app around the windows: an event loop that runs with no window ([app]), windows opened and
//! closed on demand ([Main::open]), each with its own [Runtime](crate::Runtime), where the app
//! shows besides them ([Presence]), and the way other threads reach the main thread ([Proxy]).
//! [crate::run] is one window on top of it, quitting when it closes.
//!
//! Everything here runs on the main thread: [Main] is its token, given to the start closure, to
//! [Proxy]'s closures and to the callbacks, and never leaves it. Each OS implements it in
//! `platform/<os>` (macOS: `shell.rs`; Windows and Linux: `app/shell.rs`); what they keep alike
//! (each window's app type and close callback, the reopen and quit hooks) is kept here.

use std::any::TypeId;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use crate::platform::shell as imp;
use crate::{App, Config};

/// The app as the OS knows it.
#[derive(Clone, Copy, Debug)]
pub struct AppInfo {
  /// Reverse DNS, such as `com.example.app`: its files ([crate::paths]), its launch at login
  /// ([crate::autostart]), its tray item on Linux.
  pub id: &'static str,
  /// What the user reads: the app menu's "Quit {name}", the login item.
  pub name: &'static str,
  pub presence: Presence,
}

/// Where the app shows besides its windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Presence {
  /// In the Dock and in ⌘Tab, as most apps.
  #[default]
  Regular,
  /// A menu-bar or tray app: no Dock icon, except while one of its windows is shown (macOS: the
  /// accessory activation policy, regular while a window is open). Windows and Linux have no
  /// Dock: the taskbar shows the windows only, so both are the same there.
  Accessory,
}

/// Runs the event loop with no window: `start` opens what the app needs (windows, a
/// [crate::tray::Tray], [crate::hotkey::Hotkeys]), then the loop runs until [Main::quit].
/// Closing the last window does not quit. Returns after the loop ends; [Main::quit] inside
/// `start` returns at once, without running the loop.
///
/// The app's cleanup goes into [Main::on_quit], not after `app`: when the system quits the app
/// (macOS: the Dock's Quit, logout, all `terminate:`; Windows: the session ends), the hook runs
/// from the loop and the process exits after it, so `app` does not return. After [Main::quit]
/// the hook runs once the loop has stopped, then `app` returns.
pub fn app(info: AppInfo, start: impl FnOnce(&mut Main)) {
  running(|| imp::run(&info, start));
}

/// The loop running now, by number (0: none): [Proxy]'s work runs only in the loop it was
/// posted to, so work posted after it ended is dropped, and never runs in a later one.
static LOOP: AtomicU64 = AtomicU64::new(0);
static LOOPS: AtomicU64 = AtomicU64::new(0);

fn running(run: impl FnOnce()) {
  LOOP.store(LOOPS.fetch_add(1, Ordering::Relaxed) + 1, Ordering::Release);
  HOOKS.take();
  run();
  HOOKS.take();
  LOOP.store(0, Ordering::Release);
}

pub(crate) type OnMain = Box<dyn FnOnce(&mut Main)>;
type OnReopen = Box<dyn FnMut(&mut Main)>;

/// What every platform's loop keeps alike: the open windows by id, each with its app's type and
/// close callback, and the reopen and quit hooks.
#[derive(Default)]
struct Hooks {
  windows: Vec<(u64, TypeId, Option<OnMain>)>,
  reopen: Option<OnReopen>,
  quit: Option<OnMain>,
}

thread_local! {
  static HOOKS: RefCell<Hooks> = RefCell::default();
}

fn hooks<R>(f: impl FnOnce(&mut Hooks) -> R) -> R {
  HOOKS.with_borrow_mut(f)
}

/// The platform opened window `id`, running an app of type `ty`: kept where the platform keeps
/// it, before the window can run app code.
pub(crate) fn opened(id: u64, ty: TypeId) {
  hooks(|h| h.windows.push((id, ty, None)));
}

/// The type of window `id`'s app; `None` once it closed.
fn type_of(id: u64) -> Option<TypeId> {
  hooks(|h| h.windows.iter().find(|w| w.0 == id).map(|w| w.1))
}

fn on_close(id: u64, f: OnMain) {
  hooks(|h| {
    if let Some(w) = h.windows.iter_mut().find(|w| w.0 == id) {
      w.2 = Some(f);
    }
  });
}

fn windows() -> Vec<u64> {
  hooks(|h| h.windows.iter().map(|w| w.0).collect())
}

/// Window `id` closes: it is no longer open, and its close callback is given back, for the
/// platform to run once it has let the window go.
pub(crate) fn closed(id: u64) -> Option<OnMain> {
  hooks(|h| {
    let i = h.windows.iter().position(|w| w.0 == id)?;
    h.windows.remove(i).2
  })
}

/// The app was opened again: its reopen hook runs, kept for the next time.
#[cfg(target_os = "macos")]
pub(crate) fn reopened() {
  let Some(mut f) = hooks(|h| h.reopen.take()) else { return };
  f(&mut Main::here());
  hooks(|h| _ = h.reopen.get_or_insert(f));
}

/// The app ends: its quit hook runs (once), then the platform's `close` closes each window.
pub(crate) fn finish(close: impl Fn(u64)) {
  if let Some(f) = hooks(|h| h.quit.take()) {
    f(&mut Main::here());
  }

  for id in windows() {
    close(id);
  }
}

/// One window running `app` with the loop around it, which ends when the window closes or the
/// app quits: [crate::run] without `--shot`.
pub(crate) fn window(cfg: &Config, app: Box<dyn App>) {
  let exe = std::env::current_exe().ok();
  let stem = exe.as_ref().and_then(|p| Some(p.file_stem()?.to_str()?.to_owned()));
  // The loop runs once per process: the strings live as long as it.
  let id: &'static str = Box::leak(stem.unwrap_or_else(|| "nimbo".into()).into_boxed_str());
  let info = AppInfo { id, name: cfg.title, presence: Presence::Regular };
  running(|| {
    imp::run(&info, |_| {
      let id = imp::open(cfg, app, TypeId::of::<()>());
      on_close(id, Box::new(Main::quit));
    })
  });
}

/// The main thread while the app runs: only code on it gets one (`start`, [Proxy]'s closures,
/// the callbacks). Not `Send`.
pub struct Main {
  _main: PhantomData<*mut ()>,
}

impl Main {
  /// The token; the caller is on the main thread.
  pub(crate) fn here() -> Self {
    Self { _main: PhantomData }
  }

  /// Opens a window running `app` with its own runtime, in front unless `cfg` is
  /// [hidden](Config::hidden).
  pub fn open<A: App + 'static>(&mut self, cfg: &Config, app: A) -> Window<A> {
    let id = imp::open(cfg, Box::new(app), TypeId::of::<A>());
    Window::new(WindowId(id))
  }

  /// The window `id` if it is open and runs an `A`: for a [Proxy] closure, which gets the id
  /// from another thread.
  pub fn window<A: App + 'static>(&self, id: WindowId) -> Option<Window<A>> {
    (type_of(id.0)? == TypeId::of::<A>()).then(|| Window::new(id))
  }

  /// Where the app shows besides its windows ([Presence]).
  pub fn set_presence(&mut self, p: Presence) {
    imp::set_presence(p);
  }

  /// `f` runs when the app is opened again while it runs (macOS: the Dock icon, Finder,
  /// Spotlight, `open`), as for a second launch: a menu-bar app shows its settings. macOS only
  /// for now: Windows and Linux start a second process.
  pub fn on_reopen(&mut self, f: impl FnMut(&mut Main) + 'static) {
    hooks(|h| h.reopen = Some(Box::new(f)));
  }

  /// Closes the windows and ends the loop once the work already queued is done: [app] returns.
  pub fn quit(&mut self) {
    imp::quit();
  }

  /// `f` runs once as the app ends, before its windows close (replacing an earlier one): after
  /// [Main::quit] once the loop has stopped, or on a quit the system asks for, after which the
  /// process exits (see [app]). The place for the app's cleanup.
  pub fn on_quit(&mut self, f: impl FnOnce(&mut Main) + 'static) {
    hooks(|h| h.quit = Some(Box::new(f)));
  }

  /// The system asked the app to quit: [Main::on_quit]'s hook runs next and the process exits.
  pub fn terminating(&self) -> bool {
    imp::terminating()
  }

  pub fn proxy(&self) -> Proxy {
    Proxy(())
  }
}

/// A window's id: `Send`, for a [Proxy] closure to find the window again ([Main::window]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(pub(crate) u64);

/// A window running an `A`: a handle, cheap to clone; the window lives until it is closed, by
/// the user or [Window::close], whether or not handles remain. Main thread only.
pub struct Window<A> {
  id: WindowId,
  _app: PhantomData<*const A>,
}

impl<A> Clone for Window<A> {
  fn clone(&self) -> Self {
    Self { id: self.id, _app: PhantomData }
  }
}

impl<A: App + 'static> Window<A> {
  fn new(id: WindowId) -> Self {
    Self { id, _app: PhantomData }
  }

  pub fn id(&self) -> WindowId {
    self.id
  }

  /// Not closed yet.
  pub fn is_open(&self) -> bool {
    type_of(self.id.0).is_some()
  }

  /// Shows the window in front, restored if minimized, and the app with it.
  pub fn front(&self) {
    imp::front(self.id.0);
  }

  /// Closes the window: its runtime and app are dropped, its close callback runs.
  pub fn close(&self) {
    imp::close(self.id.0);
  }

  /// `f` runs when the window closes, by the user or [Window::close] (replacing an earlier one).
  pub fn on_close(&self, f: impl FnOnce(&mut Main) + 'static) {
    on_close(self.id.0, Box::new(f));
  }

  /// Runs `f` on the window's app now and redraws it: new data from outside the window. `None`
  /// when the window is closed, or busy further up the stack (called from inside its own event
  /// or view: there the app has itself already).
  pub fn with<R>(&self, f: impl FnOnce(&mut A) -> R) -> Option<R> {
    if type_of(self.id.0) != Some(TypeId::of::<A>()) {
      return None;
    }

    let mut f = Some(f);
    let mut out = None;
    imp::with_app(self.id.0, &mut |app| {
      // SAFETY: the window was opened with an `A` (its type id is checked against the one
      // `open` kept), so the box holds an `A`.
      let app = unsafe { &mut *(app as *mut dyn App as *mut A) };
      out = f.take().map(|f| f(app));
    });
    out
  }
}

/// Hands work to the main thread from any thread: `Clone`, `Send` and `Sync`. It runs from the
/// loop also while a menu is open or a window resizes (macOS: the main dispatch queue, in every
/// run-loop mode; Windows: a message to the app's hidden window; Linux: a GLib source). Work
/// posted after the loop ended is dropped unrun.
#[derive(Clone, Debug)]
pub struct Proxy(());

impl Proxy {
  /// Runs `f` on the main thread, from the loop, after the work already queued.
  pub fn post(&self, f: impl FnOnce(&mut Main) + Send + 'static) {
    self.after(Duration::ZERO, f);
  }

  /// Runs `f` on the main thread after `delay`.
  pub fn after(&self, delay: Duration, f: impl FnOnce(&mut Main) + Send + 'static) {
    let n = LOOP.load(Ordering::Acquire);
    if n == 0 {
      return;
    }

    imp::post(
      delay,
      Box::new(move || {
        if LOOP.load(Ordering::Acquire) == n {
          f(&mut Main::here());
        }
      }),
    );
  }

  /// Runs `f` on the main thread and returns what it returns: at once on the main thread;
  /// elsewhere `None` if the main thread did not get to it within `timeout` (`f` still runs
  /// later). The main thread never waits for another, so this cannot deadlock it.
  pub fn call<R: Send + 'static>(
    &self,
    f: impl FnOnce(&mut Main) -> R + Send + 'static,
    timeout: Duration,
  ) -> Option<R> {
    if imp::is_main() {
      return Some(f(&mut Main::here()));
    }

    let (tx, rx) = mpsc::sync_channel(1);
    self.post(move |main| {
      let _ = tx.send(f(main));
    });
    rx.recv_timeout(timeout).ok()
  }
}

#[cfg(test)]
mod tests {
  use std::cell::Cell;
  use std::rc::Rc;

  use super::*;

  /// The windows by id: a close callback set on one is given back once, as it closes; the quit
  /// hook runs once.
  #[test]
  fn windows_and_hooks_by_id() {
    hooks(|h| h.windows.extend([(1, TypeId::of::<u8>(), None), (2, TypeId::of::<u16>(), None)]));
    on_close(2, Box::new(|_| {}));
    on_close(3, Box::new(|_| {}));
    assert_eq!((windows(), type_of(2), type_of(3)), (vec![1, 2], Some(TypeId::of::<u16>()), None));
    assert!(closed(1).is_none());
    assert!(closed(2).is_some());
    assert!(closed(2).is_none());
    assert_eq!((windows(), type_of(2)), (vec![], None));

    let ran = Rc::new(Cell::new(0));
    let r = ran.clone();
    Main::here().on_quit(move |_| r.set(r.get() + 1));
    hooks(|h| h.windows.push((4, TypeId::of::<u8>(), None)));
    let closing = Rc::new(Cell::new(0));
    finish(|id| closing.set(id));
    finish(|_| {});
    assert_eq!((ran.get(), closing.get()), (1, 4));

    // The reopen hook is kept after it runs.
    #[cfg(target_os = "macos")]
    {
      let r = ran.clone();
      Main::here().on_reopen(move |_| r.set(r.get() + 1));
      reopened();
      reopened();
      assert_eq!(ran.get(), 3);
    }
  }
}
