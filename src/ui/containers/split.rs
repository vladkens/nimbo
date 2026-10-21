// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The split: two panes with a divider to drag between them.
//!
//! ```ignore
//! Split::row().ratio(&mut self.side).min(160.0, 320.0).show(ui.id("split"), |ui, pane| match pane {
//!   0 => self.sidebar(ui),
//!   _ => self.main(ui),
//! });
//! ```
//!
//! The split keeps its ratio in the app or in its own state; dragging the divider honours both
//! panes' minimum sizes. The behaviour is [Ui::split_behavior].

use crate::ui::{Behavior, Cursor, Float, Id, Response, Style, Ui, grow, pct, px};

/// A split's divider ([Ui::split_behavior]): its handle's input, and the first pane's share
/// to show and the panes' minimums, both fitted to the split's size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SplitBehavior {
  pub handle: Behavior,
  pub shown: f32,
  pub min: [f32; 2],
}

/// Two panes side by side ([Split::row]) or stacked ([Split::col]) with a divider to drag
/// (`id.key("handle")`).
pub struct Split<'s> {
  axis: u8,
  ratio: Option<&'s mut f32>,
  init: f32,
  min: [f32; 2],
}

impl<'s> Split<'s> {
  /// Panes left and right of a vertical divider.
  pub fn row() -> Self {
    Self { axis: 0, ratio: None, init: 0.5, min: [0.0; 2] }
  }

  /// Panes above and below a horizontal divider.
  pub fn col() -> Self {
    Self { axis: 1, ..Self::row() }
  }

  /// The first pane's share of the split, kept by the app; without it the split keeps its
  /// own, from [Split::initial].
  pub fn ratio(self, ratio: &'s mut f32) -> Self {
    Self { ratio: Some(ratio), ..self }
  }

  pub fn initial(self, ratio: f32) -> Self {
    Self { init: ratio, ..self }
  }

  /// The least size of each pane along the split, in points.
  pub fn min(self, first: f32, second: f32) -> Self {
    Self { min: [first, second], ..self }
  }

  /// Builds the panes with `pane(ui, 0)` and `pane(ui, 1)`; `changed` when a drag moved the
  /// divider.
  pub fn show<'a>(self, ui: &mut Ui<'a>, mut pane: impl FnMut(&mut Ui<'a>, usize)) -> Response {
    /// The ratio, when the app doesn't hold it.
    #[derive(Default)]
    struct Own(Option<f32>);

    let t = ui.theme();
    let (id, ax) = (ui.take_id(), self.axis as usize);
    let handle = id.key("handle");
    let own = ui.state::<Own>(id).0;
    let mut r = self.ratio.as_deref().copied().unwrap_or(own.unwrap_or(self.init));
    let b = ui.split_behavior(id, handle, self.axis, &mut r, self.min);
    let (held, min) = (b.handle.held, b.min);
    match self.ratio {
      Some(x) => *x = r,
      None => ui.state::<Own>(id).0 = Some(r),
    }

    // Shown within the minimums; the ratio kept is the one asked, for when there is room again.
    // The split clips, so a window narrower than both minimums shrinks it, and them next frame.
    let p = pct(b.shown * 100.0);
    let (first, second, divider, grip) = match ax {
      0 => (
        Style::new().w(p).min_w(min[0]).h(grow()),
        Style::new().grow().min_w(min[1]),
        Style::new().w(px(1.0)).h(grow()),
        Style::new().w(px(7.0)).h(grow()).cursor(Cursor::ResizeX),
      ),
      _ => (
        Style::new().h(p).min_h(min[0]).w(grow()),
        Style::new().grow().min_h(min[1]),
        Style::new().h(px(1.0)).w(grow()),
        Style::new().h(px(7.0)).w(grow()).cursor(Cursor::ResizeY),
      ),
    };
    // The line takes the accent while dragged, fading in and out.
    let hot = ui.hot(handle, held || b.handle.hovered);
    let line = crate::scene::mix(t.line, t.accent, hot);
    let mut resp = ui.with_id(id).container(self.axis, Style::new().grow().clip(), |ui| {
      ui.col(first.clip(), |ui| pane(ui, 0));
      ui.row(divider.bg(line), |ui| {
        let at = Float::parent().attach(0.5, 0.5).anchor(0.5, 0.5).clip();
        ui.with_id(handle).float(at).el(grip);
      });
      ui.col(second.clip(), |ui| pane(ui, 1));
    });
    resp.changed = b.handle.changed;
    resp
  }
}

impl Ui<'_> {
  /// A split `id` along `axis` (0: panes side by side) with its divider's handle `handle`:
  /// dragging the handle sets `ratio`, the first pane's share, keeping the pane under the
  /// pointer where it was grabbed, to where it was released. The fitted share and minimums come
  /// from the last frame's size, the view running again when the layout changes it ([Ui::settle]):
  /// narrower than both minimums, each pane gets its share of what there is.
  pub fn split_behavior(
    &mut self,
    id: Id,
    handle: Id,
    axis: u8,
    ratio: &mut f32,
    min: [f32; 2],
  ) -> SplitBehavior {
    /// The pointer's distance from the divider when the drag began.
    #[derive(Default)]
    struct Grab(Option<f32>);

    let ax = axis as usize;
    let b = self.begin(handle).unwrap_or_else(|| self.pointer_state(handle));
    let span = self.rect(id).map(|rect| if ax == 0 { (rect.x, rect.w) } else { (rect.y, rect.h) });
    // Fitted to the last frame's size: the view runs again if the layout changes it.
    self.settle(id);

    let min = match span {
      Some((_, len)) if min[0] + min[1] > (len - 1.0).max(0.0) => {
        let k = (len - 1.0).max(0.0) / (min[0] + min[1]);
        [min[0] * k, min[1] * k]
      }
      _ => min,
    };

    let fit = |r: f32| match span {
      Some((_, len)) if len > 0.0 => {
        let lo = min[0] / len;
        r.clamp(lo, ((len - 1.0 - min[1]) / len).max(lo))
      }
      _ => r,
    };
    let was = *ratio;
    let mut grab = std::mem::take(&mut self.state::<Grab>(id).0);
    let along = |p: crate::scene::Point| if ax == 0 { p.x } else { p.y };
    // Held, or released since the view last ran: its last move counts.
    let held = self.press().zip(self.mouse()).filter(|_| b.held);
    match (span, held.or(self.released(handle)).filter(|_| !b.disabled)) {
      (Some((lo, len)), Some((p, m))) => {
        // From where it was pressed, though no frame came between the press and this move, to
        // the divider where it shows.
        let (at, from) = (along(m), along(p.pos));
        let g = *grab.get_or_insert(from - (lo + fit(*ratio) * len));
        *ratio = fit((at - g - lo) / len.max(1.0));
      }
      _ => grab = None,
    }

    self.state::<Grab>(id).0 = grab.filter(|_| b.held);
    let handle = Behavior { changed: *ratio != was, ..b };
    SplitBehavior { handle, shown: fit(*ratio), min }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::{Rect, Size};

  /// A split with minimums wider than the window.
  struct Narrow;

  impl App for Narrow {
    fn view(&mut self, ui: &mut Ui) {
      Split::row().min(100.0, 100.0).show(ui.id("split"), |ui, _| {
        ui.el(Style::new().grow());
      });
    }
  }

  #[test]
  fn a_split_narrower_than_its_minimums_shrinks_both_panes() {
    let mut h = Harness::new(Narrow, Size::new(150.0, 200.0), 1.0);
    // The first frame fits it, its view run again on its layout.
    h.frame();
    let split = h.rect(Id::of("split")).unwrap();
    let kids: Vec<Rect> =
      h.rt.ui.last.nodes.iter().filter(|n| n.parent != 0).map(|n| n.rect).collect();
    assert!(split.w <= 150.0, "{split:?}");
    assert!(kids.iter().all(|r| r.x + r.w <= split.x + split.w + 0.5), "{kids:?}");
  }

  /// A split asked for 10% with a first pane of 200 at least.
  struct Held(f32);

  impl App for Held {
    fn view(&mut self, ui: &mut Ui) {
      Split::row().ratio(&mut self.0).min(200.0, 100.0).show(ui.id("split"), |ui, _| {
        ui.el(Style::new().grow());
      });
    }
  }

  #[test]
  fn a_drag_starts_from_the_divider_where_it_shows() {
    let mut h = Harness::new(Held(0.1), Size::new(500.0, 200.0), 1.0);
    h.frame();
    h.frame();
    let handle = h.rect(Id::of("split").key("handle")).unwrap();
    let (x, y) = (handle.x + handle.w / 2.0, handle.y + 20.0);
    assert!((x - 200.5).abs() < 1.0, "{handle:?}");
    // 10 points right moves it 10 points: no dead zone from the ratio asked.
    h.move_to(x, y);
    h.press(x, y);
    h.move_to(x + 10.0, y);
    h.frame();
    h.release(x + 10.0, y);
    assert!((h.app().0 - 0.42).abs() < 0.005, "{}", h.app().0);
    // A drag no frame saw held: pressed, moved and released between frames.
    h.frame();
    let x = x + 10.0;
    h.press(x, y);
    h.move_to(x + 20.0, y);
    h.release(x + 20.0, y);
    // The pointer moves on before the frame: the drag ends where it was released.
    h.move_to(x + 120.0, y);
    h.frame();
    assert!((h.app().0 - 0.46).abs() < 0.005, "{}", h.app().0);
  }

  /// A split asked for 10% with a first pane of 200 at least, in a box the app sizes.
  struct Boxed(f32);

  impl App for Boxed {
    fn view(&mut self, ui: &mut Ui) {
      ui.col(Style::new().w(px(self.0)).h(px(200.0)), |ui| {
        Split::row().initial(0.1).min(200.0, 100.0).show(ui.id("split"), |ui, _| {
          ui.el(Style::new().grow());
        });
      });
    }
  }

  #[test]
  fn a_split_refits_in_the_frame_its_box_grows() {
    let mut h = Harness::new(Boxed(500.0), Size::new(1000.0, 200.0), 1.0);
    h.frame();
    let at = |h: &Harness<Boxed>| h.rect(Id::of("split").key("handle")).unwrap().x + 3.5;
    assert!((at(&h) - 200.5).abs() < 1.0, "{}", at(&h));
    // Twice as wide: the first pane keeps its 200, not 40% of the new width.
    h.app_mut().0 = 1000.0;
    h.frame();
    assert!((at(&h) - 200.5).abs() < 1.0, "{}", at(&h));
  }
}
