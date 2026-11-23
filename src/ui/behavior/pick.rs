// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Picking one of a few items ([Ui::pick_behavior]): tabs, a segmented control, the toolbar's
//! tabs; their looks are `Tabs`, `Ui::segmented` and `Ui::toolbar_tabs`.

use crate::ui::{Behavior, Id, Role, State, Ui};

impl Ui<'_> {
  /// One of `n` items picked by a click on `item(i)`, or by the arrows while `id` is focused
  /// (from none, the first): tabs, a segmented control. `at` is the picked one; `changed` when
  /// it moved. A tab list of tabs, the picked one selected, to assistive technology; a look
  /// refines them (a segmented control's radio buttons).
  pub fn pick_behavior(
    &mut self,
    id: Id,
    n: usize,
    item: impl Fn(usize) -> Id,
    at: &mut Option<usize>,
  ) -> Behavior {
    let (off, was) = (self.begin(id, Role::TabList), *at);
    if off.is_none() {
      // Right and Down to the next item.
      let [x, y] = self.arrows(id);
      let step = x + y;
      if step != 0 && n > 0 {
        let i = at.map_or(0, |i| (i as i32 + step).clamp(0, n as i32 - 1));
        *at = Some(i as usize);
      }

      for i in 0..n {
        if self.clicked(item(i)) {
          *at = Some(i);
        }
      }
    }

    for i in 0..n {
      self.a11y(item(i), Role::Tab).state(State::Selected, *at == Some(i));
    }

    off.unwrap_or_else(|| Behavior { changed: *at != was, ..self.pointer_state(id) })
  }
}
