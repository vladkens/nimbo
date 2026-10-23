// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The shortcut recorder: a capsule showing a global [Hotkey] that records a new one, a chord
//! or a modifier key alone.
//!
//! ```ignore
//! if ui.id("hotkey").shortcut(&mut self.hotkey).changed { self.save() }
//! let text = self.hotkey.map(|h| h.to_string()); // "ctrl+option+space", "right_option"
//! ```
//!
//! The hotkey is system-wide ([Hotkey::parse]): its modifiers are the keys' own (`cmd` is ⌘,
//! the Windows or the Super key) and its key a place on the keyboard, so it reads back as typed
//! on any layout. It shows as the platform writes it ([Hotkey::label]): `⌃⌥Space`, `Right ⌥` on
//! macOS, `Ctrl+Alt+Space`, `Right Alt` elsewhere.

use keyboard_types::{Code, Modifiers, NamedKey};

use super::icon::{self, Icon};
use crate::keys::{Hotkey, Key, KeyEvent, Shortcut, f_key, is_modifier, us_char};
use crate::scene::mix;
use crate::text::Weight;
use crate::ui::{Behavior, Cursor, Float, Id, Response, Style, Ui, px};

/// The recorder's width: the same recording, set or empty.
const W: f32 = 160.0;

/// A recorder between frames: recording, the focus to give back after, the modifier keys
/// down, and the one down alone, which is the hotkey if it comes up so.
#[derive(Default)]
struct Rec {
  on: bool,
  prev: Option<Id>,
  held: u8,
  lone: Option<Code>,
}

/// The global hotkey a key press makes, or `None` when it makes none: a key with Ctrl, Alt
/// (Option) or Meta (⌘, Windows), or an F-key with any. The key is the physical one, as a US
/// keyboard has it (Shift-1 is `shift+1`).
pub fn hotkey_of(k: &KeyEvent) -> Option<Hotkey> {
  let f = (1..=20).any(|n| f_key(n).is_some_and(|(c, _)| c == k.code));
  if !(f || k.mods.intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::META)) {
    return None;
  }

  let key = match us_char(k.code, false) {
    Some(c) => Key::Char(c),
    None => Key::named(k.code).filter(|k| *k != Key::Named(NamedKey::Unidentified))?,
  };

  Some(Hotkey::Shortcut(Shortcut { key, mods: k.mods }))
}

impl Ui<'_> {
  /// A shortcut recorder over `value`: a click, or Space while focused, starts recording (`on`)
  /// and focuses it; the next chord typed ([hotkey_of]), or a modifier key pressed and released
  /// with nothing else between ([crate::Event::Modifier]: left or right, fn), becomes the value
  /// and stops it, `changed`. Escape stops it unchanged, Backspace or Delete clear the value; a
  /// click again or elsewhere, or focus moving away, stop it unchanged. With `clear`, a click
  /// on `id.key("clear")` (a ⊗ the look draws) clears the value too. Every key is taken
  /// while recording, before the app's bindings ([Ui::grab_keys]), so none reaches anything
  /// else: Cmd-Z records, it doesn't undo.
  pub fn shortcut_behavior(&mut self, id: Id, value: &mut Option<Hotkey>, clear: bool) -> Behavior {
    if let Some(b) = self.begin(id) {
      self.state::<Rec>(id).on = false;
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
            rec.lone = (rec.held == 0).then_some(key);
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
            if let Some(h) = hotkey_of(&k) {
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
    *self.state::<Rec>(id) = rec;
    b
  }

  /// A shortcut recorder over `value` ([Ui::shortcut_behavior]), 160 wide whatever it shows: a
  /// capsule with the hotkey ([Hotkey::label]), "Record Shortcut" when empty, and a ⊗ that
  /// clears it (`id.key("clear")`) where the theme has one ([crate::Theme::shortcut_clear]: not
  /// the plates themes, whose recorder sits by a reset button); recording, the ground with an
  /// accent edge and the focus ring, a red dot and "Type a shortcut…". `changed` when a hotkey
  /// was recorded or the value cleared.
  pub fn shortcut(&mut self, value: &mut Option<Hotkey>) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let clear = id.key("clear");
    let b = self.shortcut_behavior(id, value, t.shortcut_clear);
    let hot = self.ctl_hot(id, b.hovered);
    let h = t.sizes.control_h;
    let bg = match (b.on, b.pressed) {
      (true, _) => t.bg,
      (_, true) => t.pressed,
      _ => mix(t.raised, t.hover, hot),
    };

    let x = if t.shortcut_clear { 22.0 } else { 10.0 };
    let frame = Style::new().w(px(W)).h(px(h)).pad_xy(x, 0.0).gap(6.0).center().radius(h / 2.0);
    let frame = self.control(frame.bg(bg).border(if b.on { 1.0 } else { 0.0 }, t.accent), &b);
    let label = t.label();
    let mut r = self.with_id(id).row(frame, |ui| {
      match (b.on, *value) {
        (true, _) => {
          ui.el(Style::new().w(px(7.0)).h(px(7.0)).radius(3.5).bg(t.danger));
          ui.text("Type a shortcut…", label.color(t.muted));
        }
        (_, None) => ui.text("Record Shortcut", label.color(t.muted)),
        (_, Some(v)) => ui.text(v.label(), label.font(t.font.weight(Weight::Medium))),
      }

      if value.is_some() && !b.on && !b.disabled && t.shortcut_clear {
        let at = Float::parent().attach(1.0, 0.5).anchor(1.0, 0.5).offset(-5.0, 0.0).clip();
        let s = Style::new().w(px(14.0)).h(px(14.0)).cursor(Cursor::Pointer);
        let hot = ui.ctl_hot(clear, ui.hovered(clear));
        let mark = mix(t.faint, t.muted, hot);
        ui.with_id(clear).float(at).canvas(s, move |f, r| icon::draw(f, Icon::CLEAR, r, mark, 0.0));
      }
    });
    (r.clicked, r.changed) = (b.clicked, b.changed);
    r
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::{App, Cx, Event};
  use crate::harness::{Harness, us_key};
  use crate::keys::{Binding, CMD, Command, bind};
  use crate::scene::Size;

  /// A recorder in an app that binds Cmd-Z.
  #[derive(Default)]
  struct Undo {
    value: Option<Hotkey>,
    undos: usize,
  }

  impl App for Undo {
    fn keymap(&self) -> &[Binding] {
      const KEYS: &[Binding] = &[bind("cmd+z", Command::Action("undo"))];
      KEYS
    }

    fn event(&mut self, _: &mut Cx, ev: &Event) -> bool {
      let undo = matches!(ev, Event::Command(Command::Action("undo")));
      self.undos += undo as usize;
      undo
    }

    fn view(&mut self, ui: &mut Ui) {
      ui.id("rec").shortcut(&mut self.value);
    }
  }

  #[test]
  fn recording_takes_the_apps_shortcuts_too() {
    let mut h = Harness::new(Undo::default(), Size::new(300.0, 100.0), 1.0);
    h.frame();
    h.key('z', CMD);
    assert_eq!(h.app().undos, 1);
    h.click_id(Id::of("rec"));
    h.key('z', CMD);
    assert_eq!(h.app().undos, 1);
    assert_eq!(h.app().value, Some(Hotkey::Shortcut(Shortcut { key: Key::Char('z'), mods: CMD })));
    // Stopped, the app has its shortcut back.
    h.key('z', CMD);
    assert_eq!(h.app().undos, 2);
  }

  fn hk(key: impl Into<Key>, mods: Modifiers) -> Option<String> {
    hotkey_of(&us_key(key.into(), mods)).map(|h| h.to_string())
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
      let h = hotkey_of(&us_key(k, m)).unwrap();
      assert_eq!(Hotkey::parse(&h.to_string()), Ok(h));
    }
  }
}
