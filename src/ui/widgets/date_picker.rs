// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The date picker's looks from the theme, as SwiftUI's date picker styles on the Mac
//! ([DatePickerStyle]): [FieldDatePickerStyle], the date in a text field's box, its selected
//! segment in the accent while focused; [StepperFieldDatePickerStyle], the same with a stepper
//! beside it, as AppKit's `NSDatePicker` (`.textFieldAndStepper`) in System Settings › General ›
//! Date & Time.

use super::button::{Variant, look};
use super::field::field_frame;
use crate::scene::{Frame, Point, Rect, alpha};
use crate::ui::{Align, ControlSize, DatePickerConfiguration, DatePickerStyle, Dim, Style, Ui, px};

/// The theme's date field, as SwiftUI's `.field`: day, month and year in a text field's box
/// ([crate::RoundedBorderTextFieldStyle]'s ground, edge and ring), as wide as they are, each
/// segment as wide as its widest number so the box keeps its width; the selected one on the
/// accent while focused, as AppKit selects a segment.
#[derive(Clone, Copy, Debug, Default)]
pub struct FieldDatePickerStyle;

/// The theme's date field with a stepper after it, as SwiftUI's `.stepperField` and System
/// Settings' date: [FieldDatePickerStyle] and, 4 points on, arrows a push button's ground
/// ([crate::BorderedButtonStyle]) as high as the field, the upper one stepping the selected
/// segment up, the lower one down. The shell's default.
#[derive(Clone, Copy, Debug, Default)]
pub struct StepperFieldDatePickerStyle;

impl DatePickerStyle for FieldDatePickerStyle {
  fn make_body(&self, ui: &mut Ui<'_>, c: DatePickerConfiguration) {
    field(ui, &c);
  }
}

impl DatePickerStyle for StepperFieldDatePickerStyle {
  fn make_body(&self, ui: &mut Ui<'_>, c: DatePickerConfiguration) {
    ui.row(Style::new().gap(4.0).align(Align::Start, Align::Center), |ui| {
      field(ui, &c);
      stepper(ui, &c);
    });
  }
}

/// The segments in the field's box, the root.
fn field(ui: &mut Ui, c: &DatePickerConfiguration) {
  let t = ui.theme();
  let style = t.field();
  let frame = field_frame(ui, &style, c.is_focused(), c.is_enabled());
  let frame = frame.w(Dim::Fit).h(px(t.sizes().control_h())).pad_xy(6.0, 0.0);
  // A segment as wide as its digits at their widest, so a change keeps the box's width.
  let text = t.label();
  let digit = (0..10).map(|d| ui.measure(&d.to_string(), text)).fold(0.0, f32::max);
  c.root(ui).row(frame, |ui| {
    for (i, s) in c.segments().iter().enumerate() {
      if i > 0 {
        ui.text(c.separator(), text);
      }

      let (ground, ink) = match s.selected {
        true => (t.accent(), t.on_accent()),
        false => ([0.0; 4], text.get_color()),
      };

      let w = (digit * s.digits as f32).ceil() + 2.0;
      let seg = Style::new().w(px(w)).align(Align::Center, Align::Center).bg(ground).radius(3.0);
      ui.with_id(s.id).row(seg, |ui| ui.text(&s.text, text.color(ink)));
    }
  });
}

/// The stepper: an upper and a lower arrow in a push button's ground, as high as the field.
fn stepper(ui: &mut Ui, c: &DatePickerConfiguration) {
  const W: f32 = 15.0;
  let t = ui.theme();
  let b = look(&t, Variant::Bordered, None, ui.environment_value::<ControlSize>());
  let h = t.sizes().control_h();
  let a = if c.is_enabled() { 1.0 } else { t.disabled };
  let frame = Style { bg: alpha(b.bg, b.bg[3] * a), ..b.frame };
  let frame = frame.w(px(W)).h(px(h)).min_w(W).min_h(h).pad(0.0).gap(0.0);

  // Each half's arrow, its ground the button's pressed one while held.
  let ink = alpha(t.text(), a * 0.85);
  let half = Style::new().w(px(W)).h(px(h / 2.0));
  let arrow = move |up: bool, held: bool| {
    let fill = if held { alpha(b.pressed, b.pressed[3] * a) } else { [0.0; 4] };
    move |f: &mut Frame, r: Rect| {
      f.rect(r, fill, 0.0);
      // A chevron 6 wide and 3 high, its middle the half's, nudged toward the other half.
      let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0 + if up { 1.0 } else { -1.0 });
      let dy = if up { 1.5 } else { -1.5 };
      let pts =
        [Point::new(cx - 3.0, cy + dy), Point::new(cx, cy - dy), Point::new(cx + 3.0, cy + dy)];
      f.polyline(&pts, 1.2, ink);
    }
  };

  let (up, down) = (c.is_incrementing(), c.is_decrementing());
  ui.col(frame.clip(), |ui| {
    c.increment(ui).canvas(half, arrow(true, up));
    c.decrement(ui).canvas(half, arrow(false, down));
  });
}

#[cfg(test)]
mod tests {
  use crate::app::App;
  use crate::scene::Size;
  use crate::ui::{Id, Ui};
  use crate::{Date, Harness};

  struct One(Date);

  impl App for One {
    fn view(&mut self, ui: &mut Ui) {
      ui.id("day").date_picker(&mut self.0);
    }
  }

  /// Under the shell a field and a stepper, whose arrows step the selected segment, the year
  /// first as the field has no focus; the field's width kept as the numbers change.
  #[test]
  fn the_shells_stepper_steps_the_selected_segment() {
    let mut h = Harness::new(One(Date::new(2026, 11, 27).unwrap()), Size::new(300.0, 100.0), 1.0);
    h.frame();
    let (id, w) = (Id::of("day"), h.rect(Id::of("day")).unwrap().w);
    assert!(h.click_id(id.key("increment")));
    assert_eq!((h.app().0, h.focus_id()), (Date::new(2027, 11, 27).unwrap(), Some(id)));
    h.click_id(id.key(("segment", 1)));
    h.click_id(id.key("decrement"));

    h.click_id(id.key("decrement"));
    assert_eq!(h.app().0, Date::new(2027, 9, 27).unwrap());
    h.frame();
    assert_eq!(h.rect(id).unwrap().w, w);
  }
}
