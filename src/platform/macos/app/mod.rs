// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What a menu-bar app needs on macOS besides its windows ([super::shell]): the status item and
//! its menu ([tray]), global hotkeys ([hotkey]: an event tap with Carbon's hotkeys as the
//! fallback), launch at login ([autostart]: SMAppService), work from other threads on the main
//! queue ([post]), and the shell's app-wide state: the Dock ([Presence]), reopen and quit hooks.

pub mod autostart;
pub mod hotkey;
pub mod tray;

use std::cell::RefCell;
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

use crate::shell::{Main, Presence};

type OnReopen = Box<dyn FnMut(&mut Main)>;
type OnQuit = Box<dyn FnOnce(&mut Main)>;

#[derive(Default)]
struct State {
  presence: Presence,
  reopen: Option<OnReopen>,
  on_quit: Option<OnQuit>,
}

thread_local! {
  static STATE: RefCell<State> = RefCell::default();
}

pub fn set_presence(p: Presence) {
  STATE.with_borrow_mut(|s| s.presence = p);
  sync_presence();
}

/// Regular while the app is or a window is shown, else accessory (no Dock icon, no ⌘Tab).
pub(super) fn sync_presence() {
  let regular = STATE.with_borrow(|s| s.presence == Presence::Regular) || super::shell::any_shown();
  let policy = match regular {
    true => NSApplicationActivationPolicy::Regular,
    false => NSApplicationActivationPolicy::Accessory,
  };

  let ns_app = NSApplication::sharedApplication(super::mtm());
  if ns_app.activationPolicy() != policy {
    ns_app.setActivationPolicy(policy);
  }
}

pub fn on_reopen(f: OnReopen) {
  STATE.with_borrow_mut(|s| s.reopen = Some(f));
}

/// The app was opened again: its reopen hook runs, kept for the next time.
pub(super) fn reopened() {
  let Some(mut f) = STATE.with_borrow_mut(|s| s.reopen.take()) else { return };
  f(&mut Main::here());
  STATE.with_borrow_mut(|s| _ = s.reopen.get_or_insert(f));
}

pub fn on_quit(f: OnQuit) {
  STATE.with_borrow_mut(|s| s.on_quit = Some(f));
}

/// The app ends: its quit hook runs, once.
pub(super) fn quitting() {
  if let Some(f) = STATE.with_borrow_mut(|s| s.on_quit.take()) {
    f(&mut Main::here());
  }
}

pub fn is_main() -> bool {
  MainThreadMarker::new().is_some()
}

/// Runs `f` on the main queue after `delay`: in every run-loop mode, menu tracking included.
pub fn post(delay: Duration, f: Box<dyn FnOnce() + Send>) {
  if delay.is_zero() {
    return DispatchQueue::main().exec_async(f);
  }

  let when = DispatchTime::try_from(delay).unwrap_or(DispatchTime::NOW);
  let _ = DispatchQueue::main().after(when, f);
}
