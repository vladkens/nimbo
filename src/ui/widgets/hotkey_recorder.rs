// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The hotkey recorder: a capsule showing a global [Hotkey] that records a new one, a chord
//! or a modifier key alone where the platform can grab one ([crate::hotkey::modifier_alone]).
//!
//! ```ignore
//! if ui.id("hotkey").hotkey_recorder(&mut self.hotkey).changed { self.save() }
//! let text = self.hotkey.map(|h| h.to_string()); // "ctrl+option+space", "right_option"
//! ```
//!
//! The hotkey is system-wide ([Hotkey::parse]): its modifiers are the keys' own (`cmd` is ⌘,
//! the Windows or the Super key) and its key a place on the keyboard, so it reads back as typed
//! on any layout. It shows as the platform writes it ([Hotkey::label]): `⌃⌥Space`, `Right ⌥` on
//! macOS, `Ctrl+Alt+Space`, `Right Alt` elsewhere.

use crate::hotkey;
use crate::keys::Hotkey;
use crate::scene::mix;
use crate::text::Weight;
use crate::ui::icon::{self, Icon};
use crate::ui::{Cursor, Float, Response, Role, Style, Ui, UnitPoint, px};

/// The recorder's width: the same recording, set or empty.
const W: f32 = 160.0;

impl Ui<'_> {
  /// A hotkey recorder over `value` ([Ui::hotkey_recorder_behavior]; a modifier key alone where
  /// [crate::hotkey::modifier_alone]), 160 wide whatever it shows: a capsule with the hotkey
  /// ([Hotkey::label]), "Record Shortcut" when empty, and a ⊗ that clears it (`id.key("clear")`)
  /// where the theme has one ([crate::Theme::hotkey_recorder_clear]: not the plates themes, whose
  /// recorder sits by a reset button); recording, the ground with an accent edge and the focus
  /// ring, a red dot and "Type a shortcut…". `changed` when a hotkey was recorded or the value
  /// cleared.
  pub fn hotkey_recorder(&mut self, value: &mut Option<Hotkey>) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let clear = id.key("clear");
    let lone = hotkey::modifier_alone();
    let b = self.hotkey_recorder_behavior(id, value, t.hotkey_recorder_clear, lone);
    let hot = self.ctl_hot(id, b.hovered);
    let h = t.sizes.control_h;
    let bg = match (b.on, b.pressed) {
      (true, _) => t.bg,
      (_, true) => t.pressed,
      _ => mix(t.raised, t.hover, hot),
    };

    let x = if t.hotkey_recorder_clear { 22.0 } else { 10.0 };
    let frame = Style::new().w(px(W)).h(px(h)).pad_xy(x, 0.0).gap(6.0).center().radius(h / 2.0);
    let frame = frame.bg(bg).border(if b.on { 1.0 } else { 0.0 }, t.accent);
    let frame = self.dim_style(frame, b.disabled);
    let label = t.label();
    let mut r = self.control(&b).row(frame, |ui| {
      match (b.on, *value) {
        (true, _) => {
          ui.el(Style::new().w(px(7.0)).h(px(7.0)).radius(3.5).bg(t.danger));
          ui.text("Type a shortcut…", label.color(t.muted));
        }
        (_, None) => ui.text("Record Shortcut", label.color(t.muted)),
        (_, Some(v)) => ui.text(v.label(), label.font(t.font.weight(Weight::Medium))),
      }

      if value.is_some() && !b.on && !b.disabled && t.hotkey_recorder_clear {
        let at = Float::parent()
          .attach(UnitPoint::TRAILING)
          .anchor(UnitPoint::TRAILING)
          .offset(-5.0, 0.0)
          .clip();
        let s = Style::new().w(px(14.0)).h(px(14.0)).cursor(Cursor::Pointer);
        ui.a11y(clear, Role::Button).name("Clear");
        let hot = ui.ctl_hot(clear, ui.hovered(clear));
        let mark = mix(t.faint, t.muted, hot);
        ui.with_id(clear).float(at).canvas(s, move |f, r| icon::draw(f, Icon::CLEAR, r, mark));
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
  use crate::harness::Harness;
  use crate::keys::{Binding, CMD, Command, Key, Shortcut, bind};
  use crate::scene::Size;
  use crate::ui::Id;

  /// A recorder in an app that binds Cmd-Z.
  #[derive(Default)]
  struct Undo {
    value: Option<Hotkey>,
    undos: usize,
  }

  impl App for Undo {
    fn keymap(&self) -> &[Binding] {
      const KEYS: &[Binding] = &[bind(Shortcut::char('z').cmd(), Command::Action("undo"))];
      KEYS
    }

    fn event(&mut self, _: &mut Cx, ev: &Event) -> bool {
      let undo = matches!(ev, Event::Command(Command::Action("undo")));
      self.undos += undo as usize;
      undo
    }

    fn view(&mut self, ui: &mut Ui) {
      ui.id("rec").hotkey_recorder(&mut self.value);
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
}
