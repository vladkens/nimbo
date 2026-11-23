// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app as the OS knows it: its id and name, and where it shows besides its windows; what
//! the services (files, launch at login, the launcher entry) name it by.

/// The app as the OS knows it.
#[derive(Clone, Copy, Debug)]
pub struct AppInfo {
  /// Reverse DNS, such as `com.example.app`: its files ([crate::paths]), its launch at login
  /// ([crate::autostart]), its tray item on Linux.
  pub id: &'static str,
  /// What the user reads: the app menu's "Quit {name}", the login item.
  pub name: &'static str,
  pub presence: Presence,
}

/// Where the app shows besides its windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Presence {
  /// In the Dock and in ⌘Tab, as most apps.
  #[default]
  Regular,
  /// A menu-bar or tray app: no Dock icon, except while one of its windows is shown (macOS: the
  /// accessory activation policy, regular while a window is open). Windows and Linux have no
  /// Dock: the taskbar shows the windows only, so both are the same there.
  Accessory,
}
