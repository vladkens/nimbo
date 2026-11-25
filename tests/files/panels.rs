// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of the files window's panels and drops through the harness: Open… and Save… ask for
//! their panels only when clicked, a picked path and a cancel come back to the app; files dragged
//! over the list light it, dragged away or dropped outside it (on the sidebar) leave it as it was,
//! dropped on it join it at its top.

use std::path::{Path, PathBuf};
use std::time::Duration;

use nimbo::{Event, FileDialog, Harness, Id, Point};

use crate::common::{diff, mid};
use crate::files::browser::LIST as ZONE;
use crate::files::{Files, LIST as SIDEBAR, OPEN, SAVE, SIZE};

fn files() -> Harness<Files> {
  let mut h = Harness::new(Files::default(), SIZE, 2.0);
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
  assert_eq!(asked.kind(), nimbo::FileDialogKind::Save);
  assert_eq!(asked.get_name(), Some("notes.txt"));
  assert_eq!(h.app().picked, Some(None));
}

/// Rewritten from the drop zone's: the list is the zone, lit by a ring inside its pane.
#[test]
fn files_dragged_over_the_list_light_it_until_they_leave() {
  let mut h = files();
  let zone = h.rect(Id::of(ZONE)).unwrap();
  let edge = |h: &mut Harness<Files>| h.render().pixel(zone.x + 0.5, zone.y + zone.h / 2.0);
  let idle = edge(&mut h);

  drag(&mut h, mid(zone), false);
  assert!(h.app().browser.over);
  let lit = edge(&mut h);
  assert!(diff(lit, idle) > 40, "the list's ring lit: {lit:?} from {idle:?}");

  h.event(Event::DragExit);
  h.wait(Duration::from_millis(300));
  assert!(!h.app().browser.over);
  assert!(diff(edge(&mut h), idle) < 4, "the ring gone");
}

/// Rewritten from the drop zone's: dropped on the sidebar, nothing; on the list, the file joins
/// it at the top.
#[test]
fn files_dropped_on_the_list_join_it_and_on_the_sidebar_not() {
  let mut h = files();
  let (zone, sidebar) = (h.rect(Id::of(ZONE)).unwrap(), h.rect(Id::of(SIDEBAR)).unwrap());
  drag(&mut h, mid(sidebar), true);
  assert!(h.app().browser.added.is_empty(), "dropped on the sidebar");

  drag(&mut h, mid(zone), false);
  drag(&mut h, mid(zone), true);
  let b = &h.app().browser;
  assert_eq!(b.added.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["a.txt"]);
  assert_eq!(b.name(b.key(0)), "a.txt", "at the top");
  assert!(!h.app().browser.over);
}
