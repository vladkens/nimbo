// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Harness tests for `examples/gallery.rs`: both panes edit one state, each in its theme: a
//! check box clicked in the light pane shows checked in the dark one, in its accent; a switch
//! flips and its knob slides; the slider drags; a select opens and chooses by letters; a
//! segment picks; an icon button's tooltip shows after a rest; a recorder records a chord; the
//! dialog opens in the dark pane's theme, Escape closes it and the action counts. The search
//! field is drawn in each pane's theme, and what is typed in one clears from the other's ⊗.
//! Assistive technology reads each widget by its role.

#[path = "../examples/gallery.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod gallery;

mod common;

use std::time::Duration;

use common::{diff, mid, px, reads};
use gallery::*;
use nimbo::{Appearance, Event, Harness, Id, Modifiers, NamedKey, Rect, Size, Theme, alpha, mix};

type H = Harness<Gallery>;

fn id(name: &str, pane: usize) -> Id {
  Id::of((name, pane))
}

fn rect(h: &H, name: &str, pane: usize) -> Rect {
  h.rect(id(name, pane)).unwrap_or_else(|| panic!("no {name} in pane {pane}"))
}

#[test]
fn every_widget_in_both_themes() {
  let mut h = Harness::new(Gallery::default(), Size::new(1040.0, 590.0), 2.0);
  h.event(Event::Appearance(Appearance::LIGHT));
  h.frame();

  // The panes' grounds, light and dark.
  let img = h.render();
  assert_eq!(img.pixel(4.0, 550.0), px(Theme::PLATES_LIGHT.bg));
  assert_eq!(img.pixel(1036.0, 550.0), px(Theme::PLATES_DARK.bg));

  // A check box clicked in the light pane unchecks it in both; again, checked in the dark
  // pane's accent.
  h.click_id(id(CHECK, 0));
  assert!(!h.app().wrap);
  h.click_id(id(CHECK, 0));
  h.leave();
  h.wait(Duration::from_millis(300));
  let c = rect(&h, CHECK, 1);
  assert_eq!(h.render().pixel(c.x + 8.0, c.y + c.h / 2.0 - 6.0), px(Theme::PLATES_DARK.accent));

  // The switch flips and its knob slides over 140 ms.
  let t = rect(&h, TOGGLE, 1);
  let at = |h: &mut H| h.render().pixel(t.x + 6.0, t.y + t.h / 2.0);
  let off = at(&mut h);
  h.click_id(id(TOGGLE, 1));
  assert!(h.app().live);
  h.leave();
  h.wait(Duration::from_millis(200));
  assert_eq!(at(&mut h), px(Theme::PLATES_DARK.accent), "from {off:?}");

  // The slider drags to its middle.
  let s = rect(&h, SLIDER, 1);
  h.press(s.x + 2.0, s.y + s.h / 2.0);
  h.frame();
  h.move_to(s.x + s.w / 2.0, s.y + s.h / 2.0);
  h.frame();
  h.release(s.x + s.w / 2.0, s.y + s.h / 2.0);
  assert!((h.app().zoom - 100.0).abs() < 1.0, "{}", h.app().zoom);

  // The select opens on a click and chooses by letters typed in its menu.
  h.click_id(id(SIZE, 0));
  h.frame();
  h.frame();
  assert!(h.rect(id(SIZE, 0).key("menu")).is_some());
  h.key('l', Modifiers::empty());
  h.key(NamedKey::Enter, Modifiers::empty());
  h.frame();
  assert_eq!(h.app().size, gallery::Size::Large);

  // A segment picks its value.
  h.click_id(id(VIEW, 1).key(2));
  assert_eq!(h.app().view, 2);

  // The reset's tooltip shows after a rest.
  let r = rect(&h, RESET, 0);
  let (x, y) = mid(r);
  h.move_to(x, y);
  h.wait(Duration::from_millis(400));
  assert!(h.rect(id(RESET, 0).key("tip")).is_some());

  // A recorder records a chord, shown in both panes.
  h.click_id(id(HOTKEY, 1));
  h.key(NamedKey::F5, Modifiers::ALT);
  assert_eq!(h.app().hotkey.map(|k| k.to_string()).as_deref(), Some("option+f5"));
  // The ⊗ of the recorder that has one clears it.
  h.frame();
  h.click_id(id(CLEARABLE, 0).key("clear"));
  assert_eq!(h.app().hotkey, None);

  // The dialog opens in the dark pane's theme; Escape closes it, Space on the action deletes.
  let dialog = Id::of(DIALOG);
  h.click_id(id(ASK, 1));
  h.wait(Duration::from_millis(200));
  let card = h.rect(dialog.key("card")).expect("the dialog");
  let fill = h.render().pixel(card.x + card.w / 2.0, card.y + 8.0);
  assert_eq!(fill, px(Theme::PLATES_DARK.dialog));
  h.key(NamedKey::Escape, Modifiers::empty());
  h.frame();
  assert!(h.app().ask.is_none() && h.rect(dialog.key("card")).is_none());
  h.click_id(id(ASK, 0));
  h.frame();
  h.click_id(dialog.key("action"));
  assert_eq!((h.app().deleted, h.app().ask), (1, None));
}

#[test]
fn the_search_field_in_both_themes() {
  let mut h = Harness::new(Gallery::default(), Size::new(1040.0, 590.0), 2.0);
  h.event(Event::Appearance(Appearance::LIGHT));
  h.frame();

  // Each pane's field: the theme's search look over its ground, a touch lighter in the dark.
  let img = h.render();
  for (pane, t) in [(0, Theme::PLATES_LIGHT), (1, Theme::PLATES_DARK)] {
    let r = rect(&h, SEARCH, pane);
    let fill = t.search().frame.bg;
    let want = px(mix(t.bg, alpha(fill, 1.0), fill[3]));
    let got = img.pixel(r.x + r.w / 2.0, r.y + 4.0);
    assert!(diff(got, want) <= 1, "pane {pane}: {got:?}, not {want:?}");
  }

  // Cleared from the dark pane's ⊗, typed in the light one: both show it.
  h.click_id(id(SEARCH, 1).key("clear"));
  h.frame();
  assert_eq!(h.app().search, "");
  assert!(h.rect(id(SEARCH, 0).key("clear")).is_none());
  h.type_text("icons");
  h.frame();
  assert_eq!(h.focused(), Some(id(SEARCH, 1)));
  h.click_id(id(SEARCH, 0));
  h.type_text("!");
  h.frame();
  assert_eq!(h.app().search, "icons!");
  assert!(h.rect(id(SEARCH, 1).key("clear")).is_some());
}

/// Each kind of widget is what it is to assistive technology: its role, name, value and states.
#[test]
fn every_widget_has_its_role() {
  let mut h = Harness::new(Gallery::default(), Size::new(1040.0, 590.0), 2.0);
  h.frame();
  let lines = [
    "button \"Save\"",
    "button \"Off\" disabled",
    "button \"Reset\" desc \"Back to the default\"",
    "button \"Info\" desc \"An ⓘ explains what one line can't\"",
    "button \"Warning\" desc \"A warning tells what is wrong\"",
    "check box \"Wrap lines\" on",
    "check box \"Off\" off disabled",
    "switch \"Live\" off",
    "switch \"Large\" on",
    "slider \"Zoom\" 60 in 0..200",
    "pop up button \"Size\" value \"Medium\"",
    "pop up button \"Size\" value \"Small\" disabled",
    "radio group",
    "radio button \"List 128\" on selected",
    "radio button \"Grid 12\" off",
    "progress indicator 0.42 in 0..1",
    "link \"Learn more…\"",
    "static text \"new\"",
    "text field \"Search widgets\" value \"Switches\"",
    "button \"Clear\"",
  ];
  reads(&h.a11y(), &lines);
}

/// The ⊗ of a recorder and of a search field are beside them, not inside the leaves, where a
/// screen reader wouldn't reach them.
#[test]
fn clear_marks_are_beside_their_controls() {
  let mut h = Harness::new(Gallery::default(), Size::new(1040.0, 590.0), 2.0);
  h.frame();
  let dump = h.a11y();
  // The recorder's value is the platform's label of its shortcut (⌃⌥Space, Ctrl+Alt+Space).
  let lines: Vec<&str> = dump.lines().collect();
  let next = |first: &str| lines.iter().rposition(|l| l.starts_with(first)).map(|i| lines[i + 1]);
  for first in ["button value ", "text field \"Search widgets\" value \"Switches\""] {
    assert_eq!(next(first), Some("button \"Clear\""), "{first} in\n{dump}");
  }
}

/// The Metal renderer draws both panes as the CPU does, and again after text typed, a select's
/// menu and a tooltip, whose new glyphs reach it by the sync after each frame.
#[cfg(all(feature = "metal", target_os = "macos"))]
#[test]
fn metal_draws_as_the_cpu() {
  let mut h = Harness::new(Gallery::default(), Size::new(1040.0, 590.0), 2.0);
  h.event(Event::Appearance(Appearance::LIGHT));
  common::metal_parity(&mut h, 0.005);
  h.click_id(id(SEARCH, 1).key("clear"));
  h.type_text("Quixotic jabberwocky 0429");
  h.wait(Duration::from_millis(200));
  common::metal_parity(&mut h, 0.005);
  h.click_id(id(SIZE, 0));
  h.wait(Duration::from_millis(200));
  common::metal_parity(&mut h, 0.005);
  h.key(NamedKey::Escape, Modifiers::empty());
  h.move_to(mid(rect(&h, RESET, 0)).0, mid(rect(&h, RESET, 0)).1);
  h.wait(Duration::from_millis(400));
  assert!(h.rect(id(RESET, 0).key("tip")).is_some());
  common::metal_parity(&mut h, 0.005);
}
