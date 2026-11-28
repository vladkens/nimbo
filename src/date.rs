// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Calendar dates: a day of the Gregorian calendar ([Date]), always a real one, as a date picker
//! edits it ([crate::DatePicker]), today's in the user's time zone ([Date::today]); and how the
//! user's region writes one in numbers ([DateFormat]): the order of day, month and year and the
//! character between them, read from the system's short date ([DateFormat::system]).
//!
//! ```ignore
//! let start = Date::new(2026, 11, 27).unwrap();
//! assert_eq!(DateFormat::new(DateOrder::DayMonthYear, '.').format(start), "27.11.2026");
//! assert_eq!(start.to_string(), "2026-11-27");
//! ```

use std::fmt;
use std::ops::{Bound, RangeBounds};
use std::sync::OnceLock;

use crate::platform;

/// A day of the Gregorian calendar, years 1 to 9999: always a real one (no 31 April), ordered as
/// days are. Shown as ISO 8601 (`2026-11-27`); [DateFormat::format] writes it the user's way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
  year: u16,
  month: u8,
  day: u8,
}

impl Date {
  pub const MIN: Self = Self { year: 1, month: 1, day: 1 };
  pub const MAX: Self = Self { year: 9999, month: 12, day: 31 };

  /// Day `day` of month `month` (1 to 12) of `year` (1 to 9999); `None` unless a real one.
  pub const fn new(year: u32, month: u32, day: u32) -> Option<Self> {
    let ok = year >= 1 && year <= 9999 && month >= 1 && month <= 12;
    if !ok || day < 1 || day > days_in(year, month) {
      return None;
    }

    Some(Self { year: year as u16, month: month as u8, day: day as u8 })
  }

  /// Today in the user's time zone, by the system's clock.
  pub fn today() -> Self {
    let (y, m, d) = platform::today();
    Self::clamped(y, m, d)
  }

  pub const fn year(self) -> u32 {
    self.year as u32
  }

  /// 1 for January.
  pub const fn month(self) -> u32 {
    self.month as u32
  }

  pub const fn day(self) -> u32 {
    self.day as u32
  }

  /// The days of its month: 28 to 31.
  pub const fn days_in_month(self) -> u32 {
    days_in(self.year(), self.month())
  }

  /// The real date nearest to `year`, `month` and `day`, each within its bounds: the 31st of
  /// February is the month's last day.
  pub(crate) fn clamped(year: u32, month: u32, day: u32) -> Self {
    let (year, month) = (year.clamp(1, 9999), month.clamp(1, 12));
    let day = day.clamp(1, days_in(year, month));
    Self { year: year as u16, month: month as u8, day: day as u8 }
  }

  /// The day after, itself at [Date::MAX].
  fn next(self) -> Self {
    match (self.day() < self.days_in_month(), self.month() < 12) {
      (true, _) => Self { day: self.day + 1, ..self },
      (false, true) => Self { month: self.month + 1, day: 1, ..self },
      (false, false) => Self::clamped(self.year() + 1, 1, 1).max(self),
    }
  }

  /// The day before, itself at [Date::MIN].
  fn prev(self) -> Self {
    match (self.day > 1, self.month > 1) {
      (true, _) => Self { day: self.day - 1, ..self },
      (false, true) => Self::clamped(self.year(), self.month() - 1, 31),
      (false, false) if self.year > 1 => Self::clamped(self.year() - 1, 12, 31),
      (false, false) => self,
    }
  }

  /// The first and the last date of `range`, [Date::MIN] and [Date::MAX] where it is open; the
  /// two swapped when it runs backwards.
  pub(crate) fn bounds(range: &impl RangeBounds<Date>) -> (Date, Date) {
    let lo = match range.start_bound() {
      Bound::Included(&d) => d,
      Bound::Excluded(&d) => d.next(),
      Bound::Unbounded => Date::MIN,
    };

    let hi = match range.end_bound() {
      Bound::Included(&d) => d,
      Bound::Excluded(&d) => d.prev(),
      Bound::Unbounded => Date::MAX,
    };

    (lo.min(hi), hi.max(lo))
  }
}

/// ISO 8601, `2026-11-27`.
impl fmt::Display for Date {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
  }
}

const fn days_in(year: u32, month: u32) -> u32 {
  let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
  match month {
    2 if leap => 29,
    2 => 28,
    4 | 6 | 9 | 11 => 30,
    _ => 31,
  }
}

/// The order a region writes a date's numbers in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DateOrder {
  /// `27.11.2026`: most of Europe, Latin America, India.
  DayMonthYear,
  /// `11/27/2026`: the United States.
  MonthDayYear,
  /// `2026-11-27`: ISO 8601, China, Japan, Korea.
  YearMonthDay,
}

/// One of a date's numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
  Day,
  Month,
  Year,
}

/// How a date is written in numbers: the day and the month in two digits, the year in four, in
/// an [order](DateOrder) with a separator between them. A date picker writes and edits the
/// environment's ([crate::Ui::environment]): the system's in a window ([DateFormat::system]),
/// [DateFormat::ISO] in the harness and with no shell, so tests read the same everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DateFormat {
  pub order: DateOrder,
  pub separator: char,
}

impl DateFormat {
  /// ISO 8601's, `2026-11-27`.
  pub const ISO: Self = Self::new(DateOrder::YearMonthDay, '-');

  pub const fn new(order: DateOrder, separator: char) -> Self {
    Self { order, separator }
  }

  /// The user's, from the system's short date (macOS: System Settings › General › Language &
  /// Region; Windows: the regional format; Linux: the C library's `LC_TIME`), read once;
  /// [DateFormat::ISO] where it has none or one with no order of numbers to read.
  pub fn system() -> Self {
    static SYSTEM: OnceLock<DateFormat> = OnceLock::new();
    *SYSTEM.get_or_init(|| platform::date_pattern().and_then(|p| Self::of(&p)).unwrap_or(Self::ISO))
  }

  /// A pattern's order and separator: Unicode's date symbols (`dd.MM.yyyy`, `M/d/y`, which
  /// macOS and Windows give) or `strftime`'s (`%d.%m.%Y`, Linux's): where its day, month and
  /// year are, and the first mark between two of them (not a letter, a space or a quote).
  fn of(pattern: &str) -> Option<Self> {
    let p = pattern.replace("%D", "%m/%d/%y").replace("%F", "%Y-%m-%d");
    let at = |set: &[char]| p.find(set);
    let (d, m, y) = (at(&['d', 'e'])?, at(&['M', 'L', 'm'])?, at(&['y', 'Y'])?);
    let order = match (y < d.min(m), d < m) {
      (true, _) => DateOrder::YearMonthDay,
      (false, true) => DateOrder::DayMonthYear,
      (false, false) => DateOrder::MonthDayYear,
    };

    let (a, b) = (d.min(m).min(y), d.max(m).max(y));
    let mark = |c: &char| !c.is_alphanumeric() && !c.is_whitespace() && !"%'\"".contains(*c);
    let separator = p[a..b].chars().find(mark)?;
    Some(Self { order, separator })
  }

  /// Its numbers in its order.
  pub(crate) const fn parts(self) -> [Part; 3] {
    match self.order {
      DateOrder::DayMonthYear => [Part::Day, Part::Month, Part::Year],
      DateOrder::MonthDayYear => [Part::Month, Part::Day, Part::Year],
      DateOrder::YearMonthDay => [Part::Year, Part::Month, Part::Day],
    }
  }

  /// `date` written in it: `27.11.2026`.
  pub fn format(self, date: Date) -> String {
    let [a, b, c] = self.parts().map(|p| number(date, p));
    let s = self.separator;
    format!("{a}{s}{b}{s}{c}")
  }

  /// A date written in it, its three numbers in its order between any marks (`27.11.2026`,
  /// `27/11/2026`); `None` unless a real date.
  pub fn parse(self, s: &str) -> Option<Date> {
    let mut nums = s.split(|c: char| !c.is_ascii_digit()).filter(|n| !n.is_empty());
    let mut ymd = [0; 3];
    for p in self.parts() {
      ymd[p as usize] = nums.next()?.parse().ok()?;
    }

    let [day, month, year] = ymd;
    nums.next().is_none().then_some(())?;
    Date::new(year, month, day)
  }
}

/// `part` of `date` in its digits: the day and the month two, the year four.
pub(crate) fn number(date: Date, part: Part) -> String {
  match part {
    Part::Day => format!("{:02}", date.day),
    Part::Month => format!("{:02}", date.month),
    Part::Year => format!("{:04}", date.year),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn d(y: u32, m: u32, day: u32) -> Date {
    Date::new(y, m, day).unwrap()
  }

  #[test]
  fn only_real_dates_are_made() {
    assert!(Date::new(2024, 2, 29).is_some() && Date::new(2023, 2, 29).is_none());
    assert!(Date::new(2000, 2, 29).is_some() && Date::new(1900, 2, 29).is_none());
    assert!(Date::new(2026, 4, 31).is_none() && Date::new(2026, 13, 1).is_none());
    assert!(Date::new(0, 1, 1).is_none() && Date::new(10000, 1, 1).is_none());
    assert_eq!(Date::clamped(2026, 2, 31), d(2026, 2, 28));
    assert_eq!(Date::clamped(0, 0, 0), Date::MIN);
    assert!(d(2026, 11, 27) < d(2026, 12, 1) && d(2025, 12, 31) < d(2026, 1, 1));
    assert_eq!(d(2026, 3, 7).to_string(), "2026-03-07");
  }

  #[test]
  fn ranges_close_on_their_days() {
    let (a, b) = (d(2026, 2, 28), d(2027, 1, 1));
    assert_eq!(Date::bounds(&(a..b)), (a, d(2026, 12, 31)));
    assert_eq!(Date::bounds(&(..=a)), (Date::MIN, a));
    assert_eq!(Date::bounds(&(a..)), (a, Date::MAX));
    let after = (Bound::Excluded(a), Bound::Unbounded);
    assert_eq!(Date::bounds(&after).0, d(2026, 3, 1));
    assert_eq!(Date::bounds(&(b..=a)), (a, b), "backwards, turned round");
    assert_eq!((Date::MAX.next(), Date::MIN.prev()), (Date::MAX, Date::MIN));
  }

  #[test]
  fn patterns_give_their_order_and_mark() {
    use DateOrder::*;
    let of = |p| DateFormat::of(p).map(|f| (f.order, f.separator));
    assert_eq!(of("dd.MM.yyyy"), Some((DayMonthYear, '.')));
    assert_eq!(of("M/d/y"), Some((MonthDayYear, '/')));
    assert_eq!(of("y-MM-dd"), Some((YearMonthDay, '-')));
    assert_eq!(of("d. M. y"), Some((DayMonthYear, '.')), "Czech, spaced");
    assert_eq!(of("%d.%m.%Y"), Some((DayMonthYear, '.')));
    assert_eq!(of("%D"), Some((MonthDayYear, '/')));
    assert_eq!(of("%F"), Some((YearMonthDay, '-')));
    assert_eq!(of("y年M月d日"), None, "no mark but letters: ISO's");
    assert_eq!((of("EEEE"), of("dMy")), (None, None));
  }

  #[test]
  fn a_format_writes_and_reads_its_dates() {
    let de = DateFormat::new(DateOrder::DayMonthYear, '.');
    let us = DateFormat::new(DateOrder::MonthDayYear, '/');
    let day = d(2026, 3, 7);
    assert_eq!((de.format(day), us.format(day)), ("07.03.2026".into(), "03/07/2026".into()));
    assert_eq!(DateFormat::ISO.format(Date::MIN), "0001-01-01");
    assert_eq!((de.parse("7.3.2026"), us.parse(" 3/7/2026 ")), (Some(day), Some(day)));
    assert_eq!(
      (de.parse("31.4.2026"), de.parse("7.3"), de.parse("7.3.2026.1")),
      (None, None, None)
    );
  }
}
