// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app as the OS knows it: its id and name, and where it shows besides its windows; what
//! the services (files, launch at login, the launcher entry) name it by.

/// The app as the OS knows it: `AppInfo::new("com.example.app", "App")`.
#[derive(Clone, Copy, Debug)]
pub struct AppInfo {
  pub(crate) id: &'static str,
  pub(crate) name: &'static str,
  pub(crate) presence: Presence,
}

impl AppInfo {
  /// A [Presence::Regular] app.
  pub const fn new(id: &'static str, name: &'static str) -> Self {
    Self { id, name, presence: Presence::Regular }
  }

  pub const fn presence(self, presence: Presence) -> Self {
    Self { presence, ..self }
  }

  /// Reverse DNS, such as `com.example.app`: its files ([crate::paths]), its launch at login
  /// ([crate::autostart]), its tray item on Linux.
  pub const fn id(&self) -> &'static str {
    self.id
  }

  /// What the user reads: the app menu's "Quit {name}", the login item.
  pub const fn name(&self) -> &'static str {
    self.name
  }

  pub const fn get_presence(&self) -> Presence {
    self.presence
  }
}

/// Where the app shows besides its windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Presence {
  /// In the Dock and in ⌘Tab, as most apps.
  #[default]
  Regular,
  /// A menu-bar or tray app: no Dock icon, except while one of its windows is shown (macOS: the
  /// accessory activation policy, regular while a window is open). Windows and Linux have no
  /// Dock: the taskbar shows the windows only, so both are the same there.
  Accessory,
}
