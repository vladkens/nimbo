// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Where the user types on macOS ([crate::focus]): the app with the keyboard focus through the
//! Accessibility API (ApplicationServices' `AXUIElement*`, declared by hand as in
//! [super::permissions]), else `NSWorkspace`'s frontmost app; its focused window and element.

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::{class, msg_send};
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_core_foundation::{CFGetTypeID, CFRetained, CFString, CFType};

use crate::focus::{self, Focused};

/// How long one request may wait for the app: a hung app does not hold the caller longer.
const TIMEOUT: f32 = 0.25;
const SUCCESS: i32 = 0;
/// `kAXErrorCannotComplete` (also a timeout), `kAXErrorAttributeUnsupported`,
/// `kAXErrorAPIDisabled` (no Accessibility access), `kAXErrorNoValue`.
const CANNOT_COMPLETE: i32 = -25204;
const API_DISABLED: i32 = -25211;
const UNSUPPORTED: i32 = -25205;
const NO_VALUE: i32 = -25212;

type Element = *const c_void;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
  fn AXUIElementCreateSystemWide() -> Element;
  fn AXUIElementCreateApplication(pid: i32) -> Element;
  fn AXUIElementCopyAttributeValue(e: Element, attr: &CFString, value: *mut Element) -> i32;
  fn AXUIElementSetMessagingTimeout(e: Element, seconds: f32) -> i32;
  fn AXUIElementGetPid(e: Element, pid: *mut i32) -> i32;
  fn AXUIElementGetTypeID() -> usize;
}

/// An accessibility element, compared with `CFEqual`.
#[derive(Clone, Debug, PartialEq)]
pub struct Window(CFRetained<CFType>);

// SAFETY: CF objects are retained and released atomically, and an AXUIElement is an immutable
// reference to another app's element, usable from any thread.
unsafe impl Send for Window {}
unsafe impl Sync for Window {}

/// The focus is never hidden here.
pub fn hidden() -> bool {
  false
}

pub fn current() -> Option<Focused> {
  let pid = focused_app().or_else(|| {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    // SAFETY: a `pid_t` (the typed method needs objc2's `libc` feature, a crate more).
    Some(unsafe { msg_send![&app, processIdentifier] })
  })?;
  // SAFETY: a pid of any value: a dead one finds no app.
  let running: Option<Retained<NSRunningApplication>> = unsafe {
    msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: pid]
  };
  let name = running.and_then(|a| a.localizedName());
  // SAFETY: any pid is accepted; the element is returned retained.
  let app = owned(unsafe { AXUIElementCreateApplication(pid) })?;
  let window = attribute(&app, "AXFocusedWindow");
  // An app that just timed out isn't asked again, nor anything without access.
  let secure = match window {
    Err(e @ (CANNOT_COMPLETE | API_DISABLED)) => Err(e),
    _ => secure_field(&app),
  };

  Some(Focused {
    pid: pid as u32,
    name: name.map_or_else(String::new, |n| n.to_string()),
    window: window.map(|w| w.filter(is_element).map(|w| focus::Window(Window(w)))).map_err(why),
    secure: secure.map_err(why),
  })
}

/// The pid of the app with the keyboard focus (`AXFocusedApplication`), asked live: `NSWorkspace`
/// learns of a switch only when the main thread's run loop turns.
fn focused_app() -> Option<i32> {
  // Not timed: the system-wide element's timeout is the whole process's (the window server
  // answers this one).
  // SAFETY: no arguments; the element is returned retained.
  let system = retained(unsafe { AXUIElementCreateSystemWide() })?;
  let app = attribute(&system, "AXFocusedApplication").ok().flatten().filter(is_element)?;
  let mut pid = 0;
  // SAFETY: an AXUIElement, `pid` receives its pid.
  let found = unsafe { AXUIElementGetPid(ptr(&app), &mut pid) } == SUCCESS;
  found.then_some(pid)
}

/// A retained element from a "Create" call, [timed].
fn owned(e: Element) -> Option<CFRetained<CFType>> {
  let e = retained(e)?;
  timed(&e);
  Some(e)
}

fn retained(e: Element) -> Option<CFRetained<CFType>> {
  // SAFETY: "Create" returns an owned reference or null.
  Some(unsafe { CFRetained::from_raw(NonNull::new(e.cast_mut())?.cast()) })
}

/// The element's requests time out at [TIMEOUT] (each element has its own).
fn timed(e: &CFRetained<CFType>) {
  // SAFETY: a valid element.
  unsafe { AXUIElementSetMessagingTimeout(ptr(e), TIMEOUT) };
}

fn ptr(cf: &CFRetained<CFType>) -> Element {
  CFRetained::as_ptr(cf).as_ptr().cast_const().cast()
}

fn is_element(cf: &CFRetained<CFType>) -> bool {
  // SAFETY: no arguments.
  CFGetTypeID(Some(cf)) == unsafe { AXUIElementGetTypeID() }
}

/// An attribute's value; `None` when the element has none.
fn attribute(e: &CFRetained<CFType>, name: &str) -> Result<Option<CFRetained<CFType>>, i32> {
  let mut value = std::ptr::null();
  // SAFETY: a valid element; `value` receives a +1 reference or stays null.
  let err = unsafe { AXUIElementCopyAttributeValue(ptr(e), &CFString::from_str(name), &mut value) };
  match err {
    // SAFETY: "Copy" returns an owned reference.
    SUCCESS => {
      Ok(NonNull::new(value.cast_mut()).map(|v| unsafe { CFRetained::from_raw(v.cast()) }))
    }
    NO_VALUE => Ok(None),
    e => Err(e),
  }
}

/// The app's focused element is a password field (`AXSecureTextField`).
fn secure_field(app: &CFRetained<CFType>) -> Result<bool, i32> {
  let focused = match attribute(app, "AXFocusedUIElement") {
    Err(UNSUPPORTED) => None,
    focused => focused?,
  };

  let Some(focused) = focused.filter(is_element) else { return Ok(false) };

  timed(&focused);
  let subrole = match attribute(&focused, "AXSubrole") {
    Ok(subrole) => subrole,
    Err(UNSUPPORTED) => None,
    Err(e) => return Err(e),
  };

  let subrole = subrole.and_then(|s| s.downcast::<CFString>().ok());
  Ok(subrole.is_some_and(|s| s.to_string() == "AXSecureTextField"))
}

fn why(e: i32) -> String {
  format!("AXError {e}")
}
