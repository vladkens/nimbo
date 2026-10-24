// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Where the user types on Windows ([crate::focus]): the foreground window, its process and the
//! focus of its thread (`GetGUIThreadInfo`), a password field when that is an `Edit` control
//! with `ES_PASSWORD`.

use std::mem::zeroed;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::Threading::{
  OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
  ES_PASSWORD, GUITHREADINFO, GWL_STYLE, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo,
  GetWindowLongW, GetWindowThreadProcessId,
};

use crate::focus::{self, Focused};

/// A top-level window's handle, as a number: it is compared, never used.
#[derive(Clone, Debug, PartialEq)]
pub struct Window(isize);

pub fn current() -> Option<Focused> {
  let hwnd = unsafe { GetForegroundWindow() };
  if hwnd.is_null() {
    return None;
  }

  let mut pid = 0;
  let thread = unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
  // SAFETY: a plain struct, all zeros but its size.
  let mut info: GUITHREADINFO = unsafe { zeroed() };
  info.cbSize = size_of::<GUITHREADINFO>() as u32;
  let found = unsafe { GetGUIThreadInfo(thread, &mut info) } != 0;
  Some(Focused {
    pid,
    name: name(pid),
    window: Ok(Some(focus::Window(Window(hwnd as isize)))),
    secure: Ok(found && password(info.hwndFocus)),
  })
}

/// `hwnd` is an `Edit` control that hides what is typed.
fn password(hwnd: HWND) -> bool {
  if hwnd.is_null() {
    return false;
  }

  let mut class = [0u16; 8];
  let n = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) }.max(0);
  let edit = String::from_utf16_lossy(&class[..n as usize]).eq_ignore_ascii_case("Edit");
  edit && unsafe { GetWindowLongW(hwnd, GWL_STYLE) } & ES_PASSWORD != 0
}

/// The process's executable without `.exe`; empty when it can't be read (an elevated process).
fn name(pid: u32) -> String {
  let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
  if process.is_null() {
    return String::new();
  }

  let mut buf = [0u16; 1024];
  let mut len = buf.len() as u32;
  let read =
    unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) };
  unsafe { CloseHandle(process) };
  if read == 0 {
    return String::new();
  }

  let path = String::from_utf16_lossy(&buf[..len as usize]);
  Path::new(&path).file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned())
}
