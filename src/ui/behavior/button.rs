// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The button over its behaviour, drawn by the environment's [ButtonStyle], as SwiftUI's `Button`
//! and `.buttonStyle`: the style builds the control's root and its label from a
//! [ButtonConfiguration]; [PlainButtonStyle], the label alone, unless the shell put its own.
//!
//! ```ignore
//! struct Pill;
//!
//! impl ButtonStyle for Pill {
//!   fn make_body(&self, ui: &mut Ui, mut c: ButtonConfiguration) {
//!     let fill = if c.is_pressed() { DOWN } else { UP };
//!     ui.row(Style::new().pad_xy(12.0, 4.0).radius(12.0).bg(fill), |ui| c.label(ui, TEXT));
//!   }
//! }
//!
//! ui.button_style(&Pill).row(Style::new(), |ui| ui.button("Save"));
//! ```

use super::{Behavior, Content};
pub use crate::menu::ButtonRole;
use crate::text::Font;
use crate::ui::icon::Icon;
use crate::ui::{ControlSize, Dim, EnvironmentValue, Id, Response, Style, TextStyle, Ui, px};

/// How a button looks, as SwiftUI's `ButtonStyle`: [ButtonStyle::make_body] builds the button's
/// root, its first element, under its id, with the focus and the pointing hand the behaviour
/// gives it, and the label in it. Chosen with [Ui::button_style] as `&'static`, a unit struct's
/// `&MyStyle`.
pub trait ButtonStyle: 'static {
  /// Builds the button `config` describes.
  fn make_body(&self, ui: &mut Ui<'_>, config: ButtonConfiguration<'_>);
}

/// What a [ButtonStyle] draws: the button's state this frame and its label, built once.
#[non_exhaustive]
#[derive(Debug)]
pub struct ButtonConfiguration<'s> {
  b: Behavior,
  label: Content<'s>,
  icon: Option<Icon>,
  icon_only: bool,
  role: Option<ButtonRole>,
}

impl ButtonConfiguration<'_> {
  /// The button's id, its root's: for the style's hover fade.
  pub fn id(&self) -> Id {
    self.b.id
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

  /// What it does ([Button::role]).
  pub fn role(&self) -> Option<ButtonRole> {
    self.role
  }

  /// Its label is an icon alone: a square button, as an icon button's.
  pub fn is_icon_only(&self) -> bool {
    self.icon_only
  }

  /// Builds the label in `style`, once: an icon ([Button::icon]) before the text, as high as the
  /// font plus 2 in its colour, stroked 1.6 unless it has a width of its own; or what a closure
  /// builds in it.
  pub fn label(&mut self, ui: &mut Ui, style: TextStyle) {
    build_label(ui, self.icon.take(), std::mem::take(&mut self.label), style);
  }
}

/// Builds `label` in `style` after `first`, an icon, as [ButtonConfiguration::label] says.
pub(super) fn build_label(ui: &mut Ui, first: Option<Icon>, label: Content, style: TextStyle) {
  let icon = |ui: &mut Ui, i: Icon| {
    let (size, color) = ((style.font.size + 2.0).round(), style.color);
    let i = i.default_stroke(1.6);
    ui.canvas(Style::new().w(px(size)).h(px(size)), move |f, r| i.draw(f, r, color))
  };
  if let Some(i) = first {
    icon(ui, i);
  }

  match label {
    Content::None | Content::Text("") => {}
    Content::Text(s) => ui.text(s, style),
    Content::Icon(i) => _ = icon(ui, i),
    Content::Built(f) => f(ui, style),
  }
}

/// A button, as SwiftUI's `Button`: a label ([Content]: a text, an [Icon], what a closure
/// builds), drawn by the environment's [ButtonStyle] ([Ui::button_style]) at the control size
/// ([Ui::control_size]).
#[derive(Debug)]
pub struct Button<'s> {
  label: Content<'s>,
  icon: Option<Icon>,
  role: Option<ButtonRole>,
}

impl<'s> Button<'s> {
  pub fn new(label: impl Into<Content<'s>>) -> Self {
    Self { label: label.into(), icon: None, role: None }
  }

  /// An icon before the label, as SwiftUI's `Button(_:systemImage:)`; with no label, the icon
  /// alone, as `Button::new(icon)`.
  pub fn icon(self, icon: Icon) -> Self {
    Self { icon: Some(icon), ..self }
  }

  /// What it does, as SwiftUI's `Button(role:)`: the theme's styles draw a destructive one red.
  pub fn role(self, role: ButtonRole) -> Self {
    Self { role: Some(role), ..self }
  }

  /// Builds it under the id set for it: its behaviour, its name to assistive technology (an icon
  /// alone names it when nothing else does), then the style's body.
  pub fn show(self, ui: &mut Ui) -> Response {
    let style = ui.environment_value::<&dyn ButtonStyle>();
    let id = ui.take_id();
    let b = ui.button_behavior(id);
    let icon = match (&self.label, self.icon) {
      (Content::Icon(i), None) => Some(*i),
      (l, Some(i)) if l.is_empty() => Some(i),
      _ => None,
    };

    if let Some((a, n)) = ui.a11y_of(id).zip(icon.and_then(Icon::name)) {
      a.name_or(n);
    }

    let (label, role) = (self.label, self.role);
    let config = ButtonConfiguration { b, label, icon: self.icon, icon_only: icon.is_some(), role };
    let root = ui.st.tree.nodes.len();
    style.make_body(ui.control(&b), config);
    debug_assert!(
      ui.st.tree.nodes.get(root).is_some_and(|n| n.id == id),
      "a button style builds the button's root first"
    );

    let mut r = ui.st.response(id);
    r.clicked = b.clicked;
    // Disabled, it reads no input: the tree's hit is under it all the same.
    if b.disabled {
      (r.hovered, r.pressed, r.focused) = (false, false, false);
    }

    r
  }
}

/// The label alone in the text colour of the colour scheme, as SwiftUI's `.plain`: fainter while
/// held, dimmed when disabled. The default under the shell's.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlainButtonStyle;

impl ButtonStyle for PlainButtonStyle {
  fn make_body(&self, ui: &mut Ui<'_>, mut c: ButtonConfiguration<'_>) {
    let s = plain_label(ui, c.is_enabled(), c.is_pressed());
    // Rigid, as every control: the text beside it wraps, the label never does.
    ui.row(Style::new().w(Dim::Rigid).gap(5.0).center(), |ui| c.label(ui, s));
  }
}

/// The plain styles' text: the system font at the control size ([Ui::control_size]) in black or
/// white of the colour scheme at opacity `a`.
pub(super) fn plain_text(ui: &Ui, a: f32) -> TextStyle {
  let ink = if ui.color_scheme().is_dark() { 1.0 } else { 0.0 };
  let size = match ui.environment_value::<ControlSize>() {
    ControlSize::Small => 11.0,
    ControlSize::Regular => 13.0,
    ControlSize::Large => 15.0,
  };

  TextStyle::new(Font::ui(size), [ink, ink, ink, a])
}

/// The plain styles' label: one line of [plain_text], fainter while `pressed`, dimmed unless
/// `enabled`.
pub(super) fn plain_label(ui: &Ui, enabled: bool, pressed: bool) -> TextStyle {
  // macOS's label colours: 85% of black or white, its secondary's 50%, its tertiary's 25%.
  let a = match (enabled, pressed) {
    (false, _) => 0.25,
    (true, true) => 0.5,
    (true, false) => 0.85,
  };

  plain_text(ui, a).line_limit(1)
}

/// [PlainButtonStyle] where neither a container nor the window nor the shell put one.
impl EnvironmentValue for &'static dyn ButtonStyle {
  fn default_value(_: &Ui) -> Self {
    &PlainButtonStyle
  }
}

impl Ui<'_> {
  /// A [Button] showing `label`.
  pub fn button<'s>(&mut self, label: impl Into<Content<'s>>) -> Response {
    Button::new(label).show(self)
  }

  /// Draws the next button, or every button built in the next container, in `style`, as
  /// SwiftUI's `.buttonStyle`: `ui.button_style(&BorderedProminentButtonStyle).button("Save")`.
  pub fn button_style(&mut self, style: &'static dyn ButtonStyle) -> &mut Self {
    self.environment(style)
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::app::App;
  use crate::keys::Key;
  use crate::scene::{Quad, Size};
  use crate::ui::{Cursor, UiState};
  use crate::{Harness, Modifiers, NamedKey};

  const UP: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
  const OVER: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
  const DOWN: [f32; 4] = [1.0; 4];

  /// A square of the app's: blue, green hovered, white held, red when disabled; it never sets
  /// focus or a pointer.
  struct Square;

  impl ButtonStyle for Square {
    fn make_body(&self, ui: &mut Ui<'_>, c: ButtonConfiguration<'_>) {
      let fill = match (c.is_enabled(), c.is_pressed(), c.is_hovered()) {
        (false, ..) => [1.0, 0.0, 0.0, 1.0],
        (_, true, _) => DOWN,
        (_, _, true) => OVER,
        _ => UP,
      };

      ui.canvas(Style::new().w(px(40.0)).h(px(40.0)), move |f, r| f.quad(Quad::new(r, fill)));
    }
  }

  #[derive(Default)]
  struct Squares {
    off: bool,
    clicks: usize,
  }

  impl App for Squares {
    fn view(&mut self, ui: &mut Ui) {
      let ui = ui.button_style(&Square).disabled(self.off);
      self.clicks += ui.id("sq").button("Square").clicked as usize;
    }
  }

  #[test]
  fn a_style_draws_over_the_behaviour() {
    let mut h = Harness::new(Squares::default(), Size::new(100.0, 100.0), 1.0);
    let px = |h: &mut Harness<Squares>| h.render().pixel(20.0, 20.0);
    let rgb = |c: [f32; 4]| c.map(|v| (v * 255.0) as u8);
    assert_eq!(px(&mut h), rgb(UP));
    assert_eq!(h.cursor(), Cursor::Arrow);

    // Hovered under the hand, held, clicked; focused by Tab and pressed by Space.
    h.move_to(20.0, 20.0);
    assert_eq!((px(&mut h), h.cursor()), (rgb(OVER), Cursor::Pointer));
    h.press(20.0, 20.0);
    assert_eq!(px(&mut h), rgb(DOWN));
    h.release(20.0, 20.0);
    assert_eq!(h.app().clicks, 1);
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focus_id(), Some(Id::of("sq")));
    h.key(Key::Char(' '), Modifiers::empty());
    assert_eq!(h.app().clicks, 2);
    assert!(h.a11y().contains("button"), "{}", h.a11y());

    // Disabled: its own red, no hand, no focus, no clicks.
    h.app_mut().off = true;
    h.redraw();
    assert_eq!(px(&mut h), [255, 0, 0, 255]);
    assert_eq!(h.focus_id(), None);
    h.move_to(21.0, 20.0);
    assert_eq!(h.cursor(), Cursor::Arrow);
    h.click(20.0, 20.0);
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!((h.focus_id(), h.app().clicks), (None, 2));
  }

  /// Buttons in a container given the square style, and one after it.
  struct Cascade;

  impl App for Cascade {
    fn view(&mut self, ui: &mut Ui) {
      ui.button_style(&Square).col(Style::new(), |ui| {
        ui.id("a").button("A");
        ui.row(Style::new(), |ui| _ = ui.id("b").button("B"));
      });
      ui.id("c").button("C");
    }
  }

  #[test]
  fn a_containers_style_reaches_its_buttons_only() {
    let mut h = Harness::new(Cascade, Size::new(200.0, 200.0), 1.0);
    h.frame();
    let side = |n: &str| h.rect(Id::of(n)).map(|r| (r.w, r.h)).unwrap();
    assert_eq!((side("a"), side("b")), ((40.0, 40.0), (40.0, 40.0)));
    assert_ne!(side("c"), (40.0, 40.0));
  }

  /// With no shell, plain: the label alone, no fill.
  #[test]
  fn plain_is_the_default_without_the_shell() {
    let mut ui = Ui::new(UiState::default(), Size::new(100.0, 100.0), None, Duration::ZERO);
    ui.id("b").button("B");
    let n = ui.st.tree.nodes.iter().find(|n| n.id == Id::of("b")).unwrap();
    assert_eq!((n.style.bg, n.style.size[1]), ([0.0; 4], Style::new().size[1]));
  }

  /// A label with a tip of its own, and a plain button beside a note wrapping in `w`.
  #[derive(Default)]
  struct Tipped {
    clicks: usize,
    w: f32,
  }

  impl App for Tipped {
    fn view(&mut self, ui: &mut Ui) {
      let label = |ui: &mut Ui, s: TextStyle| ui.help("Tip").text("Tipped", s);
      self.clicks += ui.id("t").button(label).clicked as usize;
      ui.row(Style::new().w(px(self.w)), |ui| {
        ui.id("p").button("Refresh settings");
        ui.text("A note beside it, long enough to wrap", TextStyle::new(Font::ui(13.0), [0.0; 4]));
      });
      let s = TextStyle::new(Font::ui(13.0), [0.0; 4]);
      ui.id("solo")
        .allows_hit_testing(false)
        .row(Style::new(), |ui| ui.help("Tip").text("Solo", s));
    }
  }

  /// A press on a part shown only for its tip is the button's, and the plain button keeps its
  /// width beside a note that wraps.
  #[test]
  fn a_tip_in_the_label_and_a_note_beside_leave_the_button_whole() {
    let mut h =
      Harness::new(Tipped { w: 600.0, ..Tipped::default() }, Size::new(600.0, 200.0), 1.0);
    h.frame();
    let r = h.rect(Id::of("t")).unwrap();
    h.move_to(r.x + r.w / 2.0, r.y + r.h / 2.0);
    assert_eq!(h.rt.ui.hover().len(), 2, "the label's text, then the button");
    h.click(r.x + r.w / 2.0, r.y + r.h / 2.0);
    assert_eq!(h.app().clicks, 1);
    assert!(h.a11y().lines().any(|l| l.trim().starts_with("button \"Tipped\"")), "{}", h.a11y());

    // With nothing around, the press is its own.
    let r = h.rect(Id::of("solo")).unwrap();
    h.press(r.x + r.w / 2.0, r.y + r.h / 2.0);
    assert!(h.rt.ui.press().is_some());
    h.release(r.x + r.w / 2.0, r.y + r.h / 2.0);

    let wide = h.rect(Id::of("p")).unwrap().w;
    h.app_mut().w = 120.0;
    h.redraw();
    h.frame();
    assert_eq!(h.rect(Id::of("p")).unwrap().w, wide);
  }

  /// An icon alone names its button.
  struct Gear;

  impl App for Gear {
    fn view(&mut self, ui: &mut Ui) {
      ui.button(Icon::GEAR);
      Button::new("").icon(Icon::RESET).show(ui);
      Button::new("Open").icon(Icon::FOLDER).show(ui);
    }
  }

  #[test]
  fn an_icon_alone_names_its_button() {
    let mut h = Harness::new(Gear, Size::new(200.0, 100.0), 1.0);
    h.frame();
    let a = h.a11y();
    for name in ["button \"Settings\"", "button \"Reset\"", "button \"Open\""] {
      assert!(a.lines().any(|l| l.trim() == name), "{name} in\n{a}");
    }
  }
}
