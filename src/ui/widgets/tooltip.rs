// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The tooltip: a line or two of small text under the pointer resting on an element.

use std::time::Duration;

use crate::scene::Point;
use crate::ui::{Float, Id, TextStyle, Transition, Ui, px};

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
  /// near its edges. Run for each element with [Ui::help] after the view built it; the tip is
  /// `target.key("tip")`, an overlay that doesn't take the pointer. `text` is the target's
  /// description for assistive technology ([Ui::a11y]), shown or not.
  pub(crate) fn tooltip(&mut self, target: Id, text: &str) {
    if let Some(a) = self.a11y_of(target) {
      a.description(text);
    }

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
    // As wide as its widest line, as macOS's: the layout would keep the width before wrapping.
    let (boxed, pads) = (t.tip(), t.tip().pad[1] + t.tip().pad[3]);
    let widest = self.measure_wrapped(text, text_style, boxed.max[0] - pads);
    let boxed = if widest > 0.0 { boxed.w(px(widest.ceil() + pads)) } else { boxed };
    self.next_id = Some((tip, false));
    self.float(at).row(boxed.opacity(fade), |ui| ui.text(text, text_style));
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::Size;
  use crate::ui::{Id, Style, Ui, px};

  /// `help` on a button, a text, a box with a text in it and a canvas.
  struct Helped;

  impl App for Helped {
    fn view(&mut self, ui: &mut Ui) {
      ui.help("Saves it").id("save").button("Save");
      let t = ui.theme();
      let dotted = t.label().underline(crate::ui::LinePattern::Dot);
      ui.help("Errors per 100 words").id("wer").text("WER 4.5%", dotted);
      let t = ui.theme();
      ui.help("A box").id("box").row(Style::new().h(px(20.0)), |ui| ui.text("Inside", t.label()));
      let dot =
        |f: &mut crate::scene::Frame, r: crate::scene::Rect| f.rect(r, [0.0, 0.0, 0.0, 1.0], 2.0);
      ui.help("A dot").id("dot").canvas(Style::new().w(px(8.0)).h(px(8.0)), dot);
    }
  }

  /// Each shows its tip after a rest; assistive technology reads it as the description of the
  /// button and of the text, where they stand, and of a group for the box (its text still read
  /// inside) and the canvas.
  #[test]
  fn help_tips_any_element_and_describes_it() {
    let mut h = Harness::new(Helped, Size::new(300.0, 200.0), 1.0);
    h.frame();
    let tree = h.a11y();
    let lines: Vec<&str> = tree.lines().map(str::trim).collect();
    let save = lines.iter().position(|l| *l == r#"button "Save" desc "Saves it""#);
    let wer =
      lines.iter().position(|l| *l == r#"static text "WER 4.5%" desc "Errors per 100 words""#);
    assert!(save.is_some() && wer.is_some() && save < wer, "{tree}");
    let group = lines.iter().position(|l| *l == r#"group desc "A box""#);
    let inside = lines.iter().position(|l| *l == r#"static text "Inside""#);
    assert!(group.is_some() && group.map(|g| g + 1) == inside, "the box's child read: {tree}");
    assert!(lines.contains(&r#"group desc "A dot""#), "the canvas: {tree}");
    for (name, tip) in
      [("save", "Saves it"), ("wer", "Errors per 100 words"), ("box", "A box"), ("dot", "A dot")]
    {
      let r = h.rect(Id::of(name)).unwrap();
      h.move_to(r.x + r.w / 2.0, r.y + r.h / 2.0);
      h.wait(Duration::from_millis(400));
      assert_eq!(h.text(Id::of(name).key("tip").nth(0)), Some(tip), "{name}");
      h.leave();
      h.wait(Duration::from_millis(600));
    }
  }

  struct Tipped;

  impl App for Tipped {
    fn view(&mut self, ui: &mut Ui) {
      ui.help("The first").id("a").button("A");
      ui.help("The second").id("b").button("B");
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
        ui.help("The first").id("a").button("A");
        ui.spacer();
        ui.help("A tip much wider than what is left of the window at its right")
          .id("b")
          .button("B");
      });
    }
  }

  /// A button whose tip wraps, in the light theme.
  struct Long;

  impl App for Long {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_theme(crate::ui::Theme::PLATES_LIGHT);
      let tip = "What the app shows when it starts. Restoring the last session reopens its \
                 windows where they were.";
      ui.help(tip).id("a").button("A");
    }
  }

  #[test]
  fn a_wrapped_tip_is_as_wide_as_its_widest_line() {
    let mut h = Harness::new(Long, Size::new(400.0, 200.0), 2.0);
    h.frame();
    let r = h.rect(Id::of("a")).unwrap();
    h.move_to(r.x + 2.0, r.y + 2.0);
    h.wait(Duration::from_millis(400));
    let tip = h.rect(Id::of("a").key("tip")).unwrap();
    assert!(tip.h > 30.0, "it wraps: {tip:?}");
    // The text's rightmost pixel, clear of the rounded corners: the padding (6) and a bearing
    // in from the edge, not the 16 of a box as wide as its text before wrapping.
    let img = h.render();
    let ink = |x: f32| {
      let ys = (12..(tip.h * 2.0) as i32 - 12).map(|y| tip.y + y as f32 / 2.0);
      ys.into_iter().any(|y| img.pixel(x, y)[0] < 150)
    };
    let xs = (4..(tip.w * 2.0) as i32 - 4).rev().map(|x| tip.x + x as f32 / 2.0);
    let right = xs.into_iter().find(|&x| ink(x)).unwrap();
    let gap = tip.x + tip.w - right;
    assert!(gap < 10.0, "{gap} from the text to the right edge: {tip:?}");
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
