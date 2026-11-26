// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/7guis.rs` through the harness, one per thing each task asks for: the
//! converter both ways and past bad text, the booker's red field, its rules and lone OK, the
//! timer stopping and going on, CRUD's filter, fields, update and delete, the circle drawer's
//! hover, menu, dialog and undo, and the sheet's formulas and editing. The tabs switch by a
//! click and read to assistive technology.

#[path = "../examples/7guis.rs"]
#[expect(dead_code, reason = "its main and what this file doesn't use")]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[cfg_attr(windows, expect(unused_attributes, reason = "its windows_subsystem"))]
mod seven;

#[expect(dead_code, reason = "this file uses a part of the helpers")]
mod common;

use std::time::Duration;

use common::{diff, mid, reads};
use nimbo::a11y::Role;
use nimbo::{CMD, Event, Harness, Id, Key, Modifiers, NamedKey, Phase, Point, Size};
use seven::{Kind, NEW_D, Seven, Task};

type H = Harness<Seven>;

/// The app on `task`'s tab, a frame in.
fn on(task: Task) -> H {
  let app = Seven { task, cells: seven::sheet(), ..Seven::default() };
  let mut h = Harness::new(app, Size::new(620.0, 420.0), 1.0);
  h.frame();
  h
}

fn key(h: &mut H, k: NamedKey) {
  h.key(Key::Named(k), Modifiers::default());
}

fn id(s: &str) -> Id {
  Id::of(s)
}

// MARK: Tabs

#[test]
fn a_tab_click_opens_its_task() {
  let mut h = on(Task::Temperature);
  let tab = h.find(Role::Tab, "Cells").unwrap_or_else(|| panic!("{}", h.a11y()));
  h.click_id(tab.id);
  assert_eq!(h.app().task, Task::Cells);
  h.frame();
  assert!(h.rect(id("sheet")).is_some());
  assert!(h.rect(id("c")).is_none(), "the converter's field left with its tab");
}

#[test]
fn a_task_slug_is_its_tab_in_lowercase() {
  let slugs = seven::TASKS.iter().map(|t| seven::slug(t.value())).collect::<Vec<_>>();
  assert_eq!(slugs, ["temperature", "flight", "timer", "crud", "circles", "cells"]);
}

// MARK: Temperature

#[test]
fn celsius_and_fahrenheit_update_each_other() {
  let mut h = on(Task::Temperature);
  h.click_id(id("c"));
  h.type_text("100");
  assert_eq!(h.app().temperature.f, "212");
  h.click_id(id("f"));
  h.key(Key::Char('a'), CMD);
  h.type_text("-40");
  assert_eq!(h.app().temperature.c, "-40");
}

#[test]
fn text_that_isnt_a_number_leaves_the_other_field() {
  let mut h = on(Task::Temperature);
  h.click_id(id("f"));
  h.type_text("212");
  assert_eq!(h.app().temperature.c, "100");
  h.type_text("x");
  assert_eq!((h.app().temperature.f.as_str(), h.app().temperature.c.as_str()), ("212x", "100"));
}

// MARK: Flight

#[test]
fn a_date_reads_only_when_it_is_one() {
  assert_eq!(seven::parse_date("27.03.2014"), Some((2014, 3, 27)));
  assert_eq!(seven::parse_date(" 29.02.2016 "), Some((2016, 2, 29)));
  for bad in ["29.02.2014", "31.04.2014", "1.13.2014", "27.03", "27.03.2014.1", "a.b.c", ""] {
    assert_eq!(seven::parse_date(bad), None, "{bad}");
  }
}

#[test]
fn a_one_way_flight_books_and_tells_with_one_ok() {
  let mut h = on(Task::Flight);
  assert!(h.click_id(id("book")));
  assert!(h.app().flight.booked);
  h.wait(Duration::from_millis(300));
  reads(&h.a11y(), &["static text \"You have booked a one-way flight on 27.03.2014.\""]);
  assert!(h.find(Role::Button, "Cancel").is_none(), "{}", h.a11y());
  key(&mut h, NamedKey::Enter);
  h.wait(Duration::from_millis(300));
  assert!(!h.app().flight.booked);
}

#[test]
fn the_return_date_is_off_for_a_one_way_flight() {
  let mut h = on(Task::Flight);
  h.click_id(id("back"));
  h.type_text("1");
  assert_eq!(h.app().flight.back, "27.03.2014", "a disabled field takes no text");
}

#[test]
fn book_is_off_for_a_return_before_the_start_or_a_bad_date() {
  let mut h = on(Task::Flight);
  h.app_mut().flight.kind = Kind::Return;
  h.app_mut().flight.back = "26.03.2014".into();
  h.frame();
  h.click_id(id("book"));
  assert!(!h.app().flight.booked, "back before the start");

  h.app_mut().flight.back = "28.03.2014".into();
  h.app_mut().flight.start = "31.02.2014".into();
  h.frame();
  h.click_id(id("book"));
  assert!(!h.app().flight.booked, "no such start date");

  h.app_mut().flight.start = "27.03.2014".into();
  h.frame();
  h.click_id(id("book"));
  assert!(h.app().flight.booked);
}

#[test]
fn a_bad_date_turns_its_field_red() {
  let mut h = on(Task::Flight);
  let r = h.rect(id("start")).unwrap();
  let at = (r.x + r.w - 8.0, r.y + r.h / 2.0);
  let before = h.render().pixel(at.0, at.1);
  h.click_id(id("start"));
  h.type_text("x");
  let after = h.render().pixel(at.0, at.1);
  assert!(after[0] > after[2] + 20 && diff(before, after) > 20, "{before:?} -> {after:?}");
}

// MARK: Timer

#[test]
fn the_timer_stops_at_its_duration_and_goes_on_when_raised() {
  let mut h = on(Task::Timer);
  h.wait(Duration::from_secs(12));
  assert_eq!(h.app().timer.elapsed, h.app().timer.duration);
  assert!(!h.needs_frame() && h.due_in().is_none(), "a stopped timer asks for no frames");
  h.app_mut().timer.duration = 20.0;
  h.frame();
  h.wait(Duration::from_secs(3));
  let e = h.app().timer.elapsed;
  assert!(e > 12.5 && e < 13.5, "{e}");
}

#[test]
fn reset_starts_the_timer_from_zero() {
  let mut h = on(Task::Timer);
  h.wait(Duration::from_secs(2));
  assert!(h.app().timer.elapsed >= 1.8);
  h.click_id(id("reset"));
  assert!(h.app().timer.elapsed < 0.2);
  h.wait(Duration::from_secs(1));
  let e = h.app().timer.elapsed;
  assert!(e > 0.8 && e < 1.3, "{e}");
  assert_eq!(h.text(id("elapsed")), Some(format!("{e:.1}s").as_str()));
}

#[test]
fn the_duration_slider_moves_with_the_arrows() {
  let mut h = on(Task::Timer);
  let d = h.app().timer.duration;
  h.set_focus(Some(id("duration")));
  key(&mut h, NamedKey::ArrowRight);
  assert!(h.app().timer.duration > d);
}

// MARK: CRUD

/// The point of the `n`th row of the people list.
fn row(h: &H, n: usize) -> (f32, f32) {
  let r = h.rect(id("people")).unwrap();
  (r.x + 30.0, r.y + 11.0 + 22.0 * n as f32)
}

#[test]
fn a_selected_person_fills_the_fields() {
  let mut h = on(Task::Crud);
  let (x, y) = row(&h, 1);
  h.click(x, y);
  assert_eq!((h.app().crud.name.as_str(), h.app().crud.surname.as_str()), ("Max", "Mustermann"));
  key(&mut h, NamedKey::ArrowDown);
  assert_eq!(h.app().crud.surname, "Tisch", "the keys select as a click does");
}

#[test]
fn the_prefix_filters_by_surname() {
  let mut h = on(Task::Crud);
  h.click_id(id("prefix"));
  h.type_text("M");
  h.frame();
  reads(&h.a11y(), &["row \"Mustermann, Max\""]);
  assert!(h.find(Role::Row, "Emil, Hans").is_none(), "{}", h.a11y());
}

#[test]
fn update_and_delete_act_on_the_selected_person() {
  let mut h = on(Task::Crud);
  h.click_id(id("update"));
  assert_eq!(h.app().crud.people[1].name, "Max", "nothing selected yet");

  let (x, y) = row(&h, 1);
  h.click(x, y);
  h.click_id(id("name"));
  key(&mut h, NamedKey::End);
  h.type_text("ine");
  h.click_id(id("update"));
  assert_eq!(h.app().crud.people[1].name, "Maxine");

  h.click_id(id("delete"));
  let left = h.app().crud.people.iter().map(|p| p.surname.as_str()).collect::<Vec<_>>();
  assert_eq!(left, ["Emil", "Tisch"]);
  assert!(h.app().crud.selected().is_none());
}

#[test]
fn create_adds_the_fields_names() {
  let mut h = on(Task::Crud);
  h.click_id(id("name"));
  h.type_text("Ada");
  h.click_id(id("surname"));
  h.type_text("Lovelace");
  h.click_id(id("create"));
  h.frame();
  reads(&h.a11y(), &["row \"Lovelace, Ada\""]);
}

#[test]
fn a_selection_the_filter_hides_is_none() {
  let mut h = on(Task::Crud);
  let (x, y) = row(&h, 0);
  h.click(x, y);
  assert_eq!(h.app().crud.selected(), Some(0));
  h.click_id(id("prefix"));
  h.type_text("T");
  assert_eq!(h.app().crud.selected(), None);
  h.click_id(id("delete"));
  assert_eq!(h.app().crud.people.len(), 3);
}

// MARK: Circles

/// Clicks on the canvas at points from its corner.
fn add(h: &mut H, at: &[(f32, f32)]) {
  let r = h.rect(id("canvas")).unwrap();
  for &(x, y) in at {
    h.click(r.x + x, r.y + y);
  }
}

#[test]
fn a_click_adds_a_circle_and_one_on_a_circle_doesnt() {
  let mut h = on(Task::Circles);
  add(&mut h, &[(60.0, 60.0), (200.0, 120.0), (65.0, 62.0)]);
  let c = &h.app().circles.circles;
  assert_eq!(c.len(), 2);
  assert_eq!((c[0].at.x, c[0].at.y, c[0].d), (60.0, 60.0, NEW_D));
}

#[test]
fn a_right_click_on_the_canvas_adds_nothing_and_keeps_the_redo() {
  let mut h = on(Task::Circles);
  add(&mut h, &[(60.0, 60.0)]);
  h.click_id(id("undo"));
  let r = h.rect(id("canvas")).unwrap();
  h.right_click(r.x + 200.0, r.y + 120.0);
  h.frame();
  assert!(h.app().circles.circles.is_empty());
  h.click_id(id("redo"));
  assert_eq!(h.app().circles.circles.len(), 1, "redone");
}

#[test]
fn the_circle_under_the_pointer_is_filled() {
  let mut h = on(Task::Circles);
  add(&mut h, &[(60.0, 60.0)]);
  let r = h.rect(id("canvas")).unwrap();
  h.move_to(r.x + 200.0, r.y + 200.0);
  h.wait(Duration::from_millis(200));
  let empty = h.render().pixel(r.x + 60.0, r.y + 60.0);
  h.move_to(r.x + 62.0, r.y + 62.0);
  h.wait(Duration::from_millis(200));
  let filled = h.render().pixel(r.x + 60.0, r.y + 60.0);
  assert!(diff(empty, filled) > 30, "{empty:?} -> {filled:?}");
}

#[test]
fn a_resize_is_one_undo_step() {
  let mut h = on(Task::Circles);
  add(&mut h, &[(60.0, 60.0), (200.0, 120.0)]);
  let r = h.rect(id("canvas")).unwrap();
  h.right_click(r.x + 62.0, r.y + 62.0);
  h.frame();
  h.click_menu_item("Adjust diameter…");
  h.wait(Duration::from_millis(300));
  reads(&h.a11y(), &["static text \"Adjust diameter of circle at (60, 60).\""]);

  let s = h.rect(id("d")).unwrap();
  h.click(s.x + s.w * 0.5, s.y + s.h / 2.0);
  h.click(s.x + s.w - 2.0, s.y + s.h / 2.0);
  let big = h.app().circles.circles[0].d;
  assert!(big > 100.0, "{big}");
  h.click_id(id("diameter").key("close"));
  h.wait(Duration::from_millis(300));

  h.click_id(id("undo"));
  assert_eq!(h.app().circles.circles[0].d, NEW_D);
  assert_eq!(h.app().circles.circles.len(), 2);
  h.click_id(id("redo"));
  assert_eq!(h.app().circles.circles[0].d, big);
}

/// The dialog opens on its slider: the arrows resize, Tab goes to Close and back, nothing under
/// the dialog takes the focus, Escape closes it and gives the focus back.
#[test]
fn the_diameter_dialog_keeps_the_keys_on_its_slider_and_close() {
  let mut h = on(Task::Circles);
  add(&mut h, &[(60.0, 60.0)]);
  let r = h.rect(id("canvas")).unwrap();
  h.right_click(r.x + 62.0, r.y + 62.0);
  h.frame();
  h.click_menu_item("Adjust diameter…");
  h.wait(Duration::from_millis(300));
  assert_eq!(h.focus_id(), Some(id("d")));
  key(&mut h, NamedKey::ArrowRight);
  key(&mut h, NamedKey::ArrowRight);
  let d = h.app().circles.circles[0].d;
  assert!(d > NEW_D, "{d}");

  let close = id("diameter").key("close");
  key(&mut h, NamedKey::Tab);
  assert_eq!(h.focus_id(), Some(close));
  key(&mut h, NamedKey::Tab);
  assert_eq!(h.focus_id(), Some(id("d")), "around the two");
  h.set_focus(Some(id("undo")));
  h.frame();
  assert_eq!(h.focus_id(), Some(id("d")), "nothing under it");

  key(&mut h, NamedKey::Escape);
  h.wait(Duration::from_millis(300));
  assert!(h.find(Role::Slider, "Diameter").is_none(), "closed");
  assert_eq!(h.app().circles.circles[0].d, d, "kept");
  h.click_id(id("undo"));
  assert_eq!(h.app().circles.circles[0].d, NEW_D, "one step");
}

#[test]
fn undo_takes_back_adds_and_a_new_change_drops_redo() {
  let mut h = on(Task::Circles);
  add(&mut h, &[(60.0, 60.0), (200.0, 120.0)]);
  h.click_id(id("undo"));
  h.click_id(id("undo"));
  assert!(h.app().circles.circles.is_empty());
  h.click_id(id("redo"));
  assert_eq!(h.app().circles.circles.len(), 1);
  add(&mut h, &[(300.0, 200.0)]);
  h.click_id(id("redo"));
  h.click_id(id("redo"));
  assert_eq!(h.app().circles.circles.len(), 2);
}

// MARK: Cells

fn cell(r: usize, c: usize) -> Id {
  Id::of(("cell", r, c))
}

/// Double-clicks a cell, which opens its field.
fn edit(h: &mut H, r: usize, c: usize) {
  let (x, y) = mid(h.rect(cell(r, c)).unwrap());
  h.press_with(x, y, 2, Modifiers::default());
  h.release(x, y);
  h.frame();
}

#[test]
fn formulas_show_their_values() {
  let h = on(Task::Cells);
  let s = &h.app().cells;
  assert_eq!([s.shown((2, 0)), s.shown((3, 0)), s.shown((1, 1))], ["12", "24", "48"]);
  assert!(h.find(Role::StaticText, "Total").is_some());
}

#[test]
fn an_edit_commits_on_enter_and_recomputes() {
  let mut h = on(Task::Cells);
  edit(&mut h, 0, 0);
  assert_eq!(h.focus_id(), Some(Id::of(("edit", 0usize, 0usize))));
  h.key(Key::Char('a'), CMD);
  h.type_text("10");
  key(&mut h, NamedKey::Enter);
  assert!(h.app().cells.editing.is_none());
  assert_eq!(h.app().cells.shown((3, 0)), "34");
  h.frame();
  assert!(h.find(Role::StaticText, "68").is_some(), "{}", h.a11y());
}

#[test]
fn escape_drops_an_edit() {
  let mut h = on(Task::Cells);
  edit(&mut h, 0, 0);
  h.type_text("99");
  key(&mut h, NamedKey::Escape);
  assert!(h.app().cells.editing.is_none());
  assert_eq!(h.app().cells.shown((0, 0)), "5");
}

#[test]
fn a_cycle_or_a_bad_formula_shows_err() {
  let mut s = seven::Cells::default();
  s.cells.insert((0, 0), "=B0".into());
  s.cells.insert((0, 1), "=add(A0, 1)".into());
  s.cells.insert((1, 0), "=nope(1)".into());
  s.cells.insert((2, 0), "=div(sum(C0:C2), 2".into());
  s.cells.insert((3, 0), "=prod(2, sub(10, 4), div(9, 3))".into());
  let shown = (0..4).map(|r| s.shown((r, 0))).collect::<Vec<_>>();
  assert_eq!(shown, ["#ERR", "#ERR", "#ERR", "36"]);
}

#[test]
fn a_range_keeps_its_errors_and_a_cycle_through_it_ends_at_once() {
  let mut s = seven::Cells::default();
  s.cells.insert((0, 0), "=sum(A0:B0)".into());
  s.cells.insert((0, 1), "=sum(A0:B0)".into());
  s.cells.insert((1, 0), "=sum(A1:A1)".into());
  s.cells.insert((2, 0), "=sum(A3:B3)".into());
  s.cells.insert((3, 0), "x".into());
  s.cells.insert((3, 1), "4".into());
  let shown = (0..3).map(|r| s.shown((r, 0))).collect::<Vec<_>>();
  assert_eq!(shown, ["#ERR", "#ERR", "4"], "text in a range counts as 0");

  // Two letters are no column: not the first one's.
  s.cells.insert((4, 0), "=BA3".into());
  s.cells.insert((5, 0), "=sum(B3:AB3)".into());
  assert_eq!((s.shown((4, 0)), s.shown((5, 0))), ("#ERR".into(), "#ERR".into()));

  // Each row sums the two above it: 2^98 paths, each cell evaluated once.
  s.cells.clear();
  s.cells.insert((0, 0), "1".into());
  s.cells.insert((1, 0), "1".into());
  for r in 2..99 {
    s.cells.insert((r, 0), format!("=sum(A{}:A{})", r - 2, r - 1));
  }

  let at = std::time::Instant::now();
  assert_eq!(s.shown((30, 0)), "1346269");
  assert!(at.elapsed().as_millis() < 500, "{:?}", at.elapsed());
}

/// On a 1 MB stack, an eighth of the main thread's: nesting is bounded, not by the stack.
#[test]
fn a_deeply_nested_formula_is_an_error_not_a_crash() {
  let shown = std::thread::Builder::new()
    .stack_size(1024 * 1024)
    .spawn(|| {
      let mut s = seven::Cells::default();
      let n = 100_000;
      s.cells.insert((0, 0), format!("={}1{}", "sum(".repeat(n), ")".repeat(n)));
      s.cells.insert((1, 0), format!("={}1{}", "sum(".repeat(50), ")".repeat(50)));
      for r in 2..99 {
        s.cells.insert((r, 0), format!("={}A{}{}", "sum(".repeat(5), r - 1, ")".repeat(5)));
      }

      [s.shown((0, 0)), s.shown((1, 0)), s.shown((98, 0))]
    })
    .unwrap()
    .join()
    .unwrap();
  assert_eq!(shown, ["#ERR", "1", "#ERR"], "98 cells of 5 calls each nest past the bound");
}

#[test]
fn the_sheet_scrolls_to_its_far_corner() {
  let mut h = on(Task::Cells);
  let sheet = h.rect(id("sheet")).unwrap();
  let (pos, delta) = (Point::new(sheet.x + 100.0, sheet.y + 100.0), Point::new(-5000.0, -5000.0));
  let (phase, momentum, mods) = (Phase::Changed, Phase::None, Modifiers::empty());
  h.event(Event::Scroll { pos, delta, precise: true, phase, momentum, mods });
  h.frame();
  let r = h.rect(cell(seven::ROWS - 1, seven::COLS - 1)).unwrap();
  assert!(r.x + r.w <= sheet.x + sheet.w + 1.0 && r.y + r.h <= sheet.y + sheet.h + 1.0, "{r:?}");
}

// MARK: Assistive technology

#[test]
fn the_tabs_and_the_tasks_read_to_assistive_technology() {
  let h = on(Task::Flight);
  let dump = h.a11y();
  reads(
    &dump,
    &[
      "pop up button \"Flight\" value \"one-way flight\"",
      "text field \"Start\" value \"27.03.2014\"",
    ],
  );
  reads(&dump, &["text field \"Return\" value \"27.03.2014\" disabled"]);
  let h = on(Task::Timer);
  reads(
    &h.a11y(),
    &["progress indicator \"Elapsed Time\" 0 in 0..1", "slider \"Duration\" 10 in 1..30"],
  );
  let h = on(Task::Crud);
  reads(
    &h.a11y(),
    &[
      "tab \"CRUD\" selected",
      "text field \"Filter prefix\" value \"\"",
      "row \"Emil, Hans\"",
      "button \"Update\" disabled",
      "button \"Delete\" disabled",
    ],
  );
}
