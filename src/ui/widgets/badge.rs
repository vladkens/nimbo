// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The badge: a pill of small text in a tone on its tint: counts, statuses.

use std::fmt::Display;

use crate::text::{Font, Weight};
use crate::ui::theme::{ControlSize, Tone};
use crate::ui::{Response, Style, TextStyle, Ui, px, rigid};

impl Ui<'_> {
  /// A pill: counts, `-4 +5`, statuses ("Allowed", "Not allowed"): [crate::Sizes::badge]
  /// semibold text in the tone on its tint ([crate::Theme::tint]), [crate::Sizes::badge_h] high,
  /// padded 7 at the sides. It takes no input; disabled, it dims.
  pub fn badge(&mut self, s: impl Display, tone: Tone) -> Response {
    self.badge_sized(s, tone, ControlSize::Medium)
  }

  /// [Ui::badge] at `size`, as SwiftUI's `controlSize`: [ControlSize::Small] a tag on a label's
  /// line, a point smaller, 3 lower, padded 5, its corners 4 round; Large as Medium.
  pub fn badge_sized(&mut self, s: impl Display, tone: Tone, size: ControlSize) -> Response {
    let t = self.theme();
    let opacity = if self.take_disabled() { t.disabled } else { 1.0 };
    let (font, h) = (t.sizes.badge, t.sizes.badge_h);
    let (font, h, pad, r) = match size {
      ControlSize::Small => (font - 1.0, h - 3.0, 5.0, 4.0),
      _ => (font, h, 7.0, h / 2.0),
    };

    let font = Font { size: font, ..t.small }.weight(Weight::Semibold);
    let pill = Style::new().h(px(h)).pad_xy(pad, 0.0).center().radius(r).bg(t.tint(tone));
    let pill = pill.w(rigid()).opacity(opacity);
    self.row(pill, |ui| ui.text(s, TextStyle::new(font, t.tone(tone)).nowrap()))
  }
}
