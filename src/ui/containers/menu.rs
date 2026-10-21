// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Menus: a pop-up menu of items over the window, opened by a control (its trigger), and a
//! button that opens one ([Ui::menu]).
//!
//! ```ignore
//! ui.id("view").menu("View", |ui| {
//!   if ui.menu_item("Zoom In").clicked { self.zoom += 1 }
//!   ui.disabled(self.zoom == 0).menu_item("Actual Size");
//! });
//! ```
//!
//! The behaviour is [Ui::menu_popup] (open state, placement, keys, type-select) and
//! [Ui::menu_item_behavior] (an item's place, highlight and run); [Ui::menu_item] draws one
//! from the theme. Keys come as the platform's commands and text: Up and Down, Enter or Space,
//! Escape, letters typed ([TypeSelect]).

use std::time::Duration;

use crate::keys::{Command, Move};
use crate::scene::{Point, Rect, alpha};
use crate::ui::containers::scroll::{Bars, MIN_THUMB};
use crate::ui::{Align, Behavior, Button, Float, Id, Response, Style, Ui, grow, pct, px};

/// Paint order of menus among overlays.
const MENU_Z: i32 = 1000;
/// The least distance of a menu from the window's edges.
const EDGE: f32 = 6.0;
/// How long letters typed one after another keep adding to a type-select prefix, as in
/// AppKit's menus.
pub const TYPE_SELECT: Duration = Duration::from_millis(1000);

/// Type-to-select in a list of labelled items (a menu, a pop-up button): letters typed within
/// [TYPE_SELECT] of each other make a prefix, and the first item starting with it is chosen.
#[derive(Default)]
pub struct TypeSelect {
  prefix: String,
  at: Option<Duration>,
}

impl TypeSelect {
  /// Adds `c`, typed at `now`; a pause longer than [TYPE_SELECT] starts a new prefix.
  pub fn push(&mut self, c: char, now: Duration) -> &mut Self {
    if self.at.is_none_or(|t| now.saturating_sub(t) > TYPE_SELECT) {
      self.prefix.clear();
    }

    self.at = Some(now);
    self.prefix.extend(c.to_lowercase());
    self
  }

  /// [TypeSelect::push] for each letter of `s`, text typed: no spaces (Space opens or runs)
  /// and no control characters. Whether it added any.
  pub fn push_str(&mut self, s: &str, now: Duration) -> bool {
    let mut any = false;
    for c in s.chars().filter(|c| !c.is_whitespace() && !c.is_control()) {
      self.push(c, now);
      any = true;
    }

    any
  }

  /// The prefix typed so far, lowercase.
  pub fn prefix(&self) -> &str {
    &self.prefix
  }

  /// The first of `n` items whose label starts with the prefix, ignoring case and leading
  /// spaces; `label(i)` is `None` for one that can't be chosen (disabled).
  pub fn find<'l>(&self, n: usize, label: impl Fn(usize) -> Option<&'l str>) -> Option<usize> {
    let starts = |s: &str| s.trim_start().to_lowercase().starts_with(&self.prefix);
    (!self.prefix.is_empty()).then(|| (0..n).find(|&i| label(i).is_some_and(starts)))?
  }
}

/// A menu between frames: open, the highlighted item, and the items (id, enabled) as the
/// last build made them.
#[derive(Default)]
struct Open {
  open: bool,
  cur: Option<usize>,
  items: Vec<(Id, bool)>,
  /// The item under the pointer last frame: moving onto another one highlights it.
  over: Option<usize>,
  /// Focus before it opened.
  prev: Option<Id>,
  /// The highlighted item is to scroll into view: it opened, or keys moved it.
  reveal: bool,
  /// The item it opened on, which a menu over its trigger keeps over it.
  start: Option<usize>,
  /// Keys moved the highlight: the item under a resting pointer doesn't take it back until
  /// the pointer moves or the wheel scrolls the list; and the pointer and the list's offset
  /// as last seen.
  keyed: bool,
  mouse: Option<Point>,
  offset: f32,
  /// Where a menu over its trigger went when first placed: it stays there.
  at: Option<Point>,
  /// The items' labels, a line each, for type-select, and the letters typed.
  labels: String,
  typed: TypeSelect,
}

/// A menu being built ([Ui::menu_popup]), in the [Ui::slot]: its items list themselves with
/// their labels, the one at `cur` is highlighted, the one at `run` runs (Enter), and `ran`
/// tells the menu to close.
#[derive(Default)]
struct MenuCx {
  items: Vec<(Id, bool)>,
  labels: String,
  cur: Option<usize>,
  run: Option<usize>,
  ran: bool,
}

impl Ui<'_> {
  /// A button that opens a menu of `items` ([Ui::menu_item]) under it: [Ui::menu_popup] in
  /// the theme's popup.
  pub fn menu<R>(&mut self, label: &str, items: impl FnOnce(&mut Self) -> R) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let r = Button::new(label).show(self.with_id(id));
    self.menu_popup(id, r.clicked, t.popup().min_w(160.0), items);
    r
  }

  /// Whether the menu of the control `id` ([Ui::menu_popup]) is open now: as the input since
  /// the last frame left it, not as that frame laid it out, so a trigger drawn before its menu
  /// shows it closed in the very frame it closes.
  pub fn menu_open(&self, id: Id) -> bool {
    self.st.store.peek::<Open>(id).is_some_and(|o| o.open)
  }

  /// The menu of the control `id` (its trigger, built before), opened and closed by `toggle`
  /// (the trigger's click): while open, `items` ([Ui::menu_item], [Ui::menu_item_behavior])
  /// in a `style` box under it, as an overlay; returns whether it is open. A click on an item
  /// or anywhere else closes it. The menu is `id.key("menu")`. Open, it has focus: Up and Down
  /// move the highlight over the enabled items, letters typed move it to the first one their
  /// prefix starts ([TypeSelect]), Enter or Space runs the highlighted one, Escape closes it;
  /// closed, focus goes back where it was.
  ///
  /// It stays in the window: under the trigger, or above it when it doesn't fit there and
  /// there is more room above; as high as the room at most, its items then scroll
  /// (`id.key("menu").key("list")`, a [Ui::scroll_area]), keeping the highlighted one in view.
  /// Its first frame lays it out out of sight, to place it. Opening it stops what was
  /// scrolling under it; wheel events around it scroll nothing.
  pub fn menu_popup<R>(
    &mut self,
    id: Id,
    toggle: bool,
    style: Style,
    items: impl FnOnce(&mut Self) -> R,
  ) -> bool {
    self.menu_popup_placed(id, toggle, style, None, false, items)
  }

  /// [Ui::menu_popup] opening over the trigger, as macOS opens a pop-up button's menu: item
  /// `start` (else the first) highlighted and over the trigger, the menu's left edge 12 left of
  /// the trigger's, moved into the window when it doesn't fit.
  pub fn menu_popup_over<R>(
    &mut self,
    id: Id,
    toggle: bool,
    style: Style,
    start: Option<usize>,
    items: impl FnOnce(&mut Self) -> R,
  ) -> bool {
    self.menu_popup_placed(id, toggle, style, start, true, items)
  }

  fn menu_popup_placed<R>(
    &mut self,
    id: Id,
    toggle: bool,
    style: Style,
    start: Option<usize>,
    over: bool,
    items: impl FnOnce(&mut Self) -> R,
  ) -> bool {
    let (menu, list) = (id.key("menu"), id.key("menu").key("list"));
    let mut st = std::mem::take(self.state::<Open>(id));
    let mut close = false;
    if toggle {
      match st.open {
        true => close = true,
        false => {
          let prev = self.focus_id();
          st = Open { open: true, cur: start, prev, reveal: true, start, ..Open::default() };
          // What scrolls under it stops: a trackpad's gesture, a wheel's momentum.
          self.st.halt_scrolling();
        }
      }
    }

    let mut run = None;
    if st.open && !close {
      (close, run) = self.menu_keys(menu, &mut st);
    }

    if !st.open || close {
      if st.open && self.focus_id().is_none_or(|f| f == menu) {
        self.focus(st.prev);
      }

      *self.state::<Open>(id) = Open::default();
      return false;
    }

    // Laid out once, unseen: placed from then on.
    let placed = self.rect(menu).is_some();
    // The highlighted item into view, once the list was laid out; a menu over its trigger
    // scrolls as it is placed, below.
    if st.reveal
      && !(over && st.at.is_none())
      && let Some(sc) = self.scroll(list)
    {
      let item = st.cur.and_then(|i| st.items.get(i)).and_then(|&(i, _)| self.rect(i));
      if let Some(r) = item {
        let y = r.y - sc.viewport.y + sc.offset.y;
        self.reveal(list, Rect::new(0.0, y, r.w, r.h));
      }

      st.reveal = false;
    }

    // Under the menu, the whole window takes the click that closes it.
    let away = id.key("away");
    let cover = Style::new().w(pct(100.0)).h(pct(100.0));
    self.with_id(away).float(Float::window().z(MENU_Z)).el(cover);
    let (x, y, ay, room) = self.menu_place(id, style, over, &mut st);
    if over && placed {
      st.at = Some(Point::new(x, y));
    }

    (st.mouse, st.offset) = (self.mouse(), self.scroll(list).map_or(0.0, |s| s.offset.y));
    let first = self.st.tree.nodes.len();
    let pop = style.hit().focusable_quiet().gap(0.0);
    let fl = match placed {
      true => Float::window().anchor(0.0, ay).offset(x, y).z(MENU_Z + 1),
      // Out of sight; seen from the next frame, placed and scrolled, its scrollbar showing when
      // it scrolls.
      false => Float::window().anchor(0.0, 1.0).offset(x, -100.0).z(MENU_Z + 1),
    };

    if !placed {
      self.st.animating = true;
      self.state::<Bars>(list).shown = Some(self.now());
    }

    self.next_id = Some((menu, false));
    self.autofocus();
    let outer = self.set_slot(Some(MenuCx { cur: st.cur, run, ..MenuCx::default() }));
    let frame = style.pad[0] + style.pad[2] + style.border[0] + style.border[2];
    let area = Style::new().w(grow()).max_h((room - frame).max(MIN_THUMB)).gap(style.gap);
    self.float(fl).col(pop, |ui| ui.with_id(list).scroll_area(area, |ui| items(ui)));
    let cx = self.set_slot(outer).unwrap_or_default();
    let nodes = &self.st.tree.nodes[first + 1..];
    let clicked =
      self.st.input.click.is_some_and(|c| c.id == away || nodes.iter().any(|n| n.id == c.id));
    (st.items, st.labels) = (cx.items, cx.labels);
    if clicked || cx.ran {
      if self.focus_id().is_none_or(|f| f == menu) {
        self.focus(st.prev);
      }

      st = Open::default();
    }

    *self.state::<Open>(id) = st;
    true
  }

  /// An open menu's keys and pointer: the pointer moving onto an item highlights it, Up and
  /// Down step over the enabled items, letters typed go to the first one they start; whether
  /// to close (Escape), and the item to run (Enter, Space).
  fn menu_keys(&mut self, menu: Id, st: &mut Open) -> (bool, Option<usize>) {
    let offset = self.scroll(menu.key("list")).map_or(0.0, |s| s.offset.y);
    if self.mouse() != st.mouse || offset != st.offset {
      st.keyed = false;
    }

    let over = st.items.iter().position(|&(i, on)| on && self.hovered(i));
    if over != st.over && !st.keyed {
      st.cur = over;
    }

    st.over = over;
    let step = |cur: Option<usize>, d: isize| {
      let n = st.items.len() as isize;
      let from = cur.map_or(if d > 0 { -1 } else { n }, |c| c as isize);
      (1..=n).map(|k| (from + d * k).rem_euclid(n.max(1)) as usize).find(|&i| st.items[i].1)
    };
    let mut to = None;
    match self.command(menu) {
      Some(Command::Cancel) => return (true, None),
      Some(Command::Move(Move::Down, _)) => to = step(st.cur, 1),
      Some(Command::Move(Move::Up, _)) => to = step(st.cur, -1),
      Some(Command::InsertNewline) => return (false, st.cur),
      Some(_) => self.pass(menu),
      None => {}
    }

    let now = self.now();
    if let Some(s) = self.typed(menu) {
      if s == " " {
        return (false, st.cur);
      }

      if st.typed.push_str(s, now) {
        let (items, labels) = (&st.items, &st.labels);
        let label = |i: usize| items[i].1.then(|| labels.lines().nth(i)).flatten();
        to = st.typed.find(items.len(), label);
      }
    }

    if to.is_some() {
      (st.cur, st.reveal, st.keyed) = (to, true, true);
    }

    (false, None)
  }

  /// Where an open menu goes: its left and top (or bottom, by the anchor `ay`), and the room
  /// for its height. Under the trigger, or above it when it doesn't fit below and there is
  /// more room above; from its left edge, or its right edge near the window's. Over the
  /// trigger (`over`), the item it opened on over it, by where the last frame laid it out, the
  /// list scrolled to bring it the rest of the way.
  fn menu_place(
    &mut self,
    id: Id,
    style: Style,
    over: bool,
    st: &mut Open,
  ) -> (f32, f32, f32, f32) {
    let (menu, list) = (id.key("menu"), id.key("menu").key("list"));
    let size = self.size();
    let b = self.rect(id).unwrap_or_default();
    let menu_w = self.rect(menu).map_or(160.0, |m| m.w);
    let frame = style.pad[0] + style.pad[2] + style.border[0] + style.border[2];
    let natural = self.scroll(list).map_or(0.0, |sc| sc.content.h + frame);
    let below = b.y + b.h + 3.0;
    let (down, up) = ((size.h - EDGE - below).max(0.0), (b.y - 3.0 - EDGE).max(0.0));
    let flip = natural > down && up > down;
    let (mut y, mut ay, mut room) = if flip { (b.y - 3.0, 1.0, up) } else { (below, 0.0, down) };
    let mut x = if b.x + menu_w > size.w - EDGE { b.x + b.w - menu_w } else { b.x };
    if over && let Some(p) = st.at {
      (x, y, ay, room) = (p.x, p.y, 0.0, size.h - 2.0 * EDGE);
    } else if over {
      room = size.h - 2.0 * EDGE;
      let h = natural.min(room);
      let start = st.start.or(Some(0)).and_then(|i| st.items.get(i));
      let item = start.and_then(|&(i, _)| self.rect(i));
      let top = style.pad[0];
      (y, ay) = ((b.y - top).min(size.h - EDGE - h).max(EDGE), 0.0);
      if let (Some(r), Some(sc)) = (item, self.scroll(list)) {
        let at = r.y - sc.viewport.y + sc.offset.y;
        let target = b.y + (b.h - r.h) / 2.0;
        y = (target - top - at).min(size.h - EDGE - h).max(EDGE);
        let max = (natural - h).max(0.0);
        self.scroll_to(list, Point::new(0.0, (y + top + at - target).clamp(0.0, max)));
        st.reveal = false;
      }

      x = b.x - 12.0;
    }

    (x.min(size.w - EDGE - menu_w).max(EDGE.min(b.x)), y, ay, room)
  }

  /// An item of a menu: highlighted in the accent under the pointer or by the menu's keys,
  /// dimmed when disabled.
  pub fn menu_item(&mut self, label: &str) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let b = self.menu_item_behavior(id, label);
    let (bg, text) = match b.on {
      true => (t.accent, t.label().color(t.on_accent)),
      false => (alpha(t.accent, 0.0), t.label()),
    };

    let s = Style::new()
      .w(grow())
      .h(px(t.sizes.item_h))
      .pad_xy(10.0, 0.0)
      .align(Align::Start, Align::Center)
      .radius((t.sizes.menu_radius - 5.0).max(t.radius))
      .bg(bg)
      .opacity(if b.disabled { t.disabled } else { 1.0 });
    let mut r = self.with_id(id).row(b.pointer(s), |ui| ui.text(label, text));
    r.clicked = b.clicked;
    r
  }

  /// An item of the menu being built ([Ui::menu_popup]) as `id`, `label` its text: `on` while
  /// highlighted (under the pointer, by the menu's keys, or by letters typed that start its
  /// label), `clicked` when clicked or run by Enter. Outside a menu, `on` while hovered.
  pub fn menu_item_behavior(&mut self, id: Id, label: &str) -> Behavior {
    let off = self.take_disabled();
    // In a menu: its place among the items, and whether its keys highlight or run it.
    let (cur, run, inside) = match self.slot::<MenuCx>() {
      Some(m) => {
        let i = m.items.len();
        m.items.push((id, !off));
        m.labels.push_str(label.lines().next().unwrap_or(""));
        m.labels.push('\n');
        let run = m.run == Some(i) && !off;
        m.ran |= run;
        (m.cur == Some(i), run, true)
      }
      None => (false, false, false),
    };

    if off {
      return Behavior { id, disabled: true, ..Behavior::default() };
    }

    let on = cur || !inside && self.hovered(id);
    Behavior { on, clicked: self.clicked(id) || run, ..self.pointer_state(id) }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::{App, Event, Mods, Phase};
  use crate::harness::Harness;
  use crate::keys::Key;
  use crate::scene::Size;

  /// A menu inside a scroll area over tall content; its items count their runs.
  #[derive(Default)]
  struct Menus {
    runs: [usize; 3],
  }

  impl App for Menus {
    fn view(&mut self, ui: &mut Ui) {
      ui.id("area").scroll_area(Style::new().grow(), |ui| {
        ui.el(Style::new().w(px(10.0)).h(px(150.0)));
        ui.id("menu").menu("Menu", |ui| {
          for (i, name) in ["one", "two", "three"].into_iter().enumerate() {
            if ui.disabled(i == 1).id(name).menu_item(name).clicked {
              self.runs[i] += 1;
            }
          }
        });
        ui.el(Style::new().w(px(10.0)).h(px(2000.0)));
      });
    }
  }

  #[test]
  fn the_wheel_scrolls_nothing_under_an_open_menu() {
    let mut h = Harness::new(Menus::default(), Size::new(400.0, 300.0), 1.0);
    h.frame();
    let (menu, area) = (Id::of("menu"), Id::of("area"));
    let wheel = |h: &mut Harness<Menus>, x: f32, y: f32| {
      let (pos, delta, mods) = (Point::new(x, y), Point::new(0.0, -100.0), Mods::NONE);
      h.event(Event::Scroll {
        pos,
        delta,
        precise: false,
        phase: Phase::None,
        momentum: Phase::None,
        mods,
      });
      h.wait(Duration::from_millis(500));
    };
    // A wheel's momentum still running as it opens stops.
    wheel(&mut h, 350.0, 250.0);
    let at = h.rt.ui.scroll(area).unwrap().offset.y;
    assert!(at > 0.0);
    h.click_id(menu);
    h.frame();
    h.frame();
    assert_eq!(h.focused(), Some(menu.key("menu")));
    // Over the open menu, or the cover around it: nothing under them scrolls.
    let r = h.rect(Id::of("one")).unwrap();
    wheel(&mut h, r.x + 5.0, r.y + 5.0);
    wheel(&mut h, 350.0, 250.0);
    assert_eq!(h.rt.ui.scroll(area).unwrap().offset.y, at);
    // The pointer highlights an item; Down skips the disabled one; Enter runs it.
    h.move_to(r.x + 5.0, r.y + 5.0);
    h.frame();
    h.key(Key::Down, Mods::NONE);
    h.key(Key::Enter, Mods::NONE);
    h.frame();
    assert_eq!(h.app().runs, [0, 0, 1]);
    assert!(h.rect(menu.key("menu")).is_none() && h.focused().is_none());
  }

  #[test]
  fn type_select_prefixes() {
    let mut ts = TypeSelect::default();
    let ms = Duration::from_millis;
    let items = ["Never", "  After 1 minute", "After 5 minutes", "Forever"];
    let find = |ts: &TypeSelect, off: usize| ts.find(4, |i| (i != off).then_some(items[i]));
    assert_eq!(find(ts.push('a', ms(0)), 9), Some(1), "leading spaces don't count");
    assert_eq!(find(ts.push('f', ms(500)), 9), Some(1), "af: After 1 minute");
    assert_eq!(find(&ts, 1), Some(2), "a disabled item is skipped");
    assert_eq!(ts.push('t', ms(1400)).prefix(), "aft", "within a second of the last letter");
    assert_eq!(ts.push('f', ms(2500)).prefix(), "f", "a pause starts again");
    assert_eq!(find(&ts, 9), Some(3));
    assert!(!ts.push_str(" \t", ms(2600)) && ts.push_str("O", ms(2700)));
    assert_eq!(ts.prefix(), "fo");
  }
}
