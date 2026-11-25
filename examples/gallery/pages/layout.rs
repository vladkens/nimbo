// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The layout blocks, built live: stacks, a grid, a spacer, dividers, a scroll view and an overlay.

use nimbo::{Axes, Float, Style, Ui, UnitPoint, grow, px};

use super::{Gallery, Shown, line, swatch};

impl Gallery {
  /// The live layout block `name`.
  pub(super) fn layout(&mut self, ui: &mut Ui, name: &str) {
    let t = ui.theme();
    match name {
      "Stacks" => Self::variants(ui, Shown::Alone, |ui| {
        ui.row(Style::new().gap(8.0), |ui| (0..3).for_each(|_| _ = ui.el(swatch(t.accent))));
        ui.col(Style::new().gap(4.0), |ui| (0..2).for_each(|_| _ = ui.el(swatch(t.raised))));
      }),
      "Grid" => Self::variants(ui, Shown::Alone, |ui| {
        let grid = Style::new().w(px(120.0)).gap(6.0);
        ui.grid(3, grid, |ui| (0..6).for_each(|_| _ = ui.el(swatch(t.raised))));
      }),

      "Spacer" => Self::variants(ui, Shown::Alone, |ui| {
        ui.row(Style::new().w(grow()), |ui| {
          ui.text("Left", t.label());
          ui.spacer();
          ui.text("Right", t.label());
        });
      }),

      "Divider" => Self::variants(ui, Shown::Alone, |ui| {
        ui.row(line(8.0).h(px(24.0)), |ui| {
          ui.text("One", t.label());
          ui.divider();
          ui.text("Two", t.label());
        });
        ui.col(Style::new().w(px(120.0)).gap(4.0), |ui| {
          ui.text("Above", t.label());
          ui.divider();
          ui.text("Below", t.label());
        });
      }),
      "Scroll view" => Self::variants(ui, Shown::Alone, |ui| {
        let area = Style::new().w(px(200.0)).h(px(60.0)).scroll(Axes::VERTICAL);
        ui.scroll_area(area, |ui| {
          (1..=8).for_each(|i| ui.text(format_args!("Line {i}"), t.label()))
        });
      }),

      "Overlay" => Self::variants(ui, Shown::Alone, |ui| {
        ui.row(swatch(t.raised).w(px(80.0)).h(px(48.0)), |ui| {
          let at = UnitPoint::TOP_TRAILING;
          let dot = swatch(t.accent).w(px(12.0)).h(px(12.0));
          ui.float(Float::parent().attach(at).anchor(at).offset(-4.0, 4.0)).el(dot);
        });
      }),
      _ => unreachable!("{name} is not a layout block"),
    }
  }
}
