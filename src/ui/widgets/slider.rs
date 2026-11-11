// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The slider: a knob on a track filled in the accent up to it, over a value in a range, free or
//! in steps marked by dots under the track, some of them labelled.

use std::ops::RangeInclusive;

use crate::scene::{Frame, Quad, Rect, alpha, mix};
use crate::ui::{Behavior, Id, Response, Role, Style, TextStyle, Ui, Value, px};

/// Steps closer than this, in points, get no dots: they would run into a line.
const DENSE: f32 = 4.0;

/// The least room between two tick labels, in points.
const LABEL_GAP: f32 = 4.0;

/// What [Slider::tick_label] takes.
type TickLabel<'s> = Box<dyn Fn(f32) -> Option<String> + 's>;

/// A horizontal slider over a range, 160 points wide, as SwiftUI's `Slider(value:in:step:)`: a
/// knob ([crate::Sizes::knob], a capsule when wider than high) on a track [crate::Sizes::rail]
/// thick, filled in the accent up to it, the knob's shadow deeper under the pointer where the
/// theme hovers controls. [Ui::slider] is one with no step.
pub struct Slider<'s> {
  value: &'s mut f32,
  range: RangeInclusive<f32>,
  step: Option<f32>,
  label: Option<TickLabel<'s>>,
}

impl<'s> Slider<'s> {
  pub fn new(value: &'s mut f32, range: RangeInclusive<f32>) -> Self {
    Self { value, range, step: None, label: None }
  }

  /// The distance between its values from the range's start, as SwiftUI's `step:`: a press, a
  /// click or a drag lands on the step nearest the pointer, the arrows (and assistive
  /// technology's increment) move one step. A dot under the track marks each step, as macOS's
  /// tick marks (none where steps are closer than 4 points). The last step is the last that fits
  /// in the range: the range's end only when the step divides it.
  pub fn step(self, step: f32) -> Self {
    Self { step: Some(step).filter(|s| s.is_finite() && *s > 0.0), ..self }
  }

  /// A label under each step that `label` gives one for, as the labels of SwiftUI's
  /// `Slider(…, tick:)`: `|v| (v == 50.0).then(|| format!("{v}%"))`, in the theme's small muted
  /// text. Centred on the step's dot and kept within the slider's width; one that would run into
  /// the label before it, or is wider than the slider, is left out, and all of them where the
  /// steps have no dots. A text, not [crate::Content]: it is placed by its width, measured
  /// before the layout.
  pub fn tick_label(self, label: impl Fn(f32) -> Option<String> + 's) -> Self {
    Self { label: Some(Box::new(label)), ..self }
  }

  pub fn show(self, ui: &mut Ui) -> Response {
    const W: f32 = 160.0;
    let t = ui.theme();
    let ([kw, kh], rh) = (t.sizes.knob, t.sizes.rail);
    let id = ui.take_id();
    let (lo, hi) = (*self.range.start(), *self.range.end());
    let v = self.value;
    let b = ui.slider_behavior(id, v, self.range, self.step, kw / 2.0);
    let hot = ui.ctl_hot(id, b.hovered || b.held);
    let k = if hi != lo { ((*v - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.0 };
    let a = if b.disabled { t.disabled } else { 1.0 };
    let rail = alpha(mix(t.track, t.track_hover, hot), t.track[3] * a);
    let (fill, knob, dot) = (alpha(t.accent, a), alpha(t.knob, a), alpha(t.tertiary, a));
    let drop = [0.0, 0.0, 0.0, (0.25 + 0.1 * hot) * a];
    // Each step's value and its place along the track, 0 to 1, when their dots are apart enough
    // to draw: a few dozen at most.
    let marked =
      self.step.filter(|s| last_step(lo, hi, *s) >= 1.0 && (W - kw) * s / (hi - lo) >= DENSE);
    let steps: Vec<(f32, f32)> = match marked {
      Some(s) => (0..=last_step(lo, hi, s) as usize)
        .map(|i| (lo + s * i as f32, s * i as f32 / (hi - lo)))
        .collect(),
      None => Vec::new(),
    };

    let dots: Vec<f32> = steps.iter().map(|&(_, k)| k).collect();
    let s = b.style(Style::new().w(px(W)).h(px(kh + 4.0)));
    let paint = move |f: &mut Frame, r: Rect| {
      let (x0, w, cy) = (r.x + kw / 2.0, r.w - kw, r.y + r.h / 2.0);
      f.rect(Rect::new(x0 - rh / 2.0, cy - rh / 2.0, w + rh, rh), rail, rh / 2.0);
      f.rect(Rect::new(x0 - rh / 2.0, cy - rh / 2.0, w * k + rh, rh), fill, rh / 2.0);
      // 2-point dots, their bottoms at the knob's, which hides the one under it.
      for d in &dots {
        f.rect(Rect::new(x0 + w * d - 1.0, cy + kh / 2.0 - 2.0, 2.0, 2.0), dot, 1.0);
      }

      let at = Rect::new(x0 + w * k - kw / 2.0, cy - kh / 2.0, kw, kh);
      f.shadow(Rect::new(at.x, at.y + 1.0, at.w, at.h), kh / 2.0, 2.0, drop);
      f.quad(Quad::new(at, knob).radius(kh / 2.0).border(0.5, [0.0, 0.0, 0.0, 0.1 * a]));
    };

    let labels: Vec<(f32, String)> = match &self.label {
      Some(f) => steps.iter().filter_map(|&(v, k)| Some((k, f(v)?))).collect(),
      None => Vec::new(),
    };

    let mut r = if labels.is_empty() {
      ui.with_id(id).canvas(s, paint)
    } else {
      let style = TextStyle::new(t.small, ui.dim(t.muted, b.disabled)).nowrap();
      let mut r = Response::default();
      // As much room over the track as the labels take under it: the track stays in the middle,
      // in line with a row's title or the controls beside it.
      let lh = ui.line_height(style);
      ui.next_id = Some((id.key("ticks"), false));
      ui.col(Style::new().w(px(W)).pads([lh, 0.0, 0.0, 0.0]), |ui| {
        r = ui.with_id(id).canvas(s, paint);
        ui.row(Style::new().w(px(W)), |ui| {
          // Left to right: where the last one ends, and where the next may start.
          let (mut end, mut free) = (0.0, 0.0);
          for (k, label) in &labels {
            let lw = ui.measure(label, style);
            let at = (kw / 2.0 + (W - kw) * k - lw / 2.0).min(W - lw).max(0.0);
            if at < free || at + lw > W {
              continue;
            }

            ui.el(Style::new().w(px(at - end)));
            ui.text(label, style);
            (end, free) = (at + lw, at + lw + LABEL_GAP);
          }
        });
      });
      r
    };

    r.changed = b.changed;
    r
  }
}

impl Ui<'_> {
  /// A horizontal slider over `range` on the element `id`, its track `inset` points in from
  /// either end (half a knob): a press, a click or a drag sets `v` from the pointer, Right and Up
  /// raise it while focused, Left and Down lower it; with a `step` ([Slider::step]) onto the step
  /// nearest from the range's start and by one step, else anywhere and by a twentieth of the range.
  /// `changed` when `v` moved.
  pub fn slider_behavior(
    &mut self,
    id: Id,
    v: &mut f32,
    range: RangeInclusive<f32>,
    step: Option<f32>,
    inset: f32,
  ) -> Behavior {
    let (lo, hi) = (*range.start(), *range.end());
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

  /// A [Slider] over `range` with no step: `v` anywhere in it.
  pub fn slider(&mut self, v: &mut f32, range: RangeInclusive<f32>) -> Response {
    Slider::new(v, range).show(self)
  }
}

/// `v` within `lo..=hi`, onto the nearest of its steps from `lo` with a `step`.
fn snap(v: f32, lo: f32, hi: f32, step: Option<f32>) -> f32 {
  match step {
    Some(s) => lo + ((v - lo) / s).round().clamp(0.0, last_step(lo, hi, s)) * s,
    None => v.clamp(lo.min(hi), hi.max(lo)),
  }
}

/// The index of the last step of `s` from `lo` within `lo..=hi`: 0 when the range is shorter.
fn last_step(lo: f32, hi: f32, s: f32) -> f32 {
  ((hi - lo) / s + 1e-3).floor().max(0.0)
}

#[cfg(test)]
mod tests {
  use super::snap;

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
    assert_eq!([snap(4.2, 0.0, 10.0, None), snap(11.0, 0.0, 10.0, None)], [4.2, 10.0]);
  }
}
