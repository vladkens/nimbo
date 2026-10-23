// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The theme the widgets ([super::widgets]) draw with: a palette, sizes and fonts, read from the
//! [super::Ui] (`ui.theme()`). A theme is a plain value with public fields: an app takes the
//! default one, changes what it wants and sets it, or builds its own.
//!
//! An app changes the look in three steps, from the least work to the most:
//! - a variant where the widget has one: `Button::new("Save").primary()`;
//! - a style derived from the theme, changed for one widget: `t.button(Variant::Normal)
//!   .height(36.0)` given to [super::Button::style], `t.field().hint("Search")` to
//!   [super::Ui::text_field];
//! - its own theme: `const MINE: Theme = Theme { accent: rgb(0xe06c75), ..Theme::PLATES_DARK }` and
//!   `ui.set_theme(MINE)`.
//!
//! An app that wants a widget to look different altogether draws its own over the widget's
//! behaviour ([super::behavior]).
//!
//! The default look is macOS 26 System Settings' neutral greys, as measured from its
//! screenshots: [Theme::PLATES_LIGHT] and [Theme::PLATES_DARK], borderless translucent grey
//! buttons, subtle hovers on every control ([Theme::hover_all]; macOS hovers only some). Until
//! the app sets a theme, it follows the OS ([Theme::system]): light or dark, with the user's
//! accent, switching live when they change; [super::Ui::follow_system] goes back to it.

use std::time::Duration;

use super::store::Transition;
use super::widgets::FieldStyle;
use super::{Align, Dim, FocusRing, Shadow, Style, TextStyle, grow, px};
use crate::app::Appearance;
use crate::scene::{Color, alpha, mix, rgb};
use crate::text::{Font, Weight};

const WHITE: Color = [1.0; 4];
const BLACK: Color = [0.0, 0.0, 0.0, 1.0];

/// A palette, sizes and fonts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
  pub dark: bool,
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
  /// Overlay scrollbars' thumbs.
  pub thumb: Color,
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
  /// The border of push buttons: 0 on plates, where a grey fill is the button; a theme with
  /// one draws check boxes and segments bordered too ([Theme::flat]).
  pub border: f32,
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

/// A button's look ([Theme::button]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Variant {
  /// A raised button with a border.
  Normal,
  /// The accent fill: the default action.
  Primary,
  /// No fill until hovered: toolbars, icon buttons, close buttons.
  Ghost,
  /// The danger fill: discards or deletes.
  Danger,
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
  /// A switch in rows ([ControlSize::Medium]) and the regular one ([ControlSize::Large]: a
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

/// How big a control is ([super::Button::small], [Theme::button_sized]): its height, font and
/// padding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControlSize {
  /// [Sizes::small_h] high in the small font: dense rows, inline actions.
  Small,
  /// [Sizes::control_h] high in the body font.
  #[default]
  Medium,
  /// 8 points taller, the font 2 points larger: a dialog's main actions.
  Large,
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

/// How a button looks at rest, hovered and held, and its text. `const` builders, like [Style].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonStyle {
  /// Size, padding, radius and border; the fill comes from below.
  pub frame: Style,
  pub bg: Color,
  pub hover: Color,
  pub pressed: Color,
  pub text: TextStyle,
}

impl ButtonStyle {
  pub const fn height(self, h: f32) -> Self {
    Self { frame: self.frame.h(px(h)), ..self }
  }

  pub const fn w(self, w: Dim) -> Self {
    Self { frame: self.frame.w(w), ..self }
  }

  pub const fn h(self, h: Dim) -> Self {
    Self { frame: self.frame.h(h), ..self }
  }

  pub const fn radius(self, r: f32) -> Self {
    Self { frame: self.frame.radius(r), ..self }
  }

  /// Padding left and right of the label.
  pub const fn pad_x(self, x: f32) -> Self {
    Self { frame: self.frame.pad_xy(x, 0.0), ..self }
  }

  pub const fn min_w(self, w: f32) -> Self {
    Self { frame: self.frame.min_w(w), ..self }
  }

  pub const fn text(self, text: TextStyle) -> Self {
    Self { text, ..self }
  }

  pub const fn font(self, font: Font) -> Self {
    Self { text: self.text.font(font), ..self }
  }

  /// Fill at rest, hovered and held.
  pub const fn colors(self, bg: Color, hover: Color, pressed: Color) -> Self {
    Self { bg, hover, pressed, ..self }
  }
}

impl Theme {
  /// macOS 26 System Settings' neutral greys on white, as measured: translucent grey buttons
  /// (so they read one shade on a plate and another on the ground, as macOS's do), the system
  /// blue, and the [Sizes::PLATES] controls. The focus ring is `keyboardFocusIndicatorColor`'s,
  /// for the blue accent.
  pub const PLATES_LIGHT: Self = Self {
    dark: false,
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
    thumb: alpha(BLACK, 0.24),
    plate: rgb(0xf8f8f8),
    tertiary: rgb(0xbfbfbf),
    link: rgb(0x0068da),
    track: alpha(BLACK, 0.09),
    track_hover: alpha(BLACK, 0.12),
    knob: WHITE,
    dialog: WHITE,
    tip: rgb(0xf6f6f6),
    tip_text: rgb(0x545454),
    border: 0.0,
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
    dark: true,
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
    thumb: alpha(WHITE, 0.26),
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
    let (selection, focus) = if self.dark { (0.30, 0.55) } else { (0.22, 0.45) };
    Self { accent, selection: alpha(accent, selection), focus: alpha(accent, focus), ..self }
  }

  /// The theme for the OS's appearance: [Theme::PLATES_LIGHT] or [Theme::PLATES_DARK], with the
  /// OS's accent when it has one ([Theme::with_accent]); the theme's own for its own accent
  /// (within a level: the OS's blue), which keeps its measured focus ring. What the [super::Ui]
  /// draws with until the app sets its own ([super::Ui::set_theme]).
  pub fn system(a: Appearance) -> Self {
    let base = if a.dark { Self::PLATES_DARK } else { Self::PLATES_LIGHT };
    let Some(accent) = a.accent else { return base };
    if (0..3).all(|i| (accent[i] - base.accent[i]).abs() < 1.5 / 255.0) {
      return base;
    }

    base.with_accent(accent)
  }

  /// The accent under the pointer: 12% toward black (light) or white (dark).
  pub const fn accent_hover(&self) -> Color {
    mix(self.accent, if self.dark { WHITE } else { BLACK }, 0.12)
  }

  /// Whether controls are fills without a border, as System Settings draws them ([Theme::border]
  /// 0): check boxes filled with the track, a segmented control of raised segments.
  pub const fn flat(&self) -> bool {
    self.border == 0.0
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
    mix(self.plate, self.tone(tone), if self.dark { 0.22 } else { 0.14 })
  }

  /// The selected toolbar tab's raised tile, as in macOS's preference windows: white (light) or
  /// 14% toward the text over the ground (dark, without [Theme::glass]).
  pub const fn tab_tile(&self) -> Color {
    if self.dark { mix(self.bg, self.text, 0.14) } else { WHITE }
  }

  /// One line of text, cut with "…" when it doesn't fit.
  pub const fn label(&self) -> TextStyle {
    TextStyle::new(self.font, self.text).ellipsis()
  }

  /// Wrapping text.
  pub const fn body(&self) -> TextStyle {
    TextStyle::new(self.font, self.text)
  }

  /// Secondary text on one line: an unselected tab, a hint.
  pub const fn muted(&self) -> TextStyle {
    TextStyle::new(self.font, self.muted).ellipsis()
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
    TextStyle::new(self.small.weight(Weight::Semibold), self.muted).ellipsis()
  }

  pub const fn button(&self, v: Variant) -> ButtonStyle {
    self.button_sized(v, ControlSize::Medium)
  }

  pub const fn button_sized(&self, v: Variant, size: ControlSize) -> ButtonStyle {
    let z = self.sizes;
    let (h, font) = match size {
      ControlSize::Small => (z.small_h, self.small),
      ControlSize::Medium => (z.control_h, self.font),
      ControlSize::Large => (z.control_h + 8.0, Font { size: self.font.size + 2.0, ..self.font }),
    };
    // Padding grows with the height: 12 at 24, 18 at 36. Its label's width, as AppKit's push
    // buttons keep theirs: text beside it wraps instead ([ButtonStyle::w] changes it).
    let pad = (h * 0.5).round();
    let frame = Style::new().w(Dim::Rigid).h(px(h)).min_w(h).pad_xy(pad, 0.0).gap(5.0).center();
    let frame = frame.radius(if h < z.control_h { self.radius - 1.0 } else { self.radius });
    let text = TextStyle::new(font, self.text).ellipsis();
    let (frame, bg, hover, pressed, text) = match v {
      Variant::Normal => {
        let frame = frame.border(self.border, self.line);
        (frame, self.raised, self.hover, self.pressed, text)
      }
      Variant::Primary => {
        let c = self.accent;
        let p = mix(c, BLACK, 0.15);
        let text = text.color(self.on_accent).font(font.weight(Weight::Medium));
        (frame, c, self.accent_hover(), p, text)
      }
      Variant::Ghost => (frame, alpha(self.ghost, 0.0), self.ghost, self.hover, text),
      Variant::Danger => {
        let c = self.danger;
        let (h, p) = (mix(c, WHITE, 0.12), mix(c, BLACK, 0.15));
        (frame, c, h, p, text.color(self.on_accent))
      }
    };

    ButtonStyle { frame, bg, hover, pressed, text }
  }

  /// `s` showing a selected or toggled-on state: tinted with the accent, the label in it.
  pub const fn selected(&self, s: ButtonStyle) -> ButtonStyle {
    let c = self.accent;
    let s = s.colors(alpha(c, 0.18), alpha(c, 0.25), alpha(c, 0.32));
    ButtonStyle { text: s.text.color(c), ..s }
  }

  /// A modal dialog's card ([super::Dialog]), a macOS 26 alert's as measured: the
  /// [Theme::dialog] fill, corners of 25, padded 20 at the top and 16 elsewhere (its text 6
  /// further in), a soft shadow; on dark grounds the menu's edge.
  pub const fn dialog(&self) -> Style {
    let edge = if self.dark { self.menu_line } else { [0.0; 4] };
    Style::new()
      .pads([20.0, 16.0, 16.0, 16.0])
      .bg(self.dialog)
      .radius(25.0)
      .border(self.sizes.menu_edge, edge)
      .shadow(Shadow::new(10.0, 36.0, self.shadow))
  }

  /// A dialog's button, a macOS 26 alert's: a capsule 28 high in the body font, padded 12;
  /// Cancel ([Variant::Normal]) a translucent grey of the text over the card, the action the
  /// accent ([Variant::Primary]) or red ([Variant::Danger]).
  pub const fn dialog_button(&self, v: Variant) -> ButtonStyle {
    let h = 28.0;
    let s = self.button(v);
    let s = ButtonStyle { frame: s.frame.border(0.0, [0.0; 4]), ..s };
    let s = s.height(h).radius(h / 2.0).pad_x(12.0).font(self.font);
    match v {
      Variant::Normal | Variant::Ghost => {
        let (k, c) = (if self.dark { 0.09 } else { 0.07 }, self.text);
        s.colors(alpha(c, k), alpha(c, k + 0.04), alpha(c, k + 0.1))
      }
      _ => s,
    }
  }

  /// The backdrop under a modal dialog ([super::Ui::modal]): black at 20% (light) or 45%
  /// (dark).
  pub const fn scrim(&self) -> Color {
    alpha(BLACK, if self.dark { 0.45 } else { 0.2 })
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

  /// A single-line text field: [Sizes::control_h] high at least, the field's ground and edge
  /// in the control's corners, the faint placeholder, the accent's caret and a selection of it
  /// (`.multiline()` and a height for notes, `.password()` for secrets). Focused, the ring.
  pub const fn field(&self) -> FieldStyle {
    let frame = Style::new()
      .w(grow())
      .min_h(self.sizes.control_h)
      .pad_xy((self.sizes.control_h / 3.0).round(), 4.0)
      .align(Align::Start, Align::Center)
      .bg(self.field)
      .radius(self.radius)
      .border(1.0, self.field_line);
    let sel = alpha(self.accent, if self.dark { 0.4 } else { 0.3 });
    FieldStyle::new(TextStyle::new(self.font, self.text))
      .frame(frame)
      .placeholder(self.faint)
      .selection(sel)
      .caret(self.accent)
  }

  /// A search field ([super::Ui::search]): [Theme::field]'s box [Sizes::control_h] + 4 high,
  /// padded 30 at the left for the magnifier and 10 at the right. Where controls are flat
  /// ([Theme::flat]), System Settings' sidebar field: the ground a touch lighter, a half-point
  /// edge, the hint in the muted colour.
  pub const fn search(&self) -> FieldStyle {
    let f = self.field();
    let frame = f.frame.h(px(self.sizes.control_h + 4.0)).pads([0.0, 10.0, 0.0, 30.0]);
    if !self.flat() {
      return f.frame(frame);
    }

    let (fill, edge) = match self.dark {
      true => (alpha(WHITE, 0.12), alpha(BLACK, 0.5)),
      false => (alpha(WHITE, 0.3), alpha(BLACK, 0.15)),
    };

    f.frame(frame.bg(fill).border(0.5, edge)).placeholder(self.muted)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn variants_read_the_palette() {
    let t = Theme::PLATES_DARK;
    assert_eq!(t.button(Variant::Primary).bg, t.accent);
    assert_eq!(t.button(Variant::Ghost).bg[3], 0.0);
    assert_eq!(t.button(Variant::Normal).frame.border_color, t.line);
    assert_eq!(Theme::PLATES_LIGHT.button(Variant::Normal).frame.border, [0.0; 4]);
    const TALL: ButtonStyle = Theme::PLATES_LIGHT.button(Variant::Normal).height(36.0);
    assert_eq!(TALL.frame.size[1], px(36.0));
    // Sizes and builders, all const.
    const KEY: ButtonStyle =
      Theme::PLATES_DARK.button(Variant::Normal).w(super::super::grow()).h(px(40.0)).radius(12.0);
    assert_eq!((KEY.frame.size[1], KEY.frame.radii), (px(40.0), [12.0; 4]));
    let small = t.button_sized(Variant::Ghost, ControlSize::Small);
    let large = t.button_sized(Variant::Ghost, ControlSize::Large);
    assert_eq!((small.frame.size[1], small.text.font), (px(t.sizes.small_h), t.small));
    assert_eq!((large.frame.size[1], large.text.font.size), (px(32.0), 15.0));
    let on = t.selected(t.button(Variant::Normal));
    assert_eq!((on.text.color, on.bg[3]), (t.accent, 0.18));
  }

  #[test]
  fn the_system_theme_follows_the_os_accent() {
    assert_eq!(Theme::system(Appearance::LIGHT), Theme::PLATES_LIGHT);
    assert_eq!(Theme::system(Appearance::DARK), Theme::PLATES_DARK);
    let purple = Appearance { dark: false, accent: Some(rgb(0x953d96)) };
    let t = Theme::system(purple);
    assert_eq!((t.bg, t.accent, t.sizes), (WHITE, rgb(0x953d96), Sizes::PLATES));
    // The focus ring follows another accent.
    assert_eq!(t.focus, alpha(rgb(0x953d96), 0.45));
    // The OS's own blue is the theme's: its measured focus ring stays.
    let blue = Appearance { dark: true, accent: Some(rgb(0x0a84ff)) };
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
}
