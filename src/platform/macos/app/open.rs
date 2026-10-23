// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Opening links and files on macOS through `NSWorkspace`: `openURL:configuration:…`, which
//! returns at once (`openURL:` waits for the app that opens it, which may have to start first),
//! and Finder's window with an item selected.

use std::path::Path;

use objc2_app_kit::{NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{NSArray, NSString, NSURL};

pub fn url(url: &str) -> Result<(), String> {
  let u = NSURL::URLWithString(&NSString::from_str(url)).ok_or(format!("{url:?} is not a URL"))?;
  open(&u);
  Ok(())
}

pub fn path(path: &Path) -> Result<(), String> {
  open(&file_url(path));
  Ok(())
}

pub fn reveal(path: &Path) -> Result<(), String> {
  let urls = NSArray::from_retained_slice(&[file_url(path)]);
  NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
  Ok(())
}

pub fn file_manager() -> &'static str {
  "Finder"
}

fn file_url(path: &Path) -> objc2::rc::Retained<NSURL> {
  NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

fn open(url: &NSURL) {
  let config = NSWorkspaceOpenConfiguration::configuration();
  NSWorkspace::sharedWorkspace().openURL_configuration_completionHandler(url, &config, None);
}
