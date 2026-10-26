// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Text fields and selectable text: the text behaviours ([Ui::text_field_behavior],
//! [Ui::selectable_behavior]) in a box drawn from a [FieldStyle], the theme's by default
//! ([crate::ui::Theme::field]).
//!
//! ```ignore
//! let r = ui.id("name").field(&mut self.name, "Your name");
//! if r.submitted { self.save() }
//! ui.id("key").password_field(&mut self.key, "API key");
//! ui.text_field(Id::of("notes"), &mut self.notes, &t.field().multiline().frame(NOTES));
//! ui.id("about").selectable(ABOUT);
//! ```
//!
//! The app owns the text; the field writes edits back and reports them in its
//! [Response](crate::ui::Response): `changed`, `submitted` (Enter, single line), `cancelled`
//! (Escape) and `blurred`. A single-line field scrolls sideways to keep the caret in view, a
//! multi-line one wraps at its width and scrolls down. Focused, the field is ringed and binds
//! the edit menu's keys ([EDIT_KEYS]); selectable text binds select all and copy
//! ([SELECT_KEYS]) and shows no ring, its selection dimmed while it has no focus. The focused
//! field is what input methods ask about ([TextInput]).

use std::ops::Range;

use crate::keys::{EDIT_KEYS, SELECT_KEYS};
use crate::scene::{Color, Point, Rect, alpha};
use crate::text::edit::{Editor, graphemes, range_from_utf16, range_to_utf16, to_utf16, utf16_len};
use crate::ui::behavior::{DOT, FieldState};
use crate::ui::{
  Align, FieldBehavior, Id, Response, Style, TextLayout, TextStyle, Ui, UiState, grow,
};

impl Ui<'_> {
  /// A text field over `text` in `style`'s look: one line, many ([FieldStyle::multiline]) or a
  /// password ([FieldStyle::password]). [Ui::disabled] dims it and reads no input.
  pub fn text_field(&mut self, id: Id, text: &mut String, style: &FieldStyle) -> Response {
    let b = self.text_field_behavior(id, text, style.multiline, style.password);
    self.field_box(&b, style)
  }

  /// A text field's box and text in `style`'s look over its behaviour `b`.
  pub(super) fn field_box(&mut self, b: &FieldBehavior, style: &FieldStyle) -> Response {
    let border = match b.focused && style.focus_border[3] > 0.0 {
      true => style.focus_border,
      false => style.frame.border_color,
    };

    let mut frame = b.style(Style { border_color: border, ..style.frame });
    if b.disabled {
      frame = frame.opacity(self.theme().disabled);
    }

    let (frame, axis) = match style.multiline {
      true => (frame.scroll_y(), 1),
      false => (frame.scroll_x(), 0),
    };

    let keys = self.keymap(EDIT_KEYS).with_id(b.id);
    let r = keys.container(axis, frame, |ui| ui.field_text(b, style, Style::new()));
    let (changed, submitted, cancelled, blurred) = (b.changed, b.submitted, b.cancelled, b.blurred);
    Response { hovered: b.hovered, changed, submitted, cancelled, blurred, ..r }
  }

  /// A single-line [Ui::text_field] in the theme's look, `hint` shown while it is empty; its id
  /// from [Ui::id] or [Ui::key], else its place.
  pub fn field(&mut self, text: &mut String, hint: &'static str) -> Response {
    let id = self.take_id();
    let style = self.theme().field().hint(hint);
    self.text_field(id, text, &style)
  }

  /// [Ui::field] for a password: a dot per character, no input method, nothing copied.
  pub fn password_field(&mut self, text: &mut String, hint: &'static str) -> Response {
    let id = self.take_id();
    let style = self.theme().field().password().hint(hint);
    self.text_field(id, text, &style)
  }

  /// Text the user can select and copy, in `style`'s text and selection colour (its box is
  /// not drawn): an I-beam over it, focus without a ring for the keys, its selection dimmed
  /// when the focus goes. [Ui::disabled] makes it plain text.
  pub fn selectable_text(&mut self, id: Id, text: &str, style: &FieldStyle) -> Response {
    let b = self.selectable_behavior(id, text);
    self.keymap(SELECT_KEYS).field_text(&b, style, b.style(Style::new()));
    let r = self.st.response(id);
    Response { focused: b.focused, ..r }
  }

  /// [Ui::selectable_text] in the theme's text, wrapping; its id from [Ui::id] or [Ui::key],
  /// else its place.
  pub fn selectable(&mut self, text: &str) -> Response {
    let (id, t) = (self.take_id(), self.theme());
    let style = FieldStyle { text: TextStyle::new(t.font, t.text), ..t.field() };
    self.selectable_text(id, text, &style)
  }

  /// The text element of a field or selectable text, as its behaviour `b` has it: the text (a
  /// dot per character for a password), or the hint while it is empty, with the selection in
  /// `style.selection` (half as strong without focus in selectable text, none in a field), the
  /// caret and the marked text's underline. `node` styles the element itself.
  pub fn field_text(&mut self, b: &FieldBehavior, style: &FieldStyle, node: Style) {
    let st = std::mem::take(self.st.store.raw::<FieldState>(b.id));
    let mut e = b.edit;
    e.caret_color = style.caret;
    e.sel_color = match (b.focused, st.editable) {
      _ if b.disabled => [0.0; 4],
      (true, _) => style.selection,
      (false, false) => alpha(style.selection, style.selection[3] * 0.5),
      (false, true) => [0.0; 4],
    };

    let s = st.ed.text();
    let (ts, empty) = (style.text, s.is_empty() && !style.hint.is_empty());
    match (empty, st.secure) {
      (true, _) => {
        e.placeholder = true;
        let ts = ts.color(style.placeholder);
        self.text_id(b).edit_text(style.hint, ts, e, node);
      }
      (false, true) => {
        let dots = DOT.repeat(graphemes(s).count());
        self.text_id(b).edit_text(&dots, ts, e, node);
      }
      (false, false) => _ = self.text_id(b).edit_text(s, ts, e, node),
    }

    *self.st.store.raw::<FieldState>(b.id) = st;
  }
}

// MARK: Input methods

/// The focused text field as an input method sees it: ranges are UTF-16 offsets of its text
/// (the marked text included), rects are in the window, in points.
pub struct TextInput<'a> {
  ed: &'a Editor,
  /// The last frame's layout of its text, when it shows the text as it is.
  lay: Option<TextLayout<'a>>,
}

impl TextInput<'_> {
  pub fn text(&self) -> &str {
    self.ed.text()
  }

  /// The length in UTF-16 code units.
  pub fn len(&self) -> usize {
    utf16_len(self.ed.text())
  }

  pub fn is_empty(&self) -> bool {
    self.ed.text().is_empty()
  }

  pub fn marked(&self) -> Option<Range<usize>> {
    self.ed.marked().map(|r| range_to_utf16(self.ed.text(), r))
  }

  pub fn selected(&self) -> Range<usize> {
    range_to_utf16(self.ed.text(), self.ed.sel().range())
  }

  /// The text of `r`, clamped to the text, and the range it took.
  pub fn substring(&self, r: Range<usize>) -> (String, Range<usize>) {
    let s = self.ed.text();
    let b = range_from_utf16(s, r.start.min(r.end)..r.end);
    (s[b.clone()].to_owned(), range_to_utf16(s, b))
  }

  /// Where `r` is on its first line, and the part of it there: the caret's rect when it is
  /// empty. `None` before the field is laid out with its text.
  pub fn rect(&self, r: Range<usize>) -> Option<(Rect, Range<usize>)> {
    let (s, lay) = (self.ed.text(), self.lay.as_ref()?);
    let b = range_from_utf16(s, r.start.min(r.end)..r.end);
    let k = lay.line_of(b.start);
    let end = b.end.min(lay.range(k).end).max(b.start);
    let (x0, x1) = (lay.x_in(k, b.start), lay.x_in(k, end));
    let y = lay.rect.y + k as f32 * lay.line_h;
    let rect = Rect::new(lay.rect.x + x0, y, x1 - x0, lay.line_h);
    Some((rect, range_to_utf16(s, b.start..end)))
  }

  /// The caret's rect, or the start of the marked text while composing: where the candidate
  /// window goes, under it.
  pub fn caret_rect(&self) -> Option<Rect> {
    let r = self.marked().unwrap_or_else(|| self.selected());
    self.rect(r.start..r.start).map(|(rect, _)| rect)
  }

  /// The offset nearest to `p` in the window.
  pub fn index(&self, p: Point) -> Option<usize> {
    let lay = self.lay.as_ref()?;
    Some(to_utf16(self.ed.text(), lay.hit(p, false)))
  }
}

impl UiState {
  /// The focused text field for an input method; `None` unless an editable field that takes
  /// input methods (not a password) has focus.
  pub fn text_input(&self) -> Option<TextInput<'_>> {
    let id = self.input.focus?;
    let st = self.store.peek::<FieldState>(id).filter(|s| s.editable && !s.secure)?;
    let lay = TextLayout::of(&self.last, st.text_id).filter(|l| l.text == st.ed.text());
    Some(TextInput { ed: &st.ed, lay })
  }
}

/// How a text field looks: its box, text, placeholder and selection, and what it holds: one
/// line or many, or a password. `const` builders, like [Style].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldStyle {
  /// The box: size, padding, background, border, radius. A single-line field lays out as a
  /// row, a multi-line one as a column; both scroll.
  pub frame: Style,
  pub text: TextStyle,
  /// Shown, in `placeholder`, while the text is empty.
  pub hint: &'static str,
  pub placeholder: Color,
  pub selection: Color,
  pub caret: Color,
  /// The border's colour while focused; transparent keeps the frame's.
  pub focus_border: Color,
  /// Wraps lines and takes Enter as a line break.
  pub multiline: bool,
  /// Shows a dot per character, takes no input method, and keeps its text off the clipboard.
  pub password: bool,
}

impl FieldStyle {
  /// A single-line field growing along its parent, with `text`'s font and colour.
  pub const fn new(text: TextStyle) -> Self {
    Self {
      frame: Style::new().w(grow()).pad_xy(6.0, 4.0).align(Align::Start, Align::Center),
      text: text.nowrap(),
      hint: "",
      placeholder: alpha(text.color, 0.4),
      selection: [0.25, 0.47, 0.95, 0.45],
      caret: text.color,
      focus_border: [0.0; 4],
      multiline: false,
      password: false,
    }
  }

  pub const fn frame(self, frame: Style) -> Self {
    Self { frame, ..self }
  }

  /// Wraps and takes line breaks; give the frame a height, it scrolls past it.
  pub const fn multiline(self) -> Self {
    let text = TextStyle { wrap: true, ..self.text };
    Self { text, multiline: true, password: false, ..self }
  }

  /// One line of secret text, shown as dots.
  pub const fn password(self) -> Self {
    let text = TextStyle { wrap: false, ..self.text };
    Self { text, multiline: false, password: true, ..self }
  }

  pub const fn hint(self, hint: &'static str) -> Self {
    Self { hint, ..self }
  }

  pub const fn placeholder(self, placeholder: Color) -> Self {
    Self { placeholder, ..self }
  }

  pub const fn selection(self, selection: Color) -> Self {
    Self { selection, ..self }
  }

  pub const fn caret(self, caret: Color) -> Self {
    Self { caret, ..self }
  }

  pub const fn focus_border(self, focus_border: Color) -> Self {
    Self { focus_border, ..self }
  }
}

#[cfg(test)]
mod tests {
  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::Size;
  use crate::ui::{Cursor, Id, Ui};

  /// Disabled selectable text, then a button and a field.
  #[derive(Default)]
  struct Off {
    clicks: u32,
    text: String,
  }

  impl App for Off {
    fn view(&mut self, ui: &mut Ui) {
      ui.disabled(true).id("note").selectable("Plain text");
      if ui.id("b").button("B").clicked {
        self.clicks += 1;
      }

      ui.id("f").field(&mut self.text, "");
    }
  }

  /// Lines of selectable text in a scrolled page 40 high.
  struct Page;

  impl App for Page {
    fn view(&mut self, ui: &mut Ui) {
      let page = crate::ui::Style::new().w(crate::ui::px(200.0)).h(crate::ui::px(40.0)).scroll_y();
      ui.id("page").col(page, |ui| _ = ui.id("t").selectable(&"line\n".repeat(20)));
    }
  }

  #[test]
  fn a_selection_moved_by_keys_scrolls_into_view() {
    let mut h = Harness::new(Page, Size::new(300.0, 200.0), 1.0);
    h.frame();
    h.click(10.0, 5.0);
    for _ in 0..8 {
      h.key(crate::NamedKey::ArrowDown, crate::Modifiers::SHIFT);
    }

    h.frame();
    assert!(h.rt.ui.last.get(Id::of("page")).unwrap().scroll[1] > 0.0);
    // Page Down is not the text's: it goes on to the app.
    let page_down = crate::app::Event::Command(crate::keys::Command::ScrollPageDown);
    assert!(!h.event(page_down));
  }

  /// A press and a move below the page before any frame: the drag reaches down at once, and
  /// frames go on scrolling while the pointer rests below the page's visible part.
  #[test]
  fn a_drag_below_a_scrolled_page_selects_and_scrolls_on() {
    let mut h = Harness::new(Page, Size::new(300.0, 200.0), 1.0);
    h.frame();
    h.press(10.0, 5.0);
    h.move_to(10.0, 70.0);
    h.frame();
    assert!(h.editor(Id::of("t")).unwrap().sel().head > 0);
    assert!(h.rt.due_in().is_some_and(|d| d <= std::time::Duration::from_millis(16)));
  }

  #[test]
  fn disabled_selectable_text_is_plain_and_keeps_the_flag_to_itself() {
    let mut h = Harness::new(Off::default(), Size::new(300.0, 200.0), 1.0);
    h.frame();
    let note = h.rect(Id::of("note")).unwrap();
    h.move_to(note.x + 5.0, note.y + 5.0);
    assert_eq!(h.rt.cursor(), Cursor::Arrow);
    h.click(note.x + 5.0, note.y + 5.0);
    assert_eq!(h.focused(), None);
    assert!(h.click_id(Id::of("b")));
    assert_eq!(h.app().clicks, 1);
    h.click_id(Id::of("f"));
    h.type_text("on");
    assert_eq!(h.app().text, "on");
  }
}
