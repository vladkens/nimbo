// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The scroll area: a container scrolling its children under overlay scrollbars, as macOS draws
//! them (thin, shown while the content moves or the pointer comes in, then fading out; drag
//! the thumb or click the track to page; [Ui::scroll_behavior]), and its content fading under
//! its edges ([Ui::fade_edges]).
//!
//! ```ignore
//! ui.fade_edges(t.bg, 24.0).scroll_area(Style::new().grow(), |ui| {
//!   for f in &self.files { ui.label(f) }
//! });
//! ```

use crate::scene::{Color, alpha};
use crate::ui::{
  Align, Axes, Float, Flow, Kind, Response, ScrollBar, ScrollBehavior, Style, Ui, UnitPoint, pct,
  px,
};

/// The fade the next scroll area draws under its edges ([Ui::fade_edges]): colour and length.
struct EdgeFade(Color, f32);

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
  /// padding, and the axes with [Style::scroll] (vertical by default), as SwiftUI's
  /// `ScrollView`. Its
  /// id is the scroll container's ([Ui::scroll], [Ui::scroll_to]); the thumbs are
  /// `id.key(("thumb", axis))`, the tracks `id.key(("track", axis))`, 0 for horizontal.
  pub fn scroll_area<R>(&mut self, style: Style, f: impl FnOnce(&mut Self) -> R) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let mask = self.set_slot::<EdgeFade>(None);
    let style = match style.scroll.is_empty() {
      true => style.scroll(Axes::VERTICAL),
      false => style.clip(),
    };

    let ScrollBehavior { bars, fade, shown, drag } = self.scroll_behavior(id, style.scroll);
    // Drawn from the last frame's content and viewport: again if the layout changes them.
    if fade > 0.0 || shown {
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

      for (ax, g) in bars.iter().enumerate() {
        let Some(g) = g.as_ref().filter(|_| fade > 0.0 || shown) else { continue };
        let (thumb, track) = (id.key(("thumb", ax)), id.key(("track", ax)));
        let hot = ui.hot(track, ui.hovered(track) || drag[ax]);
        let thick = 6.0 + 2.0 * hot;
        let color = alpha(t.thumb, t.thumb[3] * (1.0 + 0.6 * hot));

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
        ui.float(at.clip()).container(flow, s.pads(pad).opacity(fade), |ui| {
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
}
