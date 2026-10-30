// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What a menu-bar app needs on macOS besides its windows ([super::shell]): the status item and its
//! menu ([tray]), context menus ([menu]), global hotkeys ([hotkey]: an event tap with Carbon's
//! hotkeys as the fallback), launch at login ([autostart]: SMAppService), work from other threads
//! on the main queue ([post()]), and the Dock ([Presence]); links and files opened ([open]),
//! Accessibility, Secure Input and the microphone ([permissions]), shortcuts posted to the app in
//! focus ([post]), where the user types ([focus]), the pasteboard for a paste ([paste]), alerts
//! ([alert]), files fetched over HTTP ([http]), an update's disk image opened and checked
//! ([update]), what an uninstall resets ([uninstall]), and a microphone's transport and the lid
//! ([hardware]).

pub mod activity;
pub mod autostart;
pub mod desktop;
pub mod focus;
pub mod hardware;
pub mod hotkey;
pub mod http;
pub mod menu;
pub mod open;
pub mod paste;
pub mod permissions;
pub mod post;
pub mod tray;
pub mod uninstall;
pub mod update;

use std::cell::Cell;
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::NSString;

use crate::Event;
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

/// Sends `ev` to window `id`'s runtime; `None` when it is busy up the stack or gone.
pub fn send(id: u64, ev: Event) -> Option<bool> {
  super::shell::view(id).filter(|v| !v.busy()).map(|v| v.send(ev))
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

/// An `NSAlert`, the app activated so it shows in front.
pub fn alert(title: &str, message: &str) {
  let mtm = super::mtm();
  let alert = NSAlert::new(mtm);
  alert.setMessageText(&NSString::from_str(title));
  alert.setInformativeText(&NSString::from_str(message));
  NSApplication::sharedApplication(mtm).activate();
  alert.runModal();
}
