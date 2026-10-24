// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What the OS lets the app do to other apps and devices, where it asks the user first: macOS's
//! Privacy & Security. Accessibility is what an event tap ([crate::hotkey]) and posting keys
//! ([crate::keys::post]) need; Secure Input, which a password field turns on, keeps keys from
//! both; the microphone is AVFoundation's to grant (AVFoundation loaded at the first call, not
//! at launch). Windows and Linux are taken as asking nothing: everything is granted there, no pane
//! opens and Secure Input is never on (Windows' microphone switch in Settings › Privacy isn't read
//! yet).

/// A pane of System Settings › Privacy & Security.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
  /// Controlling the computer: event taps that change events, posting keys, reading other apps'
  /// windows.
  Accessibility,
  Microphone,
  /// Listening to the keyboard while other apps have it (a listen-only event tap).
  InputMonitoring,
  /// Paste from Other Apps (macOS 15.4+): reading the clipboard without asking
  /// ([crate::SystemClipboard::readable]).
  Pasteboard,
}

impl Pane {
  /// The pane's URL, which [open_pane] opens.
  pub fn url(self) -> &'static str {
    match self {
      Self::Accessibility => {
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
      }
      Self::Microphone => {
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
      }
      Self::InputMonitoring => {
        "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
      }
      Self::Pasteboard => {
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Pasteboard"
      }
    }
  }
}

/// Whether the app has Accessibility access; with `prompt`, macOS shows its dialog pointing the
/// user to System Settings when it has not (once per launch is enough: the answer comes later,
/// in System Settings, and the next call sees it).
pub fn accessibility(prompt: bool) -> bool {
  #[cfg(target_os = "macos")]
  return crate::platform::permissions::accessibility(prompt);
  #[cfg(not(target_os = "macos"))]
  {
    _ = prompt;
    true
  }
}

/// Opens `pane` in System Settings, for the user to allow the app there; nothing elsewhere.
pub fn open_pane(pane: Pane) -> Result<(), String> {
  match cfg!(target_os = "macos") {
    true => crate::open::url(pane.url()),
    false => Ok(()),
  }
}

/// Some app holds Secure Input (a password field, a terminal's secure keyboard entry): keys
/// don't reach event taps, and posted ones may not arrive.
pub fn secure_input() -> bool {
  #[cfg(target_os = "macos")]
  return crate::platform::permissions::secure_input();
  #[cfg(not(target_os = "macos"))]
  false
}

/// What the user decided about a device the app asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
  Granted,
  /// The user said no; System Settings changes it ([open_pane]).
  Denied,
  /// A policy (parental controls, a device profile) keeps it, and the user can't change it.
  Restricted,
  /// Never asked: the first use (or [request_microphone]) shows the system's prompt.
  NotDetermined,
}

/// Whether the app may record from the microphone; asks nothing.
pub fn microphone() -> Access {
  #[cfg(target_os = "macos")]
  return crate::platform::permissions::microphone();
  #[cfg(not(target_os = "macos"))]
  Access::Granted
}

/// Shows the system's prompt for the microphone, once ever per app (later calls answer at once
/// with the decision); `done` gets the answer on the main thread, while the app runs. Elsewhere
/// `done` gets true at once.
pub fn request_microphone(done: impl FnOnce(bool) + Send + 'static) {
  #[cfg(target_os = "macos")]
  crate::platform::permissions::request_microphone(Box::new(done));
  #[cfg(not(target_os = "macos"))]
  done(true);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn panes_are_privacy_anchors() {
    for (pane, anchor) in [
      (Pane::Accessibility, "Privacy_Accessibility"),
      (Pane::Microphone, "Privacy_Microphone"),
      (Pane::InputMonitoring, "Privacy_ListenEvent"),
      (Pane::Pasteboard, "Privacy_Pasteboard"),
    ] {
      let url = pane.url();
      assert!(url.starts_with("x-apple.systempreferences:com.apple.preference.security?"));
      assert!(url.ends_with(&format!("?{anchor}")), "{url}");
    }
  }

  /// Reading needs no permission and never prompts.
  #[test]
  fn reading_never_prompts() {
    let (trusted, secure, mic) = (accessibility(false), secure_input(), microphone());
    if !cfg!(target_os = "macos") {
      assert!(trusted && !secure && mic == Access::Granted);
    }
  }
}
