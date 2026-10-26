// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/lists.rs` through the harness: the 100k-row list builds only the rows in
//! view and scrolls by the wheel and the keys, selects by click, Shift-click, Cmd-click and the
//! arrows, opens a row by Enter; the menu opens, moves over its enabled items and runs one,
//! closes by Escape or a click outside; the pop-up button chooses by letters typed, closed and
//! open; the tabs switch and keep the hidden page's scroll; the split's divider drags. Assistive
//! technology reads the rows, the selection and the cursor, the menu's highlight, the tabs, and
//! presses a tab.

#[path = "../examples/lists.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod lists;

mod common;

use std::time::Duration;

use common::{mid, reads};
use lists::*;
use nimbo::ui::Action;
use nimbo::{CMD, Event, Harness, Id, Key, Modifiers, NamedKey, Phase, Point, Size};

type H = Harness<Lists>;

const NONE: Modifiers = Modifiers::empty();

/// The example, its first frame built.
fn lists() -> H {
  let mut h = Harness::new(Lists::new(), Size::new(760.0, 480.0), 2.0);
  h.render();
  h
}

/// The example with its list scrolled down to row 100.
fn scrolled() -> H {
  let mut h = lists();
  let at = mid(h.rect(Id::of(LIST)).unwrap());
  assert!(swipe(&mut h, at, -2400.0));
  h.frame();
  h
}

fn row(k: u64) -> Id {
  Id::of(LIST).key(("row", k))
}

/// A trackpad's scroll of `dy` points at `at`.
fn swipe(h: &mut H, at: (f32, f32), dy: f32) -> bool {
  let (pos, delta) = (Point::new(at.0, at.1), Point::new(0.0, dy));
  let (phase, momentum, mods) = (Phase::Changed, Phase::None, NONE);
  h.event(Event::Scroll { pos, delta, precise: true, phase, momentum, mods })
}

fn offset(h: &H, id: Id) -> f32 {
  h.rt.ui.scroll(id).unwrap().offset.y
}

fn click_with(h: &mut H, id: Id, mods: Modifiers) {
  let (x, y) = mid(h.rect(id).unwrap());
  h.move_to(x, y);
  h.press_with(x, y, 1, mods);
  h.release(x, y);
}

fn selected(h: &H) -> Vec<u64> {
  let mut v: Vec<_> = h.app().sel.keys().collect();
  v.sort();
  v
}

/// Rows 101, 103 and 104 selected by a click, a Shift-click and a Cmd-click.
fn select_some(h: &mut H) {
  h.click_id(row(101));
  assert_eq!(selected(h), [101]);
  click_with(h, row(104), Modifiers::SHIFT);
  assert_eq!(selected(h), [101, 102, 103, 104]);
  click_with(h, row(102), CMD);
  assert_eq!(selected(h), [101, 103, 104]);
}

/// Opens the menu or pop-up button `id` by a click; it has the focus.
fn open(h: &mut H, id: Id) {
  h.click_id(id);
  h.frame();
  h.frame();
  assert!(h.rect(id.key("menu")).is_some(), "{id:?} open");
}

/// Only the rows in view are built, under the whole content's height.
#[test]
fn only_the_rows_in_view_are_built() {
  let h = lists();
  let built = h.rt.ui.last.nodes.iter().filter(|n| n.parent != 0).count();
  assert!(h.rect(row(0)).is_some() && h.rect(row(30)).is_none());
  let sc = h.rt.ui.scroll(Id::of(LIST)).unwrap();
  assert_eq!(sc.content.h, ROWS as f32 * ROW_H + 8.0);
  assert!(built < 120, "{built} nodes");
}

/// The trackpad scrolls it; the rows follow; left alone, it goes idle.
#[test]
fn the_trackpad_scrolls_the_rows() {
  let mut h = scrolled();
  assert_eq!(offset(&h, Id::of(LIST)), 2400.0);
  assert!(h.rect(row(0)).is_none() && h.rect(row(105)).is_some());
  h.leave();
  h.wait(Duration::from_millis(2000));
  assert!(!h.frame(), "idle");
}

#[test]
fn click_shift_click_and_cmd_click_select() {
  let mut h = scrolled();
  select_some(&mut h);
  assert_eq!(h.focused(), Some(Id::of(LIST)));
}

/// Down moves the cursor from the last row clicked; Shift-Down extends; Enter opens.
#[test]
fn arrows_move_and_extend_and_enter_opens() {
  let mut h = scrolled();
  select_some(&mut h);
  h.key(NamedKey::ArrowDown, NONE);
  assert_eq!((selected(&h), h.app().sel.cursor()), (vec![103], Some(103)));
  h.key(NamedKey::ArrowDown, Modifiers::SHIFT);
  h.key(NamedKey::ArrowDown, Modifiers::SHIFT);
  assert_eq!(selected(&h), [103, 104, 105]);
  h.key(NamedKey::Enter, NONE);
  assert_eq!(h.app().opened, Some(105));
}

/// End goes to the last row, scrolled into view; Cmd-Up back to the first; Cmd-A selects all.
#[test]
fn keys_go_to_the_ends_and_select_all() {
  let mut h = scrolled();
  let list = Id::of(LIST);
  h.click_id(row(101));
  h.key(NamedKey::End, NONE);
  h.frame();
  assert_eq!(selected(&h), [ROWS as u64 - 1]);
  let (v, last) = (h.rt.ui.scroll(list).unwrap().viewport, h.rect(row(ROWS as u64 - 1)).unwrap());
  assert!(last.y >= v.y && last.y + last.h <= v.y + v.h + 0.5, "{last:?} in {v:?}");
  h.key(NamedKey::ArrowUp, CMD);
  h.frame();
  assert_eq!((selected(&h), offset(&h, list)), (vec![0], 0.0));
  h.key(Key::Char('a'), CMD);
  assert_eq!(h.app().sel.len(), ROWS);
}

/// The menu: opened by a click, focused, Down skips nothing while rows are selected, Enter runs
/// the highlighted item and closes it.
#[test]
fn the_menu_runs_the_highlighted_item() {
  let mut h = lists();
  let (menu, popup) = (Id::of(MENU), Id::of(MENU).key("menu"));
  h.click_id(row(1));
  h.key(Key::Char('a'), CMD);
  open(&mut h, menu);
  assert_eq!(h.focused(), Some(popup));
  h.key(NamedKey::ArrowDown, NONE);
  h.key(NamedKey::ArrowDown, NONE);
  h.key(NamedKey::ArrowDown, NONE);
  h.key(NamedKey::Enter, NONE);
  h.frame();
  assert_eq!(h.app().ran, [2]);
  assert!(h.rect(popup).is_none() && h.app().sel.is_empty());
}

/// Nothing selected: Up from nothing wraps past the disabled "Open" to "Select None", which
/// Space runs.
#[test]
fn the_menu_skips_disabled_items() {
  let mut h = lists();
  open(&mut h, Id::of(MENU));
  h.key(NamedKey::ArrowUp, NONE);
  h.key(Key::Char(' '), NONE);
  h.frame();
  assert_eq!(h.app().ran, [2]);
}

/// Escape closes the menu, a click outside too, running nothing.
#[test]
fn escape_or_a_click_outside_closes_the_menu() {
  let mut h = lists();
  let (menu, popup) = (Id::of(MENU), Id::of(MENU).key("menu"));
  open(&mut h, menu);
  h.key(NamedKey::Escape, NONE);
  h.frame();
  assert!(h.rect(popup).is_none());
  open(&mut h, menu);
  h.click(700.0, 400.0);
  h.frame();
  assert!(h.rect(popup).is_none() && h.app().ran.is_empty());
}

/// Letters typed highlight the item they start: "go" the first, which Space runs, the first row
/// selected and shown.
#[test]
fn letters_typed_highlight_a_menu_item() {
  let mut h = scrolled();
  open(&mut h, Id::of(MENU));
  h.key(Key::Char('g'), NONE);
  h.advance(Duration::from_millis(300));
  h.key(Key::Char('o'), NONE);
  h.key(Key::Char(' '), NONE);
  h.frame();
  assert_eq!((h.app().ran.as_slice(), selected(&h)), ([0].as_slice(), vec![0]));
  assert_eq!(offset(&h, Id::of(LIST)), 0.0);
}

/// The pop-up button, focused and closed: letters typed choose an option at once.
#[test]
fn the_pop_up_button_chooses_by_letters_typed() {
  let mut h = lists();
  h.rt.ui.set_focus(Some(Id::of(SORT)));
  h.key(Key::Char('k'), NONE);
  assert_eq!(h.app().sort, 2, "k: Kind");
  h.advance(Duration::from_millis(1100));
  h.key(Key::Char('S'), Modifiers::SHIFT);
  assert_eq!(h.app().sort, 1, "s: Size, whatever the case");
}

/// The pop-up button opens on Space over itself, as near as the window lets it; "d" highlights
/// Date Modified, Enter picks it and gives the focus back.
#[test]
fn the_pop_up_button_opens_over_itself() {
  let mut h = lists();
  let sort = Id::of(SORT);
  h.rt.ui.set_focus(Some(sort));
  h.key(Key::Char(' '), NONE);
  h.frame();
  h.frame();
  assert_eq!(h.focused(), Some(sort.key("menu")));
  // The button sits by the window's top.
  let (b, m) = (h.rect(sort).unwrap(), h.rect(sort.key("menu")).unwrap());
  assert!(m.x < b.x && m.y < b.y + b.h && m.y >= 6.0, "{m:?} over {b:?}");
  h.key(Key::Char('d'), NONE);
  h.key(NamedKey::Enter, NONE);
  h.frame();
  assert_eq!((h.app().sort, h.focused()), (3, Some(sort)));
}

/// A click on Notes shows its page; scrolled, then back and forth, it keeps its offset. Focused,
/// the strip switches by the arrows.
#[test]
fn tabs_switch_and_keep_the_hidden_page_scroll() {
  let mut h = lists();
  let tabs = Id::of(TABS);
  h.click_id(tabs.key(("tab", "Notes")));
  h.frame();
  assert_eq!(h.app().tab, 1);
  let notes = Id::of(NOTES);
  let at = mid(h.rect(notes).unwrap());
  swipe(&mut h, at, -60.0);
  h.frame();
  assert_eq!(offset(&h, notes), 60.0);
  h.click_id(tabs.key(("tab", "Details")));
  h.frame();
  assert!(h.rect(notes).is_none());
  h.rt.ui.set_focus(Some(tabs));
  h.key(NamedKey::ArrowRight, NONE);
  h.frame();
  assert_eq!((h.app().tab, offset(&h, notes)), (1, 60.0));
}

/// The split's divider drags: the list grows by as much; left alone after, it goes idle.
#[test]
fn the_split_divider_drags() {
  let mut h = lists();
  let list = Id::of(LIST);
  let handle = Id::of(SPLIT).key("handle");
  let w = h.rect(list).unwrap().w;
  let (x, y) = mid(h.rect(handle).unwrap());
  h.move_to(x, y);
  h.press(x, y);
  h.move_to(x + 80.0, y);
  h.frame();
  h.release(x + 80.0, y);
  h.frame();
  let grown = h.rect(list).unwrap().w;
  assert!((grown - (w + 80.0)).abs() <= 1.0, "{w} → {grown}");
  assert!(h.app().side > 0.45);
  h.leave();
  h.wait(Duration::from_millis(2000));
  assert!(!h.frame(), "idle");
}

/// The list reads its rows by their text, the selected ones selected, the cursor's row as its
/// active one.
#[test]
fn the_list_reads_its_rows_selection_and_cursor() {
  let mut h = lists();
  h.click_id(row(2));
  h.key(NamedKey::ArrowDown, Modifiers::SHIFT);
  h.frame();
  let lines = [
    "list focused active \"Item 000004 123 KB\"",
    "row \"Item 000002 49 KB\"",
    "row \"Item 000003 86 KB\" selected",
    "row \"Item 000004 123 KB\" selected",
    "static text \"Item 000004\"",
  ];
  reads(&h.a11y(), &lines);
}

/// An open menu reads its items, the highlighted one active, its button expanded.
#[test]
fn the_menu_reads_its_items_and_highlight() {
  let mut h = lists();
  open(&mut h, Id::of(MENU));
  h.key(NamedKey::ArrowDown, NONE);
  h.key(NamedKey::ArrowDown, NONE);
  h.frame();
  let lines = [
    "button \"Actions\" expanded",
    "menu focused active \"Go to Last\"",
    "menu item \"Go to First\"",
    "menu item \"Open\" disabled",
  ];
  reads(&h.a11y(), &lines);
}

/// Tabs read as a tab list, the shown one selected; the split's divider as a splitter with its
/// share.
#[test]
fn the_tabs_and_the_divider_read_their_state() {
  let mut h = lists();
  h.click_id(Id::of(TABS).key(("tab", "Notes")));
  h.frame();
  let lines = ["tab list", "tab \"Details\"", "tab \"Notes\" selected", "splitter 0.45 in 0..1"];
  reads(&h.a11y(), &lines);
}

/// Assistive technology presses a tab as a click does.
#[test]
fn an_assistive_press_turns_a_tab() {
  let mut h = lists();
  assert!(h.act(Id::of(TABS).key(("tab", "Notes")), Action::Press));
  assert_eq!(h.app().tab, 1);
  reads(&h.a11y(), &["tab \"Notes\" selected"]);
}
