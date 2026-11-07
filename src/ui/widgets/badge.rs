// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The badge: a pill of small text in a tone on its tint: counts, statuses.

use std::fmt::Display;

use crate::text::{Font, Weight};
use crate::ui::theme::{ControlSize, Tone};
use crate::ui::{Response, Role, Style, TextStyle, Ui, px, rigid};

impl Ui<'_> {
  /// A pill: counts, `-4 +5`, statuses ("Allowed", "Not allowed"), as SwiftUI's `.badge`, but
  /// an element of its own, wherever it is built (a tag after a form row's name):
  /// [crate::Sizes::badge] semibold text in the tone on its tint ([crate::Theme::tint]),
  /// [crate::Sizes::badge_h] high, padded 7 at the sides; at [ControlSize::Small]
  /// ([Ui::control_size]) a tag on a label's line, a point smaller, 3 lower, padded 5, its corners
  /// 4 round; Large as Regular. It takes no input; disabled, it dims. Assistive technology reads
  /// it as a text, where it stands.
  pub fn badge(&mut self, s: impl Display, tone: Tone) -> Response {
    let t = self.theme();
    let (id, size) = (self.take_id(), self.take_control_size());
    let opacity = if self.take_disabled() { t.disabled } else { 1.0 };
    let (font, h) = (t.sizes.badge, t.sizes.badge_h);
    let (font, h, pad, r) = match size {
      ControlSize::Small => (font - 1.0, h - 3.0, 5.0, 4.0),
      _ => (font, h, 7.0, h / 2.0),
    };

    let font = Font { size: font, ..t.small }.weight(Weight::Semibold);
    let pill = Style::new().h(px(h)).pad_xy(pad, 0.0).center().radius(r).bg(t.tint(tone));
    let pill = pill.w(rigid()).opacity(opacity);
    let text = s.to_string();
    self.a11y_named(id, Role::StaticText, &text);
    self.next_id = Some((id, false));
    self.row(pill, |ui| ui.text(&text, TextStyle::new(font, t.tone(tone)).fixed_size()))
  }
}
