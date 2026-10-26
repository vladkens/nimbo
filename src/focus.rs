// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Where the user types in other apps: the app with the keyboard focus, its focused window and
//! whether a password field has the focus, for an app that types or pastes there (taken before,
//! and again right before the paste, to tell whether the focus [moved](Focused::moved)).
//!
//! Per OS: macOS the app with the keyboard focus asked live through Accessibility
//! (`AXFocusedApplication`; without access, `NSWorkspace`'s frontmost app, which its run loop
//! updates), its `AXFocusedWindow`, and an `AXSecureTextField` as the focused element; each
//! request waits at most a quarter of a second for the app, and an app that doesn't answer leaves
//! that part `Err`. Windows the foreground window, its process, and an `Edit` control with
//! `ES_PASSWORD` as the thread's focus (other controls, a browser's fields among them, aren't
//! asked: UI Automation would be). Linux on X11 the window manager's `_NET_ACTIVE_WINDOW` and its
//! `_NET_WM_PID` (`libX11`, loaded at run time; X11 has no password fields to tell); on Wayland,
//! where no app may see another's windows, there is none.

use crate::platform::focus as imp;

/// The app with the keyboard focus, as [current] found it.
#[derive(Clone, Debug, PartialEq)]
pub struct Focused {
  /// Its process id; 0 where the OS doesn't tell (an X11 window without `_NET_WM_PID`).
  pub pid: u32,
  /// Its name as the user reads it (macOS its localized name, Windows its executable's without
  /// `.exe`, Linux `/proc/<pid>/comm`), empty if unknown.
  pub name: String,
  /// Its focused window, `Ok(None)` when it has none in focus; `Err` when the OS could not get
  /// it (why: the app did not answer, macOS without Accessibility access).
  pub window: Result<Option<Window>, String>,
  /// A password field has the focus; `Ok(false)` also where the OS has no way to tell (see the
  /// module docs); `Err` as for `window`.
  pub secure: Result<bool, String>,
}

/// Another app's window, compared by identity.
#[derive(Clone, Debug, PartialEq)]
pub struct Window(pub(crate) imp::Window);

impl Focused {
  /// The focus moved from `self` to `now`: another app, or another window of it where the app
  /// told both.
  pub fn moved(&self, now: &Focused) -> bool {
    self.pid != now.pid || matches!((&self.window, &now.window), (Ok(a), Ok(b)) if a != b)
  }
}

/// The app with the keyboard focus now, from any thread; `None` when there is none or the OS
/// doesn't tell (Wayland).
pub fn current() -> Option<Focused> {
  imp::current()
}

/// The OS hides where the user types from other apps, so [current] is always `None`: Linux on
/// Wayland. A paste there goes wherever the focus is, unchecked.
pub fn hidden() -> bool {
  cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn moved_compares_what_both_told() {
    let window = current().and_then(|f| f.window.ok().flatten());
    let at = |pid: u32, window: Result<Option<Window>, String>| Focused {
      pid,
      name: "app".into(),
      window,
      secure: Ok(false),
    };
    let start = at(7, Ok(None));
    assert!(!start.moved(&at(7, Ok(None))));
    assert!(start.moved(&at(8, Ok(None))), "another app");
    assert!(!start.moved(&at(7, Err("busy".into()))), "the app did not say");
    assert!(!at(7, Err("busy".into())).moved(&start));
    if let Some(window) = window {
      assert!(start.moved(&at(7, Ok(Some(window.clone())))), "a window now");
      assert!(!at(7, Ok(Some(window.clone()))).moved(&at(7, Ok(Some(window)))));
    }
  }

  /// Read-only: it asks the apps, never prompts.
  #[test]
  fn current_reads() {
    if let Some(focused) = current() {
      assert!(cfg!(target_os = "linux") || focused.pid > 0, "{focused:?}");
    }
  }
}
