// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A pop-up button's behaviour ([Ui::pop_up_behavior]): a button that opens its list, letters
//! typed choosing an option ([TypeSelect]); [crate::Picker] draws one.

use crate::keys::{Command, Move};
use crate::ui::{Behavior, Id, Role, TypeSelect, Ui};

impl Ui<'_> {
  /// A pop-up button `id` over one of `n` options labelled `label(i)`, `at` the chosen one: a
  /// button ([Ui::button_behavior]) whose `clicked` (a click, Space, Enter, Up or Down) opens or
  /// closes its list. Letters typed while it has focus and its list is closed choose the first
  /// option starting with them, as NSPopUpButton does ([TypeSelect]): `changed` then.
  pub fn pop_up_behavior<'l>(
    &mut self,
    id: Id,
    n: usize,
    label: impl Fn(usize) -> &'l str,
    at: &mut Option<usize>,
  ) -> Behavior {
    let mut b = self.button_behavior(id);
    // Its value the current option's label; expanded while open ([Ui::menu_popup]).
    if b.disabled {
      self.a11y(id, Role::PopUpButton).text(at.map_or("", label));
      return b;
    }

    match self.command(id) {
      Some(Command::InsertNewline | Command::Move(Move::Up | Move::Down, _)) => {
        self.flash(id);
        b.clicked = true;
      }
      Some(_) => self.pass(id),
      None => {}
    }

    let now = self.now();
    let mut ts = std::mem::take(self.state::<TypeSelect>(id));
    if let Some(s) = self.typed(id) {
      match ts.push_str(s, now) {
        true => {
          if let Some(i) = ts.find(n, |i| Some(label(i))) {
            b.changed = *at != Some(i);
            *at = Some(i);
          }
        }
        false => self.pass(id),
      }
    }

    *self.state::<TypeSelect>(id) = ts;
    self.a11y(id, Role::PopUpButton).text(at.map_or("", label));
    b
  }
}
