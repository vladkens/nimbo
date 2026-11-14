// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The keyboard input sources on macOS ([crate::input_source]): Text Input Sources (HIToolbox),
//! asked on the main thread, and their changes from the distributed notifications TIS posts when
//! the selected source or the enabled ones change ([crate::system]). Declared by hand: the objc2
//! crates don't cover HIToolbox. Names from MacOSX.sdk 27.0 (`TextInputSources.h`,
//! `CFNotificationCenter.h`), read 2026-11-14.

use std::ffi::c_void;
use std::ptr::null;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2_core_foundation::{CFDictionary, CFRetained, CFString};

use crate::input_source::{InputSource, primary};
use crate::system::{Change, notice};

type CFTypeRef = *const c_void;
type Callback = unsafe extern "C" fn(CFTypeRef, *mut c_void, CFTypeRef, *const c_void, CFTypeRef);

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
  static kTISPropertyInputSourceID: CFTypeRef;
  static kTISPropertyLocalizedName: CFTypeRef;
  static kTISPropertyInputSourceLanguages: CFTypeRef;
  static kTISPropertyInputSourceCategory: CFTypeRef;
  static kTISPropertyInputSourceIsSelectCapable: CFTypeRef;
  static kTISCategoryKeyboardInputSource: CFTypeRef;
  static kTISNotifySelectedKeyboardInputSourceChanged: CFTypeRef;
  static kTISNotifyEnabledKeyboardInputSourcesChanged: CFTypeRef;
  fn TISCopyCurrentKeyboardInputSource() -> CFTypeRef;
  fn TISCreateInputSourceList(filter: CFTypeRef, all_installed: u8) -> CFTypeRef;
  fn TISGetInputSourceProperty(src: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
  fn CFArrayGetCount(a: CFTypeRef) -> isize;
  fn CFArrayGetValueAtIndex(a: CFTypeRef, i: isize) -> CFTypeRef;
  fn CFBooleanGetValue(b: CFTypeRef) -> u8;
  fn CFRelease(cf: CFTypeRef);
  fn CFNotificationCenterGetDistributedCenter() -> CFTypeRef;
  fn CFNotificationCenterAddObserver(
    center: CFTypeRef,
    observer: *const c_void,
    callback: Callback,
    name: CFTypeRef,
    object: CFTypeRef,
    behavior: isize,
  );
  fn CFNotificationCenterRemoveObserver(
    center: CFTypeRef,
    observer: *const c_void,
    name: CFTypeRef,
    object: CFTypeRef,
  );
}

/// `CFNotificationSuspensionBehaviorDeliverImmediately`: told while the app is in the background
/// too, where a menu-bar app always is.
const DELIVER_IMMEDIATELY: isize = 4;

/// Watched, and the observer's identity for the center.
static WATCHED: AtomicBool = AtomicBool::new(false);

pub fn available() -> bool {
  true
}

pub fn current() -> Option<InputSource> {
  // SAFETY: a Copy function returns a retained source or null, released once it is read.
  unsafe {
    let src = TISCopyCurrentKeyboardInputSource();
    if src.is_null() {
      return None;
    }

    let source = source(src);
    CFRelease(src);
    source
  }
}

pub fn enabled() -> Vec<InputSource> {
  // SAFETY: the keys are CFString constants; the list is retained (Create rule) and released
  // once its sources, owned by it, are read.
  unsafe {
    let key = &*kTISPropertyInputSourceCategory.cast::<CFString>();
    let keyboard = &*kTISCategoryKeyboardInputSource.cast::<CFString>();
    let filter: CFRetained<CFDictionary<CFString, CFString>> =
      CFDictionary::from_slices(&[key], &[keyboard]);
    let list = TISCreateInputSourceList((&raw const *filter).cast(), 0);
    if list.is_null() {
      return Vec::new();
    }

    let sources = (0..CFArrayGetCount(list))
      .map(|i| CFArrayGetValueAtIndex(list, i))
      .filter(|&src| flag(src, kTISPropertyInputSourceIsSelectCapable))
      .filter_map(|src| source(src))
      .collect();
    CFRelease(list);
    sources
  }
}

/// Starts telling [Change::InputSource] (on the main thread, as the center calls back there).
pub fn watch() -> Result<(), String> {
  if WATCHED.swap(true, Ordering::AcqRel) {
    return Ok(());
  }

  // SAFETY: the distributed center lives as long as the process; the names are CFString
  // constants and the callback a plain function.
  unsafe {
    let center = CFNotificationCenterGetDistributedCenter();
    for name in names() {
      CFNotificationCenterAddObserver(
        center,
        observer(),
        changed,
        name,
        null(),
        DELIVER_IMMEDIATELY,
      );
    }
  }

  Ok(())
}

pub fn unwatch() {
  if !WATCHED.swap(false, Ordering::AcqRel) {
    return;
  }

  // SAFETY: the observer added by `watch`, with the same names.
  unsafe {
    let center = CFNotificationCenterGetDistributedCenter();
    for name in names() {
      CFNotificationCenterRemoveObserver(center, observer(), name, null());
    }
  }
}

fn names() -> [CFTypeRef; 2] {
  // SAFETY: reading two CFString constants.
  unsafe {
    [kTISNotifySelectedKeyboardInputSourceChanged, kTISNotifyEnabledKeyboardInputSourcesChanged]
  }
}

fn observer() -> *const c_void {
  (&raw const WATCHED).cast()
}

unsafe extern "C" fn changed(
  _: CFTypeRef,
  _: *mut c_void,
  _: CFTypeRef,
  _: *const c_void,
  _: CFTypeRef,
) {
  notice(Change::InputSource);
}

/// What `src`, a live `TISInputSourceRef`, is; `None` without an id.
unsafe fn source(src: CFTypeRef) -> Option<InputSource> {
  // SAFETY: the properties are owned by the source (Get rule): an id and a name as CFStrings, the
  // languages as a CFArray of them.
  unsafe {
    let id = string(TISGetInputSourceProperty(src, kTISPropertyInputSourceID))?;
    let name = string(TISGetInputSourceProperty(src, kTISPropertyLocalizedName));
    let langs = TISGetInputSourceProperty(src, kTISPropertyInputSourceLanguages);
    let first = (!langs.is_null() && CFArrayGetCount(langs) > 0)
      .then(|| string(CFArrayGetValueAtIndex(langs, 0)))
      .flatten();
    let language = first.as_deref().and_then(primary);
    Some(InputSource { name: name.unwrap_or_else(|| id.clone()), id, language })
  }
}

/// A CFString's text; `None` for null.
unsafe fn string(cf: CFTypeRef) -> Option<String> {
  // SAFETY: the caller passes a CFString or null.
  (!cf.is_null()).then(|| unsafe { &*cf.cast::<CFString>() }.to_string())
}

/// A CFBoolean property of `src`; false when it has none.
unsafe fn flag(src: CFTypeRef, key: CFTypeRef) -> bool {
  // SAFETY: the property is a CFBoolean owned by the source, or null.
  unsafe {
    let value = TISGetInputSourceProperty(src, key);
    !value.is_null() && CFBooleanGetValue(value) != 0
  }
}
