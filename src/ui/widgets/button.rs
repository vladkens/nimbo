// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The button: a label or an icon over the button behaviour, drawn from the theme's
//! variants at the control size ([Ui::control_size]); the ⓘ that explains a row, the warning mark
//! that tells what is wrong with it, and the round "?" that opens a page's help.

use super::icon::{self, Icon};
use crate::scene::{Color, Frame, Rect, mix};
use crate::ui::theme::{ButtonLook, ButtonStyle};
use crate::ui::{Float, Response, Role, Style, TextStyle, Ui, px};

/// How far below the middle of the line a mark ([Ui::info_button], [Ui::warning_button]) goes,
/// while a form row builds its label ([crate::FormRow::new]): onto its capitals' middle.
pub(crate) struct MarkShift(pub f32);

/// What a button does, as SwiftUI's `ButtonRole`: a [crate::Dialog]'s buttons are told apart by
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonRole {
  /// The action that goes ahead, the default one.
  Confirm,
  /// Keeps things as they are.
  Cancel,
  /// Deletes or loses something: drawn red.
  Destructive,
}

/// A button with a label, an [Icon], or both, at the control size ([Ui::control_size]).
#[derive(Clone, Copy, Debug)]
pub struct Button<'s> {
  label: &'s str,
  icon: Option<Icon>,
  style: ButtonStyle,
  role: Option<ButtonRole>,
  selected: bool,
  look: Option<ButtonLook>,
}

impl<'s> Button<'s> {
  pub fn new(label: &'s str) -> Self {
    let (style, role, selected, look) = (ButtonStyle::Bordered, None, false, None);
    Self { label, icon: None, style, role, selected, look }
  }

  /// A square borderless button showing an icon, muted until the pointer is on it: reset,
  /// delete, open elsewhere.
  pub fn icon(icon: Icon) -> Self {
    Self { icon: Some(icon), style: ButtonStyle::Borderless, ..Self::new("") }
  }

  /// An icon before the label.
  pub fn with_icon(self, icon: Icon) -> Self {
    Self { icon: Some(icon), ..self }
  }

  /// Its style, as SwiftUI's `.buttonStyle`: [ButtonStyle::BorderedProminent] for the default
  /// action.
  pub fn style(self, style: ButtonStyle) -> Self {
    Self { style, ..self }
  }

  /// What it does, as SwiftUI's `Button(role:)`: [ButtonRole::Destructive] draws it red.
  pub fn role(self, role: ButtonRole) -> Self {
    Self { role: Some(role), ..self }
  }

  /// Shows it selected or toggled on ([crate::ui::Theme::selected]) when `on`: a sidebar's toggle,
  /// the current choice among buttons.
  pub fn selected(self, on: bool) -> Self {
    Self { selected: on, ..self }
  }

  /// Its own look instead of its style's, usually derived: `t.button(ButtonStyle::Bordered,
  /// None, ControlSize::Regular).height(36.0)`.
  pub fn look(self, look: ButtonLook) -> Self {
    Self { look: Some(look), ..self }
  }

  pub fn show(self, ui: &mut Ui) -> Response {
    let (t, size) = (ui.theme(), ui.take_control_size());
    let square = self.label.is_empty() && self.icon.is_some();
    let s = self.look.unwrap_or_else(|| {
      let s = t.button(self.style, self.role, size);
      let h = s.frame.min[0];
      if square { ButtonLook { frame: s.frame.w(px(h)).pad(0.0), ..s } } else { s }
    });
    let s = if self.selected { t.selected(s) } else { s };
    let id = ui.take_id();
    let b = ui.button_behavior(id);
    // An icon alone names it when nothing else does.
    let icon = self.icon.filter(|_| self.label.is_empty()).and_then(Icon::name);
    if let Some((a, n)) = ui.a11y_of(id).zip(icon) {
      a.name_or(n);
    }

    // Borderless buttons are toolbar buttons, which macOS hovers; the others only with
    // hover_all.
    let hot = match self.style {
      ButtonStyle::Borderless => ui.hot(id, b.hovered),
      _ => ui.ctl_hot(id, b.hovered),
    };

    let bg = if b.pressed { s.pressed } else { mix(s.bg, s.hover, hot) };
    let frame = ui.control(s.frame.bg(bg), &b);
    // An icon button's mark is muted until the pointer is on it.
    let ink = match square && self.look.is_none() && !self.selected {
      true => mix(t.muted, t.text, if b.pressed { 1.0 } else { hot }),
      false => s.text.color,
    };
    // Canvases draw past the frame's opacity: the icon dims itself.
    let ink = ui.dim(ink, b.disabled);

    let size = (s.text.font.size + 2.0).round();
    let mut r = ui.with_id(id).row(frame, |ui| {
      if let Some(i) = self.icon {
        let i = i.stroke_or(1.6);
        ui.canvas(Style::new().w(px(size)).h(px(size)), move |f, r| icon::draw(f, i, r, ink));
      }

      if !self.label.is_empty() {
        ui.text(self.label, s.text);
      }
    });
    r.clicked = b.clicked;
    // Disabled, it reads no input: the tree's hit is under it all the same.
    if b.disabled {
      (r.hovered, r.pressed, r.focused) = (false, false, false);
    }

    r
  }
}

impl Ui<'_> {
  /// A [Button] with the normal look.
  pub fn button(&mut self, label: &str) -> Response {
    Button::new(label).show(self)
  }

  /// A square ghost button showing an icon ([Button::icon]).
  pub fn icon_button(&mut self, icon: Icon) -> Response {
    Button::icon(icon).show(self)
  }

  /// An ⓘ, 16 points, muted, in the label colour while held, halfway there under the pointer
  /// where the theme hovers controls ([crate::Theme::hover_all]): with [Ui::help] on it, an
  /// explanation longer than a line, as SwiftUI's `Image(systemName: "info.circle").help(…)`.
  /// A button by its behaviour, named "Info". A mark is there for its tip: a container's
  /// [Ui::disabled] leaves it enabled. In a form row's label ([crate::FormRow::new]) it sits on
  /// the middle of the label's capitals, the line keeping its height.
  pub fn info_button(&mut self) -> Response {
    let t = self.theme();
    self.mark_button(Icon::INFO, [t.muted, t.text])
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
    // Enabled in a disabled container: its tip reads all the same.
    let scope = std::mem::replace(&mut self.scope_off, false);
    let b = self.button_behavior(id);
    self.scope_off = scope;
    self.a11y(id, Role::Button).name_or(icon.name().unwrap_or_default());
    let hot = self.ctl_hot(id, b.hovered);
    let size = (t.font.size + 3.0).round();
    let k = if b.pressed { 1.0 } else { 0.5 * hot };
    let color = self.dim(mix(ink[0], ink[1], k), b.disabled);
    let s = self.control(Style::new().w(px(size)).h(px(size)).radius(size / 2.0), &b);
    let paint = move |f: &mut Frame, r: Rect| icon::draw(f, icon.stroke_or(1.5), r, color);
    let Some(&mut MarkShift(dy)) = self.slot::<MarkShift>() else {
      let mut r = self.with_id(id).canvas(s, paint);
      r.clicked = b.clicked;
      return r;
    };

    // On a form label's line: in a box as high as the mark, moved onto the capitals' middle.
    let help = self.next_help.take();
    let mut r = Response::default();
    self.row(Style::new().w(px(size)).h(px(size)), |ui| {
      ui.next_help = help;
      r = ui.with_id(id).float(Float::parent().offset(0.0, dy).clip()).canvas(s, paint);
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
    self.a11y(id, Role::Button).name_or("Help");
    let hot = self.ctl_hot(id, b.hovered);
    let d = t.sizes.control_h;
    let bg = if b.pressed { t.pressed } else { mix(t.raised, t.hover, hot) };
    let s = self.control(Style::new().w(px(d)).h(px(d)).center().radius(d / 2.0).bg(bg), &b);
    let text = TextStyle::new(t.bold, t.text).fixed_size();
    let mut r = self.with_id(id).row(s, |ui| ui.text("?", text));
    r.clicked = b.clicked;
    r
  }
}

#[cfg(test)]
mod tests {
  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::Size;
  use crate::text::Font;
  use crate::ui::{ControlSize, Id, Response, Style, TextStyle, Ui, px};

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
      self.0 += 10 * ui.id("on").button("On").clicked as usize;
    }
  }

  /// [Ui::control_size] sizes the next widget or every widget in the next container, until it
  /// closes; [Ui::disabled] on a container disables what is in it, not what follows.
  #[test]
  fn sizes_and_disabled_scope_a_container() {
    let mut h = Harness::new(Scoped::default(), Size::new(300.0, 300.0), 1.0);
    h.frame();
    let z = h.rt.ui.theme.sizes;
    let height = |n: &str| h.rect(Id::of(n)).unwrap().h;
    let large = z.control_h + 8.0;
    let hs = [height("small"), height("large"), height("inner"), height("regular")];
    assert_eq!(hs, [z.small_h, large, z.small_h, z.control_h]);
    h.click_id(Id::of("off"));
    h.click_id(Id::of("on"));
    assert_eq!(h.app().0, 10, "only the button after the container clicked");
  }

  /// An icon button, disabled or not, in the light theme.
  struct Reset(bool);

  impl App for Reset {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_theme(crate::ui::Theme::PLATES_LIGHT);
      ui.col(Style::new().grow().bg(ui.theme().bg), |ui| {
        ui.disabled(self.0).id("r").icon_button(crate::ui::Icon::RESET);
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
}
