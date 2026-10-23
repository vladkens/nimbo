// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Opening links and files on Windows: `ShellExecuteW`'s "open" verb (the app registered for the
//! scheme or the file type), and Explorer started with `/select,` for an item shown selected.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::super::wide;

pub fn url(url: &str) -> Result<(), String> {
  let (verb, file) = (wide("open"), wide(url));
  let done = unsafe {
    ShellExecuteW(null_mut(), verb.as_ptr(), file.as_ptr(), null(), null(), SW_SHOWNORMAL)
  };
  // An HINSTANCE above 32 is success, the rest error codes (its docs).
  match done as usize > 32 {
    true => Ok(()),
    false => Err(format!("could not open {url} (ShellExecute: {})", done as usize)),
  }
}

pub fn path(path: &Path) -> Result<(), String> {
  url(&path.to_string_lossy())
}

/// Explorer parses its own command line: the path is quoted after the comma, not the argument.
pub fn reveal(path: &Path) -> Result<(), String> {
  let arg = format!("/select,\"{}\"", path.display());
  let started = std::process::Command::new("explorer").raw_arg(arg).spawn();
  started.map(drop).map_err(|e| format!("explorer: {e}"))
}

pub fn file_manager() -> &'static str {
  "Explorer"
}
