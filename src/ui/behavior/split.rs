// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A split's behaviour ([Ui::split_behavior]): its divider dragged within both panes'
//! minimums, fitted to the split's size; `Split` draws it.

use crate::scene::Point;
use crate::ui::{Axis, Behavior, Id, Role, Ui, Value};

/// A split's divider ([Ui::split_behavior]): its handle's input, and the first pane's share
/// to show and the panes' minimums, both fitted to the split's size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SplitBehavior {
  pub handle: Behavior,
  pub shown: f32,
  pub min: [f32; 2],
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
    axis: Axis,
    ratio: &mut f32,
    min: [f32; 2],
  ) -> SplitBehavior {
    /// The pointer's distance from the divider when the drag began.
    #[derive(Default)]
    struct Grab(Option<f32>);

    let ax = axis.index();
    let b = self.begin(handle, Role::Splitter).unwrap_or_else(|| self.pointer_state(handle));
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
    let along = |p: Point| if ax == 0 { p.x } else { p.y };
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
    let (handle, shown) = (Behavior { changed: *ratio != was, ..b }, fit(*ratio));
    let share = Value::Number { now: shown.into(), min: 0.0, max: 1.0 };
    self.a11y(handle.id, Role::Splitter).value(share);
    SplitBehavior { handle, shown, min }
  }
}
