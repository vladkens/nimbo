// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The badge: a pill of small text in a tone on its tint: counts, statuses.

use std::fmt::Display;

use crate::text::{Font, Weight};
use crate::ui::theme::Tone;
use crate::ui::{Response, Style, TextStyle, Ui, px, rigid};

impl Ui<'_> {
  /// A pill: counts, `-4 +5`, statuses ("Allowed", "Not allowed"): [crate::Sizes::badge]
  /// semibold text in the tone on its tint ([crate::Theme::tint]), [crate::Sizes::badge_h] high,
  /// padded 7 at the sides. It takes no input; disabled, it dims.
  pub fn badge(&mut self, s: impl Display, tone: Tone) -> Response {
    let t = self.theme();
    let opacity = if self.take_disabled() { t.disabled } else { 1.0 };
    let font = Font { size: t.sizes.badge, ..t.small }.weight(Weight::Semibold);
    let h = t.sizes.badge_h;
    let pill = Style::new().h(px(h)).pad_xy(7.0, 0.0).center().radius(h / 2.0).bg(t.tint(tone));
    let pill = pill.w(rigid()).opacity(opacity);
    self.row(pill, |ui| ui.text(s, TextStyle::new(font, t.tone(tone)).nowrap()))
  }
}
