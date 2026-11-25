// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The slider's look from the theme ([SliderStyle]): [LinearSliderStyle], a knob on a track
//! filled in the accent up to it, its steps marked by dots under the track, some of them
//! labelled.

use crate::scene::{Frame, Quad, Rect, alpha, mix};
use crate::ui::{SliderConfiguration, SliderStyle, Style, TextStyle, Ui, grow, px};

/// Steps closer than this, in points, get no dots: they would run into a line.
const DENSE: f32 = 4.0;

/// The least room between two tick labels, in points.
const LABEL_GAP: f32 = 4.0;

/// The theme's slider, as macOS's: a knob ([crate::Sizes::knob], a capsule when wider than high)
/// on a track [crate::Sizes::rail] thick, filled in the accent up to it, the knob's shadow deeper
/// under the pointer where the theme hovers controls; a dot under the track at each step
/// ([crate::Slider::step]), as macOS's tick marks, none where steps are closer than 4 points. It
/// fills the width it is offered, as SwiftUI's, and is at least 160 points wide: that much among
/// a form row's controls, which keep their width, the row's whole width in its footer
/// ([crate::FormRow::footer]). The shell's default.
///
/// Tick labels ([crate::Slider::tick_label]) are in the theme's small muted text, centred on
/// their step's dot and kept within the slider's width; one that would run into the label before
/// it, or is wider than the slider, is left out, and all of them where the steps have no dots.
/// They are placed along the slider's width in the last frame (the view runs again when the
/// layout changes it), with as much room over the track as they take under it.
#[derive(Clone, Copy, Debug, Default)]
pub struct LinearSliderStyle;

impl SliderStyle for LinearSliderStyle {
  fn make_body(&self, ui: &mut Ui<'_>, c: SliderConfiguration<'_>) {
    const MIN_W: f32 = 160.0;
    let t = ui.controls_theme();
    let ([kw, kh], rh) = (t.sizes.knob, t.sizes.rail);
    let (id, off) = (c.id(), !c.is_enabled());
    let (lo, hi) = (*c.range().start(), *c.range().end());
    let hot = ui.ctl_hot(id, c.is_hovered() || c.is_held());
    // Its width from the last frame, for the dots and the labels: the view runs again when the
    // layout gives it another.
    if c.step().is_some() {
      ui.settle(id);
    }

    let w = ui.rect(id).map_or(MIN_W, |r| r.w.max(MIN_W));
    let k = c.fraction();
    let a = if off { t.disabled } else { 1.0 };
    let rail = alpha(mix(t.track, t.track_hover, hot), t.track[3] * a);
    let (fill, knob, dot) = (alpha(t.accent, a), alpha(t.knob, a), alpha(t.tertiary, a));
    let drop = [0.0, 0.0, 0.0, (0.25 + 0.1 * hot) * a];
    // Each step's value and its place along the track, 0 to 1, when their dots are apart enough
    // to draw: a few dozen at most.
    let marked = c.step().filter(|s| c.steps().len() > 1 && (w - kw) * s / (hi - lo) >= DENSE);
    let steps: Vec<(f32, f32)> = if marked.is_some() { c.steps().collect() } else { Vec::new() };

    let dots: Vec<f32> = steps.iter().map(|&(_, k)| k).collect();
    let width = Style::new().w(grow()).min_w(MIN_W);
    // The track half a knob in from either end, where the knob's centre stops.
    let s = width.h(px(kh + 4.0)).pad_xy(kw / 2.0, 0.0);
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

    let labels: Vec<(f32, String)> =
      steps.iter().filter_map(|&(v, k)| Some((k, c.tick_label(v)?))).collect();
    if labels.is_empty() {
      c.root(ui).canvas(s, paint);
      return;
    }

    let style = TextStyle::new(t.small, ui.dim(t.muted, off)).nowrap();
    // As much room over the track as the labels take under it: the track stays in the middle,
    // in line with a row's title or the controls beside it.
    let lh = ui.line_height(style);
    ui.with_id(id.key("ticks")).allows_hit_testing(false).col(
      width.pads([lh, 0.0, 0.0, 0.0]),
      |ui| {
        c.root(ui).canvas(s, paint);
        ui.row(Style::new().w(grow()), |ui| {
          // Left to right: where the last one ends, and where the next may start.
          let (mut end, mut free) = (0.0, 0.0);
          for (k, label) in &labels {
            let lw = ui.measure(label, style);
            let at = (kw / 2.0 + (w - kw) * k - lw / 2.0).min(w - lw).max(0.0);
            if at < free || at + lw > w {
              continue;
            }

            ui.el(Style::new().w(px(at - end)));
            ui.text(label, style);
            (end, free) = (at + lw, at + lw + LABEL_GAP);
          }
        });
      },
    );
  }
}

#[cfg(test)]
mod tests {
  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::Size;
  use crate::ui::{Id, Theme, Ui};

  struct One;

  impl App for One {
    fn view(&mut self, ui: &mut Ui) {
      ui.id("s").slider(&mut 0.5, 0.0..=1.0);
    }
  }

  /// Under the shell the theme's: as high as its knob and 4, at least 160 wide.
  #[test]
  fn linear_is_the_shells_default() {
    let mut h = Harness::new(One, Size::new(100.0, 100.0), 1.0);
    h.frame();
    let r = h.rect(Id::of("s")).unwrap();
    let kh = Theme::PLATES_DARK.sizes.knob[1];
    assert_eq!((r.w, r.h), (160.0, kh + 4.0));
  }
}
