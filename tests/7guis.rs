// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/7guis.rs` through the harness, one per thing each task asks for: the
//! converter both ways and past bad text, the booker's labelled dates, their ranges and lone OK,
//! the timer stopping and going on, CRUD's filter, fields, update and delete, the circle drawer's
//! hover, menu, dialog and undo, and the sheet's formulas, selection, keys and edits. The tabs
//! switch by a click and read to assistive technology.

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
use nimbo::{
  CMD, Date, DateFormat, Event, Harness, Id, Key, Modifiers, NamedKey, Phase, Point, Size,
};
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

#[test]
fn the_fields_show_their_units() {
  let h = on(Task::Temperature);
  let fields = ["text field \"Celsius\" value \"\"", "text field \"Fahrenheit\" value \"\""];
  reads(&h.a11y(), &fields);
  reads(&h.a11y(), &["static text \"°C\"", "static text \"°F\""]);
}

#[test]
fn a_temperature_shows_to_a_tenth_and_whole_when_large() {
  let cases =
    [(99.5, "99.5"), (212.0, "212"), (-0.04, "0"), (37.77, "37.8"), (2234296.4, "2234296")];
  for (v, s) in cases {
    assert_eq!(seven::fmt(v), s, "{v}");
  }

  assert_eq!(seven::fmt(1.8e20), "1.800e20");
  let mut h = on(Task::Temperature);
  h.click_id(id("c"));
  h.type_text("37,5");
  assert_eq!(h.app().temperature.f, "99.5", "a comma reads as the point");
  h.click_id(id("f"));
  h.key(Key::Char('a'), CMD);
  h.type_text("1000.37");
  assert_eq!(h.app().temperature.c, "538");
}

#[test]
fn infinity_is_no_number_and_turns_its_field_red() {
  for bad in ["inf", "NaN", "1e999", "", "1.2.3"] {
    assert_eq!(seven::number(bad), None, "{bad}");
  }

  let mut h = on(Task::Temperature);
  let r = h.rect(id("c")).unwrap();
  let at = (r.x + r.w - 8.0, r.y + r.h / 2.0);
  let before = h.render().pixel(at.0, at.1);
  h.click_id(id("c"));
  h.type_text("inf");
  assert_eq!(h.app().temperature.f, "", "the other field stays");
  let after = h.render().pixel(at.0, at.1);
  assert!(after[0] > after[2] + 20 && diff(before, after) > 20, "{before:?} -> {after:?}");
}

// MARK: Flight

fn date(y: u32, m: u32, d: u32) -> Date {
  Date::new(y, m, d).unwrap()
}

/// The `i`th segment of the date picker `name`: year, month and day in the harness's ISO order.
fn segment(name: &str, i: usize) -> Id {
  id(name).key(("segment", i))
}

#[test]
fn a_one_way_flight_books_today_and_tells_with_one_ok() {
  let mut h = on(Task::Flight);
  assert!(h.click_id(id("book")));
  assert!(h.app().flight.booked);
  h.wait(Duration::from_millis(300));
  let today = DateFormat::ISO.format(Date::today());
  let told = format!("static text \"You have booked a one-way flight on {today}.\"");
  reads(&h.a11y(), &[&told]);
  assert!(h.find(Role::Button, "Cancel").is_none(), "{}", h.a11y());
  key(&mut h, NamedKey::Enter);
  h.wait(Duration::from_millis(300));
  assert!(!h.app().flight.booked);
}

#[test]
fn the_dates_are_labelled_pickers() {
  let h = on(Task::Flight);
  let a = h.a11y();
  reads(&a, &["static text \"Departure:\"", "static text \"Return:\""]);
  assert!(
    h.find(Role::TextField, "Departure").is_some() && h.find(Role::TextField, "Return").is_some(),
    "{a}"
  );
}

#[test]
fn the_return_date_is_off_for_a_one_way_flight() {
  let mut h = on(Task::Flight);
  let back = h.app().flight.back;
  h.click_id(segment("back", 2));
  key(&mut h, NamedKey::ArrowUp);
  assert_eq!(h.app().flight.back, back, "a disabled picker takes no keys");
  assert_ne!(h.focus_id(), Some(id("back")));
}

#[test]
fn the_departure_is_never_before_today() {
  let mut h = on(Task::Flight);
  h.click_id(segment("start", 0));
  key(&mut h, NamedKey::ArrowDown);
  assert_eq!(h.app().flight.start, Date::today());
}

#[test]
fn the_return_is_never_before_the_departure_and_follows_it() {
  let mut h = on(Task::Flight);
  let f = &mut h.app_mut().flight;
  (f.kind, f.start, f.back) = (Kind::Return, date(2099, 6, 15), date(2099, 6, 15));
  h.frame();
  h.click_id(segment("back", 2));
  key(&mut h, NamedKey::ArrowDown);
  assert_eq!(h.app().flight.back, date(2099, 6, 15), "a day before the departure");

  // The departure a month on: the return comes along, and Book books both.
  h.click_id(segment("start", 1));
  key(&mut h, NamedKey::ArrowUp);
  assert_eq!((h.app().flight.start, h.app().flight.back), (date(2099, 7, 15), date(2099, 7, 15)));
  h.click_id(id("book"));
  assert!(h.app().flight.booked);
  h.wait(Duration::from_millis(300));
  reads(
    &h.a11y(),
    &["static text \"You have booked a flight on 2099-07-15, back on 2099-07-15.\""],
  );
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
fn the_gauge_and_the_slider_line_up() {
  let h = on(Task::Timer);
  let (gauge, slider) = (h.rect(id("gauge")).unwrap(), h.rect(id("duration")).unwrap());
  assert_eq!((gauge.x, gauge.w), (slider.x, slider.w));
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
  (r.x + 30.0, r.y + 5.0 + 12.0 + 24.0 * n as f32)
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
fn the_filter_is_as_high_as_the_fields() {
  let h = on(Task::Crud);
  let (filter, name) = (h.rect(id("filter")).unwrap(), h.rect(id("name")).unwrap());
  assert_eq!(filter.h, name.h);
}

/// The rows the list shows.
fn rows(h: &H) -> Vec<String> {
  h.find_all(Role::Row).into_iter().map(|e| e.name).collect()
}

/// Types `s` into the field `name` in place of its text.
fn fill(h: &mut H, name: &str, s: &str) {
  h.click_id(id(name));
  h.key(Key::Char('a'), CMD);
  h.type_text(s);
  h.frame();
}

#[test]
fn the_filter_finds_a_name_or_surname_as_typed() {
  let mut h = on(Task::Crud);
  fill(&mut h, "name", "Emin Emin");
  fill(&mut h, "surname", "XXX");
  h.click_id(id("create"));
  fill(&mut h, "filter", "Em");
  assert_eq!(rows(&h), ["Emil, Hans", "XXX, Emin Emin"]);
  fill(&mut h, "filter", "max m");
  assert_eq!(rows(&h), ["Mustermann, Max"], "the name then the surname");
  fill(&mut h, "filter", "tisch, r");
  assert_eq!(rows(&h), ["Tisch, Roman"], "as the list shows it");
  fill(&mut h, "filter", "ax");
  assert!(rows(&h).is_empty(), "the start of a word, not its middle");
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
  assert_eq!(h.app().crud.selected(), Some(1), "the next one takes the selection");
  assert_eq!((h.app().crud.name.as_str(), h.app().crud.surname.as_str()), ("Roman", "Tisch"));
  h.click_id(id("delete"));
  assert_eq!(h.app().crud.selected(), Some(0), "at the end, the one before");
  h.click_id(id("delete"));
  assert!(h.app().crud.people.is_empty() && h.app().crud.selected().is_none());
  assert_eq!((h.app().crud.name.as_str(), h.app().crud.surname.as_str()), ("", ""));
}

#[test]
fn update_is_off_until_the_names_change_and_for_someone_elses() {
  let mut h = on(Task::Crud);
  let (x, y) = row(&h, 1);
  h.click(x, y);
  h.frame();
  reads(&h.a11y(), &["button \"Update\" disabled"]);

  fill(&mut h, "name", "Hans");
  fill(&mut h, "surname", "emil ");
  reads(
    &h.a11y(),
    &["button \"Update\" disabled", "static text \"Hans Emil is in the list already.\""],
  );
  h.click_id(id("update"));
  assert_eq!(h.app().crud.people[1].surname, "Mustermann");

  fill(&mut h, "surname", "Emilia");
  reads(&h.a11y(), &["button \"Update\""]);
  assert!(h.rect(id("taken")).is_none(), "no note");
}

#[test]
fn create_is_off_for_a_person_in_the_list_or_a_blank_field() {
  let mut h = on(Task::Crud);
  reads(&h.a11y(), &["button \"Create\" disabled"]);
  fill(&mut h, "name", "max");
  reads(&h.a11y(), &["button \"Create\" disabled"]);
  fill(&mut h, "surname", " Mustermann");
  reads(&h.a11y(), &["button \"Create\" disabled"]);
  assert_eq!(h.text(id("taken")), Some("Max Mustermann is in the list already."));
  h.click_id(id("create"));
  assert_eq!(h.app().crud.people.len(), 3);
}

#[test]
fn a_new_person_is_selected_and_shown_past_the_filter() {
  let mut h = on(Task::Crud);
  fill(&mut h, "filter", "M");
  fill(&mut h, "name", " Ada");
  fill(&mut h, "surname", "Lovelace ");
  h.click_id(id("create"));
  h.frame();
  assert_eq!(h.app().crud.filter, "", "the filter hid her");
  let p = &h.app().crud.people[3];
  assert_eq!((p.name.as_str(), p.surname.as_str()), ("Ada", "Lovelace"), "trimmed");
  assert_eq!(h.app().crud.selected(), Some(3));
  reads(&h.a11y(), &["row \"Lovelace, Ada\" selected"]);
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
  reads(&h.a11y(), &["row \"Lovelace, Ada\" selected"]);
}

#[test]
fn a_selection_the_filter_hides_is_none() {
  let mut h = on(Task::Crud);
  let (x, y) = row(&h, 0);
  h.click(x, y);
  assert_eq!(h.app().crud.selected(), Some(0));
  h.click_id(id("filter"));
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

/// Clicks a cell, which selects it.
fn select(h: &mut H, r: usize, c: usize) {
  let (x, y) = mid(h.rect(cell(r, c)).unwrap());
  h.click(x, y);
  h.frame();
}

fn sheet(h: &H) -> &seven::Cells {
  &h.app().cells
}

#[test]
fn formulas_show_their_values() {
  let h = on(Task::Cells);
  let s = &h.app().cells;
  assert_eq!([s.shown((2, 0)), s.shown((3, 0)), s.shown((1, 1))], ["12", "24", "48"]);
  assert!(h.find(Role::StaticText, "Total").is_some());
}

#[test]
fn a_click_selects_a_cell_and_the_bar_shows_its_formula() {
  let mut h = on(Task::Cells);
  assert_eq!(h.focus_id(), Some(id("sheet")), "the sheet takes the keys from the start");
  select(&mut h, 1, 1);
  assert_eq!((sheet(&h).sel, sheet(&h).editing.as_ref()), ((1, 1), None));
  assert_eq!(h.focus_id(), Some(id("sheet")));
  assert_eq!(h.text(id("name")), Some("B1"));
  reads(&h.a11y(), &["text field \"Formula\" value \"=mul(A3, 2)\""]);
}

#[test]
fn the_selection_is_outlined_and_its_headers_lit() {
  let mut h = on(Task::Cells);
  select(&mut h, 2, 1);
  let img = h.render();
  let r = h.rect(cell(2, 1)).unwrap();
  let (edge, inside) = (img.pixel(r.x + 1.0, r.y + r.h / 2.0), img.pixel(r.x + 8.0, r.y + 8.0));
  assert!(edge[2] > edge[0] + 60 && diff(edge, inside) > 60, "{edge:?} inside {inside:?}");

  // Beside the label, off its glyphs.
  let at = |k: Id| {
    let r = h.rect(k).unwrap();
    img.pixel(r.x + 4.0, r.y + 4.0)
  };
  let (cols, rows) = (id("cols"), id("rows"));
  let (on, off) = (at(cols.key(("col", 1usize))), at(cols.key(("col", 0usize))));
  assert!(on[2] > on[0] + 20 && diff(on, off) > 20, "column {on:?} vs {off:?}");
  let (on, off) = (at(rows.key(("row", 2usize))), at(rows.key(("row", 0usize))));
  assert!(diff(on, off) > 20, "row {on:?} vs {off:?}");
}

#[test]
fn the_arrows_move_the_selection_and_scroll_it_into_view() {
  let mut h = on(Task::Cells);
  key(&mut h, NamedKey::ArrowLeft);
  key(&mut h, NamedKey::ArrowUp);
  assert_eq!(sheet(&h).sel, (0, 0), "kept on the sheet");
  for _ in 0..40 {
    key(&mut h, NamedKey::ArrowDown);
  }

  key(&mut h, NamedKey::ArrowRight);
  assert_eq!(sheet(&h).sel, (40, 1));
  h.frame();
  let (area, r) = (h.rect(id("sheet")).unwrap(), h.rect(cell(40, 1)).unwrap());
  assert!(r.y >= area.y + 22.0 && r.y + r.h <= area.y + area.h + 1.0, "{r:?} in {area:?}");

  h.key(NamedKey::ArrowRight, CMD);
  h.key(NamedKey::ArrowUp, CMD);
  assert_eq!(sheet(&h).sel, (0, seven::COLS - 1), "Cmd goes to the edge");
  h.frame();
  let r = h.rect(cell(0, seven::COLS - 1)).unwrap();
  assert!(r.y >= area.y + 22.0 && r.x + r.w <= area.x + area.w + 1.0, "{r:?} in {area:?}");
  assert!(r.x >= area.x + 36.0, "clear of the row numbers: {r:?}");
}

#[test]
fn the_headers_stay_in_view_as_the_sheet_scrolls() {
  let mut h = on(Task::Cells);
  let area = h.rect(id("sheet")).unwrap();
  let (cols, rows) = (h.rect(id("cols")).unwrap(), h.rect(id("rows")).unwrap());
  let (pos, delta) = (Point::new(area.x + 100.0, area.y + 100.0), Point::new(-300.0, -500.0));
  let (phase, momentum, mods) = (Phase::Changed, Phase::None, Modifiers::empty());
  h.event(Event::Scroll { pos, delta, precise: true, phase, momentum, mods });
  h.frame();
  let off = h.scroll(id("sheet")).unwrap().offset;
  assert!(off.x > 100.0 && off.y > 100.0, "{off:?}");

  let (c, r) = (h.rect(id("cols")).unwrap(), h.rect(id("rows")).unwrap());
  assert_eq!((c.y, r.x), (cols.y, rows.x), "the letters at the top, the numbers at the left");
  let (row, col) =
    (h.rect(id("rows").key(("row", 30usize))), h.rect(id("cols").key(("col", 9usize))));
  let (row, col, at) = (row.unwrap(), col.unwrap(), h.rect(cell(30, 9)).unwrap());
  assert_eq!((row.y, row.h, col.x, col.w), (at.y, at.h, at.x, at.w), "each along with the cells");
}

#[test]
fn typing_replaces_a_cell_and_return_keeps_it_and_goes_down() {
  let mut h = on(Task::Cells);
  h.key(Key::Char('4'), Modifiers::empty());
  assert_eq!(sheet(&h).editing.as_deref(), Some("4"));
  h.frame();
  assert_eq!(h.focus_id(), Some(id("edit")));
  h.type_text("2");
  key(&mut h, NamedKey::Enter);
  assert_eq!((sheet(&h).cells[&(0, 0)].as_str(), sheet(&h).sel), ("42", (1, 0)));
  assert_eq!(sheet(&h).editing, None);
  assert_eq!(h.focus_id(), Some(id("sheet")));
  assert_eq!(sheet(&h).shown((3, 0)), "98");
}

#[test]
fn return_edits_the_formula_and_escape_drops_the_edit() {
  let mut h = on(Task::Cells);
  select(&mut h, 1, 1);
  key(&mut h, NamedKey::Enter);
  h.frame();
  assert_eq!(h.focus_id(), Some(id("edit")));
  assert_eq!(h.text(id("edit")), Some("=mul(A3, 2)"));
  assert_eq!(h.selection(id("edit")), Some((11, 11)), "the caret at the end");

  key(&mut h, NamedKey::Backspace);
  key(&mut h, NamedKey::Escape);
  assert_eq!((sheet(&h).editing.as_ref(), sheet(&h).sel), (None, (1, 1)));
  assert_eq!(sheet(&h).cells[&(1, 1)], "=mul(A3, 2)");
  assert_eq!(h.focus_id(), Some(id("sheet")));
}

#[test]
fn tab_keeps_an_edit_and_goes_right() {
  let mut h = on(Task::Cells);
  select(&mut h, 5, 2);
  h.key(Key::Char('x'), Modifiers::empty());
  h.frame();
  key(&mut h, NamedKey::Tab);
  assert_eq!((sheet(&h).cells[&(5, 2)].as_str(), sheet(&h).sel), ("x", (5, 3)));
  assert_eq!(h.focus_id(), Some(id("sheet")), "not the next control");

  key(&mut h, NamedKey::Tab);
  h.key(NamedKey::Tab, Modifiers::SHIFT);
  h.key(NamedKey::Tab, Modifiers::SHIFT);
  assert_eq!(sheet(&h).sel, (5, 2), "Tab and Shift-Tab move on the sheet too");
}

#[test]
fn delete_clears_the_selected_cell() {
  let mut h = on(Task::Cells);
  key(&mut h, NamedKey::Backspace);
  assert!(!sheet(&h).cells.contains_key(&(0, 0)));
  assert_eq!(sheet(&h).shown((2, 0)), "7");
}

#[test]
fn a_click_on_another_cell_keeps_the_edit_and_selects_it() {
  let mut h = on(Task::Cells);
  h.key(Key::Char('9'), Modifiers::empty());
  h.frame();
  select(&mut h, 4, 3);
  assert_eq!((sheet(&h).cells[&(0, 0)].as_str(), sheet(&h).sel), ("9", (4, 3)));
  assert_eq!(sheet(&h).editing, None);
  assert_eq!(h.focus_id(), Some(id("sheet")));
}

#[test]
fn the_bar_edits_the_selected_cell() {
  let mut h = on(Task::Cells);
  select(&mut h, 1, 1);
  h.click_id(id("formula"));
  h.key(Key::Char('a'), CMD);
  h.type_text("=add(A0, 1)");
  h.frame();
  assert_eq!(h.text(id("edit")), Some("=add(A0, 1)"), "the cell shows the edit");
  key(&mut h, NamedKey::Enter);
  assert_eq!((sheet(&h).shown((1, 1)), sheet(&h).sel), ("6".into(), (2, 1)));
  assert_eq!(h.focus_id(), Some(id("sheet")));
}

#[test]
fn values_show_without_float_noise() {
  let mut s = seven::Cells::default();
  s.cells.insert((0, 0), "=add(0.1, 0.2)".into());
  s.cells.insert((1, 0), "=sub(0, 0)".into());
  s.cells.insert((2, 0), "=div(1, 3)".into());
  let shown = (0..3).map(|r| s.shown((r, 0))).collect::<Vec<_>>();
  assert_eq!(shown, ["0.3", "0", "0.333333333"]);
}

#[test]
fn a_double_click_edits_and_return_recomputes() {
  let mut h = on(Task::Cells);
  edit(&mut h, 0, 0);
  assert_eq!(h.focus_id(), Some(id("edit")));
  h.key(Key::Char('a'), CMD);
  h.type_text("10");
  key(&mut h, NamedKey::Enter);
  assert!(h.app().cells.editing.is_none());
  assert_eq!(h.app().cells.shown((3, 0)), "34");
  h.frame();
  assert!(h.find(Role::StaticText, "68").is_some(), "{}", h.a11y());
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
  let today = DateFormat::ISO.format(Date::today());
  let start = format!("text field \"Departure\" value \"{today}\"");
  let back = format!("text field \"Return\" value \"{today}\" disabled");
  reads(&h.a11y(), &["pop up button \"Flight\" value \"one-way flight\"", &start, &back]);
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
      "text field \"Filter\" value \"\"",
      "row \"Emil, Hans\"",
      "button \"Update\" disabled",
      "button \"Delete\" disabled",
    ],
  );
}
