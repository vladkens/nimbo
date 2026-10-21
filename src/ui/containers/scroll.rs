// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The scroll area: a container scrolling its children under overlay scrollbars, as macOS draws
//! them (thin, shown while the content moves or the pointer comes in, then fading out; drag
//! the thumb or click the track to page), and its content fading under its edges
//! ([Ui::fade_edges]).
//!
//! ```ignore
//! ui.fade_edges(t.bg, 24.0).scroll_area(Style::new().grow(), |ui| {
//!   for f in &self.files { ui.label(f) }
//! });
//! ```

use std::time::Duration;

use crate::app::MouseButton;
use crate::scene::{Color, Point, alpha};
use crate::ui::{Align, Float, Id, Kind, Response, Scroll, Style, Transition, Ui, pct, px};

/// How long scrollbars stay after the content last moved or the pointer came in.
const BAR_SHOW: Duration = Duration::from_millis(1000);
/// A scrollbar's track across: the thumb and the room to grab it.
const BAR: f32 = 12.0;
/// Gap between the track's ends and the viewport's edges.
const BAR_INSET: f32 = 2.0;
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
  /// The offset when a thumb drag began, per axis.
  drag: [Option<f32>; 2],
}

/// The fade the next scroll area draws under its edges ([Ui::fade_edges]): colour and length.
struct EdgeFade(Color, f32);

/// A scrollbar's lengths along its axis.
struct Geo {
  track: f32,
  thumb: f32,
  /// The thumb's start from the track's.
  pos: f32,
  /// Content points per track point.
  ratio: f32,
}

/// What a scroll area's bars do this frame ([Ui::scroll_bars]): each axis's bar when its
/// content overflows, how far they have faded in, whether they take clicks, and which thumb is
/// being dragged.
struct BarsState {
  geos: [Option<Geo>; 2],
  fade: f32,
  shown: bool,
  drag: [bool; 2],
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
fn geo(sc: &Scroll, ax: usize, off: f32, both: bool) -> Option<Geo> {
  let (view, content) = match ax {
    0 => (sc.viewport.w, sc.content.w),
    _ => (sc.viewport.h, sc.content.h),
  };

  if content <= view + 0.5 {
    return None;
  }

  let track = view - 2.0 * BAR_INSET - if both { BAR } else { 0.0 };
  let thumb = (track * view / content).max(MIN_THUMB).min(track);
  let (max, travel) = (content - view, (track - thumb).max(1.0));
  Some(Geo { track, thumb, pos: (off / max).clamp(0.0, 1.0) * travel, ratio: max / travel })
}

impl Ui<'_> {
  /// The next [Ui::scroll_area] fades its content out under its top and bottom edges, over `len`
  /// points into `color` (the ground it sits on), as CSS's `mask-image` does: an edge fades in
  /// as the content scrolls away from it, fully once it has moved `len`. Drawn under the
  /// scrollbars; takes no pointer input.
  pub fn fade_edges(&mut self, color: Color, len: f32) -> &mut Self {
    self.set_slot(Some(EdgeFade(color, len)));
    self
  }

  /// A container scrolling its children, with overlay scrollbars. `style` sets its size and
  /// padding, and the axes with [Style::scroll_x] / [Style::scroll] (vertical by default). Its
  /// id is the scroll container's ([Ui::scroll], [Ui::scroll_to]); the thumbs are
  /// `id.key(("thumb", axis))`, the tracks `id.key(("track", axis))`, 0 for horizontal.
  pub fn scroll_area<R>(&mut self, style: Style, f: impl FnOnce(&mut Self) -> R) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let mask = self.set_slot::<EdgeFade>(None);
    let style = match style.scroll {
      [false, false] => style.scroll_y(),
      _ => style.clip(),
    };

    let BarsState { geos, fade, shown, drag } = self.scroll_bars(id, style.scroll);
    // Drawn from the last frame's content and viewport: again if the layout changes them.
    if fade > 0.0 || shown {
      self.settle(id);
    }

    self.with_id(id).container(1, style, |ui| {
      f(ui);
      // The fades over the content, under the scrollbars, out of the pointer's way: the paint
      // pass draws them by the offset the layout settled on.
      if let Some(EdgeFade(color, len)) = mask.filter(|_| style.scroll[1]) {
        let s = Style::new().w(pct(100.0)).h(pct(100.0));
        ui.float(Float::parent().clip()).push(Kind::Fade { color, len }, s, 0);
      }

      for (ax, g) in geos.iter().enumerate() {
        let Some(g) = g.as_ref().filter(|_| fade > 0.0 || shown) else { continue };
        let (thumb, track) = (id.key(("thumb", ax)), id.key(("track", ax)));
        let hot = ui.hot(track, ui.hovered(track) || drag[ax]);
        let thick = 6.0 + 2.0 * hot;
        let color = alpha(t.thumb, t.thumb[3] * (1.0 + 0.6 * hot));
        let (at, s, pad) = match ax {
          0 => {
            let at = Float::parent().attach(0.0, 1.0).anchor(0.0, 1.0).offset(BAR_INSET, 0.0);
            let s = Style::new().w(px(g.track)).h(px(BAR)).align(Align::Start, Align::End);
            (at, s, [0.0, 0.0, 2.0, 0.0])
          }
          _ => {
            let at = Float::parent().attach(1.0, 0.0).anchor(1.0, 0.0).offset(0.0, BAR_INSET);
            let s = Style::new().w(px(BAR)).h(px(g.track)).align(Align::End, Align::Start);
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

        ui.float(at.clip()).container(ax as u8, s.pads(pad).opacity(fade), |ui| {
          ui.el(gap);
          if shown {
            ui.with_id(thumb);
          }

          ui.el(bar.radius(thick / 2.0).bg(color));
        });
      }
    })
  }

  /// The scrollbars' behaviour of scroll area `id` scrolling along `axes`: a thumb dragged from
  /// where it was pressed, a click on the track paging toward it, the bars shown while the
  /// content moves, the pointer comes in or moves in a track's strip, then fading out; they
  /// stay while a thumb is held or the pointer is on a track.
  fn scroll_bars(&mut self, id: Id, axes: [bool; 2]) -> BarsState {
    let now = self.now();
    let mut bars = std::mem::take(self.state::<Bars>(id));
    let mut geos: [Option<Geo>; 2] = [None, None];
    // A thumb held or a track under the pointer keep the bars, with no timer running.
    let mut pinned = false;
    let mouse = self.mouse();
    if let Some(sc) = self.scroll(id) {
      let over = self.hovered(id);
      if sc.offset != bars.seen || over && !bars.over {
        bars.shown = Some(now);
      }

      (bars.seen, bars.over) = (sc.offset, over);
      let mut off = sc.offset;
      let scrolls = |ax: usize| axes[ax] && geo(&sc, ax, 0.0, false).is_some();
      let both = scrolls(0) && scrolls(1);
      for (ax, g) in geos.iter_mut().enumerate() {
        if !axes[ax] {
          continue;
        }

        let Some(first) = geo(&sc, ax, axis(off, ax), both) else { continue };
        let (thumb, track) = (id.key(("thumb", ax)), id.key(("track", ax)));
        // Held, or released since the view last ran: its last move counts.
        let held = self.held(thumb);
        let press = self.press().zip(mouse).filter(|_| held).or(self.released(thumb));
        if let Some((p, m)) = press {
          let start = *bars.drag[ax].get_or_insert(axis(off, ax));
          off = with_axis(off, ax, start + (axis(m, ax) - axis(p.pos, ax)) * first.ratio);
        }

        if !held {
          bars.drag[ax] = None;
        }

        pinned |= held;

        if let Some(c) = self.click(track).filter(|c| c.button == MouseButton::Left) {
          let start = axis(Point::new(sc.viewport.x, sc.viewport.y), ax) + BAR_INSET + first.pos;
          let page = axis(Point::new(sc.viewport.w, sc.viewport.h), ax) * 0.9;
          let dir = if axis(c.pos, ax) < start { -1.0 } else { 1.0 };
          off = with_axis(off, ax, axis(off, ax) + dir * page);
        }

        // The pointer moving in the track's strip shows the bars, though hidden ones take no
        // clicks.
        let vp = sc.viewport;
        let strip = |m: Point| match ax {
          0 => m.y >= vp.y + vp.h - BAR,
          _ => m.x >= vp.x + vp.w - BAR,
        };
        if over && mouse != bars.mouse && mouse.is_some_and(strip) {
          bars.shown = Some(now);
        }

        pinned |= self.hovered(track);
        let max = sc.max();
        off = Point::new(off.x.clamp(0.0, max.x), off.y.clamp(0.0, max.y));
        *g = geo(&sc, ax, axis(off, ax), both);
      }

      if off != sc.offset {
        self.scroll_to(id, off);
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
    BarsState { geos, fade, shown: until.is_some(), drag }
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::{Size, rgb};
  use crate::ui::{Id, Style, Ui, px};

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

  #[test]
  fn edges_fade_as_the_content_scrolls_away_and_the_thumb_drags() {
    let mut h = Harness::new(Tall(1000.0), Size::new(200.0, 200.0), 1.0);
    let area = Id::of("area");
    // At the top: no fade above, the full one below.
    let img = h.render();
    assert_eq!(img.pixel(100.0, 1.0), [0, 0, 255, 255]);
    assert!(img.pixel(100.0, 199.0)[0] > 200, "{:?}", img.pixel(100.0, 199.0));
    // Scrolled 10 down: half the fade above.
    h.rt.ui.store.raw::<crate::ui::input::ScrollState>(area).offset.y = 10.0;
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
}
