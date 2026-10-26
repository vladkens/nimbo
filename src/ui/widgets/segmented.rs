// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The segmented control: segments side by side, one selected, over the pick behaviour
//! ([Ui::pick_behavior]).

use super::select::Choice;
use crate::scene::alpha;
use crate::ui::{Align, Response, Role, Style, Ui, Value, grow, px, rigid};

impl Ui<'_> {
  /// Segments side by side, one selected (`cur`): view modes, zoom levels, filters with counts.
  /// `options` are `(value, label)` or `(value, label, count)`, the count drawn after the
  /// label, dimmed ([Choice]). The segments are `id.key(i)`; a click picks one, Left and Right
  /// step a focused control. Flat ([crate::Theme::flat]), System Settings' look: one raised
  /// capsule, the selected segment the accent over it, dividers between unselected ones; else a
  /// bordered box holding inset segments. `changed` when `cur` changed.
  pub fn segmented<T: PartialEq + Copy, S: Choice<T>>(
    &mut self,
    cur: &mut T,
    options: &[S],
  ) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let was = *cur;
    let mut at = options.iter().position(|o| o.value() == *cur);
    let b = self.pick_behavior(id, options.len(), |i| id.key(i), &mut at);
    self.a11y(id, Role::RadioGroup);
    if b.changed
      && let Some(i) = at
    {
      *cur = options[i].value();
    }

    let (off, flat, h) = (b.disabled, t.flat(), t.sizes.control_h);
    let frame = match flat {
      true => Style::new().h(px(h)).bg(t.raised).radius(t.radius),
      false => Style::new().h(px(h)).pad(2.0).gap(2.0).radius(t.radius).border(t.border, t.line),
    };

    // Its labels' width: text beside it wraps instead.
    let frame = self.control(frame.w(rigid()), &b);
    let n = options.len();
    let on_at = |i: usize| options.get(i).is_some_and(|o| o.value() == *cur);
    // macOS's divider, 1 × 14: #bdbdbd on the light capsule, #6d6d6d on the dark one.
    let ink = if t.dark { [1.0; 4] } else { [0.0, 0.0, 0.0, 1.0] };
    let divider = alpha(ink, if t.dark { 0.26 } else { 0.18 });
    let mut r = self.with_id(id).row(frame, |ui| {
      for (i, o) in options.iter().enumerate() {
        let seg = id.key(i);
        let on = on_at(i);
        let down = !off && !on && ui.pressed(seg);
        let hot = ui.ctl_hot(seg, !off && !on && ui.hovered(seg));
        let (bg, text) = match (on, flat) {
          (true, _) => (t.accent, t.label().color(t.on_accent)),
          // Over the raised capsule: pressed reads macOS's #c6c6c6 / #606060.
          (false, true) if down => (t.pressed, t.label()),
          (false, true) => (alpha(t.text, 0.04 * hot), t.label()),
          (false, false) => (alpha(t.text, 0.08 * hot), t.muted()),
        };

        ui.a11y(seg, Role::RadioButton).value(if on { Value::On } else { Value::Off });
        let pad = (h * 0.375).round();
        let s = Style::new().h(grow()).pad_xy(pad, 0.0).gap(5.0).center();
        let radius = if flat { t.radius } else { t.radius - 2.0 };
        ui.with_id(seg).row(b.pointer(s.radius(radius).bg(bg)), |ui| {
          ui.text(o.label(), text);
          if let Some(n) = o.count() {
            ui.text(n, text.color(alpha(text.color, 0.65)).nowrap());
          }
        });
        if flat && i + 1 < n {
          let shown = !on && !on_at(i + 1);
          let line = Style::new().w(px(1.0)).h(px(14.0)).bg(if shown { divider } else { [0.0; 4] });
          ui.col(Style::new().h(grow()).align(Align::Start, Align::Center), |ui| ui.el(line));
        }
      }
    });
    (r.changed, r.clicked) = (*cur != was, *cur != was);
    r
  }
}

#[cfg(test)]
mod tests {
  use keyboard_types::{Modifiers, NamedKey};

  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::Size;
  use crate::ui::{Id, Ui};

  struct Seg(u8);

  impl App for Seg {
    fn view(&mut self, ui: &mut Ui) {
      ui.id("v").segmented(&mut self.0, &[(1, "List", 12), (2, "Grid", 3), (3, "Columns", 0)]);
    }
  }

  /// Segments after a note in a row `.0` wide.
  struct Beside(f32);

  impl App for Beside {
    fn view(&mut self, ui: &mut Ui) {
      ui.row(crate::ui::Style::new().w(crate::ui::px(self.0)), |ui| {
        ui.label("A note beside the control that wraps");
        ui.id("v").segmented(&mut 1, &[(1, "List"), (2, "Grid")]);
      });
    }
  }

  #[test]
  fn segments_keep_their_width_beside_a_note() {
    let width = |row: f32| {
      let mut h = Harness::new(Beside(row), Size::new(400.0, 100.0), 1.0);
      h.frame();
      h.rect(Id::of("v")).unwrap().w
    };
    let (narrow, wide) = (width(140.0), width(400.0));
    assert!((narrow - wide).abs() <= 1.0, "{narrow} beside a long note, {wide} alone");
  }

  #[test]
  fn segments_pick_by_click_and_arrows() {
    let mut h = Harness::new(Seg(1), Size::new(400.0, 100.0), 1.0);
    h.frame();
    assert!(h.click_id(Id::of("v").key(2)));
    assert_eq!(h.app().0, 3);
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focused(), Some(Id::of("v")));
    h.key(NamedKey::ArrowLeft, Modifiers::empty());
    assert_eq!(h.app().0, 2);
  }
}
