// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The check box: a box with a label over the toggle behaviour ([Ui::toggle_behavior]).

use crate::scene::{Color, Point, Rect, alpha, mix};
use crate::ui::{Align, Response, Style, Theme, Ui, px, rigid};

impl Ui<'_> {
  /// A check box with a label, toggling `on`; `changed` when it did. Off, filled with the track
  /// and no border where controls are flat ([Theme::flat]), else a bordered box; on, the
  /// accent with a white check.
  pub fn checkbox(&mut self, on: &mut bool, label: &str) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let b = self.toggle_behavior(id, on);
    let hot = self.ctl_hot(id, b.hovered);
    let (fill, edge) = check_fill(&t, *on, hot);
    let mark = alpha(t.on_accent, if b.disabled { t.disabled } else { 1.0 });
    let c = t.sizes.check;
    let row = Style::new().w(rigid()).gap((c * 0.4).round()).align(Align::Start, Align::Center);
    let row = row.radius(3.0);
    let row = self.control(row, &b);
    let checked = *on;
    let mut r = self.with_id(id).row(row, |ui| {
      let b = Style::new().w(px(c)).h(px(c)).bg(fill).radius(c / 4.0).border(edge.0, edge.1);
      match checked {
        true => _ = ui.canvas(b, move |f, r| f.polyline(&check(r), c / 8.0, mark)),
        false => _ = ui.el(b),
      }

      if !label.is_empty() {
        ui.text(label, t.label());
      }
    });
    (r.clicked, r.changed) = (b.clicked, b.changed);
    r
  }
}

/// A check box's fill and border (width, colour), `hot` under the pointer: the accent when on,
/// a little lighter under the pointer; off, the track with no border where controls are flat,
/// else the ground with a line.
fn check_fill(t: &Theme, on: bool, hot: f32) -> (Color, (f32, Color)) {
  match (on, t.flat()) {
    (true, _) => (mix(t.accent, [1.0; 4], 0.12 * hot), (0.0, [0.0; 4])),
    (false, true) => (mix(t.track, t.track_hover, hot), (0.0, [0.0; 4])),
    (false, false) => (t.bg, (t.border, mix(t.line, t.muted, hot))),
  }
}

/// A check mark in `r`, a 14-unit box.
fn check(r: Rect) -> [Point; 3] {
  let p = |x: f32, y: f32| Point::new(r.x + x * r.w / 14.0, r.y + y * r.h / 14.0);
  [p(3.5, 7.3), p(6.0, 9.8), p(10.6, 4.4)]
}

#[cfg(test)]
mod tests {
  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::Size;
  use crate::ui::{Id, Ui};

  #[derive(Default)]
  struct Boxes {
    on: bool,
    off: bool,
  }

  impl App for Boxes {
    fn view(&mut self, ui: &mut Ui) {
      ui.disabled(self.off).id("c").checkbox(&mut self.on, "Wrap lines");
    }
  }

  #[test]
  fn a_check_box_checks_in_the_accent_and_not_when_disabled() {
    let mut h = Harness::new(Boxes::default(), Size::new(200.0, 100.0), 1.0);
    h.frame();
    let r = h.rect(Id::of("c")).unwrap();
    let (x, y) = (r.x + 8.0, r.y + r.h / 2.0 - 6.0);
    assert!(h.click_id(Id::of("c")));
    assert!(h.app().on);
    h.leave();
    h.wait(std::time::Duration::from_millis(300));
    let accent = h.rt.ui.theme.accent.map(|c| (c * 255.0).round() as u8);
    assert_eq!(h.render().pixel(x, y), accent);
    h.app_mut().off = true;
    h.rt.redraw();
    h.click_id(Id::of("c"));
    assert!(h.app().on);
  }
}
