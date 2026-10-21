// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The theme the widgets ([super::widgets]) draw with: a palette, sizes and fonts, read from the
//! [super::Ui] (`ui.theme()`). A theme is a plain value with public fields: an app takes the
//! default one, changes what it wants and sets it, or builds its own.
//!
//! An app changes the look in three steps, from the least work to the most:
//! - a variant where the widget has one: `Button::new("Save").primary()`;
//! - a style derived from the theme, changed for one widget: `t.button(Variant::Normal)
//!   .height(36.0)` given to [super::Button::style];
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

use super::store::Transition;
use super::{Dim, FocusRing, Style, TextStyle, px};
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
  /// The focus ring.
  pub focus: Color,
  /// A translucent tile under the pointer: ghost buttons.
  pub ghost: Color,
  /// The border of push buttons: 0 on plates, where a grey fill is the button.
  pub border: f32,
  /// Corners of controls.
  pub radius: f32,
  /// How big controls are.
  pub sizes: Sizes,
  pub font: Font,
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
}

impl Sizes {
  /// macOS 26 System Settings': 24-point buttons in 13-point text, small ones 20 high.
  pub const PLATES: Self = Self { control_h: 24.0, small_h: 20.0 };
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
    raised: alpha(BLACK, 0.065),
    hover: alpha(BLACK, 0.09),
    pressed: alpha(BLACK, 0.145),
    line: rgb(0xe5e5e7),
    text: rgb(0x262626),
    muted: rgb(0x86868b),
    accent: rgb(0x007aff),
    on_accent: WHITE,
    danger: rgb(0xd70015),
    focus: alpha(rgb(0x0067f4), 0.45),
    ghost: alpha(BLACK, 0.05),
    border: 0.0,
    radius: 6.0,
    sizes: Sizes::PLATES,
    font: Font::ui(13.0),
    small: Font::ui(11.0),
    fade: Transition::smooth(6.0),
    hover_all: true,
    disabled: 0.4,
  };

  /// [Theme::PLATES_LIGHT]'s roles on System Settings' dark greys; the focus ring is
  /// `keyboardFocusIndicatorColor`'s cyan-blue, not the accent.
  pub const PLATES_DARK: Self = Self {
    dark: true,
    bg: rgb(0x1e1e1e),
    raised: alpha(WHITE, 0.09),
    hover: alpha(WHITE, 0.12),
    pressed: alpha(WHITE, 0.2),
    line: rgb(0x3a3a3d),
    text: rgb(0xe1e1e1),
    muted: rgb(0xa6a6a6),
    accent: rgb(0x0a84ff),
    danger: rgb(0xff453a),
    focus: alpha(rgb(0x1aa9ff), 0.5),
    ghost: alpha(WHITE, 0.07),
    ..Self::PLATES_LIGHT
  };

  /// This theme with another accent, and the focus ring made from it.
  pub const fn with_accent(self, accent: Color) -> Self {
    let focus = if self.dark { 0.55 } else { 0.45 };
    Self { accent, focus: alpha(accent, focus), ..self }
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

  /// One line of text, cut with "…" when it doesn't fit.
  pub const fn label(&self) -> TextStyle {
    TextStyle::new(self.font, self.text).ellipsis()
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

  pub const fn focus_ring(&self) -> FocusRing {
    FocusRing::new(self.focus)
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
    for (t, white, pressed) in
      [(Theme::PLATES_LIGHT, 0xefefef, 0xdadada), (Theme::PLATES_DARK, 0x333333, 0x4b4b4b)]
    {
      let ground = t.bg;
      assert!(near(over(t.raised, ground), white), "{:?}", over(t.raised, ground));
      assert!(near(over(t.pressed, ground), pressed), "{:?}", over(t.pressed, ground));
    }
  }
}
