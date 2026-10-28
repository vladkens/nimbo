// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Hardware on macOS ([crate::hardware]): an audio device's transport by its UID (CoreAudio's
//! properties) and the lid (IOPMrootDomain's `AppleClamshellState`, absent on a desktop), declared
//! by hand: the objc2 crates for them would add crates for four calls, and AppKit loads both
//! frameworks anyway.

use std::ffi::{c_char, c_void};
use std::ptr::{NonNull, null};

use objc2_core_foundation::{CFBoolean, CFRetained, CFString, CFType};

use crate::hardware::Transport;

/// `AudioObjectPropertyAddress`.
#[repr(C)]
struct Address {
  selector: u32,
  scope: u32,
  element: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
  fn AudioObjectGetPropertyData(
    id: u32,
    address: *const Address,
    qualifier_size: u32,
    qualifier: *const c_void,
    size: *mut u32,
    data: *mut c_void,
  ) -> i32;
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
  fn IOServiceMatching(name: *const c_char) -> *mut c_void;
  fn IOServiceGetMatchingService(main_port: u32, matching: *mut c_void) -> u32;
  fn IORegistryEntryCreateCFProperty(
    entry: u32,
    key: &CFString,
    alloc: *const c_void,
    options: u32,
  ) -> *mut CFType;
  fn IOObjectRelease(object: u32) -> i32;
}

/// A four-character code as CoreAudio numbers it.
const fn code(s: &[u8; 4]) -> u32 {
  u32::from_be_bytes(*s)
}

/// A UInt32 property of an audio object (element main), with a CFString qualifier or none.
fn property(object: u32, selector: &[u8; 4], scope: &[u8; 4], q: Option<&CFString>) -> Option<u32> {
  let address = Address { selector: code(selector), scope: code(scope), element: 0 };
  let (mut value, mut size) = (0u32, size_of::<u32>() as u32);
  let q: *const CFString = q.map_or(null(), |q| q);
  let qualifier = match q.is_null() {
    true => (0, null()),
    false => (size_of::<*const CFString>() as u32, (&raw const q).cast()),
  };
  // SAFETY: the qualifier is a CFStringRef of its size or none; the data a UInt32 of `size` bytes.
  let status = unsafe {
    let out = (&raw mut value).cast();
    AudioObjectGetPropertyData(object, &address, qualifier.0, qualifier.1, &mut size, out)
  };
  (status == 0).then_some(value)
}

/// The device by `kAudioHardwarePropertyTranslateUIDToDevice` of the system object, then its
/// `kAudioDevicePropertyTransportType`; the built-in one is the computer's own microphone when it
/// is Apple silicon's device or an Intel Mac's input on its internal source (`'imic'`).
pub fn transport(uid: &str) -> Transport {
  let device = property(1, b"uidd", b"glob", Some(&CFString::from_str(uid))).filter(|&d| d != 0);
  let Some(kind) = device.and_then(|d| property(d, b"tran", b"glob", None)) else {
    return Transport::Unknown;
  };

  let internal = kind == code(b"bltn")
    && (uid == "BuiltInMicrophoneDevice"
      || device.and_then(|d| property(d, b"ssrc", b"inpt", None)) == Some(code(b"imic")));
  crate::hardware::core_audio(kind.to_be_bytes(), internal)
}

pub fn lid_closed() -> Option<bool> {
  // SAFETY: the matching dictionary is consumed by the lookup; 0 is the default main port.
  let root =
    unsafe { IOServiceGetMatchingService(0, IOServiceMatching(c"IOPMrootDomain".as_ptr())) };
  if root == 0 {
    return None;
  }

  let key = CFString::from_str("AppleClamshellState");
  // SAFETY: a registry entry and a key; the value comes owned or null, the entry released once.
  let value = unsafe { IORegistryEntryCreateCFProperty(root, &key, null(), 0) };
  unsafe { IOObjectRelease(root) };
  // SAFETY: an owned CF object.
  let value = unsafe { CFRetained::from_raw(NonNull::new(value)?) };
  Some(value.downcast_ref::<CFBoolean>()?.as_bool())
}
