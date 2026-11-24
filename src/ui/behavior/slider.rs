// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The slider: its behaviour ([Ui::slider_behavior]), a value in a range set by a drag along its
//! track or by the arrows, free or in steps; and the [Slider] over it, drawn by the environment's
//! [SliderStyle] ([Ui::slider_style]) from a [SliderConfiguration]; [PlainSliderStyle], a line
//! and a knob, unless the shell put its own.
//!
//! ```ignore
//! struct Meter;
//!
//! impl SliderStyle for Meter {
//!   fn make_body(&self, ui: &mut Ui, c: SliderConfiguration) {
//!     let k = c.fraction();
//!     c.root(ui).canvas(Style::new().w(grow()).h(px(6.0)), move |f, r| {
//!       f.rect(r, TRACK, 3.0);
//!       f.rect(Rect::new(r.x, r.y, r.w * k, r.h), FILL, 3.0);
//!     });
//!   }
//! }
//!
//! ui.slider_style(&Meter).slider(&mut self.volume, 0.0..=1.0);
//! ```

use std::ops::RangeInclusive;

use crate::scene::Rect;
use crate::ui::{Behavior, EnvironmentValue, Id, Response, Role, Style, Ui, Value, grow, px};

impl Ui<'_> {
  /// A horizontal slider over `range` on the element `id`, its track the element's box inside
  /// its horizontal padding in the last frame (half a knob, so the knob's centre stops at the
  /// ends): a press, a click or a drag sets `v` from the pointer, Right and Up raise it while
  /// focused, Left and Down lower it; with a `step` ([Slider::step]) onto the step nearest from
  /// the range's start and by one step, else anywhere and by a twentieth of the range. `changed`
  /// when `v` moved.
  pub fn slider_behavior(
    &mut self,
    id: Id,
    v: &mut f32,
    range: RangeInclusive<f32>,
    step: Option<f32>,
  ) -> Behavior {
    let (lo, hi) = (*range.start(), *range.end());
    // A step as [Slider::step] takes it: none unless finite and positive.
    let step = step.filter(|s| s.is_finite() && *s > 0.0);
    let b = match self.begin(id, Role::Slider) {
      Some(b) => b,
      None => self.slide(id, v, (lo, hi), step),
    };

    let (now, min, max) = (f64::from(*v), f64::from(lo), f64::from(hi));
    self.a11y(id, Role::Slider).value(Value::Number { now, min, max });
    b
  }

  /// [Ui::slider_behavior]'s input, enabled.
  fn slide(&mut self, id: Id, v: &mut f32, (lo, hi): (f32, f32), step: Option<f32>) -> Behavior {
    let was = *v;
    let snap = |v: f32| snap(v, lo, hi, step);

    // Held, or released since the view last ran: a click, a drag's last move.
    let at = self.mouse().filter(|_| self.held(id)).or(self.released(id).map(|(_, p)| p));
    if let (Some((r, [_, right, _, left])), Some(m)) =
      (self.st.last.get(id).map(|n| (n.rect, n.style.pad)), at)
    {
      // The track: its root's box inside its horizontal padding.
      let k = ((m.x - r.x - left) / (r.w - (left + right)).max(1.0)).clamp(0.0, 1.0);
      *v = snap(lo + (hi - lo) * k);
    }

    // Right and Up raise it, as AppKit's slider.
    let [x, y] = self.arrows(id);
    if x - y != 0 {
      *v = snap(snap(*v) + step.unwrap_or((hi - lo) / 20.0) * (x - y) as f32);
    }

    Behavior { changed: *v != was, ..self.pointer_state(id) }
  }
}

/// `v` within `lo..=hi`, onto the nearest of its steps from `lo` with a `step`.
fn snap(v: f32, lo: f32, hi: f32, step: Option<f32>) -> f32 {
  let v = match step {
    Some(s) => lo + ((v - lo) / s).round().clamp(0.0, last_step(lo, hi, s)) * s,
    None => v,
  };
  // Against f32 drift on the last step.
  v.clamp(lo.min(hi), hi.max(lo))
}

/// The index of the last step of `s` from `lo` within `lo..=hi`: 0 when the range is shorter.
fn last_step(lo: f32, hi: f32, s: f32) -> f32 {
  ((hi - lo) / s + 1e-4).floor().max(0.0)
}

/// How a slider looks: [SliderStyle::make_body] builds it, its root ([SliderConfiguration::root])
/// under its id, with the focus and the pointing hand the behaviour gives it, the element pressed
/// and dragged along. Chosen with [Ui::slider_style] as `&'static`, a unit struct's `&MyStyle`.
pub trait SliderStyle: 'static {
  /// Builds the slider `config` describes.
  fn make_body(&self, ui: &mut Ui<'_>, config: SliderConfiguration<'_>);
}

/// What [Slider::tick_label] takes.
type TickLabel<'s> = Box<dyn Fn(f32) -> Option<String> + 's>;

/// What a [SliderStyle] draws: the slider's value, range and steps, and its state this frame.
#[non_exhaustive]
pub struct SliderConfiguration<'s> {
  b: Behavior,
  value: f32,
  range: (f32, f32),
  step: Option<f32>,
  label: Option<TickLabel<'s>>,
}

impl SliderConfiguration<'_> {
  /// The slider's id, its root's: for the style's hover fade, its width in the last frame.
  pub fn id(&self) -> Id {
    self.b.id
  }

  /// Its value, the pointer's and the keys' this frame applied.
  pub fn value(&self) -> f32 {
    self.value
  }

  pub fn range(&self) -> RangeInclusive<f32> {
    self.range.0..=self.range.1
  }

  /// The distance between its values ([Slider::step]).
  pub fn step(&self) -> Option<f32> {
    self.step
  }

  /// Where the value is along the range, 0 to 1: the knob's place on the track.
  pub fn fraction(&self) -> f32 {
    let (lo, hi) = self.range;
    if hi != lo { ((self.value - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.0 }
  }

  /// Each step's value and its place along the track, 0 to 1, from the range's start to its last
  /// step: for tick marks. None with no step, one where the step is longer than the range.
  pub fn steps(&self) -> impl ExactSizeIterator<Item = (f32, f32)> + use<> {
    let ((lo, hi), s) = (self.range, self.step.unwrap_or(1.0));
    let n = self.step.map_or(0, |s| (last_step(lo, hi, s) as usize).saturating_add(1));
    let place = move |i: f32| if hi != lo { s * i / (hi - lo) } else { 0.0 };
    (0..n).map(move |i| (lo + s * i as f32, place(i as f32)))
  }

  /// The label under the step of value `v` ([Slider::tick_label]), if it has one.
  pub fn tick_label(&self, v: f32) -> Option<String> {
    self.label.as_ref().and_then(|f| f(v))
  }

  /// A button went down on it and is held, wherever the pointer is: a drag.
  pub fn is_held(&self) -> bool {
    self.b.held
  }

  pub fn is_hovered(&self) -> bool {
    self.b.hovered
  }

  /// Focused by the keyboard in the key window: the core rings its root.
  pub fn is_focused(&self) -> bool {
    self.b.focused
  }

  /// Not disabled ([Ui::disabled]): it reads input, and is drawn so.
  pub fn is_enabled(&self) -> bool {
    !self.b.disabled
  }

  /// Makes the next element the slider's root ([Ui::control]): under its id, taking focus by Tab
  /// and showing the pointing hand while enabled, pressed and dragged along; its box inside its
  /// horizontal padding is the track, where the knob's centre goes from the range's start to its
  /// end. Built once, first or inside what the style builds around it (labels over and under).
  pub fn root<'u, 'a>(&self, ui: &'u mut Ui<'a>) -> &'u mut Ui<'a> {
    ui.control(&self.b)
  }
}

/// A horizontal slider over a range, as SwiftUI's `Slider(value:in:step:)`, drawn by the
/// environment's [SliderStyle] ([Ui::slider_style]). [Ui::slider] is one with no step.
pub struct Slider<'s> {
  value: &'s mut f32,
  range: RangeInclusive<f32>,
  step: Option<f32>,
  label: Option<TickLabel<'s>>,
}

impl<'s> Slider<'s> {
  pub fn new(value: &'s mut f32, range: RangeInclusive<f32>) -> Self {
    Self { value, range, step: None, label: None }
  }

  /// The distance between its values from the range's start, as SwiftUI's `step:`: a press, a
  /// click or a drag lands on the step nearest the pointer, the arrows (and assistive
  /// technology's increment) move one step; the style marks the steps ([crate::LinearSliderStyle]
  /// with dots, as macOS's tick marks). The last step is the last that fits in the range: the
  /// range's end only when the step divides it.
  pub fn step(self, step: f32) -> Self {
    Self { step: Some(step).filter(|s| s.is_finite() && *s > 0.0), ..self }
  }

  /// A label under each step that `label` gives one for, as the labels of SwiftUI's
  /// `Slider(…, tick:)`: `|v| (v == 50.0).then(|| format!("{v}%"))`. A text, not
  /// [crate::Content]: the style places it by its width, measured before the layout
  /// ([crate::LinearSliderStyle] has the details).
  pub fn tick_label(self, label: impl Fn(f32) -> Option<String> + 's) -> Self {
    Self { label: Some(Box::new(label)), ..self }
  }

  /// Builds it under the id set for it: its behaviour, then the style's body.
  pub fn show(self, ui: &mut Ui) -> Response {
    let style = ui.environment_value::<&dyn SliderStyle>();
    let id = ui.take_id();
    let range = (*self.range.start(), *self.range.end());
    let b = ui.slider_behavior(id, self.value, self.range, self.step);
    let (value, step, label) = (*self.value, self.step, self.label);
    let root = ui.st.tree.nodes.len();
    // A style may build around its root: what it builds there never takes the root's id (its
    // place's when unnamed, or one a widget took for it).
    let outer = ui.pending_root.replace(id);
    style.make_body(ui, SliderConfiguration { b, value, range, step, label });
    ui.pending_root = outer;
    debug_assert!(
      ui.st.tree.nodes[root..].iter().any(|n| n.id == id),
      "a slider style builds the slider's root"
    );

    let mut r = ui.st.response(id);
    r.changed = b.changed;
    // Disabled, it reads no input: the tree's hit is under it all the same.
    if b.disabled {
      (r.hovered, r.pressed, r.focused) = (false, false, false);
    }

    r
  }
}

/// A line and a knob in the text colour of the colour scheme, with no theme: dimmed when
/// disabled; at least 160 points wide, as the theme's. The default under the shell's.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlainSliderStyle;

impl SliderStyle for PlainSliderStyle {
  fn make_body(&self, ui: &mut Ui<'_>, c: SliderConfiguration<'_>) {
    const KNOB: f32 = 12.0;
    // macOS's label colour, 85% of black or white, its tertiary's 25% disabled.
    let ink = if ui.color_scheme().is_dark() { 1.0 } else { 0.0 };
    let a = if c.is_enabled() { 0.85 } else { 0.25 };
    let (line, knob, k) = ([ink, ink, ink, a * 0.3], [ink, ink, ink, a], c.fraction());
    let s = Style::new().w(grow()).min_w(160.0).h(px(KNOB + 4.0)).pad_xy(KNOB / 2.0, 0.0);
    c.root(ui).canvas(s, move |f, r| {
      let (x0, w, cy) = (r.x + KNOB / 2.0, r.w - KNOB, r.y + r.h / 2.0);
      f.rect(Rect::new(x0, cy - 1.0, w, 2.0), line, 1.0);
      f.rect(Rect::new(x0 + w * k - KNOB / 2.0, cy - KNOB / 2.0, KNOB, KNOB), knob, KNOB / 2.0);
    });
  }
}

/// [PlainSliderStyle] where neither a container nor the window nor the shell put one.
impl EnvironmentValue for &'static dyn SliderStyle {
  fn default_value(_: &Ui) -> Self {
    &PlainSliderStyle
  }
}

impl Ui<'_> {
  /// A [Slider] over `range` with no step: `v` anywhere in it.
  pub fn slider(&mut self, v: &mut f32, range: RangeInclusive<f32>) -> Response {
    Slider::new(v, range).show(self)
  }

  /// Draws the next slider, or every slider built in the next container, in `style`:
  /// `ui.slider_style(&PlainSliderStyle).slider(&mut v, 0.0..=1.0)`.
  pub fn slider_style(&mut self, style: &'static dyn SliderStyle) -> &mut Self {
    self.environment(style)
  }
}

#[cfg(test)]
mod tests {
  use std::collections::HashSet;
  use std::time::Duration;

  use super::*;
  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::{Quad, Size};
  use crate::ui::{Cursor, UiState};
  use crate::{Modifiers, NamedKey};

  const LEFT: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
  const RIGHT: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

  /// A bar of the app's, 120 wide, its track 10 in from either end: green up to the value, blue
  /// after it, red when disabled; it never sets focus or a pointer.
  struct Bar;

  impl SliderStyle for Bar {
    fn make_body(&self, ui: &mut Ui<'_>, c: SliderConfiguration<'_>) {
      let (k, on) = (c.fraction(), c.is_enabled());
      let s = Style::new().w(px(120.0)).h(px(20.0)).pad_xy(10.0, 0.0);
      c.root(ui).canvas(s, move |f, r| {
        let x = r.x + 10.0 + 100.0 * k;
        let (l, rt) = if on { (LEFT, RIGHT) } else { ([1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]) };
        f.quad(Quad::new(Rect::new(r.x, r.y, x - r.x, r.h), l));
        f.quad(Quad::new(Rect::new(x, r.y, r.x + r.w - x, r.h), rt));
      });
    }
  }

  #[derive(Default)]
  struct Bars {
    off: bool,
    v: f32,
  }

  impl App for Bars {
    fn view(&mut self, ui: &mut Ui) {
      let ui = ui.slider_style(&Bar).disabled(self.off);
      ui.id("bar").slider(&mut self.v, 0.0..=1.0);
    }
  }

  #[test]
  fn a_style_draws_over_the_behaviour() {
    let mut h = Harness::new(Bars::default(), Size::new(200.0, 100.0), 1.0);
    h.frame();
    let r = h.rect(Id::of("bar")).unwrap();
    let (x0, y) = (r.x + 10.0, r.y + 10.0);
    let px = |h: &mut Harness<Bars>, x: f32| h.render().pixel(x, y);
    let rgb = |c: [f32; 4]| c.map(|v| (v * 255.0) as u8);
    assert_eq!((px(&mut h, x0 + 50.0), h.rt.cursor()), (rgb(RIGHT), Cursor::Arrow));

    // The hand over it; a click on its track, inside the padding, sets the value from there.
    h.move_to(x0 + 50.0, y);
    assert_eq!(h.rt.cursor(), Cursor::Pointer);
    h.click(x0 + 75.0, y);
    assert_eq!(h.app().v, 0.75);
    assert_eq!((px(&mut h, x0 + 70.0), px(&mut h, x0 + 80.0)), (rgb(LEFT), rgb(RIGHT)));
    h.click(r.x + 2.0, y);
    assert_eq!(h.app().v, 0.0, "the padding is past the track's start");

    // Focused by Tab, raised by Right a twentieth.
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focused(), Some(Id::of("bar")));
    h.key(NamedKey::ArrowRight, Modifiers::empty());
    assert_eq!(h.app().v, 0.05);
    assert!(h.a11y().contains("slider"), "{}", h.a11y());

    // Disabled: its own red, no hand, no focus, no input.
    h.app_mut().off = true;
    h.rt.redraw();
    assert_eq!(px(&mut h, x0 + 50.0), [255, 0, 0, 255]);
    assert_eq!(h.focused(), None);
    h.move_to(x0 + 51.0, y);
    assert_eq!(h.rt.cursor(), Cursor::Arrow);
    h.click(x0 + 90.0, y);
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!((h.focused(), h.app().v), (None, 0.05));
    assert!(h.a11y().contains("disabled"), "{}", h.a11y());
  }

  /// Sliders in a container given the bar style, and one after it.
  struct Cascade;

  impl App for Cascade {
    fn view(&mut self, ui: &mut Ui) {
      ui.slider_style(&Bar).col(Style::new(), |ui| {
        ui.id("a").slider(&mut 0.5, 0.0..=1.0);
        ui.row(Style::new(), |ui| _ = ui.id("b").slider(&mut 0.5, 0.0..=1.0));
      });
      ui.id("c").slider(&mut 0.5, 0.0..=1.0);
    }
  }

  #[test]
  fn a_containers_style_reaches_its_sliders_only() {
    let mut h = Harness::new(Cascade, Size::new(300.0, 200.0), 1.0);
    h.frame();
    let side = |n: &str| h.rect(Id::of(n)).map(|r| (r.w, r.h)).unwrap();
    assert_eq!((side("a"), side("b")), ((120.0, 20.0), (120.0, 20.0)));
    assert_ne!(side("c"), (120.0, 20.0));
  }

  /// With no shell, plain: a 16-point line and knob, its track 6 in from either end.
  #[test]
  fn plain_is_the_default_without_the_shell() {
    let mut ui = Ui::new(UiState::default(), Size::new(300.0, 100.0), None, Duration::ZERO);
    ui.id("s").slider(&mut 0.5, 0.0..=1.0);
    let n = ui.st.tree.nodes.iter().find(|n| n.id == Id::of("s")).unwrap();
    assert_eq!((n.style.size[1], n.style.pad), (px(16.0), [0.0, 6.0, 0.0, 6.0]));
  }

  /// A configuration's steps: each value and its place, to the last that fits.
  #[test]
  fn steps_reach_the_last_that_fits() {
    let c = |step| SliderConfiguration {
      b: Behavior::default(),
      value: 4.0,
      range: (0.0, 10.0),
      step,
      label: Some(Box::new(|v| (v == 4.0).then(|| "4".into()))),
    };
    let steps: Vec<_> = c(Some(4.0)).steps().collect();
    assert_eq!(steps, [(0.0, 0.0), (4.0, 0.4), (8.0, 0.8)]);
    assert_eq!((c(None).steps().len(), c(None).fraction()), (0, 0.4));
    assert_eq!((c(None).tick_label(4.0), c(None).tick_label(8.0)), (Some("4".into()), None));
    assert_eq!(c(Some(1e-20)).steps().len(), usize::MAX, "too many to count: as many as fit");
  }

  /// A bar in a column of its own, its root inside.
  struct Wrapped;

  impl SliderStyle for Wrapped {
    fn make_body(&self, ui: &mut Ui<'_>, c: SliderConfiguration<'_>) {
      // Another control first, unnamed, and a container taking its id.
      ui.slider_style(&PlainSliderStyle).slider(&mut 0.5, 0.0..=1.0);
      let id = ui.take_id();
      ui.with_id(id)
        .col(Style::new(), |ui| _ = c.root(ui).el(Style::new().w(px(100.0)).h(px(10.0))));
    }
  }

  /// Unnamed sliders whose style builds around their root: each has an id of its own.
  #[test]
  fn a_style_builds_around_an_unnamed_root() {
    let mut ui = Ui::new(UiState::default(), Size::new(300.0, 100.0), None, Duration::ZERO);
    ui.slider_style(&Wrapped).col(Style::new(), |ui| {
      ui.slider(&mut 0.5, 0.0..=1.0);
      ui.slider(&mut 0.5, 0.0..=1.0);
      // The place's id a widget took for it.
      let id = ui.take_id();
      ui.with_id(id).slider(&mut 0.5, 0.0..=1.0);
    });
    let ids: HashSet<_> = ui.st.tree.nodes.iter().map(|n| n.id).collect();
    assert_eq!(ids.len(), ui.st.tree.nodes.len());

    // One place each.
    let mut ui = Ui::new(UiState::default(), Size::new(300.0, 100.0), None, Duration::ZERO);
    ui.slider(&mut 0.5, 0.0..=1.0);
    assert_eq!(ui.child_count(), 1);
  }

  /// A step that doesn't divide the range ends it at its last step, from the pointer and from
  /// the keys alike; one longer than the range keeps its start; with none, anywhere in it.
  #[test]
  fn snaps_onto_steps_within_the_range() {
    let fours = |v| snap(v, 0.0, 10.0, Some(4.0));
    assert_eq!(
      [fours(5.0), fours(9.9), fours(10.0), fours(8.0 + 4.0), fours(-3.0)],
      [4.0, 8.0, 8.0, 8.0, 0.0]
    );
    assert_eq!(snap(9.0 + 3.0, 0.0, 10.0, Some(3.0)), 9.0);
    assert_eq!([snap(6.0, 0.0, 10.0, Some(20.0)), snap(10.0, 0.0, 10.0, Some(20.0))], [0.0, 0.0]);
    assert_eq!(
      [snap(30.0, 10.0, 90.0, Some(10.0)), snap(34.9, 10.0, 90.0, Some(10.0))],
      [30.0, 30.0]
    );
    assert_eq!(
      [snap(30.0, 0.0, 29.99, Some(10.0)), snap(25.0, 0.0, 29.99, Some(10.0))],
      [20.0, 20.0]
    );
    assert_eq!([last_step(0.0, 1.0, 0.1), snap(1.0, 0.0, 1.0, Some(0.1))], [10.0, 1.0]);
    assert_eq!([snap(4.2, 0.0, 10.0, None), snap(11.0, 0.0, 10.0, None)], [4.2, 10.0]);
  }
}
