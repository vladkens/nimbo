// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/files.rs` through the harness: Open… and Save… ask for their panels only
//! when clicked, a picked path and a cancel come back to the app; files dragged over the zone
//! light it, dragged away or dropped outside it leave it as it was, dropped on it list there.

#[path = "../examples/files.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod files;

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{diff, mid};
use files::{Files, OPEN, SAVE, ZONE};
use nimbo::{Event, FileDialog, Harness, Id, Point, Size};

fn files() -> Harness<Files> {
  let mut h = Harness::new(Files::default(), Size::new(420.0, 300.0), 2.0);
  h.frame();
  h
}

fn drag(h: &mut Harness<Files>, (x, y): (f32, f32), drop: bool) {
  let (pos, paths) = (Point::new(x, y), vec![PathBuf::from("/tmp/a.txt")]);
  h.event(if drop { Event::Drop { pos, paths } } else { Event::DragOver { pos, paths } });
  h.wait(Duration::from_millis(300));
}

#[test]
fn open_asks_for_its_panel_only_when_clicked() {
  let mut h = files();
  assert_eq!(h.file_dialog(None), None, "no panel before a click");

  h.click_id(Id::of(OPEN));
  let asked = h.file_dialog(Some(Path::new("/tmp/notes.txt")));
  assert_eq!(asked, Some(FileDialog::open("Pick a text file").types(&["txt", "md"])));
  assert_eq!(h.app().picked, Some(Some(PathBuf::from("/tmp/notes.txt"))));
}

#[test]
fn save_suggests_a_name_and_a_cancel_comes_back() {
  let mut h = files();
  h.click_id(Id::of(SAVE));
  let asked = h.file_dialog(None).expect("the save panel");
  assert!(asked.save);
  assert_eq!(asked.name.as_deref(), Some("notes.txt"));
  assert_eq!(h.app().picked, Some(None));
}

#[test]
fn files_dragged_over_the_zone_light_it_until_they_leave() {
  let mut h = files();
  let zone = h.rect(Id::of(ZONE)).unwrap();
  let edge = |h: &mut Harness<Files>| h.render().pixel(zone.x + 0.5, zone.y + zone.h / 2.0);
  let idle = edge(&mut h);

  drag(&mut h, mid(zone), false);
  assert!(h.app().over);
  let lit = edge(&mut h);
  assert!(diff(lit, idle) > 40, "the zone's border lit: {lit:?} from {idle:?}");

  h.event(Event::DragExit);
  h.wait(Duration::from_millis(300));
  assert!(!h.app().over);
  assert!(diff(edge(&mut h), idle) < 4, "the border back");
}

#[test]
fn files_dropped_on_the_zone_list_there_and_elsewhere_not() {
  let mut h = files();
  let zone = h.rect(Id::of(ZONE)).unwrap();
  drag(&mut h, (zone.x + zone.w / 2.0, zone.y - 20.0), true);
  assert!(h.app().dropped.is_empty(), "dropped above the zone");

  drag(&mut h, mid(zone), false);
  drag(&mut h, mid(zone), true);
  assert_eq!(h.app().dropped, [PathBuf::from("/tmp/a.txt")]);
  assert!(!h.app().over);
}
