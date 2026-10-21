// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! macOS only (AppKit on the main thread): the scenario is in `tests/macos/shell.rs`.

#[cfg(target_os = "macos")]
#[path = "macos/shell.rs"]
mod imp;

fn main() {
  #[cfg(target_os = "macos")]
  imp::main();
}
