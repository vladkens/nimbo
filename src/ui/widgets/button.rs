// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The button: a label over the button behaviour, drawn from the theme's variants and sizes.

use crate::scene::mix;
use crate::ui::theme::{ButtonStyle, ControlSize, Variant};
use crate::ui::{Response, Ui};

/// A button with a label.
#[derive(Clone, Copy, Debug)]
pub struct Button<'s> {
  label: &'s str,
  variant: Variant,
  size: ControlSize,
  selected: bool,
  style: Option<ButtonStyle>,
}

impl<'s> Button<'s> {
  pub fn new(label: &'s str) -> Self {
    let (variant, size) = (Variant::Normal, ControlSize::Medium);
    let (selected, style) = (false, None);
    Self { label, variant, size, selected, style }
  }

  pub fn variant(self, variant: Variant) -> Self {
    Self { variant, ..self }
  }

  pub fn primary(self) -> Self {
    self.variant(Variant::Primary)
  }

  pub fn ghost(self) -> Self {
    self.variant(Variant::Ghost)
  }

  pub fn danger(self) -> Self {
    self.variant(Variant::Danger)
  }

  pub fn size(self, size: ControlSize) -> Self {
    Self { size, ..self }
  }

  pub fn small(self) -> Self {
    self.size(ControlSize::Small)
  }

  pub fn large(self) -> Self {
    self.size(ControlSize::Large)
  }

  /// Shows it selected or toggled on ([crate::ui::Theme::selected]) when `on`: a sidebar's toggle,
  /// the current choice among buttons.
  pub fn selected(self, on: bool) -> Self {
    Self { selected: on, ..self }
  }

  /// Its own look instead of the variant's, usually derived: `t.button(Variant::Normal)
  /// .height(36.0)`.
  pub fn style(self, style: ButtonStyle) -> Self {
    Self { style: Some(style), ..self }
  }

  pub fn show(self, ui: &mut Ui) -> Response {
    let t = ui.theme();
    let s = self.style.unwrap_or_else(|| t.button_sized(self.variant, self.size));
    let s = if self.selected { t.selected(s) } else { s };
    let id = ui.take_id();
    let b = ui.button_behavior(id);
    // Ghost buttons are toolbar buttons, which macOS hovers; the others only with hover_all.
    let hot = match self.variant {
      Variant::Ghost => ui.hot(id, b.hovered),
      _ => ui.ctl_hot(id, b.hovered),
    };

    let bg = if b.pressed { s.pressed } else { mix(s.bg, s.hover, hot) };
    let frame = ui.control(s.frame.bg(bg), &b);
    let mut r = ui.with_id(id).row(frame, |ui| ui.text(self.label, s.text));
    r.clicked = b.clicked;
    // Disabled, it reads no input: the tree's hit is under it all the same.
    if b.disabled {
      (r.hovered, r.pressed, r.focused) = (false, false, false);
    }

    r
  }
}

impl Ui<'_> {
  /// A [Button] with the normal look.
  pub fn button(&mut self, label: &str) -> Response {
    Button::new(label).show(self)
  }
}

#[cfg(test)]
mod tests {
  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::Size;
  use crate::text::Font;
  use crate::ui::{Id, Response, Style, TextStyle, Ui, px};

  /// A disabled button, its last response kept.
  #[derive(Default)]
  struct Off(Option<Response>);

  impl App for Off {
    fn view(&mut self, ui: &mut Ui) {
      self.0 = Some(ui.disabled(true).id("off").button("Off"));
    }
  }

  #[test]
  fn a_disabled_button_is_never_hovered_or_pressed() {
    let mut h = Harness::new(Off::default(), Size::new(200.0, 100.0), 1.0);
    h.frame();
    let r = h.rt.ui.last.rect(Id::of("off")).unwrap();
    let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    h.move_to(x, y);
    h.press(x, y);
    h.frame();
    let r = h.app().0.unwrap();
    assert!(!r.hovered && !r.pressed && !r.clicked, "{r:?}");
  }

  /// A button after a note in a row `.0` wide.
  struct Beside(f32);

  impl App for Beside {
    fn view(&mut self, ui: &mut Ui) {
      ui.row(Style::new().w(px(self.0)), |ui| {
        ui.text("A note beside the button that wraps", TextStyle::new(Font::ui(13.0), [1.0; 4]));
        ui.id("b").button("Refresh");
      });
    }
  }

  #[test]
  fn a_button_keeps_its_width_beside_a_note() {
    let width = |row: f32| {
      let mut h = Harness::new(Beside(row), Size::new(400.0, 200.0), 1.0);
      h.frame();
      h.rt.ui.last.rect(Id::of("b")).unwrap().w
    };
    // The note wraps down to its widest letter; the button stays (its edges on whole points, so
    // within one where it moves).
    let (narrow, wide) = (width(120.0), width(400.0));
    assert!((narrow - wide).abs() <= 1.0, "{narrow} beside a wrapping note, {wide} alone");
  }
}
