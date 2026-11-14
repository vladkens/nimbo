// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The keyboard input sources the user types with, layouts and input modes ([InputSource]): the
//! one in use ([current]) and those the user can switch to ([enabled]), for an app that follows
//! the user's language (a dictation app asks for the layout's language). Their changes come as
//! [Change::InputSource] ([crate::Main::watch]), to read them again.
//!
//! | | the sources | changes |
//! |---|---|---|
//! | macOS | Text Input Sources (HIToolbox), on the main thread (TSM asserts it since macOS 14) | TIS's distributed notifications |
//! | Windows | the foreground window's layout (`GetKeyboardLayout`), `GetKeyboardLayoutList` | none: polled while watched |
//! | Linux GNOME | GSettings `org.gnome.desktop.input-sources` | its `changed` signal |
//! | Linux Plasma | `org.kde.keyboard` on the session bus | its `layoutChanged`, `layoutListChanged` |
//! | other Linux desktops | none: [available] is false | refused |
//!
//! Call them on the main thread.

use crate::platform::input_source as imp;
#[cfg(doc)]
use crate::system::Change;

/// A keyboard layout or an input mode (Kana of the Japanese input method), as the user picks it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputSource {
  /// The OS's id: `com.apple.keylayout.Russian`.
  pub id: String,
  /// Its name as the user reads it: `Russian – PC`.
  pub name: String,
  /// The language it types, the primary subtag of its first one in lower case (`en`, `ru`,
  /// `zh`); `None` when it has none (an emoji palette, a layout of symbols).
  pub language: Option<String>,
}

/// The platform tells the input sources: macOS, Windows, and GNOME and Plasma on Linux.
pub fn available() -> bool {
  imp::available()
}

/// The input source in use; `None` when the OS doesn't tell.
pub fn current() -> Option<InputSource> {
  imp::current()
}

/// The input sources the user can switch to (enabled and selectable), in the OS's order; empty
/// when the OS doesn't tell.
pub fn enabled() -> Vec<InputSource> {
  imp::enabled()
}

/// The primary subtag of a BCP 47 tag in lower case: `en-US` is `en`, `zh-Hans` is `zh`; `None`
/// for a tag without one of 2 or 3 letters.
pub(crate) fn primary(tag: &str) -> Option<String> {
  let sub = tag.split(['-', '_']).next()?;
  let letters = (2..=3).contains(&sub.len()) && sub.chars().all(|c| c.is_ascii_alphabetic());
  letters.then(|| sub.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_language_is_the_primary_subtag() {
    let cases = [
      ("ru", Some("ru")),
      ("en-US", Some("en")),
      ("zh-Hans", Some("zh")),
      ("pt_BR", Some("pt")),
      ("DE", Some("de")),
      ("haw", Some("haw")),
      ("", None),
      ("x", None),
      ("i-klingon-x", None),
    ];
    for (tag, want) in cases {
      assert_eq!(primary(tag).as_deref(), want, "{tag:?}");
    }
  }
}
