// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The badge: a pill of small text in a tone on its tint: counts, statuses.

use std::fmt::Display;

use crate::text::{Font, Weight};
use crate::ui::theme::Tone;
use crate::ui::{ControlSize, Enabled, Response, Role, Style, TextStyle, Ui, px, rigid};

impl Ui<'_> {
  /// A pill: counts, `-4 +5`, statuses ("Allowed", "Not allowed"), as SwiftUI's `.badge`, but
  /// an element of its own, wherever it is built (a tag after a form row's name):
  /// [crate::Sizes::badge] semibold text in the tone on its tint ([crate::Theme::tint]),
  /// [crate::Sizes::badge_h] high, padded 7 at the sides; at [ControlSize::Small]
  /// ([Ui::control_size]) a tag on a label's line, a point smaller, 3 lower, padded 5, a capsule
  /// still; Large as Regular. It takes no input; disabled, it dims. Assistive technology reads
  /// it as a text, where it stands.
  pub fn badge(&mut self, s: impl Display, tone: Tone) -> Response {
    let t = self.theme();
    let (id, size) = (self.take_id(), self.environment_value::<ControlSize>());
    let opacity = if self.environment_value::<Enabled>().0 { 1.0 } else { t.disabled };
    let (font, h) = (t.sizes.badge, t.sizes.badge_h);
    let (font, h, pad) = match size {
      ControlSize::Small => (font - 1.0, h - 3.0, 5.0),
      _ => (font, h, 7.0),
    };

    let font = Font { size: font, ..t.small }.weight(Weight::Semibold);
    let pill = Style::new().h(px(h)).pad_xy(pad, 0.0).center().radius(h / 2.0).bg(t.tint(tone));
    let pill = pill.w(rigid()).opacity(opacity);
    let text = s.to_string();
    self.a11y(id, Role::StaticText).name_or(&text);
    self
      .with_id(id)
      .allows_hit_testing(false)
      .row(pill, |ui| ui.text(&text, TextStyle::new(font, t.tone(tone)).nowrap()))
  }
}

#[cfg(test)]
mod tests {
  use crate::Harness;
  use crate::app::App;
  use crate::scene::Size;
  use crate::ui::{ControlSize, Id, Tone, Ui};

  /// A badge at a control size.
  struct Badge(ControlSize);

  impl App for Badge {
    fn view(&mut self, ui: &mut Ui) {
      ui.control_size(self.0).id("b").badge("New", Tone::Accent);
    }
  }

  /// A badge is a capsule at every size: its corners half its height round.
  #[test]
  fn a_badge_is_a_capsule_at_every_size() {
    for c in [ControlSize::Small, ControlSize::Regular, ControlSize::Large] {
      let mut h = Harness::new(Badge(c), Size::new(200.0, 100.0), 1.0);
      h.frame();
      let r = h.rect(Id::of("b")).unwrap();
      let node = h.rt.ui.last.get(Id::of("b")).unwrap();
      assert_eq!(node.style.radii, [r.h / 2.0; 4], "{c:?}");
    }
  }
}
