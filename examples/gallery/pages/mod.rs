// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The blocks built live, a module per group: [Gallery::demo] builds the one shown in its variants.

mod containers;
mod controls;
mod form;
mod layout;
mod list;
mod modifiers;
mod presentation;
mod text;

use nimbo::{Align, Color, ControlSize, Style, Ui, grow, px};

use super::{Block, DIALOG, Gallery, SIZES};

/// The variants a block shows: itself alone (layout and text, which nothing disables); as it is
/// and disabled; and at each size too.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shown {
  Alone,
  Plain,
  Sized,
}

/// A line of widgets, `gap` apart, centred on each other.
fn line(gap: f32) -> Style {
  Style::new().gap(gap).align(Align::Start, Align::Center)
}

/// A square swatch of `c`.
fn swatch(c: Color) -> Style {
  Style::new().w(px(28.0)).h(px(28.0)).radius(6.0).bg(c)
}

impl Gallery {
  /// Builds the block's variants with `f`: a labelled line each, keyed apart; disabled by
  /// [Ui::disabled] and sized by [Ui::control_size] on the line.
  fn variants(ui: &mut Ui, shown: Shown, mut f: impl FnMut(&mut Ui)) {
    let t = ui.theme();
    let sizes = [(ControlSize::Small, "Small"), (ControlSize::Large, "Large")];
    let sizes = if shown == Shown::Sized { &sizes[..] } else { &[] };
    if shown == Shown::Alone {
      ui.row(line(10.0).w(grow()), f);
      return;
    }

    ui.col(Style::new().w(grow()).gap(12.0), |ui| {
      let put = |ui: &mut Ui, k: &str, f: &mut dyn FnMut(&mut Ui)| {
        ui.key(k).row(line(12.0).w(grow()), |ui| {
          ui.col(Style::new().w(px(64.0)), |ui| ui.text(k, t.muted().nowrap()));
          ui.row(line(10.0).w(grow()), f);
        });
      };
      put(ui, "Enabled", &mut f);
      ui.disabled(true).col(Style::new().w(grow()), |ui| put(ui, "Disabled", &mut f));
      for &(size, k) in sizes {
        ui.control_size(size).col(Style::new().w(grow()), |ui| put(ui, k, &mut f));
      }
    });
  }

  /// The live block `b`, built by its group's module.
  pub(super) fn demo(&mut self, ui: &mut Ui, b: &Block) {
    match b.group {
      "Layout" => self.layout(ui, b.name),
      "Text" => self.text(ui, b.name),
      "Controls" => self.controls(ui, b.name),
      "Form" => self.form(ui, b.name),
      "List" => self.list(ui, b.name),
      "Containers" => self.containers(ui, b.name),
      "Presentation" => self.presentation(ui, b.name),
      // Modifiers, the last group.
      _ => self.modifiers(ui, b.name),
    }
  }
}
