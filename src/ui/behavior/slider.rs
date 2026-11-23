// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A slider's behaviour ([Ui::slider_behavior]): a value in a range set by a drag along the
//! element or by the arrows, free or in steps; `Slider` draws it.

use std::ops::RangeInclusive;

use crate::ui::{Behavior, Id, Role, Ui, Value};

impl Ui<'_> {
  /// A horizontal slider over `range` on the element `id`, its track `inset` points in from
  /// either end (half a knob): a press, a click or a drag sets `v` from the pointer, Right and Up
  /// raise it while focused, Left and Down lower it; with a `step` (`Slider::step`) onto the
  /// step nearest from the range's start and by one step, else anywhere and by a twentieth of the
  /// range. `changed` when `v` moved.
  pub fn slider_behavior(
    &mut self,
    id: Id,
    v: &mut f32,
    range: RangeInclusive<f32>,
    step: Option<f32>,
    inset: f32,
  ) -> Behavior {
    let (lo, hi) = (*range.start(), *range.end());
    // A step as [crate::Slider::step] takes it: none unless finite and positive.
    let step = step.filter(|s| s.is_finite() && *s > 0.0);
    let b = match self.begin(id, Role::Slider) {
      Some(b) => b,
      None => self.slide(id, v, (lo, hi), step, inset),
    };

    let (now, min, max) = (f64::from(*v), f64::from(lo), f64::from(hi));
    self.a11y(id, Role::Slider).value(Value::Number { now, min, max });
    b
  }

  /// [Ui::slider_behavior]'s input, enabled.
  fn slide(
    &mut self,
    id: Id,
    v: &mut f32,
    (lo, hi): (f32, f32),
    step: Option<f32>,
    inset: f32,
  ) -> Behavior {
    let was = *v;
    let snap = |v: f32| snap(v, lo, hi, step);

    // Held, or released since the view last ran: a click, a drag's last move.
    let at = self.mouse().filter(|_| self.held(id)).or(self.released(id).map(|(_, p)| p));
    if let (Some(r), Some(m)) = (self.rect(id), at) {
      let k = ((m.x - r.x - inset) / (r.w - 2.0 * inset).max(1.0)).clamp(0.0, 1.0);
      *v = snap(lo + (hi - lo) * k);
    }

    // Right and Up raise it, as AppKit's slider.
    let [x, y] = self.arrows(id);
    if x - y != 0 {
      *v = snap(snap(*v) + step.unwrap_or((hi - lo) / 20.0) * (x - y) as f32);
    }

    Behavior { changed: *v != was, ..self.pointer_state(id) }
  }
}

/// `v` within `lo..=hi`, onto the nearest of its steps from `lo` with a `step`.
fn snap(v: f32, lo: f32, hi: f32, step: Option<f32>) -> f32 {
  let v = match step {
    Some(s) => lo + ((v - lo) / s).round().clamp(0.0, last_step(lo, hi, s)) * s,
    None => v,
  };
  // Against f32 drift on the last step.
  v.clamp(lo.min(hi), hi.max(lo))
}

/// The index of the last step of `s` from `lo` within `lo..=hi`: 0 when the range is shorter.
fn last_step(lo: f32, hi: f32, s: f32) -> f32 {
  ((hi - lo) / s + 1e-4).floor().max(0.0)
}

#[cfg(test)]
mod tests {
  use super::{last_step, snap};

  /// A step that doesn't divide the range ends it at its last step, from the pointer and from
  /// the keys alike; one longer than the range keeps its start; with none, anywhere in it.
  #[test]
  fn snaps_onto_steps_within_the_range() {
    let fours = |v| snap(v, 0.0, 10.0, Some(4.0));
    assert_eq!(
      [fours(5.0), fours(9.9), fours(10.0), fours(8.0 + 4.0), fours(-3.0)],
      [4.0, 8.0, 8.0, 8.0, 0.0]
    );
    assert_eq!(snap(9.0 + 3.0, 0.0, 10.0, Some(3.0)), 9.0);
    assert_eq!([snap(6.0, 0.0, 10.0, Some(20.0)), snap(10.0, 0.0, 10.0, Some(20.0))], [0.0, 0.0]);
    assert_eq!(
      [snap(30.0, 10.0, 90.0, Some(10.0)), snap(34.9, 10.0, 90.0, Some(10.0))],
      [30.0, 30.0]
    );
    assert_eq!(
      [snap(30.0, 0.0, 29.99, Some(10.0)), snap(25.0, 0.0, 29.99, Some(10.0))],
      [20.0, 20.0]
    );
    assert_eq!([last_step(0.0, 1.0, 0.1), snap(1.0, 0.0, 1.0, Some(0.1))], [10.0, 1.0]);
    assert_eq!([snap(4.2, 0.0, 10.0, None), snap(11.0, 0.0, 10.0, None)], [4.2, 10.0]);
  }
}
