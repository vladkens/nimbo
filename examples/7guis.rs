// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The 7GUIs benchmark's tasks 2 to 7 (<https://7guis.github.io/7guis/tasks>) on toolbar tabs, so
//! nimbo sits beside other toolkits on the same programs (task 1, the counter, is
//! `examples/counter.rs`): a temperature converter, a flight booker, a timer, CRUD, a circle
//! drawer and cells. Each task is a struct of its state and a view over it: fields bound to
//! strings, a picker, date pickers in a range, `ui.disabled`, a dialog that tells and one of the
//! app's own, time from the clock, a filtered list, a canvas with a context menu and undo, and a
//! sheet that scrolls both ways with formulas.
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
  Align, App, Axes, Binding, BorderedProminentButtonStyle, CMD, Command, Config,
  ContentMarginPlacement, Cx, Date, DateFormat, DatePicker, Dialog, Enabled, Event, Float,
  FormStyle, Frame, Icon, Id, Item, Key, KeyEvent, List, Menu, MouseButton, NamedKey, Point, Quad,
  Rect, Response, Selection, Shortcut, Style, Tag, TextFieldConfiguration, TextFieldStyle,
  Transition, Ui, alpha, grow, mix, px,
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

/// The labels' column of a form in columns ([labeled]): as wide as the widest of `titles` with
/// its colon.
fn column(ui: &mut Ui, titles: &[&str]) -> f32 {
  let t = ui.theme();
  titles.iter().map(|s| ui.measure(&format!("{s}:"), t.label())).fold(0.0, f32::max).ceil()
}

/// A row of a form in columns, as SwiftUI's `.formStyle(.columns)` on the Mac: `title` and a
/// colon at the right of a column `w` wide, dimmed when the row is disabled, and what `control`
/// builds 8 after it; an empty title leaves the column empty, for a button under the controls.
fn labeled<R>(ui: &mut Ui, w: f32, title: &str, control: impl FnOnce(&mut Ui) -> R) {
  let t = ui.theme();
  ui.row(line(8.0).w(grow()), |ui| {
    let on = ui.environment_value::<Enabled>().0;
    let text = if on { t.label() } else { t.label().color(t.tertiary()) };
    let column = Style::new().w(px(w)).align(Align::End, Align::Center);
    ui.row(column, |ui| {
      if !title.is_empty() {
        ui.text(format_args!("{title}:"), text.nowrap());
      }
    });
    _ = control(ui);
  });
}

/// A text field ringed and tinted red: text that is no number, no date. The theme's rounded
/// field takes no tint, so its frame is written again here.
struct Invalid;

impl TextFieldStyle for Invalid {
  fn make_body(&self, ui: &mut Ui, mut c: TextFieldConfiguration) {
    let t = ui.theme();
    let h = t.sizes().control_h();
    let frame = Style::new().w(grow()).min_h(h).pad_xy((h / 3.0).round(), 4.0);
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

/// Task 3: a one-way or return flight from today on, its dates in date pickers; the return date
/// is off for a one-way flight and never before the departure, as 7GUIs asks, and comes along
/// as the departure moves past it. Book tells what was booked.
pub struct Flight {
  pub kind: Kind,
  pub start: Date,
  pub back: Date,
  pub booked: bool,
}

impl Default for Flight {
  fn default() -> Self {
    let today = Date::today();
    Self { kind: Kind::OneWay, start: today, back: today, booked: false }
  }
}

impl Flight {
  fn view(&mut self, ui: &mut Ui) {
    let w = column(ui, &["Flight", "Departure", "Return"]);
    let one_way = self.kind == Kind::OneWay;
    ui.col(Style::new().gap(10.0), |ui| {
      labeled(ui, w, "Flight", |ui| {
        ui.a11y_label("Flight").id("kind").picker(&mut self.kind, &KINDS)
      });
      labeled(ui, w, "Departure", |ui| {
        let date = DatePicker::new(&mut self.start).range(Date::today()..);
        date.show(ui.a11y_label("Departure").id("start"))
      });

      let start = self.start;
      labeled(ui.disabled(one_way), w, "Return", |ui| {
        DatePicker::new(&mut self.back).range(start..).show(ui.a11y_label("Return").id("back"))
      });

      // The spec's rule, though the return's range keeps it.
      let ok = one_way || self.back >= self.start;
      labeled(ui, w, "", |ui| {
        if ui.disabled(!ok).id("book").button("Book").clicked {
          self.booked = true;
        }
      });
    });

    let f = ui.environment_value::<DateFormat>();
    let told = match self.kind {
      Kind::OneWay => format!("You have booked a one-way flight on {}.", f.format(self.start)),
      Kind::Return => {
        let (start, back) = (f.format(self.start), f.format(self.back));
        format!("You have booked a flight on {start}, back on {back}.")
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

    // The gauge and the slider as wide, their ends in line.
    let w = column(ui, &["Elapsed Time", "Duration"]);
    let (t, wide) = (ui.theme(), Style::new().w(px(240.0)));
    ui.col(Style::new().gap(10.0), |ui| {
      let done = self.elapsed / self.duration;
      labeled(ui, w, "Elapsed Time", |ui| {
        ui.col(wide, |ui| ui.a11y_label("Elapsed Time").id("gauge").progress(Some(done)))
      });
      labeled(ui, w, "", |ui| {
        ui.id("elapsed").text(format_args!("{:.1}s", self.elapsed), t.label())
      });
      labeled(ui, w, "Duration", |ui| {
        ui.col(wide, |ui| {
          ui.a11y_label("Duration").id("duration").slider(&mut self.duration, 1.0..=30.0)
        })
      });
      labeled(ui, w, "", |ui| {
        if ui.id("reset").button("Reset").clicked {
          self.elapsed = 0.0;
        }
      });
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

    // The filter over the list, the fields beside it from its top.
    let w = column(ui, &["Name", "Surname"]);
    let list_w = 250.0;
    // A text field's box: its ground, edge and corners; the rows a little in from its ends.
    let boxed = Style::new().grow().pad_xy(0.0, 5.0).radius(t.radius());
    let boxed = boxed.bg(t.field_fill()).border(1.0, t.field_line());
    ui.col(Style::new().grow().gap(10.0), |ui| {
      ui.col(Style::new().w(px(list_w)), |ui| {
        ui.a11y_label("Filter").id("filter").search_field(&mut self.filter, "Filter by name")
      });

      ui.row(Style::new().grow().gap(20.0), |ui| {
        ui.col(Style::new().w(px(list_w)).h(grow()), |ui| {
          let people = &self.people;
          let r = List::new(shown.len(), 24.0)
            .key(&|i| people[shown[i]].id)
            .selection(&mut self.sel)
            .style(boxed)
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

        ui.col(Style::new().w(grow()).gap(10.0), |ui| {
          for (title, id, text) in
            [("Name", "name", &mut self.name), ("Surname", "surname", &mut self.surname)]
          {
            labeled(ui, w, title, |ui| {
              let field = Style::new().w(grow()).max_w(220.0);
              ui.col(field, |ui| ui.a11y_label(title).id(id).text_field(text, ""))
            });
          }

          // Under the fields: the reason Create and Update are off.
          if let Some(note) = &taken {
            labeled(ui, w, "", |ui| {
              let note_style = t.body().font(t.small()).color(t.secondary());
              ui.col(Style::new().w(grow()).max_w(220.0), |ui| {
                ui.id("taken").text(note, note_style)
              })
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

      // A return flight, its return's day being stepped.
      Task::Flight => {
        h.app_mut().flight.kind = Kind::Return;
        h.frame();
        h.click_id(Id::of("back").key(("segment", 2usize)));
        for _ in 0..3 {
          h.key(Key::Named(NamedKey::ArrowUp), Default::default());
        }
      }
      Task::Timer => _ = h.wait(Duration::from_millis(3700)),

      Task::Crud => {
        h.click_id(Id::of("filter"));
        h.type_text("m");
        h.frame();
        let r = h.rect(Id::of("people")).unwrap();
        h.click(r.x + 30.0, r.y + 17.0);
      }

      Task::Circles => {
        let r = h.rect(Id::of("canvas")).unwrap();
        for (x, y) in [(70.0, 70.0), (150.0, 110.0), (260.0, 150.0), (160.0, 120.0)] {
          h.click(r.x + x, r.y + y);
        }

        h.move_to(r.x + 152.0, r.y + 112.0);
      }

      Task::Cells => {
        let r = h.rect(Id::of(("cell", 1usize, 1usize))).unwrap();
        h.click(r.x + 10.0, r.y + 10.0);
      }
    }

    h.frame();
    h.wait(Duration::from_millis(300));
  });
}

// MARK: Cells

// A spreadsheet of formulas over the cells they read, worked out as each cell shows, with a
// selection the keys move, edits in place and in a formula bar, and headers that stay in view as
// the sheet scrolls.

pub const COLS: usize = 26;
pub const ROWS: usize = 100;
const CELL_W: f32 = 72.0;
const CELL_H: f32 = 22.0;
/// The row numbers' column.
const HEAD_W: f32 = 36.0;

/// Task 7: a spreadsheet of 26 columns A–Z by 100 rows, each cell text, a number or a formula:
/// `=A1`, `=add(A1, 2)`, `=sum(A1:B3)` (add, sub, mul, div, sum, prod), worked out as it shows,
/// so a change shows everywhere at once.
#[derive(Default)]
pub struct Cells {
  /// By (row, column).
  pub cells: HashMap<(usize, usize), String>,
  /// The selected cell.
  pub sel: (usize, usize),
  /// The selected cell's text while it is edited, in the cell or in the bar.
  pub editing: Option<String>,
}

/// A cell's name: `B12`.
pub fn cell_name((r, c): (usize, usize)) -> String {
  format!("{}{r}", (b'A' + c as u8) as char)
}

/// The sheet, the field in the edited cell, and the formula bar.
const SHEET: &str = "sheet";
const EDIT: &str = "edit";
const BAR: &str = "formula";

/// Tab and Shift-Tab in a cell's field keep the text and move, as Return does down.
const EDIT_KEYS: &[Binding] = &[
  Binding::new(Shortcut::new(Key::Named(NamedKey::Tab)), Command::Action("right")),
  Binding::new(Shortcut::new(Key::Named(NamedKey::Tab)).shift(), Command::Action("left")),
];

/// How an edit ends: kept, the selection moving by (rows, columns) and the sheet taking the
/// keys again, or kept where the focus went (a click elsewhere), or dropped.
enum End {
  Keep(Option<(isize, isize)>),
  Drop,
}

/// The step the selection takes for a key on the sheet: the arrows, Tab and Shift-Tab; with
/// Cmd, the arrows go to the sheet's edge.
fn step(k: &KeyEvent) -> Option<(isize, isize)> {
  let far = if k.mods.contains(CMD) { ROWS.max(COLS) as isize } else { 1 };
  Some(match k.key {
    Key::Named(NamedKey::ArrowUp) => (-far, 0),
    Key::Named(NamedKey::ArrowDown) => (far, 0),
    Key::Named(NamedKey::ArrowLeft) => (0, -far),
    Key::Named(NamedKey::ArrowRight) => (0, far),
    Key::Named(NamedKey::Tab) if k.mods.shift() => (0, -1),
    Key::Named(NamedKey::Tab) => (0, 1),
    _ => return None,
  })
}

/// A cell's field: the text on the cell's ground, the selection's outline around it drawn by
/// the cell.
struct CellField;

impl TextFieldStyle for CellField {
  fn make_body(&self, ui: &mut Ui, mut c: TextFieldConfiguration) {
    let t = ui.theme();
    let frame = Style::new().w(grow()).h(grow()).pad_xy(3.0, 0.0).focusable_quiet();
    let frame = frame.align(Align::Start, Align::Center).scroll(Axes::HORIZONTAL);
    ui.row(frame, |ui| c.text(ui, &t.field()));
  }
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

/// A cell's number as it shows: whole, or to nine decimals, trimmed (`0.3`, not
/// `0.30000000000000004`).
fn num(v: f64) -> String {
  let v = (v * 1e9).round() / 1e9;
  if v == 0.0 { "0".into() } else { v.to_string() }
}

impl Cells {
  fn value(&self, at: (usize, usize)) -> Option<f64> {
    Eval { cells: self, memo: HashMap::new(), stack: Vec::new(), depth: 0 }.value(at)
  }

  /// What a cell shows: its value, its text, or `#ERR`.
  pub fn shown(&self, at: (usize, usize)) -> String {
    match self.cells.get(&at) {
      Some(t) if t.starts_with('=') => self.value(at).map_or("#ERR".into(), num),
      Some(t) => t.clone(),
      None => String::new(),
    }
  }

  /// The selected cell's text as typed.
  fn text(&self) -> String {
    self.cells.get(&self.sel).cloned().unwrap_or_default()
  }

  /// Moves the selection by (rows, columns), kept on the sheet, and scrolls it into view clear
  /// of the headers.
  fn go(&mut self, ui: &mut Ui, (dr, dc): (isize, isize)) {
    let r = self.sel.0.saturating_add_signed(dr).min(ROWS - 1);
    let c = self.sel.1.saturating_add_signed(dc).min(COLS - 1);
    self.sel = (r, c);
    let at = Rect::new(c as f32 * CELL_W, r as f32 * CELL_H, CELL_W + HEAD_W, CELL_H * 2.0);
    ui.reveal(Id::of(SHEET), at);
  }

  /// Starts editing the selected cell from `text` in its field.
  fn edit(&mut self, ui: &mut Ui, text: String) {
    self.editing = Some(text);
    ui.focus(Some(Id::of(EDIT)));
  }

  /// The keys on the sheet while no cell is edited: moves, Return to edit, Delete to clear, and
  /// text typed to start an edit with.
  fn keys(&mut self, ui: &mut Ui) {
    let sheet = Id::of(SHEET);
    if let Some(k) = ui.key_down(sheet) {
      match k.key {
        Key::Named(NamedKey::Enter) if k.mods.is_empty() => self.edit(ui, self.text()),
        Key::Named(NamedKey::Backspace | NamedKey::Delete) if k.mods.is_empty() => {
          _ = self.cells.remove(&self.sel)
        }
        _ => match step(&k) {
          Some(d) => self.go(ui, d),
          None => ui.pass(sheet),
        },
      }
    }

    if let Some(s) = ui.typed(sheet).map(str::to_owned) {
      self.edit(ui, s);
    }
  }

  pub fn view(&mut self, ui: &mut Ui) {
    if self.editing.is_none() {
      self.keys(ui);
    }

    let mut end = None;
    ui.col(Style::new().grow().gap(8.0), |ui| {
      end = self.bar(ui);
      let click = self.sheet(ui, &mut end);

      // A click elsewhere keeps the edit, then selects; a second click edits.
      if let Some((at, clicks)) = click {
        if self.editing.is_some() {
          end = end.take().or(Some(End::Keep(None)));
        }

        self.finish(ui, end.take());
        self.sel = at;
        ui.focus(Some(Id::of(SHEET)));
        if clicks >= 2 {
          self.edit(ui, self.text());
        }
      }
    });

    self.finish(ui, end);
  }

  /// The formula bar: the cell's name and its text, which an edit there changes; how that edit
  /// ended.
  fn bar(&mut self, ui: &mut Ui) -> Option<End> {
    let t = ui.theme();
    let keys = Id::of(BAR).key("keys");
    let mut r = Response::default();
    ui.row(line(8.0).w(grow()), |ui| {
      let name = Style::new().w(px(52.0)).h(px(t.sizes().control_h())).center();
      let name = name.radius(t.radius()).border(1.0, t.line());
      ui.row(name, |ui| ui.id("name").text(cell_name(self.sel), t.muted()));

      ui.keymap(EDIT_KEYS).with_id(keys).col(Style::new().w(grow()), |ui| {
        let field = ui.a11y_label("Formula").id(BAR);
        match &mut self.editing {
          Some(text) => r = field.text_field(text, ""),
          None => {
            let mut text = self.text();
            r = field.text_field(&mut text, "");
            if r.changed {
              self.editing = Some(text);
            }
          }
        }
      });
    });

    self.ended(ui, &r, keys, Id::of(EDIT))
  }

  /// The sheet: the cells in a column clear of the headers, which stay over it as it scrolls,
  /// placed by its offset; the cell clicked, and how many times.
  fn sheet(&mut self, ui: &mut Ui, end: &mut Option<End>) -> Option<((usize, usize), u32)> {
    let t = ui.theme();
    let sheet = Id::of(SHEET);
    let off = ui.scroll(sheet).map_or(Point::new(0.0, 0.0), |s| s.offset);
    let area = Style::new().grow().scroll(Axes::ALL).bg(t.bg()).border(1.0, t.line());
    let mut click = None;

    // The cells and the scrollers clear of the headers.
    let heads = [CELL_H, 0.0, 0.0, HEAD_W];
    let ui = ui.autofocus().with_id(sheet);
    ui.content_margins(heads, ContentMarginPlacement::ScrollIndicators);
    ui.scroll_area(area.focusable_quiet(), |ui| {
      ui.col(Style::new().pads(heads), |ui| {
        for r in 0..ROWS {
          ui.row(Style::new(), |ui| {
            for c in 0..COLS {
              if let Some(k) = self.cell(ui, (r, c), end) {
                click = Some(((r, c), k));
              }
            }
          });
        }
      });

      self.headers(ui, off);
    });

    click
  }

  /// A cell: its value, or its field while it is edited, outlined while selected; a left click
  /// on it, by its count.
  fn cell(&mut self, ui: &mut Ui, at: (usize, usize), end: &mut Option<End>) -> Option<u32> {
    let t = ui.theme();
    let style = Style::new().w(px(CELL_W)).h(px(CELL_H)).border(0.5, t.line()).clip();
    let selected = at == self.sel;
    let style = if selected { style.border(2.0, t.accent()) } else { style };
    let id = Id::of(("cell", at.0, at.1));
    if selected && self.editing.is_some() {
      *end = end.take().or(self.editor(ui, id));
      return None;
    }

    // Numbers and values at the right, text at the left.
    let shown = self.shown(at);
    let text = self.cells.get(&at).is_some_and(|s| !s.starts_with('=') && number(s).is_none());
    let align = if text { Align::Start } else { Align::End };
    let style = style.pad_xy(4.0, 0.0).align(align, Align::Center);
    let color = if shown == "#ERR" { t.danger() } else { t.text() };
    ui.with_id(id).row(style, |ui| ui.text(shown, t.body().nowrap().color(color)));
    ui.click(id).filter(|k| k.button == MouseButton::Left).map(|k| k.clicks)
  }

  /// The selected cell's field, `id` the cell's, over the cells to its right as its text grows,
  /// up to three more; how its edit ended.
  fn editor(&mut self, ui: &mut Ui, id: Id) -> Option<End> {
    let t = ui.theme();
    let (keys, name) = (id.key("keys"), cell_name(self.sel));
    let over = Style::new().min_w(CELL_W).max_w(CELL_W * 4.0).h(px(CELL_H)).pad(1.0);
    let over = over.bg(t.bg()).border(2.0, t.accent());
    let mut r = Response::default();
    let text = self.editing.as_mut()?;
    ui.with_id(id).row(Style::new().w(px(CELL_W)).h(px(CELL_H)), |ui| {
      let ui = ui.float(Float::parent().clip()).keymap(EDIT_KEYS).with_id(keys);
      ui.row(over, |ui| {
        let ui = ui.text_field_style(&CellField).a11y_label(&name).id(EDIT);
        r = ui.text_field(text, "");
      });
    });

    self.ended(ui, &r, keys, Id::of(BAR))
  }

  /// How the edit in a field ends: Return keeps it and goes down, Tab and Shift-Tab across (the
  /// commands of `keys`, its keymap), Escape drops it, and leaving the field keeps it unless
  /// the focus went to `other`, the edit's other field.
  fn ended(&self, ui: &mut Ui, r: &Response, keys: Id, other: Id) -> Option<End> {
    self.editing.as_ref()?;
    match ui.command(keys) {
      Some(Command::Action("right")) => return Some(End::Keep(Some((0, 1)))),
      Some(Command::Action("left")) => return Some(End::Keep(Some((0, -1)))),
      Some(_) => ui.pass(keys),
      None => {}
    }

    match () {
      _ if r.submitted => Some(End::Keep(Some((1, 0)))),
      _ if r.cancelled => Some(End::Drop),
      _ if r.blurred && !ui.focused(other) => Some(End::Keep(None)),
      _ => None,
    }
  }

  /// Ends the edit: keeps its text in the cell (an empty one clears it) or drops it; the sheet
  /// takes the keys again unless the focus went elsewhere.
  fn finish(&mut self, ui: &mut Ui, end: Option<End>) {
    let Some(end) = end else { return };
    let Some(text) = self.editing.take() else { return };
    if let End::Keep(_) = end {
      match text.trim().is_empty() {
        true => _ = self.cells.remove(&self.sel),
        false => _ = self.cells.insert(self.sel, text),
      }
    }

    match end {
      End::Keep(Some(d)) => self.go(ui, d),
      End::Keep(None) => return,
      End::Drop => {}
    }

    ui.focus(Some(Id::of(SHEET)));
  }

  /// The column letters and row numbers over the sheet, placed against its offset `off` so they
  /// stay in view, the selection's highlighted; the corner over both.
  fn headers(&self, ui: &mut Ui, off: Point) {
    let t = ui.theme();
    let head = Style::new().center().bg(t.panel()).border(0.5, t.line());
    let lit = head.bg(mix(t.panel(), t.accent(), 0.22));
    let label = |on: bool| if on { t.label().color(t.accent()) } else { t.muted() };
    let float = |x: f32, y: f32| Float::parent().offset(x, y).clip();
    ui.float(float(0.0, CELL_H - off.y)).id("rows").col(Style::new(), |ui| {
      for r in 0..ROWS {
        let on = r == self.sel.0;
        let s = if on { lit } else { head };
        ui.key(("row", r)).row(s.w(px(HEAD_W)).h(px(CELL_H)), |ui| ui.text(r, label(on)));
      }
    });

    ui.float(float(HEAD_W - off.x, 0.0)).id("cols").row(Style::new(), |ui| {
      for c in 0..COLS {
        let on = c == self.sel.1;
        let s = if on { lit } else { head };
        let s = s.w(px(CELL_W)).h(px(CELL_H));
        ui.key(("col", c)).row(s, |ui| ui.text((b'A' + c as u8) as char, label(on)));
      }
    });

    ui.float(float(0.0, 0.0)).el(head.w(px(HEAD_W)).h(px(CELL_H)));
  }
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
