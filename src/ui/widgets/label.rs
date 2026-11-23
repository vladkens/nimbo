// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The label: one line of text in the theme's colour.

use std::fmt::Display;

use crate::ui::{Enabled, Ui};

impl Ui<'_> {
  /// One line of text in the theme's colour, cut with "…" when it doesn't fit; dimmed when
  /// disabled ([Ui::disabled], [Enabled]).
  pub fn label(&mut self, s: impl Display) {
    let mut style = self.theme().label();
    if !self.environment_value::<Enabled>().0 {
      style.color[3] *= self.theme().disabled;
    }

    self.text(s, style);
  }
}

#[cfg(test)]
mod tests {
  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::Size;
  use crate::ui::{Id, Kind, Theme, Ui};

  /// A disabled note above a button.
  #[derive(Default)]
  struct Note {
    clicks: u32,
  }

  impl App for Note {
    fn view(&mut self, ui: &mut Ui) {
      ui.disabled(true).id("note").label("Note");
      if ui.id("b").button("B").clicked {
        self.clicks += 1;
      }
    }
  }

  #[test]
  fn a_disabled_label_dims_and_leaves_the_next_button_enabled() {
    let mut h = Harness::new(Note::default(), Size::new(200.0, 100.0), 1.0);
    h.frame();
    let t = Theme::system(h.rt.ui.appearance());
    let Kind::Text { style, .. } = h.rt.ui.last.get(Id::of("note")).unwrap().kind else { panic!() };
    assert_eq!(style.color[3], t.label().color[3] * t.disabled);
    assert!(h.click_id(Id::of("b")));
    assert_eq!(h.app().clicks, 1);
  }
}
