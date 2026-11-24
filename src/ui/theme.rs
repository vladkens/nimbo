// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The theme the widgets ([super::widgets]) draw with: a palette, sizes and fonts, read from the
//! [super::Ui] (`ui.theme()`). A theme is a plain value with public fields: an app takes the
//! default one, changes what it wants and sets it, or builds its own.
//!
//! An app changes the look in three steps, from the least work to the most:
//! - a style where the widget has one:
//!   `ui.button_style(&BorderedProminentButtonStyle).button("Save")` ([super::Ui::button_style]);
//! - a style derived from the theme, changed for one widget: `FieldStyle { text: mono, ..t.field()
//!   }` given to [super::Ui::selectable_text];
//! - its own theme: `const MINE: Theme = Theme { accent: rgb(0xe06c75), ..Theme::PLATES_DARK }` and
//!   `ui.set_theme(MINE)`.
//!
//! An app that wants a widget to look different altogether writes a style of its own
//! ([super::ButtonStyle], [super::TextFieldStyle]) or draws its own over the widget's behaviour
//! ([super::behavior]).
//!
//! The default look is macOS 26 System Settings' neutral greys, as measured from its
//! screenshots: [Theme::PLATES_LIGHT] and [Theme::PLATES_DARK], borderless translucent grey
//! buttons, subtle hovers on every control ([Theme::hover_all]; macOS hovers only some). Until
//! the app sets a theme, it follows the OS ([Theme::system]): light or dark, with the user's
//! accent, switching live when they change; [super::Ui::follow_system] goes back to it.

use std::time::Duration;

pub use super::ControlSize;
use super::store::Transition;
use super::{
  Align, EnvironmentValue, FieldStyle, FocusRing, Shadow, Style, TextStyle, Ui, grow, px,
};
use crate::app::{Appearance, ColorScheme};
use crate::scene::{Color, alpha, mix, rgb};
use crate::text::{Font, Weight};

const WHITE: Color = [1.0; 4];
const BLACK: Color = [0.0, 0.0, 0.0, 1.0];

/// A palette, sizes and fonts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
  pub scheme: ColorScheme,
  /// The window's ground.
  pub bg: Color,
  /// Sidebars, toolbars, tab strips.
  pub panel: Color,
  /// Controls: buttons.
  pub raised: Color,
  /// A raised control under the pointer.
  pub hover: Color,
  /// A raised control held down.
  pub pressed: Color,
  /// Borders and dividers.
  pub line: Color,
  pub text: Color,
  /// Secondary text: hints, counts, section titles.
  pub muted: Color,
  pub accent: Color,
  /// Text and marks on the accent (or another tone's fill).
  pub on_accent: Color,
  pub danger: Color,
  pub success: Color,
  pub warning: Color,
  /// Selected rows of a list with focus.
  pub selection: Color,
  /// The focus ring.
  pub focus: Color,
  /// A translucent tile under the pointer: ghost buttons.
  pub ghost: Color,
  /// Under menus.
  pub shadow: Color,
  /// Menus, and their edge ([Sizes::menu_edge] wide).
  pub menu: Color,
  pub menu_line: Color,
  /// Grouped content on the ground: a form's plates.
  pub plate: Color,
  /// The third tier of text: disabled labels and values, a disabled pop-up's chevrons.
  pub tertiary: Color,
  /// Links ([super::Ui::link] under the pointer).
  pub link: Color,
  /// A switch's track when off, a slider's and a progress bar's, and under the pointer.
  pub track: Color,
  pub track_hover: Color,
  /// Switch and slider knobs.
  pub knob: Color,
  /// A modal dialog's card ([Theme::dialog]): white on light grounds, a grey a step above the
  /// window on dark ones, as macOS's alerts.
  pub dialog: Color,
  /// A tooltip's fill and text.
  pub tip: Color,
  pub tip_text: Color,
  /// The border of push buttons: none on plates, where a grey fill is the button; a theme with
  /// one draws check boxes and segments bordered too ([Theme::flat]).
  pub control_border: ControlBorder,
  /// Corners of controls.
  pub radius: f32,
  /// How big controls are.
  pub sizes: Sizes,
  pub font: Font,
  /// Section titles, a dialog's title.
  pub bold: Font,
  pub small: Font,
  /// Hover fades, Sublime's `{"speed": 6.0, "interpolation": "smoothstep"}` (units per
  /// second: 0 to 1 in 170 ms). macOS switches states within a frame; the plates themes keep
  /// the fade.
  pub fade: Transition,
  /// Every control shows a hover state. Off, only the hovers macOS draws remain: ghost
  /// (toolbar) buttons. The pointing hand shows either way.
  pub hover_all: bool,
  /// Opacity of disabled widgets.
  pub disabled: f32,
  /// The ground of text fields, and their edge.
  pub field: Color,
  pub field_line: Color,
  /// Fainter than muted: placeholders.
  pub faint: Color,
  /// How long the pointer rests on an element before its tooltip shows, and how long after one
  /// hid the next shows at once, as the pointer moves along a row of icons.
  pub tip_delay: Duration,
  pub tip_warm: Duration,
  /// The shortcut recorder shows a ⊗ that clears it ([super::Ui::shortcut]); off in the
  /// plates themes, whose recorder sits by a reset button (Backspace clears while recording).
  pub shortcut_clear: bool,
  /// The selected toolbar tab is macOS 26's Liquid Glass tile in dark appearances
  /// ([super::form::Glass]); off, or in light ones, the raised tile ([Theme::tab_tile]).
  pub glass: bool,
}

/// Whether a theme's controls have a border ([Theme::control_border]), and how wide in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ControlBorder {
  /// Fills alone, as System Settings' controls.
  None,
  Hairline(f32),
}

impl ControlBorder {
  /// Its width, none's 0.
  pub const fn width(self) -> f32 {
    if let ControlBorder::Hairline(w) = self { w } else { 0.0 }
  }
}

/// A theme's control metrics ([Theme::sizes]) in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizes {
  /// Push buttons.
  pub control_h: f32,
  /// Small buttons ([ControlSize::Small]): dense rows, inline actions.
  pub small_h: f32,
  /// A menu's items.
  pub item_h: f32,
  /// A tab strip.
  pub tab_h: f32,
  /// The least width of a pop-up button's menu.
  pub menu_w: f32,
  /// The corners of menus ([Theme::popup]) and the width of their edge.
  pub menu_radius: f32,
  pub menu_edge: f32,
  /// A check box's box.
  pub check: f32,
  /// A switch in rows ([ControlSize::Regular]) and the large one ([ControlSize::Large]: a
  /// header row's, by a big icon).
  pub switch: SwitchSize,
  pub switch_large: SwitchSize,
  /// A slider's knob, wide and high, and its track's thickness.
  pub knob: [f32; 2],
  pub rail: f32,
  /// A badge's text, and its pill's height (padded 7 at the sides).
  pub badge: f32,
  pub badge_h: f32,
}

/// A switch's size ([Sizes::switch]): its track and its pill knob, wide and high; the knob
/// sits as far in from the track's end as from its top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwitchSize {
  pub track: [f32; 2],
  pub knob: [f32; 2],
}

impl Sizes {
  /// macOS 26 System Settings': 24-point buttons in 13-point text, small ones 20 high; menus
  /// of 24-point items in corners of 13 with a half-point edge, a pop-up's 220 wide at least;
  /// 16-point check boxes, the small switch (36 × 16, a 21 × 13 knob) in rows and the regular
  /// one (44 × 20) by a big icon, a slider's 20 × 16 knob on a 6-point track, 18-point badges
  /// in 11-point text.
  pub const PLATES: Self = Self {
    control_h: 24.0,
    small_h: 20.0,
    item_h: 24.0,
    tab_h: 30.0,
    menu_w: 220.0,
    menu_radius: 13.0,
    menu_edge: 0.5,
    check: 16.0,
    switch: SwitchSize { track: [36.0, 16.0], knob: [21.0, 13.0] },
    switch_large: SwitchSize { track: [44.0, 20.0], knob: [26.0, 16.0] },
    knob: [20.0, 16.0],
    rail: 6.0,
    badge: 11.0,
    badge_h: 18.0,
  };
}

/// A colour role for badges ([Theme::tone]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
  #[default]
  Neutral,
  Accent,
  Success,
  Danger,
  Warning,
}

impl Theme {
  /// macOS 26 System Settings' neutral greys on white, as measured: translucent grey buttons
  /// (so they read one shade on a plate and another on the ground, as macOS's do), the system
  /// blue, and the [Sizes::PLATES] controls. The focus ring is `keyboardFocusIndicatorColor`'s,
  /// for the blue accent.
  pub const PLATES_LIGHT: Self = Self {
    scheme: ColorScheme::Light,
    bg: WHITE,
    panel: rgb(0xf8f8f8),
    raised: alpha(BLACK, 0.065),
    hover: alpha(BLACK, 0.09),
    pressed: alpha(BLACK, 0.145),
    line: rgb(0xe5e5e7),
    text: rgb(0x262626),
    muted: rgb(0x86868b),
    accent: rgb(0x007aff),
    on_accent: WHITE,
    danger: rgb(0xd70015),
    success: rgb(0x248a3d),
    warning: rgb(0xc45500),
    selection: alpha(rgb(0x007aff), 0.22),
    focus: alpha(rgb(0x0067f4), 0.45),
    ghost: alpha(BLACK, 0.05),
    shadow: alpha(BLACK, 0.2),
    menu: WHITE,
    menu_line: alpha(BLACK, 0.25),
    plate: rgb(0xf8f8f8),
    tertiary: rgb(0xbfbfbf),
    link: rgb(0x0068da),
    track: alpha(BLACK, 0.09),
    track_hover: alpha(BLACK, 0.12),
    knob: WHITE,
    dialog: WHITE,
    tip: rgb(0xf6f6f6),
    tip_text: rgb(0x545454),
    control_border: ControlBorder::None,
    radius: 6.0,
    sizes: Sizes::PLATES,
    font: Font::ui(13.0),
    bold: Font::ui(13.0).weight(Weight::Semibold),
    small: Font::ui(11.0),
    fade: Transition::smooth(6.0),
    hover_all: true,
    disabled: 0.4,
    field: WHITE,
    field_line: rgb(0xdadade),
    faint: rgb(0xb2b2b7),
    tip_delay: Duration::from_millis(200),
    tip_warm: Duration::from_millis(1000),
    shortcut_clear: false,
    glass: true,
  };

  /// [Theme::PLATES_LIGHT]'s roles on System Settings' dark greys; the focus ring is
  /// `keyboardFocusIndicatorColor`'s cyan-blue, not the accent.
  pub const PLATES_DARK: Self = Self {
    scheme: ColorScheme::Dark,
    bg: rgb(0x1e1e1e),
    panel: rgb(0x272727),
    raised: alpha(WHITE, 0.09),
    hover: alpha(WHITE, 0.12),
    pressed: alpha(WHITE, 0.2),
    line: rgb(0x3a3a3d),
    text: rgb(0xe1e1e1),
    muted: rgb(0xa6a6a6),
    accent: rgb(0x0a84ff),
    danger: rgb(0xff453a),
    success: rgb(0x30d158),
    warning: rgb(0xff9f0a),
    selection: alpha(rgb(0x0a84ff), 0.30),
    focus: alpha(rgb(0x1aa9ff), 0.5),
    ghost: alpha(WHITE, 0.07),
    shadow: alpha(BLACK, 0.5),
    menu: rgb(0x1a1a1a),
    menu_line: alpha(WHITE, 0.25),
    field: rgb(0x252527),
    field_line: rgb(0x424245),
    faint: rgb(0x646468),
    plate: rgb(0x272727),
    tertiary: rgb(0x646464),
    link: rgb(0x409cff),
    track: alpha(WHITE, 0.12),
    track_hover: alpha(WHITE, 0.15),
    knob: rgb(0xe4e4e4),
    dialog: rgb(0x2c2c2c),
    tip: rgb(0x383838),
    tip_text: rgb(0xededed),
    ..Self::PLATES_LIGHT
  };

  /// This theme with another accent, and the selection and the focus ring made from it.
  pub const fn with_accent(self, accent: Color) -> Self {
    let (selection, focus) = if self.scheme.is_dark() { (0.30, 0.55) } else { (0.22, 0.45) };
    Self { accent, selection: alpha(accent, selection), focus: alpha(accent, focus), ..self }
  }

  /// The theme for the OS's appearance: [Theme::PLATES_LIGHT] or [Theme::PLATES_DARK], with the
  /// OS's accent when it has one ([Theme::with_accent]); the theme's own for its own accent
  /// (within a level: the OS's blue), which keeps its measured focus ring. What the [super::Ui]
  /// draws with until the app sets its own ([super::Ui::set_theme]).
  pub fn system(a: Appearance) -> Self {
    let base = if a.scheme.is_dark() { Self::PLATES_DARK } else { Self::PLATES_LIGHT };
    let Some(accent) = a.accent else { return base };
    if (0..3).all(|i| (accent[i] - base.accent[i]).abs() < 1.5 / 255.0) {
      return base;
    }

    base.with_accent(accent)
  }

  /// This theme in a window that isn't key, as AppKit draws its controls there: the accent a
  /// grey, the ground 20% toward black (light) or white (dark), as a switch's track, a check
  /// box, a slider's fill and a selected segment are there (14–23%), the marks on it in the
  /// text's colour; [Ui::controls_theme] while the window isn't key.
  pub const fn inactive(self) -> Self {
    let grey = mix(self.bg, if self.scheme.is_dark() { WHITE } else { BLACK }, 0.2);
    Self { accent: grey, on_accent: self.text, ..self }
  }

  /// The accent under the pointer: 12% toward black (light) or white (dark).
  pub const fn accent_hover(&self) -> Color {
    mix(self.accent, if self.scheme.is_dark() { WHITE } else { BLACK }, 0.12)
  }

  /// Whether controls are fills without a border, as System Settings draws them
  /// ([ControlBorder::None]): check boxes filled with the track, a segmented control of raised
  /// segments.
  pub const fn flat(&self) -> bool {
    matches!(self.control_border, ControlBorder::None)
  }

  pub const fn tone(&self, tone: Tone) -> Color {
    match tone {
      Tone::Neutral => self.muted,
      Tone::Accent => self.accent,
      Tone::Success => self.success,
      Tone::Danger => self.danger,
      Tone::Warning => self.warning,
    }
  }

  /// A badge's ground for `tone`: the tone at 14% (light) or 22% (dark) over the plate.
  pub const fn tint(&self, tone: Tone) -> Color {
    mix(self.plate, self.tone(tone), if self.scheme.is_dark() { 0.22 } else { 0.14 })
  }

  /// The selected toolbar tab's raised tile, as in macOS's preference windows: white (light) or
  /// 14% toward the text over the ground (dark, without [Theme::glass]).
  pub const fn tab_tile(&self) -> Color {
    if self.scheme.is_dark() { mix(self.bg, self.text, 0.14) } else { WHITE }
  }

  /// One line of text, cut with "…" when it doesn't fit.
  pub const fn label(&self) -> TextStyle {
    TextStyle::new(self.font, self.text).line_limit(1)
  }

  /// Wrapping text.
  pub const fn body(&self) -> TextStyle {
    TextStyle::new(self.font, self.text)
  }

  /// Secondary text on one line: an unselected tab, a hint.
  pub const fn muted(&self) -> TextStyle {
    TextStyle::new(self.font, self.muted).line_limit(1)
  }

  /// A menu's box: the menu fill in its corners and edge, padded 5, over a soft shadow.
  pub const fn popup(&self) -> Style {
    Style::new()
      .pad(5.0)
      .bg(self.menu)
      .radius(self.sizes.menu_radius)
      .border(self.sizes.menu_edge, self.menu_line)
      .shadow(Shadow::new(4.0, 24.0, self.shadow))
  }

  /// A small semibold section title (write it in capitals): "LOCATIONS".
  pub const fn section(&self) -> TextStyle {
    TextStyle::new(self.small.weight(Weight::Semibold), self.muted).line_limit(1)
  }

  /// The padding at each side of a push button's label at `size` ([crate::Ui::control_size]):
  /// half its height, rounded, 12 at 24 points; with [Sizes::control_h], for lining things up
  /// with one.
  pub fn button_padding(&self, size: ControlSize) -> f32 {
    let z = self.sizes;
    let h = match size {
      ControlSize::Small => z.small_h,
      ControlSize::Regular => z.control_h,
      ControlSize::Large => z.control_h + 8.0,
    };

    (h * 0.5).round()
  }

  /// A modal dialog's card ([super::Dialog]), a macOS 26 alert's as measured: the
  /// [Theme::dialog] fill, corners of 25, padded 20 at the top and 16 elsewhere (its text 6
  /// further in), a soft shadow; on dark grounds the menu's edge.
  pub const fn dialog(&self) -> Style {
    let edge = if self.scheme.is_dark() { self.menu_line } else { [0.0; 4] };
    Style::new()
      .pads([20.0, 16.0, 16.0, 16.0])
      .bg(self.dialog)
      .radius(25.0)
      .border(self.sizes.menu_edge, edge)
      .shadow(Shadow::new(10.0, 36.0, self.shadow))
  }

  /// The backdrop under a modal dialog ([super::Ui::modal]): black at 20% (light) or 45%
  /// (dark).
  pub const fn scrim(&self) -> Color {
    alpha(BLACK, if self.scheme.is_dark() { 0.45 } else { 0.2 })
  }

  /// A tooltip's box, macOS's: the tip fill padded 6 by 3 (20 high for a line of small text),
  /// corners of 5, the menu's edge, a small shadow, at most 260 wide.
  pub const fn tip(&self) -> Style {
    Style::new()
      .pad_xy(6.0, 3.0)
      .bg(self.tip)
      .radius(5.0)
      .border(self.sizes.menu_edge, self.menu_line)
      .shadow(Shadow::new(1.0, 4.0, alpha(self.shadow, self.shadow[3] * 0.6)))
      .max_w(260.0)
  }

  pub const fn focus_ring(&self) -> FocusRing {
    FocusRing::new(self.focus)
  }

  /// A text field's look ([super::RoundedBorderTextFieldStyle]): [Sizes::control_h] high at
  /// least, the field's ground and edge in the control's corners, the faint placeholder, the
  /// accent's caret and a selection of it. Focused, the ring.
  pub const fn field(&self) -> FieldStyle {
    let frame = Style::new()
      .w(grow())
      .min_h(self.sizes.control_h)
      .pad_xy((self.sizes.control_h / 3.0).round(), 4.0)
      .align(Align::Start, Align::Center)
      .bg(self.field)
      .radius(self.radius)
      .border(1.0, self.field_line);
    let sel = alpha(self.accent, if self.scheme.is_dark() { 0.4 } else { 0.3 });
    FieldStyle::new(TextStyle::new(self.font, self.text))
      .frame(frame)
      .placeholder(self.faint)
      .selection(sel)
      .caret(self.accent)
  }

  /// A search field ([super::Ui::search]): [Theme::field]'s box [Sizes::control_h] + 4 high,
  /// padded 30 at the left for the magnifier and 10 at the right. Where controls are flat
  /// ([Theme::flat]), System Settings' sidebar field: the ground a touch lighter, a half-point
  /// edge, the prompt in the muted colour.
  pub const fn search(&self) -> FieldStyle {
    let f = self.field();
    let frame = f.frame.h(px(self.sizes.control_h + 4.0)).pads([0.0, 10.0, 0.0, 30.0]);
    if !self.flat() {
      return f.frame(frame);
    }

    let (fill, edge) = match self.scheme.is_dark() {
      true => (alpha(WHITE, 0.12), alpha(BLACK, 0.5)),
      false => (alpha(WHITE, 0.3), alpha(BLACK, 0.15)),
    };

    f.frame(frame.bg(fill).border(0.5, edge)).placeholder(self.muted)
  }
}

/// The OS's for the window's appearance ([Theme::system]) where neither the app nor an element
/// put one.
impl EnvironmentValue for Theme {
  fn default_value(ui: &Ui) -> Self {
    Theme::system(ui.appearance())
  }
}

impl Ui<'_> {
  /// The theme widgets draw with, a copy: the environment's ([Ui::environment_value]), the one
  /// the app set ([Ui::set_theme]) unless an element around puts its own, else the OS's
  /// ([Theme::system]).
  pub fn theme(&self) -> Theme {
    self.environment_value()
  }

  /// The theme of controls filled with the accent: [Ui::theme], its accent a grey while the
  /// window isn't key, as AppKit's switches, check boxes, sliders, segments and progress bars
  /// turn grey there. For a control of the app's own that should do the same.
  pub fn controls_theme(&self) -> Theme {
    let t = self.theme();
    if self.window_focused() { t } else { t.inactive() }
  }

  /// Sets the theme, kept until changed, over the OS's appearance, and its light or dark
  /// ([Ui::color_scheme], which scroll areas' knobs follow); its focus ring is the one the shell
  /// puts after each view, unless the view sets its own ([Ui::set_focus_ring]).
  pub fn set_theme(&mut self, theme: Theme) {
    self.set_environment(Some(theme));
    self.set_environment(Some(theme.scheme));
  }

  /// Follows the OS's appearance again ([Theme::system]), the default: light or dark with
  /// its accent, switching when it changes.
  pub fn follow_system(&mut self) {
    self.set_environment::<Theme>(None);
    self.set_environment::<ColorScheme>(None);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::{App, Event};
  use crate::harness::Harness;
  use crate::scene::Size;

  #[test]
  fn the_system_theme_follows_the_os_accent() {
    assert_eq!(Theme::system(Appearance::LIGHT), Theme::PLATES_LIGHT);
    assert_eq!(Theme::system(Appearance::DARK), Theme::PLATES_DARK);
    let purple = Appearance { scheme: ColorScheme::Light, accent: Some(rgb(0x953d96)) };
    let t = Theme::system(purple);
    assert_eq!((t.bg, t.accent, t.sizes), (WHITE, rgb(0x953d96), Sizes::PLATES));

    // The focus ring follows another accent.
    assert_eq!(t.focus, alpha(rgb(0x953d96), 0.45));
    // The OS's own blue is the theme's: its measured focus ring stays.
    let blue = Appearance { scheme: ColorScheme::Dark, accent: Some(rgb(0x0a84ff)) };
    assert_eq!(Theme::system(blue), Theme::PLATES_DARK);
    let px = |c: Color| c.map(|v| (v * 255.0).round() as u8);
    assert_eq!(px(Theme::PLATES_LIGHT.accent_hover()), [0, 107, 224, 255]);
    // Links stay the system's blue.
    assert_eq!(t.link, rgb(0x0068da));
    // The derived colours: a tab tile, a badge's tint.
    assert_eq!(px(Theme::PLATES_LIGHT.tab_tile()), [255, 255, 255, 255]);
    assert_eq!(px(Theme::PLATES_DARK.tab_tile()), [57, 57, 57, 255]);
    assert_eq!(px(Theme::PLATES_LIGHT.tint(Tone::Success)), [218, 233, 222, 255]);
  }

  /// The translucent fills read what System Settings shows on the window's ground, within a
  /// level.
  #[test]
  fn the_plates_fills_read_as_measured() {
    let over = |c: Color, g: Color| -> [u8; 3] {
      std::array::from_fn(|i| ((g[i] * (1.0 - c[3]) + c[i] * c[3]) * 255.0).round() as u8)
    };
    let near = |a: [u8; 3], b: u32| {
      let b = [(b >> 16) as u8, (b >> 8) as u8, b as u8];
      (0..3).all(|i| a[i].abs_diff(b[i]) <= 1)
    };
    for (t, raised, white, pressed, track) in [
      (Theme::PLATES_LIGHT, 0xe7e7e7, 0xefefef, 0xdadada, 0xe2e2e2),
      (Theme::PLATES_DARK, 0x3a3a3a, 0x333333, 0x4b4b4b, 0x404040),
    ] {
      let (plate, ground) = (t.plate, t.bg);
      assert!(near(over(t.raised, plate), raised), "{:?}", over(t.raised, plate));
      assert!(near(over(t.raised, ground), white), "{:?}", over(t.raised, ground));
      assert!(near(over(t.pressed, ground), pressed), "{:?}", over(t.pressed, ground));
      assert!(near(over(t.track, plate), track), "{:?}", over(t.track, plate));
    }
  }

  /// Pins a theme or follows the OS's, and sets a focus ring of its own or doesn't, each frame.
  #[derive(Default)]
  struct Ringed {
    pin: Option<Theme>,
    own: Option<Option<FocusRing>>,
    /// The colour scheme as the view last saw it.
    scheme: Option<ColorScheme>,
  }

  impl App for Ringed {
    fn view(&mut self, ui: &mut Ui) {
      match self.pin {
        Some(t) => ui.set_theme(t),
        None => ui.follow_system(),
      }

      if let Some(r) = self.own {
        ui.set_focus_ring(r);
      }

      self.scheme = Some(ui.color_scheme());
    }
  }

  /// The theme's focus ring rings the focused box and follows the OS's appearance; one the app
  /// sets in a frame overrides it there, whatever the theme, and the theme's comes back when the
  /// app stops setting it.
  #[test]
  fn the_focus_ring_is_the_themes_unless_the_app_sets_one() {
    let mut h = Harness::new(Ringed::default(), Size::new(40.0, 40.0), 1.0);
    let ring = |h: &mut Harness<Ringed>| {
      h.rt.redraw();
      h.frame();
      h.rt.ui.own_ring.unwrap_or(h.rt.ui.ring)
    };
    assert_eq!(ring(&mut h), Some(Theme::PLATES_DARK.focus_ring()));
    h.event(Event::Appearance(Appearance::LIGHT));
    assert_eq!(ring(&mut h), Some(Theme::PLATES_LIGHT.focus_ring()));

    // The app's theme, then a ring of its own after it, then none.
    let mine = Theme { focus: rgb(0xff0000), ..Theme::PLATES_DARK };
    h.app_mut().pin = Some(mine);
    assert_eq!(ring(&mut h), Some(mine.focus_ring()));
    let green = FocusRing::new(rgb(0x00ff00));
    h.app_mut().own = Some(Some(green));
    assert_eq!(ring(&mut h), Some(green));
    h.event(Event::Appearance(Appearance::DARK));
    assert_eq!(ring(&mut h), Some(green));
    h.app_mut().own = Some(None);
    assert_eq!(ring(&mut h), None);

    // The theme set again, the OS's followed again.
    h.app_mut().own = None;
    assert_eq!(ring(&mut h), Some(mine.focus_ring()));
    h.app_mut().pin = None;
    assert_eq!(ring(&mut h), Some(Theme::PLATES_DARK.focus_ring()));
    h.event(Event::Appearance(Appearance::LIGHT));
    assert_eq!(ring(&mut h), Some(Theme::PLATES_LIGHT.focus_ring()));
  }

  /// A theme the app set gives its light or dark (scroll areas' knobs follow it); following the
  /// OS again gives the OS's.
  #[test]
  fn the_color_scheme_is_the_themes_or_the_oss() {
    let mut h = Harness::new(Ringed::default(), Size::new(40.0, 40.0), 1.0);
    h.event(Event::Appearance(Appearance::LIGHT));
    let scheme = |h: &mut Harness<Ringed>| {
      h.rt.redraw();
      h.frame();
      h.app().scheme
    };
    assert_eq!(scheme(&mut h), Some(ColorScheme::Light));
    h.app_mut().pin = Some(Theme::PLATES_DARK);
    assert_eq!(scheme(&mut h), Some(ColorScheme::Dark));
    h.app_mut().pin = None;
    assert_eq!(scheme(&mut h), Some(ColorScheme::Light));
  }
}
