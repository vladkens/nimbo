// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The modifier blocks, built live: disabled, control size, accessibility, keymaps, the
//! environment, groups, identity, focus and the pointer, widget parts, frame requests and the
//! theme.

use nimbo::ui::LabeledPairRole;
use nimbo::{Color, Cursor, Enabled, EnvironmentValue, Icon, Id, Shortcut, Tone, Ui, px};

use super::{Gallery, Shown, line, swatch};

/// The environment block's value: the tint of the swatches built while it is there, the
/// theme's raised colour by default.
#[derive(Clone, Copy)]
pub struct Tint(pub Color);

impl EnvironmentValue for Tint {
  fn default_value(ui: &Ui) -> Self {
    Self(ui.theme().raised)
  }
}

impl Gallery {
  /// The live modifier block `name`.
  pub(super) fn modifiers(&mut self, ui: &mut Ui, name: &str) {
    let t = ui.theme();
    match name {
      "Disabled" => Self::variants(ui, Shown::Plain, |ui| {
        ui.button("Save");
        ui.toggle(&mut self.on, ());
      }),
      "Control size" => Self::variants(ui, Shown::Sized, |ui| {
        ui.button("Save");
        ui.toggle(&mut self.on, ());
        ui.badge("Beta", Tone::Neutral);
      }),
      "Accessibility label" => {
        Self::variants(ui, Shown::Plain, |ui| _ = ui.a11y_label("Reset").icon_button(Icon::RESET))
      }

      // Swatches that take their colour from the environment, put for the row of the last two.
      "Environment" => Self::variants(ui, Shown::Alone, |ui| {
        let tile = |ui: &mut Ui| _ = ui.el(swatch(ui.environment_value::<Tint>().0));
        tile(ui);
        ui.environment(Tint(t.success)).row(line(10.0), |ui| (0..2).for_each(|_| tile(ui)));
      }),

      // Two widgets side by side disabled as one, the button after them enabled.
      "Group" => Self::variants(ui, Shown::Alone, |ui| {
        ui.disabled(true).group(|ui| {
          ui.button("Save");
          ui.toggle(&mut self.on, ());
        });
        ui.button("Open");
      }),

      // Keyed boxes: a key of its own each, the last a ready id.
      "Identity" => Self::variants(ui, Shown::Alone, |ui| {
        (0..3).for_each(|k| _ = ui.key(k).el(swatch(t.raised)));
        ui.with_id(Id::of("identity").key("box")).allows_hit_testing(false).el(swatch(t.accent));
      }),

      // A swatch Tab focuses with the hand over it, and a focusable one Tab passes by.
      "Focus and pointer" => Self::variants(ui, Shown::Alone, |ui| {
        let id = Id::of("focus and pointer");
        ui.with_id(id).focusable(true).pointer_style(Cursor::Pointer).el(swatch(t.accent));
        ui.with_id(id.key("not")).focusable(false).el(swatch(t.raised).focusable());
      }),

      // A part enabled in a disabled container, with the tip the widget was given.
      "Widget parts" => Self::variants(ui, Shown::Plain, |ui| {
        ui.button("Save");
        let tip = ui.help("Always enabled").take_help();
        if let Some(tip) = &tip {
          ui.help(tip);
        }

        ui.environment(Enabled(true)).button("Undo");
      }),

      // A bar that moves every frame.
      "Frame request" => Self::variants(ui, Shown::Alone, |ui| {
        ui.request_frame();
        let k = ui.now().as_secs_f32() % 1.0;
        ui.el(swatch(t.accent).w(px(28.0 + 120.0 * k)));
      }),

      // The slider named by the text before it; the badge by its own text.
      "Accessibility pair" => Self::variants(ui, Shown::Plain, |ui| {
        let pair = Id::of("volume");
        ui.a11y_labeled_pair(LabeledPairRole::Label, pair, |ui| ui.text("Volume", t.label()));
        ui.a11y_labeled_pair(LabeledPairRole::Content, pair, |ui| {
          _ = ui.slider(&mut self.level, 0.0..=100.0);
        });
        ui.badge("New", Tone::Accent);
      }),

      // The accent while the window is key, a grey while it isn't, as the switch beside it.
      "Controls theme" => Self::variants(ui, Shown::Alone, |ui| {
        let c = ui.controls_theme();
        ui.el(swatch(c.accent));
        ui.toggle(&mut self.on, ());
      }),

      // Light or dark as drawn: the theme's when the app set one, else the OS's.
      "Color scheme" => Self::variants(ui, Shown::Alone, |ui| {
        let name = if ui.color_scheme().is_dark() { "Dark" } else { "Light" };
        ui.text(name, t.label());
      }),

      _ => Self::variants(ui, Shown::Plain, |ui| {
        let keys = Shortcut::char('z').cmd().label();
        ui.text(format_args!("{keys} undoes, in the window's keymap"), t.label());
      }),
    }
  }
}
