// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Widgets drawn from the theme ([Ui::theme]): label, link, button (variants, sizes, an
//! [Icon]), ⓘ and "?" buttons, text field, search field and selectable text, check box, switch,
//! slider, select, segmented control, progress bar and spinner, badge, tooltip and the shortcut
//! recorder. Each control is its behaviour (`*_behavior`, as [super::behavior] lists) plus
//! drawing from the theme: an app that wants its own look draws over the same behaviour.
//!
//! ```ignore
//! if ui.button("Refresh").clicked { self.refresh() }
//! if Button::new("Save").primary().show(ui).clicked { self.save() }
//! ui.disabled(!self.dirty).button("Revert");
//! ui.checkbox(&mut self.wrap, "Wrap lines");
//! ui.id("size").select(&mut self.size, &[(Size::Small, "Small"), (Size::Large, "Large")]);
//! let r = ui.icon_button(Icon::RESET);
//! ui.tooltip(r.id, "Back to the default");
//! ```
//!
//! Each widget takes the id set by [Ui::id] or [Ui::key], else one by its position, and returns
//! a [Response](super::Response) with `clicked`, and `changed` when it changed the value it
//! edits (written back at once, so the view reads the new value further down). Hover fades in
//! and out ([super::Theme::fade]) where the theme hovers that control
//! ([super::Theme::hover_all]), and every control shows the pointing hand; [Ui::disabled] dims a
//! widget and ignores its input (no hover, no hand). Controls take focus by Tab only, as
//! AppKit's do; a focused one is ringed ([super::Theme::focus_ring]), Space activates it, and
//! the arrows step a segmented control or a slider.

mod badge;
mod button;
mod checkbox;
mod field;
pub mod icon;
mod label;
mod link;
mod progress;
mod search;
mod segmented;
mod select;
pub mod shortcut;
mod slider;
mod toggle;
mod tooltip;

pub use button::Button;
pub use field::{FieldStyle, TextInput};
pub use icon::Icon;
pub use select::{Choice, Select};

use super::{Behavior, Id, Style, Ui};
use crate::scene::{Color, alpha, mix};

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

  /// `c` dimmed as opacity dims an element when `off`, for what opacity doesn't reach
  /// (canvases): an opaque colour mixed toward the ground, so strokes stay opaque where they
  /// overlap; a translucent one fainter.
  fn dim(&self, c: Color, off: bool) -> Color {
    let t = self.theme();
    match (off, c[3] < 1.0) {
      (false, _) => c,
      (true, true) => alpha(c, c[3] * t.disabled),
      (true, false) => mix(t.bg, c, t.disabled),
    }
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

#[cfg(test)]
mod tests {
  use crate::scene::{Color, Size};
  use crate::ui::{Theme, Ui, UiState};

  #[test]
  fn dimmed_colours_fade_toward_the_ground() {
    let mut ui = Ui::new(UiState::default(), Size::new(10.0, 10.0), None, Default::default());
    for t in [Theme::PLATES_LIGHT, Theme::PLATES_DARK] {
      ui.set_theme(t);
      // Composited over the ground, a dimmed colour is nearer the ground than the colour.
      let over = |c: Color| -> f32 { t.bg[0] * (1.0 - c[3]) + c[0] * c[3] };
      for c in [t.track, t.accent, t.raised, t.muted] {
        let d = ui.dim(c, true);
        assert!((over(d) - t.bg[0]).abs() < (over(c) - t.bg[0]).abs(), "{c:?} dims to {d:?}");
      }

      assert_eq!(ui.dim(t.track, false), t.track);
    }
  }
}
