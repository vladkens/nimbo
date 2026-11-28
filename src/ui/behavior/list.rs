// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A virtualized list's behaviour: its selection by row key ([Selection]), its keys and presses
//! ([Ui::list_behavior]), and its rows in view built between two spacers that give its scroll
//! area the whole content's height ([Ui::list_rows]); `List` draws one, and its docs say
//! what each key and press does.

use std::collections::HashSet;
use std::fmt;
use std::hash::BuildHasherDefault;
use std::ops::Range;

use crate::Modifiers;
use crate::app::MouseButton;
use crate::keys::{Binding, CMD, Command, Delete, Extent, Move, Os, Shortcut};
use crate::scene::{Point, Rect};
use crate::text::FxHasher;
use crate::ui::input::ScrollState;
use crate::ui::{Enabled, Focus, Id, Role, State, Style, Ui, px};

/// Rows built past each edge of the viewport.
const OVERSCAN: usize = 2;

/// Cmd-A selects every row of a multi-select list.
const MULTI_KEYS: &[Binding] = &[Binding::new(Shortcut::char('a').cmd(), Command::SelectAll)];

type KeySet = HashSet<u64, BuildHasherDefault<FxHasher>>;

// MARK: Selection

/// The selected rows of a list, by row key: one at a time, or several ([Self::multiple]), as
/// SwiftUI's `List(selection:)` takes an optional value or a set. The cursor is the row keys
/// move from (the last one pressed or moved to), the anchor where Shift extends from. Each keeps
/// the index it was last seen at as a hint, so finding it again is one lookup unless rows moved,
/// then a scan.
#[derive(Clone, Debug, Default)]
pub struct Selection {
  keys: KeySet,
  cursor: Option<(u64, usize)>,
  anchor: Option<(u64, usize)>,
  multiple: bool,
}

/// Row `at`'s index among `n` rows keyed by `key`: its hint, else a scan.
fn find(at: (u64, usize), n: usize, key: &dyn Fn(usize) -> u64) -> Option<usize> {
  if at.1 < n && key(at.1) == at.0 {
    return Some(at.1);
  }

  (0..n).find(|&i| key(i) == at.0)
}

impl Selection {
  /// Several rows at a time in a list over it when `yes`: Cmd-click toggles, Shift-click and
  /// Shift-arrows extend, Cmd-A selects all; else one at a time, as by default.
  pub fn multiple(self, yes: bool) -> Self {
    Self { multiple: yes, ..self }
  }

  /// Whether a list over it selects several rows at a time ([Self::multiple]).
  pub fn is_multiple(&self) -> bool {
    self.multiple
  }

  pub fn contains(&self, key: u64) -> bool {
    self.keys.contains(&key)
  }

  pub fn len(&self) -> usize {
    self.keys.len()
  }

  pub fn is_empty(&self) -> bool {
    self.keys.is_empty()
  }

  /// The selected keys, in no order.
  pub fn keys(&self) -> impl Iterator<Item = u64> + '_ {
    self.keys.iter().copied()
  }

  /// The cursor row's key.
  pub fn cursor(&self) -> Option<u64> {
    self.cursor.map(|c| c.0)
  }

  /// The cursor row's index among `n` rows keyed by `key`, if it is still there.
  pub fn cursor_index(&self, n: usize, key: &dyn Fn(usize) -> u64) -> Option<usize> {
    find(self.cursor?, n, key)
  }

  pub fn clear(&mut self) {
    self.keys.clear();
    (self.cursor, self.anchor) = (None, None);
  }

  /// Keeps the keys `f` keeps (rows still there after a delete or a reload), the cursor and the
  /// anchor too while theirs is kept; `f` is asked once per key.
  pub fn retain(&mut self, mut f: impl FnMut(u64) -> bool) {
    // A cursor or an anchor off the selection (Cmd-click took its row out) is asked about here;
    // one on a selected row goes with its key.
    let off = |at: Option<(u64, usize)>| at.map(|a| a.0).filter(|k| !self.keys.contains(k));
    let (c, a) = (off(self.cursor), off(self.anchor));
    let keep_c = c.map(&mut f);
    let keep_a = if a == c { keep_c } else { a.map(&mut f) };
    self.keys.retain(|&k| f(k));
    self.cursor = self.cursor.filter(|c| keep_c.unwrap_or_else(|| self.keys.contains(&c.0)));
    self.anchor = self.anchor.filter(|a| keep_a.unwrap_or_else(|| self.keys.contains(&a.0)));
  }

  /// Selects only row `i`, keyed `key`, with the cursor and the anchor on it.
  pub fn select(&mut self, key: u64, i: usize) {
    self.keys.clear();
    self.keys.insert(key);
    (self.cursor, self.anchor) = (Some((key, i)), Some((key, i)));
  }

  /// Adds row `i` or takes it out (Cmd-click); the cursor and the anchor go to it.
  pub fn toggle(&mut self, key: u64, i: usize) {
    if !self.keys.remove(&key) {
      self.keys.insert(key);
    }

    (self.cursor, self.anchor) = (Some((key, i)), Some((key, i)));
  }

  /// Selects the rows from the anchor to row `i` (Shift-click, Shift-arrows), in place of what
  /// was selected; the cursor goes to `i`, the anchor stays (or starts at `i`).
  pub fn extend(&mut self, i: usize, n: usize, key: &dyn Fn(usize) -> u64) {
    let a = self.anchor.and_then(|a| find(a, n, key)).unwrap_or(i);
    self.keys.clear();
    self.keys.extend((a.min(i)..=a.max(i)).map(key));
    self.cursor = Some((key(i), i));
    self.anchor = Some((key(a), a));
  }

  /// What a click on row `i` of `n` rows keyed by `key` does in a multi-select `List`: Cmd
  /// toggles it ([Self::toggle]), else Shift extends to it ([Self::extend]), else it is selected
  /// alone ([Self::select]). A list's rows do it already; an app calls it for rows it lays out
  /// itself, such as rows grouped under headings.
  pub fn click(&mut self, i: usize, mods: Modifiers, n: usize, key: &dyn Fn(usize) -> u64) {
    match (mods.contains(CMD), mods.shift()) {
      (true, _) => self.toggle(key(i), i),
      (_, true) => self.extend(i, n, key),
      _ => self.select(key(i), i),
    }
  }

  /// Selects all `n` rows, the cursor staying.
  pub fn select_all(&mut self, n: usize, key: &dyn Fn(usize) -> u64) {
    self.keys.clear();
    self.keys.extend((0..n).map(key));
  }
}

/// Where a list's rows lie ([Ui::list_behavior]): rows of one height from the top of its
/// content, under its padding, in sections with a header above each.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ListRows<'s> {
  n: usize,
  h: f32,
  starts: &'s [usize],
  head_h: f32,
  gap: f32,
  pad: [f32; 2],
}

/// The header above a row that starts a section ([ListRows::header]).
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct SectionHeader {
  /// The section's place among the list's.
  pub section: usize,
  /// Its height, the spacing above it included.
  pub height: f32,
}

impl<'s> ListRows<'s> {
  /// `count` rows `row_height` high, in no sections, with no padding.
  pub fn new(count: usize, row_height: f32) -> Self {
    Self { n: count, h: row_height, ..Self::default() }
  }

  /// Rows in sections: section `g` starts at row `starts[g]` (ascending, none empty), under a
  /// header `header_height` high; rows before the first start have none.
  pub fn sections(self, starts: &'s [usize], header_height: f32) -> Self {
    Self { starts, head_h: header_height, ..self }
  }

  /// The space above each header but one at the top of the content (between grouped plates).
  pub fn section_spacing(self, spacing: f32) -> Self {
    Self { gap: spacing, ..self }
  }

  /// The content's padding above the first row and below the last.
  pub fn padding(self, top: f32, bottom: f32) -> Self {
    Self { pad: [top, bottom], ..self }
  }

  /// The header above row `i` when it starts a section; `None` for a row inside one.
  pub fn header(&self, i: usize) -> Option<SectionHeader> {
    let g = self.heads(i);
    let height = || self.above(g + 1) - self.above(g);
    (self.starts.get(g) == Some(&i)).then(|| SectionHeader { section: g, height: height() })
  }

  /// The headers above row `i`'s own: those of the sections that start before it.
  fn heads(&self, i: usize) -> usize {
    self.starts.partition_point(|&s| s < i)
  }

  /// The top of row `i`, or of its section's header when it starts one; the content's height
  /// for `n`.
  fn top(&self, i: usize) -> f32 {
    i as f32 * self.h + self.above(self.heads(i))
  }

  /// The height of the first `k` headers with their gaps.
  fn above(&self, k: usize) -> f32 {
    let top = (self.starts.first() == Some(&0)) as usize;
    k as f32 * self.head_h + k.saturating_sub(top) as f32 * self.gap
  }

  /// The row at depth `y`, or under the header there, clamped to the rows.
  fn at(&self, y: f32) -> usize {
    let g = self.starts.partition_point(|&s| self.top(s) <= y);
    let (from, y) = match g {
      0 => (0, y),
      g => (self.starts[g - 1], y - self.starts[g - 1] as f32 * self.h - self.above(g)),
    };

    (from + (y / self.h).floor().max(0.0) as usize).min(self.n.saturating_sub(1))
  }
}

/// A list's state in the store, under its id.
#[derive(Default)]
struct ListState {
  /// A row to scroll into view once the list knows its viewport.
  reveal: Option<usize>,
  /// The press a pass saw last ([crate::ui::Press::seq]).
  pressed: Option<u32>,
  /// The rows built last time, keyed as then, from index `from`: what a press can be on.
  shown: Vec<u64>,
  from: usize,
}

// MARK: Behaviour

/// What a list got this frame ([Ui::list_behavior]), read before its rows are built, which it
/// builds ([Ui::list_rows]).
#[derive(Clone, Copy, Debug)]
pub struct ListBehavior<'s> {
  pub id: Id,
  /// Disabled by [Ui::disabled]: no input was read, and it gave its focus up.
  pub disabled: bool,
  /// Focused in the key window: when to draw the selected rows in the accent.
  pub focused: bool,
  /// The selection changed.
  pub changed: bool,
  /// A row was pressed (and selected as the press asked).
  pub pressed: Option<usize>,
  /// A row was double-clicked, or Enter pressed on the cursor row.
  pub activated: Option<usize>,
  /// Delete or Backspace with rows selected: the app asks to delete them.
  pub delete: bool,
  rows: ListRows<'s>,
  sel: &'s Selection,
  key: RowKeys<'s>,
}

/// A list's row keys ([Ui::list_behavior]), shown by name alone.
#[derive(Clone, Copy)]
struct RowKeys<'s>(&'s dyn Fn(usize) -> u64);

impl fmt::Debug for RowKeys<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("RowKeys")
  }
}

/// What the input did to a list ([ListBehavior]'s events).
#[derive(Default)]
struct Events {
  changed: bool,
  pressed: Option<usize>,
  activated: Option<usize>,
  delete: bool,
}

impl<'s> ListBehavior<'s> {
  /// The selection after the input, as the rows are to show it.
  pub fn selection(&self) -> &'s Selection {
    self.sel
  }

  /// The list's box: takes focus by a press, with no ring of its own (its selection shows it);
  /// disabled, none.
  pub const fn style(&self, s: Style) -> Style {
    if self.disabled { s } else { s.focusable_quiet() }
  }
}

impl<'a> Ui<'a> {
  /// The input of list `id`, its rows lying as `rows` says, keyed by `key` (unique and stable
  /// while the row is the same thing), before it is built: a press selects a row into `sel` and
  /// focuses the list, the keys move and extend the selection, Enter and a double click activate
  /// a row, Delete asks to delete the selection; several rows at a time in a
  /// [Selection::multiple] (`List` has the whole of it). A row asked for by the keys or
  /// [Ui::reveal_row] scrolls into view. A list with its rows selected to assistive technology, the
  /// cursor's row the active one. Build the list right after as a scroll area under `id` with
  /// [ListBehavior::style], its rows with [Ui::list_rows]; [Ui::disabled] before the call disables
  /// it.
  pub fn list_behavior<'s>(
    &mut self,
    id: Id,
    rows: ListRows<'s>,
    sel: &'s mut Selection,
    key: &'s dyn Fn(usize) -> u64,
  ) -> ListBehavior<'s> {
    let disabled = !self.environment_value::<Enabled>().0;
    if disabled && self.focused(id) {
      self.focus(None);
    }

    let mut st = std::mem::take(self.state::<ListState>(id));
    let focused = self.focus_visible(id);
    let (mut out, multi) = (Events::default(), sel.multiple);
    let sc = self.scroll(id);
    let view_h = sc.map_or(self.size().h, |s| s.viewport.h);
    if rows.n > 0 && !disabled {
      self.list_keys(id, &mut st, sel, &mut out, (rows, view_h, multi), key);
    }

    if !disabled {
      self.list_press(id, &mut st, sel, &mut out, rows.n, multi, key);
    }

    // Scroll a row into view, by the last frame's viewport; the first and the last with the
    // list's padding.
    // Not laid out yet, it waits for the next frame, which comes at once.
    let [pad_t, pad_b] = rows.pad;
    match (st.reveal, sc) {
      (Some(i), Some(sc)) => {
        st.reveal = None;
        let top = if i == 0 { 0.0 } else { pad_t + rows.top(i) };
        let end = pad_t + rows.top(i + 1) + if i + 1 == rows.n { pad_b } else { 0.0 };
        if i < rows.n {
          self.reveal(id, Rect::new(sc.offset.x, top, 1.0, end - top));
        }
      }
      (Some(_), None) => self.st.animating = true,
      _ => {}
    }

    if multi && !disabled {
      self.keymap(MULTI_KEYS);
    }

    // A list of rows, the cursor's the active one.
    let active = sel.cursor.map(|c| id.key(("row", c.0)));
    self.a11y(id, Role::List).state(State::Disabled, disabled).active(active);
    *self.state::<ListState>(id) = st;
    let Events { changed, pressed, activated, delete } = out;
    let key = RowKeys(key);
    ListBehavior { id, disabled, focused, changed, pressed, activated, delete, rows, sel, key }
  }

  /// The rows in view and a few around them of the list whose input `b` took
  /// ([Ui::list_behavior]), built inside its scroll area: a spacer for the rows above,
  /// `row(ui, index, key)` for each (its element `b.id.key(("row", key))`, a row selected in the
  /// selection to assistive technology), a spacer for the rows below; returns the rows built. They
  /// come from the scroll offset as of the last frame and the input since, clamped to the content
  /// as it is now; when the layout shows more than was built, the view runs again at once, so a
  /// frame never shows a gap.
  pub fn list_rows(
    &mut self,
    b: &ListBehavior,
    row: impl FnMut(&mut Ui<'a>, usize, u64),
  ) -> Range<usize> {
    // The values put for them reach every row past the spacer above.
    self.group(|ui| ui.rows_in_view(b, row))
  }

  fn rows_in_view(
    &mut self,
    b: &ListBehavior,
    mut row: impl FnMut(&mut Ui<'a>, usize, u64),
  ) -> Range<usize> {
    let (id, rows, sel, key) = (b.id, b.rows, b.sel, b.key.0);
    let mut st = std::mem::take(self.state::<ListState>(id));
    let view_h = self.scroll(id).map_or(self.size().h, |s| s.viewport.h);
    // The offset after the input and the scroll area's own (a thumb drag), clamped to the
    // content as it is now.
    let ([pad_t, pad_b], n) = (rows.pad, rows.n);
    let total = rows.top(n);
    let max = (pad_t + total + pad_b - view_h).max(0.0);
    let off = self.scroll_raw(id).y.clamp(0.0, max);
    let at = |y: f32| rows.at(y - pad_t);
    let shown = match n {
      0 => 0..0,
      _ => at(off).saturating_sub(OVERSCAN)..(at(off + view_h) + 1 + OVERSCAN).min(n),
    };

    self.el(Style::new().h(px(rows.top(shown.start))));
    st.shown.clear();
    st.shown.extend(shown.clone().map(key));
    st.from = shown.start;
    for (i, &k) in shown.clone().zip(&st.shown) {
      self.a11y(id.key(("row", k)), Role::Row).state(State::Selected, sel.contains(k));
      row(self, i, k);
    }

    self.el(Style::new().h(px(total - rows.top(shown.end))));
    // What the built rows cover, for the runtime to run the view again if the layout shows
    // more (the ends are open when they reach the list's).
    let lo = if shown.start == 0 { f32::NEG_INFINITY } else { pad_t + rows.top(shown.start) };
    let hi = if shown.end == n { f32::INFINITY } else { pad_t + rows.top(shown.end) };
    let node = self.st.stack.last().unwrap().0;
    self.st.tree.partial.push((node, node, lo.into(), hi.into()));
    *self.state::<ListState>(id) = st;
    shown
  }

  /// Scrolls row `i` of list `id` into view, the least that shows it, in the list's next build.
  pub fn reveal_row(&mut self, id: Id, i: usize) {
    self.state::<ListState>(id).reveal = Some(i);
  }

  /// A scroll container's offset as last set, before the layout clamps it.
  fn scroll_raw(&self, id: Id) -> Point {
    let raw = self.st.store.peek::<ScrollState>(id);
    let raw = raw.map(|s| Point::new(s.offset[0] as f32, s.offset[1] as f32));
    raw.or_else(|| self.scroll(id).map(|s| s.offset)).unwrap_or_default()
  }

  /// The list's keys, as commands: the cursor moves (Shift extends a multi-select), Enter
  /// activates, Cmd-A selects all, Delete asks to delete the selection, Escape clears it; `geo` is
  /// where the rows lie, the viewport's height and whether several rows can be selected.
  fn list_keys(
    &mut self,
    id: Id,
    st: &mut ListState,
    sel: &mut Selection,
    out: &mut Events,
    geo: (ListRows, f32, bool),
    key: &dyn Fn(usize) -> u64,
  ) {
    let (sp, view_h, multi) = geo;
    let n = sp.n;
    let Some(c) = self.command(id) else { return };
    let cur = sel.cursor_index(n, key);

    // A page: the farthest row within a viewport of the cursor's row, by the rows' own tops
    // (under their headers), its bottom from the cursor's top going down, its top from the
    // cursor's bottom going up; at least one row.
    let page = |dir: isize| {
      let c = cur.unwrap_or(0);
      let top = |i: usize| i as f32 * sp.h + sp.above(sp.heads(i + 1));
      let reach = view_h - sp.h;

      match dir > 0 {
        true => {
          let y = top(c) + reach;
          let i = sp.at(y);
          let i = if top(i) > y { i.saturating_sub(1) } else { i };
          i.max(c + 1).min(n - 1)
        }
        false => {
          let y = top(c) - reach;
          let i = sp.at(y);
          let i = if top(i) < y { i + 1 } else { i };
          i.min(c.saturating_sub(1))
        }
      }
    };

    let step = |d: isize| match cur {
      Some(c) => (c as isize + d).clamp(0, n as isize - 1) as usize,
      None if d > 0 => 0,
      None => n - 1,
    };

    use Delete::*;
    use Move::*;

    let (to, shift) = match c {
      Command::Move(Up, s) => (step(-1), s),
      Command::Move(Down, s) => (step(1), s),
      Command::Move(PageUp, s) => (page(-1), s),
      Command::Move(PageDown, s) => (page(1), s),
      Command::ScrollPageUp => (page(-1), Extent::Caret),
      Command::ScrollPageDown => (page(1), Extent::Caret),
      Command::Move(DocStart | LineStart, s) => (0, s),
      Command::Move(DocEnd | LineEnd, s) => (n - 1, s),
      Command::ScrollToTop => (0, Extent::Caret),
      Command::ScrollToBottom => (n - 1, Extent::Caret),
      Command::InsertNewline => {
        out.activated = cur;
        return;
      }
      Command::SelectAll if multi => {
        sel.select_all(n, key);
        out.changed = true;
        return;
      }

      Command::Delete(Backward | Forward | ToLineStart) if !sel.is_empty() => {
        out.delete = true;
        return;
      }
      Command::Cancel if !sel.is_empty() => {
        sel.clear();
        out.changed = true;
        return;
      }
      _ => {
        self.pass(id);
        return;
      }
    };

    match shift == Extent::Selection && multi {
      true => sel.extend(to, n, key),
      false => sel.select(key(to), to),
    }

    (st.reveal, out.changed) = (Some(to), true);
  }

  /// A press on a row selects it (a click when no frame saw its press: a quick one), as the
  /// modifiers ask; a double click activates it. A secondary one, as a context menu opens
  /// ([Ui::context_press]), focuses the list and selects the row alone unless it is selected
  /// (Finder's rule): the app's context menu after it reads that selection.
  #[expect(clippy::too_many_arguments, reason = "the list's state, its response and the rows")]
  fn list_press(
    &mut self,
    id: Id,
    st: &mut ListState,
    sel: &mut Selection,
    out: &mut Events,
    n: usize,
    multi: bool,
    key: &dyn Fn(usize) -> u64,
  ) {
    // The secondary button's press, on Windows its release (a Control-click is the Mac's).
    let win = self.st.os == Os::Windows;
    let press = self
      .press()
      .filter(|p| p.button == MouseButton::Left || p.button == MouseButton::Right && !win);
    // A press is seen once; a click whose press no pass saw (a quick one) stands for it.
    let both = |b| matches!(b, MouseButton::Left | MouseButton::Right);
    let click = self.st.input.click.filter(|c| both(c.button));
    let hit = match (press, click) {
      (Some(p), _) if st.pressed != Some(p.seq) => Some((p.id, p.mods, p.clicks, p.button, p.seq)),
      (None, Some(c)) if st.pressed != Some(c.seq) => {
        Some((c.id, c.mods, c.clicks, c.button, c.seq))
      }
      _ => None,
    };

    st.pressed = press.map(|p| p.seq).or(click.map(|c| c.seq)).or(st.pressed);
    let Some((on, mods, clicks, button, seq)) = hit else { return };
    let right = button == MouseButton::Right;
    // Its index as built, a hint that saves a scan unless the rows moved since; a press on an
    // element in a row is the row's (text with a tooltip), unless that element is a control
    // (focusable: a button) or, for a secondary one, has a menu of its own, as in SwiftUI.
    let row = |on: Id| st.shown.iter().position(|&k| id.key(("row", k)) == on);
    let (t, mut own) = (&self.st.last, false);
    let mut up = t.find(on);
    let at = row(on).or_else(|| {
      loop {
        let n = &t.nodes[up? as usize];
        if let Some(at) = row(n.id) {
          break Some(at);
        }

        if n.id == id || up == Some(0) {
          break None;
        }

        own |= if right { t.menus.contains(&n.id) } else { n.style.focus != Focus::None };
        up = Some(n.parent);
      }
    });
    if own {
      return;
    }

    let Some((k, i)) =
      at.and_then(|at| Some((st.shown[at], find((st.shown[at], st.from + at), n, key)?)))
    else {
      // Off the rows (the scrollbar, the padding) the list's menu, a row's, opens none; with
      // none of its own, its parent's does.
      if right && self.st.last.menus.contains(&id) && self.st.last.within(on, id) {
        self.st.menu_seq = seq;
      }

      // A plain press on its ground clears the selection, as a table view's on macOS.
      let plain = !mods.contains(CMD) && !mods.shift();
      if !right && plain && !sel.is_empty() && self.list_ground(on, id) {
        sel.clear();
        out.changed = true;
      }

      return;
    };
    match (right, multi) {
      (true, _) => {
        self.focus(Some(id));
        if sel.contains(k) {
          return;
        }

        sel.select(k, i);
      }
      (_, true) => sel.click(i, mods, n, key),
      _ => sel.select(k, i),
    }

    (out.pressed, out.changed) = (Some(i), true);
    if clicks == 2 && !right {
      out.activated = Some(i);
    }
  }

  /// Whether element `on`, off the rows, is list `id`'s ground: a header, the padding, the room
  /// under the rows; not a control in them (a disabled one too, by its role) nor a scrollbar.
  fn list_ground(&self, on: Id, id: Id) -> bool {
    let t = &self.st.last;
    if (0..2usize).any(|ax| t.within(on, id.key(("track", ax)))) {
      return false;
    }

    let mut up = t.find(on);
    while let Some(i) = up {
      let n = &t.nodes[i as usize];
      if n.id == id {
        return true;
      }

      if i == 0 || t.a11y.role(n.id).is_some_and(Role::control) {
        return false;
      }

      up = Some(n.parent);
    }

    false
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn selection_by_key_survives_moves() {
    let rows = |v: &'static [u64]| move |i: usize| v[i];
    let mut s = Selection::default();
    let key = rows(&[10, 11, 12, 13, 14]);
    s.select(12, 2);
    assert_eq!((s.cursor(), s.cursor_index(5, &key)), (Some(12), Some(2)));

    // Shift extends from the anchor, both ways, replacing the range.
    s.extend(4, 5, &key);
    assert_eq!(sorted(&s), [12, 13, 14]);
    s.extend(0, 5, &key);
    assert_eq!(sorted(&s), [10, 11, 12]);

    // Cmd toggles and moves the anchor.
    s.toggle(11, 1);
    assert_eq!(sorted(&s), [10, 12]);
    s.extend(3, 5, &key);
    assert_eq!(sorted(&s), [11, 12, 13]);

    // Rows inserted above: found again by key.
    let moved = rows(&[1, 2, 10, 11, 12, 13, 14]);
    assert_eq!(s.cursor_index(7, &moved), Some(5));
    assert!(s.contains(13) && !s.contains(1));
    s.select_all(7, &moved);
    assert_eq!(s.len(), 7);

    // Rows gone: their keys go, the cursor with its row; the rest stays.
    s.retain(|k| k != 13 && k != 2);
    assert_eq!((s.len(), s.cursor()), (5, None));
    s.select(12, 4);
    s.retain(|k| k != 2);
    assert_eq!((sorted(&s), s.cursor()), (vec![12], Some(12)));

    // Each key is asked once: a predicate that drops 12 and would keep it when asked again
    // leaves no cursor on it.
    s.select_all(7, &moved);
    let mut asked = Vec::new();
    s.retain(|k| {
      asked.push(k);
      k != 12 || asked.iter().filter(|&&a| a == 12).count() > 1
    });
    asked.sort_unstable();
    assert_eq!(asked, [1, 2, 10, 11, 12, 13, 14]);
    assert!(!s.contains(12) && s.cursor().is_none());

    // A row Cmd-click took out keeps the cursor and the anchor while `f` keeps it, asked once.
    s.select_all(7, &moved);
    s.toggle(13, 5);
    asked.clear();
    s.retain(|k| {
      asked.push(k);
      k != 2
    });
    asked.sort_unstable();
    assert_eq!(asked, [1, 2, 10, 11, 12, 13, 14]);
    assert_eq!((s.contains(13), s.cursor()), (false, Some(13)));

    s.extend(6, 7, &moved);
    assert_eq!(sorted(&s), [13, 14]);
    s.toggle(13, 5);
    s.retain(|k| k != 13);
    assert_eq!((sorted(&s), s.cursor()), (vec![14], None));
    s.clear();
    assert!(s.is_empty() && s.cursor().is_none());
  }

  #[test]
  fn rows_lie_under_their_headers() {
    // Rows 0-2 and 3-5, 10 high, under headers 5 high.
    let sp = ListRows { n: 6, h: 10.0, starts: &[0, 3], head_h: 5.0, gap: 0.0, pad: [0.0; 2] };
    let tops: Vec<f32> = (0..=6).map(|i| sp.top(i)).collect();
    assert_eq!(tops, [0.0, 15.0, 25.0, 35.0, 50.0, 60.0, 70.0]);
    // A header's depth is its first row's; past the end, the last row.
    let at = [(0.0, 0), (4.9, 0), (5.0, 0), (15.0, 1), (34.9, 2), (36.0, 3), (49.9, 3)];
    for (y, i) in at.into_iter().chain([(50.0, 4), (1e6, 5), (-3.0, 0)]) {
      assert_eq!(sp.at(y), i, "at {y}");
    }

    // Rows before the first section have no header; with none, rows only.
    let sp = ListRows { starts: &[2], ..sp };
    assert_eq!((sp.top(2), sp.top(3), sp.at(26.0)), (20.0, 35.0, 2));
    let sp = ListRows { starts: &[], ..sp };
    assert_eq!((sp.top(6), sp.at(25.0)), (60.0, 2));
    // A gap of 20 above each header but one at the top.
    let sp = ListRows { starts: &[0, 3], gap: 20.0, ..sp };
    assert_eq!((sp.top(1), sp.top(3), sp.top(4), sp.top(6)), (15.0, 35.0, 70.0, 90.0));
    assert_eq!((sp.at(50.0), sp.at(64.0), sp.at(75.0)), (3, 3, 4));
    let sp = ListRows { starts: &[2], ..sp };
    assert_eq!((sp.top(2), sp.top(3), sp.at(30.0)), (20.0, 55.0, 2));
  }

  #[test]
  fn selection_click_follows_modifiers() {
    let key = |i: usize| [10, 11, 12, 13, 14][i];
    let (none, shift) = (Modifiers::empty(), Modifiers::SHIFT);
    let mut s = Selection::default();

    // Shift with no anchor yet starts at the row.
    s.click(2, shift, 5, &key);
    assert_eq!(sorted(&s), [12]);

    // A plain click selects the row alone.
    s.select_all(5, &key);
    s.click(2, none, 5, &key);
    assert_eq!((sorted(&s), s.cursor()), (vec![12], Some(12)));

    // Shift extends from the anchor, both ways, replacing the range.
    s.click(4, shift, 5, &key);
    assert_eq!(sorted(&s), [12, 13, 14]);
    s.click(0, shift, 5, &key);
    assert_eq!((sorted(&s), s.cursor()), (vec![10, 11, 12], Some(10)));

    // Cmd toggles, out and back in, and wins over Shift.
    s.click(1, CMD, 5, &key);
    assert_eq!(sorted(&s), [10, 12]);
    s.click(4, CMD | shift, 5, &key);
    assert_eq!(sorted(&s), [10, 12, 14]);
    s.click(4, CMD, 5, &key);
    assert_eq!(sorted(&s), [10, 12]);

    // Shift after Cmd extends from the row Cmd clicked last.
    s.click(2, shift, 5, &key);
    assert_eq!(sorted(&s), [12, 13, 14]);
  }

  fn sorted(s: &Selection) -> Vec<u64> {
    let mut v: Vec<_> = s.keys().collect();
    v.sort();
    v
  }
}
