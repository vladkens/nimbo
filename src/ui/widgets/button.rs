// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The button's looks from the theme, as SwiftUI's button styles ([ButtonStyle]):
//! [BorderedButtonStyle], [BorderedProminentButtonStyle] and [BorderlessButtonStyle] at the
//! control size ([Ui::control_size]), the icon button; the ⓘ that explains a row, the warning
//! mark that tells what is wrong with it, and the round "?" that opens a page's help.

use crate::scene::{Color, Frame, Rect, alpha, mix};
use crate::t;
use crate::text::{Font, Weight};
use crate::ui::icon::Icon;
use crate::ui::{
  ButtonConfiguration, ButtonRole, ButtonStyle, ControlSize, Dim, Enabled, Float, Id, Response,
  Role, Style, TextStyle, Theme, ToggleConfiguration, Ui, px,
};

const WHITE: Color = [1.0; 4];
const BLACK: Color = [0.0, 0.0, 0.0, 1.0];

/// How far below the middle of the line a mark ([Ui::info_button], [Ui::warning_button]) goes,
/// while a form row builds its label ([crate::FormRow::new]): onto its capitals' middle.
pub(crate) struct MarkShift(pub f32);

/// The theme's push button, as SwiftUI's `.bordered`: a raised grey fill (with the theme's border,
/// if it has one), a destructive one's label red. The shell's default.
#[derive(Clone, Copy, Debug, Default)]
pub struct BorderedButtonStyle;

/// The default action, as SwiftUI's `.borderedProminent`: the accent's fill, the danger's for a
/// destructive one.
#[derive(Clone, Copy, Debug, Default)]
pub struct BorderedProminentButtonStyle;

/// No fill until hovered, as SwiftUI's `.borderless`: toolbars, icon buttons, close buttons.
#[derive(Clone, Copy, Debug, Default)]
pub struct BorderlessButtonStyle;

impl ButtonStyle for BorderedButtonStyle {
  fn make_body(&self, ui: &mut Ui<'_>, mut c: ButtonConfiguration<'_>) {
    themed(ui, Face::of(&c), Variant::Bordered, c.role(), |ui, s| c.label(ui, s));
  }
}

impl ButtonStyle for BorderedProminentButtonStyle {
  fn make_body(&self, ui: &mut Ui<'_>, mut c: ButtonConfiguration<'_>) {
    themed(ui, Face::of(&c), Variant::Prominent, c.role(), |ui, s| c.label(ui, s));
  }
}

impl ButtonStyle for BorderlessButtonStyle {
  fn make_body(&self, ui: &mut Ui<'_>, mut c: ButtonConfiguration<'_>) {
    themed(ui, Face::of(&c), Variant::Borderless, c.role(), |ui, s| c.label(ui, s));
  }
}

/// A button's state as the theme's looks draw it: a button's, or a toggle's in the button style
/// ([crate::ButtonToggleStyle]), selected while on.
#[derive(Clone, Copy)]
pub(crate) struct Face {
  id: Id,
  hovered: bool,
  pressed: bool,
  enabled: bool,
  icon_only: bool,
  selected: bool,
}

impl Face {
  /// Button `c`'s.
  pub(crate) fn of(c: &ButtonConfiguration) -> Self {
    let (id, hovered, pressed, enabled) = (c.id(), c.is_hovered(), c.is_pressed(), c.is_enabled());
    Self { id, hovered, pressed, enabled, icon_only: c.is_icon_only(), selected: false }
  }

  /// Toggle `c`'s, selected while on.
  pub(crate) fn of_toggle(c: &ToggleConfiguration) -> Self {
    let (id, hovered, pressed, enabled) = (c.id(), c.is_hovered(), c.is_pressed(), c.is_enabled());
    Self { id, hovered, pressed, enabled, icon_only: c.is_icon_only(), selected: c.is_on() }
  }
}

/// The theme's looks a button style draws.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Variant {
  Bordered,
  Prominent,
  Borderless,
}

/// How a button looks at rest, hovered and held, and its label ([look]).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ButtonLook {
  /// Size, padding, radius and border; the fill comes from below.
  pub(crate) frame: Style,
  pub(crate) bg: Color,
  pub(crate) hover: Color,
  pub(crate) pressed: Color,
  pub(crate) text: TextStyle,
}

/// A button's look in `v` at `size`, tinted by its `role`: a destructive prominent one filled
/// with the danger colour, any other destructive one's label in it.
pub(crate) fn look(
  t: &Theme,
  v: Variant,
  role: Option<ButtonRole>,
  size: ControlSize,
) -> ButtonLook {
  let z = t.sizes;
  let (h, font) = match size {
    ControlSize::Small => (z.small_h, t.small),
    ControlSize::Regular => (z.control_h, t.font),
    ControlSize::Large => (z.control_h + 8.0, Font { size: t.font.size + 2.0, ..t.font }),
  };

  // Its label's width, as AppKit's push buttons keep theirs: text beside it wraps instead.
  let pad = t.button_padding(size);
  let frame = Style::new().w(Dim::Rigid).h(px(h)).min_w(h).pad_xy(pad, 0.0).gap(5.0).center();
  let frame = frame.radius(if h < z.control_h { t.radius - 1.0 } else { t.radius });
  let text = TextStyle::new(font, t.text).line_limit(1);
  let destructive = matches!(role, Some(ButtonRole::Destructive));
  let red = if destructive { text.color(t.danger) } else { text };

  let (frame, bg, hover, pressed, text) = match v {
    Variant::Bordered => {
      let frame = frame.border(t.control_border.width(), t.line);
      (frame, t.raised, t.hover, t.pressed, red)
    }
    Variant::Prominent if destructive => {
      let c = t.danger;
      let (h, p) = (mix(c, WHITE, 0.12), mix(c, BLACK, 0.15));
      (frame, c, h, p, text.color(t.on_accent))
    }
    Variant::Prominent => {
      let c = t.accent;
      let p = mix(c, BLACK, 0.15);
      let text = text.color(t.on_accent).font(font.weight(Weight::Medium));
      (frame, c, t.accent_hover(), p, text)
    }
    Variant::Borderless => (frame, alpha(t.ghost, 0.0), t.ghost, t.hover, red),
  };

  ButtonLook { frame, bg, hover, pressed, text }
}

/// Button `c` in the theme's look `v` at the control size and its `role`'s tint: square for an
/// icon alone, muted until hovered unless selected; selected, the accent's tint and label.
/// Borderless buttons (toolbar ones) hover; the others where the theme hovers every control.
pub(crate) fn themed(
  ui: &mut Ui,
  c: Face,
  v: Variant,
  role: Option<ButtonRole>,
  label: impl FnOnce(&mut Ui, TextStyle),
) {
  let t = ui.theme();
  let mut s = look(&t, v, role, ui.environment_value::<ControlSize>());
  if c.icon_only {
    let h = s.frame.min[0];
    s.frame = s.frame.w(px(h)).pad(0.0);
  }

  if c.selected {
    let a = t.accent;
    (s.bg, s.hover, s.pressed) = (alpha(a, 0.18), alpha(a, 0.25), alpha(a, 0.32));
    s.text = s.text.color(a);
  }

  let hot = match v {
    Variant::Borderless => ui.hot(c.id, c.hovered),
    _ => ui.ctl_hot(c.id, c.hovered),
  };

  let muted = c.icon_only && !c.selected;
  draw(ui, c, s, hot, muted, label);
}

/// Draws button `c` in `s`, hovered by `hot` (0 to 1), the `label` in its text style, an icon
/// alone muted until hovered when `muted`. Disabled, its colours fade themselves, as a canvas
/// (an icon) draws past an element's opacity; an icon alone toward the ground ([Ui::dim]).
pub(crate) fn draw(
  ui: &mut Ui,
  c: Face,
  s: ButtonLook,
  hot: f32,
  muted: bool,
  label: impl FnOnce(&mut Ui, TextStyle),
) {
  let t = ui.theme();
  let (pressed, off) = (c.pressed, !c.enabled);
  let bg = if pressed { s.pressed } else { mix(s.bg, s.hover, hot) };
  let ink = match muted {
    true => mix(t.secondary, t.text, if pressed { 1.0 } else { hot }),
    false => s.text.color,
  };

  let fade = |c: Color| if off { alpha(c, c[3] * t.disabled) } else { c };
  let ink = if c.icon_only { ui.dim(ink, off) } else { fade(ink) };
  let frame = Style { bg: fade(bg), border_color: fade(s.frame.border_color), ..s.frame };
  ui.row(frame, |ui| label(ui, s.text.color(ink)));
}

impl Ui<'_> {
  /// A square borderless button showing an icon, muted until the pointer is on it
  /// ([BorderlessButtonStyle], `Button::new(icon)`).
  pub fn icon_button(&mut self, icon: Icon) -> Response {
    self.button_style(&BorderlessButtonStyle).button(icon)
  }

  /// An ⓘ, 16 points, muted, in the label colour while held, halfway there under the pointer
  /// where the theme hovers controls ([crate::Theme::hover_all]): with [Ui::help] on it, an
  /// explanation longer than a line, as SwiftUI's `Image(systemName: "info.circle").help(…)`.
  /// A button by its behaviour, named "Info". A mark is there for its tip: [Ui::disabled], on it
  /// or on a container around, leaves it enabled. In a form row's label ([crate::FormRow::new]) it
  /// sits on the middle of the label's capitals, the line keeping its height.
  pub fn info_button(&mut self) -> Response {
    let t = self.theme();
    self.mark_button(Icon::INFO, [t.secondary, t.text])
  }

  /// A warning mark in the ⓘ's size and place ([Ui::info_button]): a triangle with a "!" cut out
  /// of it ([Icon::WARNING]) in the theme's warning colour ([crate::Theme::warning]), a third of
  /// the way to the label colour while held: with [Ui::help] on it, what is wrong. A button by
  /// its behaviour, named "Warning", enabled and placed as the ⓘ.
  pub fn warning_button(&mut self) -> Response {
    let t = self.theme();
    self.mark_button(Icon::WARNING, [t.warning, mix(t.warning, t.text, 0.33)])
  }

  /// A mark 16 points square over the button behaviour, named by its icon ([Icon::name]): in
  /// `ink[0]`, `ink[1]` while held, halfway there under the pointer where the theme hovers
  /// controls.
  fn mark_button(&mut self, icon: Icon, ink: [Color; 2]) -> Response {
    let t = self.theme();
    let id = self.take_id();
    // Enabled whatever disabled it: its tip reads all the same.
    let b = self.environment(Enabled(true)).button_behavior(id);
    self.a11y(id, Role::Button).name_or(icon.name().unwrap_or_default());

    let hot = self.ctl_hot(id, b.hovered);
    let size = (t.font.size + 3.0).round();
    let k = if b.pressed { 1.0 } else { 0.5 * hot };
    let color = self.dim(mix(ink[0], ink[1], k), b.disabled);
    let s = self.dim_style(Style::new().w(px(size)).h(px(size)).radius(size / 2.0), b.disabled);
    let paint = move |f: &mut Frame, r: Rect| icon.default_stroke(1.5).draw(f, r, color);
    let Some(&mut MarkShift(dy)) = self.slot::<MarkShift>() else {
      let mut r = self.control(&b).canvas(s, paint);
      r.clicked = b.clicked;
      return r;
    };

    // On a form label's line: in a box as high as the mark, moved onto the capitals' middle.
    let help = self.take_help();
    let mut r = Response::default();
    self.row(Style::new().w(px(size)).h(px(size)), |ui| {
      if let Some(tip) = &help {
        ui.help(tip);
      }

      r = ui.control(&b).float(Float::parent().offset(0.0, dy).clip()).canvas(s, paint);
    });
    r.clicked = b.clicked;
    r
  }

  /// The round "?" at the bottom of a settings page: opens its help. As high as a push button
  /// ([crate::Sizes::control_h]), raised, pressed as one.
  pub fn help_button(&mut self) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let b = self.button_behavior(id);
    self.a11y(id, Role::Button).name_or(&t!("Help"));
    let hot = self.ctl_hot(id, b.hovered);
    let d = t.sizes.control_h;
    let bg = if b.pressed { t.pressed } else { mix(t.raised, t.hover, hot) };
    let s = Style::new().w(px(d)).h(px(d)).center().radius(d / 2.0).bg(bg);
    let s = self.dim_style(s, b.disabled);
    let text = TextStyle::new(t.bold, t.text).nowrap();
    let mut r = self.control(&b).row(s, |ui| ui.text("?", text));
    r.clicked = b.clicked;
    r
  }
}

#[cfg(test)]
mod tests {
  use super::{Variant, look};
  use crate::app::App;
  use crate::keys::Key;
  use crate::scene::{Rect, Size};
  use crate::text::Font;
  use crate::ui::{
    ButtonStyle, ControlSize, Cursor, Icon, Id, PlainButtonStyle, Response, Style, TextStyle,
    Theme, Ui, px,
  };
  use crate::{Harness, Modifiers, NamedKey};

  #[test]
  fn looks_read_the_palette() {
    let t = Theme::PLATES_DARK;
    let at = |v, size| look(&t, v, None, size);
    assert_eq!(at(Variant::Prominent, ControlSize::Regular).bg, t.accent);
    assert_eq!(at(Variant::Borderless, ControlSize::Regular).bg[3], 0.0);
    assert_eq!(at(Variant::Bordered, ControlSize::Regular).frame.border_color, t.line);
    let light = look(&Theme::PLATES_LIGHT, Variant::Bordered, None, ControlSize::Regular);
    assert_eq!(light.frame.border, [0.0; 4]);
    let (small, large) =
      (at(Variant::Borderless, ControlSize::Small), at(Variant::Borderless, ControlSize::Large));
    assert_eq!((small.frame.size[1], small.text.font), (px(t.sizes.small_h), t.small));
    assert_eq!((large.frame.size[1], large.text.font.size), (px(32.0), 15.0));
    assert_eq!(large.frame.pad[1], t.button_padding(ControlSize::Large));
    assert_eq!(t.button_padding(ControlSize::Regular), 12.0);
  }

  /// A button in the window's style the view put, else in the shell's.
  struct Window(Option<&'static dyn ButtonStyle>);

  impl App for Window {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_environment(self.0);
      ui.id("b").button("B");
    }
  }

  /// The shell puts the bordered style under the window's: a push button's height.
  #[test]
  fn the_shell_puts_the_bordered_style() {
    let mut h = Harness::new(Window(None), Size::new(200.0, 100.0), 1.0);
    h.frame();
    let push = Theme::system(h.rt.ui.appearance()).sizes.control_h;
    let height = |h: &mut Harness<Window>, s| {
      h.app_mut().0 = s;
      h.redraw();
      h.frame();
      h.rect(Id::of("b")).unwrap().h
    };
    assert_eq!(height(&mut h, None), push);
    assert_ne!(height(&mut h, Some(&PlainButtonStyle)), push);
    assert_eq!(height(&mut h, None), push, "back to the shell's");
  }

  /// A disabled button, its last response kept.
  #[derive(Default)]
  struct Off(Option<Response>);

  impl App for Off {
    fn view(&mut self, ui: &mut Ui) {
      self.0 = Some(ui.disabled(true).id("off").button("Off"));
    }
  }

  #[test]
  fn a_disabled_button_is_never_hovered_or_pressed() {
    let mut h = Harness::new(Off::default(), Size::new(200.0, 100.0), 1.0);
    h.frame();
    let r = h.rt.ui.last.rect(Id::of("off")).unwrap();
    let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    h.move_to(x, y);
    h.press(x, y);
    h.frame();
    let r = h.app().0.unwrap();
    assert!(!r.hovered && !r.pressed && !r.clicked, "{r:?}");
  }

  /// A button after a note in a row `.0` wide.
  struct Beside(f32);

  impl App for Beside {
    fn view(&mut self, ui: &mut Ui) {
      ui.row(Style::new().w(px(self.0)), |ui| {
        ui.text("A note beside the button that wraps", TextStyle::new(Font::ui(13.0), [1.0; 4]));
        ui.id("b").button("Refresh");
      });
    }
  }

  #[test]
  fn a_button_keeps_its_width_beside_a_note() {
    let width = |row: f32| {
      let mut h = Harness::new(Beside(row), Size::new(400.0, 200.0), 1.0);
      h.frame();
      h.rt.ui.last.rect(Id::of("b")).unwrap().w
    };
    // The note wraps down to its widest letter; the button stays (its edges on whole points, so
    // within one where it moves).
    let (narrow, wide) = (width(120.0), width(400.0));
    assert!((narrow - wide).abs() <= 1.0, "{narrow} beside a wrapping note, {wide} alone");
  }

  /// Buttons sized on themselves, by a container and after it; and a disabled container's.
  #[derive(Default)]
  struct Scoped(usize);

  impl App for Scoped {
    fn view(&mut self, ui: &mut Ui) {
      ui.control_size(ControlSize::Small).id("small").button("S");
      ui.control_size(ControlSize::Large).col(Style::new(), |ui| {
        ui.id("large").button("L");
        ui.control_size(ControlSize::Small).id("inner").button("I");
      });
      ui.id("regular").button("R");
      ui.disabled(true).col(Style::new(), |ui| {
        self.0 += ui.id("off").button("Off").clicked as usize;
      });
      self.0 += 100 * ui.disabled(true).id("one").button("One").clicked as usize;
      self.0 += 10 * ui.id("on").button("On").clicked as usize;
    }
  }

  /// [Ui::control_size] sizes the next widget or every widget in the next container, until it
  /// closes; [Ui::disabled] on a container or a widget disables what is in it, not what follows.
  #[test]
  fn sizes_and_disabled_scope_a_container() {
    let mut h = Harness::new(Scoped::default(), Size::new(300.0, 300.0), 1.0);
    h.frame();
    let z = Theme::system(h.rt.ui.appearance()).sizes;
    let height = |n: &str| h.rect(Id::of(n)).unwrap().h;
    let large = z.control_h + 8.0;
    let hs = [height("small"), height("large"), height("inner"), height("regular")];
    assert_eq!(hs, [z.small_h, large, z.small_h, z.control_h]);
    h.click_id(Id::of("off"));
    h.click_id(Id::of("one"));
    h.click_id(Id::of("on"));
    assert_eq!(h.app().0, 10, "only the button after the disabled ones clicked");
  }

  /// Icon buttons of the same icon, at the button's width and at one the app gave it.
  struct Strokes;

  impl App for Strokes {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_theme(Theme::PLATES_LIGHT);
      ui.col(Style::new().grow().bg(ui.theme().bg), |ui| {
        ui.id("own").icon_button(Icon::GEAR);
        ui.id("app").icon_button(Icon::GEAR.stroke(3.0));
      });
    }
  }

  /// A width the app gave the icon wins over the button's own.
  #[test]
  fn an_icons_own_stroke_stays() {
    let mut h = Harness::new(Strokes, Size::new(100.0, 100.0), 2.0);
    h.frame();
    let img = h.render();
    let ink = |r: Rect| {
      let px = (0..(r.w * 2.0) as i32).flat_map(|x| (0..(r.h * 2.0) as i32).map(move |y| (x, y)));
      px.filter(|&(x, y)| img.pixel(r.x + x as f32 / 2.0, r.y + y as f32 / 2.0)[0] < 160).count()
    };
    let (own, app) = (ink(h.rect(Id::of("own")).unwrap()), ink(h.rect(Id::of("app")).unwrap()));
    assert!(app > own + own / 3, "the app's width: {app} inked, the button's {own}");
  }

  /// An icon button, disabled or not, in the light theme.
  struct Reset(bool);

  impl App for Reset {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_theme(Theme::PLATES_LIGHT);
      ui.col(Style::new().grow().bg(ui.theme().bg), |ui| {
        ui.disabled(self.0).id("r").icon_button(Icon::RESET);
      });
    }
  }

  #[test]
  fn a_disabled_icon_button_dims_its_icon() {
    let darkest = |off: bool| {
      let mut h = Harness::new(Reset(off), Size::new(100.0, 60.0), 2.0);
      h.frame();
      let r = h.rect(Id::of("r")).unwrap();
      let img = h.render();
      let px = (0..(r.w * 2.0) as i32).flat_map(|x| (0..(r.h * 2.0) as i32).map(move |y| (x, y)));
      px.map(|(x, y)| img.pixel(r.x + x as f32 / 2.0, r.y + y as f32 / 2.0)[0]).min().unwrap()
    };
    let (on, off) = (darkest(false), darkest(true));
    assert!(off > on + 40, "the ink at {on} enabled, {off} disabled");
  }

  /// A button the app made not focusable with a pointer of its own, which it may later make
  /// focusable again; a disabled one the app asks focus for; and a field that asks to be focused
  /// but is not focusable.
  #[derive(Default)]
  struct Own {
    focusable: bool,
    clicks: usize,
    text: String,
  }

  impl App for Own {
    fn view(&mut self, ui: &mut Ui) {
      let ui = ui.focusable(self.focusable).pointer_style(Cursor::Crosshair);
      if ui.id("own").button("Own").clicked {
        self.clicks += 1;
      }

      ui.focusable(true).disabled(true).id("off").button("Off");
      ui.autofocus().focusable(false).id("field").text_field(&mut self.text, "");
    }
  }

  #[test]
  fn the_apps_focus_and_pointer_win_over_the_controls() {
    let mut h = Harness::new(Own::default(), Size::new(300.0, 100.0), 1.0);
    h.frame();
    assert_eq!(h.focus_id(), None, "the field doesn't autofocus");
    let r = h.rect(Id::of("own")).unwrap();
    h.move_to(r.x + 4.0, r.y + 4.0);
    assert_eq!(h.cursor(), Cursor::Crosshair);
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focus_id(), None, "Tab passes all three by");

    // Focusable, it takes Tab; made not focusable while focused, it gives focus up and Space
    // no longer clicks it.
    h.app_mut().focusable = true;
    h.redraw();
    h.frame();
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focus_id(), Some(Id::of("own")));
    h.key(Key::Char(' '), Modifiers::empty());
    assert_eq!(h.app().clicks, 1);
    h.app_mut().focusable = false;
    h.key(Key::Char(' '), Modifiers::empty());
    assert_eq!((h.focus_id(), h.app().clicks), (None, 1));
  }
}
