// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Launching the app at login. The OS keeps the state; an app that records the user's choice
//! should still read [state] back, as the user can change it in the OS's settings.
//!
//! | | where | when not [available] |
//! |---|---|---|
//! | macOS | `SMAppService.mainApp` (macOS 13+): System Settings → General → Login Items | below macOS 13, or a bare binary: it registers the app bundle the process runs from |
//! | Windows | the `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value named by the app's id; Task Manager's Startup apps can turn it off ([State::RequiresApproval]) | — |
//! | Linux | `$XDG_CONFIG_HOME/autostart/<id>.desktop` (XDG Autostart), run by the session | — |
//!
//! Windows and Linux start the executable that is running now (Linux: `$APPIMAGE` when set);
//! moved, the entry is stale until [set] is called again.

use crate::AppInfo;
use crate::platform::autostart as imp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
  Enabled,
  /// Registered, but the user has to allow it in the OS's settings ([open_settings]): macOS
  /// Login Items, Windows Task Manager's Startup apps.
  RequiresApproval,
  NotRegistered,
}

impl State {
  /// The user asked for it (it may still wait for approval).
  pub fn wanted(self) -> bool {
    matches!(self, Self::Enabled | Self::RequiresApproval)
  }
}

/// `Err` with the reason when this process cannot launch at login (macOS: below 13, or not run
/// from an app bundle).
pub fn available() -> Result<(), String> {
  imp::available()
}

pub fn state(app: &AppInfo) -> Result<State, String> {
  imp::available()?;
  imp::state(app)
}

/// Registers the app to launch at login, or unregisters it; the state after.
pub fn set(app: &AppInfo, on: bool) -> Result<State, String> {
  imp::available()?;
  imp::set(app, on)
}

/// Opens the OS's login items settings (macOS: Login Items; Windows: Settings → Apps →
/// Startup); nothing on Linux, which has no common one.
pub fn open_settings() {
  imp::open_settings()
}

/// A Desktop Entry spec entry that runs `exe` (quoted as the spec asks) under the app's name and
/// icon (named by its id), the `keys` after: an autostart entry or a launcher's
/// ([crate::desktop]).
#[cfg(any(test, target_os = "linux"))]
pub(crate) fn desktop_entry(app: &AppInfo, exe: &str, keys: &[(&str, &str)]) -> String {
  let mut arg = String::from("\"");
  for c in exe.chars() {
    match c {
      // Escaped inside the quotes (`\\` for `\`), then every backslash escaped again for the
      // string value: `\\\\` for a backslash, `\\$` for a dollar.
      '\\' => arg.push_str("\\\\\\\\"),
      '"' | '`' | '$' => arg.extend(['\\', '\\', c]),
      '%' => arg.push_str("%%"),
      '\n' | '\r' => arg.push(' '),
      c => arg.push(c),
    }
  }

  arg.push('"');
  // A string value: a backslash escaped (Exec's are above), line breaks as spaces.
  let esc = |v: &str| v.replace('\\', "\\\\").replace(['\n', '\r'], " ");
  let (name, id) = (esc(app.name), app.id);
  let mut out = format!(
    "[Desktop Entry]\nType=Application\nName={name}\nExec={arg}\nIcon={id}\nTerminal=false\n"
  );
  for (k, v) in keys {
    out += &format!("{k}={}\n", esc(v));
  }

  out
}

/// Whether an autostart entry runs: not `Hidden`, not turned off by GNOME's key.
#[cfg(any(test, target_os = "linux"))]
pub(crate) fn entry_runs(text: &str) -> bool {
  let off =
    |key: &str, v: &str| text.lines().any(|l| l.trim().replace(' ', "") == format!("{key}={v}"));
  !off("Hidden", "true") && !off("X-GNOME-Autostart-enabled", "false")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn desktop_entries_quote_the_path() {
    let app = AppInfo { id: "com.example.app", name: "Example", presence: Default::default() };
    let e = desktop_entry(
      &app,
      "/opt/My App/run$1 \"x\" 100% a\\b",
      &[("X-GNOME-Autostart-enabled", "true")],
    );
    assert!(e.contains("Name=Example\n") && e.contains("Icon=com.example.app\n"));
    let want = r#"Exec="/opt/My App/run\\$1 \\"x\\" 100%% a\\\\b""#;
    assert!(e.contains(&format!("{want}\n")), "{e}");
    assert!(entry_runs(&e));
    assert!(!entry_runs(&(e.clone() + "Hidden=true\n")));
    assert!(!entry_runs(&e.replace("enabled=true", "enabled=false")));
  }
}
