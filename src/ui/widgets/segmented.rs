// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The picker's segmented look, [SegmentedPickerStyle]: segments side by side, one selected, over
//! the segments' input ([PickerConfiguration::segments]).

use crate::scene::alpha;
use crate::ui::{Align, PickerConfiguration, PickerStyle, Style, Ui, grow, px, rigid};

/// Segments side by side, the selected one in the accent, as SwiftUI's `.segmented`: view modes,
/// filters with a count after the label ([crate::Tag::count]). Flat ([crate::Theme::flat]), one
/// raised capsule with dividers between unselected segments, as System Settings; else a bordered
/// box of inset segments. As wide as its labels.
#[derive(Clone, Copy, Debug, Default)]
pub struct SegmentedPickerStyle;

impl PickerStyle for SegmentedPickerStyle {
  fn make_body(&self, ui: &mut Ui<'_>, mut c: PickerConfiguration<'_>) {
    let t = ui.controls_theme();
    let b = c.segments(ui);
    let (off, flat, h) = (b.disabled, t.flat(), t.sizes.control_h);
    let frame = match flat {
      true => Style::new().h(px(h)).bg(t.raised).radius(t.radius),
      false => Style::new()
        .h(px(h))
        .pad(2.0)
        .gap(2.0)
        .radius(t.radius)
        .border(t.control_border.width(), t.line),
    };

    // Its labels' width: text beside it wraps instead.
    let frame = ui.dim_style(frame.w(rigid()), b.disabled);
    let n = c.len();
    let on_at = |i: usize| c.selection() == Some(i);
    // macOS's divider, 1 × 14: #bdbdbd on the light capsule, #6d6d6d on the dark one.
    let ink = if t.scheme.is_dark() { [1.0; 4] } else { [0.0, 0.0, 0.0, 1.0] };
    let divider = alpha(ink, if t.scheme.is_dark() { 0.26 } else { 0.18 });
    ui.row(frame, |ui| {
      for i in 0..n {
        let seg = c.item_id(i);
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

        let pad = (h * 0.375).round();
        let s = Style::new().h(grow()).pad_xy(pad, 0.0).gap(5.0).center();
        let radius = if flat { t.radius } else { t.radius - 2.0 };
        ui.with_id(seg).row(b.pointer(s.radius(radius).bg(bg)), |ui| {
          ui.text(c.label(i), text);
          if let Some(n) = c.count(i) {
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
  }
}

#[cfg(test)]
mod tests {
  use super::SegmentedPickerStyle;
  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::Size;
  use crate::ui::{Id, Style, Tag, Ui, px};
  use crate::{Modifiers, NamedKey};

  struct Seg(u8);

  impl App for Seg {
    fn view(&mut self, ui: &mut Ui) {
      ui.picker_style(&SegmentedPickerStyle).id("v").picker(
        &mut self.0,
        &[
          Tag::new(1, "List").count(12),
          Tag::new(2, "Grid").count(3),
          Tag::new(3, "Columns").count(0),
        ],
      );
    }
  }

  /// Segments after a note in a row `.0` wide.
  struct Beside(f32);

  impl App for Beside {
    fn view(&mut self, ui: &mut Ui) {
      ui.row(Style::new().w(px(self.0)), |ui| {
        ui.label("A note beside the control that wraps");
        ui.picker_style(&SegmentedPickerStyle)
          .id("v")
          .picker(&mut 1, &[Tag::new(1, "List"), Tag::new(2, "Grid")]);
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
    assert!(h.click_id(Id::of("v").key(("item", 2usize))));
    assert_eq!(h.app().0, 3);
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focused(), Some(Id::of("v")));
    h.key(NamedKey::ArrowLeft, Modifiers::empty());
    assert_eq!(h.app().0, 2);
  }
}
