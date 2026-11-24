// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The control blocks, built live: buttons, toggles, sliders, pickers and text fields in the
//! system's styles and in the gallery's own, progress, a badge, a hotkey recorder, a control
//! drawn on a button's behaviour and help marks.

use nimbo::ui::icon;
use nimbo::{
  Align, Axis, BorderedProminentButtonStyle, BorderlessButtonStyle, Button, ButtonConfiguration,
  ButtonRole, ButtonStyle, ButtonToggleStyle, CheckboxToggleStyle, FieldStyle, Icon,
  InlinePickerStyle, Picker, PickerConfiguration, PickerStyle, PlainSliderStyle,
  PlainTextFieldStyle, PlainToggleStyle, Quad, Rect, SegmentedPickerStyle, Slider,
  SliderConfiguration, SliderStyle, Style, Tag, TextField, TextFieldConfiguration, TextFieldStyle,
  TextStyle, ToggleConfiguration, ToggleStyle, Tone, Ui, alpha, grow, mix, px,
};

use super::{Gallery, SIZES, Shown, line};

/// A button style of the gallery's own: a capsule tinted with the accent, deeper while held,
/// faint when disabled.
struct Pill;

impl ButtonStyle for Pill {
  fn make_body(&self, ui: &mut Ui, mut c: ButtonConfiguration) {
    let t = ui.theme();
    let (k, ink) = match (c.is_enabled(), c.is_pressed()) {
      (false, _) => (0.06, alpha(t.accent, 0.4)),
      (true, pressed) => (if pressed { 0.3 } else { 0.15 }, t.accent),
    };

    let s = Style::new().h(px(24.0)).pad_xy(12.0, 0.0).gap(5.0).center().radius(12.0);
    ui.row(s.bg(alpha(t.accent, k)), |ui| c.label(ui, TextStyle::new(t.font, ink)));
  }
}

/// A toggle style of the gallery's own: a dot in the accent before the label, filled while on,
/// ringed while off; faint when disabled.
struct Dot;

impl ToggleStyle for Dot {
  fn make_body(&self, ui: &mut Ui, mut c: ToggleConfiguration) {
    let t = ui.theme();
    let k = if c.is_enabled() { 1.0 } else { 0.4 };
    let dot = Style::new().w(px(12.0)).h(px(12.0)).radius(6.0).border(1.5, alpha(t.accent, k));
    let dot = if c.is_on() { dot.bg(alpha(t.accent, k)) } else { dot };
    ui.row(Style::new().gap(6.0).center(), |ui| {
      ui.el(dot);
      c.label(ui, TextStyle::new(t.font, alpha(t.text, k)));
    });
  }
}

/// A slider style of the gallery's own: a capsule filled in the accent up to the value, a notch
/// at each step between its ends, no knob; faint when disabled.
struct Meter;

impl SliderStyle for Meter {
  fn make_body(&self, ui: &mut Ui, c: SliderConfiguration) {
    let t = ui.theme();
    let a = if c.is_enabled() { 1.0 } else { 0.4 };
    let (k, track, fill, notch) =
      (c.fraction(), alpha(t.accent, 0.15 * a), alpha(t.accent, a), alpha(t.bg, 0.8));
    let notches: Vec<f32> = c.steps().map(|(_, p)| p).filter(|p| *p > 0.0 && *p < 1.0).collect();
    c.root(ui).canvas(Style::new().w(grow()).h(px(8.0)), move |f, r| {
      f.rect(r, track, 4.0);
      f.rect(Rect::new(r.x, r.y, r.w * k, r.h), fill, 4.0);
      for p in &notches {
        f.rect(Rect::new(r.x + r.w * p - 0.5, r.y, 1.0, r.h), notch, 0.0);
      }
    });
  }
}

/// A text field style of the gallery's own: the text over a line, the accent's while focused,
/// which rings nothing else.
struct Underline;

impl TextFieldStyle for Underline {
  fn make_body(&self, ui: &mut Ui, mut c: TextFieldConfiguration) {
    let t = ui.theme();
    let line = if c.is_focused() { t.accent } else { t.line };
    let s = Style::new().w(grow()).h(px(24.0)).pad_xy(2.0, 0.0).borders([0.0, 0.0, 1.0, 0.0], line);
    let s = s.align(Align::Start, Align::Center).focusable_quiet().scroll(c.axis().into());
    let s = if c.is_enabled() { s } else { s.opacity(t.disabled) };
    let text = FieldStyle::new(TextStyle::new(t.font, t.text)).placeholder(t.faint);
    ui.row(s, |ui| c.text(ui, &text.caret(t.accent)));
  }
}

/// A picker style of the gallery's own: the options as capsules, the selected one filled with the
/// accent, faint when disabled.
struct Chips;

impl PickerStyle for Chips {
  fn make_body(&self, ui: &mut Ui, mut c: PickerConfiguration) {
    let t = ui.theme();
    let b = c.segments(ui);
    let k = if c.is_enabled() { 1.0 } else { 0.4 };
    ui.row(Style::new().gap(4.0), |ui| {
      for i in 0..c.len() {
        let on = c.selection() == Some(i);
        let (bg, ink) = match on {
          true => (alpha(t.accent, k), alpha(t.on_accent, k)),
          false => (alpha(t.accent, 0.12 * k), alpha(t.accent, k)),
        };

        let chip = Style::new().h(px(22.0)).pad_xy(10.0, 0.0).center().radius(11.0).bg(bg);
        ui.with_id(c.item_id(i))
          .row(b.pointer(chip), |ui| ui.text(c.label(i), TextStyle::new(t.font, ink)));
      }
    });
  }
}

/// The views a picker shows.
const VIEWS: [Tag<u8>; 3] = [Tag::new(0, "List"), Tag::new(1, "Grid"), Tag::new(2, "Columns")];

impl Gallery {
  /// The live control block `name`.
  pub(super) fn controls(&mut self, ui: &mut Ui, name: &str) {
    let t = ui.theme();
    match name {
      "Button" => Self::variants(ui, Shown::Sized, |ui| {
        ui.button("Bordered");
        ui.button_style(&BorderedProminentButtonStyle).button("Prominent");
        ui.button_style(&BorderlessButtonStyle).button("Borderless");
      }),
      "Button role" => Self::variants(ui, Shown::Sized, |ui| {
        Button::new("Delete").role(ButtonRole::Destructive).show(ui);
        let red = Button::new("Delete").role(ButtonRole::Destructive);
        red.show(ui.button_style(&BorderedProminentButtonStyle));
      }),

      "Icon button" => Self::variants(ui, Shown::Sized, |ui| {
        ui.a11y_label("Reset").icon_button(Icon::RESET);
        ui.icon_button(Icon::TRASH);
      }),

      // Two buttons borderless by their row, the one after it in the window's style.
      "Button style" => Self::variants(ui, Shown::Sized, |ui| {
        ui.button_style(&BorderlessButtonStyle).row(line(10.0), |ui| {
          ui.button("Cut");
          ui.button("Copy");
        });
        ui.button("Paste");
      }),
      "Custom button style" => Self::variants(ui, Shown::Plain, |ui| {
        ui.button_style(&Pill).button("Pill");
        Button::new("Open").icon(Icon::FOLDER).show(ui.button_style(&Pill));
      }),

      "Switch" => Self::variants(ui, Shown::Sized, |ui| _ = ui.toggle(&mut self.live, "Live")),
      "Check box" => Self::variants(ui, Shown::Plain, |ui| {
        _ = ui.toggle_style(&CheckboxToggleStyle).toggle(&mut self.wrap, "Wrap")
      }),
      "Button toggle" => Self::variants(ui, Shown::Sized, |ui| {
        ui.toggle_style(&ButtonToggleStyle).toggle(&mut self.bold, "Bold");
        ui.toggle_style(&ButtonToggleStyle).toggle(&mut self.italic, "Italic");
      }),

      // Two check boxes by their column, the switch after it in the window's style.
      "Toggle style" => Self::variants(ui, Shown::Plain, |ui| {
        ui.toggle_style(&CheckboxToggleStyle).col(Style::new().gap(6.0), |ui| {
          ui.toggle(&mut self.wrap, "Wrap");
          ui.toggle(&mut self.numbers, "Numbers");
        });
        ui.toggle(&mut self.live, "Live");
      }),
      "Custom toggle style" => Self::variants(ui, Shown::Plain, |ui| {
        ui.toggle_style(&Dot).toggle(&mut self.live, "Live");
        ui.toggle_style(&PlainToggleStyle).toggle(&mut self.wrap, "Plain");
      }),

      // A slider fills the width it is offered: 160 here.
      "Slider" => Self::variants(ui, Shown::Plain, |ui| {
        ui.col(Style::new().w(px(160.0)), |ui| {
          _ = ui.a11y_label("Zoom").slider(&mut self.zoom, 0.0..=200.0)
        });
        ui.text(format_args!("{:.0}%", self.zoom), t.muted().nowrap());
      }),
      "Slider steps" => Self::variants(ui, Shown::Plain, |ui| {
        let label = |v: f32| [10.0, 50.0, 90.0].contains(&v).then(|| format!("{v}%"));
        let s = Slider::new(&mut self.level, 10.0..=90.0).step(10.0).tick_label(label);
        ui.col(Style::new().w(px(160.0)), |ui| _ = s.show(ui.a11y_label("Level")));
        ui.text(format_args!("{:.0}%", self.level), t.muted().nowrap());
      }),

      // A plain slider by its column, the one after it in the window's style.
      "Slider style" => Self::variants(ui, Shown::Plain, |ui| {
        ui.col(Style::new().w(px(160.0)).gap(8.0), |ui| {
          ui.slider_style(&PlainSliderStyle).col(Style::new().w(grow()), |ui| {
            _ = ui.a11y_label("Zoom").slider(&mut self.zoom, 0.0..=200.0)
          });
          _ = ui.a11y_label("Zoom").slider(&mut self.zoom, 0.0..=200.0);
        });
      }),
      "Custom slider style" => Self::variants(ui, Shown::Plain, |ui| {
        let s = Slider::new(&mut self.level, 10.0..=90.0).step(10.0);
        ui.col(Style::new().w(px(160.0)), |ui| {
          _ = s.show(ui.slider_style(&Meter).a11y_label("Level"))
        });
        ui.text(format_args!("{:.0}%", self.level), t.muted().nowrap());
      }),

      "Picker" => Self::variants(ui, Shown::Plain, |ui| {
        let p = Picker::new(&mut self.size).options(&SIZES[..1]).divider();
        p.options(&SIZES[1..]).show(ui.a11y_label("Size"));
      }),
      "Segmented picker" => Self::variants(ui, Shown::Plain, |ui| {
        let views = [
          Tag::new(0, "List").count(128),
          Tag::new(1, "Grid").count(12),
          Tag::new(2, "Columns").count(3),
        ];
        ui.picker_style(&SegmentedPickerStyle).picker(&mut self.view, &views);
      }),

      // Pickers inline by their row, the one after it in the window's style.
      "Picker style" => Self::variants(ui, Shown::Plain, |ui| {
        ui.picker_style(&InlinePickerStyle).row(line(10.0), |ui| {
          _ = ui.a11y_label("Size").picker(&mut self.size, &SIZES);
        });
        ui.a11y_label("Size").picker(&mut self.size, &SIZES);
      }),
      "Custom picker style" => Self::variants(ui, Shown::Plain, |ui| {
        ui.picker_style(&Chips).picker(&mut self.view, &VIEWS);
      }),

      "Text field" => Self::variants(ui, Shown::Plain, |ui| {
        ui.col(Style::new().w(px(200.0)), |ui| _ = ui.text_field(&mut self.name, "Name"));
      }),
      // A multi-line field fills the height its parent gives it.
      "Multi-line field" => Self::variants(ui, Shown::Plain, |ui| {
        ui.col(Style::new().w(px(200.0)).h(px(60.0)), |ui| {
          _ = TextField::new(&mut self.notes, "Notes").axis(Axis::Vertical).show(ui)
        });
      }),
      "Secure field" => Self::variants(ui, Shown::Plain, |ui| {
        ui.col(Style::new().w(px(200.0)), |ui| _ = ui.secure_field(&mut self.key, "Password"));
      }),
      "Search field" => Self::variants(ui, Shown::Plain, |ui| {
        ui.col(Style::new().w(px(200.0)), |ui| _ = ui.search_field(&mut self.query, "Search"));
      }),

      // Two fields plain by their column, the one after it in the window's style.
      "Text field style" => Self::variants(ui, Shown::Plain, |ui| {
        let col = Style::new().w(px(120.0)).gap(6.0);
        ui.text_field_style(&PlainTextFieldStyle).col(col, |ui| {
          ui.text_field(&mut self.name, "Name");
          ui.text_field(&mut self.city, "City");
        });
        ui.col(Style::new().w(px(160.0)), |ui| _ = ui.text_field(&mut self.email, "E-mail"));
      }),
      "Custom text field style" => Self::variants(ui, Shown::Plain, |ui| {
        ui.col(Style::new().w(px(200.0)), |ui| {
          _ = ui.text_field_style(&Underline).text_field(&mut self.city, "City")
        });
      }),

      "Progress" => Self::variants(ui, Shown::Plain, |ui| {
        ui.col(Style::new().w(px(150.0)), |ui| ui.progress(Some(0.42)));
        ui.col(Style::new().w(px(150.0)), |ui| ui.progress(None));
      }),
      "Spinner" => Self::variants(ui, Shown::Sized, |ui| _ = ui.spinner()),
      "Badge" => Self::variants(ui, Shown::Sized, |ui| {
        ui.badge(128, Tone::Neutral);
        ui.badge("New", Tone::Accent);
        ui.badge("+5", Tone::Success);
        ui.badge("-4", Tone::Danger);
        ui.badge("Conflict", Tone::Warning);
      }),

      "Hotkey recorder" => Self::variants(ui, Shown::Plain, |ui| {
        ui.hotkey_recorder(&mut self.hotkey);
        // The ⊗ that clears one, which the plates themes leave to a reset button.
        let t = ui.theme();
        ui.set_theme(nimbo::Theme { hotkey_recorder_clear: true, ..t });
        ui.hotkey_recorder(&mut self.hotkey);
        ui.set_theme(t);
      }),

      // A round button the app draws on a button's behaviour: Tab focuses it, the hand over it.
      "Control behavior" => Self::variants(ui, Shown::Plain, |ui| {
        let id = ui.take_id();
        let b = ui.button_behavior(id);
        let fill = match (b.pressed, b.hovered) {
          (true, _) => t.accent_hover(),
          (_, true) => mix(t.accent, t.accent_hover(), 0.5),
          _ => t.accent,
        };

        let (fill, mark) = match b.disabled {
          true => (alpha(fill, t.disabled), alpha([1.0; 4], t.disabled)),
          false => (fill, [1.0; 4]),
        };

        let ring = if b.focused { t.focus } else { [0.0; 4] };
        let s = Style::new().w(px(28.0)).h(px(28.0));

        ui.control(&b).canvas(s, move |f, r| {
          f.quad(Quad::new(r, fill).radius(14.0).border(2.0, ring));
          icon::draw(f, Icon::PLAY, Rect::new(r.x + 7.0, r.y + 7.0, 14.0, 14.0), mark);
        });
      }),

      "Help marks" => Self::variants(ui, Shown::Plain, |ui| {
        ui.help("An ⓘ explains what one line can't").info_button();
        ui.help("A warning tells what is wrong").warning_button();
        ui.help("Opens the page's help").help_button();
      }),
      _ => unreachable!("{name} is not a control block"),
    }
  }
}
