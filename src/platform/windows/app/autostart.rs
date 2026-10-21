// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Launch at login on Windows: a value named by the app's id under HKCU
//! `Software\Microsoft\Windows\CurrentVersion\Run`, the quoted path of the running executable.
//! Task Manager's Startup apps turns it off without removing it, in `...\Explorer\
//! StartupApproved\Run` (the first byte of the value odd), which reads as
//! [State::RequiresApproval]: only the user turns it on again there.

use std::ptr::{null, null_mut};

use windows_sys::Win32::System::Registry::{
  HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
  RegSetKeyValueW,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::super::wide;
use crate::AppInfo;
use crate::autostart::State;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

pub fn available() -> Result<(), String> {
  Ok(())
}

/// The Run value's data for `exe`: quoted, as a path may have spaces.
fn command(exe: &str) -> String {
  format!("\"{exe}\"")
}

/// Task Manager's record of a startup app: its first byte odd when turned off (2 on, 3 off).
fn turned_off(record: &[u8]) -> bool {
  record.first().is_some_and(|b| b % 2 == 1)
}

/// Value `name` under HKCU `key` of type `ty`: its bytes, `None` when missing.
fn value(key: &str, name: &str, ty: u32) -> Option<Vec<u8>> {
  let (key, name) = (wide(key), wide(name));
  let mut len = 0u32;
  let get = |buf: *mut u8, len: &mut u32| unsafe {
    RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), ty, null_mut(), buf.cast(), len)
  };
  if get(null_mut(), &mut len) != 0 {
    return None;
  }

  let mut buf = vec![0u8; len as usize];
  (get(buf.as_mut_ptr(), &mut len) == 0).then(|| {
    buf.truncate(len as usize);
    buf
  })
}

pub fn state(app: &AppInfo) -> Result<State, String> {
  if value(RUN, app.id, RRF_RT_REG_SZ).is_none() {
    return Ok(State::NotRegistered);
  }

  match value(APPROVED, app.id, RRF_RT_REG_BINARY) {
    Some(r) if turned_off(&r) => Ok(State::RequiresApproval),
    _ => Ok(State::Enabled),
  }
}

pub fn set(app: &AppInfo, on: bool) -> Result<State, String> {
  let (key, name) = (wide(RUN), wide(app.id));
  let err = match on {
    true => {
      let exe = std::env::current_exe().map_err(|e| format!("the executable's path: {e}"))?;
      let data = wide(&command(&exe.to_string_lossy()));
      let bytes = (data.len() * 2) as u32;
      unsafe {
        RegSetKeyValueW(
          HKEY_CURRENT_USER,
          key.as_ptr(),
          name.as_ptr(),
          REG_SZ,
          data.as_ptr().cast(),
          bytes,
        )
      }
    }
    // Missing already (2, ERROR_FILE_NOT_FOUND) is what was asked.
    false => match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) } {
      2 => 0,
      e => e,
    },
  };

  if err != 0 {
    return Err(format!("the Run key: error {err}"));
  }

  state(app)
}

/// Settings › Apps › Startup.
pub fn open_settings() {
  let (op, uri) = (wide("open"), wide("ms-settings:startupapps"));
  unsafe { ShellExecuteW(null_mut(), op.as_ptr(), uri.as_ptr(), null(), null(), SW_SHOWNORMAL) };
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn run_values_quote_and_task_manager_turns_off() {
    assert_eq!(command(r"C:\Program Files\App\app.exe"), r#""C:\Program Files\App\app.exe""#);
    assert!(
      turned_off(&[3, 0, 0]) && !turned_off(&[2, 0]) && !turned_off(&[6]) && !turned_off(&[])
    );
  }
}
