// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The keyboard layouts on Windows ([crate::input_source]): the foreground window's thread's
//! (`GetKeyboardLayout`, from any process: the one the user types with, by default the same for
//! every window), those the user can switch to (`GetKeyboardLayoutList`), each named by its
//! language's locale, as the language bar lists them (two layouts of a language share the name).
//! Windows tells no other app of a switch, so a thread of its own looks every [POLL] while they are
//! watched ([crate::system]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Globalization::{
  GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SLOCALIZEDDISPLAYNAME,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
  GetKeyboardLayout, GetKeyboardLayoutList, HKL,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::super::wide;
use crate::input_source::{InputSource, primary};
use crate::system::{Change, notice};

/// How often a watch looks: a switch reaches the app within this and the system's settle.
const POLL: Duration = Duration::from_millis(200);

static WATCHED: AtomicBool = AtomicBool::new(false);

pub fn available() -> bool {
  true
}

pub fn current() -> Option<InputSource> {
  Some(source(layout()))
}

pub fn enabled() -> Vec<InputSource> {
  let mut out: Vec<InputSource> = Vec::new();
  for hkl in layouts() {
    let s = source(hkl);
    if !out.iter().any(|o| o.id == s.id) {
      out.push(s);
    }
  }

  out
}

/// Starts the poll (on the main thread): a change of the layout in use or of the list.
pub fn watch() -> Result<(), String> {
  if WATCHED.swap(true, Ordering::AcqRel) {
    return Ok(());
  }

  let spawn = thread::Builder::new().name("nimbo-input-source".into()).spawn(|| {
    let mut last = (layout(), layouts());
    while WATCHED.load(Ordering::Acquire) {
      thread::sleep(POLL);
      let now = (layout(), layouts());
      if now != last {
        last = now;
        notice(Change::InputSource);
      }
    }
  });
  if let Err(e) = spawn {
    WATCHED.store(false, Ordering::Release);
    return Err(e.to_string());
  }

  Ok(())
}

pub fn unwatch() {
  WATCHED.store(false, Ordering::Release);
}

/// The foreground window's thread's layout, else this thread's.
fn layout() -> HKL {
  // SAFETY: plain calls; a null window gives thread 0, this one.
  unsafe {
    let window = GetForegroundWindow();
    let thread = if window.is_null() { 0 } else { GetWindowThreadProcessId(window, &mut 0) };
    GetKeyboardLayout(thread)
  }
}

fn layouts() -> Vec<HKL> {
  // SAFETY: the count first, then a buffer as long.
  unsafe {
    let n = GetKeyboardLayoutList(0, std::ptr::null_mut());
    let mut list = vec![std::ptr::null_mut(); n.max(0) as usize];
    let got = GetKeyboardLayoutList(n, list.as_mut_ptr());
    list.truncate(got.max(0) as usize);
    list
  }
}

/// A layout by its language (the HKL's low word, a LANGID): `ru-RU`, named `Russian (Russia)`.
fn source(hkl: HKL) -> InputSource {
  let lang = (hkl as usize & 0xffff) as u32;
  let mut tag = [0u16; 85];
  // SAFETY: the buffer and its length.
  let n = unsafe { LCIDToLocaleName(lang, tag.as_mut_ptr(), tag.len() as i32, 0) };
  let tag = String::from_utf16_lossy(&tag[..(n.max(1) - 1) as usize]);

  let mut name = [0u16; 128];
  let len = name.len() as i32;
  // SAFETY: the tag ends in a nul; the buffer and its length.
  let m = unsafe {
    GetLocaleInfoEx(wide(&tag).as_ptr(), LOCALE_SLOCALIZEDDISPLAYNAME, name.as_mut_ptr(), len)
  };
  let name = String::from_utf16_lossy(&name[..(m.max(1) - 1) as usize]);

  let id = format!("{:08x}", hkl as usize as u32);
  InputSource { name: if name.is_empty() { id.clone() } else { name }, id, language: primary(&tag) }
}
