// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Headless controls: what a widget does (hover, press, click, focus, keys, the value it edits)
//! without how it looks, for an app that draws its own. Every styled widget ([super::widgets])
//! is one of these plus drawing from the theme; the rule for a new widget is the same: its input
//! in a `*_behavior` here, its look over the [Behavior] in the widget.
//!
//! ```ignore
//! let b = ui.button_behavior(Id::of("play"));
//! let fill = if b.pressed { DOWN } else if b.hovered { OVER } else { REST };
//! ui.with_id(b.id).canvas(b.style(Style::new().w(px(32.0)).h(px(32.0))), move |f, r| {
//!   f.quad(Quad::new(r, fill).radius(16.0));
//! });
//! if b.clicked { self.play() }
//! ```
//!
//! A behaviour reads the input for its id before the element is built, as of the last frame and
//! the input since ([super::Ui::hovered] and the like), applies it and returns a [Behavior]. The
//! app then builds the element under that id with [Behavior::style], which makes it take focus by
//! Tab while enabled, as AppKit's controls do, and show the pointing hand over it.
//! [super::Ui::disabled] before the call disables it: no input is read, no hand. Hover fades are
//! [super::Ui::hot]. What each one covers:
//!
//! - [Ui::button_behavior]: click, Space while focused (flashes it pressed).

use super::{Cursor, Focus, Id, Style, Ui};
use crate::keys::Key;

/// What a control got this frame, read before it is built; see the module's docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Behavior {
  pub id: Id,
  /// Disabled by [Ui::disabled]: no input was read and the rest is false.
  pub disabled: bool,
  pub hovered: bool,
  /// A button held on it with the pointer over it, or Space flashing it.
  pub pressed: bool,
  /// A button went down on it and is held, wherever the pointer is (drags).
  pub held: bool,
  /// Focused in the key window: when to draw a focus ring of its own.
  pub focused: bool,
  /// Clicked with the left button, or activated by Space.
  pub clicked: bool,
}

impl Behavior {
  /// The control's box: takes focus by Tab and shows the pointing hand while enabled; disabled,
  /// neither, whatever `s` asked for, so Tab passes it by.
  pub const fn style(&self, s: Style) -> Style {
    match self.disabled {
      true => Style { focus: Focus::None, cursor: Cursor::Inherit, ..s },
      false => s.tab_focusable().cursor(Cursor::Pointer),
    }
  }
}

impl Ui<'_> {
  // MARK: Keys

  /// Whether Space was pressed on `id` (focused), which flashes it pressed; another key goes on
  /// unused, Space with Cmd, Ctrl or Alt too (Alt-Space opens the window menu on Windows).
  pub(super) fn activated(&mut self, id: Id) -> bool {
    match self.key_down(id) {
      Some(k) if k.key == Key::Char(' ') && !(k.mods.cmd || k.mods.ctrl || k.mods.alt) => {
        self.flash(id);
        true
      }
      Some(_) => {
        self.pass(id);
        false
      }
      None => false,
    }
  }

  /// The disabled flag and autofocus for `id`; a disabled behaviour, or `None` to read input.
  /// Disabled while focused, it gives focus up: its keys no longer route through it.
  fn begin(&mut self, id: Id) -> Option<Behavior> {
    // Autofocus reads the disabled flag, so before it is taken.
    self.take_autofocus(id);
    let off = self.take_disabled();
    if off && self.focused(id) {
      self.focus(None);
    }

    off.then_some(Behavior { id, disabled: true, ..Behavior::default() })
  }

  /// Pointer state of `id`, without keys.
  fn pointer_state(&self, id: Id) -> Behavior {
    Behavior {
      id,
      hovered: self.hovered(id),
      pressed: self.pressed(id),
      held: self.held(id),
      focused: self.focus_visible(id),
      clicked: self.clicked(id),
      ..Behavior::default()
    }
  }

  // MARK: Behaviours

  /// A button: hover, press, and a click or Space while focused.
  pub fn button_behavior(&mut self, id: Id) -> Behavior {
    if let Some(b) = self.begin(id) {
      return b;
    }

    let mut b = self.pointer_state(id);
    b.clicked |= self.activated(id);
    b
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::app::{App, Cx, Event, Mods};
  use crate::harness::Harness;
  use crate::scene::{Color, Quad, Size};
  use crate::ui::px;

  const REST: Color = [0.2, 0.2, 0.2, 1.0];
  const OVER: Color = [0.0, 0.0, 1.0, 1.0];
  const DOWN: Color = [1.0, 0.0, 0.0, 1.0];

  /// A round button the app draws itself, on the button's behaviour.
  #[derive(Default)]
  struct Round {
    plays: usize,
    off: bool,
  }

  impl App for Round {
    fn view(&mut self, ui: &mut Ui) {
      let b = ui.disabled(self.off).button_behavior(Id::of("play"));
      let fill = match (b.pressed, b.hovered) {
        (true, _) => DOWN,
        (_, true) => OVER,
        _ => REST,
      };

      let s = b.style(Style::new().w(px(40.0)).h(px(40.0)));
      ui.with_id(b.id).canvas(s, move |f, r| f.quad(Quad::new(r, fill).radius(20.0)));
      if b.clicked {
        self.plays += 1;
      }
    }
  }

  #[test]
  fn a_custom_drawn_button_on_the_behaviour() {
    let mut h = Harness::new(Round::default(), Size::new(100.0, 100.0), 1.0);
    let px = |h: &mut Harness<Round>| h.render().pixel(20.0, 20.0);
    assert_eq!(px(&mut h), [51, 51, 51, 255]);
    // Hovered, held, clicked: the app's colours, one play.
    h.move_to(20.0, 20.0);
    assert_eq!(px(&mut h), [0, 0, 255, 255]);
    h.press(20.0, 20.0);
    assert_eq!(px(&mut h), [255, 0, 0, 255]);
    h.release(20.0, 20.0);
    assert_eq!(h.app().plays, 1);
    // Tab focuses it, Space plays and flashes it pressed.
    h.leave();
    h.key(Key::Tab, Mods::NONE);
    assert_eq!(h.focused(), Some(Id::of("play")));
    h.key(Key::Char(' '), Mods::NONE);
    assert_eq!(h.app().plays, 2);
    assert_eq!(px(&mut h), [255, 0, 0, 255]);
    // Space again before the flash ends: pressed until the second one ends, then at rest.
    h.wait(Duration::from_millis(80));
    h.key(Key::Char(' '), Mods::NONE);
    h.wait(Duration::from_millis(60));
    assert_eq!(px(&mut h), [255, 0, 0, 255]);
    h.wait(Duration::from_millis(100));
    assert_eq!((px(&mut h), h.app().plays), ([51, 51, 51, 255], 3));
    // Alt-Space is the window menu's on Windows: it goes on unused.
    assert!(!h.key(Key::Char(' '), Mods { alt: true, ..Mods::NONE }));
    assert_eq!(h.app().plays, 3);
    // Disabled while focused: focus goes, no input, and Tab passes it by.
    h.app_mut().off = true;
    h.rt.redraw();
    h.frame();
    assert_eq!(h.focused(), None);
    h.click(20.0, 20.0);
    assert_eq!(h.app().plays, 3);
    h.key(Key::Tab, Mods::NONE);
    assert_eq!(h.focused(), None);
  }

  /// A button, and a timer a second away that the app sets on T.
  struct Alarm;

  impl App for Alarm {
    fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
      let Event::KeyDown(k) = ev else { return false };
      if k.key != Key::Char('t') {
        return false;
      }

      cx.after(Duration::from_secs(1));
      true
    }

    fn view(&mut self, ui: &mut Ui) {
      let b = ui.button_behavior(Id::of("ok"));
      ui.with_id(b.id).el(b.style(Style::new().w(px(20.0)).h(px(20.0))));
    }
  }

  #[test]
  fn a_flash_keeps_the_apps_timer() {
    let mut h = Harness::new(Alarm, Size::new(100.0, 100.0), 1.0);
    h.frame();
    h.key(Key::Tab, Mods::NONE);
    h.key(Key::Char('t'), Mods::NONE);
    h.key(Key::Char(' '), Mods::NONE);
    // The flash's frame comes first, then the timer's, which it did not replace.
    assert_eq!(h.rt.due_in(), Some(Duration::from_millis(120)));
    h.wait(Duration::from_millis(200));
    assert_eq!(h.rt.due_in(), Some(Duration::from_millis(800)));
    assert!(h.wait(Duration::from_millis(800)) > 0);
    assert!(!h.rt.needs_frame() && h.rt.due_in().is_none());
  }

  /// A disabled control whose own style asks for focus, then an enabled one.
  struct Pair;

  impl App for Pair {
    fn view(&mut self, ui: &mut Ui) {
      let sq = Style::new().w(px(20.0)).h(px(20.0));
      let a = ui.disabled(true).button_behavior(Id::of("a"));
      ui.with_id(a.id).el(a.style(sq.tab_focusable().cursor(Cursor::Pointer)));
      let c = ui.button_behavior(Id::of("c"));
      ui.with_id(c.id).el(c.style(sq));
    }
  }

  #[test]
  fn tab_passes_a_disabled_control_by_whatever_its_style() {
    let mut h = Harness::new(Pair, Size::new(100.0, 100.0), 1.0);
    h.frame();
    h.move_to(10.0, 10.0);
    assert_eq!(h.rt.cursor(), Cursor::Arrow);
    h.key(Key::Tab, Mods::NONE);
    assert_eq!(h.focused(), Some(Id::of("c")));
  }
}
