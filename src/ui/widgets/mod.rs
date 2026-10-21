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
//! a [Response](super::Response) with `clicked`. Hover fades in and out ([super::Theme::fade])
//! where the theme hovers that control ([super::Theme::hover_all]), and every control shows the
//! pointing hand; [Ui::disabled] dims a widget and ignores its input (no hover, no hand). Controls
//! take focus by Tab only, as AppKit's do; a focused one is ringed ([super::Theme::focus_ring]) and
//! Space activates it.

mod button;
mod label;

pub use button::Button;

use super::{Behavior, Id, Style, Ui};

impl Ui<'_> {
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
      true => b.style(s).opacity(self.theme().disabled),
      false => b.style(s),
    }
  }
}
