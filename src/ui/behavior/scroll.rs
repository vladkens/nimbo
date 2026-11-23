// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The scroll area ([Ui::scroll_area]): a container scrolling its children under overlay
//! scrollbars as macOS draws them, thin, shown while the content moves or the pointer comes in,
//! then fading out; a thumb dragged, a track clicked to page ([Ui::scroll_behavior], for an app
//! that draws its own); its content fading under its edges ([Ui::fade_edges]). Headless but
//! drawn: menus and lists scroll in it, so its knobs are the environment's ([ScrollerKnob]), by
//! default the appearance's.
//!
//! ```ignore
//! ui.fade_edges(t.bg, 24.0).scroll_area(Style::new().grow(), |ui| {
//!   for f in &self.files { ui.label(f) }
//! });
//! ```

use std::time::Duration;

use crate::app::MouseButton;
use crate::scene::{Color, Point, alpha};
use crate::ui::{
  Align, Axes, Axis, Float, Flow, Id, Kind, Response, Scroll, Style, Transition, Ui, UnitPoint,
  pct, px,
};

/// How long scrollbars stay after the content last moved or the pointer came in.
const BAR_SHOW: Duration = Duration::from_millis(1000);
/// A thumb's least length.
pub(super) const MIN_THUMB: f32 = 24.0;

/// How [Ui::scroll_area] draws its scrollers' knobs (the thumbs), an environment value
/// ([Ui::set_environment]), as AppKit's `NSScroller.knobStyle`: their colour, and how they
/// thicken and darken while the pointer is on a track. Without one, [Self::DARK] in the light
/// appearance, [Self::LIGHT] in the dark one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollerKnob {
  pub color: Color,
  pub hover: Transition,
}

impl ScrollerKnob {
  /// AppKit's `.dark`: a dark knob, for light content (the light appearance's).
  pub const DARK: Self = Self { color: alpha([0.0, 0.0, 0.0, 1.0], 0.24), hover: HOVER };
  /// AppKit's `.light`: a light knob, for dark content (the dark appearance's).
  pub const LIGHT: Self = Self { color: alpha([1.0; 4], 0.26), hover: HOVER };
}

/// The knobs' change on a track's hover, as the theme's hover fades ([crate::Theme::fade]).
const HOVER: Transition = Transition::smooth(6.0);

/// The fade the next scroll area draws under its edges ([Ui::fade_edges]): colour and length.
struct EdgeFade(Color, f32);

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

/// A scroll area's bar along one axis ([ScrollBehavior::bar]): its lengths along it.
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
/// content overflows ([Self::bar]), its thumb dragged or not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollBehavior {
  /// How far the bars have faded in, 0 to 1: the opacity to draw them at.
  pub opacity: f32,
  /// Shown: they take clicks ([Self::thumb], [Self::track]); fading out, they draw but take
  /// none.
  pub shown: bool,
  id: Id,
  bars: [Option<ScrollBar>; 2],
  drag: [bool; 2],
}

impl ScrollBehavior {
  /// The bar along `axis`, when the content overflows along it.
  pub fn bar(&self, axis: Axis) -> Option<ScrollBar> {
    self.bars[axis.index()]
  }

  /// Whether the thumb along `axis` is being dragged.
  pub fn dragged(&self, axis: Axis) -> bool {
    self.drag[axis.index()]
  }

  /// The id of the thumb along `axis`, for the element drawing it, which takes its drag.
  pub fn thumb(&self, axis: Axis) -> Id {
    self.id.key(("thumb", axis.index()))
  }

  /// The id of the track along `axis`, for the element under the thumb, which takes its clicks.
  pub fn track(&self, axis: Axis) -> Id {
    self.id.key(("track", axis.index()))
  }
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

    let opacity =
      self.animate(id.key("bars"), until.is_some() as u8 as f32, Transition::linear(5.0));
    let drag = bars.drag.map(|d| d.is_some());
    *self.state::<Bars>(id) = bars;
    ScrollBehavior { opacity, shown: until.is_some(), id, bars: geos, drag }
  }

  /// The next [Ui::scroll_area] fades its content out under its top and bottom edges, over `len`
  /// points into `color` (the ground it sits on), as CSS's `mask-image` does: an edge fades in
  /// as the content scrolls away from it, fully once it has moved `len`. Drawn under the
  /// scrollbars; takes no pointer input.
  pub fn fade_edges(&mut self, color: Color, len: f32) -> &mut Self {
    self.set_slot(Some(EdgeFade(color, len)));
    self
  }

  /// A container scrolling its children, with overlay scrollbars, their knobs the
  /// environment's ([ScrollerKnob]). `style` sets its size and padding, and the axes with
  /// [Style::scroll] (vertical by default), as SwiftUI's `ScrollView`. Its id is the scroll
  /// container's ([Ui::scroll], [Ui::scroll_to]); the thumbs and tracks are its behaviour's
  /// ([ScrollBehavior::thumb], [ScrollBehavior::track]).
  pub fn scroll_area<R>(&mut self, style: Style, f: impl FnOnce(&mut Self) -> R) -> Response {
    let knob = match self.environment::<ScrollerKnob>() {
      Some(&s) => s,
      None if self.appearance().scheme.is_dark() => ScrollerKnob::LIGHT,
      None => ScrollerKnob::DARK,
    };

    let id = self.take_id();
    let mask = self.set_slot::<EdgeFade>(None);
    let style = match style.scroll.is_empty() {
      true => style.scroll(Axes::VERTICAL),
      false => style.clip(),
    };

    let b = self.scroll_behavior(id, style.scroll);
    let (opacity, shown) = (b.opacity, b.shown);
    // Drawn from the last frame's content and viewport: again if the layout changes them.
    if opacity > 0.0 || shown {
      self.settle(id);
    }

    self.with_id(id).container(Flow::Column, style, |ui| {
      f(ui);
      // The fades over the content, under the scrollbars, out of the pointer's way: the paint
      // pass draws them by the offset the layout settled on.
      if let Some(EdgeFade(color, len)) = mask.filter(|_| style.scroll.at(1)) {
        let s = Style::new().w(pct(100.0)).h(pct(100.0));
        ui.float(Float::parent().clip()).push(Kind::Fade { color, len }, s, Flow::Row);
      }

      for (ax, axis) in [Axis::Horizontal, Axis::Vertical].into_iter().enumerate() {
        let Some(g) = b.bar(axis).filter(|_| opacity > 0.0 || shown) else { continue };
        let (thumb, track) = (b.thumb(axis), b.track(axis));
        let on = ui.hovered(track) || b.dragged(axis);
        let hot = ui.animate(track.key("hot"), on as u8 as f32, knob.hover);
        let thick = 6.0 + 2.0 * hot;
        let color = alpha(knob.color, knob.color[3] * (1.0 + 0.6 * hot));

        let (at, s, pad) = match ax {
          0 => {
            let at = Float::parent()
              .attach(UnitPoint::BOTTOM_LEADING)
              .anchor(UnitPoint::BOTTOM_LEADING)
              .offset(ScrollBar::INSET, 0.0);
            let s =
              Style::new().w(px(g.track)).h(px(ScrollBar::BREADTH)).align(Align::Start, Align::End);
            (at, s, [0.0, 0.0, 2.0, 0.0])
          }
          _ => {
            let at = Float::parent()
              .attach(UnitPoint::TOP_TRAILING)
              .anchor(UnitPoint::TOP_TRAILING)
              .offset(0.0, ScrollBar::INSET);
            let s =
              Style::new().w(px(ScrollBar::BREADTH)).h(px(g.track)).align(Align::End, Align::Start);
            (at, s, [0.0, 2.0, 0.0, 0.0])
          }
        };

        let (gap, bar) = match ax {
          0 => (Style::new().w(px(g.pos)), Style::new().w(px(g.thumb)).h(px(thick))),
          _ => (Style::new().h(px(g.pos)), Style::new().w(px(thick)).h(px(g.thumb))),
        };
        // Fading out they draw but take no clicks.
        if shown {
          ui.with_id(track);
        }

        let flow = if ax == 0 { Flow::Row } else { Flow::Column };
        ui.float(at.clip()).container(flow, s.pads(pad).opacity(opacity), |ui| {
          ui.el(gap);
          if shown {
            ui.with_id(thumb);
          }

          ui.el(bar.radius(thick / 2.0).bg(color));
        });
      }
    })
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::{Size, rgb};
  use crate::ui::input::ScrollState;
  use crate::ui::{Id, ScrollerKnob, Style, Ui, px};

  /// A scroll area 200 high over 1000 points of content, fading into red.
  struct Tall(f32);

  impl App for Tall {
    fn view(&mut self, ui: &mut Ui) {
      let s = Style::new().w(px(200.0)).h(px(200.0));
      ui.id("area").fade_edges(rgb(0xff0000), 20.0).scroll_area(s, |ui| {
        ui.el(Style::new().w(px(200.0)).h(px(self.0)).bg(rgb(0x0000ff)));
      });
    }
  }

  /// 53 million points of content under a 2-point prefix, past `f32`'s whole numbers: at the
  /// end, frames that draw the bars leave the offset exactly there.
  #[test]
  fn the_bars_keep_an_offset_past_f32() {
    struct Long;
    impl App for Long {
      fn view(&mut self, ui: &mut Ui) {
        ui.id("area").scroll_area(Style::new().w(px(200.0)).h(px(300.0)), |ui| {
          ui.el(Style::new().h(px(2.0)));
          ui.el(Style::new().h(px(52_998_000.0)));
          ui.el(Style::new().h(px(2_000.0)));
        });
      }
    }

    let mut h = Harness::new(Long, Size::new(200.0, 300.0), 2.0);
    let area = Id::of("area");
    h.frame();
    h.rt.ui.store.raw::<ScrollState>(area).offset[1] = 1e12;
    h.rt.redraw();
    for _ in 0..3 {
      h.frame();
    }

    let end = 2.0 + 52_998_000.0 + 2_000.0 - 300.0;
    assert_eq!(h.rt.ui.scroll_exact(area).map(|s| s.0[1]), Some(end));
  }

  /// The same content, its thumb dragged past the track's end: the offset reaches the end
  /// exactly, not the nearest `f32`.
  #[test]
  fn a_thumb_dragged_past_the_end_reaches_it_exactly() {
    struct Long;
    impl App for Long {
      fn view(&mut self, ui: &mut Ui) {
        ui.id("area").scroll_area(Style::new().w(px(200.0)).h(px(300.0)), |ui| {
          ui.el(Style::new().h(px(2.0)));
          ui.el(Style::new().h(px(52_998_000.0)));
          ui.el(Style::new().h(px(2_000.0)));
        });
      }
    }

    let mut h = Harness::new(Long, Size::new(200.0, 300.0), 2.0);
    let area = Id::of("area");
    h.frame();
    h.move_to(195.0, 150.0);
    h.wait(Duration::from_millis(300));

    let thumb = h.rect(area.key(("thumb", 1usize))).expect("a thumb");
    let (x, y) = (thumb.x + thumb.w / 2.0, thumb.y + thumb.h / 2.0);
    h.press(x, y);
    h.move_to(x, y + 1000.0);
    h.frame();
    h.release(x, y + 1000.0);
    for _ in 0..3 {
      h.frame();
    }

    let end = 2.0 + 52_998_000.0 + 2_000.0 - 300.0;
    assert_eq!(h.rt.ui.scroll_exact(area).map(|s| s.0[1]), Some(end));
  }

  #[test]
  fn edges_fade_as_the_content_scrolls_away_and_the_thumb_drags() {
    let mut h = Harness::new(Tall(1000.0), Size::new(200.0, 200.0), 1.0);
    let area = Id::of("area");

    // At the top: no fade above, the full one below.
    let img = h.render();
    assert_eq!(img.pixel(100.0, 1.0), [0, 0, 255, 255]);
    assert!(img.pixel(100.0, 199.0)[0] > 200, "{:?}", img.pixel(100.0, 199.0));

    // Scrolled 10 down: half the fade above.
    h.rt.ui.store.raw::<ScrollState>(area).offset[1] = 10.0;
    h.rt.redraw();
    let top = h.render().pixel(100.0, 0.0);
    assert!(top[0] > 100 && top[0] < 150 && top[2] > 100, "{top:?}");

    // The pointer in the bar's strip shows the bars; the thumb drags the content 4 points per
    // point (800 over a track of 196 less the thumb of 40).
    h.move_to(195.0, 100.0);
    h.wait(Duration::from_millis(300));
    let thumb = h.rect(area.key(("thumb", 1usize))).expect("a thumb");
    let (x, y) = (thumb.x + thumb.w / 2.0, thumb.y + thumb.h / 2.0);
    h.press(x, y);
    h.move_to(x, y + 10.0);
    h.frame();
    let off = h.rt.ui.scroll(area).unwrap().offset.y;
    assert!((off - (10.0 + 10.0 * 800.0 / 156.0)).abs() < 0.5, "{off}");
    h.release(x, y + 10.0);

    // A drag no frame saw held: its last move counts.
    h.frame();
    let thumb = h.rect(area.key(("thumb", 1usize))).unwrap();
    let (x, y) = (thumb.x + thumb.w / 2.0, thumb.y + thumb.h / 2.0);
    h.press(x, y);
    h.move_to(x, y + 10.0);
    h.release(x, y + 10.0);
    h.frame();
    let now = h.rt.ui.scroll(area).unwrap().offset.y;
    assert!((now - off - 10.0 * 800.0 / 156.0).abs() < 0.5, "{off} → {now}");
    let off = now;

    // A click on the track below the thumb pages down.
    h.click(195.0, 190.0);
    h.frame();
    assert!(h.rt.ui.scroll(area).unwrap().offset.y > off + 150.0);

    // The pointer resting on the track keeps the bars, with no frames.
    h.move_to(195.0, 100.0);
    h.wait(Duration::from_millis(300));
    assert!(h.wait(Duration::from_millis(3000)) == 0 && h.rt.due_in().is_none());
    assert!(h.rect(area.key(("thumb", 1usize))).is_some());

    // The content twice as tall: the thumb fits it in the frame that shows it.
    h.app_mut().0 = 2000.0;
    h.frame();
    let thumb = h.rect(area.key(("thumb", 1usize))).unwrap();
    assert!((thumb.h - 24.0).abs() < 0.5, "{thumb:?}");

    // Left alone, the bars fade out and the frames stop.
    h.leave();
    h.wait(Duration::from_millis(2000));
    assert!(h.rect(area.key(("thumb", 1usize))).is_none());
    assert!(!h.frame());
  }

  /// The thumbs in the environment's colour, here red.
  #[test]
  fn the_thumbs_are_the_environments() {
    struct Red;
    impl App for Red {
      fn view(&mut self, ui: &mut Ui) {
        let red = ScrollerKnob { color: rgb(0xff0000), ..ScrollerKnob::DARK };
        ui.set_environment(Some(red));
        ui.id("area").scroll_area(Style::new().w(px(200.0)).h(px(200.0)), |ui| {
          ui.el(Style::new().h(px(1000.0)));
        });
      }
    }

    let mut h = Harness::new(Red, Size::new(200.0, 200.0), 1.0);
    h.frame();
    h.move_to(195.0, 100.0);
    h.wait(Duration::from_millis(300));
    let thumb = h.rect(Id::of("area").key(("thumb", 1usize))).expect("a thumb");
    let p = h.render().pixel(thumb.x + thumb.w / 2.0, thumb.y + thumb.h / 2.0);
    assert!(p[0] > 200 && p[1] < 60 && p[2] < 60, "{p:?}");
  }
}
