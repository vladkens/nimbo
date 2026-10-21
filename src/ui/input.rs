// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Pointer input over the last frame's laid-out tree: hit testing, hover, press, click, focus
//! and scroll containers (offsets, nested chaining, wheel momentum), and the window's key
//! status.
//!
//! Only interactive elements take part: named ones ([super::Ui::id]), scroll containers and
//! those styled [super::Style::hit]; text, decoration and unnamed boxes let the pointer through
//! to what is under them. Hover is the topmost interactive element under the pointer and its
//! interactive ancestors; a press, a click and focus go to the topmost one alone.

use std::cell::Cell;
use std::time::Duration;

use super::layout::{Node, Tree};
use super::{Focus, Id, UiState};
use crate::app::{Event, Mods, MouseButton, Phase};
use crate::scene::{Point, Rect, Size};

/// Time constant of wheel momentum: a wheel step's distance is covered exponentially, 95% of it
/// in three of these.
const WHEEL_TAU: f32 = 0.05;

/// A press and release on the same element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Click {
  pub id: Id,
  pub button: MouseButton,
  /// 2 for a double click, as the system counts them.
  pub clicks: u32,
  pub pos: Point,
  /// Held when the button went down (Shift-click, Cmd-click).
  pub mods: Mods,
}

/// A button held down since it was pressed on `id`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Press {
  pub id: Id,
  pub button: MouseButton,
  pub clicks: u32,
  /// Where it went down: drags measure from here.
  pub pos: Point,
  pub mods: Mods,
}

/// A scroll container's position, for scrollbars: `offset` of `content` shown in `viewport`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scroll {
  pub offset: Point,
  pub content: Size,
  pub viewport: Rect,
}

impl Scroll {
  pub fn max(&self) -> Point {
    let x = (self.content.w - self.viewport.w).max(0.0);
    Point::new(x, (self.content.h - self.viewport.h).max(0.0))
  }
}

/// A scroll container's state in the store.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ScrollState {
  pub offset: Point,
  /// Wheel distance still to cover.
  pub pending: Point,
  /// When `pending` last moved.
  pub t: Duration,
}

/// A key, text or command being delivered to an element in a view pass.
pub(crate) struct Deliver {
  pub target: Id,
  pub ev: Event,
  pub used: Cell<bool>,
  /// The view read it, even if it gave it back: what it did may show.
  pub seen: Cell<bool>,
}

#[derive(Default)]
pub(crate) struct Input {
  /// Interactive elements under the pointer, innermost first.
  pub hover: Vec<Id>,
  pub press: Option<Press>,
  /// A click not yet seen by the app's view.
  pub click: Option<Click>,
  pub focus: Option<Id>,
  /// The window is not the key window.
  pub blurred: bool,
  pub deliver: Option<Deliver>,
  /// An element shown pressed until then, though no button is down on it ([UiState::flash]).
  pub flash: Option<(Id, Duration)>,
  /// The time of the view being built, for the flash.
  pub now: Duration,
  /// Scroll containers of the trackpad gesture in progress, innermost first.
  latch: Vec<Id>,
  /// Scroll containers with wheel distance left.
  pub moving: Vec<Id>,
  scratch: Vec<Id>,
}

// MARK: Hit testing

/// The interactive elements under `p`, innermost first: the topmost hit (overlays from the top
/// by z, then the tree back to front, inside every clip on the way) and its interactive
/// ancestors.
pub fn hit(t: &Tree, p: Point, out: &mut Vec<Id>) {
  out.clear();
  let Some(mut i) = top(t, p) else { return };
  loop {
    let n = &t.nodes[i as usize];
    if n.style.hit {
      out.push(n.id);
    }

    if i == 0 {
      return;
    }

    i = n.parent;
  }
}

/// The topmost interactive node under `p`: overlays from the top by z, then the tree; none
/// outside the window (a release after a drag out of it), where nothing shows.
fn top(t: &Tree, p: Point) -> Option<u32> {
  if t.nodes.len() <= 1 || !t.nodes[0].rect.contains(p) {
    return None;
  }

  t.floats
    .iter()
    .rev()
    .filter(|&&f| t.float_clip(f).is_none_or(|c| c.contains(p)))
    .find_map(|&f| top_in(t, f, p))
    .or_else(|| top_in(t, 0, p))
}

/// The scroll containers a wheel at `p` moves, innermost first: those around the topmost
/// interactive node, up to an overlay placed on the window (a menu), whose ancestors are not
/// under it.
fn scroll_chain(t: &Tree, p: Point, out: &mut Vec<Id>) {
  out.clear();
  let Some(mut i) = top(t, p) else { return };
  loop {
    let n = &t.nodes[i as usize];
    if scrolls(n) {
      out.push(n.id);
    }

    if i == 0 || n.float.is_some_and(|f| f.window) {
      return;
    }

    i = n.parent;
  }
}

/// The last interactive node in paint order under `p` in the subtree at `root`, leaving out
/// overlays nested in it.
fn top_in(t: &Tree, root: u32, p: Point) -> Option<u32> {
  let end = t.nodes[root as usize].end;
  let (a, b) = (t.hits.partition_point(|&i| i < root), t.hits.partition_point(|&i| i < end));
  t.hits[a..b]
    .iter()
    .rev()
    .copied()
    .find(|&i| t.nodes[i as usize].rect.contains(p) && reaches(t, i, root, p))
}

/// Whether `p` on node `i` gets through every clip up to `root`, with no overlay between.
fn reaches(t: &Tree, i: u32, root: u32, p: Point) -> bool {
  let mut k = i;
  while k != root {
    let n = &t.nodes[k as usize];
    if n.float.is_some() {
      return false;
    }

    k = n.parent;
    let a = &t.nodes[k as usize];
    if a.style.clip && !a.rect.contains(p) {
      return false;
    }
  }

  true
}

fn scrolls(n: &Node) -> bool {
  n.style.scroll[0] || n.style.scroll[1]
}

/// How far a scroll container can scroll on each axis.
fn max_scroll(n: &Node) -> [f32; 2] {
  let m = |ax: usize, view: f32| match n.style.scroll[ax] {
    true => (n.content[ax] - view).max(0.0),
    false => 0.0,
  };
  [m(0, n.rect.w), m(1, n.rect.h)]
}

// MARK: Routing

impl UiState {
  /// Routes a pointer event through the last frame's tree: hover, press, click, focus and
  /// scrolling. Returns whether anything a view can show changed.
  pub(crate) fn pointer(&mut self, ev: &Event, now: Duration) -> bool {
    match *ev {
      // A move with a button held is a drag, which the view follows.
      Event::MouseMove { pos, .. } => self.hover_at(Some(pos), false) | self.input.press.is_some(),
      Event::Magnify { pos, .. } => self.hover_at(Some(pos), false),
      Event::MouseExit => self.hover_at(None, false),
      Event::MouseDown { pos, button, clicks, mods } => {
        let mut changed = self.hover_at(Some(pos), false);
        let top = self.input.hover.first().copied();
        self.input.press = top.map(|id| Press { id, button, clicks, pos, mods });
        if button == MouseButton::Left {
          // The innermost focusable element under the pointer takes focus, unless it takes it
          // by Tab only; none clears it.
          let last = &self.last;
          let at = self.input.hover.iter().find_map(|&id| {
            last.get(id).filter(|n| n.style.focus != Focus::None).map(|n| (id, n.style.focus))
          });
          let focus = match at {
            Some((_, Focus::Tab | Focus::TabQuiet)) => self.input.focus,
            at => at.map(|(id, _)| id),
          };

          changed |= focus != self.input.focus;
          self.input.focus = focus;
        }

        changed | top.is_some()
      }
      Event::MouseUp { pos, button, .. } => {
        let changed = self.hover_at(Some(pos), false);
        let Some(p) = self.input.press.filter(|p| p.button == button) else { return changed };
        self.input.press = None;
        if self.input.hover.contains(&p.id) {
          let (clicks, mods) = (p.clicks, p.mods);
          self.input.click = Some(Click { id: p.id, button, clicks, pos, mods });
        }

        true
      }
      Event::Scroll { pos, delta, precise, phase, momentum, .. } => {
        let changed = self.hover_at(Some(pos), false);
        self.scroll_by(pos, delta, precise, phase, momentum, now) | changed
      }
      Event::Focus(key) => {
        let changed = self.input.blurred == key;
        self.input.blurred = !key;
        // A press ends with the window's focus: its release may go to another window.
        let dropped = !key && self.input.press.take().is_some();
        changed | dropped
      }
      _ => false,
    }
  }

  /// Hit tests `p` on the last tree, or on the one just laid out (`fresh`); returns whether
  /// the hover changed.
  pub(crate) fn hover_at(&mut self, p: Option<Point>, fresh: bool) -> bool {
    let t = if fresh { &self.tree } else { &self.last };
    let scratch = &mut self.input.scratch;
    match p {
      Some(p) => hit(t, p, scratch),
      None => scratch.clear(),
    }

    if *scratch == self.input.hover {
      return false;
    }

    std::mem::swap(&mut self.input.scratch, &mut self.input.hover);
    true
  }

  /// Scrolls the containers under `pos`, innermost first, each passing on what it can't take.
  /// A trackpad gesture stays with the containers it began over, through its momentum; a mouse
  /// wheel gets momentum of its own, run by [Self::tick].
  fn scroll_by(
    &mut self,
    pos: Point,
    delta: Point,
    precise: bool,
    phase: Phase,
    momentum: Phase,
    now: Duration,
  ) -> bool {
    let gesture = matches!(phase, Phase::Changed | Phase::Ended) || momentum != Phase::None;
    if !(precise && gesture) || self.input.latch.is_empty() {
      scroll_chain(&self.last, pos, &mut self.input.latch);
    }

    // What the offsets should move by: content moving down scrolls back.
    let mut want = [-delta.x, -delta.y];
    let mut changed = false;
    for &id in &self.input.latch {
      let Some(n) = self.last.get(id) else { continue };
      let max = max_scroll(n);
      // A mouse wheel over a horizontal strip scrolls it sideways.
      let sideways = !precise && n.style.scroll == [true, false] && want[0] == 0.0;
      let src = if sideways { [1, 1] } else { [0, 1] };
      let s = self.store.raw::<ScrollState>(id);
      let (mut off, mut pend) = ([s.offset.x, s.offset.y], [s.pending.x, s.pending.y]);
      for ax in 0..2 {
        let w = want[src[ax]];
        if !n.style.scroll[ax] || w == 0.0 {
          continue;
        }

        // A trackpad takes over from the wheel's momentum, from where the content is.
        if precise {
          pend[ax] = 0.0;
        }

        let from = off[ax] + pend[ax];
        let used = (from + w).clamp(0.0, max[ax]) - from;
        if used == 0.0 {
          continue;
        }

        match precise {
          true => off[ax] += used,
          false => pend[ax] += used,
        }

        want[src[ax]] -= used;
        changed = true;
      }

      if !precise && pend != [s.pending.x, s.pending.y] {
        if s.pending == Point::default() {
          s.t = now;
        }

        if !self.input.moving.contains(&id) {
          self.input.moving.push(id);
        }
      }

      (s.offset, s.pending) = (Point::new(off[0], off[1]), Point::new(pend[0], pend[1]));
    }

    changed
  }

  /// Moves wheel momentum on to `now`.
  pub(crate) fn tick(&mut self, now: Duration) {
    let mut moving = std::mem::take(&mut self.input.moving);
    moving.retain(|&id| {
      let Some(n) = self.last.get(id) else { return false };
      let max = max_scroll(n);
      let s = self.store.raw::<ScrollState>(id);
      let k = 1.0 - (-now.saturating_sub(s.t).as_secs_f32() / WHEEL_TAU).exp();
      s.t = now;
      let (mut off, mut pend) = ([s.offset.x, s.offset.y], [s.pending.x, s.pending.y]);
      for ax in 0..2 {
        // The last half point goes in one step.
        let step = if (pend[ax] * (1.0 - k)).abs() < 0.5 { pend[ax] } else { pend[ax] * k };
        let to = (off[ax] + step).clamp(0.0, max[ax]);
        pend[ax] = if to == off[ax] + step { pend[ax] - step } else { 0.0 };
        off[ax] = to;
      }

      (s.offset, s.pending) = (Point::new(off[0], off[1]), Point::new(pend[0], pend[1]));
      pend != [0.0; 2]
    });
    self.input.moving = moving;
  }

  /// A scroll container's offset, content and viewport, as of the last frame and the input
  /// since.
  pub fn scroll(&self, id: Id) -> Option<Scroll> {
    let n = self.last.get(id).filter(|n| scrolls(n))?;
    let off = self.store.peek::<ScrollState>(id).map_or(n.scroll, |s| s.offset);
    let max = max_scroll(n);
    let offset = Point::new(off.x.clamp(0.0, max[0]), off.y.clamp(0.0, max[1]));
    Some(Scroll { offset, content: Size::new(n.content[0], n.content[1]), viewport: n.rect })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::ui::layout::tests::Fake;
  use crate::ui::{Float, Style, Ui, px};

  /// A frame of a 200 x 100 window built by `f`.
  fn frame(st: UiState, f: impl FnOnce(&mut Ui)) -> UiState {
    let mut ui = Ui::new(st, Size::new(200.0, 100.0), None, Duration::ZERO);
    f(&mut ui);
    ui.layout(&mut Fake, 1.0);
    ui.finish()
  }

  fn at(st: &UiState, x: f32, y: f32) -> Vec<Id> {
    let mut out = Vec::new();
    hit(&st.last, Point::new(x, y), &mut out);
    out
  }

  fn ev(st: &mut UiState, e: Event) -> bool {
    st.pointer(&e, Duration::ZERO)
  }

  fn down(x: f32, y: f32) -> Event {
    Event::MouseDown {
      pos: Point::new(x, y),
      button: MouseButton::Left,
      clicks: 1,
      mods: Mods::NONE,
    }
  }

  fn up(x: f32, y: f32) -> Event {
    Event::MouseUp { pos: Point::new(x, y), button: MouseButton::Left, mods: Mods::NONE }
  }

  fn wheel(x: f32, y: f32, dx: f32, dy: f32, precise: bool, phase: Phase) -> Event {
    let (pos, delta) = (Point::new(x, y), Point::new(dx, dy));
    Event::Scroll { pos, delta, precise, phase, momentum: Phase::None, mods: Mods::NONE }
  }

  #[test]
  fn autofocus_takes_focus_when_an_element_first_shows() {
    let field = Style::new().w(px(50.0)).h(px(20.0)).focusable();
    let (a, b) = (Id::of("a"), Id::of("b"));
    let st = frame(UiState::default(), |ui| _ = ui.id("a").autofocus().el(field));
    assert_eq!(st.focus(), Some(a));
    // Built again it doesn't take focus back; a new one does, and the view sees it at once.
    let mut st = frame(st, |ui| _ = ui.id("a").autofocus().el(field));
    st.set_focus(None);
    let mut seen = false;
    let st = frame(st, |ui| {
      ui.id("a").autofocus().el(field);
      ui.autofocus().take_autofocus(b);
      seen = ui.focused(b);
      ui.with_id(b).el(field);
    });
    assert_eq!((st.focus(), seen), (Some(b), true));
    // The flag goes with the element after it, not further.
    let st = frame(UiState::default(), |ui| {
      ui.autofocus().el(Style::new());
      ui.id("a").el(field);
    });
    assert_eq!(st.focus(), None);
    // A disabled widget doesn't take it, named before its behaviour as a button is: the focus
    // stays where it was.
    let mut st = frame(UiState::default(), |ui| _ = ui.id("a").el(field));
    st.set_focus(Some(a));
    let st = frame(st, |ui| {
      ui.id("a").el(field);
      let id = ui.autofocus().disabled(true).id("b").take_id();
      let off = ui.button_behavior(id);
      ui.with_id(id).el(off.style(field));
    });
    assert_eq!(st.focus(), Some(a));
  }

  #[test]
  fn losing_the_window_focus_ends_a_press() {
    let b = Id::of("b");
    let mut st =
      frame(UiState::default(), |ui| _ = ui.id("b").el(Style::new().w(px(50.0)).h(px(20.0))));
    ev(&mut st, down(10.0, 10.0));
    assert!(st.held(b));
    assert!(ev(&mut st, Event::Focus(false)));
    assert!(!st.held(b));
    // Back, a release that comes after is no click.
    ev(&mut st, Event::Focus(true));
    ev(&mut st, up(10.0, 10.0));
    assert!(st.input.click.is_none());
  }

  #[test]
  fn hit_respects_order_clips_and_opt_in() {
    let box_ = |w: f32, h: f32| Style::new().w(px(w)).h(px(h));
    let st = frame(UiState::default(), |ui| {
      ui.id("panel").row(box_(100.0, 50.0).clip(), |ui| {
        ui.id("a").el(box_(60.0, 50.0));
        // Overflows the panel's clip; decoration on top of `a` doesn't take the pointer.
        ui.id("b").row(box_(60.0, 50.0), |ui| ui.el(box_(60.0, 50.0).bg([1.0; 4])));
        ui.float(Float::parent().offset(10.0, 10.0)).el(box_(20.0, 20.0).bg([1.0; 4]));
      });
      // An overlay with a hit area, over `a`.
      ui.id("pop").float(Float::window().offset(40.0, 0.0)).el(box_(10.0, 10.0));
    });
    let (panel, a, b, pop) = (Id::of("panel"), Id::of("a"), Id::of("b"), Id::of("pop"));
    assert_eq!(at(&st, 15.0, 15.0), [a, panel]);
    assert_eq!(at(&st, 70.0, 5.0), [b, panel]);
    // `b` reaches 120, the panel clips it at 100.
    assert_eq!(at(&st, 110.0, 5.0), []);
    assert_eq!(at(&st, 45.0, 5.0), [pop]);
    assert_eq!(at(&st, 45.0, 15.0), [a, panel]);
  }

  #[test]
  fn nothing_hits_outside_the_window() {
    // A button and an overlay running past the 200-wide window's right edge.
    let mut st = frame(UiState::default(), |ui| {
      ui.id("b").el(Style::new().w(px(300.0)).h(px(20.0)));
      ui.id("pop")
        .float(Float::window().offset(190.0, 50.0))
        .el(Style::new().w(px(50.0)).h(px(20.0)));
    });
    assert_eq!(st.last.rect(Id::of("b")).unwrap().w, 300.0);
    assert_eq!(
      (at(&st, 195.0, 5.0), at(&st, 195.0, 55.0)),
      (vec![Id::of("b")], vec![Id::of("pop")])
    );
    assert_eq!((at(&st, 220.0, 5.0), at(&st, 220.0, 55.0)), (vec![], vec![]));
    // Pressed inside, released outside (a captured release): no click.
    ev(&mut st, down(195.0, 5.0));
    ev(&mut st, up(220.0, 5.0));
    assert_eq!(st.input.click, None);
  }

  #[test]
  fn press_click_and_focus() {
    let mut st = frame(UiState::default(), |ui| {
      ui.id("row").row(Style::new().w(px(100.0)).h(px(40.0)).focusable(), |ui| {
        ui.id("btn").el(Style::new().w(px(20.0)).h(px(20.0)));
      });
    });
    let (row, btn) = (Id::of("row"), Id::of("btn"));
    assert!(ev(&mut st, Event::MouseMove { pos: Point::new(5.0, 5.0), mods: Mods::NONE }));
    assert_eq!(st.input.hover, [btn, row]);
    assert!(ev(&mut st, down(5.0, 5.0)));
    assert!(st.pressed(btn) && !st.pressed(row));
    // The button is focused through its focusable row.
    assert_eq!(st.input.focus, Some(row));
    // Released elsewhere: no click; on it: one, to the innermost.
    ev(&mut st, up(50.0, 5.0));
    assert_eq!(st.input.click, None);
    ev(&mut st, down(5.0, 5.0));
    ev(&mut st, Event::MouseMove { pos: Point::new(50.0, 5.0), mods: Mods::NONE });
    assert!(st.held(btn) && !st.pressed(btn));
    ev(&mut st, up(6.0, 6.0));
    assert_eq!(st.input.click.map(|c| (c.id, c.pos)), Some((btn, Point::new(6.0, 6.0))));
    // Outside: hover clears, focus goes.
    ev(&mut st, down(150.0, 80.0));
    assert_eq!((st.input.hover.len(), st.input.focus), (0, None));
  }

  #[test]
  fn tab_focusable_keeps_focus_on_click() {
    let mut st = frame(UiState::default(), |ui| {
      ui.id("field").el(Style::new().w(px(50.0)).h(px(20.0)).focusable());
      ui.id("btn").el(Style::new().w(px(50.0)).h(px(20.0)).tab_focusable());
    });
    let (field, btn) = (Id::of("field"), Id::of("btn"));
    ev(&mut st, down(5.0, 5.0));
    assert_eq!(st.focus(), Some(field));
    // A click on the button leaves the field focused; Tab reaches the button.
    ev(&mut st, down(5.0, 25.0));
    assert_eq!(st.focus(), Some(field));
    st.focus_next(true);
    assert_eq!(st.focus(), Some(btn));
  }

  #[test]
  fn clipped_overlay_takes_the_pointer_only_inside_the_clips() {
    let st = frame(UiState::default(), |ui| {
      ui.row(Style::new().w(px(50.0)).h(px(50.0)).clip(), |ui| {
        ui.el(Style::new().w(px(100.0)).h(px(100.0)));
        let at = Float::parent().offset(30.0, 0.0);
        ui.id("clipped").float(at.clip()).el(Style::new().w(px(40.0)).h(px(10.0)));
        ui.id("free").float(at.offset(30.0, 20.0)).el(Style::new().w(px(40.0)).h(px(10.0)));
      });
    });
    assert_eq!(at(&st, 40.0, 5.0), [Id::of("clipped")]);
    assert_eq!(at(&st, 60.0, 5.0), []);
    assert_eq!(at(&st, 60.0, 25.0), [Id::of("free")]);
    assert_eq!(st.last.float_clip(st.last.find(Id::of("clipped")).unwrap()).unwrap().w, 50.0);
  }

  /// A 100-high column scrolling 10 rows of 30 (300 content), with an inner 50-high one
  /// scrolling 4 rows of 25 (100 content) at its top.
  fn nested(st: UiState) -> UiState {
    frame(st, |ui| {
      ui.id("outer").col(Style::new().w(px(100.0)).h(px(100.0)).scroll_y(), |ui| {
        ui.id("inner").col(Style::new().w(px(100.0)).h(px(50.0)).scroll_y(), |ui| {
          for _ in 0..4 {
            ui.el(Style::new().w(px(100.0)).h(px(25.0)));
          }
        });
        for _ in 0..8 {
          ui.el(Style::new().w(px(100.0)).h(px(30.0)));
        }
      });
    })
  }

  fn offset(st: &UiState, name: &str) -> f32 {
    st.scroll(Id::of(name)).unwrap().offset.y
  }

  #[test]
  fn scroll_chains_inner_then_outer() {
    let mut st = nested(UiState::default());
    let outer = st.scroll(Id::of("outer")).unwrap();
    assert_eq!((outer.content, outer.max()), (Size::new(100.0, 290.0), Point::new(0.0, 190.0)));
    // Children keep their size along the scroll axis: no shrinking to fit.
    assert_eq!(st.last.rect(Id::of("inner")).unwrap().h, 50.0);

    // Over the inner column: it takes all 40 (its max is 50).
    assert!(ev(&mut st, wheel(10.0, 10.0, 0.0, -40.0, true, Phase::None)));
    assert_eq!((offset(&st, "inner"), offset(&st, "outer")), (40.0, 0.0));
    // 10 more to its limit, 30 to the outer.
    ev(&mut st, wheel(10.0, 10.0, 0.0, -40.0, true, Phase::None));
    assert_eq!((offset(&st, "inner"), offset(&st, "outer")), (50.0, 30.0));
    // The layout applies the offsets: the inner column moved up 30.
    let st2 = nested(std::mem::take(&mut st));
    assert_eq!(st2.last.rect(Id::of("inner")).unwrap().y, -30.0);
    st = st2;
    // Past the end: clamped, and at the limit nothing changes; then back to the top.
    ev(&mut st, wheel(10.0, 60.0, 0.0, -1000.0, true, Phase::None));
    assert_eq!(offset(&st, "outer"), 190.0);
    assert!(!ev(&mut st, wheel(10.0, 60.0, 0.0, -10.0, true, Phase::None)));
    st = nested(st);
    ev(&mut st, wheel(10.0, 60.0, 0.0, 1000.0, true, Phase::None));
    assert_eq!(offset(&st, "outer"), 0.0);
  }

  #[test]
  fn trackpad_gesture_stays_latched() {
    let mut st = nested(UiState::default());
    // Begins over the inner column: the whole gesture goes there first, even once the pointer
    // is over the outer one.
    ev(&mut st, wheel(10.0, 10.0, 0.0, 0.0, true, Phase::Began));
    ev(&mut st, wheel(10.0, 80.0, 0.0, -20.0, true, Phase::Changed));
    assert_eq!((offset(&st, "inner"), offset(&st, "outer")), (20.0, 0.0));
    // A new gesture over the outer one.
    ev(&mut st, wheel(10.0, 80.0, 0.0, 0.0, true, Phase::Began));
    ev(&mut st, wheel(10.0, 80.0, 0.0, -20.0, true, Phase::Changed));
    assert_eq!((offset(&st, "inner"), offset(&st, "outer")), (20.0, 20.0));
  }

  #[test]
  fn a_trackpad_takes_over_from_wheel_momentum() {
    let mut st = nested(UiState::default());
    let outer = Id::of("outer");
    // Below the inner column: 10 by trackpad, then 50 by the wheel, still to run.
    ev(&mut st, wheel(10.0, 80.0, 0.0, -10.0, true, Phase::None));
    ev(&mut st, wheel(10.0, 80.0, 0.0, -50.0, false, Phase::None));
    // Back 20 by trackpad: from where the content is, down to the top, the wheel's rest gone.
    ev(&mut st, wheel(10.0, 80.0, 0.0, 20.0, true, Phase::None));
    let s = st.store.peek::<ScrollState>(outer).unwrap();
    assert_eq!((s.offset.y, s.pending.y), (0.0, 0.0));
    st.tick(Duration::from_secs(1));
    assert_eq!(offset(&st, "outer"), 0.0);
  }

  #[test]
  fn a_grid_scrolls() {
    let grid = |st| {
      frame(st, |ui| {
        ui.id("g").grid(2, Style::new().w(px(100.0)).h(px(50.0)).scroll_y(), |ui| {
          for k in 0..6 {
            ui.id(("c", k)).el(Style::new().w(px(50.0)).h(px(20.0)));
          }
        });
      })
    };
    // Three rows of 20 in 50: 10 to scroll, past it clamped.
    let mut st = grid(UiState::default());
    assert!(ev(&mut st, wheel(10.0, 10.0, 0.0, -30.0, true, Phase::None)));
    let st = grid(st);
    assert_eq!(offset(&st, "g"), 10.0);
    assert_eq!(st.last.rect(Id::of(("c", 0))).unwrap().y, -10.0);
    assert_eq!(st.last.rect(Id::of(("c", 5))).unwrap().y, 30.0);
  }

  #[test]
  fn wheel_momentum_runs_on_ticks() {
    let mut st = nested(UiState::default());
    ev(&mut st, wheel(10.0, 80.0, 0.0, -36.0, false, Phase::None));
    assert_eq!(offset(&st, "outer"), 0.0);
    assert_eq!(st.input.moving, [Id::of("outer")]);
    let mut last = 0.0;
    for k in 1..=60 {
      st.tick(Duration::from_millis(k * 8));
      let y = offset(&st, "outer");
      assert!(y >= last, "{y} < {last}");
      last = y;
    }

    assert!((last - 36.0).abs() < 1e-3 && st.input.moving.is_empty(), "{last}");
    // A horizontal strip scrolls sideways under a wheel.
    let mut st = frame(UiState::default(), |ui| {
      ui.id("strip").row(Style::new().w(px(100.0)).h(px(20.0)).scroll_x(), |ui| {
        ui.el(Style::new().w(px(300.0)).h(px(20.0)));
      });
    });
    ev(&mut st, wheel(10.0, 10.0, 0.0, -50.0, false, Phase::None));
    st.tick(Duration::from_secs(1));
    assert_eq!(st.scroll(Id::of("strip")).unwrap().offset.x, 50.0);
  }

  #[test]
  fn hidden_tab_keeps_its_scroll_offset_while_retained() {
    let tab = |show: bool, keep: bool| {
      move |ui: &mut Ui| {
        if keep {
          ui.retain(Id::of("tab"));
        }

        if show {
          ui.id("tab").col(Style::new(), |ui| {
            ui.id("list").col(Style::new().w(px(50.0)).h(px(50.0)).scroll_y(), |ui| {
              ui.el(Style::new().w(px(50.0)).h(px(500.0)));
            });
          });
        }
      }
    };
    let mut st = frame(UiState::default(), tab(true, false));
    ev(&mut st, wheel(10.0, 10.0, 0.0, -70.0, true, Phase::None));
    // Hidden two frames but retained: the offset is there when it shows again.
    st = frame(st, tab(false, true));
    st = frame(st, tab(false, true));
    st = frame(st, tab(true, false));
    assert_eq!(offset(&st, "list"), 70.0);
    // Hidden without retaining: gone.
    st = frame(st, tab(false, false));
    st = frame(st, tab(true, false));
    assert_eq!(offset(&st, "list"), 0.0);
  }

  #[test]
  fn tab_focus_reveals_the_focused_element() {
    let list = |ui: &mut Ui| {
      ui.id("list").col(Style::new().w(px(50.0)).h(px(100.0)).scroll_y(), |ui| {
        for i in 0..10 {
          ui.id(("item", i)).el(Style::new().w(px(50.0)).h(px(30.0)).focusable());
        }
      });
    };
    let mut st = frame(UiState::default(), list);
    for _ in 0..5 {
      st.focus_next(true);
    }
    // The fifth item, 120..150, shows at the bottom of the 100 high list.
    assert_eq!(st.focus(), Some(Id::of(("item", 4))));
    st = frame(st, list);
    assert_eq!(offset(&st, "list"), 50.0);
    st.focus_next(false);
    st.focus_next(false);
    st.focus_next(false);
    st = frame(st, list);
    assert_eq!(offset(&st, "list"), 30.0);
  }

  #[test]
  fn reveal_scrolls_the_least() {
    let list = |ui: &mut Ui| {
      ui.id("list").col(Style::new().w(px(50.0)).h(px(100.0)).scroll_y(), |ui| {
        ui.el(Style::new().w(px(50.0)).h(px(1000.0)));
      });
    };
    let id = Id::of("list");
    let mut st = frame(UiState::default(), list);
    for (r, want) in [((300.0, 20.0), 220.0), ((250.0, 20.0), 220.0), ((100.0, 20.0), 100.0)] {
      st = frame(st, |ui| {
        ui.reveal(id, Rect::new(0.0, r.0, 10.0, r.1));
        list(ui);
      });
      assert_eq!(offset(&st, "list"), want, "{r:?}");
    }
  }
}
