// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/custom-look.rs` through the harness: its look paints from its own palette in
//! light and dark, its tooltip too, and the quiet row keeps its own button style; while the
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

type H = Harness<Brew>;

fn brew(look: Appearance) -> H {
  let mut h = Harness::new(Brew::default(), Size::new(600.0, 420.0), 2.0);
  h.event(Event::Appearance(look));
  h.frame();
  h
}

fn rect(h: &H, name: &str) -> Rect {
  h.rect(Id::of(name)).unwrap_or_else(|| panic!("no {name}"))
}

/// Row `i` of the saved recipes.
fn row(h: &H, i: u64) -> Rect {
  h.rect(Id::of(SAVED).key(("row", i))).unwrap()
}

/// Option `i` of the method's picker.
fn tile(i: usize) -> Id {
  Id::of(METHOD).key(("item", i))
}

/// Types `name` into the name field, the pointer then away and the hover faded.
fn name(h: &mut H, name: &str) {
  h.click_id(Id::of(NAME));
  h.type_text(name);
  h.leave();
  h.wait(Duration::from_millis(300));
}

#[test]
fn the_palette_paints_light_and_dark() {
  for (look, p) in [(Appearance::LIGHT, Palette::LIGHT), (Appearance::DARK, Palette::DARK)] {
    let mut h = brew(look);
    name(&mut h, "Overnight jar");
    let img = h.render();
    assert_eq!(img.pixel(4.0, 4.0), px(p.ground), "{look:?}: the ground");

    let list = rect(&h, SAVED);
    assert_eq!(img.pixel(list.x + 20.0, list.y + list.h - 20.0), px(p.card), "the list's card");
    let field = rect(&h, NAME);
    assert_eq!(img.pixel(field.x + field.w - 10.0, field.y + 15.0), px(p.card), "the field's");
    let save = rect(&h, SAVE);
    assert_eq!(img.pixel(save.x + 8.0, save.y + 15.0), px(p.accent), "the button's fill");

    let iced = rect(&h, ICED);
    assert_eq!(img.pixel(iced.x + 30.0, iced.y + 10.0), px(p.line), "an off toggle's track");

    // The window's tooltip in the help style: a slip of ink.
    h.move_to(save.x + 40.0, save.y + 10.0);
    h.wait(Duration::from_millis(400));
    let tip = h.rect(Id::of(SAVE).key("tip")).expect("the tip shows");
    assert_eq!(h.render().pixel(tip.x + 3.0, tip.y + tip.h / 2.0), px(p.ink), "the tip");
  }
}

#[test]
fn the_quiet_row_keeps_its_own_button_style() {
  let mut h = brew(Appearance::LIGHT);
  name(&mut h, "Overnight jar");
  let img = h.render();
  let (save, clear) = (rect(&h, SAVE), rect(&h, CLEAR));
  assert_eq!(img.pixel(save.x + 8.0, save.y + 15.0), px(Palette::LIGHT.accent), "Bean outside");
  assert_eq!(img.pixel(clear.x + 8.0, clear.y + 15.0), px(Palette::LIGHT.ground), "Quiet in it");
  assert_eq!(img.pixel(clear.x + clear.w / 2.0, clear.y + 0.5), px(Palette::LIGHT.line));
}

#[test]
fn tab_goes_through_the_enabled_controls() {
  let mut h = brew(Appearance::LIGHT);
  let tabs = |h: &mut H, n: usize| {
    (0..n)
      .map(|_| {
        h.key(NamedKey::Tab, Modifiers::empty());
        h.frame();
        h.focus_id()
      })
      .collect::<Vec<_>>()
  };

  // Save and Delete disabled: skipped, and back to the list after the last.
  let order = [SAVED, NAME, METHOD, RATIO, BLOOM, ICED, CLEAR, SAVED].map(|n| Some(Id::of(n)));
  assert_eq!(tabs(&mut h, 8), order);

  // A name enables Save, a row Delete; cold brew disables Bloom.
  name(&mut h, "Overnight jar");
  h.click(row(&h, 0).x + 20.0, row(&h, 0).y + 10.0);
  h.click_id(tile(2));
  h.set_focus(None);
  let order = [SAVED, NAME, METHOD, RATIO, ICED, SAVE, CLEAR, DELETE].map(|n| Some(Id::of(n)));
  assert_eq!(tabs(&mut h, 8), order);
}

#[test]
fn the_pointing_hand_shows_over_what_clicks() {
  let mut h = brew(Appearance::LIGHT);
  let at = |h: &mut H, (x, y): (f32, f32)| {
    h.move_to(x, y);
    h.frame();
    h.cursor()
  };

  for (what, r) in [
    ("a tile", h.rect(tile(1)).unwrap()),
    ("the slider", rect(&h, RATIO)),
    ("a toggle", rect(&h, ICED)),
    ("a button", rect(&h, CLEAR)),
    ("a row", row(&h, 2)),
  ] {
    assert_eq!(at(&mut h, mid(r)), Cursor::Pointer, "{what}");
  }

  let (field, delete) = (mid(rect(&h, NAME)), mid(rect(&h, DELETE)));
  assert_eq!(at(&mut h, field), Cursor::Text, "the field's I-beam");
  assert_eq!(at(&mut h, delete), Cursor::Arrow, "a disabled button");
  assert_eq!(at(&mut h, (4.0, 4.0)), Cursor::Arrow, "the ground");
}

#[test]
fn assistive_technology_reads_the_behaviours_roles() {
  let mut h = brew(Appearance::LIGHT);
  reads(
    &h.a11y(),
    &[
      "list",
      "row \"Morning V60\"",
      "text field \"Recipe name\" value \"\"",
      "radio group \"Method\"",
      "radio button \"Pour-over\" on selected",
      "radio button \"Cold brew\" off",
      "static text \"Ratio\" desc \"Grams of water for each gram of coffee\"",
      "slider \"Ratio\" 16 in 10..18",
      "check box \"Bloom first\" on",
      "check box \"Over ice\" off",
      "button \"Save\" disabled desc \"Adds it to the recipes\"",
      "button \"Clear\"",
      "button \"Delete\" disabled",
    ],
  );

  h.click_id(tile(2));
  h.frame();
  reads(
    &h.a11y(),
    &["radio button \"Cold brew\" on selected", "check box \"Bloom first\" on disabled"],
  );
}

#[test]
fn typing_names_a_recipe_and_save_adds_it() {
  let mut h = brew(Appearance::LIGHT);
  name(&mut h, "Overnight jar");
  assert_eq!(h.app().name, "Overnight jar");
  assert!(h.click_id(Id::of(SAVE)));
  assert_eq!((h.app().saved.len(), h.app().saved[3].as_str()), (4, "Overnight jar"));
  assert_eq!(h.app().name, "", "the form is free for the next");
}

#[test]
fn a_tile_picks_the_method() {
  let mut h = brew(Appearance::LIGHT);
  h.click_id(tile(1));
  assert_eq!(h.app().method, Method::Press);
  h.click_id(tile(2));
  assert_eq!(h.app().method, Method::ColdBrew);
}

#[test]
fn a_toggle_flips_on_a_click_and_space() {
  let mut h = brew(Appearance::LIGHT);
  h.click_id(Id::of(ICED));
  assert!(h.app().iced);
  h.set_focus(Some(Id::of(ICED)));
  h.key(Key::Char(' '), Modifiers::empty());
  h.frame();
  assert!(!h.app().iced);
}

#[test]
fn the_slider_drags_from_step_to_step() {
  let mut h = brew(Appearance::LIGHT);
  let r = rect(&h, RATIO);
  // Its track runs inside 4 points of padding, 10 to 18.
  let x = |v: f32| r.x + 4.0 + (r.w - 8.0) * (v - 10.0) / 8.0;
  let y = r.y + r.h / 2.0;
  h.press(x(12.3), y);
  h.frame();
  assert_eq!(h.app().ratio, 12.0);

  h.move_to(x(16.8), y);
  h.frame();
  assert_eq!(h.app().ratio, 17.0);

  h.release(x(16.8), y);
  h.frame();
  assert_eq!(h.app().ratio, 17.0);
  reads(&h.a11y(), &["static text \"1:17\"", "slider \"Ratio\" 17 in 10..18"]);
}

#[test]
fn a_row_selects_and_delete_removes_it() {
  let mut h = brew(Appearance::LIGHT);
  let r = row(&h, 1);
  h.click(r.x + 20.0, r.y + r.h / 2.0);
  assert!(h.app().sel.contains(1));
  h.frame();
  assert!(h.click_id(Id::of(DELETE)));
  assert_eq!(h.app().saved, ["Morning V60", "Iced pour-over"]);
  assert!(h.app().sel.is_empty());
}

#[test]
fn disabled_controls_ignore_input() {
  let mut h = brew(Appearance::LIGHT);
  // Save with no name, Delete with no row selected.
  h.click_id(Id::of(SAVE));
  h.click_id(Id::of(DELETE));
  assert_eq!(h.app().saved.len(), 3);

  // Bloom under cold brew: neither a click nor Tab's focus.
  h.click_id(tile(2));
  h.frame();
  h.click_id(Id::of(BLOOM));
  for _ in 0..8 {
    h.key(NamedKey::Tab, Modifiers::empty());
    h.frame();
    assert_ne!(h.focus_id(), Some(Id::of(BLOOM)));
  }

  assert!(h.app().bloom);
}
