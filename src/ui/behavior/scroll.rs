// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A scroll area's scrollbars' behaviour ([Ui::scroll_behavior]): overlay bars as macOS has
//! them, shown while the content moves or the pointer comes in, then fading out; a thumb
//! dragged, a track clicked to page. [Ui::scroll_area] draws them.

use std::time::Duration;

use crate::app::MouseButton;
use crate::scene::Point;
use crate::ui::{Axes, Id, Scroll, Transition, Ui};

/// How long scrollbars stay after the content last moved or the pointer came in.
const BAR_SHOW: Duration = Duration::from_millis(1000);
/// A thumb's least length.
pub(super) const MIN_THUMB: f32 = 24.0;

/// A scroll area's scrollbars between frames.
#[derive(Default)]
pub(super) struct Bars {
  /// The offset last frame: a change shows the bars.
  seen: Point,
  /// When they were last asked to show.
  pub shown: Option<Duration>,
  /// The pointer was over the area last frame, and where.
  over: bool,
  mouse: Option<Point>,
  /// The offset when a thumb drag began, per axis, in `f64` as the layout keeps it.
  drag: [Option<f64>; 2],
}

/// A scroll area's bar along one axis ([ScrollBehavior::bars]): its lengths along it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollBar {
  pub track: f32,
  pub thumb: f32,
  /// The thumb's start from the track's.
  pub pos: f32,
  /// Content points per track point.
  pub ratio: f32,
}

impl ScrollBar {
  /// A bar's breadth across its axis: the thumb and the room to grab it, the strip where the
  /// pointer shows the bars.
  pub const BREADTH: f32 = 12.0;
  /// The gap between the track's ends and the viewport's edges.
  pub const INSET: f32 = 2.0;
}

/// What a scroll area's bars do this frame ([Ui::scroll_behavior]): each axis's bar when its
/// content overflows, 0 for horizontal.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollBehavior {
  pub bars: [Option<ScrollBar>; 2],
  /// How far they have faded in, 0 to 1.
  pub fade: f32,
  /// Shown: they take clicks (the thumbs `id.key(("thumb", axis))`, the tracks
  /// `id.key(("track", axis))`); fading out, they draw but take none.
  pub shown: bool,
  /// The thumb being dragged, per axis.
  pub drag: [bool; 2],
}

fn axis(p: Point, ax: usize) -> f32 {
  if ax == 0 { p.x } else { p.y }
}

fn with_axis(mut p: Point, ax: usize, v: f32) -> Point {
  match ax {
    0 => p.x = v,
    _ => p.y = v,
  }

  p
}

/// The bar of axis `ax` at `off`, when the content overflows; `both` when the other axis has
/// one too, which takes the corner.
fn geo(sc: &Scroll, ax: usize, off: f32, both: bool) -> Option<ScrollBar> {
  let (view, content) = match ax {
    0 => (sc.viewport.w, sc.content.w),
    _ => (sc.viewport.h, sc.content.h),
  };

  if content <= view + 0.5 {
    return None;
  }

  let track = view - 2.0 * ScrollBar::INSET - if both { ScrollBar::BREADTH } else { 0.0 };
  let thumb = (track * view / content).max(MIN_THUMB).min(track);
  let (max, travel) = (content - view, (track - thumb).max(1.0));
  Some(ScrollBar { track, thumb, pos: (off / max).clamp(0.0, 1.0) * travel, ratio: max / travel })
}

impl Ui<'_> {
  /// The scrollbars' behaviour of scroll area `id` scrolling along `axes`: a thumb dragged from
  /// where it was pressed, a click on the track paging toward it, the bars shown while the
  /// content moves, the pointer comes in or moves in a track's strip, then fading out; they
  /// stay while a thumb is held or the pointer is on a track. Read before the area is built, as
  /// of the last frame's content and viewport.
  pub fn scroll_behavior(&mut self, id: Id, axes: Axes) -> ScrollBehavior {
    let now = self.now();
    let mut bars = std::mem::take(self.state::<Bars>(id));
    let mut geos: [Option<ScrollBar>; 2] = [None, None];
    // A thumb held or a track under the pointer keep the bars, with no timer running.
    let mut pinned = false;
    let mouse = self.mouse();
    if let Some(sc) = self.scroll(id) {
      let over = self.hovered(id);
      if sc.offset != bars.seen || over && !bars.over {
        bars.shown = Some(now);
      }

      (bars.seen, bars.over) = (sc.offset, over);
      // Moved in `f64` and written back only when a drag or a click moved it, unclamped: the
      // layout clamps it exactly, the bars draw it in `f32`.
      let exact = self.st.scroll_exact(id).map_or([0.0; 2], |(o, _)| o);
      let (mut off, mut ex, mut moved) = (sc.offset, exact, false);
      let scrolls = |ax: usize| axes.at(ax) && geo(&sc, ax, 0.0, false).is_some();
      let both = scrolls(0) && scrolls(1);
      for (ax, g) in geos.iter_mut().enumerate() {
        if !axes.at(ax) {
          continue;
        }

        let Some(first) = geo(&sc, ax, axis(off, ax), both) else { continue };
        let (thumb, track) = (id.key(("thumb", ax)), id.key(("track", ax)));
        // Held, or released since the view last ran: its last move counts.
        let held = self.held(thumb);
        let press = self.press().zip(mouse).filter(|_| held).or(self.released(thumb));
        if let Some((p, m)) = press {
          let start = *bars.drag[ax].get_or_insert(ex[ax]);
          ex[ax] = start + f64::from((axis(m, ax) - axis(p.pos, ax)) * first.ratio);
          moved = true;
        }

        if !held {
          bars.drag[ax] = None;
        }

        pinned |= held;

        if let Some(c) = self.click(track).filter(|c| c.button == MouseButton::Left) {
          let start =
            axis(Point::new(sc.viewport.x, sc.viewport.y), ax) + ScrollBar::INSET + first.pos;
          let page = axis(Point::new(sc.viewport.w, sc.viewport.h), ax) * 0.9;
          let dir = if axis(c.pos, ax) < start { -1.0 } else { 1.0 };
          ex[ax] += f64::from(dir * page);
          moved = true;
        }

        // The pointer moving in the track's strip shows the bars, though hidden ones take no
        // clicks.
        let vp = sc.viewport;
        let strip = |m: Point| match ax {
          0 => m.y >= vp.y + vp.h - ScrollBar::BREADTH,
          _ => m.x >= vp.x + vp.w - ScrollBar::BREADTH,
        };
        if over && mouse != bars.mouse && mouse.is_some_and(strip) {
          bars.shown = Some(now);
        }

        pinned |= self.hovered(track);
        off = with_axis(off, ax, ex[ax] as f32);
        let max = sc.max();
        off = Point::new(off.x.clamp(0.0, max.x), off.y.clamp(0.0, max.y));
        *g = geo(&sc, ax, axis(off, ax), both);
      }

      if moved && ex != exact {
        self.scroll_exact(id, ex);
        bars.seen = off;
      }
    }

    bars.mouse = mouse;
    if pinned {
      bars.shown = Some(now);
    }

    // Pinned, they wait for the pointer to leave, a frame of its own (the hover changes).
    let until = bars.shown.map(|s| s + BAR_SHOW).filter(|&u| u > now);
    if let Some(u) = until.filter(|_| !pinned) {
      self.request_frame_in(u - now);
    }

    let fade = self.animate(id.key("bars"), until.is_some() as u8 as f32, Transition::linear(5.0));
    let drag = bars.drag.map(|d| d.is_some());
    *self.state::<Bars>(id) = bars;
    ScrollBehavior { bars: geos, fade, shown: until.is_some(), drag }
  }
}
