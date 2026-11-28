// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The divider: a hairline across its stack, in the theme's line colour.

use crate::ui::{Axis, Response, Style, Ui, grow, px};

impl Ui<'_> {
  /// A line a point thick in [crate::Theme::line] across the stack it is in, as SwiftUI's
  /// `Divider`: down a row, along a column (or a grid). It takes no input.
  pub fn divider(&mut self) -> Response {
    let t = self.theme();
    let line = match self.parent_axis() {
      Some(Axis::Horizontal) => Style::new().w(px(1.0)).h(grow()),
      Some(Axis::Vertical) | None => Style::new().w(grow()).h(px(1.0)),
    };

    self.el(line.bg(t.line))
  }
}

#[cfg(test)]
mod tests {
  use crate::Harness;
  use crate::app::App;
  use crate::scene::Size;
  use crate::ui::{Id, Style, Ui, px};

  struct Lines;

  impl App for Lines {
    fn view(&mut self, ui: &mut Ui) {
      ui.col(Style::new().w(px(100.0)), |ui| _ = ui.id("across").divider());
      ui.row(Style::new().h(px(30.0)), |ui| _ = ui.id("down").divider());
    }
  }

  /// Along a column it spans the width; down a row, the height.
  #[test]
  fn a_divider_crosses_its_stack() {
    let mut h = Harness::new(Lines, Size::new(200.0, 100.0), 1.0);
    h.frame();
    let (a, d) = (h.rect(Id::of("across")).unwrap(), h.rect(Id::of("down")).unwrap());
    assert_eq!((a.w, a.h, d.w, d.h), (100.0, 1.0, 1.0, 30.0));
  }
}
