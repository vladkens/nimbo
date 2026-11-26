// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/custom-look.rs` through the harness: its look paints from its own palette
//! in dark and light, its tooltip too, and the quiet row keeps its own button style; while the
//! behaviours under the look do what they do under nimbo's: Tab through the enabled controls, the
//! pointing hand over what clicks, the roles and states assistive technology reads, a click, a
//! toggle, a slider's drag, typing and a pick changing the app, and disabled controls taking none
//! of it. The styles set none of these.

#[path = "../examples/custom-look.rs"]
#[expect(dead_code, reason = "its main and what this file doesn't use")]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[cfg_attr(windows, expect(unused_attributes, reason = "its windows_subsystem"))]
mod custom_look;

#[expect(dead_code, reason = "this file uses a part of the helpers")]
mod common;

use std::time::Duration;

use common::{mid, px, reads};
use custom_look::*;
use nimbo::{Appearance, Cursor, Event, Harness, Id, Key, Modifiers, NamedKey, Rect, Size};

type H = Harness<Sift>;

fn sift(look: Appearance) -> H {
  let mut h = Harness::new(Sift::default(), Size::new(640.0, 360.0), 2.0);
  h.event(Event::Appearance(look));
  h.frame();
  h
}

fn rect(h: &H, name: &str) -> Rect {
  h.rect(Id::of(name)).unwrap_or_else(|| panic!("no {name}"))
}

/// Row `i` of the queue.
fn row(h: &H, i: u64) -> Rect {
  h.rect(Id::of(QUEUE).key(("row", i))).unwrap()
}

/// Option `i` of the kind's picker.
fn tile(i: usize) -> Id {
  Id::of(KIND).key(("item", i))
}

/// Types `title` into the title field, the pointer then away and the hover faded.
fn title(h: &mut H, title: &str) {
  h.click_id(Id::of(TITLE));
  h.type_text(title);
  h.leave();
  h.wait(Duration::from_millis(300));
}

#[test]
fn the_palette_paints_dark_and_light() {
  for (look, p) in [(Appearance::DARK, Palette::DARK), (Appearance::LIGHT, Palette::LIGHT)] {
    let mut h = sift(look);
    title(&mut h, "Pin the formatter");
    let img = h.render();
    assert_eq!(img.pixel(4.0, 4.0), px(p.panel), "{look:?}: the bar");

    let list = rect(&h, QUEUE);
    assert_eq!(img.pixel(list.x + 20.0, list.y + list.h - 20.0), px(p.panel), "the queue");
    let ground = (list.x + list.w + 6.0, list.y + list.h - 80.0);
    assert_eq!(img.pixel(ground.0, ground.1), px(p.ground), "the working area");
    let field = rect(&h, TITLE);
    assert_eq!(img.pixel(field.x + field.w - 10.0, field.y + 12.0), px(p.card), "the field's");
    let file = rect(&h, FILE);
    assert_eq!(img.pixel(file.x + 6.0, file.y + 12.0), px(p.accent), "the button's fill");

    let blocked = rect(&h, BLOCKED);
    let inside = blocked.y + blocked.h / 2.0;
    assert_eq!(img.pixel(blocked.x + 4.0, inside), px(p.card), "an off box");
    let review = rect(&h, REVIEW);
    assert_eq!(img.pixel(review.x + 2.0, inside), px(p.accent), "an on box");

    // The window's tooltip in the help style: a slip of the panel.
    h.move_to(file.x + 30.0, file.y + 10.0);
    h.wait(Duration::from_millis(400));
    let tip = h.rect(Id::of(FILE).key("tip")).expect("the tip shows");
    assert_eq!(h.render().pixel(tip.x + 4.0, tip.y + tip.h / 2.0), px(p.panel), "the tip");
  }
}

#[test]
fn the_quiet_row_keeps_its_own_button_style() {
  let mut h = sift(Appearance::DARK);
  title(&mut h, "Pin the formatter");
  let img = h.render();
  let (file, clear) = (rect(&h, FILE), rect(&h, CLEAR));
  let p = Palette::DARK;
  assert_eq!(img.pixel(file.x + 6.0, file.y + 12.0), px(p.accent), "Solid outside");
  assert_eq!(img.pixel(clear.x + 6.0, clear.y + 12.0), px(p.ground), "Hairline in it");
  assert_eq!(img.pixel(clear.x + clear.w / 2.0, clear.y + 0.25), px(p.line));
}

#[test]
fn tab_goes_through_the_enabled_controls() {
  let mut h = sift(Appearance::DARK);
  let tabs = |h: &mut H, n: usize| {
    (0..n)
      .map(|_| {
        h.key(NamedKey::Tab, Modifiers::empty());
        h.frame();
        h.focus_id()
      })
      .collect::<Vec<_>>()
  };

  // File and Close disabled: skipped, and back to the queue after the last.
  let order =
    [QUEUE, TITLE, KIND, PRIORITY, BLOCKED, REVIEW, CLEAR, QUEUE].map(|n| Some(Id::of(n)));
  assert_eq!(tabs(&mut h, 8), order);

  // A title enables File, a row Close; a chore disables Needs review.
  title(&mut h, "Pin the formatter");
  h.click(row(&h, 0).x + 20.0, row(&h, 0).y + 10.0);
  h.click_id(tile(2));
  h.set_focus(None);
  let order = [QUEUE, TITLE, KIND, PRIORITY, BLOCKED, FILE, CLEAR, CLOSE].map(|n| Some(Id::of(n)));
  assert_eq!(tabs(&mut h, 8), order);
}

#[test]
fn the_pointing_hand_shows_over_what_clicks() {
  let mut h = sift(Appearance::DARK);
  let at = |h: &mut H, (x, y): (f32, f32)| {
    h.move_to(x, y);
    h.frame();
    h.cursor()
  };

  for (what, r) in [
    ("a segment", h.rect(tile(1)).unwrap()),
    ("the slider", rect(&h, PRIORITY)),
    ("a toggle", rect(&h, BLOCKED)),
    ("a button", rect(&h, CLEAR)),
    ("a row", row(&h, 2)),
  ] {
    assert_eq!(at(&mut h, mid(r)), Cursor::Pointer, "{what}");
  }

  let (field, close) = (mid(rect(&h, TITLE)), mid(rect(&h, CLOSE)));
  assert_eq!(at(&mut h, field), Cursor::Text, "the field's I-beam");
  assert_eq!(at(&mut h, close), Cursor::Arrow, "a disabled button");
  assert_eq!(at(&mut h, (4.0, 4.0)), Cursor::Arrow, "the bar");
}

#[test]
fn assistive_technology_reads_the_behaviours_roles() {
  let mut h = sift(Appearance::DARK);
  reads(
    &h.a11y(),
    &[
      "list \"Queue\"",
      "row \"P1 Crash on an empty search Bug\"",
      "text field \"Title\" value \"\"",
      "radio group \"Kind\"",
      "radio button \"Bug\" on selected",
      "radio button \"Chore\" off",
      "static text \"Priority\" desc \"1 is the most urgent\"",
      "slider \"Priority\" 2 in 1..4",
      "check box \"Blocked\" off",
      "check box \"Needs review\" on",
      "button \"File\" disabled desc \"Adds it to the queue\"",
      "button \"Clear\"",
      "button \"Close\" disabled",
      "static text \"4 open · 1 blocked\"",
    ],
  );

  h.click_id(tile(2));
  h.frame();
  reads(
    &h.a11y(),
    &["radio button \"Chore\" on selected", "check box \"Needs review\" on disabled"],
  );
}

#[test]
fn typing_a_title_and_file_queue_it() {
  let mut h = sift(Appearance::DARK);
  title(&mut h, "Pin the formatter");
  h.click_id(Id::of(BLOCKED));
  assert_eq!(h.app().title, "Pin the formatter");
  h.click_id(Id::of(FILE));
  let last = h.app().queue.last().unwrap();
  assert_eq!((last.title.as_str(), last.priority, last.blocked), ("Pin the formatter", 2, true));
  assert_eq!(h.app().title, "", "the form is free for the next");
  h.frame();
  reads(&h.a11y(), &["static text \"5 open · 2 blocked\""]);
}

#[test]
fn a_chore_goes_in_unreviewed() {
  let mut h = sift(Appearance::DARK);
  h.click_id(tile(2));
  assert_eq!(h.app().kind, Kind::Chore);
  title(&mut h, "Pin the formatter");
  h.click_id(Id::of(FILE));
  assert!(h.app().review, "the toggle keeps its value");
  assert!(!h.app().queue.last().unwrap().review);
}

#[test]
fn a_toggle_flips_on_a_click_and_space() {
  let mut h = sift(Appearance::DARK);
  h.click_id(Id::of(BLOCKED));
  assert!(h.app().blocked);
  h.set_focus(Some(Id::of(BLOCKED)));
  h.key(Key::Char(' '), Modifiers::empty());
  h.frame();
  assert!(!h.app().blocked);
}

#[test]
fn the_slider_drags_from_step_to_step() {
  let mut h = sift(Appearance::DARK);
  let r = rect(&h, PRIORITY);
  // Its rail runs inside 4 points of padding, 1 to 4.
  let x = |v: f32| r.x + 4.0 + (r.w - 8.0) * (v - 1.0) / 3.0;
  let y = r.y + r.h / 2.0;
  h.press(x(1.3), y);
  h.frame();
  assert_eq!(h.app().priority, 1.0);

  h.move_to(x(3.8), y);
  h.frame();
  assert_eq!(h.app().priority, 4.0);

  h.release(x(3.8), y);
  h.frame();
  assert_eq!(h.app().priority, 4.0);
  reads(&h.a11y(), &["static text \"P4\"", "slider \"Priority\" 4 in 1..4"]);
}

#[test]
fn a_row_selects_and_close_removes_it() {
  let mut h = sift(Appearance::DARK);
  let r = row(&h, 1);
  h.click(r.x + 20.0, r.y + r.h / 2.0);
  assert!(h.app().sel.contains(1));
  h.frame();
  h.click_id(Id::of(CLOSE));
  let left = h.app().queue.iter().map(|i| i.priority).collect::<Vec<_>>();
  assert_eq!(left, [1, 3, 4]);
  assert!(h.app().sel.is_empty());
}

#[test]
fn clear_empties_the_form_and_keeps_the_queue() {
  let mut h = sift(Appearance::DARK);
  title(&mut h, "Pin the formatter");
  h.click_id(tile(1));
  h.click_id(Id::of(CLEAR));
  assert_eq!((h.app().title.as_str(), h.app().kind), ("", Kind::Bug));
  assert_eq!(h.app().queue.len(), 4);
}

#[test]
fn disabled_controls_ignore_input() {
  let mut h = sift(Appearance::DARK);
  // File with no title, Close with no row selected.
  h.click_id(Id::of(FILE));
  h.click_id(Id::of(CLOSE));
  assert_eq!(h.app().queue.len(), 4);

  // Needs review on a chore: neither a click nor Tab's focus.
  h.click_id(tile(2));
  h.frame();
  h.click_id(Id::of(REVIEW));
  for _ in 0..8 {
    h.key(NamedKey::Tab, Modifiers::empty());
    h.frame();
    assert_ne!(h.focus_id(), Some(Id::of(REVIEW)));
  }

  assert!(h.app().review);
}
