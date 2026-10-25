// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Files in and out: Open… and Save… ask for the system's panels (a text file to open, notes to
//! save under a suggested name), the answer shown under them, and a zone to drop files on, lit
//! while files are dragged over it, listing the files dropped there.
//!
//! `cargo run --release --example files [-- --shot <file.png>]`; the shot has files dragged
//! over the zone.

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;

use nimbo::{App, Config, Cx, Event, FileDialog, Id, Point, Style, Ui, alpha, mix};

#[derive(Default)]
pub struct Files {
  /// The last panel's answer: a path, or `None` when cancelled.
  pub picked: Option<Option<PathBuf>>,
  pub dropped: Vec<PathBuf>,
  /// Files are dragged over the zone.
  pub over: bool,
}

pub const OPEN: &str = "open";
pub const SAVE: &str = "save";
pub const ZONE: &str = "zone";

impl App for Files {
  fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
    let zone = cx.hovered(Id::of(ZONE));
    match ev {
      Event::DragOver { .. } => self.over = zone,
      Event::DragExit => self.over = false,
      Event::Drop { paths, .. } => {
        if zone {
          self.dropped = paths.clone();
        }

        self.over = false;
      }
      Event::FileDialog(path) => self.picked = Some(path.clone()),
      _ => return false,
    }

    true
  }

  fn view(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    ui.col(Style::new().grow().pad(20.0).gap(12.0).bg(t.bg), |ui| {
      ui.row(Style::new().gap(8.0), |ui| {
        if ui.id(OPEN).button("Open…").clicked {
          ui.file_dialog(FileDialog::open("Pick a text file").types(&["txt", "md"]));
        }

        if ui.id(SAVE).button("Save…").clicked {
          ui.file_dialog(FileDialog::save("Save the notes").name("notes.txt"));
        }
      });
      match &self.picked {
        None => ui.label("Nothing picked yet"),
        Some(None) => ui.label("Cancelled"),
        Some(Some(p)) => ui.label(p.display()),
      }

      let lit = ui.hot(Id::of(ZONE), self.over);
      let (line, fill) = (mix(t.line, t.accent, lit), alpha(t.accent, 0.12 * lit));
      let zone = Style::new().grow().center().gap(4.0).radius(10.0).border(1.5, line).bg(fill);
      ui.id(ZONE).col(zone, |ui| match self.dropped.is_empty() {
        true => ui.label("Drop files here"),
        false => self.dropped.iter().for_each(|p| ui.label(p.display())),
      });
    });
  }
}

fn main() {
  // The shot: two files dragged over the zone.
  nimbo::run(Config::new("Files", 420.0, 300.0), Files::default(), |h| {
    h.frame();
    let paths = vec![PathBuf::from("/tmp/notes.txt"), PathBuf::from("/tmp/todo.md")];
    h.event(Event::DragOver { pos: Point::new(210.0, 200.0), paths });
    h.wait(std::time::Duration::from_millis(400));
  });
}
