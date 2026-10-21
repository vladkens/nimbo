// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Widgets drawn from the theme ([Ui::theme]): label and button (variants, sizes). Each control
//! is its behaviour ([super::behavior]) plus drawing from the theme: an app that wants its own
//! look draws over the same behaviour.
//!
//! ```ignore
//! if ui.button("Refresh").clicked { self.refresh() }
//! if Button::new("Save").primary().show(ui).clicked { self.save() }
//! ui.disabled(!self.dirty).button("Revert");
//! ```
//!
//! Each widget takes the id set by [Ui::id] or [Ui::key], else one by its position, and returns
//! a [Response] with `clicked`. Hover fades in and out ([super::Theme::fade]) where the theme
//! hovers that control ([super::Theme::hover_all]), and every control shows the pointing hand;
//! [Ui::disabled] dims a widget and ignores its input (no hover, no hand). Controls take focus
//! by Tab only, as AppKit's do; a focused one is ringed ([super::Theme::focus_ring]) and Space
//! activates it.

use std::fmt::Display;

use super::theme::{ButtonStyle, ControlSize, Variant};
use super::{Behavior, Id, Response, Style, Ui};
use crate::scene::mix;

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

  /// Shows it selected or toggled on ([super::Theme::selected]) when `on`: a sidebar's toggle,
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
    r
  }
}

impl Ui<'_> {
  // MARK: Helpers

  /// A hover fade for `id`, 0 to 1, on the theme's transition: `on` while hovered. Its state
  /// is `id.key("hot")`.
  pub fn hot(&mut self, id: Id, on: bool) -> f32 {
    let fade = self.theme().fade;
    self.animate(id.key("hot"), on as u8 as f32, fade)
  }

  /// [Ui::hot] for a control macOS draws no hover on (a push button, a switch): the fade while
  /// hovered when the theme hovers every control ([super::Theme::hover_all]), else 0.
  pub fn ctl_hot(&mut self, id: Id, on: bool) -> f32 {
    let all = self.theme().hover_all;
    self.hot(id, on && all)
  }

  /// A control's box over its behaviour: focusable by Tab with the pointing hand
  /// ([Behavior::style]), or dimmed when disabled.
  fn control(&self, s: Style, b: &Behavior) -> Style {
    match b.disabled {
      true => s.opacity(self.theme().disabled),
      false => b.style(s),
    }
  }

  // MARK: Widgets

  /// One line of text in the theme's colour, cut with "…" when it doesn't fit.
  pub fn label(&mut self, s: impl Display) {
    let style = self.theme().label();
    self.text(s, style);
  }

  /// A [Button] with the normal look.
  pub fn button(&mut self, label: &str) -> Response {
    Button::new(label).show(self)
  }
}
