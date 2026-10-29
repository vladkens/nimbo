// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Menus: a pop-up menu of items over the window, opened by a control (its trigger) or at a
//! point as a context menu ([Ui::menu_popup_at]), a button that opens one ([Ui::menu]), and
//! the context menu of any element from a [Menu] ([Ui::context_menu]): the platform's own in a
//! window, drawn from the model in the harness.
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

use keyboard_types::NamedKey;

use crate::app::Event;
use crate::keys::{Command, Key, Move};
use crate::menu::{Item, Menu};
use crate::scene::{Point, Rect, alpha};
use crate::ui::containers::popup::check as check_mark;
use crate::ui::containers::scroll::{Bars, MIN_THUMB};
use crate::ui::{
  Align, Behavior, Button, Float, Id, Response, Role, State, Style, Ui, UiState, grow, pct, px,
};

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
  /// A context menu's point ([Ui::menu_popup_at]), where it goes in place of under its trigger.
  point: Option<Point>,
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
    let off = self.take_disabled();
    let r = Button::new(label).show(self.disabled(off).with_id(id));
    // Disabled while open, it closes.
    let toggle = r.clicked || off && self.menu_open(id);
    self.menu_popup(id, toggle, t.popup().min_w(160.0), items);
    r
  }

  /// Whether the menu of the control `id` ([Ui::menu_popup]) is open now: as the input since
  /// the last frame left it, not as that frame laid it out, so a trigger drawn before its menu
  /// shows it closed in the very frame it closes. A native context menu ([Ui::context_menu]) is
  /// open until its answer comes.
  pub fn menu_open(&self, id: Id) -> bool {
    self.st.menu_open(id)
  }

  /// The menu of the control `id` (its trigger, built before), opened and closed by `toggle`
  /// (the trigger's click): while open, `items` ([Ui::menu_item], [Ui::menu_item_behavior])
  /// in a `style` box under it, as an overlay; returns whether it is open after this frame's
  /// input. A click on an enabled item or outside the menu closes it. The menu is
  /// `id.key("menu")`. Open, it has focus, kept from Tab: Up and Down move the highlight over
  /// the enabled items, letters typed move it to the first one their prefix starts
  /// ([TypeSelect]), Enter or Space runs the highlighted one, Escape closes it before any
  /// binding (the menu grabs the keys, [Ui::grab_keys]); closed, focus
  /// goes back where it was. A trigger disabled while its menu is open closes it with `toggle`.
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
    self.menu_popup_placed(id, toggle.then_some(None), style, None, false, items)
  }

  /// [Ui::menu_popup] as a context menu, opened by `at`, a secondary click's point, over `id`
  /// (which needs no element): its top left there, or its right or bottom edge where it
  /// doesn't fit, as AppKit, Windows and GTK place one.
  pub fn menu_popup_at<R>(
    &mut self,
    id: Id,
    at: Option<Point>,
    style: Style,
    items: impl FnOnce(&mut Self) -> R,
  ) -> bool {
    self.menu_popup_placed(id, at.map(Some), style, None, false, items)
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
    self.menu_popup_placed(id, toggle.then_some(None), style, start, true, items)
  }

  /// Opened or closed by `toggle`, with the point of a context menu.
  fn menu_popup_placed<R>(
    &mut self,
    id: Id,
    toggle: Option<Option<Point>>,
    style: Style,
    start: Option<usize>,
    over: bool,
    items: impl FnOnce(&mut Self) -> R,
  ) -> bool {
    let (menu, list) = (id.key("menu"), id.key("menu").key("list"));
    let mut st = std::mem::take(self.state::<Open>(id));
    let mut close = false;
    if let Some(point) = toggle {
      match st.open {
        true => close = true,
        false => {
          let prev = self.focus_id();
          st = Open { open: true, cur: start, prev, reveal: true, start, point, ..Open::default() };
          // What scrolls under it stops: a trackpad's gesture, a wheel's momentum.
          self.st.halt_scrolling();
        }
      }
    }

    let mut run = None;
    if st.open && !close {
      (close, run) = self.menu_keys(menu, &mut st);
      self.grab_keys(menu);
    }

    if !st.open || close {
      if st.open && self.focus_id().is_none_or(|f| f == menu) {
        self.focus(st.prev);
      }

      *self.state::<Open>(id) = Open::default();
      return false;
    }

    // Its highlight follows the pointer within an item too.
    self.track_moves();
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
    // Its nodes are named: the elements after it keep their places among their siblings, so
    // their ids, open or not.
    let (away, place) = (id.key("away"), *self.st.stack.last().unwrap());
    let cover = Style::new().w(pct(100.0)).h(pct(100.0));
    self.with_id(away).float(Float::window().z(MENU_Z)).el(cover);
    let (x, y, ay, room) = self.menu_place(id, style, over, &mut st);
    if over && placed {
      st.at = Some(Point::new(x, y));
    }

    (st.mouse, st.offset) = (self.mouse(), self.scroll(list).map_or(0.0, |s| s.offset.y));
    // No wider than the window: its labels end in "…" instead.
    let w = (self.size().w - 2.0 * EDGE).max(0.0);
    let pop = style.hit().focusable_quiet().gap(0.0).max_w(w).min_w(style.min[0].min(w));
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
    *self.st.stack.last_mut().unwrap() = place;
    let cx = self.set_slot(outer).unwrap_or_default();
    // A menu, its highlighted item the active one; its trigger expanded.
    let active = st.cur.and_then(|i| cx.items.get(i)).map(|x| x.0);
    self.a11y(menu, Role::Menu).active(active);
    if let Some(a) = self.a11y_of(id) {
      a.state(State::Expanded, true);
    }

    // A click on an enabled item or around the menu closes it; one on its scrollbar, padding or
    // a disabled item leaves it open.
    let on_item = |c: Id| cx.items.iter().any(|&(i, on)| on && i == c);
    let clicked = self.st.input.click.is_some_and(|c| c.id == away || on_item(c.id));
    (st.items, st.labels) = (cx.items, cx.labels);
    if clicked || cx.ran {
      if self.focus_id().is_none_or(|f| f == menu) {
        self.focus(st.prev);
      }

      st = Open::default();
    }

    let open = st.open;
    *self.state::<Open>(id) = st;
    open
  }

  /// An open menu's keys and pointer: the pointer moving onto an item highlights it, Up and
  /// Down step over the enabled items, letters typed go to the first one they start; whether
  /// to close (Escape), and the item to run (Enter, Space).
  fn menu_keys(&mut self, menu: Id, st: &mut Open) -> (bool, Option<usize>) {
    let offset = self.scroll(menu.key("list")).map_or(0.0, |s| s.offset.y);
    let over = st.items.iter().position(|&(i, on)| on && self.hovered(i));
    // The pointer moving, or the list scrolling under it, takes the highlight back from keys.
    if st.keyed && (self.mouse() != st.mouse || offset != st.offset) {
      st.keyed = false;
      st.cur = over.or(st.cur);
    }

    if over != st.over && !st.keyed {
      st.cur = over;
    }

    st.over = over;
    let step = |cur: Option<usize>, d: isize| {
      let n = st.items.len() as isize;
      let from = cur.map_or(if d > 0 { -1 } else { n }, |c| c as isize);
      (1..=n).map(|k| (from + d * k).rem_euclid(n.max(1)) as usize).find(|&i| st.items[i].1)
    };
    // Escape closes it before any binding of the app's: the open menu grabs the keys.
    match self.key_down(menu) {
      Some(k) if k.key == Key::Named(NamedKey::Escape) && k.mods.is_empty() => return (true, None),
      Some(_) => self.pass(menu),
      None => {}
    }

    let mut to = None;
    match self.command(menu) {
      Some(Command::Cancel) => return (true, None),
      Some(Command::Move(Move::Down, _)) => to = step(st.cur, 1),
      Some(Command::Move(Move::Up, _)) => to = step(st.cur, -1),
      Some(Command::InsertNewline) => return (false, st.cur),
      // Focus stays in the menu while it is open.
      Some(Command::InsertTab | Command::InsertBacktab) => {}
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
    // At a point, the menu goes 3 below it as under a trigger, or 3 above it flipped: the
    // trigger's gap taken back.
    let at = st.point.map(|p| Rect::new(p.x, p.y + 3.0, 0.0, -6.0));
    let b = at.or(self.rect(id)).unwrap_or_default();
    let menu_w = self.rect(menu).map_or(160.0, |m| m.w);
    let frame = style.pad[0] + style.pad[2] + style.border[0] + style.border[2];
    let natural = self.scroll(list).map_or(0.0, |sc| sc.content.h + frame);
    let below = b.y + b.h + 3.0;
    let (down, up) = ((size.h - EDGE - below).max(0.0), (b.y - 3.0 - EDGE).max(0.0));
    let flip = natural > down && up > down;
    let (mut y, mut ay, mut room) = if flip { (b.y - 3.0, 1.0, up) } else { (below, 0.0, down) };
    let mut x = if b.x + menu_w > size.w - EDGE { b.x + b.w - menu_w } else { b.x };
    if over && let Some(p) = st.at {
      // Where it went, inside the window still when the window shrank.
      room = size.h - 2.0 * EDGE;
      (x, y, ay) = (p.x, p.y.min(size.h - EDGE - natural.min(room)).max(EDGE), 0.0);
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
    self.a11y(id, Role::MenuItem).state(State::Disabled, off);
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

impl Ui<'_> {
  /// The context menu of the element `id`, built before or after it: a secondary press on it or
  /// inside it ([Ui::context_press]) opens `menu()` at the pointer (the closure runs only then,
  /// and while a drawn one is open); returns the id of the item chosen. In a window the
  /// platform shows its own menu ([crate::Runtime::set_native_menus]), free to go past the
  /// window's edges, and the answer comes in a later pass as [Event::Menu]; in the harness it is
  /// drawn ([Ui::menu_popup_at] of `id`): a check column, headers in secondary text, shortcuts
  /// at the right, destructive items red, a submenu's items under its label; each item
  /// `id.key(item.id)`.
  pub fn context_menu(&mut self, id: Id, menu: impl FnOnce() -> Menu) -> Option<String> {
    let at = self.context_press(id);
    self.context_menu_at(id, at, menu)
  }

  /// [Ui::context_menu] opened at `at` by a behaviour that saw the press itself (a field's edit
  /// menu, [crate::EditMenu]).
  pub fn context_menu_at(
    &mut self,
    id: Id,
    at: Option<Point>,
    menu: impl FnOnce() -> Menu,
  ) -> Option<String> {
    let answer = self.st.take(id, |e| match e {
      Event::Menu { item, .. } => Some(item.clone()),
      _ => None,
    });
    if let Some(item) = answer {
      self.state::<Open>(id).open = false;
      return item;
    }

    // Asked for, it stays open until the answer comes.
    if self.st.native_menus {
      if at.is_some() || self.menu_open(id) {
        self.state::<Open>(id).open = true;
      }

      if let Some(at) = at {
        self.st.context_menu = Some((id, at, menu()));
      }

      return None;
    }

    let t = self.theme();
    let menu = (at.is_some() || self.menu_open(id)).then(menu);
    let (check, mut chosen) = (menu.as_ref().is_some_and(|m| checks(&m.items)), None);
    self.menu_popup_at(id, at, t.popup().min_w(160.0), |ui| {
      for it in menu.iter().flat_map(|m| &m.items) {
        ui.model_item(id, it, check, true, &mut chosen);
      }
    });
    chosen
  }

  /// An item of a drawn context menu from its model: a line, a header (a submenu's label, its
  /// items under it), or an item `id.key(it.id)` with its check column, its label (red when
  /// destructive) and its shortcut; `chosen` takes its id when clicked. Under a disabled
  /// submenu (`on` false) none is.
  fn model_item(&mut self, id: Id, it: &Item, check: bool, on: bool, chosen: &mut Option<String>) {
    let t = self.theme();
    let row = |left: f32| {
      let s = Style::new().w(grow()).h(px(t.sizes.item_h)).pads([0.0, 10.0, 0.0, left]);
      s.gap(4.0).align(Align::Start, Align::Center)
    };
    if it.separator {
      let line = Style::new().w(grow()).h(px(1.0)).bg(t.line);
      self.row(Style::new().w(grow()).pads([5.0, 10.0, 5.0, 10.0]), |ui| _ = ui.el(line));
      return;
    }

    // Under the check column, the item's text.
    if it.header || it.submenu.is_some() {
      self.row(row(if check { 26.0 } else { 10.0 }), |ui| ui.text(&it.label, t.section()));
      // A header is only its title, whatever else it carries.
      for sub in it.submenu.iter().filter(|_| !it.header).flat_map(|m| &m.items) {
        self.model_item(id, sub, check, on && it.enabled, chosen);
      }

      return;
    }

    // Named by its label alone, its shortcut apart as in the OS's menus.
    let item = id.key(it.id.as_str());
    let off = !(on && it.enabled);
    let b = self.a11y_label(&it.label).disabled(off).menu_item_behavior(item, &it.label);
    let (bg, ink) = match b.on {
      true => (t.accent, t.on_accent),
      false => (alpha(t.accent, 0.0), if it.destructive { t.danger } else { t.text }),
    };

    let s = row(if check { 6.0 } else { 10.0 })
      .radius((t.sizes.menu_radius - 5.0).max(t.radius))
      .bg(bg)
      .opacity(if b.disabled { t.disabled } else { 1.0 });
    let (on, key) = (it.checked, it.key_label());
    self.a11y(item, Role::MenuItem).state(State::Selected, on);
    self.with_id(item).row(b.pointer(s), |ui| {
      if check {
        ui.canvas(Style::new().w(px(16.0)).h(px(16.0)), move |f, r| {
          if on {
            check_mark(f, r, ink);
          }
        });
      }

      ui.text(&it.label, t.label().color(ink));
      if !key.is_empty() {
        ui.el(Style::new().w(grow()).min_w(20.0));
        ui.text(&key, t.label().color(if b.on { ink } else { t.muted }).nowrap());
      }
    });
    if b.clicked {
      *chosen = Some(it.id.clone());
    }
  }
}

impl UiState {
  /// [Ui::menu_open] as the last frame and the input since left it.
  pub fn menu_open(&self, id: Id) -> bool {
    self.store.peek::<Open>(id).is_some_and(|o| o.open)
  }
}

/// Whether any of `items` has a check mark to show, in a submenu too: the menu has a column
/// for them then.
fn checks(items: &[Item]) -> bool {
  items.iter().any(|it| it.checkable || it.submenu.as_ref().is_some_and(|m| checks(&m.items)))
}

#[cfg(test)]
mod tests {
  use keyboard_types::{Modifiers, NamedKey};

  use super::*;
  use crate::app::{App, Event, Phase};
  use crate::harness::Harness;
  use crate::scene::Size;

  /// A menu inside a scroll area over tall content; its items count their runs.
  #[derive(Default)]
  struct Menus {
    runs: [usize; 3],
    off: bool,
  }

  impl App for Menus {
    fn view(&mut self, ui: &mut Ui) {
      ui.id("area").scroll_area(Style::new().grow(), |ui| {
        ui.el(Style::new().w(px(10.0)).h(px(150.0)));
        ui.disabled(self.off).id("menu").menu("Menu", |ui| {
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
      let (pos, delta, mods) = (Point::new(x, y), Point::new(0.0, -100.0), Modifiers::empty());
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
    h.key(NamedKey::ArrowDown, Modifiers::empty());
    h.key(NamedKey::Enter, Modifiers::empty());
    h.frame();
    assert_eq!(h.app().runs, [0, 0, 1]);
    assert!(h.rect(menu.key("menu")).is_none() && h.focused().is_none());
    // Its button disabled while it is open, it closes.
    h.click_id(menu);
    h.frame();
    h.frame();
    h.app_mut().off = true;
    h.frame();
    h.frame();
    assert!(h.rect(menu.key("menu")).is_none());
  }

  /// A menu of 30 items, taller than its window, opened by a button the app can disable; the
  /// item run last and what [Ui::menu_popup] returned last.
  #[derive(Default)]
  struct Long {
    ran: Option<usize>,
    open: bool,
    off: bool,
  }

  impl App for Long {
    fn view(&mut self, ui: &mut Ui) {
      let id = Id::of("open");
      let b = ui.disabled(self.off).with_id(id).button("Open");
      let toggle = b.clicked || self.off && ui.menu_open(id);
      let t = ui.theme();
      self.open = ui.menu_popup(id, toggle, t.popup(), |ui| {
        for i in 0..30 {
          if ui.key(i).menu_item("Item").clicked {
            self.ran = Some(i);
          }
        }
      });
    }
  }

  #[test]
  fn a_tall_menu_scrolls_keeps_focus_and_follows_the_pointer() {
    let mut h = Harness::new(Long::default(), Size::new(300.0, 240.0), 1.0);
    h.frame();
    let open = Id::of("open");
    let (menu, list) = (open.key("menu"), open.key("menu").key("list"));
    h.click_id(open);
    h.frame();
    h.frame();
    let m = h.rect(menu).unwrap();
    assert!(m.y + m.h <= 240.0 && h.app().open, "{m:?}");
    // A click on its scrollbar's track pages the items and leaves it open.
    let track = h.rect(list.key(("track", 1usize))).expect("a scrollbar");
    h.click(track.x + track.w / 2.0, track.y + track.h - 4.0);
    h.frame();
    assert!(h.rect(menu).is_some() && h.rt.ui.scroll(list).unwrap().offset.y > 0.0);
    // Tab leaves focus in it.
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focused(), Some(menu));
    // The pointer on an item, Down past it, then the pointer moving within it: it takes the
    // highlight back, and Enter runs it.
    // Items are keyed in the menu's list.
    let under = (0..30usize).find_map(|k| {
      let r = h.rect(list.key(k))?;
      (r.y > m.y + 30.0).then_some((k, r))
    });
    let (k, rr) = under.expect("an item in view");
    h.move_to(rr.x + 10.0, rr.y + 5.0);
    h.frame();
    h.key(NamedKey::ArrowDown, Modifiers::empty());
    assert!(h.move_to(rr.x + 20.0, rr.y + 6.0), "a move within an item draws");
    h.key(NamedKey::Enter, Modifiers::empty());
    assert_eq!(h.app().ran, Some(k));
    h.frame();
    // A click on an item: the call that sees it says closed.
    h.click_id(open);
    h.frame();
    h.frame();
    let rr = h.rect(list.key(k)).unwrap();
    h.click(rr.x + 10.0, rr.y + 5.0);
    assert!(!h.app().open && h.rect(menu).is_some(), "closed in the pass that ran it");
    // Disabled while open, it closes.
    h.click_id(open);
    h.frame();
    h.frame();
    h.app_mut().off = true;
    h.frame();
    h.frame();
    assert!(h.rect(menu).is_none() && !h.app().open);
  }

  /// A menu with a label longer than its window is wide.
  struct Wide;

  impl App for Wide {
    fn view(&mut self, ui: &mut Ui) {
      ui.id("m").menu("Menu", |ui| {
        ui.menu_item(
          "A label far longer than this narrow window is wide, which ends in an ellipsis",
        );
      });
    }
  }

  #[test]
  fn a_menu_is_no_wider_than_its_window() {
    let mut h = Harness::new(Wide, Size::new(200.0, 200.0), 1.0);
    h.frame();
    h.click_id(Id::of("m"));
    h.frame();
    h.frame();
    let m = h.rect(Id::of("m").key("menu")).unwrap();
    assert!(m.x >= 0.0 && m.x + m.w <= 200.0, "{m:?}");
  }

  /// An element whose context menu has a disabled submenu, its items "a" and "b".
  #[derive(Default)]
  struct Sub {
    chosen: Option<String>,
  }

  impl App for Sub {
    fn view(&mut self, ui: &mut Ui) {
      let el = ui.id("el").el(Style::new().w(px(100.0)).h(px(40.0)));
      let items = vec![Item::new("a", "A"), Item::new("b", "B")];
      let sub = Item::new("sub", "Sub").submenu(Menu::new(items)).enabled(false);
      if let Some(item) = ui.context_menu(el.id, || Menu::new(vec![Item::new("x", "X"), sub])) {
        self.chosen = Some(item);
      }
    }
  }

  #[test]
  fn a_disabled_submenus_items_are_disabled_when_drawn() {
    let mut h = Harness::new(Sub::default(), Size::new(300.0, 300.0), 1.0);
    h.frame();
    h.right_click(50.0, 20.0);
    h.frame();
    h.frame();
    let dump = h.a11y();
    assert!(dump.contains("menu item \"A\" disabled"), "{dump}");
    assert!(dump.contains("menu item \"X\"") && !dump.contains("\"X\" disabled"), "{dump}");
    h.click_id(Id::of("el").key("a"));
    h.frame();
    assert_eq!(h.app().chosen, None);
  }

  /// A button inside an element with a menu; its clicks counted.
  #[derive(Default)]
  struct Boxed {
    clicks: usize,
  }

  impl App for Boxed {
    fn view(&mut self, ui: &mut Ui) {
      let r = ui.id("box").row(Style::new().w(px(200.0)).h(px(60.0)).pad(10.0), |ui| {
        if ui.id("b").button("B").clicked {
          self.clicks += 1;
        }
      });
      ui.context_menu(r.id, || Menu::new(vec![Item::new("x", "X")]));
    }
  }

  #[test]
  fn a_control_click_on_a_button_is_no_click_on_the_mac() {
    use crate::keys::Os;
    let mut h = Harness::new(Boxed::default(), Size::new(300.0, 300.0), 1.0);
    h.set_os(Os::Mac);
    h.native_menus();
    h.frame();
    let r = h.rect(Id::of("b")).unwrap();
    let pos = Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
    let (button, mods) = (crate::app::MouseButton::Left, Modifiers::CONTROL);
    h.move_to(pos.x, pos.y);
    h.event(Event::MouseDown { pos, button, clicks: 1, mods });
    h.frame();
    h.event(Event::MouseUp { pos, button, mods });
    h.frame();
    assert!(h.context_menu(None).is_some(), "the box's menu");
    h.frame();
    assert_eq!(h.app().clicks, 0);
    h.click(pos.x, pos.y);
    h.frame();
    assert_eq!(h.app().clicks, 1, "a plain click still clicks");
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
