// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A search field's behaviour ([Ui::search_behavior]): a single-line text field's, its text
//! cleared by a ⊗ or Escape, as macOS's search fields; `Ui::search_field` draws it.

use super::field::FieldState;
use crate::NamedKey;
use crate::app::MouseButton;
use crate::keys::Key;
use crate::ui::{Enabled, FieldBehavior, FieldKind, Id, Ui};

impl Ui<'_> {
  /// A search field's input: [Ui::text_field_behavior]'s on one line, and a click on
  /// `id.key("clear")` (the ⊗ a look draws while there is text) or Escape clears the text,
  /// `changed`, an input method's composition with it, as one edit that undoes. The ⊗ (a press
  /// of the left button on it) focuses the field; Escape is the
  /// field's before any binding while there is text and nothing composing, else it goes on.
  pub fn search_behavior(&mut self, id: Id, text: &mut String) -> FieldBehavior {
    let off = !self.environment_value::<Enabled>().0;
    let x = id.key("clear");
    // The ⊗ is the field's: a press on it leaves the focus there, or brings it.
    if !off && self.press().is_some_and(|p| p.id == x && p.button == MouseButton::Left) {
      self.focus(Some(id));
    }

    // A click on the ⊗ clears what it was shown over, whatever the app set since.
    let had = !text.is_empty();
    let mut cleared = !off && self.clicked(x);
    if cleared {
      self.focus(Some(id));
    }

    if let Some(k) = self.key_down(id) {
      let composing = self.state::<FieldState>(id).ed.marked().is_some();
      match k.key == Key::Named(NamedKey::Escape) && k.mods.is_empty() {
        true if had && !composing => cleared = true,
        _ => self.pass(id),
      }
    }

    if cleared {
      let now = self.now;
      self.state::<FieldState>(id).clear(text, now);
    }

    let mut b = self.text_field_behavior(id, text, FieldKind::SingleLine);
    if b.focused && !text.is_empty() {
      self.grab_keys(id);
    }

    b.changed |= cleared && had;
    b
  }
}
