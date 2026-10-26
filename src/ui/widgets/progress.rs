// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Progress: a bar filling in the accent, or sliding while the end is unknown, and a spinner.

use std::f32::consts::{PI, TAU};

use crate::scene::{Point, Rect};
use crate::ui::{Response, Role, State, Style, Ui, Value, grow, px};

impl Ui<'_> {
  /// A progress bar across its parent, 4 points high: `Some(k)` fills `k` (0 to 1) of it in the
  /// accent; `None` slides a third of it along every 1.4 s while frames follow (indeterminate).
  /// Disabled, both dim toward the ground and stop.
  pub fn progress(&mut self, k: Option<f32>) -> Response {
    let t = self.theme();
    let (id, off) = (self.take_id(), self.take_disabled());
    let (rail, fill) = (self.dim(t.track, off), self.dim(t.accent, off));
    let now = self.now().as_secs_f32();
    if k.is_none() && !off {
      self.st.animating = true;
    }

    let value = k.map_or(Value::None, |k| Value::Number { now: k.into(), min: 0.0, max: 1.0 });
    self.a11y(id, Role::ProgressIndicator).value(value).state(State::Disabled, off);
    self.with_id(id).canvas(Style::new().w(grow()).h(px(4.0)), move |f, r| {
      f.rect(r, rail, r.h / 2.0);
      let (x0, x1) = match k {
        Some(k) => (0.0, k.clamp(0.0, 1.0) * r.w),
        None if off => (0.0, 0.0),
        None => {
          let at = (now / 1.4).fract() * 1.35 - 0.35;
          ((at * r.w).max(0.0), ((at + 0.35) * r.w).min(r.w))
        }
      };

      if x1 > x0 {
        f.rect(Rect::new(r.x + x0, r.y, x1 - x0, r.h), fill, r.h / 2.0);
      }
    })
  }

  /// A spinning arc `size` points wide in the muted colour, for work without a known end, a
  /// turn every 0.9 s while it shows; disabled, dimmed and still.
  pub fn spinner(&mut self, size: f32) -> Response {
    let t = self.theme();
    let (id, off) = (self.take_id(), self.take_disabled());
    let color = self.dim(t.muted, off);
    let turn = if off { 0.0 } else { self.now().as_secs_f32() / 0.9 * TAU };
    self.st.animating |= !off;
    self.a11y(id, Role::ProgressIndicator).state(State::Disabled, off);
    self.with_id(id).canvas(Style::new().w(px(size)).h(px(size)), move |f, r| {
      let w = (size / 8.0).max(1.5);
      let (cx, cy, rad) = (r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0 - w);
      let pts: Vec<Point> = (0..=24)
        .map(|i| turn + i as f32 / 24.0 * 1.5 * PI)
        .map(|a| Point::new(cx + rad * a.cos(), cy + rad * a.sin()))
        .collect();
      f.polyline(&pts, w, color);
    })
  }
}

#[cfg(test)]
mod tests {
  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::Size;
  use crate::ui::{Style, Ui, grow};

  struct Bar(Option<f32>);

  impl App for Bar {
    fn view(&mut self, ui: &mut Ui) {
      let k = self.0;
      ui.col(Style::new().w(grow()).pad(10.0), |ui| ui.progress(k));
    }
  }

  #[test]
  fn a_bar_fills_its_share_and_an_unknown_one_animates() {
    let mut h = Harness::new(Bar(Some(0.5)), Size::new(220.0, 60.0), 1.0);
    let img = h.render();
    let accent = h.rt.ui.theme.accent.map(|c| (c * 255.0).round() as u8);
    assert_eq!(img.pixel(60.0, 12.0), accent);
    assert_ne!(img.pixel(160.0, 12.0), accent);
    assert!(!h.rt.needs_frame());
    h.app_mut().0 = None;
    h.rt.redraw();
    h.frame();
    assert!(h.rt.needs_frame());
  }

  /// A disabled spinner, then a button.
  #[derive(Default)]
  struct Still(usize);

  impl App for Still {
    fn view(&mut self, ui: &mut Ui) {
      ui.disabled(true).spinner(16.0);
      if ui.id("b").button("B").clicked {
        self.0 += 1;
      }
    }
  }

  #[test]
  fn a_disabled_spinner_stands_still_and_leaves_the_next_widget_enabled() {
    let mut h = Harness::new(Still::default(), Size::new(200.0, 60.0), 1.0);
    h.frame();
    assert!(!h.rt.needs_frame());
    h.click_id(crate::ui::Id::of("b"));
    assert_eq!(h.app().0, 1);
  }
}
