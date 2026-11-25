// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The check box ([CheckboxToggleStyle]): a box with a check before the label.

use crate::scene::{Color, Point, Rect, alpha, mix};
use crate::ui::{Align, Style, Theme, ToggleConfiguration, ToggleStyle, Ui, px, rigid};

/// The check box, as SwiftUI's `.checkbox`: off, filled with the track and no border where
/// controls are flat ([Theme::flat]), else a bordered box; on, the accent with a white check.
#[derive(Clone, Copy, Debug, Default)]
pub struct CheckboxToggleStyle;

impl ToggleStyle for CheckboxToggleStyle {
  fn make_body(&self, ui: &mut Ui<'_>, mut c: ToggleConfiguration<'_>) {
    let t = ui.controls_theme();
    let (on, off) = (c.is_on(), !c.is_enabled());
    let hot = ui.ctl_hot(c.id(), c.is_hovered());
    let (fill, edge) = check_fill(&t, on, hot);
    let mark = alpha(t.on_accent, if off { t.disabled } else { 1.0 });

    let d = t.sizes.check;
    let row = Style::new().w(rigid()).gap((d * 0.4).round()).align(Align::Start, Align::Center);
    let row = ui.dim_style(row.radius(3.0), off);
    ui.row(row, |ui| {
      let b = Style::new().w(px(d)).h(px(d)).bg(fill).radius(d / 4.0).border(edge.0, edge.1);
      match on {
        true => _ = ui.canvas(b, move |f, r| f.polyline(&check(r), d / 8.0, mark)),
        false => _ = ui.el(b),
      }

      // An icon draws past the row's opacity: dimmed on its own.
      let s = t.label();
      c.label(ui, s.color(ui.dim(s.color, c.is_icon_only() && off)));
    });
  }
}

/// A check box's fill and border (width, colour), `hot` under the pointer: the accent when on,
/// a little lighter under the pointer; off, the track with no border where controls are flat,
/// else the ground with a line.
fn check_fill(t: &Theme, on: bool, hot: f32) -> (Color, (f32, Color)) {
  match (on, t.flat()) {
    (true, _) => (mix(t.accent, [1.0; 4], 0.12 * hot), (0.0, [0.0; 4])),
    (false, true) => (mix(t.track, t.track_hover, hot), (0.0, [0.0; 4])),
    (false, false) => (t.bg, (t.control_border.width(), mix(t.line, t.secondary, hot))),
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
  use crate::ui::{CheckboxToggleStyle, Id, Theme, Ui};

  #[derive(Default)]
  struct Boxes {
    on: bool,
    off: bool,
  }

  impl App for Boxes {
    fn view(&mut self, ui: &mut Ui) {
      let ui = ui.toggle_style(&CheckboxToggleStyle).disabled(self.off);
      ui.id("c").toggle(&mut self.on, "Wrap lines");
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
    let accent = Theme::system(h.rt.ui.appearance()).accent.map(|c| (c * 255.0).round() as u8);
    assert_eq!(h.render().pixel(x, y), accent);

    h.app_mut().off = true;
    h.redraw();
    h.click_id(Id::of("c"));
    assert!(h.app().on);
  }
}
