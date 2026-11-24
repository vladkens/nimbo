// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The text field over its behaviour, drawn by the environment's [TextFieldStyle], as SwiftUI's
//! `TextField`, `SecureField` and `.textFieldStyle`: the style builds the field's root and the
//! editable text in it from a [TextFieldConfiguration]; [PlainTextFieldStyle], the text alone,
//! unless the shell put its own.
//!
//! ```ignore
//! struct Underlined;
//!
//! impl TextFieldStyle for Underlined {
//!   fn make_body(&self, ui: &mut Ui, mut c: TextFieldConfiguration) {
//!     let fill = if c.is_focused() { FOCUSED } else { GROUND };
//!     let root = Style::new().w(grow()).pad_xy(8.0, 4.0).bg(fill).scroll(c.axis().into());
//!     ui.row(root, |ui| c.text(ui, &FieldStyle::new(TEXT)));
//!   }
//! }
//!
//! ui.text_field_style(&Underlined).text_field(&mut self.name, "Name");
//! TextField::new(&mut self.notes, "Notes").kind(FieldKind::MultiLine).show(ui);
//! ```

use super::FieldBehavior;
use super::button::plain_text;
use crate::keys::EDIT_KEYS;
use crate::ui::{
  Align, Axes, Axis, Cursor, EnvironmentValue, FieldKind, FieldStyle, Focus, Id, Response, Style,
  TextLines, TextStyle, Ui, grow,
};

/// How a text field looks, as SwiftUI's `TextFieldStyle`: [TextFieldStyle::make_body] builds the
/// field's root first, scrolling along [TextFieldConfiguration::axis] (the field gives it its id,
/// focus, I-beam and keys), and the editable text in it. Chosen with [Ui::text_field_style] as
/// `&'static`, a unit struct's `&MyStyle`.
pub trait TextFieldStyle: 'static {
  /// Builds the field `config` describes.
  fn make_body(&self, ui: &mut Ui<'_>, config: TextFieldConfiguration<'_>);
}

/// What a [TextFieldStyle] draws: the field's state this frame, its prompt and its text, built
/// once.
#[non_exhaustive]
#[derive(Debug)]
pub struct TextFieldConfiguration<'s> {
  b: FieldBehavior,
  prompt: &'s str,
  kind: FieldKind,
  empty: bool,
  built: bool,
}

impl TextFieldConfiguration<'_> {
  /// The field's id, its root's.
  pub fn id(&self) -> Id {
    self.b.id
  }

  pub fn is_hovered(&self) -> bool {
    self.b.hovered
  }

  /// Focused in the key window: the core rings its root, the caret blinks.
  pub fn is_focused(&self) -> bool {
    self.b.focused
  }

  /// Not disabled ([Ui::disabled]): it reads input, and is drawn so.
  pub fn is_enabled(&self) -> bool {
    !self.b.disabled
  }

  /// It has no text, nor an input method's composition: the prompt shows.
  pub fn is_empty(&self) -> bool {
    self.empty
  }

  /// Shown while it is empty ([TextField::new]).
  pub fn prompt(&self) -> &str {
    self.prompt
  }

  /// One line, many or a password ([TextField::kind]).
  pub fn kind(&self) -> FieldKind {
    self.kind
  }

  /// Where text past the root goes, as SwiftUI's `TextField(axis:)`: down for many lines,
  /// sideways for one. The root lays out along it and scrolls along it
  /// (`Style::scroll(c.axis().into())`), keeping the caret in view.
  pub fn axis(&self) -> Axis {
    axis(self.kind)
  }

  /// Builds the editable text in `style`, once ([Ui::field_text]): wrapping at the root's width
  /// for many lines, one line otherwise; the prompt in the placeholder colour while empty.
  pub fn text(&mut self, ui: &mut Ui, style: &FieldStyle) {
    if std::mem::replace(&mut self.built, true) {
      return;
    }

    let lines = if self.axis() == Axis::Vertical { TextLines::Wrap } else { TextLines::NoWrap };
    let style = FieldStyle { text: TextStyle { lines, ..style.text }, ..*style };
    ui.field_text(&self.b, self.prompt, &style, Style::new());
  }
}

/// A text field over the app's text, as SwiftUI's `TextField`: one line, many or a password
/// ([FieldKind]), its prompt shown while it is empty, drawn by the environment's
/// [TextFieldStyle] ([Ui::text_field_style]) over [Ui::text_field_behavior].
#[derive(Debug)]
pub struct TextField<'a, 's> {
  text: &'a mut String,
  prompt: &'s str,
  kind: FieldKind,
}

impl<'a, 's> TextField<'a, 's> {
  pub fn new(text: &'a mut String, prompt: &'s str) -> Self {
    Self { text, prompt, kind: FieldKind::SingleLine }
  }

  /// One line (the default), many, as SwiftUI's `TextField(axis: .vertical)`, or a password, as
  /// `SecureField`.
  pub fn kind(self, kind: FieldKind) -> Self {
    Self { kind, ..self }
  }

  /// Builds it under the id set for it, else its place: its behaviour, its name to assistive
  /// technology (the prompt when nothing else names it, as a browser names a field), the
  /// style's body and the edit menu. The values put for it reach the edit menu too.
  pub fn show(self, ui: &mut Ui) -> Response {
    let Self { text, prompt, kind } = self;
    let style = ui.environment_value::<&dyn TextFieldStyle>();
    ui.group(|ui| {
      let id = ui.take_id();
      let b = ui.text_field_behavior(id, text, kind);
      if let Some(a) = ui.a11y_of(id) {
        a.name_or(prompt);
      }

      let empty = ui.st.editor(id).is_none_or(|e| e.text().is_empty());
      let config = TextFieldConfiguration { b, prompt, kind, empty, built: false };
      let root = ui.st.tree.nodes.len();
      style.make_body(ui.control_root(id, b.disabled, Cursor::Text).keymap(EDIT_KEYS), config);
      let n = ui.st.tree.nodes.get_mut(root).filter(|n| n.id == id);
      debug_assert!(
        n.as_ref().is_some_and(|n| n.style.scroll.contains(axis(kind))),
        "a text field style builds the field's root first, scrolling along its axis"
      );

      // A click focuses it, as Tab does.
      if let Some(n) = n {
        n.style.focus = match n.style.focus {
          Focus::Tab => Focus::Click,
          Focus::TabQuiet => Focus::Quiet,
          f => f,
        };
      }

      ui.edit_menu(&b);
      let (changed, submitted, cancelled, blurred) =
        (b.changed, b.submitted, b.cancelled, b.blurred);
      let r = ui.st.response(id);
      Response { hovered: b.hovered, changed, submitted, cancelled, blurred, ..r }
    })
  }
}

/// [TextFieldConfiguration::axis] of a field of `kind`.
fn axis(kind: FieldKind) -> Axis {
  match kind {
    FieldKind::MultiLine => Axis::Vertical,
    _ => Axis::Horizontal,
  }
}

/// The text alone in the text colour of the colour scheme, as SwiftUI's `.plain`: no box, the
/// prompt fainter, dimmed when disabled. The default under the shell's.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlainTextFieldStyle;

impl TextFieldStyle for PlainTextFieldStyle {
  fn make_body(&self, ui: &mut Ui<'_>, mut c: TextFieldConfiguration<'_>) {
    // macOS's label colours: 85% of black or white, its tertiary's 25%.
    let text = plain_text(ui, if c.is_enabled() { 0.85 } else { 0.25 });
    let style = FieldStyle::new(text).placeholder(plain_text(ui, 0.25).color);
    let axis = c.axis();
    let root = Style::new().w(grow()).scroll(Axes::from(axis));
    match axis {
      Axis::Vertical => ui.col(root.h(grow()), |ui| c.text(ui, &style)),
      Axis::Horizontal => ui.row(root.align(Align::Start, Align::Center), |ui| c.text(ui, &style)),
    };
  }
}

/// [PlainTextFieldStyle] where neither a container nor the window nor the shell put one.
impl EnvironmentValue for &'static dyn TextFieldStyle {
  fn default_value(_: &Ui) -> Self {
    &PlainTextFieldStyle
  }
}

impl Ui<'_> {
  /// A single-line [TextField] over `text`, `prompt` shown while it is empty, as SwiftUI's
  /// `TextField(prompt, text:)`.
  pub fn text_field(&mut self, text: &mut String, prompt: &str) -> Response {
    TextField::new(text, prompt).show(self)
  }

  /// A [TextField] for a password, as SwiftUI's `SecureField`: a dot per character, no input
  /// method, nothing copied.
  pub fn secure_field(&mut self, text: &mut String, prompt: &str) -> Response {
    TextField::new(text, prompt).kind(FieldKind::Secure).show(self)
  }

  /// Draws the next text field, or every text field built in the next container, in `style`, as
  /// SwiftUI's `.textFieldStyle`: `ui.text_field_style(&PlainTextFieldStyle).text_field(..)`.
  pub fn text_field_style(&mut self, style: &'static dyn TextFieldStyle) -> &mut Self {
    self.environment(style)
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::app::App;
  use crate::harness::Harness;
  use crate::keys::Key;
  use crate::scene::{Color, Size};
  use crate::text::Font;
  use crate::ui::{Id, UiState, px};
  use crate::{Modifiers, NamedKey};

  const REST: Color = [0.0, 0.0, 1.0, 1.0];
  const FOCUSED: Color = [0.0, 1.0, 0.0, 1.0];
  const OFF: Color = [1.0, 0.0, 0.0, 1.0];

  /// A block of the app's: blue, green focused, red when disabled; it never sets focus, a
  /// pointer or keys.
  struct Block;

  impl TextFieldStyle for Block {
    fn make_body(&self, ui: &mut Ui<'_>, mut c: TextFieldConfiguration<'_>) {
      let fill = match (c.is_enabled(), c.is_focused()) {
        (false, _) => OFF,
        (_, true) => FOCUSED,
        _ => REST,
      };

      let s = Style::new().w(px(120.0)).h(px(30.0)).bg(fill).scroll(c.axis().into());
      let text = FieldStyle::new(TextStyle::new(Font::ui(13.0), [1.0; 4]));
      ui.row(s, |ui| c.text(ui, &text));
    }
  }

  #[derive(Default)]
  struct Blocks {
    text: String,
    off: bool,
    changes: usize,
  }

  impl App for Blocks {
    fn view(&mut self, ui: &mut Ui) {
      let ui = ui.text_field_style(&Block).disabled(self.off);
      self.changes += ui.id("f").text_field(&mut self.text, "Name").changed as usize;
    }
  }

  #[test]
  fn a_style_draws_over_the_behaviour() {
    let mut h = Harness::new(Blocks::default(), Size::new(200.0, 100.0), 1.0);
    let px = |h: &mut Harness<Blocks>| h.render().pixel(100.0, 25.0);
    let rgb = |c: Color| c.map(|v| (v * 255.0) as u8);
    assert_eq!(px(&mut h), rgb(REST));

    // The I-beam over it; a click focuses it, typing writes back.
    h.move_to(100.0, 15.0);
    assert_eq!(h.rt.cursor(), Cursor::Text);
    h.click(100.0, 15.0);
    assert_eq!((h.focused(), px(&mut h)), (Some(Id::of("f")), rgb(FOCUSED)));
    h.type_text("hi");
    assert_eq!((h.app().text.as_str(), h.app().changes), ("hi", 1));
    h.frame();
    assert_eq!(h.text(Id::of("f").key("text")), Some("hi"));
    assert!(h.a11y().contains("text field \"Name\""), "{}", h.a11y());

    // Out, and back by Tab; the edit menu, drawn in the harness, whatever the style.
    h.click(150.0, 80.0);
    assert_eq!(h.focused(), None);
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focused(), Some(Id::of("f")));
    h.right_click(100.0, 15.0);
    h.frame();
    h.frame();
    assert!(h.rect(Id::of("f").key("edit").key("menu")).is_some());
    h.key(NamedKey::Escape, Modifiers::empty());

    // Disabled: its own red, no I-beam, no focus, no typing.
    h.app_mut().off = true;
    h.rt.redraw();
    assert_eq!(px(&mut h), rgb(OFF));
    assert_eq!(h.focused(), None);
    h.move_to(101.0, 15.0);
    assert_eq!(h.rt.cursor(), Cursor::Arrow);
    h.click(100.0, 15.0);
    h.key(NamedKey::Tab, Modifiers::empty());
    h.key(Key::Char('x'), Modifiers::empty());
    assert_eq!((h.focused(), h.app().text.as_str()), (None, "hi"));
  }

  /// Fields in a container given the block style, and one after it.
  #[derive(Default)]
  struct Cascade {
    texts: [String; 3],
  }

  impl App for Cascade {
    fn view(&mut self, ui: &mut Ui) {
      let [a, b, c] = &mut self.texts;
      ui.text_field_style(&Block).col(Style::new(), |ui| {
        ui.id("a").text_field(a, "");
        ui.row(Style::new(), |ui| _ = ui.id("b").text_field(b, ""));
      });
      ui.id("c").text_field(c, "");
    }
  }

  #[test]
  fn a_containers_style_reaches_its_fields_only() {
    let mut h = Harness::new(Cascade::default(), Size::new(200.0, 200.0), 1.0);
    h.frame();
    let side = |n: &str| h.rect(Id::of(n)).map(|r| (r.w, r.h)).unwrap();
    assert_eq!((side("a"), side("b")), ((120.0, 30.0), (120.0, 30.0)));
    assert_ne!(side("c"), (120.0, 30.0));
  }

  /// With no shell, plain: the text alone, no box, taking focus by a click under the I-beam.
  #[test]
  fn plain_is_the_default_without_the_shell() {
    let mut ui = Ui::new(UiState::default(), Size::new(100.0, 100.0), None, Duration::ZERO);
    let mut text = String::new();
    ui.id("f").text_field(&mut text, "Name");
    let n = ui.st.tree.nodes.iter().find(|n| n.id == Id::of("f")).unwrap();
    let s = n.style;
    assert_eq!((s.bg, s.border, s.scroll), ([0.0; 4], [0.0; 4], Axes::HORIZONTAL));
    assert_eq!((s.focus, s.cursor), (Focus::Click, Cursor::Text));
  }

  /// The shell's: the theme's box, a password's text kept from assistive technology.
  struct Shell(String);

  impl App for Shell {
    fn view(&mut self, ui: &mut Ui) {
      ui.id("f").secure_field(&mut self.0, "Password");
    }
  }

  #[test]
  fn the_shell_draws_a_rounded_border() {
    let mut h = Harness::new(Shell("secret".into()), Size::new(200.0, 100.0), 1.0);
    h.frame();
    let s = h.rt.ui.last.get(Id::of("f")).unwrap().style;
    assert_eq!((s.border, s.scroll), ([1.0; 4], Axes::HORIZONTAL));
    assert_ne!(s.bg, [0.0; 4]);
    let a = h.a11y();
    assert!(a.contains("secure text field \"Password\"") && !a.contains("secret"), "{a}");
  }
}
