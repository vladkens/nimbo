// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The shortcut recorder: a capsule showing a global [Hotkey] that records a new one, a chord
//! or a modifier key alone where the platform can grab one ([crate::hotkey::modifier_alone]), or
//! a key taken while a hotkey is held ([Ui::shortcut_key]: any key, with or without modifiers).
//!
//! ```ignore
//! if ui.id("hotkey").shortcut(&mut self.hotkey).changed { self.save() }
//! let text = self.hotkey.map(|h| h.to_string()); // "ctrl+option+space", "right_option"
//! ```
//!
//! The hotkey is system-wide ([Hotkey::parse]): its modifiers are the keys' own (`cmd` is ⌘,
//! the Windows or the Super key) and its key a place on the keyboard, so it reads back as typed
//! on any layout. It shows as the platform writes it ([Hotkey::label]), keys as a US keyboard has
//! them: `⌃⌥Space`, `Right ⌥`, `fn 1` on macOS, `Ctrl+Alt+Space`, `Right Alt` elsewhere.

use keyboard_types::{Code, Modifiers, NamedKey};

use super::icon::{self, Icon};
use crate::keys::{Hotkey, Key, KeyEvent, Shortcut, carries_fn, f_key, is_modifier, us_char};
use crate::scene::mix;
use crate::text::Weight;
use crate::ui::{Behavior, Cursor, Float, Id, Response, Role, Style, Ui, px};

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

/// What a recorder records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Record {
  /// A chord ([hotkey_of]).
  Chords,
  /// A chord, or a modifier key pressed and released alone (left or right, fn).
  ChordsOrModifier,
  /// A key taken while a hotkey is held ([crate::hotkey::Hotkeys::set]): the next one that is no
  /// modifier, by place, with the modifiers held but fn (`z`, `shift+z`, `option+1`, `f5`).
  Key,
}

/// The global hotkey a key press makes, or `None` when it makes none: a key with Ctrl, Alt
/// (Option), Meta (⌘, Windows) or fn, or an F-key with any. The key is the physical one, as a US
/// keyboard has it (Shift-1 is `shift+1`).
pub fn hotkey_of(k: &KeyEvent) -> Option<Hotkey> {
  let f = (1..=20).any(|n| f_key(n).is_some_and(|(c, _)| c == k.code));
  let chord = Modifiers::CONTROL | Modifiers::ALT | Modifiers::META | Modifiers::FN;
  (f || k.mods.intersects(chord)).then(|| key_of(k, k.mods)).flatten()
}

/// The key press `k` with `mods`, its key by place.
fn key_of(k: &KeyEvent, mods: Modifiers) -> Option<Hotkey> {
  let key = match us_char(k.code, false) {
    Some(c) => Key::Char(c),
    None => Key::named(k.code).filter(|k| *k != Key::Named(NamedKey::Unidentified))?,
  };

  Some(Hotkey::Shortcut(Shortcut { key, mods }))
}

impl Ui<'_> {
  /// A shortcut recorder over `value`: a click, or Space while focused, starts recording (`on`)
  /// and focuses it; what `mode` records typed next (a chord, [hotkey_of]; a modifier key
  /// pressed and released with nothing else between, [crate::Event::Modifier]; a key) becomes
  /// the value and stops it, `changed`. Escape stops it unchanged, Backspace or Delete
  /// clear the value; a click again or elsewhere, or focus moving away, stop it unchanged. With
  /// `clear`, a click on `id.key("clear")` (a ⊗ the look draws) clears the value too. Every key
  /// is taken while recording, before the app's bindings ([Ui::grab_keys]), so none reaches
  /// anything else: Cmd-Z records, it doesn't undo.
  pub fn shortcut_behavior(
    &mut self,
    id: Id,
    value: &mut Option<Hotkey>,
    clear: bool,
    mode: Record,
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
            rec.lone = (rec.held == 0 && mode == Record::ChordsOrModifier).then_some(key);
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

      if let Some(mut k) = self.key_down(id) {
        rec.lone = None;
        // fn held (macOS), which key events don't carry: the chord's modifier.
        k.mods.set(Modifiers::FN, self.st.input.fn_held && !carries_fn(k.key));
        match k.key {
          Key::Named(NamedKey::Escape) if (k.mods - Modifiers::FN).is_empty() => stop = true,
          Key::Named(NamedKey::Backspace | NamedKey::Delete)
            if (k.mods - Modifiers::FN).is_empty() =>
          {
            set(&mut b, None);
            stop = true;
          }
          _ => {
            let key = || key_of(&k, k.mods - Modifiers::FN).filter(|_| !is_modifier(k.code));
            if let Some(h) = if mode == Record::Key { key() } else { hotkey_of(&k) } {
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

  /// A shortcut recorder over `value` ([Ui::shortcut_behavior]; a modifier key alone where
  /// [crate::hotkey::modifier_alone]), 160 wide whatever it shows: a
  /// capsule with the hotkey ([Hotkey::label]), "Record Shortcut" when empty, and a ⊗ that
  /// clears it (`id.key("clear")`) where the theme has one ([crate::Theme::shortcut_clear]: not
  /// the plates themes, whose recorder sits by a reset button); recording, the ground with an
  /// accent edge and the focus ring, a red dot and "Type a shortcut…". `changed` when a hotkey
  /// was recorded or the value cleared.
  pub fn shortcut(&mut self, value: &mut Option<Hotkey>) -> Response {
    let lone = crate::hotkey::modifier_alone();
    self.recorder(value, if lone { Record::ChordsOrModifier } else { Record::Chords })
  }

  /// A recorder of a key taken while a hotkey is held ([Record::Key]), as [Ui::shortcut] draws
  /// one: "Record Key" when empty, "Type a key…" recording. Escape alone stops it, Backspace or
  /// Delete alone clear it: those three are set as text if wanted.
  pub fn shortcut_key(&mut self, value: &mut Option<Hotkey>) -> Response {
    self.recorder(value, Record::Key)
  }

  fn recorder(&mut self, value: &mut Option<Hotkey>, mode: Record) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let clear = id.key("clear");
    let b = self.shortcut_behavior(id, value, t.shortcut_clear, mode);
    let (empty, prompt) = match mode {
      Record::Key => ("Record Key", "Type a key…"),
      _ => ("Record Shortcut", "Type a shortcut…"),
    };

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
          ui.text(prompt, label.color(t.muted));
        }
        (_, None) => ui.text(empty, label.color(t.muted)),
        (_, Some(v)) => ui.text(v.label(), label.font(t.font.weight(Weight::Medium))),
      }

      if value.is_some() && !b.on && !b.disabled && t.shortcut_clear {
        let at = Float::parent().attach(1.0, 0.5).anchor(1.0, 0.5).offset(-5.0, 0.0).clip();
        let s = Style::new().w(px(14.0)).h(px(14.0)).cursor(Cursor::Pointer);
        ui.a11y(clear, Role::Button).name("Clear");
        let hot = ui.ctl_hot(clear, ui.hovered(clear));
        let mark = mix(t.faint, t.muted, hot);
        ui.with_id(clear).float(at).canvas(s, move |f, r| icon::draw(f, Icon::CLEAR, r, mark, 0.0));
      }
    });
    (r.clicked, r.changed, r.recording) = (b.clicked, b.changed, b.on);
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

  /// A recorder's behaviour where a modifier key alone can't be the hotkey (Linux).
  #[derive(Default)]
  struct Chords(Option<Hotkey>);

  impl App for Chords {
    fn view(&mut self, ui: &mut Ui) {
      let b = ui.shortcut_behavior(Id::of("rec"), &mut self.0, false, Record::Chords);
      ui.with_id(b.id).el(b.style(Style::new().w(px(W)).h(px(22.0))));
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

    assert_eq!((h.app().0, h.focused()), (None, Some(Id::of("rec"))), "still recording");
    h.key('k', Modifiers::CONTROL);
    let ctrl_k = Hotkey::Shortcut(Shortcut { key: Key::Char('k'), mods: Modifiers::CONTROL });
    assert_eq!(h.app().0, Some(ctrl_k));
  }

  /// A key recorder's behaviour.
  #[derive(Default)]
  struct Keys(Option<Hotkey>);

  impl App for Keys {
    fn view(&mut self, ui: &mut Ui) {
      let b = ui.shortcut_behavior(Id::of("rec"), &mut self.0, false, Record::Key);
      ui.with_id(b.id).el(b.style(Style::new().w(px(W)).h(px(22.0))));
    }
  }

  #[test]
  fn a_key_recorder_records_any_key_but_fn() {
    let mut h = Harness::new(Keys::default(), Size::new(300.0, 100.0), 1.0);
    h.frame();
    let mut record = |key: Key, mods| {
      h.click_id(Id::of("rec"));
      h.key(key, mods);
      h.app().0.map(|k| k.to_string())
    };
    assert_eq!(record('z'.into(), Modifiers::empty()).as_deref(), Some("z"));
    assert_eq!(record('Z'.into(), Modifiers::SHIFT).as_deref(), Some("shift+z"));
    assert_eq!(record('1'.into(), Modifiers::ALT).as_deref(), Some("option+1"));
    assert_eq!(record(NamedKey::F5.into(), Modifiers::empty()).as_deref(), Some("f5"));
    assert_eq!(record(' '.into(), Modifiers::empty()).as_deref(), Some("space"));
    assert_eq!(record('x'.into(), Modifiers::FN).as_deref(), Some("x"), "fn dropped");
    // A modifier alone is ignored: still recording; Escape stops; Backspace clears.
    h.click_id(Id::of("rec"));
    for down in [true, false] {
      h.event(Event::Modifier { key: Code::AltRight, down });
    }

    assert_eq!(h.focused(), Some(Id::of("rec")), "still recording");
    h.key(NamedKey::Escape, Modifiers::empty());
    assert_eq!((h.focused(), h.app().0.map(|k| k.to_string())), (None, Some("x".into())));
    h.click_id(Id::of("rec"));
    h.key(NamedKey::Backspace, Modifiers::empty());
    assert_eq!(h.app().0, None);
  }

  /// fn reaches the recorder as a modifier key of its own (key events don't carry it): held, it
  /// is the chord's modifier, but not with a key that carries it anyway, nor in a key recorder.
  #[test]
  fn fn_held_is_a_chords_modifier() {
    let mut h = Harness::new(Chords::default(), Size::new(300.0, 100.0), 1.0);
    h.frame();
    let mut record = |key: Key| {
      h.click_id(Id::of("rec"));
      h.event(Event::Modifier { key: Code::Fn, down: true });
      h.key(key, Modifiers::empty());
      h.event(Event::Modifier { key: Code::Fn, down: false });
      h.app().0.map(|k| k.to_string())
    };
    assert_eq!(record('1'.into()).as_deref(), Some("fn+1"));
    assert_eq!(record(NamedKey::F5.into()).as_deref(), Some("f5"));
    // Held already as the recording starts: still the chord's.
    h.event(Event::Modifier { key: Code::Fn, down: true });
    h.click_id(Id::of("rec"));
    h.key('2', Modifiers::empty());
    assert_eq!(h.app().0.map(|k| k.to_string()).as_deref(), Some("fn+2"));
    let mut h = Harness::new(Keys::default(), Size::new(300.0, 100.0), 1.0);
    h.frame();
    h.click_id(Id::of("rec"));
    h.event(Event::Modifier { key: Code::Fn, down: true });
    h.key('z', Modifiers::empty());
    assert_eq!(h.app().0.map(|k| k.to_string()).as_deref(), Some("z"));
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
    // fn is a chord's modifier (macOS): fn+1.
    assert_eq!(hk('1', Modifiers::FN).as_deref(), Some("fn+1"));
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
