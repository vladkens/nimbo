// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The scenarios that need AppKit's objects on the main thread, run from `main` rather than the
//! test harness, whose tests run on threads of their own (`harness = false`): the app shell with
//! hidden windows (`tests/macos/shell.rs`), then the detached tray's NSMenu
//! (`tests/tray/detached.rs`, a plain test elsewhere). macOS only.

#[cfg(target_os = "macos")]
#[path = "macos/shell.rs"]
mod shell;

#[cfg(target_os = "macos")]
#[path = "tray/detached.rs"]
mod detached;

fn main() {
  #[cfg(target_os = "macos")]
  {
    shell::main();
    // The menu's actions go through NSApp.
    let mtm = objc2::MainThreadMarker::new().expect("main thread");
    objc2_app_kit::NSApplication::sharedApplication(mtm);
    detached::run();
    println!("appkit scenarios: ok");
  }
}
