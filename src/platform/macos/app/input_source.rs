// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The keyboard input sources on macOS ([crate::input_source]): Text Input Sources (HIToolbox,
//! declared in [super::super::keyboard]), asked on the main thread, and their changes from the
//! distributed notifications TIS posts when the selected source or the enabled ones change
//! ([crate::system]).

use std::cell::RefCell;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{AnyThread, define_class, msg_send, sel};
use objc2_core_foundation::{CFArray, CFBoolean, CFDictionary, CFRetained, CFString, CFType, Type};
use objc2_foundation::{
  NSDistributedNotificationCenter, NSNotification, NSNotificationSuspensionBehavior, NSString,
};

use super::super::keyboard::{
  CFTypeRef, TISCopyCurrentKeyboardInputSource, TISCreateInputSourceList,
  TISGetInputSourceProperty, kTISCategoryKeyboardInputSource,
  kTISNotifyEnabledKeyboardInputSourcesChanged, kTISNotifySelectedKeyboardInputSourceChanged,
  kTISPropertyInputSourceCategory, kTISPropertyInputSourceID,
  kTISPropertyInputSourceIsSelectCapable, kTISPropertyInputSourceLanguages,
  kTISPropertyLocalizedName,
};
use crate::input_source::{InputSource, primary};
use crate::system::{Change, notice};

define_class!(
  // SAFETY: NSObject has no subclassing requirements, and `Observer` does not implement `Drop`.
  #[unsafe(super = NSObject)]
  #[name = "NimboInputSourceObserver"]
  struct Observer;

  impl Observer {
    /// The selected source or the enabled ones changed.
    #[unsafe(method(changed:))]
    fn changed(&self, _: &NSNotification) {
      notice(Change::InputSource);
    }
  }
);

thread_local! {
  /// The observer while watched (on the main thread, where the center calls it back).
  static OBSERVER: RefCell<Option<Retained<Observer>>> = const { RefCell::new(None) };
}

pub fn available() -> bool {
  true
}

pub fn current() -> Option<InputSource> {
  // SAFETY: a Copy function returns a retained source or null.
  let src: CFRetained<CFType> = unsafe { owned(TISCopyCurrentKeyboardInputSource())? };
  // SAFETY: the source lives while it is read.
  unsafe { source((&raw const *src).cast()) }
}

pub fn enabled() -> Vec<InputSource> {
  // SAFETY: the keys are CFString constants; the list is retained (Create rule), its sources
  // owned by it.
  unsafe {
    let key = &*kTISPropertyInputSourceCategory.cast::<CFString>();
    let keyboard = &*kTISCategoryKeyboardInputSource.cast::<CFString>();
    let filter = CFDictionary::<CFString, CFString>::from_slices(&[key], &[keyboard]);
    let list = TISCreateInputSourceList((&raw const *filter).cast(), 0);
    let Some(list) = owned::<CFArray>(list) else { return Vec::new() };
    (0..list.count())
      .map(|i| list.value_at_index(i))
      .filter(|&src| flag(src, kTISPropertyInputSourceIsSelectCapable))
      .filter_map(|src| source(src))
      .collect()
  }
}

/// Starts telling [Change::InputSource]; delivered at once, as a menu-bar app is always in the
/// background, where the center holds back notifications by default.
pub fn watch() -> Result<(), String> {
  if OBSERVER.with_borrow(Option::is_some) {
    return Ok(());
  }

  // SAFETY: NSObject's `init`.
  let observer: Retained<Observer> = unsafe { msg_send![Observer::alloc(), init] };
  let center = NSDistributedNotificationCenter::defaultCenter();
  let now = NSNotificationSuspensionBehavior::DeliverImmediately;
  for name in names() {
    // SAFETY: the observer has `changed:`, taking a notification; `unwatch` removes it.
    unsafe {
      center.addObserver_selector_name_object_suspensionBehavior(
        &observer,
        sel!(changed:),
        Some(name),
        None,
        now,
      );
    }
  }

  OBSERVER.set(Some(observer));
  Ok(())
}

pub fn unwatch() {
  let Some(observer) = OBSERVER.take() else { return };
  // SAFETY: the observer `watch` added, for every name.
  unsafe { NSDistributedNotificationCenter::defaultCenter().removeObserver(&observer) };
}

fn names() -> [&'static NSString; 2] {
  // SAFETY: two CFString constants, toll-free bridged to NSString.
  unsafe {
    [kTISNotifySelectedKeyboardInputSourceChanged, kTISNotifyEnabledKeyboardInputSourcesChanged]
      .map(|n| &*n.cast::<NSString>())
  }
}

/// A Copy or Create function's result, released when dropped; `None` for null.
unsafe fn owned<T: Type>(cf: CFTypeRef) -> Option<CFRetained<T>> {
  // SAFETY: the caller passes a retained `T` or null.
  NonNull::new(cf.cast_mut().cast::<T>()).map(|p| unsafe { CFRetained::from_raw(p) })
}

/// What `src`, a live `TISInputSourceRef`, is; `None` without an id.
unsafe fn source(src: CFTypeRef) -> Option<InputSource> {
  // SAFETY: the properties are owned by the source (Get rule): an id and a name as CFStrings, the
  // languages as a CFArray of them.
  unsafe {
    let id = string(TISGetInputSourceProperty(src, kTISPropertyInputSourceID))?;
    let name = string(TISGetInputSourceProperty(src, kTISPropertyLocalizedName));
    let langs = TISGetInputSourceProperty(src, kTISPropertyInputSourceLanguages);
    let langs = langs.cast::<CFArray>().as_ref().filter(|l| l.count() > 0);
    let first = langs.and_then(|l| string(l.value_at_index(0)));
    let language = first.as_deref().and_then(primary);
    Some(InputSource { name: name.unwrap_or_else(|| id.clone()), id, language })
  }
}

/// A CFString's text; `None` for null.
unsafe fn string(cf: CFTypeRef) -> Option<String> {
  // SAFETY: the caller passes a CFString or null.
  unsafe { cf.cast::<CFString>().as_ref() }.map(|s| s.to_string())
}

/// A CFBoolean property of `src`; false when it has none.
unsafe fn flag(src: CFTypeRef, key: CFTypeRef) -> bool {
  // SAFETY: the property is a CFBoolean owned by the source, or null.
  unsafe { TISGetInputSourceProperty(src, key).cast::<CFBoolean>().as_ref() }
    .is_some_and(|b| b.as_bool())
}
