// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Accessibility access (ApplicationServices' `AXIsProcessTrustedWithOptions`) and Secure Input
//! (Carbon's `IsSecureEventInputEnabled`), declared by hand: the objc2 crates for them would add
//! crates for two calls; the microphone through `AVCaptureDevice`, sent by name.

use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2::{class, msg_send};
use objc2_core_foundation::{CFBoolean, CFDictionary, CFString};
use objc2_foundation::{NSBundle, NSString, ns_string};

use crate::permissions::Access;

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

/// AVFoundation, loaded at the first question about the microphone (not linked: most apps never
/// ask, and it would load at every launch); then its `AVMediaTypeAudio`, the constant "soun".
fn audio() -> Option<&'static NSString> {
  static LOADED: OnceLock<bool> = OnceLock::new();
  let path = ns_string!("/System/Library/Frameworks/AVFoundation.framework");
  // SAFETY: a system framework, whose initializers are Apple's.
  let load = || NSBundle::bundleWithPath(path).is_some_and(|b| unsafe { b.load() });
  let loaded = LOADED.get_or_init(load);
  loaded.then(|| ns_string!("soun"))
}

/// `AVCaptureDevice`'s status for audio; granted when AVFoundation can't be loaded (no prompt can
/// show then either).
pub fn microphone() -> Access {
  let Some(audio) = audio() else { return Access::Granted };
  // SAFETY: the class method takes a media type and returns an `AVAuthorizationStatus`.
  let status: isize =
    unsafe { msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: audio] };
  match status {
    0 => Access::NotDetermined,
    1 => Access::Restricted,
    2 => Access::Denied,
    _ => Access::Granted,
  }
}

/// `done` goes to the main thread from AVFoundation's.
pub fn request_microphone(done: Box<dyn FnOnce(bool) + Send>) {
  let Some(audio) = audio() else { return done(true) };
  let done = Mutex::new(Some(done));
  let block = RcBlock::new(move |granted: Bool| {
    if let Some(done) = done.lock().ok().and_then(|mut d| d.take()) {
      super::post(Duration::ZERO, Box::new(move || done(granted.as_bool())));
    }
  });
  // SAFETY: a media type and a block, which the method copies.
  let _: () = unsafe {
    msg_send![class!(AVCaptureDevice), requestAccessForMediaType: audio, completionHandler: &*block]
  };
}
