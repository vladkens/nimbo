// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! nimbo's [Error]: why a call into the system failed, where no error of its own says more
//! ([crate::keys::ShortcutError], [crate::paste::NotPasted]) and the failure is not plain I/O
//! (`std::io::Error`: [crate::instance], [crate::uninstall::remove]).

use std::fmt;

/// Why a call failed, with the system's message; shown, it is the message alone. A failure nimbo
/// tells apart later is a new variant: match the ones you handle and `_` for the rest.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
  /// This system or session can't do it at all: launch at login below macOS 13 or outside an
  /// app bundle, a tray on a desktop with no host for it.
  Unsupported(String),
  /// What was given can't be read or used: no URL, a malformed SVG, an image no decoder takes, a
  /// hotkey already set at another place.
  Invalid(String),
  /// The system refused or failed: another app has the hotkey, no Accessibility access, a file
  /// that can't be written.
  System(String),
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Unsupported(why) | Self::Invalid(why) | Self::System(why) => f.write_str(why),
    }
  }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::open;

  #[test]
  fn shown_as_its_message() {
    let e = open::url("no url").unwrap_err();
    assert_eq!(e, Error::Invalid("\"no url\" is not a URL".into()));
    assert_eq!(e.to_string(), "\"no url\" is not a URL");
  }
}
