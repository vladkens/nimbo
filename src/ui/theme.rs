// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The theme the widgets ([super::widgets]) draw with: a palette, sizes and fonts, read from the
//! [super::Ui] (`ui.theme()`). A theme is a plain value read by its roles (`t.accent()`): an app
//! takes the default one, changes what it wants (`with_` each role) and sets it.
//!
//! An app changes the look in three steps, from the least work to the most:
//! - a style where the widget has one:
//!   `ui.button_style(&BorderedProminentButtonStyle).button("Save")` ([super::Ui::button_style]);
//! - a style derived from the theme, changed for one widget: `t.body().font(mono)` given to
//!   [super::Ui::selectable_text];
//! - its own theme: `const MINE: Theme = Theme::PLATES_DARK.with_accent(rgb(0xe06c75))` and
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

/// A palette, sizes and fonts, each read by its role (`t.accent()`) and changed with `with_` it
/// (`Theme::PLATES_DARK.with_text(WHITE)`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
  pub(crate) scheme: ColorScheme,
  pub(crate) bg: Color,
  pub(crate) panel: Color,
  pub(crate) raised: Color,
  pub(crate) hover: Color,
  pub(crate) pressed: Color,
  pub(crate) line: Color,
  pub(crate) text: Color,
  pub(crate) secondary: Color,
  pub(crate) accent: Color,
  pub(crate) on_accent: Color,
  pub(crate) danger: Color,
  pub(crate) success: Color,
  pub(crate) warning: Color,
  pub(crate) selection: Color,
  pub(crate) focus: Color,
  pub(crate) ghost: Color,
  pub(crate) shadow: Color,
  pub(crate) menu: Color,
  pub(crate) menu_line: Color,
  pub(crate) plate: Color,
  pub(crate) tertiary: Color,
  pub(crate) link: Color,
  pub(crate) track: Color,
  pub(crate) track_hover: Color,
  pub(crate) knob: Color,
  pub(crate) dialog_fill: Color,
  pub(crate) tip_fill: Color,
  pub(crate) tip_text: Color,
  pub(crate) control_border: ControlBorder,
  pub(crate) radius: f32,
  pub(crate) sizes: Sizes,
  pub(crate) font: Font,
  pub(crate) bold: Font,
  pub(crate) small: Font,
  pub(crate) fade: Transition,
  pub(crate) hover_all: bool,
  pub(crate) disabled: f32,
  pub(crate) field_fill: Color,
  pub(crate) field_line: Color,
  pub(crate) faint: Color,
  pub(crate) hotkey_recorder_clear: bool,
  pub(crate) glass: bool,
}

impl Theme {
  pub const fn scheme(&self) -> ColorScheme {
    self.scheme
  }

  pub const fn with_scheme(self, scheme: ColorScheme) -> Self {
    Self { scheme, ..self }
  }

  /// The window's ground.
  pub const fn bg(&self) -> Color {
    self.bg
  }

  pub const fn with_bg(self, bg: Color) -> Self {
    Self { bg, ..self }
  }

  /// Sidebars, toolbars, tab strips.
  pub const fn panel(&self) -> Color {
    self.panel
  }

  pub const fn with_panel(self, panel: Color) -> Self {
    Self { panel, ..self }
  }

  /// Controls: buttons.
  pub const fn raised(&self) -> Color {
    self.raised
  }

  pub const fn with_raised(self, raised: Color) -> Self {
    Self { raised, ..self }
  }

  /// A raised control under the pointer.
  pub const fn hover(&self) -> Color {
    self.hover
  }

  pub const fn with_hover(self, hover: Color) -> Self {
    Self { hover, ..self }
  }

  /// A raised control held down.
  pub const fn pressed(&self) -> Color {
    self.pressed
  }

  pub const fn with_pressed(self, pressed: Color) -> Self {
    Self { pressed, ..self }
  }

  /// Borders and dividers.
  pub const fn line(&self) -> Color {
    self.line
  }

  pub const fn with_line(self, line: Color) -> Self {
    Self { line, ..self }
  }

  pub const fn text(&self) -> Color {
    self.text
  }

  pub const fn with_text(self, text: Color) -> Self {
    Self { text, ..self }
  }

  /// Secondary text: hints, counts, section titles.
  pub const fn secondary(&self) -> Color {
    self.secondary
  }

  pub const fn with_secondary(self, secondary: Color) -> Self {
    Self { secondary, ..self }
  }

  pub const fn accent(&self) -> Color {
    self.accent
  }

  /// Text and marks on the accent (or another tone's fill).
  pub const fn on_accent(&self) -> Color {
    self.on_accent
  }

  pub const fn with_on_accent(self, on_accent: Color) -> Self {
    Self { on_accent, ..self }
  }

  pub const fn danger(&self) -> Color {
    self.danger
  }

  pub const fn with_danger(self, danger: Color) -> Self {
    Self { danger, ..self }
  }

  pub const fn success(&self) -> Color {
    self.success
  }

  pub const fn with_success(self, success: Color) -> Self {
    Self { success, ..self }
  }

  pub const fn warning(&self) -> Color {
    self.warning
  }

  pub const fn with_warning(self, warning: Color) -> Self {
    Self { warning, ..self }
  }

  /// Selected rows of a list with focus.
  pub const fn selection(&self) -> Color {
    self.selection
  }

  pub const fn with_selection(self, selection: Color) -> Self {
    Self { selection, ..self }
  }

  /// The focus ring.
  pub const fn focus(&self) -> Color {
    self.focus
  }

  pub const fn with_focus(self, focus: Color) -> Self {
    Self { focus, ..self }
  }

  /// A translucent tile under the pointer: ghost buttons.
  pub const fn ghost(&self) -> Color {
    self.ghost
  }

  pub const fn with_ghost(self, ghost: Color) -> Self {
    Self { ghost, ..self }
  }

  /// Under menus.
  pub const fn shadow(&self) -> Color {
    self.shadow
  }

  pub const fn with_shadow(self, shadow: Color) -> Self {
    Self { shadow, ..self }
  }

  /// Menus, and their edge ([Sizes::menu_edge] wide).
  pub const fn menu(&self) -> Color {
    self.menu
  }

  pub const fn with_menu(self, menu: Color) -> Self {
    Self { menu, ..self }
  }

  pub const fn menu_line(&self) -> Color {
    self.menu_line
  }

  pub const fn with_menu_line(self, menu_line: Color) -> Self {
    Self { menu_line, ..self }
  }

  /// Grouped content on the ground: a form's plates.
  pub const fn plate(&self) -> Color {
    self.plate
  }

  pub const fn with_plate(self, plate: Color) -> Self {
    Self { plate, ..self }
  }

  /// The third tier of text: disabled labels and values, a disabled pop-up's chevrons.
  pub const fn tertiary(&self) -> Color {
    self.tertiary
  }

  pub const fn with_tertiary(self, tertiary: Color) -> Self {
    Self { tertiary, ..self }
  }

  /// Links ([super::Ui::link] under the pointer).
  pub const fn link(&self) -> Color {
    self.link
  }

  pub const fn with_link(self, link: Color) -> Self {
    Self { link, ..self }
  }

  /// A switch's track when off, a slider's and a progress bar's, and under the pointer.
  pub const fn track(&self) -> Color {
    self.track
  }

  pub const fn with_track(self, track: Color) -> Self {
    Self { track, ..self }
  }

  pub const fn track_hover(&self) -> Color {
    self.track_hover
  }

  pub const fn with_track_hover(self, track_hover: Color) -> Self {
    Self { track_hover, ..self }
  }

  /// Switch and slider knobs.
  pub const fn knob(&self) -> Color {
    self.knob
  }

  pub const fn with_knob(self, knob: Color) -> Self {
    Self { knob, ..self }
  }

  /// A modal dialog's card ([Theme::dialog]): white on light grounds, a grey a step above the
  /// window on dark ones, as macOS's alerts.
  pub const fn dialog_fill(&self) -> Color {
    self.dialog_fill
  }

  pub const fn with_dialog_fill(self, dialog_fill: Color) -> Self {
    Self { dialog_fill, ..self }
  }

  /// A tooltip's fill and text.
  pub const fn tip_fill(&self) -> Color {
    self.tip_fill
  }

  pub const fn with_tip_fill(self, tip_fill: Color) -> Self {
    Self { tip_fill, ..self }
  }

  pub const fn tip_text(&self) -> Color {
    self.tip_text
  }

  pub const fn with_tip_text(self, tip_text: Color) -> Self {
    Self { tip_text, ..self }
  }

  /// The border of push buttons: none on plates, where a grey fill is the button; a theme with
  /// one draws check boxes and segments bordered too ([Theme::flat]).
  pub const fn control_border(&self) -> ControlBorder {
    self.control_border
  }

  pub const fn with_control_border(self, control_border: ControlBorder) -> Self {
    Self { control_border, ..self }
  }

  /// Corners of controls.
  pub const fn radius(&self) -> f32 {
    self.radius
  }

  pub const fn with_radius(self, radius: f32) -> Self {
    Self { radius, ..self }
  }

  /// How big controls are.
  pub const fn sizes(&self) -> Sizes {
    self.sizes
  }

  pub const fn with_sizes(self, sizes: Sizes) -> Self {
    Self { sizes, ..self }
  }

  pub const fn font(&self) -> Font {
    self.font
  }

  pub const fn with_font(self, font: Font) -> Self {
    Self { font, ..self }
  }

  /// Section titles, a dialog's title.
  pub const fn bold(&self) -> Font {
    self.bold
  }

  pub const fn with_bold(self, bold: Font) -> Self {
    Self { bold, ..self }
  }

  pub const fn small(&self) -> Font {
    self.small
  }

  pub const fn with_small(self, small: Font) -> Self {
    Self { small, ..self }
  }

  /// Hover fades, Sublime's `{"speed": 6.0, "interpolation": "smoothstep"}` (units per
  /// second: 0 to 1 in 170 ms). macOS switches states within a frame; the plates themes keep
  /// the fade.
  pub const fn fade(&self) -> Transition {
    self.fade
  }

  pub const fn with_fade(self, fade: Transition) -> Self {
    Self { fade, ..self }
  }

  /// Every control shows a hover state. Off, only the hovers macOS draws remain: ghost
  /// (toolbar) buttons. The pointing hand shows either way.
  pub const fn hover_all(&self) -> bool {
    self.hover_all
  }

  pub const fn with_hover_all(self, hover_all: bool) -> Self {
    Self { hover_all, ..self }
  }

  /// Opacity of disabled widgets.
  pub const fn disabled(&self) -> f32 {
    self.disabled
  }

  pub const fn with_disabled(self, disabled: f32) -> Self {
    Self { disabled, ..self }
  }

  /// The ground of text fields, and their edge.
  pub const fn field_fill(&self) -> Color {
    self.field_fill
  }

  pub const fn with_field_fill(self, field_fill: Color) -> Self {
    Self { field_fill, ..self }
  }

  pub const fn field_line(&self) -> Color {
    self.field_line
  }

  pub const fn with_field_line(self, field_line: Color) -> Self {
    Self { field_line, ..self }
  }

  /// Fainter than [Theme::secondary]: placeholders.
  pub const fn faint(&self) -> Color {
    self.faint
  }

  pub const fn with_faint(self, faint: Color) -> Self {
    Self { faint, ..self }
  }

  /// The hotkey recorder shows a ⊗ that clears it ([super::Ui::hotkey_recorder]); off in the
  /// plates themes, whose recorder sits by a reset button (Backspace clears while recording).
  pub const fn hotkey_recorder_clear(&self) -> bool {
    self.hotkey_recorder_clear
  }

  pub const fn with_hotkey_recorder_clear(self, hotkey_recorder_clear: bool) -> Self {
    Self { hotkey_recorder_clear, ..self }
  }

  /// The selected toolbar tab is macOS 26's Liquid Glass tile in dark appearances
  /// ([super::form::Glass]); off, or in light ones, the raised tile ([Theme::tab_tile]).
  pub const fn glass(&self) -> bool {
    self.glass
  }

  pub const fn with_glass(self, glass: bool) -> Self {
    Self { glass, ..self }
  }
}

/// Whether a theme's controls have a border ([Theme::control_border]), and how wide in points.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
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

/// A theme's control metrics ([Theme::sizes]) in points, read and changed as the theme's roles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizes {
  pub(crate) control_h: f32,
  pub(crate) small_h: f32,
  pub(crate) item_h: f32,
  pub(crate) tab_h: f32,
  pub(crate) menu_w: f32,
  pub(crate) menu_radius: f32,
  pub(crate) menu_edge: f32,
  pub(crate) check: f32,
  pub(crate) switch: SwitchSize,
  pub(crate) switch_large: SwitchSize,
  pub(crate) knob: [f32; 2],
  pub(crate) rail: f32,
  pub(crate) badge: f32,
  pub(crate) badge_h: f32,
}

impl Sizes {
  /// Push buttons.
  pub const fn control_h(&self) -> f32 {
    self.control_h
  }

  pub const fn with_control_h(self, control_h: f32) -> Self {
    Self { control_h, ..self }
  }

  /// Small buttons ([ControlSize::Small]): dense rows, inline actions.
  pub const fn small_h(&self) -> f32 {
    self.small_h
  }

  pub const fn with_small_h(self, small_h: f32) -> Self {
    Self { small_h, ..self }
  }

  /// A menu's items.
  pub const fn item_h(&self) -> f32 {
    self.item_h
  }

  pub const fn with_item_h(self, item_h: f32) -> Self {
    Self { item_h, ..self }
  }

  /// A tab strip.
  pub const fn tab_h(&self) -> f32 {
    self.tab_h
  }

  pub const fn with_tab_h(self, tab_h: f32) -> Self {
    Self { tab_h, ..self }
  }

  /// The least width of a pop-up button's menu.
  pub const fn menu_w(&self) -> f32 {
    self.menu_w
  }

  pub const fn with_menu_w(self, menu_w: f32) -> Self {
    Self { menu_w, ..self }
  }

  /// The corners of menus ([Theme::popup]) and the width of their edge.
  pub const fn menu_radius(&self) -> f32 {
    self.menu_radius
  }

  pub const fn with_menu_radius(self, menu_radius: f32) -> Self {
    Self { menu_radius, ..self }
  }

  pub const fn menu_edge(&self) -> f32 {
    self.menu_edge
  }

  pub const fn with_menu_edge(self, menu_edge: f32) -> Self {
    Self { menu_edge, ..self }
  }

  /// A check box's box.
  pub const fn check(&self) -> f32 {
    self.check
  }

  pub const fn with_check(self, check: f32) -> Self {
    Self { check, ..self }
  }

  /// A switch in rows ([ControlSize::Regular]) and the large one ([ControlSize::Large]: a
  /// header row's, by a big icon).
  pub const fn switch(&self) -> SwitchSize {
    self.switch
  }

  pub const fn with_switch(self, switch: SwitchSize) -> Self {
    Self { switch, ..self }
  }

  pub const fn switch_large(&self) -> SwitchSize {
    self.switch_large
  }

  pub const fn with_switch_large(self, switch_large: SwitchSize) -> Self {
    Self { switch_large, ..self }
  }

  /// A slider's knob, wide and high, and its track's thickness.
  pub const fn knob(&self) -> [f32; 2] {
    self.knob
  }

  pub const fn with_knob(self, knob: [f32; 2]) -> Self {
    Self { knob, ..self }
  }

  pub const fn rail(&self) -> f32 {
    self.rail
  }

  pub const fn with_rail(self, rail: f32) -> Self {
    Self { rail, ..self }
  }

  /// A badge's text, and its pill's height (padded 7 at the sides).
  pub const fn badge(&self) -> f32 {
    self.badge
  }

  pub const fn with_badge(self, badge: f32) -> Self {
    Self { badge, ..self }
  }

  pub const fn badge_h(&self) -> f32 {
    self.badge_h
  }

  pub const fn with_badge_h(self, badge_h: f32) -> Self {
    Self { badge_h, ..self }
  }
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
#[non_exhaustive]
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
    secondary: rgb(0x86868b),
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
    dialog_fill: WHITE,
    tip_fill: rgb(0xf6f6f6),
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
    field_fill: WHITE,
    field_line: rgb(0xdadade),
    faint: rgb(0xb2b2b7),
    hotkey_recorder_clear: false,
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
    secondary: rgb(0xa6a6a6),
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
    field_fill: rgb(0x252527),
    field_line: rgb(0x424245),
    faint: rgb(0x646468),
    plate: rgb(0x272727),
    tertiary: rgb(0x646464),
    link: rgb(0x409cff),
    track: alpha(WHITE, 0.12),
    track_hover: alpha(WHITE, 0.15),
    knob: rgb(0xe4e4e4),
    dialog_fill: rgb(0x2c2c2c),
    tip_fill: rgb(0x383838),
    tip_text: rgb(0xededed),
    ..Self::PLATES_LIGHT
  };

  /// This theme with another accent, and the selection and the focus ring made from it
  /// ([Theme::with_selection] and [Theme::with_focus] after it set those apart).
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
      Tone::Neutral => self.secondary,
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
    TextStyle::new(self.font, self.secondary).line_limit(1)
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
    TextStyle::new(self.small.weight(Weight::Semibold), self.secondary).line_limit(1)
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
  /// [Theme::dialog_fill], corners of 25, padded 20 at the top and 16 elsewhere (its text 6
  /// further in), a soft shadow; on dark grounds the menu's edge.
  pub const fn dialog(&self) -> Style {
    let edge = if self.scheme.is_dark() { self.menu_line } else { [0.0; 4] };
    Style::new()
      .pads([20.0, 16.0, 16.0, 16.0])
      .bg(self.dialog_fill)
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
      .bg(self.tip_fill)
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
      .bg(self.field_fill)
      .radius(self.radius)
      .border(1.0, self.field_line);
    let sel = alpha(self.accent, if self.scheme.is_dark() { 0.4 } else { 0.3 });
    FieldStyle::new(TextStyle::new(self.font, self.text))
      .frame(frame)
      .placeholder(self.faint)
      .selection(sel)
      .caret(self.accent)
  }

  /// A search field ([super::Ui::search_field]): [Theme::field]'s box [Sizes::control_h] + 4 high,
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

    f.frame(frame.bg(fill).border(0.5, edge)).placeholder(self.secondary)
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
      h.redraw();
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
      h.redraw();
      h.frame();
      h.app().scheme
    };
    assert_eq!(scheme(&mut h), Some(ColorScheme::Light));
    h.app_mut().pin = Some(Theme::PLATES_DARK);
    assert_eq!(scheme(&mut h), Some(ColorScheme::Dark));
    h.app_mut().pin = None;
    assert_eq!(scheme(&mut h), Some(ColorScheme::Light));
  }

  #[test]
  fn every_with_sets_its_role() {
    let t = Theme::PLATES_LIGHT
      .with_scheme(ColorScheme::Dark)
      .with_bg(rgb(2))
      .with_panel(rgb(3))
      .with_raised(rgb(4))
      .with_hover(rgb(5))
      .with_pressed(rgb(6))
      .with_line(rgb(7))
      .with_text(rgb(8))
      .with_secondary(rgb(9))
      .with_accent(rgb(10))
      .with_on_accent(rgb(11))
      .with_danger(rgb(12))
      .with_success(rgb(13))
      .with_warning(rgb(14))
      .with_selection(rgb(15))
      .with_focus(rgb(16))
      .with_ghost(rgb(17))
      .with_shadow(rgb(18))
      .with_menu(rgb(19))
      .with_menu_line(rgb(20))
      .with_plate(rgb(21))
      .with_tertiary(rgb(22))
      .with_link(rgb(23))
      .with_track(rgb(24))
      .with_track_hover(rgb(25))
      .with_knob(rgb(26))
      .with_dialog_fill(rgb(27))
      .with_tip_fill(rgb(28))
      .with_tip_text(rgb(29))
      .with_control_border(ControlBorder::Hairline(1.0))
      .with_radius(30.5)
      .with_sizes(Sizes::PLATES.with_tab_h(1.0))
      .with_font(Font::mono(32.0))
      .with_bold(Font::mono(33.0))
      .with_small(Font::mono(34.0))
      .with_fade(Transition::linear(2.0))
      .with_hover_all(!Theme::PLATES_LIGHT.hover_all)
      .with_disabled(37.5)
      .with_field_fill(rgb(39))
      .with_field_line(rgb(40))
      .with_faint(rgb(41))
      .with_hotkey_recorder_clear(!Theme::PLATES_LIGHT.hotkey_recorder_clear)
      .with_glass(!Theme::PLATES_LIGHT.glass);
    assert_eq!(
      t,
      Theme {
        scheme: ColorScheme::Dark,
        bg: rgb(2),
        panel: rgb(3),
        raised: rgb(4),
        hover: rgb(5),
        pressed: rgb(6),
        line: rgb(7),
        text: rgb(8),
        secondary: rgb(9),
        accent: rgb(10),
        on_accent: rgb(11),
        danger: rgb(12),
        success: rgb(13),
        warning: rgb(14),
        selection: rgb(15),
        focus: rgb(16),
        ghost: rgb(17),
        shadow: rgb(18),
        menu: rgb(19),
        menu_line: rgb(20),
        plate: rgb(21),
        tertiary: rgb(22),
        link: rgb(23),
        track: rgb(24),
        track_hover: rgb(25),
        knob: rgb(26),
        dialog_fill: rgb(27),
        tip_fill: rgb(28),
        tip_text: rgb(29),
        control_border: ControlBorder::Hairline(1.0),
        radius: 30.5,
        sizes: Sizes::PLATES.with_tab_h(1.0),
        font: Font::mono(32.0),
        bold: Font::mono(33.0),
        small: Font::mono(34.0),
        fade: Transition::linear(2.0),
        hover_all: !Theme::PLATES_LIGHT.hover_all,
        disabled: 37.5,
        field_fill: rgb(39),
        field_line: rgb(40),
        faint: rgb(41),
        hotkey_recorder_clear: !Theme::PLATES_LIGHT.hotkey_recorder_clear,
        glass: !Theme::PLATES_LIGHT.glass
      }
    );
  }

  #[test]
  fn every_size_sets_its_field() {
    let t = Sizes::PLATES
      .with_control_h(0.5)
      .with_small_h(1.5)
      .with_item_h(2.5)
      .with_tab_h(3.5)
      .with_menu_w(4.5)
      .with_menu_radius(5.5)
      .with_menu_edge(6.5)
      .with_check(7.5)
      .with_switch(SwitchSize { track: [1.0, 2.0], knob: [3.0, 4.0] })
      .with_switch_large(SwitchSize { track: [1.0, 2.0], knob: [3.0, 4.0] })
      .with_knob([10.0, 0.5])
      .with_rail(11.5)
      .with_badge(12.5)
      .with_badge_h(13.5);
    assert_eq!(
      t,
      Sizes {
        control_h: 0.5,
        small_h: 1.5,
        item_h: 2.5,
        tab_h: 3.5,
        menu_w: 4.5,
        menu_radius: 5.5,
        menu_edge: 6.5,
        check: 7.5,
        switch: SwitchSize { track: [1.0, 2.0], knob: [3.0, 4.0] },
        switch_large: SwitchSize { track: [1.0, 2.0], knob: [3.0, 4.0] },
        knob: [10.0, 0.5],
        rail: 11.5,
        badge: 12.5,
        badge_h: 13.5
      }
    );
  }
}
