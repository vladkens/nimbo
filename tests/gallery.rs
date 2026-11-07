// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Harness tests for `examples/gallery.rs`: every block has a page that builds, named by the
//! block; the sidebar turns the pages; a block's disabled variant reads disabled and its sizes
//! differ; a block alone is as high as its variants; the alert asks and counts its action; and
//! `docs/api.md` is the gallery's table, with a picture for every block.

#[path = "../examples/gallery.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod gallery;

use gallery::*;
use nimbo::ui::{Action, Role};
use nimbo::{Harness, Id, Size};

type H = Harness<Gallery>;

fn gallery(page: usize, only: bool) -> H {
  let mut h = Harness::new(Gallery::on(page, only), Size::new(WIDTH + 200.0, 600.0), 1.0);
  h.frame();
  h
}

fn page(name: &str) -> usize {
  BLOCKS.iter().position(|b| b.name == name).unwrap_or_else(|| panic!("no {name}"))
}

/// The id of the first node assistive technology reads as `role` named `name`.
fn find(h: &H, role: Role, name: &str) -> Id {
  let t = h.a11y_tree();
  let n = t.nodes.iter().find(|n| n.props.role == role && t.str(n.props.name) == name);
  n.unwrap_or_else(|| panic!("no {role:?} {name:?} in\n{}", h.a11y())).id
}

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

/// The switch reads on in its enabled line, disabled in its disabled one; a large one is wider.
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
}

/// Alone, a block's window is as high as its variants.
#[test]
fn a_block_alone_is_as_high_as_its_variants() {
  let h = gallery(page("Button"), true);
  let size = h.size();
  assert_eq!(size.w, WIDTH);
  assert!(size.h > 100.0 && size.h < 300.0, "{size:?}");
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

/// `docs/api.md` is what the gallery's table makes, every block's picture beside it.
#[test]
fn the_api_doc_is_the_gallerys() {
  let root = env!("CARGO_MANIFEST_DIR");
  let doc = std::fs::read_to_string(format!("{root}/docs/api.md")).unwrap();
  assert!(doc == api_md(), "docs/api.md is stale: make api-shots");
  for b in BLOCKS {
    let png = format!("{root}/docs/api/{}.png", b.slug());
    assert!(std::path::Path::new(&png).exists(), "no {png}: make api-shots");
  }
}
