// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests through the harness of a window that follows the system, with a fake lid: a change of the
//! system reaches the window as `Event::System`, which reads again and redraws (the count, the last
//! kind, the lid), the rows keeping their heights; other events change nothing; the tray's menu
//! from what came.

mod system {
  //! A window that follows the system's changes ([nimbo::system]): it reads again on each
  //! `Event::System` and redraws: the lid, the changes of each kind and the last; and the tray's
  //! menu a tray app would build from what came.

  use nimbo::system::Change;
  use nimbo::{App, Cx, Event, FormRow, FormStyle, Item, Menu, Shortcut, Style, Ui, grow};

  pub const KINDS: [Change; 3] = [Change::AudioInputs, Change::DefaultInput, Change::Lid];
  pub const LID: &str = "lid";
  pub const COUNT: &str = "count";
  pub const LAST: &str = "last";

  /// What came: the changes of each kind, the last, and the lid as read after it.
  #[derive(Clone, Debug, PartialEq)]
  pub struct Seen {
    pub counts: [u32; 3],
    pub last: Option<Change>,
    pub lid: Option<bool>,
  }

  impl Seen {
    pub fn new(lid: Option<bool>) -> Self {
      Self { counts: [0; 3], last: None, lid }
    }

    /// `change` came; the lid read again after it is `lid`.
    pub fn add(&mut self, change: Change, lid: Option<bool>) {
      if let Some(i) = KINDS.iter().position(|&k| k == change) {
        self.counts[i] += 1;
      }

      self.last = Some(change);
      self.lid = lid;
    }
  }

  pub fn lid_label(lid: Option<bool>) -> &'static str {
    match lid {
      Some(true) => "Closed",
      Some(false) => "Open",
      None => "None",
    }
  }

  pub fn kind_label(kind: Change) -> &'static str {
    match kind {
      Change::AudioInputs => "Audio inputs",
      Change::DefaultInput => "Default input",
      Change::Lid => "Lid",
      _ => "Other",
    }
  }

  /// The tray's menu: the lid and the audio changes, then the window and Quit.
  pub fn menu(s: &Seen) -> Menu<Action> {
    let audio = s.counts[0] + s.counts[1];
    Menu::new(vec![
      Item::button(Action::None, &format!("Lid: {}", lid_label(s.lid))).enabled(false),
      Item::button(Action::None, &format!("Audio changes: {audio}")).enabled(false),
      Item::Divider,
      Item::button(Action::Show, "Show Window"),
      Item::Divider,
      Item::button(Action::Quit, "Quit").key(Shortcut::char('q').cmd()),
    ])
  }

  /// What the tray's menu does; its status lines do nothing.
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Action {
    None,
    Show,
    Quit,
  }

  /// The window: what came, read again on each change.
  pub struct System {
    pub seen: Seen,
    /// How the lid is read: [hardware::lid_closed], a fake in the tests.
    pub read_lid: fn() -> Option<bool>,
  }

  impl System {
    pub fn new(read_lid: fn() -> Option<bool>) -> Self {
      Self { seen: Seen::new(read_lid()), read_lid }
    }
  }

  impl App for System {
    fn event(&mut self, _: &mut Cx, ev: &Event) -> bool {
      let Event::System(change) = ev else { return false };
      self.seen.add(*change, (self.read_lid)());
      true
    }

    fn view(&mut self, ui: &mut Ui) {
      let fs = FormStyle::plates(&ui.theme());
      let s = &self.seen;
      ui.col(Style::new().grow().bg(fs.bg()), |ui| {
        ui.form_scroll(&fs, Style::new().grow(), |ui| {
          ui.col(fs.page().pads([20.0, 20.0, 20.0, 20.0]).w(grow()), |ui| {
            ui.form_section(&fs, "Hardware", |ui| {
              ui.form_row(&fs, FormRow::new("Lid"), |ui| {
                ui.id(LID).text(lid_label(s.lid), fs.note())
              });
            });
            ui.form_section(&fs, "Changes", |ui| {
              for (i, &kind) in KINDS.iter().enumerate() {
                let row = FormRow::new(kind_label(kind));
                ui.form_row(&fs, row, |ui| ui.id((COUNT, i as u8)).text(s.counts[i], fs.note()));
              }

              let last = s.last.map_or("None", kind_label);
              ui.form_row(&fs, FormRow::new("Last"), |ui| ui.id(LAST).text(last, fs.note()));
            });
          });
        });
      });
    }
  }
}

use nimbo::harness::Image;
use nimbo::system::Change;
use nimbo::{Event, Harness, Id, Rect, Size};
use system::{COUNT, KINDS, LAST, LID, Seen, System};

fn closed() -> Option<bool> {
  Some(true)
}

fn window() -> Harness<System> {
  Harness::new(System::new(|| Some(false)), Size::new(420.0, 310.0), 2.0)
}

fn count(kind: Change) -> Id {
  let i = KINDS.iter().position(|&k| k == kind).unwrap();
  Id::of((COUNT, i as u8))
}

/// How many pixels of `r` differ between two frames.
fn changed(a: &Image, b: &Image, r: Rect) -> usize {
  let mut n = 0;
  for y in r.y as i32..(r.y + r.h) as i32 {
    for x in r.x as i32..(r.x + r.w) as i32 {
      n += (a.pixel(x as f32, y as f32) != b.pixel(x as f32, y as f32)) as usize;
    }
  }

  n
}

#[test]
fn a_change_redraws_the_window() {
  let mut h = window();
  let before = h.render();
  let rows: Vec<_> = KINDS.iter().map(|&k| h.rect(count(k)).unwrap().h).collect();
  assert_eq!(h.text(count(Change::AudioInputs)), Some("0"));
  assert_eq!(h.text(Id::of(LAST)), Some("None"));

  assert!(h.event(Event::System(Change::AudioInputs)));
  assert!(h.needs_frame());
  let after = h.render();
  assert_eq!(h.text(count(Change::AudioInputs)), Some("1"));
  assert_eq!(h.text(Id::of(LAST)), Some("Audio inputs"));
  assert_eq!(h.app().seen.counts, [1, 0, 0]);
  let last = h.rect(Id::of(LAST)).unwrap();
  assert!(changed(&before, &after, last) > 0, "the last kind's row drawn again");
  let now: Vec<_> = KINDS.iter().map(|&k| h.rect(count(k)).unwrap().h).collect();
  assert_eq!(rows, now, "rows keep their heights");
}

/// The lid read again after its change: closed now.
#[test]
fn the_lid_is_read_again() {
  let mut h = window();
  h.render();
  assert_eq!(h.text(Id::of(LID)), Some("Open"));
  h.app_mut().read_lid = closed;
  assert!(h.event(Event::System(Change::Lid)));
  h.render();
  assert_eq!(h.text(Id::of(LID)), Some("Closed"));
  assert_eq!(h.text(count(Change::Lid)), Some("1"));
}

/// Input that isn't a change of the system leaves what came alone.
#[test]
fn other_events_change_nothing() {
  let mut h = window();
  h.render();
  h.event(Event::Focus(true));
  h.move_to(10.0, 10.0);
  assert_eq!(h.app().seen, Seen::new(Some(false)));
}

/// The tray's menu: the lid and the audio changes, both kinds counted.
#[test]
fn the_tray_menu() {
  let mut s = Seen::new(None);
  let labels =
    |s: &Seen| system::menu(s).items().iter().map(|i| i.label().to_owned()).collect::<Vec<_>>();
  assert_eq!(labels(&s)[..2], ["Lid: None", "Audio changes: 0"]);
  s.add(Change::AudioInputs, Some(false));
  s.add(Change::DefaultInput, Some(false));
  s.add(Change::Lid, Some(true));
  assert_eq!(labels(&s)[..2], ["Lid: Closed", "Audio changes: 2"]);
  assert_eq!(s.last, Some(Change::Lid));
}
