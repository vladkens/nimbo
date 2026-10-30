// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Hardware on macOS ([crate::hardware]): an audio device's transport by its UID (CoreAudio's
//! properties) and the lid (IOPMrootDomain's `AppleClamshellState`, absent on a desktop), and
//! their changes ([crate::system]): CoreAudio's property listeners on the system object and
//! IOPMrootDomain's clamshell message. Declared by hand: the objc2 crates for them would add
//! crates for a few calls, and AppKit loads both frameworks anyway. Constants from MacOSX.sdk 27.0
//! (`AudioHardware.h`, `IOKitLib.h`, `pwr_mgt/IOPM.h`), read 2026-10-30.

use std::cell::Cell;
use std::ffi::{c_char, c_void};
use std::ptr::{NonNull, null, null_mut};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use dispatch2::{DispatchQueue, DispatchRetained};
use objc2_core_foundation::{CFBoolean, CFRetained, CFString, CFType};

use crate::hardware::Transport;
use crate::system::{Change, notice, refused};

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
  fn AudioObjectAddPropertyListener(
    id: u32,
    at: *const Address,
    f: Listener,
    data: *mut c_void,
  ) -> i32;
  fn AudioObjectRemovePropertyListener(
    id: u32,
    at: *const Address,
    f: Listener,
    data: *mut c_void,
  ) -> i32;
}

/// `AudioObjectPropertyListenerProc`.
type Listener = unsafe extern "C" fn(u32, u32, *const Address, *mut c_void) -> i32;
/// `IOServiceInterestCallback`.
type Interest = unsafe extern "C" fn(*mut c_void, u32, u32, *mut c_void);

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
  fn IONotificationPortCreate(main_port: u32) -> *mut c_void;
  fn IONotificationPortDestroy(port: *mut c_void);
  fn IONotificationPortSetDispatchQueue(port: *mut c_void, queue: &DispatchQueue);
  fn IOServiceAddInterestNotification(
    port: *mut c_void,
    service: u32,
    interest: *const c_char,
    f: Interest,
    refcon: *mut c_void,
    out: *mut u32,
  ) -> i32;
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
  let root = root_domain()?;

  let key = CFString::from_str("AppleClamshellState");
  // SAFETY: a registry entry and a key; the value comes owned or null, the entry released once.
  let value = unsafe { IORegistryEntryCreateCFProperty(root, &key, null(), 0) };
  unsafe { IOObjectRelease(root) };
  // SAFETY: an owned CF object.
  let value = unsafe { CFRetained::from_raw(NonNull::new(value)?) };
  Some(value.downcast_ref::<CFBoolean>()?.as_bool())
}

/// IOPMrootDomain, to release; `None` where there is none.
fn root_domain() -> Option<u32> {
  // SAFETY: the matching dictionary is consumed by the lookup; 0 is the default main port.
  let root =
    unsafe { IOServiceGetMatchingService(0, IOServiceMatching(c"IOPMrootDomain".as_ptr())) };
  (root != 0).then_some(root)
}

// MARK: Changes

/// `kIOPMMessageClamshellStateChange`: `iokit_family_msg(sub_iokit_powermanagement, 0x100)`,
/// `sys_iokit` being `err_system(0x38)` and the family `err_sub(13)`.
const CLAMSHELL: u32 = 0xe003_4100;

/// The audio kinds whose listeners are on, a bit each (`address`'s index), on [audio]'s queue.
static AUDIO: AtomicU8 = AtomicU8::new(0);

thread_local! {
  /// The lid's port and notification while it is watched.
  static LID: Cell<Option<(*mut c_void, u32)>> = const { Cell::new(None) };
}

/// CoreAudio's listeners are added and removed here, in order, off the main thread: the first
/// call connects to coreaudiod, 35–75 ms in the macOS 27 VM (2026-10-30), which the app's first
/// window shouldn't wait for.
fn audio() -> &'static DispatchQueue {
  QUEUE.get_or_init(|| DispatchQueue::new("nimbo.audio", None))
}

static QUEUE: OnceLock<DispatchRetained<DispatchQueue>> = OnceLock::new();

/// The system object's property that tells `kind`: its device list or its default input.
fn address(kind: Change) -> Option<(usize, Address)> {
  let (i, selector) = match kind {
    Change::AudioInputs => (0, b"dev#"),
    Change::DefaultInput => (1, b"dIn "),
    _ => return None,
  };

  Some((i, Address { selector: code(selector), scope: code(b"glob"), element: 0 }))
}

/// The kind a changed property of the system object tells.
fn told(selector: u32) -> Option<Change> {
  [Change::AudioInputs, Change::DefaultInput]
    .into_iter()
    .find(|&k| address(k).is_some_and(|(_, a)| a.selector == selector))
}

/// Watches `kind` (on the main thread): a listener on the system object, added on [audio]'s
/// queue, then one change told, as what the app read before it listened may be old (a rare
/// failure can't come back as `Err`: [crate::system::refused]); or the clamshell.
pub fn watch(kind: Change) -> Result<(), String> {
  if kind == Change::Lid {
    return watch_lid();
  }

  let Some((i, at)) = address(kind) else { return Err("not on macOS".into()) };
  audio().exec_async(move || {
    // SAFETY: the system object, a property of it, a listener that lives as long as the process.
    match unsafe { AudioObjectAddPropertyListener(1, &at, listener, null_mut()) } {
      0 => {
        AUDIO.fetch_or(1 << i, Ordering::AcqRel);
        notice(kind);
      }
      e => refused(kind, &format!("CoreAudio's listener failed ({e})")),
    }
  });
  Ok(())
}

/// The clamshell's message from IOPMrootDomain, on the main queue.
fn watch_lid() -> Result<(), String> {
  let root = root_domain().ok_or("no IOPMrootDomain")?;
  // SAFETY: a port on the default main port, its callbacks on the main queue; the root domain
  // released once the notification holds it.
  let port = unsafe { IONotificationPortCreate(0) };
  unsafe { IONotificationPortSetDispatchQueue(port, DispatchQueue::main()) };
  let mut note = 0;
  let interest = c"IOGeneralInterest".as_ptr();
  let status = unsafe {
    IOServiceAddInterestNotification(port, root, interest, clamshell, null_mut(), &mut note)
  };
  unsafe { IOObjectRelease(root) };
  if status != 0 {
    unsafe { IONotificationPortDestroy(port) };
    return Err(format!("IOKit's notification failed ({status:#x})"));
  }

  LID.set(Some((port, note)));
  Ok(())
}

/// Stops every listener and the clamshell's notification.
pub fn unwatch() {
  // Nothing was added without the queue: none made for the loop's end.
  let Some(audio) = QUEUE.get() else { return unwatch_lid() };
  audio.exec_async(|| {
    let on = AUDIO.swap(0, Ordering::AcqRel);
    for (i, at) in [Change::AudioInputs, Change::DefaultInput].into_iter().filter_map(address) {
      if on & 1 << i != 0 {
        // SAFETY: the listener added by `watch`, with the same address and data.
        unsafe { AudioObjectRemovePropertyListener(1, &at, listener, null_mut()) };
      }
    }
  });
  unwatch_lid();
}

fn unwatch_lid() {
  if let Some((port, note)) = LID.take() {
    // SAFETY: the notification and its port, made by `watch_lid`, released once.
    unsafe { IOObjectRelease(note) };
    unsafe { IONotificationPortDestroy(port) };
  }
}

/// CoreAudio told a change of the system object's properties: each may name others too.
unsafe extern "C" fn listener(_: u32, n: u32, at: *const Address, _: *mut c_void) -> i32 {
  // SAFETY: CoreAudio passes `n` addresses.
  let at = unsafe { std::slice::from_raw_parts(at, n as usize) };
  for kind in at.iter().filter_map(|a| told(a.selector)) {
    notice(kind);
  }

  0
}

/// IOPMrootDomain's general interest: the clamshell opened or closed (its argument's bits, the
/// state and whether it sleeps, read again by [lid_closed]).
unsafe extern "C" fn clamshell(_: *mut c_void, _: u32, message: u32, _: *mut c_void) {
  if message == CLAMSHELL {
    notice(Change::Lid);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn changes_told() {
    assert_eq!(told(code(b"dev#")), Some(Change::AudioInputs));
    assert_eq!(told(code(b"dIn ")), Some(Change::DefaultInput));
    assert_eq!(told(code(b"dOut")), None);
    assert_eq!(address(Change::Lid).map(|a| a.0), None);
    assert_eq!(CLAMSHELL, (0x38 << 26) | (13 << 14) | 0x100);
  }
}
