// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The slider: a knob on a track filled in the accent up to it, over a value in a range.

use std::ops::RangeInclusive;

use crate::scene::{Quad, Rect, alpha, mix};
use crate::ui::{Behavior, Id, Response, Role, Style, Ui, Value, px};

impl Ui<'_> {
  /// A horizontal slider over `range` on the element `id`, its track `inset` points in from
  /// either end (half a knob): a press, a click or a drag sets `v` from the pointer, Left and Right
  /// (and Up and Down) step it by a twentieth while focused. `changed` when `v` moved.
  pub fn slider_behavior(
    &mut self,
    id: Id,
    v: &mut f32,
    range: RangeInclusive<f32>,
    inset: f32,
  ) -> Behavior {
    let (lo, hi) = (*range.start(), *range.end());
    let b = match self.begin(id, Role::Slider) {
      Some(b) => b,
      None => self.slide(id, v, (lo, hi), inset),
    };

    let (now, min, max) = (f64::from(*v), f64::from(lo), f64::from(hi));
    self.a11y(id, Role::Slider).value(Value::Number { now, min, max });
    b
  }

  /// [Ui::slider_behavior]'s input, enabled.
  fn slide(&mut self, id: Id, v: &mut f32, (lo, hi): (f32, f32), inset: f32) -> Behavior {
    let was = *v;
    // Held, or released since the view last ran: a click, a drag's last move.
    let at = self.mouse().filter(|_| self.held(id)).or(self.released(id).map(|(_, p)| p));
    if let (Some(r), Some(m)) = (self.rect(id), at) {
      let k = ((m.x - r.x - inset) / (r.w - 2.0 * inset).max(1.0)).clamp(0.0, 1.0);
      *v = lo + (hi - lo) * k;
    }

    let step = self.arrows(id);
    if step != 0 {
      *v = (*v + (hi - lo) / 20.0 * step as f32).clamp(lo.min(hi), hi.max(lo));
    }

    Behavior { changed: *v != was, ..self.pointer_state(id) }
  }

  /// A horizontal slider over `range`, 160 points wide: a knob ([crate::Sizes::knob], a capsule
  /// when wider than high) on a track [crate::Sizes::rail] thick, filled in the accent up to
  /// it, the knob's shadow deeper under the pointer where the theme hovers controls.
  pub fn slider(&mut self, v: &mut f32, range: RangeInclusive<f32>) -> Response {
    const W: f32 = 160.0;
    let t = self.theme();
    let ([kw, kh], rh) = (t.sizes.knob, t.sizes.rail);
    let id = self.take_id();
    let (lo, hi) = (*range.start(), *range.end());
    let b = self.slider_behavior(id, v, range, kw / 2.0);
    let hot = self.ctl_hot(id, b.hovered || b.held);
    let k = if hi != lo { ((*v - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.0 };
    let a = if b.disabled { t.disabled } else { 1.0 };
    let rail = alpha(mix(t.track, t.track_hover, hot), t.track[3] * a);
    let (fill, knob) = (alpha(t.accent, a), alpha(t.knob, a));
    let drop = [0.0, 0.0, 0.0, (0.25 + 0.1 * hot) * a];
    let s = b.style(Style::new().w(px(W)).h(px(kh + 4.0)));
    let mut r = self.with_id(id).canvas(s, move |f, r| {
      let (x0, w, cy) = (r.x + kw / 2.0, r.w - kw, r.y + r.h / 2.0);
      f.rect(Rect::new(x0 - rh / 2.0, cy - rh / 2.0, w + rh, rh), rail, rh / 2.0);
      f.rect(Rect::new(x0 - rh / 2.0, cy - rh / 2.0, w * k + rh, rh), fill, rh / 2.0);
      let at = Rect::new(x0 + w * k - kw / 2.0, cy - kh / 2.0, kw, kh);
      f.shadow(Rect::new(at.x, at.y + 1.0, at.w, at.h), kh / 2.0, 2.0, drop);
      f.quad(Quad::new(at, knob).radius(kh / 2.0).border(0.5, [0.0, 0.0, 0.0, 0.1 * a]));
    });
    r.changed = b.changed;
    r
  }
}
