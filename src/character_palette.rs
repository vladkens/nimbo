// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The system's emoji & symbols picker for the focused text field
//! ([Ui::character_palette](crate::Ui::character_palette)),
//! as AppKit's `orderFrontCharacterPalette(_:)` (SwiftUI has none): the OS shows it at the
//! caret, and what the person picks comes into the field as typed text does. A field's edit menu
//! has it as Emoji & Symbols ([crate::Command::CharacterPalette]); an app shows an emoji button
//! of its own only where the OS has one ([available]):
//!
//! ```ignore
//! if character_palette::available().is_ok() && ui.icon_button(Icon::SMILE).clicked {
//!   ui.character_palette();
//! }
//! ```
//!
//! - macOS: Emoji & Symbols (`orderFrontCharacterPalette:`), the popover at the caret; AppKit adds
//!   the item, ⌃⌘Space, to the menu bar's Edit menu itself.
//! - Windows: the emoji panel, Win+. sent with `SendInput` while the window is in front, as
//!   Chromium opens it (there is no Win32 API); not [available] below Windows 10 1709.
//! - Linux: none, never [available]: GTK 3's emoji chooser is private to its own text widgets.
//!   IBus's emoji typing (Ctrl+Shift+E) still reaches a field, as any input method does.
//!
//! The harness answers it as a test says ([crate::Harness::character_palette]).

use crate::{Error, platform};

/// [Error::Unsupported] with the reason when the OS has no emoji picker for a nimbo field: Linux,
/// Windows below 10 1709.
pub fn available() -> Result<(), Error> {
  platform::character_palette().map_err(Error::Unsupported)
}

#[cfg(test)]
mod tests {
  use crate::ui::Id;
  use crate::{App, Harness, Size, Ui};

  /// A field focused as it first shows, the picker asked for by the view of a frame, not in
  /// answer to an event (a click's pass): the window takes it after that frame.
  #[derive(Default)]
  struct Composer {
    text: String,
    ask: bool,
  }

  impl App for Composer {
    fn view(&mut self, ui: &mut Ui) {
      ui.autofocus().text_field(&mut self.text, "Message");
      if self.ask {
        ui.character_palette();
      }
    }
  }

  #[test]
  fn a_frame_asks_for_it() {
    let mut h = Harness::new(Composer::default(), Size::new(200.0, 60.0), 1.0);
    h.frame();
    assert!(!h.character_palette(None), "not asked");
    h.app_mut().ask = true;
    h.frame();
    let asked = h.character_palette(Some("🙂"));
    assert_eq!(asked, super::available().is_ok());
    h.frame();
    assert_eq!(h.app().text, if asked { "🙂" } else { "" });
  }

  /// A field, a password and selectable text.
  #[derive(Default)]
  struct Three {
    text: String,
    secret: String,
  }

  impl App for Three {
    fn view(&mut self, ui: &mut Ui) {
      ui.id("f").text_field(&mut self.text, "Message");
      ui.id("p").secure_field(&mut self.secret, "Password");
      let body = ui.theme().body();
      _ = ui.id("s").selectable_text("Selectable", body);
    }
  }

  /// The edit menu offers Emoji & Symbols where the OS has a picker, in a field that takes it:
  /// not in a password, nor in selectable text; chosen, it asks for the picker.
  #[test]
  fn the_edit_menu_offers_it_where_a_field_takes_it() {
    let mut h = Harness::new(Three::default(), Size::new(300.0, 120.0), 1.0);
    h.set_native_menus(true);
    h.frame();
    let offers = |h: &mut Harness<Three>, id: &str, answer: Option<&str>| {
      let r = h.rect(Id::of(id)).unwrap();
      h.right_click(r.x + 5.0, r.y + r.h / 2.0);
      h.frame();
      let (_, menu) = h.context_menu(answer).expect("an edit menu");
      h.frame();
      menu.items.iter().any(|i| i.label() == "Emoji & Symbols")
    };

    let on = super::available().is_ok();
    assert_eq!(offers(&mut h, "f", Some("Emoji & Symbols")), on);
    assert_eq!(h.character_palette(Some("🙂")), on);
    assert!(!offers(&mut h, "p", None) && !offers(&mut h, "s", None));
  }
}
