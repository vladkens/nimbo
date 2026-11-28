// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A hotkey recorder's behaviour ([Ui::hotkey_recorder_behavior]): recording a global hotkey, a
//! chord ([Hotkey::from_key_event]) or a modifier key alone, with every key taken while it records;
//! `Ui::hotkey_recorder` draws it.

use crate::keys::{Hotkey, Key, is_modifier};
use crate::ui::{Behavior, Id, Role, Ui};
use crate::{Code, NamedKey};

/// A recorder between frames: recording, the focus to give back after, the modifier keys
/// down, and the one down alone, which is the hotkey if it comes up so.
#[derive(Default)]
struct Rec {
  on: bool,
  prev: Option<Id>,
  held: u8,
  lone: Option<Code>,
}

impl Ui<'_> {
  /// A hotkey recorder over `value`: a click, or Space while focused, starts recording (`on`)
  /// and focuses it; the next chord typed ([Hotkey::from_key_event]), or with `lone` a modifier key
  /// pressed and released with nothing else between ([crate::Event::Modifier]: left or right, fn),
  /// becomes the value and stops it, `changed`. Escape stops it unchanged, Backspace or Delete
  /// clear the value; a click again or elsewhere, or focus moving away, stop it unchanged. With
  /// `clear`, a click on `id.key("clear")` (a ⊗ the look draws) clears the value too. Every key
  /// is taken while recording, before the app's bindings ([Ui::grab_keys]), so none reaches
  /// anything else: Cmd-Z records, it doesn't undo.
  pub fn hotkey_recorder_behavior(
    &mut self,
    id: Id,
    value: &mut Option<Hotkey>,
    clear: bool,
    lone: bool,
  ) -> Behavior {
    // A button whose value is the hotkey, as the platform writes it.
    let shown = |v: &Option<Hotkey>| v.map(|h| h.label()).unwrap_or_default();
    if let Some(b) = self.begin(id, Role::Button) {
      self.state::<Rec>(id).on = false;
      self.a11y(id, Role::Button).text(&shown(value));
      return b;
    }

    let mut b = self.pointer_state(id);
    if clear && self.clicked(id.key("clear")) && value.is_some() {
      (*value, b.changed) = (None, true);
    }

    let mut rec = std::mem::take(self.state::<Rec>(id));
    let mut stop = false;
    let mut set = |b: &mut Behavior, v: Option<Hotkey>| {
      b.changed |= *value != v;
      *value = v;
    };

    if rec.on {
      if let Some((key, down)) = self.modifier_key(id).filter(|m| is_modifier(m.0)) {
        match down {
          true => {
            rec.lone = (rec.held == 0 && lone).then_some(key);
            rec.held += 1;
          }
          false => {
            rec.held = rec.held.saturating_sub(1);
            if rec.lone.take() == Some(key) {
              set(&mut b, Some(Hotkey::Modifier(key)));
              stop = true;
            }
          }
        }
      }

      if let Some(k) = self.key_down(id) {
        rec.lone = None;
        match k.key {
          Key::Named(NamedKey::Escape) if k.mods.is_empty() => stop = true,
          Key::Named(NamedKey::Backspace | NamedKey::Delete) if k.mods.is_empty() => {
            set(&mut b, None);
            stop = true;
          }
          _ => {
            if let Some(h) = Hotkey::from_key_event(&k) {
              set(&mut b, Some(h));
              stop = true;
            }
          }
        }
      }

      // A button down or a click on anything else, whatever took the focus.
      let click = self.st.input.click.map(|c| c.id);
      let away = self.press().map(|p| p.id).or(click).is_some_and(|p| p != id);
      stop |= b.clicked || away || !self.focused(id);
    } else if b.clicked || self.activated(id) {
      b.clicked = true;
      rec = Rec { on: true, prev: self.focus_id().filter(|&f| f != id), ..Rec::default() };
      self.focus(Some(id));
    }

    if stop {
      if self.focused(id) {
        self.focus(rec.prev);
      }

      rec = Rec::default();
    }

    if rec.on {
      self.grab_keys(id);
    }

    b.on = rec.on;
    self.a11y(id, Role::Button).text(&shown(value));
    *self.state::<Rec>(id) = rec;
    b
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::{App, Event};
  use crate::harness::us_key;
  use crate::keys::Shortcut;
  use crate::scene::Size;
  use crate::ui::{Style, px};
  use crate::{Harness, Modifiers};

  /// A recorder's behaviour where a modifier key alone can't be the hotkey (Linux).
  #[derive(Default)]
  struct Chords(Option<Hotkey>);

  impl App for Chords {
    fn view(&mut self, ui: &mut Ui) {
      let b = ui.hotkey_recorder_behavior(Id::of("rec"), &mut self.0, false, false);
      ui.control(&b).el(Style::new().w(px(160.0)).h(px(22.0)));
    }
  }

  #[test]
  fn no_modifier_alone_where_the_platform_grabs_none() {
    let mut h = Harness::new(Chords::default(), Size::new(300.0, 100.0), 1.0);
    h.frame();
    h.click_id(Id::of("rec"));
    for down in [true, false] {
      h.event(Event::Modifier { key: Code::AltRight, down });
    }

    assert_eq!((h.app().0, h.focus_id()), (None, Some(Id::of("rec"))), "still recording");
    h.key('k', Modifiers::CONTROL);
    let ctrl_k = Hotkey::Shortcut(Shortcut { key: Key::Char('k'), mods: Modifiers::CONTROL });
    assert_eq!(h.app().0, Some(ctrl_k));
  }

  fn hk(key: impl Into<Key>, mods: Modifiers) -> Option<String> {
    Hotkey::from_key_event(&us_key(key.into(), mods)).map(|h| h.to_string())
  }

  #[test]
  fn key_presses_become_hotkeys_that_read_back() {
    let all = Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT | Modifiers::META;
    assert_eq!(hk(' ', Modifiers::ALT).as_deref(), Some("option+space"));
    assert_eq!(hk(NamedKey::F12, all).as_deref(), Some("ctrl+option+shift+cmd+f12"));
    assert_eq!(hk('K', Modifiers::META).as_deref(), Some("shift+cmd+k"));

    // The physical key: Shift-1 is 1, Shift-/ is slash.
    let ctrl_shift = Modifiers::CONTROL | Modifiers::SHIFT;
    assert_eq!(hk('!', ctrl_shift).as_deref(), Some("ctrl+shift+1"));
    assert_eq!(hk('?', ctrl_shift).as_deref(), Some("ctrl+shift+slash"));
    assert_eq!(hk(NamedKey::Enter, Modifiers::ALT).as_deref(), Some("option+return"));
    assert_eq!(hk(NamedKey::F5, Modifiers::empty()).as_deref(), Some("f5"));
    // No modifier, or Shift alone: no hotkey.
    assert_eq!(hk('a', Modifiers::empty()), None);
    assert_eq!(hk('a', Modifiers::SHIFT), None);

    // What it writes parses back to the same hotkey.
    for (k, m) in [(Key::Char(' '), Modifiers::CONTROL | Modifiers::ALT), ('/'.into(), all)] {
      let h = Hotkey::from_key_event(&us_key(k, m)).unwrap();
      assert_eq!(Hotkey::parse(&h.to_string()), Ok(h));
    }
  }
}
