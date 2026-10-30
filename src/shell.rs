// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app around the windows: an event loop that runs with no window ([app]), windows opened and
//! closed on demand ([Main::open]), each with its own [Runtime](crate::Runtime), where the app
//! shows besides them ([Presence]), and the way other threads reach the main thread ([Proxy]).
//! [crate::run] is one window on top of it, quitting when it closes.
//!
//! Everything here runs on the main thread: [Main] is its token, given to the start closure, to
//! [Proxy]'s closures and to the callbacks, and never leaves it. Each OS implements it in
//! `platform/<os>` (macOS: `shell.rs`; Windows and Linux: `app/shell.rs`); what they keep alike
//! (each window's app type and close callback, the reopen and quit hooks, the app's one instance
//! that [Main::serve] keeps) is kept here.

use std::any::TypeId;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use crate::instance::{Command, Instance, OPEN, QUIT};
use crate::platform::shell as imp;
use crate::system::{self, Change};
use crate::{App, Config, Event, Point, Size};

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

/// Where an overlay ([crate::Config::overlay]) shows: centred across the work area (the screen
/// less the menu bar, the Dock or the taskbar) of the screen with the pointer, a margin in points
/// from its top or bottom edge, or in its middle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
  Top(f32),
  Center,
  Bottom(f32),
}

impl Anchor {
  /// The top-left corner of a window of `size` in a work area of `area`, from the area's top
  /// left (y down), in whole points.
  pub fn offset(self, area: Size, size: Size) -> Point {
    let y = match self {
      Self::Top(margin) => margin,
      Self::Center => (area.h - size.h) / 2.0,
      Self::Bottom(margin) => area.h - size.h - margin,
    };

    Point::new(((area.w - size.w) / 2.0).round(), y.round())
  }
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
  system::unwatch();
  HOOKS.take();
  LOOP.store(0, Ordering::Release);
}

pub(crate) type OnMain = Box<dyn FnOnce(&mut Main)>;
type OnReopen = Box<dyn FnMut(&mut Main)>;
type OnCommand = Box<dyn FnMut(&mut Main, &Command)>;
type OnChange = Box<dyn FnMut(&mut Main, Change)>;

/// What every platform's loop keeps alike: the open windows by id, each with its app's type and
/// close callback, the reopen and quit hooks, and the instance served with its handler, the
/// commands that came while one runs, the activation token of the one it handles (and whether a
/// window used it), and the hook of the system's changes with those that came while it runs.
#[derive(Default)]
struct Hooks {
  windows: Vec<(u64, TypeId, Option<OnMain>)>,
  reopen: Option<OnReopen>,
  quit: Option<OnMain>,
  instance: Option<Instance>,
  command: Option<OnCommand>,
  queued: Option<VecDeque<Command>>,
  token: Option<(String, bool)>,
  change: Option<OnChange>,
  changes: Option<VecDeque<Change>>,
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
pub(crate) fn reopened() {
  let Some(mut f) = hooks(|h| h.reopen.take()) else { return };
  f(&mut Main::here());
  hooks(|h| _ = h.reopen.get_or_insert(f));
}

/// A command came to the instance [Main::serve] keeps: the reopen hook for a bare [OPEN], then
/// the handler, the command's activation token kept meanwhile, then [Main::quit] for [QUIT]. One
/// that comes while another runs (from a nested loop: a dialog, a paste waiting for the portal)
/// is queued and runs after it. `quit` is [Main::quit], a test's fake in tests.
fn command(main: &mut Main, cmd: Command, quit: &dyn Fn(&mut Main)) {
  let busy = hooks(|h| h.queued.as_mut().map(|q| q.push_back(cmd.clone())).is_some());
  if busy {
    return;
  }

  hooks(|h| h.queued = Some(VecDeque::new()));
  let mut next = Some(cmd);
  while let Some(cmd) = next {
    handle(main, &cmd, quit);
    next = hooks(|h| h.queued.as_mut().and_then(VecDeque::pop_front));
  }

  hooks(|h| h.queued = None);
}

/// One command, as [command] says.
fn handle(main: &mut Main, cmd: &Command, quit: &dyn Fn(&mut Main)) {
  hooks(|h| h.token = cmd.token.clone().map(|t| (t, false)));
  if cmd.name == OPEN {
    reopened();
  }

  if let Some(mut f) = hooks(|h| h.command.take()) {
    f(main, cmd);
    hooks(|h| _ = h.command.get_or_insert(f));
  }

  if cmd.name == QUIT {
    quit(main);
  }

  // No window came forward with the token: the launcher stops its busy cursor (Linux).
  let _unused = hooks(|h| h.token.take()).filter(|t| !t.1);
  #[cfg(target_os = "linux")]
  if let Some((token, _)) = _unused {
    crate::platform::startup_complete(&token);
  }
}

/// A watched change of the system ([Main::watch]): the hook, then every open window. One that
/// comes while the hook runs (a nested loop up the stack) is queued and delivered after it.
pub(crate) fn changed(main: &mut Main, change: Change) {
  deliver(main, change, imp::send, post);
}

/// The platform's send of an event to a window: `None` when it is busy up the stack or gone.
type SendFn = fn(u64, Event) -> Option<bool>;
/// Work for the main thread after a delay: [Proxy::after], a fake in tests.
type PostFn = fn(Duration, Later);
type Later = Box<dyn FnOnce() + Send>;

fn post(delay: Duration, f: Later) {
  Proxy(()).after(delay, move |_| f());
}

/// [changed] with the platform's `send` to a window and the loop's `post`.
fn deliver(main: &mut Main, change: Change, send: SendFn, post: PostFn) {
  let busy = hooks(|h| h.changes.as_mut().map(|q| q.push_back(change)).is_some());
  if busy {
    return;
  }

  hooks(|h| h.changes = Some(VecDeque::new()));
  let mut next = Some(change);
  while let Some(change) = next {
    if let Some(mut f) = hooks(|h| h.change.take()) {
      f(main, change);
      // Kept unless the hook set another.
      hooks(|h| _ = h.change.get_or_insert(f));
    }

    retry(windows(), change, send, post);
    next = hooks(|h| h.changes.as_mut().and_then(VecDeque::pop_front));
  }

  hooks(|h| h.changes = None);
}

/// Sends `change` to the windows `ids` still open; those busy are tried again after
/// [system::RETRY], until they take it or close.
fn retry(ids: Vec<u64>, change: Change, send: SendFn, post: PostFn) {
  let open = windows();
  let ids = ids.into_iter().filter(|id| open.contains(id));
  let missed: Vec<_> = ids.filter(|&id| send(id, Event::System(change)).is_none()).collect();
  if !missed.is_empty() {
    post(system::RETRY, Box::new(move || retry(missed, change, send, post)));
  }
}

/// The activation token of the command being handled, for a window that shows or comes to the
/// front meanwhile: used by it.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn token() -> Option<String> {
  hooks(|h| {
    let (token, used) = h.token.as_mut()?;
    *used = true;
    Some(token.clone())
  })
}

/// The app ends: its quit hook runs (once), a paste's clipboard comes back ([crate::paste]),
/// then the platform's `close` closes each window.
pub(crate) fn finish(close: impl Fn(u64)) {
  if let Some(f) = hooks(|h| h.quit.take()) {
    f(&mut Main::here());
  }

  crate::paste::finish();

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

/// Prints to the console of the command line that started the app, on Windows, where an app
/// built as a GUI program (`windows_subsystem = "windows"`) has none; nothing elsewhere, where it
/// prints there anyway. Call it only on the app's command-line paths (a command, `--help`, a
/// mistake in the arguments), not for a launch that runs the app: attached, its output would show
/// after the prompt, and closing that console would end it without its quit hook.
pub fn attach_console() {
  #[cfg(windows)]
  crate::platform::attach_console();
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
  /// Spotlight, `open`) and when a second launch sends [OPEN] to the instance [Main::serve]
  /// keeps (Windows and Linux start a second process): a menu-bar app shows its settings.
  pub fn on_reopen(&mut self, f: impl FnMut(&mut Main) + 'static) {
    hooks(|h| h.reopen = Some(Box::new(f)));
  }

  /// Serves the app's one instance ([crate::instance::claim]) until the loop ends, keeping it
  /// and its lock (replacing an earlier one): `f` runs on the main thread with each command
  /// another process sends ([crate::instance::send]), one at a time; a bare [OPEN] runs
  /// [Main::on_reopen]'s hook first, and [QUIT] quits after it ([Main::quit]: the app's cleanup
  /// is in [Main::on_quit]). While they run, a window [Main::open] shows or [Window::front] brings
  /// forward comes to the front with the command's activation token (Linux; Windows lets it
  /// through the sender). `Err` when its thread can't start.
  pub fn serve(
    &mut self,
    mut instance: Instance,
    f: impl FnMut(&mut Main, &Command) + 'static,
  ) -> io::Result<()> {
    let proxy = self.proxy();
    instance.listen(move |cmd| proxy.post(move |main| command(main, cmd, &Main::quit)))?;
    let f: OnCommand = Box::new(f);
    hooks(|h| (h.instance, h.command) = (Some(instance), Some(f)));
    Ok(())
  }

  /// Closes the windows and ends the loop once the work already queued is done: [app] returns.
  pub fn quit(&mut self) {
    imp::quit();
  }

  /// Whether an overlay ([crate::Config::overlay]) takes the keyboard focus while it shows, so
  /// keys typed meanwhile go to it, not to the app before. Only on Linux under Wayland with
  /// neither the layer shell nor XWayland (see the overlay's docs there): an app may show it
  /// less, or not at all.
  pub fn overlay_takes_focus(&self) -> bool {
    imp::overlay_takes_focus()
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

  /// A modal alert with an OK button, in front of every app, that returns once the user closes
  /// it: what failed where no window or terminal shows it (a menu bar app that can't start).
  /// macOS an `NSAlert` (the app activated for it), Windows a `MessageBoxW`, Linux a GTK message
  /// dialog; printed to stderr where no dialog shows (GTK not loaded). Tests can't reach it: it
  /// waits for the user.
  pub fn alert(&mut self, title: &str, message: &str) {
    imp::alert(title, message);
  }

  /// Watches the system's changes of `kinds` (besides those watched already) until the loop
  /// ends ([crate::system]): each, a burst of one kind coalesced into one, runs `f` on the main
  /// thread (replacing an earlier one), then reaches every open window as [Event::System]. `Err`
  /// names the kinds the OS refused; the rest are watched. A kind registered off the main thread
  /// (macOS's audio, whose first call costs tens of milliseconds) or refused later (a sound
  /// server's subscription) can't come back here: it goes to stderr and is no longer watched,
  /// so a later call tries again.
  pub fn watch(
    &mut self,
    kinds: &[Change],
    f: impl FnMut(&mut Main, Change) + 'static,
  ) -> Result<(), String> {
    hooks(|h| h.change = Some(Box::new(f)));
    system::watch(kinds)
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

  /// Shows the window in front, restored if minimized, and the app with it. An overlay
  /// ([Config::overlay]) shows at its [Anchor] on the screen with the pointer instead, without
  /// activating the app or taking the keys.
  pub fn front(&self) {
    imp::front(self.id.0);
  }

  /// Hides the window without closing it: its app and state stay, no frames are drawn, and
  /// [Window::front] shows it again. How an overlay goes away.
  pub fn hide(&self) {
    imp::hide(self.id.0);
  }

  /// Closes the window: its runtime and app are dropped, its close callback runs.
  pub fn close(&self) {
    imp::close(self.id.0);
  }

  /// `f` runs when the window closes, by the user or [Window::close] (replacing an earlier one).
  pub fn on_close(&self, f: impl FnOnce(&mut Main) + 'static) {
    on_close(self.id.0, Box::new(f));
  }

  /// Redraws the window: its view runs again, showing what it reads from outside its app (a
  /// model shared with the main thread). Nothing when it is closed or busy further up the stack
  /// (called from inside its own event or view), as [Window::with].
  pub fn redraw(&self) {
    self.with(|_| {});
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
pub struct Proxy(pub(crate) ());

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

  /// A HUD 200x40 in a 1000x600 work area: centred across, 20 points off the edge.
  #[test]
  fn anchors_place_in_the_work_area() {
    let (area, size) = (Size::new(1000.0, 600.0), Size::new(200.0, 40.5));
    assert_eq!(Anchor::Top(20.0).offset(area, size), Point::new(400.0, 20.0));
    assert_eq!(Anchor::Bottom(20.0).offset(area, size), Point::new(400.0, 540.0));
    assert_eq!(Anchor::Center.offset(area, size), Point::new(400.0, 280.0));
  }

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
    let r = ran.clone();
    Main::here().on_reopen(move |_| r.set(r.get() + 1));
    reopened();
    reopened();
    assert_eq!(ran.get(), 3);
  }

  /// A command runs the handler with its token kept meanwhile; a bare "open" the reopen hook
  /// first, with the token too; "quit" quits after the handler.
  #[test]
  fn commands_run_their_handler_with_the_token() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    Main::here().on_reopen(move |_| s.borrow_mut().push(format!("reopen {:?}", token())));
    let s = seen.clone();
    let f =
      move |_: &mut Main, c: &Command| s.borrow_mut().push(format!("{} {:?}", c.name, token()));
    hooks(|h| h.command = Some(Box::new(f)));
    let s = seen.clone();
    let quit = move |_: &mut Main| s.borrow_mut().push("quits".into());
    command(&mut Main::here(), cmd("open", Some("t1")), &quit);
    command(&mut Main::here(), cmd("quit", None), &quit);
    command(&mut Main::here(), cmd("open now", Some("t2")), &quit);
    let want = [
      r#"reopen Some("t1")"#,
      r#"open Some("t1")"#,
      "quit None",
      "quits",
      r#"open now Some("t2")"#,
    ];
    assert_eq!(*seen.borrow(), want);
    assert_eq!(token(), None, "only while it runs");
    HOOKS.take();
  }

  /// A command that comes while the handler runs (a nested loop dispatches it) waits for it,
  /// then runs with its own token; the first keeps its token meanwhile.
  #[test]
  fn a_command_during_the_handler_runs_after_it() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    let f = move |main: &mut Main, c: &Command| {
      if c.name == "first" {
        command(main, cmd("second", Some("t2")), &|_| {});
      }

      s.borrow_mut().push(format!("{} {:?}", c.name, token()));
    };
    hooks(|h| h.command = Some(Box::new(f)));
    command(&mut Main::here(), cmd("first", Some("t1")), &|_| {});
    assert_eq!(*seen.borrow(), [r#"first Some("t1")"#, r#"second Some("t2")"#]);
    assert_eq!(token(), None);
    HOOKS.take();
  }

  thread_local! {
    static SEEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static BUSY: Cell<u32> = const { Cell::new(0) };
    static POSTED: RefCell<Vec<(Duration, Later)>> = RefCell::default();
  }

  fn seen(s: impl Into<String>) {
    SEEN.with_borrow_mut(|v| v.push(s.into()));
  }

  /// Window 2 is busy the first [BUSY] times it's sent to.
  fn send_busy(id: u64, ev: Event) -> Option<bool> {
    seen(format!("{id} {ev:?}"));
    let busy = id == 2 && BUSY.get() > 0;
    BUSY.set(BUSY.get().saturating_sub(busy as u32));
    (!busy).then_some(true)
  }

  fn post_later(delay: Duration, f: Later) {
    POSTED.with_borrow_mut(|p| p.push((delay, f)));
  }

  fn run_posted() -> Vec<Duration> {
    let run = |(d, f): (Duration, Later)| {
      f();
      d
    };
    POSTED.take().into_iter().map(run).collect()
  }

  fn two_windows() {
    hooks(|h| h.windows.extend([(1, TypeId::of::<u8>(), None), (2, TypeId::of::<u8>(), None)]));
  }

  /// A change runs the hook, then each window; one delivered while the hook runs (a nested loop
  /// up the stack) waits for it.
  #[test]
  fn changes_run_the_hook_then_the_windows() {
    let hook = move |main: &mut Main, c: Change| {
      seen(format!("hook {c:?}"));
      if c == Change::AudioInputs {
        deliver(main, Change::Lid, |_, _| None, post_later);
        seen("hook done");
      }
    };
    hooks(|h| h.change = Some(Box::new(hook)));
    two_windows();
    deliver(&mut Main::here(), Change::AudioInputs, send_busy, post_later);
    let want = [
      "hook AudioInputs",
      "hook done",
      "1 System(AudioInputs)",
      "2 System(AudioInputs)",
      "hook Lid",
      "1 System(Lid)",
      "2 System(Lid)",
    ];
    assert_eq!(SEEN.take(), want);
    assert!(POSTED.take().is_empty(), "no window busy");
    HOOKS.take();
  }

  /// A busy window is tried again after the retry's delay until it takes the change; a window
  /// closed meanwhile isn't.
  #[test]
  fn a_busy_window_gets_the_change_later() {
    two_windows();
    BUSY.set(2);
    deliver(&mut Main::here(), Change::Lid, send_busy, post_later);
    assert_eq!(SEEN.take(), ["1 System(Lid)", "2 System(Lid)"]);
    assert_eq!(run_posted(), [system::RETRY]);
    assert_eq!(SEEN.take(), ["2 System(Lid)"], "still busy");
    assert_eq!(run_posted(), [system::RETRY]);
    assert_eq!(SEEN.take(), ["2 System(Lid)"], "taken");
    assert!(POSTED.with_borrow(Vec::is_empty));

    BUSY.set(1);
    deliver(&mut Main::here(), Change::Lid, send_busy, post_later);
    SEEN.take();
    closed(2);
    run_posted();
    assert!(SEEN.take().is_empty(), "closed: not tried again");
    HOOKS.take();
  }

  /// A hook that watches again with another keeps the other.
  #[test]
  fn a_hook_replaced_by_itself_stays_replaced() {
    Main::here()
      .watch(&[], move |main, _| _ = main.watch(&[], move |_, _| seen("second")))
      .unwrap();
    deliver(&mut Main::here(), Change::Lid, |_, _| Some(true), post_later);
    deliver(&mut Main::here(), Change::Lid, |_, _| Some(true), post_later);
    assert_eq!(SEEN.take(), ["second"]);
    HOOKS.take();
  }

  fn cmd(name: &str, token: Option<&str>) -> Command {
    Command { name: name.into(), token: token.map(Into::into) }
  }
}
