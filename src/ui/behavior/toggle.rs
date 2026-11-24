// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The toggle over its behaviour ([Ui::toggle_behavior]: a button that flips a `bool`), drawn by
//! the environment's [ToggleStyle], as SwiftUI's `Toggle` and `.toggleStyle`: the style builds
//! the control's root and its label from a [ToggleConfiguration]; [PlainToggleStyle], a box
//! before the label, unless the shell put its own.
//!
//! ```ignore
//! struct Dot;
//!
//! impl ToggleStyle for Dot {
//!   fn make_body(&self, ui: &mut Ui, mut c: ToggleConfiguration) {
//!     let fill = if c.is_on() { ON } else { OFF };
//!     ui.row(Style::new().gap(6.0).center(), |ui| {
//!       ui.el(Style::new().w(px(10.0)).h(px(10.0)).radius(5.0).bg(fill));
//!       c.label(ui, TEXT);
//!     });
//!   }
//! }
//!
//! ui.toggle_style(&Dot).col(Style::new(), |ui| ui.toggle(&mut self.live, "Live"));
//! ```

use super::button::{build_label, plain_label};
use super::{Behavior, Content};
use crate::ui::{Dim, EnvironmentValue, Id, Response, Role, Style, TextStyle, Ui, Value, px};

impl Ui<'_> {
  /// A check box or switch over `on`, flipped by a click or Space while focused: `changed` when
  /// it was, `on` its value after. A check box on or off to assistive technology; a switch's look
  /// refines it.
  pub fn toggle_behavior(&mut self, id: Id, on: &mut bool) -> Behavior {
    let mut b = self.button_behavior(id);
    if b.clicked {
      *on = !*on;
    }

    (b.changed, b.on) = (b.clicked, *on);
    self.a11y(id, Role::CheckBox).value(if *on { Value::On } else { Value::Off });
    b
  }
}

/// How a toggle looks, as SwiftUI's `ToggleStyle`: [ToggleStyle::make_body] builds the toggle's
/// root first, as a button style builds a button's ([crate::ButtonStyle]). Chosen with
/// [Ui::toggle_style] as `&'static`, a unit struct's `&MyStyle`.
pub trait ToggleStyle: 'static {
  /// Builds the toggle `config` describes.
  fn make_body(&self, ui: &mut Ui<'_>, config: ToggleConfiguration<'_>);
}

/// What a [ToggleStyle] draws: the toggle's state this frame and its label, built once.
#[non_exhaustive]
#[derive(Debug)]
pub struct ToggleConfiguration<'s> {
  b: Behavior,
  label: Content<'s>,
  icon_only: bool,
}

impl ToggleConfiguration<'_> {
  /// The toggle's id, its root's: for the style's hover fade and slide.
  pub fn id(&self) -> Id {
    self.b.id
  }

  /// On, the value after this frame's click.
  pub fn is_on(&self) -> bool {
    self.b.on
  }

  /// Held with the pointer over it, or Space flashing it.
  pub fn is_pressed(&self) -> bool {
    self.b.pressed
  }

  pub fn is_hovered(&self) -> bool {
    self.b.hovered
  }

  /// Focused by the keyboard in the key window: the core rings its root.
  pub fn is_focused(&self) -> bool {
    self.b.focused
  }

  /// Not disabled ([Ui::disabled]): it reads input, and is drawn so.
  pub fn is_enabled(&self) -> bool {
    !self.b.disabled
  }

  /// Its label is an icon alone: a square button in [ButtonToggleStyle](crate::ButtonToggleStyle).
  pub fn is_icon_only(&self) -> bool {
    self.icon_only
  }

  /// Builds the label in `style`, once, as a button's ([crate::ButtonConfiguration::label]).
  pub fn label(&mut self, ui: &mut Ui, style: TextStyle) {
    build_label(ui, None, std::mem::take(&mut self.label), style);
  }
}

/// A box ringed in the plain button's colour ([crate::PlainButtonStyle]), filled while on,
/// before the label in it. The default under the shell's.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlainToggleStyle;

impl ToggleStyle for PlainToggleStyle {
  fn make_body(&self, ui: &mut Ui<'_>, mut c: ToggleConfiguration<'_>) {
    let s = plain_label(ui, c.is_enabled(), c.is_pressed());
    let d = (s.font.size * 0.9).round();
    let mark = Style::new().w(px(d)).h(px(d)).radius(2.0).border(1.0, s.color);
    let mark = if c.is_on() { mark.bg(s.color) } else { mark };
    // Rigid, as every control: the text beside it wraps, the label never does.
    ui.row(Style::new().w(Dim::Rigid).gap(5.0).center(), |ui| {
      ui.el(mark);
      c.label(ui, s);
    });
  }
}

/// [PlainToggleStyle] where neither a container nor the window nor the shell put one.
impl EnvironmentValue for &'static dyn ToggleStyle {
  fn default_value(_: &Ui) -> Self {
    &PlainToggleStyle
  }
}

impl Ui<'_> {
  /// A toggle over `on` showing `label` ([Content]), as SwiftUI's `Toggle`: flipped by a click or
  /// Space while focused, `changed` when it was; drawn by the environment's [ToggleStyle]
  /// ([Ui::toggle_style]) at the control size. An icon alone names it when nothing else does;
  /// `()` shows none, as `Toggle(isOn:) {}`: a form row's switch, named by its row
  /// (`ui.form_row(&fs, row, |ui| ui.toggle(&mut on, ()))`).
  pub fn toggle<'s>(&mut self, on: &mut bool, label: impl Into<Content<'s>>) -> Response {
    let style = self.environment_value::<&dyn ToggleStyle>();
    let id = self.take_id();
    let b = self.toggle_behavior(id, on);
    let label = label.into();
    let icon = if let Content::Icon(i) = label { i.name() } else { None };
    if let Some((a, n)) = self.a11y_of(id).zip(icon) {
      a.name_or(n);
    }

    let icon_only = matches!(label, Content::Icon(_));
    let root = self.st.tree.nodes.len();
    style.make_body(self.control(&b), ToggleConfiguration { b, label, icon_only });
    debug_assert!(
      self.st.tree.nodes.get(root).is_some_and(|n| n.id == id),
      "a toggle style builds the toggle's root first"
    );

    let mut r = self.st.response(id);
    (r.clicked, r.changed) = (b.clicked, b.changed);
    // Disabled, it reads no input: the tree's hit is under it all the same.
    if b.disabled {
      (r.hovered, r.pressed, r.focused) = (false, false, false);
    }

    r
  }

  /// Draws the next toggle, or every toggle built in the next container, in `style`, as
  /// SwiftUI's `.toggleStyle`: `ui.toggle_style(&CheckboxToggleStyle).toggle(&mut on, "Wrap")`.
  pub fn toggle_style(&mut self, style: &'static dyn ToggleStyle) -> &mut Self {
    self.environment(style)
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::app::App;
  use crate::harness::Harness;
  use crate::keys::Key;
  use crate::scene::{Quad, Size};
  use crate::ui::{Cursor, Icon, UiState};
  use crate::{Modifiers, NamedKey};

  const OFF: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
  const ON: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

  /// A square of the app's: blue off, green on, white held, red when disabled; it never sets
  /// focus or a pointer.
  struct Square;

  impl ToggleStyle for Square {
    fn make_body(&self, ui: &mut Ui<'_>, c: ToggleConfiguration<'_>) {
      let fill = match (c.is_enabled(), c.is_pressed(), c.is_on()) {
        (false, ..) => [1.0, 0.0, 0.0, 1.0],
        (_, true, _) => [1.0; 4],
        (_, _, true) => ON,
        _ => OFF,
      };

      ui.canvas(Style::new().w(px(40.0)).h(px(40.0)), move |f, r| f.quad(Quad::new(r, fill)));
    }
  }

  #[derive(Default)]
  struct Squares {
    disabled: bool,
    on: bool,
    flips: usize,
  }

  impl App for Squares {
    fn view(&mut self, ui: &mut Ui) {
      let ui = ui.toggle_style(&Square).disabled(self.disabled);
      self.flips += ui.id("sq").toggle(&mut self.on, "Square").changed as usize;
    }
  }

  #[test]
  fn a_style_draws_over_the_behaviour() {
    let mut h = Harness::new(Squares::default(), Size::new(100.0, 100.0), 1.0);
    let px = |h: &mut Harness<Squares>| h.render().pixel(20.0, 20.0);
    let rgb = |c: [f32; 4]| c.map(|v| (v * 255.0) as u8);
    assert_eq!(px(&mut h), rgb(OFF));
    assert_eq!(h.rt.cursor(), Cursor::Arrow);

    // Under the hand, held, flipped on; focused by Tab and flipped off by Space.
    h.move_to(20.0, 20.0);
    assert_eq!(h.rt.cursor(), Cursor::Pointer);
    h.press(20.0, 20.0);
    assert_eq!(px(&mut h), [255; 4]);
    h.release(20.0, 20.0);
    assert_eq!((px(&mut h), h.app().on, h.app().flips), (rgb(ON), true, 1));
    assert!(h.a11y().contains("check box on"), "{}", h.a11y());

    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focused(), Some(Id::of("sq")));
    h.key(Key::Char(' '), Modifiers::empty());
    assert_eq!((h.app().on, h.app().flips), (false, 2));
    h.frame();
    assert!(h.a11y().contains("check box off"), "{}", h.a11y());

    // Disabled: its own red, no hand, no focus, no flips.
    h.app_mut().disabled = true;
    h.rt.redraw();
    assert_eq!(px(&mut h), [255, 0, 0, 255]);
    assert_eq!(h.focused(), None);
    h.move_to(21.0, 20.0);
    assert_eq!(h.rt.cursor(), Cursor::Arrow);
    h.click(20.0, 20.0);
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!((h.focused(), h.app().on, h.app().flips), (None, false, 2));
  }

  /// Toggles in a container given the square style, and one after it.
  struct Cascade;

  impl App for Cascade {
    fn view(&mut self, ui: &mut Ui) {
      ui.toggle_style(&Square).col(Style::new(), |ui| {
        ui.id("a").toggle(&mut false, "A");
        ui.row(Style::new(), |ui| _ = ui.id("b").toggle(&mut false, "B"));
      });
      ui.id("c").toggle(&mut false, "C");
    }
  }

  #[test]
  fn a_containers_style_reaches_its_toggles_only() {
    let mut h = Harness::new(Cascade, Size::new(200.0, 200.0), 1.0);
    h.frame();
    let side = |n: &str| h.rect(Id::of(n)).map(|r| (r.w, r.h)).unwrap();
    assert_eq!((side("a"), side("b")), ((40.0, 40.0), (40.0, 40.0)));
    assert_ne!(side("c"), (40.0, 40.0));
  }

  /// With no shell, plain: a box filled while on, before the label.
  #[test]
  fn plain_is_the_default_without_the_shell() {
    let mut ui = Ui::new(UiState::default(), Size::new(100.0, 100.0), None, Duration::ZERO);
    ui.id("t").toggle(&mut true, "T");
    let at = ui.st.tree.nodes.iter().position(|n| n.id == Id::of("t")).unwrap();
    let (root, mark) = (&ui.st.tree.nodes[at], &ui.st.tree.nodes[at + 1]);
    assert_eq!(root.style.bg, [0.0; 4]);
    assert!(mark.style.bg[3] > 0.0 && mark.style.size[0] == px(12.0), "{:?}", mark.style);
  }

  /// An icon alone names its toggle.
  struct Bell;

  impl App for Bell {
    fn view(&mut self, ui: &mut Ui) {
      ui.toggle(&mut true, Icon::GEAR);
    }
  }

  #[test]
  fn an_icon_alone_names_its_toggle() {
    let mut h = Harness::new(Bell, Size::new(200.0, 100.0), 1.0);
    h.frame();
    let a = h.a11y();
    assert!(a.lines().any(|l| l.trim() == "switch \"Settings\" on"), "{a}");
  }
}
