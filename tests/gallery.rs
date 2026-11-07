// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Harness tests for `examples/gallery.rs`: every block has a page that builds, named by the
//! block; the sidebar turns the pages; a block's disabled variant reads disabled and its sizes
//! differ; a block alone is as high as its variants; in each appearance the widgets work and draw
//! in its theme (a check box checks in its accent, a switch slides, the slider drags, a picker
//! chooses by letters, a segment picks, a tooltip shows, a recorder records and its ⊗ clears,
//! the alert asks in its theme, Escape closes it, its action counts), the search field draws in
//! its theme and clears from its ⊗; assistive technology reads each widget by its role, the ⊗
//! marks beside their controls; and `docs/api.md` and its pictures are the gallery's.

#[path = "../examples/gallery.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod gallery;

mod common;

use std::time::Duration;

use common::{diff, mid, px, reads};
use gallery::*;
use nimbo::ui::{Action, Role};
use nimbo::{Appearance, Event, Harness, Id, Modifiers, NamedKey, Rect, Size, Theme, alpha, mix};

type H = Harness<Gallery>;

fn gallery(page: usize, only: bool) -> H {
  let mut h = Harness::new(Gallery::on(page, only), Size::new(WIDTH + 200.0, 600.0), 1.0);
  h.frame();
  h
}

fn page(name: &str) -> usize {
  BLOCKS.iter().position(|b| b.name == name).unwrap_or_else(|| panic!("no {name}"))
}

/// The first node assistive technology reads as `role` named `name`, in the enabled line: its
/// id and rect.
fn node(h: &H, role: Role, name: &str) -> (Id, Rect) {
  let t = h.a11y_tree();
  let n = t.nodes.iter().find(|n| n.props.role == role && t.str(n.props.name) == name);
  let n = n.unwrap_or_else(|| panic!("no {role:?} {name:?} in\n{}", h.a11y()));
  (n.id, n.rect)
}

fn find(h: &H, role: Role, name: &str) -> Id {
  node(h, role, name).0
}

/// The gallery on the page of block `name` in `look`.
fn on(name: &str, look: Appearance) -> H {
  let mut h = gallery(page(name), false);
  h.event(Event::Appearance(look));
  h.frame();
  h
}

const LOOKS: [(Appearance, Theme); 2] =
  [(Appearance::LIGHT, Theme::PLATES_LIGHT), (Appearance::DARK, Theme::PLATES_DARK)];

#[test]
fn every_block_has_a_page_named_by_it() {
  for (i, b) in BLOCKS.iter().enumerate() {
    let h = gallery(i, false);
    let title = format!("static text {:?}", b.name);
    assert!(h.a11y().lines().any(|l| l.trim() == title), "{}: {}", b.name, h.a11y());
    assert!(GROUPS.contains(&b.group), "{}", b.name);
  }
}

#[test]
fn the_sidebar_turns_the_pages() {
  let mut h = gallery(0, false);
  let button = page("Button");
  h.click_id(Id::of(SIDEBAR).key(("row", button as u64)));
  h.frame();
  assert_eq!(h.app().page, button);
  let read = h.a11y();
  assert!(
    read.contains("static text \"Button\"") && read.contains("button \"Prominent\""),
    "{read}"
  );
}

/// The switch reads on in its enabled line, disabled in its disabled one; a large one is wider;
/// a small button is lower than the regular one, a large one higher.
#[test]
fn variants_are_disabled_and_sized() {
  let h = gallery(page("Switch"), false);
  let tree = h.a11y();
  let switches: Vec<&str> =
    tree.lines().map(str::trim).filter(|l| l.starts_with("switch")).collect();
  assert_eq!(switches.len(), 4, "enabled, disabled, small, large: {tree}");
  assert!(!switches[0].contains("disabled") && switches[1].contains("disabled"), "{switches:?}");
  let t = h.a11y_tree();
  let widths: Vec<f32> =
    t.nodes.iter().filter(|n| n.props.role == Role::Switch).map(|n| n.rect.w).collect();
  assert!(widths[3] > widths[0], "large wider: {widths:?}");
  let h = gallery(page("Button"), false);
  let t = h.a11y_tree();
  let hs: Vec<f32> = (t.nodes.iter())
    .filter(|n| n.props.role == Role::Button && t.str(n.props.name) == "Bordered")
    .map(|n| n.rect.h)
    .collect();
  assert!(hs.len() == 4 && hs[2] < hs[0] && hs[3] > hs[0], "small, large: {hs:?}");
}

/// Alone, a block's window is as wide as a page and as high as its variants, whatever it was.
#[test]
fn a_block_alone_is_as_high_as_its_variants() {
  for name in ["Button", "List sections", "Text"] {
    let mut h = gallery(page(name), true);
    h.frame();
    let (size, block) = (h.size(), h.rect(Id::of(BLOCK)).unwrap());
    assert_eq!((size.w, size.h), (WIDTH, block.h.ceil()), "{name}");
  }
}

/// In each appearance: a check box checked in its theme's accent, a switch that slides, the
/// slider dragged, a picker choosing by letters, a segment picked, a tooltip after a rest, a
/// recorder recording a chord and its ⊗ clearing it, the alert in its theme.
#[test]
fn every_widget_in_both_themes() {
  for (look, t) in LOOKS {
    let mut h = on("Check box", look);
    let (check, r) = node(&h, Role::CheckBox, "Wrap");
    h.click_id(check);
    h.leave();
    h.wait(Duration::from_millis(300));
    assert!(h.app().wrap);
    assert_eq!(h.render().pixel(r.x + 8.0, r.y + r.h / 2.0 - 6.0), px(t.accent), "{look:?}");

    let mut h = on("Switch", look);
    let (switch, r) = node(&h, Role::Switch, "Live");
    let at = |h: &mut H| h.render().pixel(r.x + 6.0, r.y + r.h / 2.0);
    assert_eq!(at(&mut h), px(t.accent), "on, in the accent");
    h.click_id(switch);
    h.leave();
    h.wait(Duration::from_millis(200));
    assert!(!h.app().live && at(&mut h) != px(t.accent), "{look:?}");

    let mut h = on("Slider", look);
    let s = node(&h, Role::Slider, "Zoom").1;
    h.press(s.x + 2.0, s.y + s.h / 2.0);
    h.frame();
    h.move_to(s.x + s.w / 2.0, s.y + s.h / 2.0);
    h.frame();
    h.release(s.x + s.w / 2.0, s.y + s.h / 2.0);
    assert!((h.app().zoom - 100.0).abs() < 1.0, "{}", h.app().zoom);

    let mut h = on("Picker", look);
    let size = find(&h, Role::PopUpButton, "Size");
    h.click_id(size);
    h.frame();
    h.frame();
    assert!(h.rect(size.key("menu")).is_some(), "open");
    h.key('l', Modifiers::empty());
    h.key(NamedKey::Enter, Modifiers::empty());
    h.frame();
    assert_eq!(h.app().size, gallery::Size::Large);

    let mut h = on("Segmented picker", look);
    h.click_id(find(&h, Role::RadioButton, "Columns 3"));
    assert_eq!(h.app().view, 2);

    let mut h = on("Tooltip", look);
    let (save, r) = node(&h, Role::Button, "Save");
    let (x, y) = mid(r);
    h.move_to(x, y);
    h.wait(Duration::from_millis(400));
    assert!(h.rect(save.key("tip")).is_some(), "the tip after a rest");

    let mut h = on("Shortcut recorder", look);
    // The recorder: a button with no name, its shortcut its value.
    let t2 = h.a11y_tree();
    let rec =
      t2.nodes.iter().find(|n| n.props.role == Role::Button && t2.str(n.props.name).is_empty());
    h.click_id(rec.expect("a recorder").id);
    h.key(NamedKey::F5, Modifiers::ALT);
    assert_eq!(h.app().hotkey.map(|k| k.to_string()).as_deref(), Some("option+f5"));
    h.frame();
    h.click_id(find(&h, Role::Button, "Clear"));
    assert_eq!(h.app().hotkey, None, "the ⊗ clears it");

    let mut h = on("Alert", look);
    h.click_id(find(&h, Role::Button, "Delete…"));
    h.wait(Duration::from_millis(200));
    let dialog = Id::of(DIALOG);
    let card = h.rect(dialog.key("card")).expect("the dialog");
    let fill = h.render().pixel(card.x + card.w / 2.0, card.y + 8.0);
    assert_eq!(fill, px(t.dialog), "{look:?}");
    h.key(NamedKey::Escape, Modifiers::empty());
    h.frame();
    assert!(!h.app().ask && h.rect(dialog.key("card")).is_none());
    h.click_id(find(&h, Role::Button, "Delete…"));
    h.frame();
    h.click_id(dialog.key("action"));
    assert_eq!((h.app().deleted, h.app().ask), (1, false));
  }
}

/// The search field in each theme's look over its ground; cleared from its ⊗, typed again, the
/// text kept from one appearance to the other.
#[test]
fn the_search_field_in_both_themes() {
  let mut h = on("Search field", Appearance::LIGHT);
  for (look, t) in LOOKS {
    h.event(Event::Appearance(look));
    h.frame();
    let (field, r) = node(&h, Role::TextField, "Search");
    let fill = t.search().frame.bg;
    let want = px(mix(t.bg, alpha(fill, 1.0), fill[3]));
    let got = h.render().pixel(r.x + r.w / 2.0, r.y + 4.0);
    assert!(diff(got, want) <= 1, "{look:?}: {got:?}, not {want:?}");
    if look == Appearance::LIGHT {
      h.click_id(field.key("clear"));
      h.frame();
      assert_eq!(h.app().query, "");
      assert!(h.rect(field.key("clear")).is_none());
      h.type_text("icons");
      h.frame();
    }
  }

  assert_eq!(h.app().query, "icons");
  let field = find(&h, Role::TextField, "Search");
  assert!(h.rect(field.key("clear")).is_some(), "its ⊗ back in the dark");
}

/// Each kind of widget is what it is to assistive technology: its role, name, value and states.
#[test]
fn every_widget_has_its_role() {
  let pages: [(&str, &[&str]); 10] = [
    ("Button", &["button \"Bordered\"", "button \"Prominent\" disabled"]),
    (
      "Help marks",
      &[
        "button \"Info\" desc \"An ⓘ explains what one line can't\"",
        "button \"Warning\" desc \"A warning tells what is wrong\"",
      ],
    ),
    ("Icon button", &["button \"Reset\""]),
    ("Check box", &["check box \"Wrap\" off", "check box \"Wrap\" off disabled"]),
    ("Switch", &["switch \"Live\" on"]),
    ("Slider", &["slider \"Zoom\" 60 in 0..200"]),
    (
      "Picker",
      &[
        "pop up button \"Size\" value \"Medium\"",
        "pop up button \"Size\" value \"Medium\" disabled",
      ],
    ),
    (
      "Segmented picker",
      &["radio group", "radio button \"List 128\" on selected", "radio button \"Grid 12\" off"],
    ),
    ("Progress", &["progress indicator 0.42 in 0..1"]),
    ("Link", &["link \"Learn more…\""]),
  ];
  for (name, lines) in pages {
    let h = gallery(page(name), false);
    reads(&h.a11y(), lines);
  }

  let h = gallery(page("Badge"), false);
  reads(&h.a11y(), &["static text \"New\""]);
  let h = gallery(page("Search field"), false);
  reads(&h.a11y(), &["text field \"Search\" value \"Switches\"", "button \"Clear\""]);
}

/// The ⊗ of a recorder and of a search field are beside them, not inside the leaves, where a
/// screen reader wouldn't reach them.
#[test]
fn clear_marks_are_beside_their_controls() {
  for (name, first) in
    [("Shortcut recorder", "button value "), ("Search field", "text field \"Search\" value")]
  {
    let h = gallery(page(name), false);
    let dump = h.a11y();
    // The recorder's value is the platform's label of its shortcut (⌃⌥Space, Ctrl+Alt+Space).
    let lines: Vec<&str> = dump.lines().collect();
    let at =
      lines.iter().position(|l| l.trim_start().starts_with(first) && !l.contains("disabled"));
    let at = at.unwrap_or_else(|| panic!("no {first} in\n{dump}"));
    let next = lines[at + 1..].iter().find(|l| l.trim_start().starts_with("button \"Clear\""));
    let depth = |l: &str| l.len() - l.trim_start().len();
    assert_eq!(next.map(|l| depth(l)), Some(depth(lines[at])), "{name}: beside it\n{dump}");
  }
}

#[test]
fn the_alert_asks_and_counts_its_action() {
  let mut h = gallery(page("Alert"), false);
  let ask = find(&h, Role::Button, "Delete…");
  assert!(h.act(ask, Action::Press));
  h.frame();
  h.frame();
  assert!(h.app().ask);
  assert!(h.act(Id::of(DIALOG).key("action"), Action::Press));
  h.frame();
  assert_eq!((h.app().ask, h.app().deleted), (false, 1));
}

/// `docs/api.md` is what the gallery's table makes, every block's picture beside it; on macOS,
/// where `make api-shots` draws them, each picture is what the block draws now.
#[test]
fn the_api_doc_is_the_gallerys() {
  let root = env!("CARGO_MANIFEST_DIR");
  let doc = std::fs::read_to_string(format!("{root}/docs/api.md")).unwrap();
  assert!(doc == api_md(), "docs/api.md is stale: make api-shots");
  let fresh = std::env::temp_dir().join(format!("nimbo-api-{}", std::process::id()));
  std::fs::create_dir_all(&fresh).unwrap();
  for (i, b) in BLOCKS.iter().enumerate() {
    let png = format!("{root}/docs/api/{}.png", b.slug());
    let kept = std::fs::read(&png).unwrap_or_else(|_| panic!("no {png}: make api-shots"));
    if !cfg!(target_os = "macos") {
      continue;
    }

    // As `gallery <slug> --shot` draws it.
    let mut h = Harness::new(Gallery::on(i, true), Size::new(WIDTH, 300.0), 2.0);
    h.event(Event::Appearance(Appearance::LIGHT));
    h.frame();
    let now = fresh.join(format!("{}.png", b.slug()));
    h.shot(&now).unwrap();
    assert!(std::fs::read(&now).unwrap() == kept, "{png} is stale: make api-shots");
  }

  _ = std::fs::remove_dir_all(&fresh);
}
