// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The date picker: its behaviour ([Ui::date_picker_behavior]), a date in three segments (day,
//! month and year in the environment's [DateFormat]), one selected and edited by typing its
//! digits or by the arrows, within a range and never an invalid date; and the [DatePicker] over
//! it, as SwiftUI's `DatePicker(selection:in:displayedComponents: .date)`, drawn by the
//! environment's [DatePickerStyle] ([Ui::date_picker_style]) from a [DatePickerConfiguration];
//! [PlainDatePickerStyle], the segments alone, unless the shell put its own.
//!
//! ```ignore
//! struct Boxed;
//!
//! impl DatePickerStyle for Boxed {
//!   fn make_body(&self, ui: &mut Ui, c: DatePickerConfiguration) {
//!     c.root(ui).row(Style::new().pad_xy(6.0, 2.0).bg(GROUND), |ui| {
//!       for (i, s) in c.segments().iter().enumerate() {
//!         if i > 0 { ui.text(c.separator(), TEXT) }
//!         let ground = if s.selected { SELECTED } else { [0.0; 4] };
//!         ui.with_id(s.id).row(Style::new().bg(ground), |ui| ui.text(&s.text, TEXT));
//!       }
//!     });
//!   }
//! }
//!
//! DatePicker::new(&mut self.start).range(Date::today()..).show(ui.date_picker_style(&Boxed));
//! ```

use std::ops::RangeBounds;

use crate::date::{Date, DateFormat, Part, number};
use crate::keys::{Command, Move};
use crate::text::Font;
use crate::ui::{
  Behavior, EnvironmentValue, Id, MouseButton, Press, Response, Role, Style, TextStyle, Ui,
};

/// A date picker's input this frame ([Ui::date_picker_behavior]) and its segments to draw.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct DatePickerBehavior {
  /// Its pointer and focus; `changed` when the date did.
  pub control: Behavior,
  /// Day, month and year in the format's order.
  pub segments: [DateSegment; 3],
  /// Between the segments: the format's.
  pub separator: char,
  /// A button held on [DatePickerBehavior::increment] with the pointer on it.
  pub incrementing: bool,
  /// One held on [DatePickerBehavior::decrement].
  pub decrementing: bool,
}

impl DatePickerBehavior {
  /// The part a click on raises the selected segment by one: a stepper's upper arrow.
  pub fn increment(&self) -> Id {
    self.control.id.key("increment")
  }

  /// The part a click on lowers it: a stepper's lower arrow.
  pub fn decrement(&self) -> Id {
    self.control.id.key("decrement")
  }
}

/// One of a date picker's numbers as it shows.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DateSegment {
  /// The element to build it under: a press on it selects it.
  pub id: Id,
  /// Its number in two digits (four for the year), or the digits typed into it so far.
  pub text: String,
  /// The most digits it takes: for a width that stays as it changes.
  pub digits: usize,
  /// The one the keys edit, while the picker has focus.
  pub selected: bool,
}

/// What a date picker keeps between frames: the segment selected and the digits typed into it.
#[derive(Default)]
struct Edit {
  at: usize,
  typed: String,
  /// The digits typed filled the segment before and moved on, nothing typed since: a mark stays.
  moved: bool,
  focused: bool,
}

impl Ui<'_> {
  /// A date picker `id` over `date` within `range` ([Date::bounds]), in the environment's
  /// [DateFormat]. A press on it focuses it, as a text field's does. While it has focus one of its
  /// segments is selected (the first as focus comes in; a press on a segment selects it): Left and
  /// Right move the selection, Up and Down step its number (the day and the month round within the
  /// month and the year, the year on), and digits typed set it, moving on once no other digit fits
  /// (`4` in a day, `12` or `01` in a month, four in a year), as does a mark (`.`, `/`) unless they
  /// just did; Backspace takes the last digit typed back. A year of one or two digits is in this
  /// century as the segment is left. A click on [DatePickerBehavior::increment] or
  /// [DatePickerBehavior::decrement] steps the selected segment. The date stays real (the day
  /// within its month) and in `range`, a date outside it brought in; `changed` when it moved.
  /// Assistive technology reads it as a text field with the date in the format, and sets it so.
  pub fn date_picker_behavior(
    &mut self,
    id: Id,
    date: &mut Date,
    range: impl RangeBounds<Date>,
  ) -> DatePickerBehavior {
    let (f, (lo, hi), was) = (self.environment_value::<DateFormat>(), Date::bounds(&range), *date);
    *date = was.clamp(lo, hi);
    let mut control = match self.begin(id, Role::TextField) {
      Some(b) => b,
      None => self.edit_date(id, date, f, (lo, hi)),
    };

    control.changed = *date != was;
    self.a11y(id, Role::TextField).text(&f.format(*date));
    let e = self.state::<Edit>(id);
    let (at, typed, on) = (e.at, e.typed.clone(), control.focused && !control.disabled);
    let segments = [0, 1, 2].map(|i| {
      let part = f.parts()[i];
      let selected = on && i == at;
      let text = if selected && !typed.is_empty() { typed.clone() } else { number(*date, part) };
      let digits = if part == Part::Year { 4 } else { 2 };
      DateSegment { id: id.key(("segment", i)), text, digits, selected }
    });
    let (up, down) = (id.key("increment"), id.key("decrement"));
    let on = !control.disabled;
    let (incrementing, decrementing) = (on && self.pressed(up), on && self.pressed(down));
    DatePickerBehavior { control, segments, separator: f.separator, incrementing, decrementing }
  }

  /// [Ui::date_picker_behavior]'s input, enabled.
  fn edit_date(&mut self, id: Id, date: &mut Date, f: DateFormat, range: (Date, Date)) -> Behavior {
    let mut e = std::mem::take(self.state::<Edit>(id));
    // A press anywhere on it focuses it, as a text field: AppKit's controls take focus by Tab.
    // Held, or let go since the view last ran (a click no view saw down).
    let segment = |i: usize| id.key(("segment", i));
    let ours = [id, segment(0), segment(1), segment(2), id.key("increment"), id.key("decrement")];
    let left = |p: &Press| p.button == MouseButton::Left;
    let ended = ours.into_iter().find(|&x| self.released(x).is_some_and(|(p, _)| left(&p)));
    let press = self.press().filter(left).map(|p| p.id).or(ended);
    if press.is_some_and(|p| ours.contains(&p)) && !self.focused(id) {
      self.focus(Some(id));
    }

    let focused = self.focused(id);
    // Focus coming in selects the first segment; going, it ends what was typed.
    match (focused, e.focused) {
      (true, false) => e.at = 0,
      (false, true) => e.commit(date, f, range),
      _ => {}
    }

    e.focused = focused;
    let pressed = (0..3).find(|&i| press == Some(segment(i)));
    if let Some(i) = pressed.filter(|&i| i != e.at) {
      e.commit(date, f, range);
      e.at = i;
    }

    let mut step =
      self.clicked(id.key("increment")) as i32 - self.clicked(id.key("decrement")) as i32;
    match self.command(id) {
      Some(Command::Move(Move::Left, _)) => e.go(date, f, range, -1),
      Some(Command::Move(Move::Right, _)) => e.go(date, f, range, 1),
      Some(Command::Move(Move::Up, _)) => step += 1,
      Some(Command::Move(Move::Down, _)) => step -= 1,
      Some(Command::Delete(_)) if !e.typed.is_empty() => _ = e.typed.pop(),
      Some(_) => self.pass(id),
      None => {}
    }

    if step != 0 {
      e.commit(date, f, range);
      *date = stepped(*date, f.parts()[e.at], step).clamp(range.0, range.1);
    }

    let typed = self.typed(id).map(str::to_owned);
    let used = typed.as_deref().map(|s| e.type_in(s, date, f, range));
    if used == Some(false) {
      self.pass(id);
    }

    if let Some(d) = self.a11y_value(id).and_then(|s| f.parse(s)) {
      e.typed.clear();
      *date = d.clamp(range.0, range.1);
    }

    *self.state::<Edit>(id) = e;
    self.pointer_state(id)
  }
}

impl Edit {
  /// Ends what was typed into the selected segment: a year of one or two digits in this
  /// century, the rest already set as it was typed.
  fn commit(&mut self, date: &mut Date, f: DateFormat, (lo, hi): (Date, Date)) {
    self.moved = false;
    let typed = std::mem::take(&mut self.typed);
    let Ok(n) = typed.parse::<u32>() else { return };
    if f.parts()[self.at] == Part::Year {
      let year = if typed.len() <= 2 { 2000 + n } else { n };
      *date = Date::clamped(year, date.month(), date.day()).clamp(lo, hi);
    }
  }

  /// Selects the segment `by` from the selected one, if there is one, ending what was typed.
  fn go(&mut self, date: &mut Date, f: DateFormat, range: (Date, Date), by: i32) {
    self.commit(date, f, range);
    self.at = (self.at as i32 + by).clamp(0, 2) as usize;
  }

  /// Text typed into the selected segment: digits set it, a mark moves on (unless the digits
  /// before it just did, as AppKit's). Whether it used any.
  fn type_in(&mut self, s: &str, date: &mut Date, f: DateFormat, range: (Date, Date)) -> bool {
    let mut used = false;
    for c in s.chars() {
      let moved = std::mem::take(&mut self.moved);
      if c.is_ascii_digit() {
        self.typed.push(c);
        used = true;
      } else if c.is_ascii_punctuation() || c == f.separator {
        if moved {
          used = true;
          continue;
        }

        used |= !self.typed.is_empty() || self.at < 2;
        self.go(date, f, range, 1);
        continue;
      } else {
        continue;
      }

      let part = f.parts()[self.at];
      let n: u32 = self.typed.parse().unwrap_or(0);
      // The most the segment takes, and whether a digit more could still fit: not past two in a
      // day or a month.
      let max = match part {
        Part::Day => date.days_in_month(),
        Part::Month => 12,
        Part::Year => 9999,
      };

      let digits = self.typed.len();
      let full = if part == Part::Year { digits >= 4 } else { digits >= 2 || n * 10 > max };
      if part != Part::Year && n > 0 {
        let (y, m, d) = (date.year(), date.month(), date.day());
        let (m, d) = if part == Part::Day { (m, n) } else { (n, d) };
        *date = Date::clamped(y, m, d).clamp(range.0, range.1);
      }

      if full {
        self.commit(date, f, range);
        self.moved = self.at < 2;
        self.at = (self.at + 1).min(2);
      }
    }

    used
  }
}

/// `date` with `part` stepped `by`: the day and the month round (within the month, the year),
/// the year goes on; the day kept within the month.
fn stepped(date: Date, part: Part, by: i32) -> Date {
  let round = |v: u32, n: u32| (v as i32 - 1 + by).rem_euclid(n as i32) as u32 + 1;
  let (y, m, d) = (date.year(), date.month(), date.day());
  match part {
    Part::Day => Date::clamped(y, m, round(d, date.days_in_month())),
    Part::Month => Date::clamped(y, round(m, 12), d),
    Part::Year => Date::clamped((y as i32 + by).max(1) as u32, m, d),
  }
}

/// How a date picker looks, as SwiftUI's `DatePickerStyle`: [DatePickerStyle::make_body] builds
/// its root ([DatePickerConfiguration::root]) and each segment under its id inside it. Chosen
/// with [Ui::date_picker_style] as `&'static`, a unit struct's `&MyStyle`.
pub trait DatePickerStyle: 'static {
  /// Builds the date picker `config` describes.
  fn make_body(&self, ui: &mut Ui<'_>, config: DatePickerConfiguration);
}

/// What a [DatePickerStyle] draws: the date picker's segments and its state this frame.
#[non_exhaustive]
pub struct DatePickerConfiguration {
  b: DatePickerBehavior,
}

impl DatePickerConfiguration {
  /// The date picker's id, its root's.
  pub fn id(&self) -> Id {
    self.b.control.id
  }

  /// Day, month and year in the format's order, each to build under its id.
  pub fn segments(&self) -> &[DateSegment; 3] {
    &self.b.segments
  }

  /// Between the segments: the format's.
  pub fn separator(&self) -> char {
    self.b.separator
  }

  pub fn is_hovered(&self) -> bool {
    self.b.control.hovered
  }

  /// A button held on its increment part ([DatePickerConfiguration::increment]), the pointer on
  /// it.
  pub fn is_incrementing(&self) -> bool {
    self.b.incrementing
  }

  pub fn is_decrementing(&self) -> bool {
    self.b.decrementing
  }

  /// Focused by the keyboard in the key window: the core rings its root, a segment is selected.
  pub fn is_focused(&self) -> bool {
    self.b.control.focused
  }

  /// Not disabled ([Ui::disabled]): it reads input, and is drawn so.
  pub fn is_enabled(&self) -> bool {
    !self.b.control.disabled
  }

  /// Makes the next element the date picker's root ([Ui::control]): under its id, taking focus
  /// by Tab and showing the pointing hand while enabled. Built once, the segments inside it.
  pub fn root<'u, 'a>(&self, ui: &'u mut Ui<'a>) -> &'u mut Ui<'a> {
    ui.control(&self.b.control)
  }

  /// Makes the next element the part a click on steps the selected segment up: a stepper's upper
  /// arrow, the pointing hand over it while enabled.
  pub fn increment<'u, 'a>(&self, ui: &'u mut Ui<'a>) -> &'u mut Ui<'a> {
    self.part(ui, self.b.increment())
  }

  /// [DatePickerConfiguration::increment]'s down.
  pub fn decrement<'u, 'a>(&self, ui: &'u mut Ui<'a>) -> &'u mut Ui<'a> {
    self.part(ui, self.b.decrement())
  }

  fn part<'u, 'a>(&self, ui: &'u mut Ui<'a>, id: Id) -> &'u mut Ui<'a> {
    let b = Behavior { id, ..self.b.control };
    ui.focusable(false).control(&b)
  }
}

/// A date, as SwiftUI's `DatePicker(selection:in:displayedComponents: .date)`: day, month and
/// year edited in place ([Ui::date_picker_behavior]), drawn by the environment's
/// [DatePickerStyle] ([Ui::date_picker_style]). [Ui::date_picker] is one with no range.
pub struct DatePicker<'d> {
  date: &'d mut Date,
  range: (Date, Date),
}

impl<'d> DatePicker<'d> {
  pub fn new(date: &'d mut Date) -> Self {
    Self { date, range: (Date::MIN, Date::MAX) }
  }

  /// The dates it takes, as SwiftUI's `in:`: `start..=end`, `Date::today()..` or `..=end`.
  pub fn range(self, range: impl RangeBounds<Date>) -> Self {
    Self { range: Date::bounds(&range), ..self }
  }

  /// Builds it under the id set for it, else its place: its behaviour, then the style's body.
  pub fn show(self, ui: &mut Ui) -> Response {
    let style = ui.environment_value::<&dyn DatePickerStyle>();
    let id = ui.take_id();
    let b = ui.date_picker_behavior(id, self.date, self.range.0..=self.range.1);
    let control = b.control;
    ui.control_body(id, |ui| style.make_body(ui, DatePickerConfiguration { b }));
    let mut r = ui.st.response(id);
    r.changed = control.changed;
    if control.disabled {
      (r.hovered, r.pressed, r.focused) = (false, false, false);
    }

    r
  }
}

/// The segments in the text colour of the colour scheme with no theme, the selected one on a
/// faint ground of it; dimmed when disabled. The default under the shell's.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlainDatePickerStyle;

impl DatePickerStyle for PlainDatePickerStyle {
  fn make_body(&self, ui: &mut Ui<'_>, c: DatePickerConfiguration) {
    let ink = if ui.color_scheme().is_dark() { 1.0 } else { 0.0 };
    let a = if c.is_enabled() { 0.85 } else { 0.25 };
    let text = TextStyle::new(Font::ui(13.0), [ink, ink, ink, a]);
    c.root(ui).row(Style::new().pad_xy(4.0, 2.0), |ui| {
      for (i, s) in c.segments().iter().enumerate() {
        if i > 0 {
          ui.text(c.separator(), text);
        }

        let ground = if s.selected { [ink, ink, ink, 0.2] } else { [0.0; 4] };
        ui.with_id(s.id).row(Style::new().bg(ground).radius(2.0), |ui| ui.text(&s.text, text));
      }
    });
  }
}

/// [PlainDatePickerStyle] where neither a container nor the window nor the shell put one.
impl EnvironmentValue for &'static dyn DatePickerStyle {
  fn default_value(_: &Ui) -> Self {
    &PlainDatePickerStyle
  }
}

/// [DateFormat::ISO] with no shell and in the harness; a window's runtime puts the system's
/// ([DateFormat::system]).
impl EnvironmentValue for DateFormat {
  fn default_value(_: &Ui) -> Self {
    DateFormat::ISO
  }
}

impl Ui<'_> {
  /// A [DatePicker] over `date`, any date.
  pub fn date_picker(&mut self, date: &mut Date) -> Response {
    DatePicker::new(date).show(self)
  }

  /// Draws the next date picker, or every one built in the next container, in `style`:
  /// `ui.date_picker_style(&FieldDatePickerStyle).date_picker(&mut day)`.
  pub fn date_picker_style(&mut self, style: &'static dyn DatePickerStyle) -> &mut Self {
    self.environment(style)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::a11y::Action;
  use crate::app::App;
  use crate::date::DateOrder;
  use crate::scene::Size;
  use crate::{Harness, Modifiers, NamedKey};

  const NONE: Modifiers = Modifiers::empty();

  fn d(y: u32, m: u32, day: u32) -> Date {
    Date::new(y, m, day).unwrap()
  }

  /// A date picker in a format, within a range, disabled at will, in the plain look.
  struct Day {
    date: Date,
    format: DateFormat,
    range: (Date, Date),
    off: bool,
  }

  impl Day {
    fn new(date: Date) -> Self {
      Self { date, format: DateFormat::ISO, range: (Date::MIN, Date::MAX), off: false }
    }
  }

  impl App for Day {
    fn view(&mut self, ui: &mut Ui) {
      let ui = ui.date_picker_style(&PlainDatePickerStyle).environment(self.format);
      let picker = DatePicker::new(&mut self.date).range(self.range.0..=self.range.1);
      picker.show(ui.disabled(self.off).id("day"));
    }
  }

  fn harness(app: Day) -> Harness<Day> {
    let mut h = Harness::new(app, Size::new(300.0, 100.0), 1.0);
    h.frame();
    h
  }

  #[test]
  fn typing_sets_each_segment_and_moves_on() {
    let mut h = harness(Day::new(d(2026, 11, 27)));
    h.key(NamedKey::Tab, NONE);
    assert_eq!(h.focus_id(), Some(Id::of("day")));
    // The year waits for four digits, the month moves on after a 2 (no 2x month).
    h.type_text("2027");
    h.type_text("2");
    assert_eq!(h.app().date, d(2027, 2, 27));
    // A day of 3 in February is full: 30 doesn't fit; the 31st a month later is the 3rd.
    h.type_text("3");
    assert_eq!(h.app().date, d(2027, 2, 3));
    h.key(NamedKey::ArrowLeft, NONE);
    h.type_text("12");
    h.type_text("31");
    assert_eq!(h.app().date, d(2027, 12, 31));
  }

  /// A mark right after the digits moved on stays in the segment they moved to.
  #[test]
  fn a_mark_after_a_full_segment_does_not_skip_the_next() {
    let mut h = harness(Day::new(d(2026, 11, 27)));
    h.key(NamedKey::Tab, NONE);
    h.type_text("2027-02-03");
    assert_eq!(h.app().date, d(2027, 2, 3));
    // Each its own key, as typed.
    h.key(NamedKey::ArrowLeft, NONE);
    h.key(NamedKey::ArrowLeft, NONE);
    for c in "2028-03-04".chars() {
      h.type_text(&c.to_string());
    }

    assert_eq!(h.app().date, d(2028, 3, 4));
  }

  /// Two digits fill a day or a month whatever they are: `01` is January, not the start of 10.
  #[test]
  fn two_digits_fill_a_segment() {
    let mut h = harness(Day::new(d(2026, 11, 27)));
    h.key(NamedKey::Tab, NONE);
    h.type_text("20270102");
    assert_eq!(h.app().date, d(2027, 1, 2));
  }

  #[test]
  fn a_short_year_is_this_centurys_as_its_segment_is_left() {
    let mut h = harness(Day::new(d(2026, 11, 27)));
    h.key(NamedKey::Tab, NONE);
    h.type_text("9");
    assert_eq!(h.app().date.year(), 2026, "not yet");
    h.type_text(".");
    assert_eq!(h.app().date.year(), 2009);
    // Backspace takes a digit back before it counts.
    h.key(NamedKey::ArrowLeft, NONE);
    h.type_text("31");
    h.key(NamedKey::Backspace, NONE);
    h.key(NamedKey::ArrowRight, NONE);
    assert_eq!(h.app().date, d(2003, 11, 27));
  }

  #[test]
  fn arrows_select_and_step_round_the_month_and_the_year() {
    let mut h = harness(Day {
      format: DateFormat::new(DateOrder::DayMonthYear, '.'),
      ..Day::new(d(2026, 1, 31))
    });
    h.key(NamedKey::Tab, NONE);
    h.key(NamedKey::ArrowUp, NONE);
    assert_eq!(h.app().date, d(2026, 1, 1), "the 31st rounds to the 1st");
    h.key(NamedKey::ArrowDown, NONE);
    h.key(NamedKey::ArrowRight, NONE);
    h.key(NamedKey::ArrowUp, NONE);
    assert_eq!(h.app().date, d(2026, 2, 28), "within February");

    h.key(NamedKey::ArrowDown, NONE);
    h.key(NamedKey::ArrowDown, NONE);
    assert_eq!(h.app().date, d(2026, 12, 28), "December, round");
    h.key(NamedKey::ArrowRight, NONE);
    h.key(NamedKey::ArrowRight, NONE);
    h.key(NamedKey::ArrowUp, NONE);
    assert_eq!(h.app().date, d(2027, 12, 28));
    h.frame();
    assert!(h.a11y().contains("text field value \"28.12.2027\" focused"), "{}", h.a11y());
  }

  #[test]
  fn the_range_holds_the_date() {
    let range = (d(2026, 11, 10), d(2026, 12, 20));
    let mut h = harness(Day { range, ..Day::new(d(2027, 1, 1)) });
    assert_eq!(h.app().date, range.1, "brought in");
    h.key(NamedKey::Tab, NONE);
    h.key(NamedKey::ArrowUp, NONE);
    assert_eq!(h.app().date, range.1);
    h.key(NamedKey::ArrowRight, NONE);
    h.type_text("11");
    h.type_text("3");
    assert_eq!(h.app().date, range.0, "the 3rd of November is before it");
    assert!(h.act(Id::of("day"), Action::SetValue("2026-12-01".into())));
    assert_eq!(h.app().date, d(2026, 12, 1));
  }

  #[test]
  fn a_press_selects_a_segment() {
    let mut h = harness(Day::new(d(2026, 11, 27)));
    let day = Id::of("day").key(("segment", 2));
    assert!(h.click_id(day));
    assert_eq!(h.focus_id(), Some(Id::of("day")));
    h.key(NamedKey::ArrowUp, NONE);
    assert_eq!(h.app().date, d(2026, 11, 28));

    h.app_mut().off = true;
    h.frame();
    h.key(NamedKey::ArrowUp, NONE);
    h.click_id(day);
    assert_eq!((h.app().date, h.focus_id()), (d(2026, 11, 28), None), "disabled");
  }
}
