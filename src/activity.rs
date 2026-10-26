// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Work the person waits for, at full speed while it runs ([begin]): an export, an import, a
//! download. Its [Activity], held on any thread, keeps the system from slowing the app or
//! sleeping until it is dropped.
//!
//! | | |
//! |---|---|
//! | macOS | `NSProcessInfo` `beginActivityWithOptions:` user-initiated: no App Nap (an Accessory app out of view is otherwise throttled), no idle sleep |
//! | Windows | a power request (`PowerCreateRequest`), the system required: no idle sleep |
//! | Linux | nothing yet |

use crate::platform::activity as imp;

/// Work under way; it ends when dropped. `Send`: begun on one thread, held and dropped on
/// another.
#[must_use = "the activity ends when dropped"]
pub struct Activity(Option<imp::Token>);

/// Starts an activity, `reason` saying what for (the system shows it to the person on macOS).
pub fn begin(reason: &str) -> Activity {
  Activity(Some(imp::begin(reason)))
}

impl Drop for Activity {
  fn drop(&mut self) {
    if let Some(t) = self.0.take() {
      imp::end(t);
    }
  }
}

#[cfg(test)]
mod tests {
  /// An activity begun on one thread ends on another.
  #[test]
  fn an_activity_moves_across_threads() {
    let a = super::begin("nimbo test");
    std::thread::spawn(move || drop(a)).join().unwrap();
  }
}
