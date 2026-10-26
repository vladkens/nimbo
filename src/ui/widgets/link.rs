// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The link: text that acts on a click, over the button behaviour.

use crate::scene::mix;
use crate::ui::{Response, Role, Style, TextStyle, Ui};

impl Ui<'_> {
  /// Text that acts on a click, a footer's "Check for updates": `style` at rest, the theme's
  /// text colour under the pointer (the link colour when `style` is in the text colour
  /// already), where the theme hovers controls ([crate::Theme::hover_all]).
  pub fn link(&mut self, label: &str, style: TextStyle) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let b = self.button_behavior(id);
    self.a11y(id, Role::Link);
    let hot = self.ctl_hot(id, b.hovered);
    let to = if style.color == t.text { t.link } else { t.text };
    let text = style.color(mix(style.color, to, hot));
    let s = self.control(Style::new().radius(3.0), &b);
    let mut r = self.with_id(id).row(s, |ui| ui.text(label, text));
    r.clicked = b.clicked;
    r
  }
}
