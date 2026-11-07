// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The link: text that acts on a click, over the button behaviour.

use crate::scene::mix;
use crate::ui::{Response, Role, Style, Ui};

impl Ui<'_> {
  /// Text that acts on a click, a footer's "Check for updates", as SwiftUI's `Link`: the
  /// label's text in the link colour ([crate::Theme::link]) at rest, the text colour under the
  /// pointer where the theme hovers controls ([crate::Theme::hover_all]).
  pub fn link(&mut self, label: &str) -> Response {
    let t = self.theme();
    let style = t.label().color(t.link);
    let id = self.take_id();
    let b = self.button_behavior(id);
    self.a11y(id, Role::Link);
    let hot = self.ctl_hot(id, b.hovered);
    let text = style.color(mix(style.color, t.text, hot));
    let s = self.control(Style::new().radius(3.0), &b);
    let mut r = self.with_id(id).row(s, |ui| ui.text(label, text));
    r.clicked = b.clicked;
    r
  }
}
