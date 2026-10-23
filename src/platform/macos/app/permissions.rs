// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Accessibility access (ApplicationServices' `AXIsProcessTrustedWithOptions`) and Secure Input
//! (Carbon's `IsSecureEventInputEnabled`), declared by hand: the objc2 crates for them would add
//! crates for two calls.

use std::ffi::c_void;

use objc2_core_foundation::{CFBoolean, CFDictionary, CFString};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
  static kAXTrustedCheckOptionPrompt: &'static CFString;
  fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
  fn IsSecureEventInputEnabled() -> u8;
}

/// Whether the process is trusted for Accessibility; `prompt` shows the system's dialog if not.
pub fn accessibility(prompt: bool) -> bool {
  // SAFETY: a static of the framework.
  let key: &CFString = unsafe { kAXTrustedCheckOptionPrompt };
  let options = CFDictionary::from_slices(&[key], &[CFBoolean::new(prompt)]);
  // SAFETY: a dictionary of the documented key to a CFBoolean, alive across the call.
  unsafe { AXIsProcessTrustedWithOptions((&raw const *options).cast()) != 0 }
}

pub fn secure_input() -> bool {
  // SAFETY: no arguments, no preconditions.
  unsafe { IsSecureEventInputEnabled() != 0 }
}
