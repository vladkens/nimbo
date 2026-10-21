// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Harness scenario for `examples/lists.rs`: the 100k-row list builds only the rows in view and
//! scrolls by the wheel and the keys, selects by click, Shift-click, Cmd-click and the arrows,
//! opens a row by Enter; the menu opens, moves over its enabled items and runs one, closes by
//! Escape or a click outside; the pop-up button chooses by letters typed, closed and open; the
//! tabs switch and keep the hidden page's scroll; the split's divider drags.

#[path = "../examples/lists.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod lists;

use std::time::Duration;

use lists::*;
use nimbo::{Event, Harness, Id, Key, Mods, Phase, Point, Rect, Size};

type H = Harness<Lists>;

fn mid(r: Rect) -> (f32, f32) {
  (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn row(k: u64) -> Id {
  Id::of(LIST).key(("row", k))
}

/// A trackpad's scroll of `dy` points at `at`.
fn swipe(h: &mut H, at: (f32, f32), dy: f32) -> bool {
  let (pos, delta) = (Point::new(at.0, at.1), Point::new(0.0, dy));
  let (phase, momentum, mods) = (Phase::Changed, Phase::None, Mods::NONE);
  h.event(Event::Scroll { pos, delta, precise: true, phase, momentum, mods })
}

fn offset(h: &H, id: Id) -> f32 {
  h.rt.ui.scroll(id).unwrap().offset.y
}

fn click_with(h: &mut H, id: Id, mods: Mods) {
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

const CMD: Mods = Mods { cmd: true, ..Mods::NONE };

fn main() {
  let mut h = Harness::new(Lists::new(), Size::new(760.0, 480.0), 2.0);
  h.render();
  let list = Id::of(LIST);

  // Only the rows in view are built, under the whole content's height.
  let built = h.rt.ui.last.nodes.iter().filter(|n| n.parent != 0).count();
  assert!(h.rect(row(0)).is_some() && h.rect(row(30)).is_none());
  let sc = h.rt.ui.scroll(list).unwrap();
  assert_eq!(sc.content.h, ROWS as f32 * ROW_H + 8.0);
  assert!(built < 120, "{built} nodes");

  // The trackpad scrolls it; the rows follow.
  let at = mid(h.rect(list).unwrap());
  assert!(swipe(&mut h, at, -2400.0));
  h.frame();
  assert_eq!(offset(&h, list), 2400.0);
  assert!(h.rect(row(0)).is_none() && h.rect(row(105)).is_some());

  // Click, Shift-click and Cmd-click select; the status shows the count.
  h.click_id(row(101));
  assert_eq!(selected(&h), [101]);
  click_with(&mut h, row(104), Mods::SHIFT);
  assert_eq!(selected(&h), [101, 102, 103, 104]);
  click_with(&mut h, row(102), CMD);
  assert_eq!(selected(&h), [101, 103, 104]);
  assert_eq!(h.focused(), Some(list));

  // Down moves the cursor from the last row clicked; Shift-Down extends; Enter opens.
  h.key(Key::Down, Mods::NONE);
  assert_eq!((selected(&h), h.app().sel.cursor()), (vec![103], Some(103)));
  h.key(Key::Down, Mods::SHIFT);
  h.key(Key::Down, Mods::SHIFT);
  assert_eq!(selected(&h), [103, 104, 105]);
  h.key(Key::Enter, Mods::NONE);
  assert_eq!(h.app().opened, Some(105));

  // End goes to the last row, scrolled into view; Home back to the first.
  h.key(Key::End, Mods::NONE);
  h.frame();
  assert_eq!(selected(&h), [ROWS as u64 - 1]);
  let (v, last) = (h.rt.ui.scroll(list).unwrap().viewport, h.rect(row(ROWS as u64 - 1)).unwrap());
  assert!(last.y >= v.y && last.y + last.h <= v.y + v.h + 0.5, "{last:?} in {v:?}");
  h.key(Key::Up, CMD);
  h.frame();
  assert_eq!((selected(&h), offset(&h, list)), (vec![0], 0.0));

  // Cmd-A selects all 100k.
  h.key(Key::Char('a'), CMD);
  assert_eq!(h.app().sel.len(), ROWS);

  // The menu: opened by a click, focused, Down skips nothing while rows are selected, Enter
  // runs the highlighted item and closes it, focus going back to the list.
  let (menu, popup) = (Id::of(MENU), Id::of(MENU).key("menu"));
  let open = |h: &mut H, id: Id| {
    h.click_id(id);
    h.frame();
    h.frame();
    assert!(h.rect(id.key("menu")).is_some(), "{id:?} open");
  };
  open(&mut h, menu);
  assert_eq!(h.focused(), Some(popup));
  h.key(Key::Down, Mods::NONE);
  h.key(Key::Down, Mods::NONE);
  h.key(Key::Down, Mods::NONE);
  h.key(Key::Enter, Mods::NONE);
  h.frame();
  assert_eq!(h.app().ran, [2]);
  assert!(h.rect(popup).is_none() && h.app().sel.is_empty());
  // Nothing selected: Up from nothing wraps past the disabled "Open" to "Select None".
  open(&mut h, menu);
  h.key(Key::Up, Mods::NONE);
  h.key(Key::Char(' '), Mods::NONE);
  h.frame();
  assert_eq!(h.app().ran, [2, 2]);
  // Escape closes it, a click outside too, running nothing; letters typed highlight the item
  // they start.
  open(&mut h, menu);
  h.key(Key::Escape, Mods::NONE);
  h.frame();
  assert!(h.rect(popup).is_none());
  open(&mut h, menu);
  h.click(700.0, 400.0);
  h.frame();
  assert!(h.rect(popup).is_none() && h.app().ran == [2, 2]);
  open(&mut h, menu);
  h.key(Key::Char('g'), Mods::NONE);
  h.advance(Duration::from_millis(300));
  h.key(Key::Char('o'), Mods::NONE);
  h.key(Key::Char(' '), Mods::NONE);
  h.frame();
  // "go" highlights the first, Space runs it: the first row selected and shown.
  assert_eq!((h.app().ran.as_slice(), selected(&h)), ([2, 2, 0].as_slice(), vec![0]));

  // The pop-up button, focused: letters typed choose an option at once, closed.
  let sort = Id::of(SORT);
  h.rt.ui.set_focus(Some(sort));
  h.key(Key::Char('k'), Mods::NONE);
  assert_eq!(h.app().sort, 2, "k: Kind");
  h.advance(Duration::from_millis(1100));
  h.key(Key::Char('S'), Mods::SHIFT);
  assert_eq!(h.app().sort, 1, "s: Size, whatever the case");
  // Open on Space; "d" highlights Date Modified, Enter
  // picks it and gives focus back.
  h.advance(Duration::from_millis(1100));
  h.key(Key::Char(' '), Mods::NONE);
  h.frame();
  h.frame();
  assert_eq!(h.focused(), Some(sort.key("menu")));
  // Over the button, as near as the window lets it: the button sits by the window's top.
  let (b, m) = (h.rect(sort).unwrap(), h.rect(sort.key("menu")).unwrap());
  assert!(m.x < b.x && m.y < b.y + b.h && m.y >= 6.0, "{m:?} over {b:?}");
  h.key(Key::Char('d'), Mods::NONE);
  h.key(Key::Enter, Mods::NONE);
  h.frame();
  assert_eq!((h.app().sort, h.focused()), (3, Some(sort)));

  // Tabs: a click on Notes shows its page; scrolled, then back and forth, it keeps its offset.
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
  // Tab focuses the strip; the arrows switch.
  h.rt.ui.set_focus(Some(tabs));
  h.key(Key::Right, Mods::NONE);
  h.frame();
  assert_eq!((h.app().tab, offset(&h, notes)), (1, 60.0));

  // The split's divider drags: the list grows by as much.
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

  // Left alone, it goes idle.
  h.leave();
  h.wait(Duration::from_millis(2000));
  assert!(!h.frame(), "idle");

  println!("lists scenario: ok");
}
