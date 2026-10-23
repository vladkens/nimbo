// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What a menu-bar app needs on macOS besides its windows ([super::shell]): the status item and
//! its menu ([tray]), global hotkeys ([hotkey]: an event tap with Carbon's hotkeys as the
//! fallback), launch at login ([autostart]: SMAppService), work from other threads on the main
//! queue ([post]), and the Dock ([Presence]).

pub mod autostart;
pub mod hotkey;
pub mod tray;

use std::cell::Cell;
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

use crate::shell::Presence;

thread_local! {
  static PRESENCE: Cell<Presence> = const { Cell::new(Presence::Regular) };
}

pub fn set_presence(p: Presence) {
  PRESENCE.set(p);
  sync_presence();
}

/// Regular while the app is or a window is shown, else accessory (no Dock icon, no ⌘Tab).
pub(super) fn sync_presence() {
  let regular = PRESENCE.get() == Presence::Regular || super::shell::any_shown();
  let policy = match regular {
    true => NSApplicationActivationPolicy::Regular,
    false => NSApplicationActivationPolicy::Accessory,
  };

  let ns_app = NSApplication::sharedApplication(super::mtm());
  if ns_app.activationPolicy() != policy {
    ns_app.setActivationPolicy(policy);
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
