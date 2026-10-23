// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The button: a label or an icon over the button behaviour, drawn from the theme's
//! variants and sizes; the ⓘ that explains a row and the round "?" that opens a page's help.

use super::icon::{self, Icon};
use crate::scene::mix;
use crate::ui::theme::{ButtonStyle, ControlSize, Variant};
use crate::ui::{Response, Style, TextStyle, Ui, px};

/// A button with a label, an [Icon], or both.
#[derive(Clone, Copy, Debug)]
pub struct Button<'s> {
  label: &'s str,
  icon: Option<Icon>,
  variant: Variant,
  size: ControlSize,
  selected: bool,
  style: Option<ButtonStyle>,
}

impl<'s> Button<'s> {
  pub fn new(label: &'s str) -> Self {
    let (variant, size) = (Variant::Normal, ControlSize::Medium);
    let (selected, style) = (false, None);
    Self { label, icon: None, variant, size, selected, style }
  }

  /// A square ghost button showing an icon, muted until the pointer is on it: reset,
  /// delete, open elsewhere.
  pub fn icon(icon: Icon) -> Self {
    Self { icon: Some(icon), variant: Variant::Ghost, ..Self::new("") }
  }

  /// An icon before the label.
  pub fn with_icon(self, icon: Icon) -> Self {
    Self { icon: Some(icon), ..self }
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
    let square = self.label.is_empty() && self.icon.is_some();
    let s = self.style.unwrap_or_else(|| {
      let s = t.button_sized(self.variant, self.size);
      let h = s.frame.min[0];
      if square { ButtonStyle { frame: s.frame.w(px(h)).pad(0.0), ..s } } else { s }
    });
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
    // An icon button's mark is muted until the pointer is on it.
    let ink = match square && self.style.is_none() && !self.selected {
      true => mix(t.muted, t.text, if b.pressed { 1.0 } else { hot }),
      false => s.text.color,
    };
    // Canvases draw past the frame's opacity: the icon dims itself.
    let ink = ui.dim(ink, b.disabled);

    let size = (s.text.font.size + 2.0).round();
    let mut r = ui.with_id(id).row(frame, |ui| {
      if let Some(i) = self.icon {
        ui.canvas(Style::new().w(px(size)).h(px(size)), move |f, r| icon::draw(f, i, r, ink, 1.6));
      }

      if !self.label.is_empty() {
        ui.text(self.label, s.text);
      }
    });
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

  /// A square ghost button showing an icon ([Button::icon]).
  pub fn icon_button(&mut self, icon: Icon) -> Response {
    Button::icon(icon).show(self)
  }

  /// An ⓘ, 16 points, muted, in the label colour while held, halfway there under the pointer
  /// where the theme hovers controls ([crate::Theme::hover_all]): with a [Ui::tooltip], an
  /// explanation longer than a line. A button by its behaviour.
  pub fn info_button(&mut self) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let b = self.button_behavior(id);
    let hot = self.ctl_hot(id, b.hovered);
    let size = (t.font.size + 3.0).round();
    let k = if b.pressed { 1.0 } else { 0.5 * hot };
    let color = self.dim(mix(t.muted, t.text, k), b.disabled);
    let s = self.control(Style::new().w(px(size)).h(px(size)).radius(size / 2.0), &b);
    let mut r = self.with_id(id).canvas(s, move |f, r| icon::draw(f, Icon::INFO, r, color, 1.5));
    r.clicked = b.clicked;
    r
  }

  /// The round "?" at the bottom of a settings page: opens its help. As high as a push button
  /// ([crate::Sizes::control_h]), raised, pressed as one.
  pub fn help_button(&mut self) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let b = self.button_behavior(id);
    let hot = self.ctl_hot(id, b.hovered);
    let d = t.sizes.control_h;
    let bg = if b.pressed { t.pressed } else { mix(t.raised, t.hover, hot) };
    let s = self.control(Style::new().w(px(d)).h(px(d)).center().radius(d / 2.0).bg(bg), &b);
    let text = TextStyle::new(t.bold, t.text).nowrap();
    let mut r = self.with_id(id).row(s, |ui| ui.text("?", text));
    r.clicked = b.clicked;
    r
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

  /// An icon button, disabled or not, in the light theme.
  struct Reset(bool);

  impl App for Reset {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_theme(crate::ui::Theme::PLATES_LIGHT);
      ui.col(Style::new().grow().bg(ui.theme().bg), |ui| {
        ui.disabled(self.0).id("r").icon_button(crate::ui::Icon::RESET);
      });
    }
  }

  #[test]
  fn a_disabled_icon_button_dims_its_icon() {
    let darkest = |off: bool| {
      let mut h = Harness::new(Reset(off), Size::new(100.0, 60.0), 2.0);
      h.frame();
      let r = h.rect(Id::of("r")).unwrap();
      let img = h.render();
      let px = (0..(r.w * 2.0) as i32).flat_map(|x| (0..(r.h * 2.0) as i32).map(move |y| (x, y)));
      px.map(|(x, y)| img.pixel(r.x + x as f32 / 2.0, r.y + y as f32 / 2.0)[0]).min().unwrap()
    };
    let (on, off) = (darkest(false), darkest(true));
    assert!(off > on + 40, "the ink at {on} enabled, {off} disabled");
  }
}
