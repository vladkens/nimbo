// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A check box's and a switch's behaviour ([Ui::toggle_behavior]): a button that flips a `bool`.

use crate::ui::{Behavior, Id, Role, Ui, Value};

impl Ui<'_> {
  /// A check box or switch over `on`, flipped by a click or Space while focused: `changed` when
  /// it was, `on` its value after. A check box on or off to assistive technology; a switch's look
  /// refines it.
  pub fn toggle_behavior(&mut self, id: Id, on: &mut bool) -> Behavior {
    let mut b = self.button_behavior(id);
    if b.clicked {
      *on = !*on;
    }

    (b.changed, b.on) = (b.clicked, *on);
    self.a11y(id, Role::CheckBox).value(if *on { Value::On } else { Value::Off });
    b
  }
}
