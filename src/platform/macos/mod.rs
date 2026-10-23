// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The macOS platform: NSApplication and its windows ([shell]), each window's content our NSView
//! ([view]: input, cursors, display link and timers, occlusion, present through [surface]), the
//! keyboard layout ([keyboard]), overlay panels ([overlay]), the clipboard on NSPasteboard, the
//! CoreText text backend ([text]) and the performance cores; the tray, global hotkeys and launch at
//! login in [app]. macOS 14 or later: the app's activation and the view's display link need it.

mod view;

mod app;
mod keyboard;
mod overlay;
pub mod shell;
mod surface;
pub mod text;

use std::cell::Cell;

use block2::StackBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{
  NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSColor, NSColorSpace,
  NSPasteboard, NSPasteboardTypeString,
};
use objc2_foundation::{NSArray, NSString, NSUserDefaults, ns_string};

pub use self::app::{autostart, hotkey, tray};
use crate::app::{Appearance, Clipboard};

/// The system clipboard, plain text only.
pub struct Pasteboard;

impl Clipboard for Pasteboard {
  fn read(&self) -> Option<String> {
    let pb = NSPasteboard::generalPasteboard();
    pb.stringForType(unsafe { NSPasteboardTypeString }).map(|s| s.to_string())
  }

  fn write(&mut self, s: &str) {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    pb.setString_forType(&NSString::from_str(s), unsafe { NSPasteboardTypeString });
  }
}

/// The main thread's marker: AppKit, the app's windows and its tray are main thread only.
fn mtm() -> MainThreadMarker {
  MainThreadMarker::new().expect("nimbo: AppKit is main thread only")
}

/// The app's effective appearance (light or dark Aqua, as System Settings sets it unless the
/// app overrides it) and the accent colour, `None` for Multicolor: `AppleAccentColor` is then
/// unset and controls use the app's own. The accent is `controlAccentColor` resolved in that
/// appearance, in sRGB. On the main thread only; light elsewhere.
pub fn appearance() -> Appearance {
  let Some(mtm) = MainThreadMarker::new() else { return Appearance::LIGHT };
  let look = NSApplication::sharedApplication(mtm).effectiveAppearance();
  let (aqua, dark) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
  let best = look.bestMatchFromAppearancesWithNames(&NSArray::from_slice(&[aqua, dark]));
  let dark = best.is_some_and(|n| &*n == dark);
  let picked = NSUserDefaults::standardUserDefaults().objectForKey(ns_string!("AppleAccentColor"));
  let accent = Cell::new(None);
  if picked.is_some() {
    let resolve = StackBlock::new(|| {
      let c = NSColor::controlAccentColor().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace());
      let rgba =
        |c: &NSColor| [c.redComponent(), c.greenComponent(), c.blueComponent(), c.alphaComponent()];
      accent.set(c.map(|c| rgba(&c).map(|v| v as f32)));
    });
    look.performAsCurrentDrawingAppearance(&resolve);
  }

  Appearance { dark, accent: accent.get() }
}

/// The performance cores of an Apple Silicon Mac (`hw.perflevel0.physicalcpu`).
pub fn perf_cores() -> Option<usize> {
  use std::ffi::{c_char, c_int, c_void};

  // libSystem, <sys/sysctl.h>.
  unsafe extern "C" {
    fn sysctlbyname(
      name: *const c_char,
      old: *mut c_void,
      old_len: *mut usize,
      new: *mut c_void,
      new_len: usize,
    ) -> c_int;
  }

  let (mut n, mut len) = (0i32, size_of::<i32>());
  // SAFETY: a NUL-terminated name and an int-sized output buffer with its length.
  let rc = unsafe {
    sysctlbyname(
      c"hw.perflevel0.physicalcpu".as_ptr(),
      (&raw mut n).cast(),
      &mut len,
      std::ptr::null_mut(),
      0,
    )
  };
  (rc == 0 && n > 0).then_some(n as usize)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn perf_cores_are_found() {
    assert!(perf_cores().is_some_and(|n| n >= 1));
  }
}
