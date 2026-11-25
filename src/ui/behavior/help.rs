// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The tooltip over its timing and placement, drawn by the window's [HelpStyle]: a line or two of
//! text under the pointer resting on an element with [Ui::help]; [PlainHelpStyle], a box of the
//! colour scheme's, unless the shell put its own.
//!
//! ```ignore
//! struct Tag;
//!
//! impl HelpStyle for Tag {
//!   fn make_body(&self, ui: &mut Ui, c: HelpConfiguration) {
//!     let s = Style::new().pad_xy(8.0, 4.0).radius(4.0).bg(INK).opacity(c.opacity());
//!     ui.row(s, |ui| ui.text(c.text(), TextStyle::new(Font::ui(12.0), PAPER)));
//!   }
//! }
//!
//! ui.help_style(&Tag);
//! ```

use std::time::Duration;

use crate::scene::Point;
use crate::text::Font;
use crate::ui::{EnvironmentValue, Float, Id, Style, TextStyle, Transition, Ui, UnitPoint};

/// How long the pointer rests on an element before its tooltip shows, as macOS's.
const REST: Duration = Duration::from_millis(200);
/// How long after one showed the next shows at once, as the pointer moves along a row of icons.
const WARM: Duration = Duration::from_millis(1000);
/// Paint order of tooltips among overlays: above menus and dialogs.
const TIP_Z: i32 = 2000;
/// A tooltip fades in and out over 125 ms.
const TIP_FADE: Transition = Transition::linear(8.0);

/// How a tooltip looks: [HelpStyle::make_body] builds its box, the tip's root, which the window
/// has placed and keeps from taking the pointer, and its text in it. Chosen for the window with
/// [Ui::help_style] as `&'static`, a unit struct's `&MyStyle`.
pub trait HelpStyle: 'static {
  /// Builds the tooltip `config` describes.
  fn make_body(&self, ui: &mut Ui<'_>, config: HelpConfiguration<'_>);
}

/// What a [HelpStyle] draws: the tip of the element the pointer rests on, this frame.
#[non_exhaustive]
#[derive(Clone, Copy, Debug)]
pub struct HelpConfiguration<'s> {
  target: Id,
  text: &'s str,
  opacity: f32,
  anchor: Point,
}

impl<'s> HelpConfiguration<'s> {
  /// The element the tip is for; the tip's own id is `target.key("tip")`.
  pub fn target(&self) -> Id {
    self.target
  }

  pub fn text(&self) -> &'s str {
    self.text
  }

  /// The fade in and out, 0 to 1: the box's opacity.
  pub fn opacity(&self) -> f32 {
    self.opacity
  }

  /// The pointer where it rested when the tip showed, which the tip hangs from: its left edge
  /// there and its top 19 below, moved into the window near its edges.
  pub fn anchor(&self) -> Point {
    self.anchor
  }
}

/// When a tooltip shows, from the pointer coming onto its target, and where the pointer was
/// then.
#[derive(Default)]
struct Tip {
  due: Option<Duration>,
  at: Option<Point>,
}

/// Until when the next tooltip shows at once ([WARM]), across targets.
#[derive(Default)]
struct Warm(Duration);

impl Ui<'_> {
  /// Shows `text` under the pointer once it has rested on `target` for 200 ms, or at once within
  /// a second of another tooltip showing, until it leaves or a button goes down. As macOS places
  /// one: its left edge at the pointer, its top 19 below it, where the pointer was when it
  /// showed; moved into the window near its edges. Run after the view for each element
  /// [Ui::take_tooltips] hands out, which made `text` the target's description for assistive
  /// technology; the tip is `target.key("tip")`, an overlay that doesn't take the pointer, drawn
  /// by the window's [HelpStyle].
  pub(crate) fn tooltip(&mut self, target: Id, text: &str) {
    let tip = target.key("tip");
    let over = self.hovered(target) && self.press().is_none();
    let (now, mouse, size) = (self.now(), self.mouse(), self.size());
    let warm = now < self.state::<Warm>(Id::of("nimbo.tip")).0;
    let st = self.state::<Tip>(tip);
    if !over {
      (st.due, st.at) = (None, None);
      return;
    }

    let due = *st.due.get_or_insert(if warm { now } else { now + REST });
    if now < due {
      self.animate(tip, 0.0, TIP_FADE);
      self.request_frame_in(due - now);
      return;
    }

    let Some(p) = st.at.or(mouse) else { return };
    st.at = Some(p);
    self.state::<Warm>(Id::of("nimbo.tip")).0 = now + WARM;
    let opacity = self.animate(tip, 1.0, TIP_FADE);
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

    let at = Float::window().anchor(UnitPoint::new(0.0, ay)).offset(x, y).z(TIP_Z);
    let style = self.environment_value::<&dyn HelpStyle>();
    let config = HelpConfiguration { target, text, opacity, anchor: p };
    let root = self.st.tree.nodes.len();
    style.make_body(self.with_id(tip).allows_hit_testing(false).float(at), config);
    debug_assert!(
      self.st.tree.nodes.get(root).is_some_and(|n| n.id == tip),
      "a help style builds the tip's root first"
    );
  }

  /// Draws the window's tooltips ([Ui::help]) in `style`, in place of the shell's (the styled
  /// `TooltipHelpStyle`); kept across frames, as [Ui::set_environment]. The window's only: tips
  /// are built after the view, over all of it, where no element's values reach, so
  /// `ui.environment(style)` on a container changes none of the tips in it.
  pub fn help_style(&mut self, style: &'static dyn HelpStyle) {
    self.set_environment(Some(style));
  }
}

/// A box of the colour scheme's, as plain as a tooltip goes: white or a dark grey with a faint
/// edge, 11-point text in the label colour, padded 6 by 3, at most 260 wide. The default under
/// the shell's.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlainHelpStyle;

impl HelpStyle for PlainHelpStyle {
  fn make_body(&self, ui: &mut Ui<'_>, c: HelpConfiguration<'_>) {
    let (ink, bg) = if ui.color_scheme().is_dark() { (1.0, 0.16) } else { (0.0, 1.0) };
    let text = TextStyle::new(Font::ui(11.0), [ink, ink, ink, 0.85]);
    let s = Style::new().pad_xy(6.0, 3.0).radius(4.0).bg([bg, bg, bg, 1.0]).max_w(260.0);
    let s = s.border(1.0, [ink, ink, ink, 0.15]).opacity(c.opacity());
    ui.row(s, |ui| ui.text(c.text(), text));
  }
}

/// [PlainHelpStyle] where neither the window nor the shell put one.
impl EnvironmentValue for &'static dyn HelpStyle {
  fn default_value(_: &Ui) -> Self {
    &PlainHelpStyle
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::{App, Appearance};
  use crate::harness::Harness;
  use crate::scene::Size;
  use crate::ui::{UiState, px};

  /// A red tip of the app's, 30 by 10, with no text.
  struct Ink;

  impl HelpStyle for Ink {
    fn make_body(&self, ui: &mut Ui<'_>, c: HelpConfiguration<'_>) {
      let s = Style::new().w(px(30.0)).h(px(10.0));
      ui.el(s.bg([1.0, 0.0, 0.0, 1.0]).opacity(c.opacity()));
    }
  }

  /// A button helped in the window's style, as `.0` puts it, or in a container's.
  struct Helped(bool);

  impl App for Helped {
    fn view(&mut self, ui: &mut Ui) {
      match self.0 {
        true => ui.help_style(&Ink),
        false => _ = ui.set_environment::<&dyn HelpStyle>(None),
      }

      ui.environment::<&dyn HelpStyle>(&Ink).col(Style::new(), |ui| {
        ui.help("Saves it").id("a").button("A");
      });
    }
  }

  /// The window's style draws the tip, placed under the pointer, and keeps its description;
  /// one put on a container doesn't reach the tips in it, built after the view.
  #[test]
  fn the_windows_style_draws_the_tips() {
    let mut h = Harness::new(Helped(true), Size::new(200.0, 100.0), 1.0);
    h.frame();
    let r = h.rect(Id::of("a")).unwrap();
    let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    h.move_to(x, y);
    h.wait(Duration::from_millis(400));
    let tip = h.rect(Id::of("a").key("tip")).unwrap();
    assert!((tip.x - x).abs() <= 0.5 && (tip.y - y - 19.0).abs() <= 0.5, "{tip:?}");
    assert_eq!((tip.w, tip.h), (30.0, 10.0));
    assert_eq!(h.render().pixel(tip.x + 15.0, tip.y + 5.0), [255, 0, 0, 255]);
    assert!(h.a11y().contains(r#"button "A" desc "Saves it""#), "{}", h.a11y());

    h.app_mut().0 = false;
    h.rt.redraw();
    h.frame();
    let tip = h.rect(Id::of("a").key("tip")).unwrap();
    assert_ne!((tip.w, tip.h), (30.0, 10.0));
    assert_eq!(h.text(Id::of("a").key("tip").nth(0)), Some("Saves it"));
  }

  /// With no shell, plain: the colour scheme's box, white in the light one, corners of 4.
  #[test]
  fn plain_is_the_default_without_the_shell() {
    let mut st = UiState::default();
    st.set_appearance(Appearance::LIGHT);
    let mut ui = Ui::new(st, Size::new(100.0, 100.0), None, Duration::ZERO);
    let style = ui.environment_value::<&dyn HelpStyle>();
    let anchor = Point::new(0.0, 0.0);
    let c = HelpConfiguration { target: Id::of("a"), text: "Tip", opacity: 1.0, anchor };
    style.make_body(ui.id("tip"), c);
    let n = ui.st.tree.nodes.iter().find(|n| n.id == Id::of("tip")).unwrap();
    assert_eq!((n.style.bg, n.style.radii), ([1.0; 4], [4.0; 4]));
  }
}
