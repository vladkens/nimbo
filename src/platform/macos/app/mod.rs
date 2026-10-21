// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What a menu-bar app needs on macOS besides its windows ([super::shell]): the status item and
//! its menu ([tray]), global hotkeys ([hotkey]: an event tap with Carbon's hotkeys as the
//! fallback), launch at login ([autostart]: SMAppService), and work from other threads on the
//! main queue ([post]).

pub mod autostart;
pub mod hotkey;
pub mod tray;

use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::MainThreadMarker;

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
