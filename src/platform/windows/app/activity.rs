// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Activities on Windows ([crate::activity]): a power request keeping the system awake.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Power::{
  PowerClearRequest, PowerCreateRequest, PowerRequestSystemRequired, PowerSetRequest,
};
use windows_sys::Win32::System::Threading::{
  POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
};

use super::super::wide;

/// The request's handle, null when it couldn't be made.
pub struct Token(HANDLE);

// SAFETY: a power request is a kernel handle, not tied to a thread.
unsafe impl Send for Token {}

pub fn begin(reason: &str) -> Token {
  let mut why = wide(reason);
  let reason = REASON_CONTEXT_0 { SimpleReasonString: why.as_mut_ptr() };
  // Version 0: POWER_REQUEST_CONTEXT_VERSION.
  let ctx =
    REASON_CONTEXT { Version: 0, Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING, Reason: reason };
  let h = unsafe { PowerCreateRequest(&ctx) };
  // INVALID_HANDLE_VALUE when it fails.
  if h as isize == -1 || h.is_null() {
    return Token(null_mut());
  }

  unsafe { PowerSetRequest(h, PowerRequestSystemRequired) };
  Token(h)
}

pub fn end(t: Token) {
  if !t.0.is_null() {
    unsafe { PowerClearRequest(t.0, PowerRequestSystemRequired) };
    unsafe { CloseHandle(t.0) };
  }
}
