// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Launch at login through `SMAppService.mainApp` (macOS 13+; unavailable below), which registers
//! the app bundle the process runs from. Registering shows the system's "Login Item Added"
//! notification.

use objc2::rc::Retained;
use objc2_foundation::{NSBundle, NSError};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

use crate::AppInfo;
use crate::autostart::State;

/// The process runs from an app bundle with an identifier.
fn in_bundle() -> bool {
  let bundle = NSBundle::mainBundle();
  bundle.bundleIdentifier().is_some() && bundle.bundlePath().to_string().ends_with(".app")
}

pub fn available() -> Result<(), String> {
  if !objc2::available!(macos = 13.0) {
    return Err("launch at login needs macOS 13 or later".into());
  }

  match in_bundle() {
    true => Ok(()),
    false => Err("launch at login needs the app bundle; this runs as a bare binary".into()),
  }
}

fn service() -> Retained<SMAppService> {
  // SAFETY: a plain class method; the service is the main bundle's.
  unsafe { SMAppService::mainAppService() }
}

/// `NotFound` is what the main app's service reports until it was first registered (measured on
/// macOS 27): not registered, for the user.
fn state_of(status: SMAppServiceStatus) -> State {
  match status {
    SMAppServiceStatus::Enabled => State::Enabled,
    SMAppServiceStatus::RequiresApproval => State::RequiresApproval,
    _ => State::NotRegistered,
  }
}

fn describe(e: &NSError) -> String {
  format!("{} (code {})", e.localizedDescription(), e.code())
}

/// The bundle's state; `app` is the bundle itself here.
pub fn state(_app: &AppInfo) -> Result<State, String> {
  // SAFETY: a plain getter.
  Ok(state_of(unsafe { service().status() }))
}

pub fn set(app: &AppInfo, on: bool) -> Result<State, String> {
  let before = state(app)?;
  let service = service();
  // Registering twice or unregistering what is not registered is an error of the framework.
  if on && !before.wanted() {
    // SAFETY: a plain call on the main app's service.
    unsafe { service.registerAndReturnError() }
      .map_err(|e| format!("register: {}", describe(&e)))?;
  } else if !on && before.wanted() {
    // SAFETY: as above.
    unsafe { service.unregisterAndReturnError() }
      .map_err(|e| format!("unregister: {}", describe(&e)))?;
  }

  state(app)
}

/// Nothing below macOS 13, which has no `SMAppService`.
pub fn open_settings() {
  if objc2::available!(macos = 13.0) {
    // SAFETY: a plain class method.
    unsafe { SMAppService::openSystemSettingsLoginItems() };
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The test binary is no bundle.
  #[test]
  fn unavailable_unbundled() {
    let app = AppInfo { id: "com.example.app", name: "Example", presence: Default::default() };
    assert!(available().is_err());
    assert!(crate::autostart::state(&app).is_err() && crate::autostart::set(&app, true).is_err());
  }
}
