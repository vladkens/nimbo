// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Headless controls: what a widget does (hover, press, click, focus, keys, the value it edits)
//! without how it looks, for an app that draws its own. Every styled widget and container
//! ([super::widgets], [super::containers]) is one of these plus drawing from the theme; the rule
//! for a new one is the same: its input in a `*_behavior` here, its look over what that returns
//! (a [Behavior] for most) beside the other looks.
//!
//! ```ignore
//! let b = ui.button_behavior(Id::of("play"));
//! let fill = if b.pressed { DOWN } else if b.hovered { OVER } else { REST };
//! ui.control(&b).canvas(Style::new().w(px(32.0)).h(px(32.0)), move |f, r| {
//!   f.quad(Quad::new(r, fill).radius(16.0));
//! });
//! if b.clicked { self.play() }
//! ```
//!
//! A behaviour reads the input for its id before the element is built, as of the last frame and
//! the input since ([super::Ui::hovered] and the like), applies it and returns a [Behavior]. The
//! app then builds the element as the control's root with [Ui::control]: under its id, taking
//! focus by Tab while enabled, as AppKit's controls do, and showing the pointing hand over it.
//! [super::Ui::disabled] before the call disables it: no input is read, no hand. Hover fades are
//! the look's (`Ui::hot`). What each one covers, each in a module of its own here:
//!
//! - [Ui::button_behavior]: click, Space while focused (flashes it pressed); the [Button] over it,
//!   drawn by a [ButtonStyle] ([Ui::button_style]);
//! - [Ui::toggle_behavior]: a button that flips a `bool`; [Ui::toggle] over it, drawn by a
//!   [ToggleStyle] ([Ui::toggle_style]);
//! - [Ui::slider_behavior]: a drag along the element onto the nearest step if it has one, the
//!   arrows move a step or a twentieth; the [Slider] over it, drawn by a [SliderStyle]
//!   ([Ui::slider_style]);
//! - [Ui::pick_behavior] (tabs, segmented control, toolbar tabs): one of `n` items by a click, the
//!   arrows step;
//! - [Ui::hotkey_recorder_behavior]: a recorder of a global hotkey ([crate::Hotkey]);
//! - [Ui::dialog_behavior]: a modal dialog's keys, focus trap and fade ([super::dialog]);
//! - [Ui::split_behavior]: a divider's drag within both panes' minimums;
//! - [Ui::scroll_behavior]: a scroll area's bars, a thumb dragged and a track clicked, shown while
//!   the content moves or the pointer comes in ([ScrollBehavior]); [Ui::scroll_area] draws them
//!   here, as the environment says ([ScrollerKnob]), since menus and lists scroll in it;
//! - [Ui::menu_popup] and [Ui::menu_item_behavior]: a menu's open state, placement, keys and items;
//! - [Ui::pop_up_behavior]: a pop-up button, opened by a click, Space, Enter or the arrows, letters
//!   typed choosing an option ([TypeSelect]);
//! - the [Picker] over one of them, drawn by a [PickerStyle] ([Ui::picker_style]) that chooses its
//!   input: the pop-up's and its menu, or the segments' ([PickerConfiguration]);
//! - [Ui::list_behavior] and [Ui::list_rows]: a virtualized list's selection ([Selection]), keys
//!   and presses, and its rows in view, which the app draws; the [List] over them, drawn by a
//!   [ListStyle] ([Ui::list_style]);
//! - [Ui::help]'s tooltip: when it shows and where, drawn by a [HelpStyle] ([Ui::help_style]);
//! - [Ui::text_field_behavior] and [Ui::selectable_behavior]: text editing on the editing model
//!   ([crate::Editor]), drawn by [Ui::field_text] ([FieldBehavior] has the details), with the edit
//!   menu ([Ui::edit_menu]); the [TextField] over it, drawn by a [TextFieldStyle]
//!   ([Ui::text_field_style]);
//! - [Ui::search_behavior]: a single-line field's, its text cleared by a ⊗ or Escape.
//!
//! The looks over them are the widgets' and the containers' ([super::widgets],
//! [super::containers]), which build on what is public here and in the core; but the scroll
//! area's, which is here.

mod button;
mod content;
mod dialog;
mod field;
mod help;
mod hotkey_recorder;
mod list;
mod list_style;
mod menu;
mod pick;
mod picker;
mod pop_up;
mod scroll;
mod search;
mod slider;
mod split;
mod text_field;
mod toggle;

pub use button::{Button, ButtonConfiguration, ButtonRole, ButtonStyle, PlainButtonStyle};
pub use content::{Build, Content};
pub use dialog::DialogBehavior;
#[cfg(feature = "styled")]
pub(crate) use field::DrawnEditMenu;
pub use field::{EditMenu, Editing, FieldBehavior, FieldKind, FieldStyle};
pub use help::{HelpConfiguration, HelpStyle, PlainHelpStyle};
pub use list::{ListBehavior, ListRows, SectionHeader, Selection};
pub use list_style::{
  List, ListConfiguration, ListLayout, ListResponse, ListRowConfiguration, ListStyle,
  PlainListStyle, Row,
};
pub use menu::{MenuPlacement, TYPE_SELECT, TypeSelect};
pub use picker::{InlinePickerStyle, Picker, PickerConfiguration, PickerStyle, Tag};
pub use scroll::{ScrollBar, ScrollBehavior, ScrollerKnob};
pub use slider::{PlainSliderStyle, Slider, SliderConfiguration, SliderStyle};
pub use split::SplitBehavior;
pub use text_field::{PlainTextFieldStyle, TextField, TextFieldConfiguration, TextFieldStyle};
pub use toggle::{PlainToggleStyle, ToggleConfiguration, ToggleStyle};

use super::{Cursor, Enabled, Id, Role, State, Style, Ui};
use crate::Modifiers;
use crate::keys::{Command, Key, Move};

/// What a control got this frame, read before it is built; see the module's docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
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
  /// The value it edits changed, written back already.
  pub changed: bool,
  /// Chosen or highlighted: the selected tab, a menu's highlighted item.
  pub on: bool,
}

impl Behavior {
  /// A part of a control that doesn't take focus itself (a tab, a menu's item): the pointing
  /// hand while enabled.
  pub const fn pointer(&self, s: Style) -> Style {
    if self.disabled { s } else { s.cursor(Cursor::Pointer) }
  }
}

impl Ui<'_> {
  /// Makes the next element control `b`'s root: under its id ([Ui::with_id]), taking focus by Tab
  /// ([Ui::focusable]) and showing the pointing hand ([Ui::pointer_style]) while enabled, unless
  /// the app set its own on the control; disabled, neither, whatever its style or the app asks, so
  /// Tab passes it by.
  pub fn control(&mut self, b: &Behavior) -> &mut Self {
    self.control_root(b.id, b.disabled, Cursor::Pointer)
  }

  /// [Ui::control] for control `id`, `cursor` over it while enabled (a text field's I-beam).
  fn control_root(&mut self, id: Id, disabled: bool, cursor: Cursor) -> &mut Self {
    let mods = self.take_kept_modifiers(id);
    let (focus, pointer) = match disabled {
      true => (Some(false), Some(Cursor::Inherit)),
      false => (self.next_focus.or(mods.0), self.next_cursor.or(mods.1)),
    };

    self.next_focus = focus.or(Some(true));
    self.next_cursor = pointer.or(Some(cursor));
    self.with_id(id)
  }

  /// Whether Space was pressed on `id` (focused), which flashes it pressed; another key goes on
  /// unused, Space with Cmd, Ctrl or Alt too (Alt-Space opens the window menu on Windows). A
  /// key something built before used (a dialog around it) stays used.
  fn activated(&mut self, id: Id) -> bool {
    match self.key_unused(id) {
      Some(k) if k.key == Key::Char(' ') && (k.mods - Modifiers::SHIFT).is_empty() => {
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

  /// The disabled flag and autofocus for `id`, recorded for assistive technology as `role`
  /// ([Ui::a11y]), and the focus and pointer the app set for its root ([Ui::focusable]); a
  /// disabled behaviour, or `None` to read input. Disabled or not focusable while focused, it
  /// gives focus up: its keys no longer route through it.
  fn begin(&mut self, id: Id, role: Role) -> Option<Behavior> {
    // Autofocus reads the disabled and focusable flags, so before they are taken.
    self.take_autofocus(id);
    let off = !self.environment_value::<Enabled>().0;
    let focusable = self.take_control_modifiers(id, off);
    self.a11y(id, role).state(State::Disabled, off);
    if (off || !focusable) && self.focused(id) {
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

  /// Gives `id` the key presses while it has focus before any binding of its own, of its
  /// ancestors or of the app ([crate::keys] has the order), this frame: a hotkey recorder
  /// recording, a modal dialog. A key it doesn't use goes on as usual.
  pub fn grab_keys(&mut self, id: Id) {
    self.st.grab = Some(id);
  }

  /// A button: hover, press, and a click or Space while focused.
  pub fn button_behavior(&mut self, id: Id) -> Behavior {
    if let Some(b) = self.begin(id, Role::Button) {
      return b;
    }

    let mut b = self.pointer_state(id);
    b.clicked |= self.activated(id);
    b
  }

  /// A step from the arrows while `id` has focus, as the platform's commands, along x and y on
  /// the screen: `[-1, 0]` for Left, `[1, 0]` Right, `[0, -1]` Up, `[0, 1]` Down; each caller
  /// says which way is forward (a tab list's Down, a slider's Up). Other commands go on unused.
  fn arrows(&mut self, id: Id) -> [i32; 2] {
    match self.command(id) {
      Some(Command::Move(Move::Left, _)) => [-1, 0],
      Some(Command::Move(Move::Right, _)) => [1, 0],
      Some(Command::Move(Move::Up, _)) => [0, -1],
      Some(Command::Move(Move::Down, _)) => [0, 1],
      Some(_) => {
        self.pass(id);
        [0, 0]
      }
      None => [0, 0],
    }
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::NamedKey;
  use crate::app::{App, Cx, Event};
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

      let s = Style::new().w(px(40.0)).h(px(40.0));
      ui.control(&b).canvas(s, move |f, r| f.quad(Quad::new(r, fill).radius(20.0)));
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

    // Hovered, held, clicked: the app's colours under the pointing hand, one play.
    h.move_to(20.0, 20.0);
    assert_eq!(px(&mut h), [0, 0, 255, 255]);
    assert_eq!(h.rt.cursor(), Cursor::Pointer);
    h.press(20.0, 20.0);
    assert_eq!(px(&mut h), [255, 0, 0, 255]);
    h.release(20.0, 20.0);
    assert_eq!(h.app().plays, 1);

    // Tab focuses it, Space plays and flashes it pressed.
    h.leave();
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focused(), Some(Id::of("play")));
    h.key(Key::Char(' '), Modifiers::empty());
    assert_eq!(h.app().plays, 2);
    assert_eq!(px(&mut h), [255, 0, 0, 255]);

    // Space again before the flash ends: pressed until the second one ends, then at rest.
    h.wait(Duration::from_millis(80));
    h.key(Key::Char(' '), Modifiers::empty());
    h.wait(Duration::from_millis(60));
    assert_eq!(px(&mut h), [255, 0, 0, 255]);
    h.wait(Duration::from_millis(100));
    assert_eq!((px(&mut h), h.app().plays), ([51, 51, 51, 255], 3));

    // Alt-Space is the window menu's on Windows: it goes on unused.
    assert!(!h.key(Key::Char(' '), Modifiers::ALT));
    assert_eq!(h.app().plays, 3);

    // Disabled while focused: focus goes, no input, no hand, and Tab passes it by.
    h.app_mut().off = true;
    h.rt.redraw();
    h.frame();
    assert_eq!(h.focused(), None);
    h.move_to(21.0, 20.0);
    assert_eq!(h.rt.cursor(), Cursor::Arrow);
    h.click(20.0, 20.0);
    assert_eq!(h.app().plays, 3);
    h.key(NamedKey::Tab, Modifiers::empty());
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
      ui.control(&b).el(Style::new().w(px(20.0)).h(px(20.0)));
    }
  }

  #[test]
  fn a_flash_keeps_the_apps_timer() {
    let mut h = Harness::new(Alarm, Size::new(100.0, 100.0), 1.0);
    h.frame();
    h.key(NamedKey::Tab, Modifiers::empty());
    h.key(Key::Char('t'), Modifiers::empty());
    h.key(Key::Char(' '), Modifiers::empty());
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
      ui.control(&a).el(sq.tab_focusable().cursor(Cursor::Pointer));
      // Built under its id without [Ui::control]: disabled all the same.
      let d = ui.disabled(true).button_behavior(Id::of("d"));
      ui.with_id(d.id).el(sq.tab_focusable().cursor(Cursor::Pointer));
      let c = ui.button_behavior(Id::of("c"));
      ui.control(&c).el(sq);
    }
  }

  /// Two controls whose behaviours both run before their roots are built, as a split's divider
  /// after a button in its first pane: each root keeps what the app set on it.
  struct Ahead;

  impl App for Ahead {
    fn view(&mut self, ui: &mut Ui) {
      let sq = Style::new().w(px(20.0)).h(px(20.0));
      let a = ui.focusable(false).button_behavior(Id::of("a"));
      let b = ui.pointer_style(Cursor::Crosshair).button_behavior(Id::of("b"));
      ui.row(Style::new(), |ui| {
        ui.control(&a).el(sq);
        ui.control(&b).el(sq);
      });
    }
  }

  #[test]
  fn controls_built_after_their_behaviours_keep_their_modifiers() {
    let mut h = Harness::new(Ahead, Size::new(100.0, 100.0), 1.0);
    h.frame();
    h.move_to(10.0, 10.0);
    assert_eq!(h.rt.cursor(), Cursor::Pointer);
    h.move_to(30.0, 10.0);
    assert_eq!(h.rt.cursor(), Cursor::Crosshair);
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focused(), Some(Id::of("b")));
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focused(), Some(Id::of("b")), "a is not in Tab order");
  }

  #[test]
  fn tab_passes_a_disabled_control_by_whatever_its_style() {
    let mut h = Harness::new(Pair, Size::new(100.0, 100.0), 1.0);
    h.frame();
    h.move_to(10.0, 10.0);
    assert_eq!(h.rt.cursor(), Cursor::Arrow);
    h.move_to(10.0, 30.0);
    assert_eq!(h.rt.cursor(), Cursor::Arrow);
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focused(), Some(Id::of("c")));
  }
}
