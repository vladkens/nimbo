// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The scroll area ([Ui::scroll_area]): a container scrolling its children under overlay
//! scrollbars as macOS draws them, thin, shown while the content moves or the pointer comes in,
//! then fading out; a thumb dragged, a track clicked to page ([Ui::scroll_behavior], for an app
//! that draws its own); its content fading under its edges ([Ui::fade_edges]), its margins
//! ([Ui::content_margins]). Headless but
//! drawn: menus and lists scroll in it, so its knobs are the environment's ([ScrollerKnob]), by
//! default the color scheme's ([Ui::color_scheme]).
//!
//! ```ignore
//! ui.fade_edges(t.bg(), 24.0).scroll_area(Style::new().grow(), |ui| {
//!   for f in &self.files { ui.label(f) }
//! });
//! ```

use std::time::Duration;

use crate::app::MouseButton;
use crate::scene::{Color, Point, alpha};
use crate::ui::input::ScrollState;
use crate::ui::{
  Align, Axes, Axis, EnvironmentValue, Float, Flow, Id, Kind, Response, Scroll, Style, Transition,
  Ui, UnitPoint, pct, px,
};

/// How long scrollbars stay after the content last moved or the pointer came in.
const BAR_SHOW: Duration = Duration::from_millis(1000);
/// A thumb's least length.
pub(super) const MIN_THUMB: f32 = 24.0;

/// How [Ui::scroll_area] draws its knobs (the thumbs), an environment value, as AppKit's
/// `NSScroller.knobStyle`: their colour, and how they thicken and darken while the pointer is on
/// a track. [Self::DARK] in the light color scheme ([Ui::color_scheme]), [Self::LIGHT] in the dark.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollerKnob {
  pub(crate) color: Color,
  pub(crate) hover: Transition,
}

impl ScrollerKnob {
  /// AppKit's `.dark`: a dark knob, for light content (the light appearance's).
  pub const DARK: Self = Self { color: alpha([0.0, 0.0, 0.0, 1.0], 0.24), hover: HOVER };
  /// AppKit's `.light`: a light knob, for dark content (the dark appearance's).
  pub const LIGHT: Self = Self { color: alpha([1.0; 4], 0.26), hover: HOVER };

  /// In `color`, as an app's own knob: `ScrollerKnob::DARK.color(red)`.
  pub const fn color(self, color: Color) -> Self {
    Self { color, ..self }
  }
}

impl EnvironmentValue for ScrollerKnob {
  fn default_value(ui: &Ui) -> Self {
    if ui.color_scheme().is_dark() { Self::LIGHT } else { Self::DARK }
  }
}

/// The knobs' change on a track's hover, as the theme's hover fades (`Theme::fade`).
const HOVER: Transition = Transition::smooth(6.0);

/// The anchor the next scroll area keeps its content at ([Ui::default_scroll_anchor]).
struct ScrollAnchor(UnitPoint);

/// The fade the next scroll area draws under its edges ([Ui::fade_edges]): colour and length.
struct EdgeFade(Color, f32);

/// Where [Ui::content_margins] puts its margins, as SwiftUI's `ContentMarginPlacement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentMarginPlacement {
  /// Around the content and the scroll indicators both.
  Automatic,
  /// Around the content alone: padding that scrolls with it.
  ScrollContent,
  /// Around the scroll indicators alone: their tracks kept off what stays over the content's
  /// edges (a sheet's headers), as AppKit's `NSScrollView.scrollerInsets`.
  ScrollIndicators,
}

/// The next scroll area's margins ([Ui::content_margins]), top, right, bottom and left: around its
/// content and around its scroll indicators.
#[derive(Default)]
struct Margins {
  content: [f32; 4],
  indicators: [f32; 4],
}

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
#[non_exhaustive]
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

/// Margin `m`'s (top, right, bottom, left) at the start and the end of axis `ax`.
fn ends(m: [f32; 4], ax: usize) -> (f32, f32) {
  if ax == 0 { (m[3], m[1]) } else { (m[0], m[2]) }
}

/// The bar of axis `ax` at `off`, when the content overflows; `both` when the other axis has
/// one too, which takes the corner; its track inside the indicators' margins `m`.
fn geo(sc: &Scroll, ax: usize, off: f32, both: bool, m: [f32; 4]) -> Option<ScrollBar> {
  let (view, content) = match ax {
    0 => (sc.viewport.w, sc.content.w),
    _ => (sc.viewport.h, sc.content.h),
  };

  if content <= view + 0.5 {
    return None;
  }

  let (start, end) = ends(m, ax);
  let corner = if both { ScrollBar::BREADTH } else { 0.0 };
  let track = view - 2.0 * ScrollBar::INSET - corner - start - end;
  let thumb = (track * view / content).max(MIN_THUMB).min(track);
  let (max, travel) = (content - view, (track - thumb).max(1.0));
  Some(ScrollBar { track, thumb, pos: (off / max).clamp(0.0, 1.0) * travel, ratio: max / travel })
}

impl Ui<'_> {
  /// The scrollbars' behaviour of scroll area `id` scrolling along `axes`: a thumb dragged from
  /// where it was pressed, a click on the track paging toward it, the bars shown while the
  /// content moves, the pointer comes in or moves in a track's strip, then fading out; they
  /// stay while a thumb is held or the pointer is on a track. Read before the area is built, as
  /// of the last frame's content and viewport; the view runs again when the layout changes them
  /// while the bars show, or lays the area out first ([Ui::settle]). A design that draws its bars
  /// while they are hidden settles the area itself to follow its content.
  pub fn scroll_behavior(&mut self, id: Id, axes: Axes) -> ScrollBehavior {
    if self.scroll(id).is_none() {
      self.settle(id);
    }

    self.scroll_bars(id, axes, [0.0; 4])
  }

  /// [Ui::scroll_behavior] with no run of its own for an area not laid out yet: a bar hidden
  /// until something moves has nothing to show on its first frame. The tracks inside the
  /// indicators' margins `m`.
  fn scroll_bars(&mut self, id: Id, axes: Axes, m: [f32; 4]) -> ScrollBehavior {
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
      let scrolls = |ax: usize| axes.at(ax) && geo(&sc, ax, 0.0, false, m).is_some();
      let both = scrolls(0) && scrolls(1);
      for (ax, g) in geos.iter_mut().enumerate() {
        if !axes.at(ax) {
          continue;
        }

        let Some(first) = geo(&sc, ax, axis(off, ax), both, m) else { continue };
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
          let start = axis(Point::new(sc.viewport.x, sc.viewport.y), ax) + ends(m, ax).0;
          let start = start + ScrollBar::INSET + first.pos;
          let page = axis(Point::new(sc.viewport.w, sc.viewport.h), ax) * 0.9;
          let dir = if axis(c.pos, ax) < start { -1.0 } else { 1.0 };
          ex[ax] += f64::from(dir * page);
          moved = true;
        }

        // The pointer moving in the track's strip shows the bars, though hidden ones take no
        // clicks.
        let vp = sc.viewport;
        let strip = |p: Point| match ax {
          0 => (0.0..=ScrollBar::BREADTH).contains(&(vp.y + vp.h - m[2] - p.y)),
          _ => (0.0..=ScrollBar::BREADTH).contains(&(vp.x + vp.w - m[1] - p.x)),
        };
        if over && mouse != bars.mouse && mouse.is_some_and(strip) {
          bars.shown = Some(now);
        }

        pinned |= self.hovered(track);
        off = with_axis(off, ax, ex[ax] as f32);
        let max = sc.max();
        off = Point::new(off.x.clamp(0.0, max.x), off.y.clamp(0.0, max.y));
        *g = geo(&sc, ax, axis(off, ax), both, m);
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
    // The bars are of the last frame's content and viewport: the view runs again if the layout
    // changes them while they show.
    if opacity > 0.0 || until.is_some() {
      self.settle(id);
    }

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

  /// The next [Ui::scroll_area]'s margins `insets` (top, right, bottom, left) around its content,
  /// its scroll indicators or both, as `placement` says ([ContentMarginPlacement]) and SwiftUI's
  /// `contentMargins(_:_:for:)`; a call for each sets both.
  pub fn content_margins(
    &mut self,
    insets: [f32; 4],
    placement: ContentMarginPlacement,
  ) -> &mut Self {
    let mut m = self.set_slot::<Margins>(None).unwrap_or_default();
    if placement != ContentMarginPlacement::ScrollIndicators {
      m.content = insets;
    }

    if placement != ContentMarginPlacement::ScrollContent {
      m.indicators = insets;
    }

    self.set_slot(Some(m));
    self
  }

  /// The margins around its content the next [Ui::scroll_area] puts ([Ui::content_margins]),
  /// for what lays its content out itself (a list's rows).
  pub(crate) fn next_content_margins(&mut self) -> [f32; 4] {
    self.slot::<Margins>().map_or([0.0; 4], |m| m.content)
  }

  /// The next [Ui::scroll_area] keeps its content at `anchor`, as SwiftUI's
  /// `defaultScrollAnchor(_:)`: it opens there (unless [Ui::scroll_to] said where first),
  /// content smaller than the viewport sits at its start, centre or end, whichever the anchor
  /// is nearest, and at an end ([UnitPoint::BOTTOM], a chat) it stays there as the content
  /// changes until scrolled away, and again once scrolled (or [Ui::scroll_to]) back.
  pub fn default_scroll_anchor(&mut self, anchor: UnitPoint) -> &mut Self {
    self.set_slot(Some(ScrollAnchor(anchor)));
    self
  }

  /// A container scrolling its children, with overlay scrollbars, their knobs the
  /// environment's ([ScrollerKnob]). `style` sets its size and padding, and the axes with
  /// [Style::scroll] (vertical by default), as SwiftUI's `ScrollView`. Its id is the scroll
  /// container's ([Ui::scroll], [Ui::scroll_to]); the thumbs and tracks are its behaviour's
  /// ([ScrollBehavior::thumb], [ScrollBehavior::track]).
  pub fn scroll_area<R>(&mut self, style: Style, f: impl FnOnce(&mut Self) -> R) -> Response {
    let knob = self.environment_value::<ScrollerKnob>();
    let id = self.take_id();
    let mask = self.set_slot::<EdgeFade>(None);
    let Margins { content, indicators: m } = self.set_slot::<Margins>(None).unwrap_or_default();
    let mut style = match style.scroll.is_empty() {
      true => style.scroll(Axes::VERTICAL),
      false => style.clip(),
    };

    style.pad = [0, 1, 2, 3].map(|i| style.pad[i] + content[i]);

    // The layout keeps the offset at the anchor; content smaller than the viewport sits there
    // along the axes that scroll.
    let anchor = self.set_slot::<ScrollAnchor>(None).map(|a| a.0);
    self.state::<ScrollState>(id).anchor = anchor;
    for ax in (0..2).filter(|&ax| style.scroll.at(ax)) {
      if let Some(f) = anchor.map(|a| a.at(ax)) {
        style.align[ax] = match f {
          _ if f > 0.75 => Align::End,
          _ if f >= 0.25 => Align::Center,
          _ => Align::Start,
        };
      }
    }

    let b = self.scroll_bars(id, style.scroll, m);
    let (opacity, shown) = (b.opacity, b.shown);
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
              .offset(ScrollBar::INSET + m[3], -m[2]);
            let s =
              Style::new().w(px(g.track)).h(px(ScrollBar::BREADTH)).align(Align::Start, Align::End);
            (at, s, [0.0, 0.0, 2.0, 0.0])
          }
          _ => {
            let at = Float::parent()
              .attach(UnitPoint::TOP_TRAILING)
              .anchor(UnitPoint::TOP_TRAILING)
              .offset(-m[1], ScrollBar::INSET + m[0]);
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

  use crate::app::{App, Event, Phase};
  use crate::scene::{Point, Size, rgb};
  use crate::ui::input::ScrollState;
  use crate::ui::{
    Axes, Axis, ContentMarginPlacement, Float, Id, ScrollBar, ScrollerKnob, Style, Ui, UnitPoint,
    px,
  };
  use crate::{Harness, Modifiers};

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

  /// A design's own bar on [Ui::scroll_behavior], always shown: it is there on the first frame,
  /// with no input to run the view again.
  #[test]
  fn a_custom_bar_shows_on_the_first_frame() {
    struct Custom;
    impl App for Custom {
      fn view(&mut self, ui: &mut Ui) {
        let id = Id::of("area");
        let b = ui.scroll_behavior(id, Axes::VERTICAL);
        let s = Style::new().w(px(200.0)).h(px(200.0)).scroll(Axes::VERTICAL);
        ui.with_id(id).col(s, |ui| {
          ui.el(Style::new().h(px(1000.0)));
          if let Some(g) = b.bar(Axis::Vertical) {
            let bar = Style::new().w(px(6.0)).h(px(g.thumb));
            ui.with_id(b.thumb(Axis::Vertical)).float(Float::parent()).el(bar);
          }
        });
      }
    }

    let mut h = Harness::new(Custom, Size::new(200.0, 200.0), 1.0);
    h.frame();
    assert!(h.rect(Id::of("area").key(("thumb", 1usize))).is_some());
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
    h.redraw();
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
    h.redraw();
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
    let off = h.scroll(area).unwrap().offset.y;
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
    let now = h.scroll(area).unwrap().offset.y;
    assert!((now - off - 10.0 * 800.0 / 156.0).abs() < 0.5, "{off} → {now}");
    let off = now;

    // A click on the track below the thumb pages down.
    h.click(195.0, 190.0);
    h.frame();
    assert!(h.scroll(area).unwrap().offset.y > off + 150.0);

    // The pointer resting on the track keeps the bars, with no frames.
    h.move_to(195.0, 100.0);
    h.wait(Duration::from_millis(300));
    assert!(h.wait(Duration::from_millis(3000)) == 0 && h.due_in().is_none());
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

  /// A sheet 200 square over 1000 square, with headers 20 high and 30 wide over its top and left
  /// edges: its scrollers' tracks start past them; its content padded where the app says.
  #[test]
  fn content_margins_keep_the_scrollers_off_the_headers() {
    struct Sheet(ContentMarginPlacement);
    impl App for Sheet {
      fn view(&mut self, ui: &mut Ui) {
        let s = Style::new().w(px(200.0)).h(px(200.0)).scroll(Axes::ALL);
        ui.id("area").content_margins([20.0, 0.0, 0.0, 30.0], self.0).scroll_area(s, |ui| {
          ui.id("cells").el(Style::new().w(px(1000.0)).h(px(1000.0)));
        });
      }
    }

    let at = |p: ContentMarginPlacement| {
      let mut h = Harness::new(Sheet(p), Size::new(200.0, 200.0), 1.0);
      h.frame();
      h.move_to(100.0, 100.0);
      h.wait(Duration::from_millis(300));
      let thumb = |ax: usize| h.rect(Id::of("area").key(("thumb", ax)));
      (thumb(0), thumb(1), h.rect(Id::of("cells")).unwrap())
    };

    let (x, y, cells) = at(ContentMarginPlacement::ScrollIndicators);
    let (x, y) = (x.expect("a horizontal thumb"), y.expect("a vertical one"));
    assert_eq!((x.x, y.y), (30.0 + ScrollBar::INSET, 20.0 + ScrollBar::INSET));
    // The tracks less the headers, the corner and the insets: 1000 over 200 in each.
    assert!((x.w - (200.0 - 30.0 - 12.0 - 4.0) / 5.0).abs() < 0.5, "{x:?}");
    assert!((y.h - (200.0 - 20.0 - 12.0 - 4.0) / 5.0).abs() < 0.5, "{y:?}");
    assert_eq!((cells.x, cells.y), (0.0, 0.0), "the content where it was");

    let (x, _, cells) = at(ContentMarginPlacement::ScrollContent);
    assert_eq!((x.unwrap().x, cells.x, cells.y), (ScrollBar::INSET, 30.0, 20.0));
    let (x, _, cells) = at(ContentMarginPlacement::Automatic);
    assert_eq!((x.unwrap().x, cells.x, cells.y), (30.0 + ScrollBar::INSET, 30.0, 20.0));
  }

  /// A scroll area 200 high at the bottom anchor over `n` rows 30 high.
  struct Chat(usize);

  impl App for Chat {
    fn view(&mut self, ui: &mut Ui) {
      let s = Style::new().w(px(200.0)).h(px(200.0));
      ui.id("area").default_scroll_anchor(UnitPoint::BOTTOM).scroll_area(s, |ui| {
        (0..self.0).for_each(|i| _ = ui.key(i).el(Style::new().h(px(30.0))));
      });
    }
  }

  fn offset(h: &Harness<Chat>) -> f32 {
    h.scroll(Id::of("area")).unwrap().offset.y
  }

  /// A trackpad's scroll of `dy` points, the content moving down, over the area.
  fn swipe<A: App>(h: &mut Harness<A>, dy: f32) {
    let (pos, delta, mods) = (Point::new(100.0, 100.0), Point::new(0.0, dy), Modifiers::empty());
    let (phase, momentum) = (Phase::Changed, Phase::None);
    h.event(Event::Scroll { pos, delta, precise: true, phase, momentum, mods });
    h.frame();
  }

  /// At the bottom anchor it opens at the end and stays there as rows come; scrolled up, the
  /// content stays put as they do; scrolled back down, it holds again.
  #[test]
  fn the_bottom_anchor_follows_the_end_until_scrolled_away() {
    let mut h = Harness::new(Chat(20), Size::new(200.0, 200.0), 1.0);
    h.frame();
    assert_eq!(offset(&h), 400.0, "opens at the end");

    h.app_mut().0 = 21;
    h.redraw();
    h.frame();
    assert_eq!(offset(&h), 430.0, "a row more, still at the end");

    swipe(&mut h, 100.0);
    assert_eq!(offset(&h), 330.0);
    h.app_mut().0 = 22;
    h.redraw();
    h.frame();
    assert_eq!(offset(&h), 330.0, "scrolled up, it stays");

    swipe(&mut h, -1000.0);
    assert_eq!(offset(&h), 460.0, "back at the end");
    h.app_mut().0 = 23;
    h.redraw();
    h.frame();
    assert_eq!(offset(&h), 490.0, "held again");
  }

  /// Steps smaller than a point, a trackpad's slow start, and a mouse wheel's notch each move it
  /// off the end, one after the other.
  #[test]
  fn small_steps_and_the_wheel_leave_the_end() {
    let mut h = Harness::new(Chat(20), Size::new(200.0, 200.0), 1.0);
    h.frame();
    for _ in 0..4 {
      swipe(&mut h, 0.5);
    }

    assert_eq!(offset(&h), 398.0);

    let mut h = Harness::new(Chat(20), Size::new(200.0, 200.0), 1.0);
    h.frame();
    let (pos, delta, mods) = (Point::new(100.0, 100.0), Point::new(0.0, 10.0), Modifiers::empty());
    let (phase, momentum) = (Phase::None, Phase::None);
    h.event(Event::Scroll { pos, delta, precise: false, phase, momentum, mods });
    h.wait(Duration::from_millis(500));
    assert_eq!(offset(&h), 390.0, "the wheel's momentum ran out");
  }

  /// Rows fewer than fill it sit at the bottom, as a chat's first messages.
  #[test]
  fn short_content_sits_at_the_anchor() {
    let mut h = Harness::new(Chat(2), Size::new(200.0, 200.0), 1.0);
    h.frame();
    let area = h.rect(Id::of("area")).unwrap();
    let row = h.rect(Id::of("area").key(1usize)).unwrap();
    assert_eq!(row.y + row.h, area.y + area.h);
  }

  /// `n` rows 30 high in a scroll area 200 high at `anchor`, scrolled to `ask` as it first
  /// shows.
  struct Anchored {
    n: usize,
    anchor: UnitPoint,
    ask: Option<f32>,
  }

  impl App for Anchored {
    fn view(&mut self, ui: &mut Ui) {
      if let Some(y) = self.ask.take() {
        ui.scroll_to(Id::of("area"), Point::new(0.0, y));
      }

      let s = Style::new().w(px(200.0)).h(px(200.0));
      ui.id("area").default_scroll_anchor(self.anchor).scroll_area(s, |ui| {
        (0..self.n).for_each(|i| _ = ui.key(i).el(Style::new().h(px(30.0))));
      });
    }
  }

  /// Scrolled to before it first shows, it opens there, not at its anchor.
  #[test]
  fn a_scroll_asked_first_wins_over_the_anchor() {
    let app = Anchored { n: 20, anchor: UnitPoint::BOTTOM, ask: Some(100.0) };
    let mut h = Harness::new(app, Size::new(200.0, 200.0), 1.0);
    h.frame();
    assert_eq!(h.scroll(Id::of("area")).unwrap().offset.y, 100.0);
  }

  /// Short content sits at the start, centre or end the anchor is nearest.
  #[test]
  fn short_content_sits_where_the_anchor_is_nearest() {
    for (y, top) in [(0.2, 0.0), (0.3, 70.0), (0.7, 70.0), (0.8, 140.0)] {
      let app = Anchored { n: 2, anchor: UnitPoint::new(0.0, y), ask: None };
      let mut h = Harness::new(app, Size::new(200.0, 200.0), 1.0);
      h.frame();
      let (area, row) = (h.rect(Id::of("area")).unwrap(), h.rect(Id::of("area").key(0usize)));
      assert_eq!(row.unwrap().y - area.y, top, "at {y}");
    }
  }
}
