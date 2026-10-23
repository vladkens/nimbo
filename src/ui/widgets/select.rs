// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The select: a pop-up button ([PopUp]) over a value of the app's own type, its options
//! `(value, label)` pairs ([Choice]).
//!
//! ```ignore
//! ui.id("launch").select(&mut self.launch, &[(Launch::Window, "Show the window"), ..]);
//! Select::new(&mut self.lang, &LANGS).separators(&[0]).width(180.0).show(ui.id("lang"));
//! ```

use crate::ui::{PopUp, Response, Ui};

/// An option of a [Select] or a segment of [Ui::segmented]: `(value, label)`, or `(value,
/// label, count)` with a count drawn after a segment's label (formatted while drawing).
pub trait Choice<T> {
  fn value(&self) -> T;
  fn label(&self) -> &str;
  fn count(&self) -> Option<usize> {
    None
  }
}

impl<T: Copy> Choice<T> for (T, &str) {
  fn value(&self) -> T {
    self.0
  }

  fn label(&self) -> &str {
    self.1
  }
}

impl<T: Copy> Choice<T> for (T, &str, usize) {
  fn value(&self) -> T {
    self.0
  }

  fn label(&self) -> &str {
    self.1
  }

  fn count(&self) -> Option<usize> {
    Some(self.2)
  }
}

/// A pop-up button choosing `cur` among `options`; see [PopUp] for its look, keys and menu.
/// A `cur` that is none of the options shows no label; `changed` when an option was picked
/// that wasn't the current, written back to `cur`.
pub struct Select<'s, T, S> {
  cur: &'s mut T,
  options: &'s [S],
  width: Option<f32>,
  separators: &'s [usize],
}

impl<'s, T: PartialEq + Copy, S: Choice<T>> Select<'s, T, S> {
  pub fn new(cur: &'s mut T, options: &'s [S]) -> Self {
    Self { cur, options, width: None, separators: &[] }
  }

  /// A fixed width in points; a label longer than it ends in "…".
  pub fn width(self, w: f32) -> Self {
    Self { width: Some(w), ..self }
  }

  /// A line in the list after each of these options ([PopUp::separators]).
  pub fn separators(self, after: &'s [usize]) -> Self {
    Self { separators: after, ..self }
  }

  pub fn show(self, ui: &mut Ui) -> Response {
    let labels: Vec<&str> = self.options.iter().map(|o| o.label()).collect();
    let n = labels.len();
    let mut at = self.options.iter().position(|o| o.value() == *self.cur).unwrap_or(n);
    let p = PopUp::new(&mut at, &labels).separators(self.separators);
    let r = match self.width {
      Some(w) => p.width(w).show(ui),
      None => p.show(ui),
    };

    if r.changed && at < n {
      *self.cur = self.options[at].value();
    }

    r
  }
}

impl Ui<'_> {
  /// A [Select] as wide as its current option.
  pub fn select<T: PartialEq + Copy, S: Choice<T>>(
    &mut self,
    cur: &mut T,
    options: &[S],
  ) -> Response {
    Select::new(cur, options).show(self)
  }
}

#[cfg(test)]
mod tests {
  use keyboard_types::{Modifiers, NamedKey};

  use crate::app::App;
  use crate::harness::Harness;
  use crate::keys::Key;
  use crate::scene::Size;
  use crate::ui::{Id, Ui};

  #[derive(Clone, Copy, Debug, PartialEq)]
  enum Size2 {
    Small,
    Large,
  }

  struct Pick(Size2);

  impl App for Pick {
    fn view(&mut self, ui: &mut Ui) {
      ui.id("s").select(&mut self.0, &[(Size2::Small, "Small"), (Size2::Large, "Large")]);
    }
  }

  #[test]
  fn a_select_writes_back_the_value_picked() {
    let mut h = Harness::new(Pick(Size2::Small), Size::new(300.0, 200.0), 1.0);
    let s = Id::of("s");
    h.frame();
    h.rt.ui.set_focus(Some(s));
    h.key(Key::Char('l'), Modifiers::empty());
    assert_eq!(h.app().0, Size2::Large);
    // Open, Up and Enter pick the first.
    h.key(NamedKey::Enter, Modifiers::empty());
    h.frame();
    h.frame();
    h.key(NamedKey::ArrowUp, Modifiers::empty());
    h.key(NamedKey::Enter, Modifiers::empty());
    assert_eq!(h.app().0, Size2::Small);
  }
}
