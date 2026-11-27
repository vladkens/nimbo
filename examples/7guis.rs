// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The 7GUIs benchmark's tasks 2 to 7 (<https://7guis.github.io/7guis/tasks>) on toolbar tabs, so
//! nimbo sits beside other toolkits on the same programs (task 1, the counter, is
//! `examples/counter.rs`): a temperature converter, a flight booker, a timer, CRUD, a circle
//! drawer and cells. Each task is a struct of its state and a view over it: fields bound to
//! strings, a picker, `ui.disabled`, a dialog that tells and one of the app's own, time from the
//! clock, a filtered list, a canvas with a context menu and undo, and a sheet that scrolls both
//! ways with formulas.
//!
//! `cargo run --release --example 7guis [-- <task>] [--shot <file.png>]`: a task's slug
//! (`temperature`, `flight`, `timer`, `crud`, `circles`, `cells`) opens on its tab.

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "common/example.rs"]
mod example;

use std::collections::HashMap;
use std::time::Duration;

use nimbo::{
  Align, App, Axes, Binding, BorderedProminentButtonStyle, Command, Config, Cx, Dialog, Event,
  FormStyle, Frame, Icon, Id, Item, Key, List, Menu, MouseButton, NamedKey, Point, Quad, Rect,
  Response, Selection, Shortcut, Style, Tag, TextFieldConfiguration, TextFieldStyle, Transition,
  Ui, alpha, grow, px,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Task {
  #[default]
  Temperature,
  Flight,
  Timer,
  Crud,
  Circles,
  Cells,
}

pub const TASKS: [Tag<Task>; 6] = [
  Tag::new(Task::Temperature, "Temperature")
    .icon(Icon::new("M10 14V5a2 2 0 0 1 4 0v9a4 4 0 1 1-4 0ZM12 10v7")),
  Tag::new(Task::Flight, "Flight").icon(Icon::new("M4 6h16v14H4zM4 10h16M8 3v4M16 3v4")),
  Tag::new(Task::Timer, "Timer").icon(Icon::HISTORY),
  Tag::new(Task::Crud, "CRUD")
    .icon(Icon::new("M12 11a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7ZM5 20a7 7 0 0 1 14 0")),
  Tag::new(Task::Circles, "Circles")
    .icon(Icon::new("M3 10a6 6 0 1 0 12 0a6 6 0 1 0-12 0M14 17a3 3 0 1 0 6 0a3 3 0 1 0-6 0")),
  Tag::new(Task::Cells, "Cells").icon(Icon::new("M4 4h16v16H4zM4 10h16M4 15h16M10 4v16")),
];

pub const TABS: &str = "tasks";

/// A text field `w` wide that reads as `name`, ringed red while `invalid`; its answer out of the
/// column that sizes it: a container drops what its closure returns.
fn field(ui: &mut Ui, w: f32, name: &str, id: &str, text: &mut String, invalid: bool) -> Response {
  let mut r = Response::default();
  let ui = if invalid { ui.text_field_style(&Invalid) } else { ui };
  ui.col(Style::new().w(px(w)), |ui| r = ui.a11y_label(name).id(id).text_field(text, ""));
  r
}

/// A row of controls centred on each other, `gap` apart.
fn line(gap: f32) -> Style {
  Style::new().gap(gap).align(Align::Start, Align::Center)
}

/// A text field ringed and tinted red: text that is no number, no date. The theme's rounded
/// field takes no tint, so its frame is written again here.
struct Invalid;

impl TextFieldStyle for Invalid {
  fn make_body(&self, ui: &mut Ui, mut c: TextFieldConfiguration) {
    let t = ui.theme();
    let frame = Style::new().w(grow()).h(px(t.sizes().control_h())).pad_xy(6.0, 0.0);
    let frame = frame.align(Align::Start, Align::Center).radius(t.radius());
    let frame = frame.border(1.0, t.danger()).bg(alpha(t.danger(), 0.18));
    ui.row(frame.scroll(Axes::HORIZONTAL), |ui| c.text(ui, &t.field()));
  }
}

// MARK: Temperature

/// Task 2: Celsius and Fahrenheit, each field updating the other while its text is a number;
/// text that isn't turns its field red and leaves the other as it was.
#[derive(Default)]
pub struct Temperature {
  pub c: String,
  pub f: String,
}

/// A field's text as a number: a decimal point or comma, no infinity or NaN; `None` for empty
/// text too.
pub fn number(s: &str) -> Option<f64> {
  s.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A temperature as the fields show it: to a tenth, trimmed (`37.5`, `212`), whole from 10,000
/// up, where a tenth is noise, and in powers of ten past 10^15; never `-0`.
pub fn fmt(v: f64) -> String {
  let v = if v.abs() >= 10_000.0 { v.round() } else { (v * 10.0).round() / 10.0 };
  if v == 0.0 {
    "0".into()
  } else if v.abs() >= 1e15 {
    format!("{v:.3e}")
  } else {
    format!("{v}")
  }
}

impl Temperature {
  fn view(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    let unit = t.label();
    let bad = |s: &str| !s.trim().is_empty() && number(s).is_none();
    let (bad_c, bad_f) = (bad(&self.c), bad(&self.f));
    ui.row(line(6.0), |ui| {
      if field(ui, 100.0, "Celsius", "c", &mut self.c, bad_c).changed
        && let Some(c) = number(&self.c)
      {
        self.f = fmt(c * 9.0 / 5.0 + 32.0);
      }

      ui.text("°C", unit);
      ui.row(Style::new().pad_xy(10.0, 0.0), |ui| ui.text("=", t.muted()));
      if field(ui, 100.0, "Fahrenheit", "f", &mut self.f, bad_f).changed
        && let Some(f) = number(&self.f)
      {
        self.c = fmt((f - 32.0) * 5.0 / 9.0);
      }

      ui.text("°F", unit);
    });
  }
}

// MARK: Flight

/// A one-way flight or a return one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
  OneWay,
  Return,
}

const KINDS: [Tag<Kind>; 2] =
  [Tag::new(Kind::OneWay, "one-way flight"), Tag::new(Kind::Return, "return flight")];

/// Task 3: a one-way or return flight, its dates as DD.MM.YYYY. A date that doesn't parse turns
/// its field red; the return date is off for a one-way flight; Book is off while a date is bad or
/// the return is before the start, and tells what was booked.
pub struct Flight {
  pub kind: Kind,
  pub start: String,
  pub back: String,
  pub booked: bool,
}

impl Default for Flight {
  fn default() -> Self {
    let today = "27.03.2014".to_string();
    Self { kind: Kind::OneWay, start: today.clone(), back: today, booked: false }
  }
}

/// `DD.MM.YYYY` as (year, month, day), comparable; `None` unless a real date written so: the day
/// and month in one or two digits, the year in four (`11.03.12` is none).
pub fn parse_date(s: &str) -> Option<(u32, u32, u32)> {
  let parts: Vec<&str> = s.trim().split('.').collect();
  let digits = |p: &str, n: std::ops::RangeInclusive<usize>| {
    p.bytes().all(|c| c.is_ascii_digit()) && n.contains(&p.len())
  };
  let &[d, m, y] = parts.as_slice() else { return None };
  if !(digits(d, 1..=2) && digits(m, 1..=2) && digits(y, 4..=4)) {
    return None;
  }

  let (d, m, y) = (d.parse::<u32>().ok()?, m.parse::<u32>().ok()?, y.parse::<u32>().ok()?);
  let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
  let days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  let ok = (1..=12).contains(&m) && d >= 1 && d <= days[m as usize - 1];
  ok.then_some((y, m, d))
}

impl Flight {
  /// A date's field, red while its text is no date; the date as it was before this pass's edit,
  /// which the next pass sees.
  fn date(ui: &mut Ui, name: &str, id: &str, text: &mut String) -> Option<(u32, u32, u32)> {
    let date = parse_date(text);
    let ui = if date.is_none() { ui.text_field_style(&Invalid) } else { ui };
    ui.a11y_label(name).id(id).text_field(text, "DD.MM.YYYY");
    date
  }

  fn view(&mut self, ui: &mut Ui) {
    ui.col(Style::new().w(px(220.0)).gap(8.0), |ui| {
      ui.a11y_label("Flight").id("kind").picker(&mut self.kind, &KINDS);
      let start = Self::date(ui, "Start", "start", &mut self.start);
      let one_way = self.kind == Kind::OneWay;
      let back = ui.disabled(one_way).group(|ui| Self::date(ui, "Return", "back", &mut self.back));
      let ok = match (start, back) {
        (Some(_), _) if one_way => true,
        (Some(s), Some(b)) => b >= s,
        _ => false,
      };

      if ui.disabled(!ok).id("book").button("Book").clicked {
        self.booked = true;
      }
    });

    let told = match self.kind {
      Kind::OneWay => format!("You have booked a one-way flight on {}.", self.start.trim()),
      Kind::Return => {
        format!("You have booked a flight on {}, back on {}.", self.start.trim(), self.back.trim())
      }
    };

    Dialog::new("Booked").message(&told).show(ui.id("booked"), &mut self.booked);
  }
}

// MARK: Timer

/// Task 4: a gauge and a label of the time elapsed, a slider for the duration and a reset. The
/// time stops at the duration and goes on when the slider raises it.
pub struct Timer {
  /// Seconds counted so far, up to `duration`.
  pub elapsed: f32,
  pub duration: f32,
  /// The clock when `elapsed` was last counted, and whether it was counting then: a stopped
  /// timer draws no frames, so the gap until the next pass is no time it ran.
  last: Duration,
  running: bool,
}

impl Default for Timer {
  fn default() -> Self {
    Self { elapsed: 0.0, duration: 10.0, last: Duration::ZERO, running: true }
  }
}

impl Timer {
  fn view(&mut self, ui: &mut Ui) {
    // The time since the last pass: a second pass at the same time adds nothing.
    let now = ui.now();
    if self.running {
      self.elapsed = (self.elapsed + (now - self.last).as_secs_f32()).min(self.duration);
    }

    self.last = now;

    let t = ui.theme();
    let label = Style::new().w(px(110.0));
    ui.col(Style::new().w(px(320.0)).gap(10.0), |ui| {
      ui.row(line(8.0).w(grow()), |ui| {
        ui.row(label, |ui| ui.text("Elapsed Time:", t.label().nowrap()));
        ui.col(Style::new().w(grow()), |ui| {
          ui.a11y_label("Elapsed Time").id("gauge").progress(Some(self.elapsed / self.duration))
        });
      });

      ui.id("elapsed").text(format_args!("{:.1}s", self.elapsed), t.label());
      ui.row(line(8.0).w(grow()), |ui| {
        ui.row(label, |ui| ui.text("Duration:", t.label().nowrap()));
        ui.col(Style::new().w(grow()), |ui| {
          ui.a11y_label("Duration").id("duration").slider(&mut self.duration, 1.0..=30.0)
        });
      });
      if ui.id("reset").button("Reset").clicked {
        self.elapsed = 0.0;
      }
    });

    // After the slider and the reset: either starts it again.
    self.running = self.elapsed < self.duration;
    if self.running {
      ui.request_frame_in(Duration::from_millis(100));
    }
  }
}

// MARK: CRUD

pub struct Person {
  id: u64,
  pub name: String,
  pub surname: String,
}

impl Person {
  /// Whether the filter `q` finds this person: the start of the name, the surname, either's
  /// words, or "Surname, Name" or "Name Surname" as typed, in any case; empty finds everyone.
  pub fn matches(&self, q: &str) -> bool {
    let q = q.trim().to_lowercase();
    let (n, s) = (self.name.to_lowercase(), self.surname.to_lowercase());
    let whole = [format!("{s}, {n}"), format!("{n} {s}")];
    let words = n.split_whitespace().chain(s.split_whitespace());
    q.is_empty()
      || whole.iter().any(|w| w.starts_with(&q))
      || words.into_iter().any(|w| w.starts_with(&q))
  }

  /// Whether this is the person named `name` and `surname`, trimmed, in any case.
  fn is(&self, name: &str, surname: &str) -> bool {
    let same = |a: &str, b: &str| a.trim().to_lowercase() == b.trim().to_lowercase();
    same(&self.name, name) && same(&self.surname, surname)
  }
}

/// Task 5: people as "Surname, Name" in a list filtered by name, Name and Surname fields,
/// Create, and Update and Delete for the one selected, whose names fill the fields. Create and
/// Update are off for an empty field or someone's names, which a note tells.
pub struct Crud {
  pub people: Vec<Person>,
  next: u64,
  pub filter: String,
  pub name: String,
  pub surname: String,
  pub sel: Selection,
}

impl Default for Crud {
  fn default() -> Self {
    let (name, surname, filter) = (String::new(), String::new(), String::new());
    let mut app =
      Self { people: Vec::new(), next: 0, filter, name, surname, sel: Selection::default() };
    for (n, s) in [("Hans", "Emil"), ("Max", "Mustermann"), ("Roman", "Tisch")] {
      app.add(n, s);
    }

    app
  }
}

impl Crud {
  fn add(&mut self, name: &str, surname: &str) -> u64 {
    self.next += 1;
    let p = Person { id: self.next, name: name.trim().into(), surname: surname.trim().into() };
    self.people.push(p);
    self.next
  }

  /// The indices in `people` of those the filter shows.
  fn shown(&self) -> Vec<usize> {
    (0..self.people.len()).filter(|&i| self.people[i].matches(&self.filter)).collect()
  }

  /// The selected person's index in `people`, while the filter shows them.
  pub fn selected(&self) -> Option<usize> {
    let key = self.sel.cursor().filter(|k| self.sel.contains(*k))?;
    let i = self.people.iter().position(|p| p.id == key)?;
    self.people[i].matches(&self.filter).then_some(i)
  }

  /// Selects the person keyed `key` and fills the fields with their names; clears the filter
  /// first if it hides them.
  fn select(&mut self, key: u64) {
    let Some(i) = self.people.iter().position(|p| p.id == key) else { return };
    if !self.people[i].matches(&self.filter) {
      self.filter.clear();
    }

    let row = self.shown().iter().position(|&j| j == i).unwrap_or(0);
    self.sel.select(key, row);
    (self.name, self.surname) = (self.people[i].name.clone(), self.people[i].surname.clone());
  }

  /// Someone other than `but` with the fields' names.
  fn taken(&self, but: Option<usize>) -> Option<&Person> {
    let others = self.people.iter().enumerate().filter(|(i, _)| Some(*i) != but);
    others.map(|(_, p)| p).find(|p| p.is(&self.name, &self.surname))
  }

  fn view(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    let shown = self.shown();
    let sel = self.selected();
    let blank = self.name.trim().is_empty() || self.surname.trim().is_empty();
    let taken =
      self.taken(sel).map(|p| format!("{} {} is in the list already.", p.name, p.surname));
    // Nothing to update: the fields hold the selected person's names as they are.
    let same = sel.is_some_and(|i| {
      let p = &self.people[i];
      p.name == self.name.trim() && p.surname == self.surname.trim()
    });
    let create = !blank && self.taken(None).is_none();
    let update = sel.is_some() && !blank && taken.is_none() && !same;

    let label = Style::new().w(px(64.0)).align(Align::End, Align::Center);
    // The filter over the list, the fields beside it from its top.
    let list_w = 250.0;
    ui.col(Style::new().grow().gap(10.0), |ui| {
      ui.col(Style::new().w(px(list_w)), |ui| {
        ui.a11y_label("Filter").id("filter").search_field(&mut self.filter, "Filter by name")
      });

      ui.row(Style::new().grow().gap(20.0), |ui| {
        ui.col(Style::new().w(px(list_w)).h(grow()), |ui| {
          let people = &self.people;
          let r = List::new(shown.len(), 22.0)
            .key(&|i| people[shown[i]].id)
            .selection(&mut self.sel)
            .style(Style::new().grow().border(1.0, t.line()))
            .show(ui.a11y_label("People").id("people"), |ui, row| {
              let p = &people[shown[row.index]];
              ui.text(format_args!("{}, {}", p.surname, p.name), t.label());
            });
          // By a click or the keys: the names fill the fields to edit.
          if r.changed
            && let Some(i) = self.selected()
          {
            (self.name, self.surname) =
              (self.people[i].name.clone(), self.people[i].surname.clone());
          }
        });

        ui.col(Style::new().w(grow()).gap(8.0), |ui| {
          for (title, id, text) in
            [("Name", "name", &mut self.name), ("Surname", "surname", &mut self.surname)]
          {
            ui.row(line(8.0).w(grow()), |ui| {
              ui.row(label, |ui| ui.text(format_args!("{title}:"), t.label()));
              ui.col(Style::new().w(grow()).max_w(220.0), |ui| {
                ui.a11y_label(title).id(id).text_field(text, "")
              });
            });
          }

          // Under the fields, past the labels: the reason Create and Update are off.
          if let Some(note) = &taken {
            ui.row(Style::new().pads([0.0, 0.0, 0.0, 72.0]).w(grow()), |ui| {
              ui.id("taken").text(note, t.body().font(t.small()).color(t.secondary()))
            });
          }
        });
      });

      ui.row(line(8.0), |ui| {
        if ui.disabled(!create).id("create").button("Create").clicked {
          let (n, s) = (self.name.clone(), self.surname.clone());
          let key = self.add(&n, &s);
          self.select(key);
        }

        if ui.disabled(!update).id("update").button("Update").clicked
          && let Some(i) = sel
        {
          let p = &mut self.people[i];
          (p.name, p.surname) = (self.name.trim().into(), self.surname.trim().into());
          (self.name, self.surname) = (p.name.clone(), p.surname.clone());
        }

        if ui.disabled(sel.is_none()).id("delete").button("Delete").clicked
          && let Some(i) = sel
        {
          // The one after it takes the selection, or the one before at the end.
          let at = shown.iter().position(|&j| j == i).unwrap_or(0);
          let next = shown.get(at + 1).or(at.checked_sub(1).and_then(|a| shown.get(a)));
          let next = next.map(|&j| self.people[j].id);
          self.people.remove(i);
          self.sel.clear();
          match next {
            Some(key) => self.select(key),
            None => (self.name, self.surname) = (String::new(), String::new()),
          }
        }
      });
    });
  }
}

// MARK: Circles

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle {
  pub at: Point,
  pub d: f32,
}

/// A step Undo takes back: a circle added, or one resized from a diameter to another.
#[derive(Clone, Copy)]
enum Change {
  Add(Circle),
  Resize { i: usize, from: f32, to: f32 },
}

/// The diameter a click gives a new circle.
pub const NEW_D: f32 = 30.0;

/// Task 6: a click on the canvas adds a circle there, the one nearest the pointer is filled, a
/// right-click on it opens "Adjust diameter…", a dialog with a slider that resizes it live; Undo
/// and Redo step through the changes, a whole resize being one.
#[derive(Default)]
pub struct Circles {
  pub circles: Vec<Circle>,
  done: Vec<Change>,
  undone: Vec<Change>,
  /// The circle the dialog resizes and its diameter when it opened.
  editing: Option<(usize, f32)>,
  /// The circle a right press was on, which its menu acts on.
  target: Option<usize>,
  /// The focus before the dialog opened, given back as it closes.
  before: Option<Id>,
}

/// The diameter dialog's keys: Tab and Shift-Tab between the slider and Close, Return and Escape
/// close it (the slider's arrows stay its own).
const SHEET_KEYS: &[Binding] = &[
  Binding::new(Shortcut::new(Key::Named(NamedKey::Tab)), Command::Action("next")),
  Binding::new(Shortcut::new(Key::Named(NamedKey::Tab)).shift(), Command::Action("next")),
  Binding::new(Shortcut::new(Key::Named(NamedKey::Enter)), Command::Action("close")),
  Binding::new(Shortcut::new(Key::Named(NamedKey::Escape)), Command::Action("close")),
];

impl Circles {
  fn apply(&mut self, c: Change, undo: bool) {
    match (c, undo) {
      (Change::Add(c), false) => self.circles.push(c),
      (Change::Add(_), true) => _ = self.circles.pop(),
      (Change::Resize { i, from, to }, _) => self.circles[i].d = if undo { from } else { to },
    }
  }

  fn step(&mut self, undo: bool) {
    let (from, to) = match undo {
      true => (&mut self.done, &mut self.undone),
      false => (&mut self.undone, &mut self.done),
    };

    if let Some(c) = from.pop() {
      to.push(c);
      self.apply(c, undo);
    }
  }

  fn push(&mut self, c: Change) {
    self.apply(c, false);
    self.done.push(c);
    self.undone.clear();
  }

  /// The circle under `p` nearest its centre.
  pub fn nearest(&self, p: Point) -> Option<usize> {
    let dist = |c: &Circle| (c.at.x - p.x).hypot(c.at.y - p.y);
    let inside = self.circles.iter().enumerate().filter(|(_, c)| dist(c) <= c.d / 2.0);
    inside.min_by(|a, b| dist(a.1).total_cmp(&dist(b.1))).map(|(i, _)| i)
  }

  fn view<'a>(&'a mut self, ui: &mut Ui<'a>) {
    let t = ui.theme();
    let canvas = Id::of("canvas");
    let origin = ui.rect(canvas).map_or(Point::new(0.0, 0.0), |r| Point::new(r.x, r.y));
    let local = |p: Point| Point::new(p.x - origin.x, p.y - origin.y);
    // No hover while the dialog is up: the circle it resizes stays filled.
    let hot = match self.editing {
      Some((i, _)) => Some(i),
      None => ui.mouse().filter(|_| ui.hovered(canvas)).and_then(|p| self.nearest(local(p))),
    };

    if let Some(c) = ui.click(canvas).filter(|c| c.button == MouseButton::Left && hot.is_none()) {
      self.push(Change::Add(Circle { at: local(c.pos), d: NEW_D }));
    }

    // The menu acts on the circle under the right press: a window's menu answers in a later
    // pass, when the pointer may be elsewhere.
    let at = ui.context_press(canvas);
    if let Some(p) = at {
      self.target = self.nearest(local(p));
    }

    let menu = || Menu::new(vec![Item::button(0, "Adjust diameter…")]);
    if let Some(i) = self.target
      && ui.context_menu_at(canvas, at, menu).is_some()
    {
      self.editing = Some((i, self.circles[i].d));
      self.before = ui.focus_id();
      ui.focus(Some(Id::of("d")));
    }

    ui.col(Style::new().grow().gap(10.0), |ui| {
      ui.row(line(8.0), |ui| {
        if ui.disabled(self.done.is_empty()).id("undo").button("Undo").clicked {
          self.step(true);
        }

        if ui.disabled(self.undone.is_empty()).id("redo").button("Redo").clicked {
          self.step(false);
        }
      });

      let (circles, line, fill) = (self.circles.clone(), t.text(), alpha(t.text(), 0.25));
      let paint = move |f: &mut Frame, r: Rect| {
        f.push_clip(r);
        for (i, c) in circles.iter().enumerate() {
          let at = Rect::new(r.x + c.at.x - c.d / 2.0, r.y + c.at.y - c.d / 2.0, c.d, c.d);
          let bg = if Some(i) == hot { fill } else { [0.0; 4] };
          f.quad(Quad::new(at, bg).radius(c.d / 2.0).border(1.0, line));
        }

        f.pop_clip();
      };
      let area = Style::new().grow().radius(6.0).bg(t.bg()).border(1.0, t.line());
      ui.with_id(canvas).canvas(area, paint);
    });

    // The dialog: a card of the app's own over the modal backdrop, a slider and Close. Not an
    // alert's behaviour (`dialog_behavior`), which keeps the focus and the keys to its buttons:
    // the focus stays on the two, the slider keeps its arrows.
    let id = Id::of("diameter");
    let (slider, close, sheet) = (Id::of("d"), id.key("close"), id.key("sheet"));
    let Some((i, from)) = self.editing else { return };
    let mut done = ui.command(sheet) == Some(Command::Action("close"));
    if ui.command(sheet) == Some(Command::Action("next")) {
      ui.focus(Some(if ui.focused(slider) { close } else { slider }));
    }

    if !matches!(ui.focus_id(), Some(f) if f == slider || f == close) {
      ui.focus(Some(slider));
    }

    let shown = ui.animate(id.key("shown"), 1.0, Transition::smooth(1000.0 / 120.0));
    let c = self.circles[i];
    let d = &mut self.circles[i].d;
    ui.modal(id, shown, t.dialog().w(px(260.0)), |ui| {
      ui.keymap(SHEET_KEYS).with_id(sheet).col(Style::new().w(grow()).gap(10.0), |ui| {
        let title = format!("Adjust diameter of circle at ({:.0}, {:.0}).", c.at.x, c.at.y);
        ui.text(&title, t.body());
        ui.a11y_label("Diameter").with_id(slider).slider(d, 4.0..=120.0);
        let button = ui.with_id(close).button_style(&BorderedProminentButtonStyle);
        done |= button.button("Close").clicked;
      });
    });

    if done {
      let to = self.circles[i].d;
      self.editing = None;
      ui.focus(self.before.take());
      ui.animate(id.key("shown"), 0.0, Transition::linear(0.0));
      // The whole resize one step: from the diameter it had when the dialog opened.
      if to != from {
        self.circles[i].d = from;
        self.push(Change::Resize { i, from, to });
      }
    }
  }
}

// MARK: Cells

pub const COLS: usize = 26;
pub const ROWS: usize = 100;
const CELL_W: f32 = 72.0;
const CELL_H: f32 = 22.0;

/// Task 7: a sheet of 26 columns A–Z by 100 rows that scrolls both ways; a double click edits a
/// cell in a field (Enter or leaving it keeps the text, Escape drops it). A cell holds text, a
/// number or a formula: `=A1`, `=add(A1, 2)`, `=sum(A1:B3)` (add, sub, mul, div, sum, prod),
/// worked out as it shows from the cells it reads, so a change shows everywhere at once.
#[derive(Default)]
pub struct Cells {
  /// By (row, column).
  pub cells: HashMap<(usize, usize), String>,
  /// The cell in a field, and the text being edited.
  pub editing: Option<((usize, usize), String)>,
}

/// A formula's text and how far it is read.
/// How deep expressions nest, in one formula and through the cells it reads.
const DEEP: usize = 200;

struct Formula<'s>(&'s [u8], usize);

impl Formula<'_> {
  /// Skips spaces, then takes `c` if it is next.
  fn eat(&mut self, c: u8) -> bool {
    while self.0.get(self.1) == Some(&b' ') {
      self.1 += 1;
    }

    let yes = self.0.get(self.1) == Some(&c);
    self.1 += yes as usize;
    yes
  }

  fn take(&mut self, f: impl Fn(u8) -> bool) -> &str {
    let start = self.1;
    while self.0.get(self.1).is_some_and(|&c| f(c)) {
      self.1 += 1;
    }

    std::str::from_utf8(&self.0[start..self.1]).unwrap_or("")
  }

  /// `A12` as (row, column).
  fn cell(&mut self) -> Option<(usize, usize)> {
    // One letter, A to Z: `AA1` is no cell here.
    let col = match self.take(|c| c.is_ascii_uppercase()).as_bytes() {
      &[c] => c - b'A',
      _ => return None,
    };

    let row: usize = self.take(|c| c.is_ascii_digit()).parse().ok()?;
    (row < ROWS).then_some((row, col as usize))
  }

  /// The values an expression stands for: one, or a range's. `None` past [DEEP] expressions
  /// inside each other, through calls and cells alike: a pasted formula can't overflow the stack.
  fn values(&mut self, e: &mut Eval) -> Option<Vec<f64>> {
    if e.depth >= DEEP {
      return None;
    }

    e.depth += 1;
    let v = self.value_of(e);
    e.depth -= 1;
    v
  }

  fn value_of(&mut self, e: &mut Eval) -> Option<Vec<f64>> {
    self.eat(b' ');
    let c = *self.0.get(self.1)?;
    if c.is_ascii_digit() || c == b'-' || c == b'.' {
      let n = self.take(|c| c.is_ascii_digit() || c == b'-' || c == b'.');
      return Some(vec![n.parse().ok()?]);
    }

    if c.is_ascii_lowercase() {
      let f = self.take(|c| c.is_ascii_lowercase()).to_owned();
      self.eat(b'(').then_some(())?;
      let mut args = self.values(e)?;
      while self.eat(b',') {
        args.extend(self.values(e)?);
      }

      self.eat(b')').then_some(())?;
      let (first, rest) = args.split_first()?;
      let v = match f.as_str() {
        "add" | "sum" => args.iter().sum(),
        "mul" | "prod" => args.iter().product(),
        "sub" => rest.iter().fold(*first, |a, b| a - b),
        "div" => rest.iter().fold(*first, |a, b| a / b),
        _ => return None,
      };

      return Some(vec![v]);
    }

    let a = self.cell()?;
    if !self.eat(b':') {
      return Some(vec![e.value(a)?]);
    }

    let b = self.cell()?;
    // A range's text counts as 0, its errors as errors.
    let mut out = Vec::new();
    for r in a.0.min(b.0)..=a.0.max(b.0) {
      for c in a.1.min(b.1)..=a.1.max(b.1) {
        let formula = e.cells.cells.get(&(r, c)).is_some_and(|t| t.trim().starts_with('='));
        out.push(if formula { e.value((r, c))? } else { e.value((r, c)).unwrap_or(0.0) });
      }
    }

    Some(out)
  }
}

/// One evaluation of the sheet: each cell's value once, the cells being evaluated to find cycles.
struct Eval<'c> {
  cells: &'c Cells,
  memo: HashMap<(usize, usize), Option<f64>>,
  stack: Vec<(usize, usize)>,
  /// Expressions being evaluated inside each other.
  depth: usize,
}

impl Eval<'_> {
  /// A cell's number: its formula's value, or its text read as one; empty is 0. `None` for an
  /// error, a cycle, or nesting past [DEEP].
  fn value(&mut self, at: (usize, usize)) -> Option<f64> {
    if let Some(&v) = self.memo.get(&at) {
      return v;
    }

    if self.stack.contains(&at) {
      return None;
    }

    let text = self.cells.cells.get(&at).map_or("", |s| s.as_str()).trim();
    let v = match text.strip_prefix('=') {
      Some(f) => {
        self.stack.push(at);
        let mut p = Formula(f.as_bytes(), 0);
        let v = p.values(self).filter(|v| p.1 == f.len() && v.len() == 1).map(|v| v[0]);
        self.stack.pop();
        v
      }
      None if text.is_empty() => Some(0.0),
      None => text.parse().ok(),
    };

    self.memo.insert(at, v);
    v
  }
}

impl Cells {
  fn value(&self, at: (usize, usize)) -> Option<f64> {
    Eval { cells: self, memo: HashMap::new(), stack: Vec::new(), depth: 0 }.value(at)
  }

  /// What a cell shows: its value, its text, or `#ERR`.
  pub fn shown(&self, at: (usize, usize)) -> String {
    match self.cells.get(&at) {
      Some(t) if t.starts_with('=') => self.value(at).map_or("#ERR".into(), |v| v.to_string()),
      Some(t) => t.clone(),
      None => String::new(),
    }
  }

  fn view(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    let head =
      Style::new().w(px(CELL_W)).h(px(CELL_H)).center().bg(t.panel()).border(0.5, t.line());
    let cell = Style::new().w(px(CELL_W)).h(px(CELL_H)).pad_xy(4.0, 0.0).border(0.5, t.line());
    let cell = cell.align(Align::End, Align::Center).clip();
    let sheet = Style::new().grow().scroll(Axes::ALL).bg(t.bg()).border(1.0, t.line());

    let mut done = None;
    ui.id("sheet").scroll_area(sheet, |ui| {
      ui.row(Style::new(), |ui| {
        ui.el(head.w(px(36.0)));
        for c in 0..COLS {
          ui.row(head, |ui| ui.text((b'A' + c as u8) as char, t.label()));
        }
      });

      for r in 0..ROWS {
        ui.row(Style::new(), |ui| {
          ui.row(head.w(px(36.0)), |ui| ui.text(r, t.label()));
          for c in 0..COLS {
            match &mut self.editing {
              Some((at, text)) if *at == (r, c) => {
                let mut f = Response::default();
                ui.col(Style::new().w(px(CELL_W)).h(px(CELL_H)), |ui| {
                  let name = format!("{}{r}", (b'A' + c as u8) as char);
                  f = ui.autofocus().a11y_label(&name).id(("edit", r, c)).text_field(text, "");
                });

                if f.submitted || f.blurred {
                  done = Some(true);
                } else if f.cancelled {
                  done = Some(false);
                }
              }

              _ => {
                let id = Id::of(("cell", r, c));
                ui.with_id(id).row(cell, |ui| ui.text(self.shown((r, c)), t.body().nowrap()));
                if ui.click(id).is_some_and(|k| k.clicks == 2) {
                  let text = self.cells.get(&(r, c)).cloned().unwrap_or_default();
                  self.editing = Some(((r, c), text));
                }
              }
            }
          }
        });
      }
    });

    if let Some(keep) = done
      && let Some((at, text)) = self.editing.take()
      && keep
    {
      self.cells.insert(at, text);
    }
  }
}

// MARK: The app

/// The six tasks, each on its tab.
#[derive(Default)]
pub struct Seven {
  pub task: Task,
  pub temperature: Temperature,
  pub flight: Flight,
  pub timer: Timer,
  pub crud: Crud,
  pub circles: Circles,
  pub cells: Cells,
}

impl App for Seven {
  /// A move over the canvas redraws: the circle nearest the pointer changes inside one element.
  fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
    matches!(ev, Event::MouseMove { .. }) && cx.hovered(Id::of("canvas"))
  }

  fn view<'a>(&'a mut self, ui: &mut Ui<'a>) {
    ui.follow_system();
    let fs = FormStyle::plates(&ui.theme());
    ui.col(Style::new().grow().bg(fs.bg()), |ui| {
      ui.id(TABS).toolbar_tabs(&fs, &mut self.task, &TASKS);
      let page = Style::new().grow().pad(20.0).gap(12.0);
      ui.key(self.task as u8).col(page, |ui| match self.task {
        Task::Temperature => self.temperature.view(ui),
        Task::Flight => self.flight.view(ui),
        Task::Timer => self.timer.view(ui),
        Task::Crud => self.crud.view(ui),
        Task::Circles => self.circles.view(ui),
        Task::Cells => self.cells.view(ui),
      });
    });
  }
}

/// A task's slug, its tab's label in lowercase: `crud`.
pub fn slug(task: Task) -> String {
  let tag = TASKS.iter().find(|t| t.value() == task);
  tag.map(|t| t.label().to_lowercase()).unwrap_or_default()
}

/// The sheet the cells task shows at first: a few numbers and formulas over them.
pub fn sheet() -> Cells {
  let mut c = Cells::default();
  let at = [((0, 0), "5"), ((1, 0), "7"), ((2, 0), "=add(A0, A1)"), ((3, 0), "=sum(A0:A2)")];
  for (at, s) in at.into_iter().chain([((0, 1), "Total"), ((1, 1), "=mul(A3, 2)")]) {
    c.cells.insert(at, s.into());
  }

  c
}

fn main() {
  let mut args: Vec<String> = std::env::args().skip(1).collect();
  let task = TASKS.iter().map(|t| t.value()).find(|&t| args.first() == Some(&slug(t)));
  if task.is_some() {
    args.remove(0);
  }

  let app = Seven { task: task.unwrap_or_default(), cells: sheet(), ..Seven::default() };
  let cfg = Config::new("7GUIs", 620.0, 420.0).min_size(520.0, 360.0).unified();

  // The shot: each task part way through, as a person would leave it.
  example::run(cfg, app, &args, |h| {
    h.frame();
    match h.app().task {
      Task::Temperature => {
        h.click_id(Id::of("c"));
        h.type_text("100");
      }

      Task::Flight => {
        let f = &mut h.app_mut().flight;
        (f.kind, f.start, f.back) = (Kind::Return, "31.02.2014".into(), "26.03.2014".into());
      }
      Task::Timer => _ = h.wait(Duration::from_millis(3700)),

      Task::Crud => {
        h.click_id(Id::of("filter"));
        h.type_text("m");
        h.frame();
        let r = h.rect(Id::of("people")).unwrap();
        h.click(r.x + 30.0, r.y + 11.0);
      }

      Task::Circles => {
        let r = h.rect(Id::of("canvas")).unwrap();
        for (x, y) in [(70.0, 70.0), (150.0, 110.0), (260.0, 150.0), (160.0, 120.0)] {
          h.click(r.x + x, r.y + y);
        }

        h.move_to(r.x + 152.0, r.y + 112.0);
      }

      Task::Cells => {
        let r = h.rect(Id::of(("cell", 1usize, 0usize))).unwrap();
        h.press_with(r.x + 10.0, r.y + 10.0, 2, Default::default());
        h.release(r.x + 10.0, r.y + 10.0);
      }
    }

    h.frame();
    h.wait(Duration::from_millis(300));
  });
}
