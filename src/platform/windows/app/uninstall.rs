// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What [crate::uninstall::system] undoes on Windows besides launch at login: nothing, as nimbo
//! sets nothing else up. Ported from wavo-app's `src/cli.rs` (vladkens, MIT).

use std::io;

use windows_sys::Win32::Foundation::{
  ERROR_ACCESS_DENIED, ERROR_DIR_NOT_EMPTY, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION,
};

use crate::AppInfo;
use crate::uninstall::Step;

pub fn steps(_: &AppInfo) -> Vec<Step> {
  Vec::new()
}

/// A file another process has open (a sharing or lock violation) or is deleting (access denied),
/// a folder not empty yet: the app that just quit may hold its files a moment after its lock.
pub fn busy(e: &io::Error) -> bool {
  let codes =
    [ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION, ERROR_ACCESS_DENIED, ERROR_DIR_NOT_EMPTY];
  e.raw_os_error().is_some_and(|c| codes.contains(&(c as u32)))
}
