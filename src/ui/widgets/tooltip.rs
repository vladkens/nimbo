// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The tooltip: a line or two of small text under the pointer resting on an element.

use std::time::Duration;

use crate::scene::Point;
use crate::ui::{Float, Id, TextStyle, Transition, Ui};

/// Paint order of tooltips among overlays: above menus and dialogs.
const TIP_Z: i32 = 2000;
/// A tooltip fades in and out over 125 ms.
const TIP_FADE: Transition = Transition::linear(8.0);

/// When a tooltip shows, from the pointer coming onto its target, and where the pointer was
/// then.
#[derive(Default)]
struct Tip {
  due: Option<Duration>,
  at: Option<Point>,
}

/// Until when the next tooltip shows at once ([crate::Theme::tip_warm]), across targets.
#[derive(Default)]
struct Warm(Duration);

impl Ui<'_> {
  /// Shows `text` under the pointer once it has rested on `target` for
  /// [crate::Theme::tip_delay], or at once within [crate::Theme::tip_warm] of another tooltip
  /// showing, until it leaves or a button goes down. As macOS places one: its left edge at the
  /// pointer, its top 19 below it, where the pointer was when it showed; moved into the window
  /// near its edges. Call it each frame after building the target; the tip is
  /// `target.key("tip")`, an overlay that doesn't take the pointer.
  pub fn tooltip(&mut self, target: Id, text: &str) {
    let t = self.theme();
    let tip = target.key("tip");
    let over = self.hovered(target) && self.press().is_none();
    let (now, mouse, size) = (self.now(), self.mouse(), self.size());
    let warm = now < self.state::<Warm>(Id::of("nimbo.tip")).0;
    let st = self.state::<Tip>(tip);
    if !over {
      (st.due, st.at) = (None, None);
      return;
    }

    let due = *st.due.get_or_insert(if warm { now } else { now + t.tip_delay });
    if now < due {
      self.animate(tip, 0.0, TIP_FADE);
      self.request_frame_in(due - now);
      return;
    }

    let Some(p) = st.at.or(mouse) else { return };
    st.at = Some(p);
    self.state::<Warm>(Id::of("nimbo.tip")).0 = now + t.tip_warm;
    let fade = self.animate(tip, 1.0, TIP_FADE);
    // Placed by its size, measured last frame: built again before painting when it showed
    // only now, or its size changed.
    self.settle(tip);
    // Measured last frame: moved left to fit the window, above the pointer near the bottom.
    let (w, h) = self.rect(tip).map_or((0.0, 0.0), |r| (r.w, r.h));
    let x = p.x.min(size.w - 4.0 - w).max(4.0);
    let (y, ay) = match p.y + 19.0 + h.max(20.0) + 4.0 > size.h {
      true => (p.y - 4.0, 1.0),
      false => (p.y + 19.0, 0.0),
    };

    let at = Float::window().anchor(0.0, ay).offset(x, y).z(TIP_Z);
    let text_style = TextStyle::new(t.small, t.tip_text).line_h((t.small.size * 1.3).round());
    self.next_id = Some((tip, false));
    self.float(at).row(t.tip().opacity(fade), |ui| ui.text(text, text_style));
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::Size;
  use crate::ui::{Id, Ui};

  struct Tipped;

  impl App for Tipped {
    fn view(&mut self, ui: &mut Ui) {
      let a = ui.id("a").button("A");
      ui.tooltip(a.id, "The first");
      let b = ui.id("b").button("B");
      ui.tooltip(b.id, "The second");
    }
  }

  #[test]
  fn a_tip_shows_after_a_rest_and_the_next_at_once() {
    let mut h = Harness::new(Tipped, Size::new(300.0, 200.0), 1.0);
    h.frame();
    let mid = |h: &Harness<Tipped>, n: &str| {
      let r = h.rect(Id::of(n)).unwrap();
      (r.x + r.w / 2.0, r.y + r.h / 2.0)
    };
    let (x, y) = mid(&h, "a");
    h.move_to(x, y);
    h.wait(Duration::from_millis(100));
    assert!(h.rect(Id::of("a").key("tip")).is_none());
    h.wait(Duration::from_millis(200));
    let tip = h.rect(Id::of("a").key("tip")).unwrap();
    assert!((tip.x - x).abs() <= 0.5 && (tip.y - y - 19.0).abs() <= 0.5, "{tip:?}");
    // Onto the next one: warm, it shows at once; a press hides it.
    let (x, y) = mid(&h, "b");
    h.move_to(x, y);
    h.frame();
    assert!(h.rect(Id::of("b").key("tip")).is_some());
    h.press(x, y);
    h.frame();
    assert!(h.rect(Id::of("b").key("tip")).is_none());
  }

  /// Two buttons, the second at the window's right edge.
  struct Edge;

  impl App for Edge {
    fn view(&mut self, ui: &mut Ui) {
      ui.row(crate::ui::Style::new().w(crate::ui::grow()), |ui| {
        let a = ui.id("a").button("A");
        ui.tooltip(a.id, "The first");
        ui.spacer();
        let b = ui.id("b").button("B");
        ui.tooltip(b.id, "A tip much wider than what is left of the window at its right");
      });
    }
  }

  #[test]
  fn a_warm_tip_at_the_edge_shows_inside_the_window_at_once() {
    let mut h = Harness::new(Edge, Size::new(300.0, 200.0), 1.0);
    h.frame();
    let r = h.rect(Id::of("a")).unwrap();
    h.move_to(r.x + 5.0, r.y + 5.0);
    h.wait(Duration::from_millis(300));
    let r = h.rect(Id::of("b")).unwrap();
    h.move_to(r.x + r.w - 2.0, r.y + 5.0);
    h.frame();
    let tip = h.rect(Id::of("b").key("tip")).unwrap();
    assert!(tip.x + tip.w <= 300.0 - 4.0 + 0.5, "{tip:?}");
  }
}
