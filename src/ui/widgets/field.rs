// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Text fields and selectable text: the text behaviours ([Ui::text_field_behavior],
//! [Ui::selectable_behavior]) in a box drawn from a [FieldStyle], the theme's by default
//! ([crate::ui::Theme::field]); the text in it is [Ui::field_text]'s, the edit menu
//! [Ui::edit_menu]'s.
//!
//! ```ignore
//! let r = ui.id("name").field(&mut self.name, "Your name");
//! if r.submitted { self.save() }
//! ui.id("key").password_field(&mut self.key, "API key");
//! ui.id("notes").text_field(&mut self.notes, &t.field().kind(FieldKind::MultiLine).frame(NOTES));
//! ui.id("about").selectable(ABOUT);
//! ```
//!
//! The app owns the text; the field writes edits back and reports them in its
//! [Response](crate::ui::Response): `changed`, `submitted` (Enter, single line), `cancelled`
//! (Escape) and `blurred`. A single-line field scrolls sideways to keep the caret in view, a
//! multi-line one wraps at its width and scrolls down. Focused, the field is ringed and binds
//! the edit menu's keys ([EDIT_KEYS]); selectable text binds select all and copy
//! ([SELECT_KEYS]) and shows no ring, its selection dimmed while it has no focus. A right-click
//! opens the edit menu at the pointer ([Ui::edit_menu]): Cut, Copy, Paste and Select All, or
//! Copy and Select All in selectable text, the platform's own menu in a window. The focused
//! field is what input methods ask about ([crate::ui::TextInput]).

use crate::keys::{EDIT_KEYS, SELECT_KEYS};
use crate::scene::Point;
use crate::ui::{
  Axes, FieldBehavior, FieldKind, FieldStyle, NativeMenu, Response, Style, TextStyle, Ui,
};

impl Ui<'_> {
  /// A text field over `text` in `style`'s look, as SwiftUI's `TextField` with a
  /// `textFieldStyle`: one line, many or a password ([FieldKind]); its id from [Ui::id] or
  /// [Ui::key], else its place.
  /// [Ui::disabled] dims it and reads no input.
  pub fn text_field(&mut self, text: &mut String, style: &FieldStyle) -> Response {
    let id = self.take_id();
    let b = self.text_field_behavior(id, text, style.kind);
    self.field_box(&b, style)
  }

  /// A text field's box and text in `style`'s look over its behaviour `b`.
  pub(super) fn field_box(&mut self, b: &FieldBehavior, style: &FieldStyle) -> Response {
    // Named by its hint when nothing else names it, as a browser names a field.
    if let Some(a) = self.a11y_of(b.id) {
      a.name_or(style.hint);
    }

    let border = match b.focused && style.focus_border[3] > 0.0 {
      true => style.focus_border,
      false => style.frame.border_color,
    };

    let mut frame = b.style(Style { border_color: border, ..style.frame });
    if b.disabled {
      frame = frame.opacity(self.theme().disabled);
    }

    let keys = self.keymap(EDIT_KEYS).with_id(b.id);
    let text = |ui: &mut Self| ui.field_text(b, style, Style::new());
    let r = match style.kind {
      FieldKind::MultiLine => keys.col(frame.scroll(Axes::VERTICAL), text),
      _ => keys.row(frame.scroll(Axes::HORIZONTAL), text),
    };

    self.edit_menu(b);
    let (changed, submitted, cancelled, blurred) = (b.changed, b.submitted, b.cancelled, b.blurred);
    Response { hovered: b.hovered, changed, submitted, cancelled, blurred, ..r }
  }

  /// A single-line [Ui::text_field] in the theme's look, `hint` shown while it is empty:
  /// SwiftUI's `TextField(hint, text:)`.
  pub fn field(&mut self, text: &mut String, hint: &'static str) -> Response {
    let style = self.theme().field().hint(hint);
    self.text_field(text, &style)
  }

  /// [Ui::field] for a password, as SwiftUI's `SecureField`: a dot per character, no input
  /// method, nothing copied.
  pub fn password_field(&mut self, text: &mut String, hint: &'static str) -> Response {
    let style = self.theme().field().kind(FieldKind::Secure).hint(hint);
    self.text_field(text, &style)
  }

  /// Text the user can select and copy, as SwiftUI's `.textSelection(.enabled)` on a `Text`, in
  /// `style`'s text and selection colour (its box is not drawn): an I-beam over it, focus
  /// without a ring for the keys, its selection dimmed when the focus goes; its id from
  /// [Ui::id] or [Ui::key], else its place. [Ui::disabled] makes it plain text.
  pub fn selectable_text(&mut self, text: &str, style: &FieldStyle) -> Response {
    let id = self.take_id();
    let b = self.selectable_behavior(id, text);
    self.keymap(SELECT_KEYS).field_text(&b, style, b.style(Style::new()));
    self.edit_menu(&b);
    let (hovered, pressed, clicked) = (self.hovered(id), self.pressed(id), self.clicked(id));
    Response { id, hovered, pressed, focused: b.focused, clicked, ..Response::default() }
  }

  /// The edit menu of a field or selectable text over its behaviour `b`, at the secondary click
  /// that opened it ([crate::ui::FieldBehavior::menu]): [crate::ui::EditMenu::items] as
  /// `b.id.key("edit")`'s context menu ([Ui::context_menu_at]), the native one or drawn in the
  /// theme's look; the one chosen runs in the field ([Ui::field_command]).
  pub fn edit_menu(&mut self, b: &FieldBehavior) {
    let (id, m) = (b.id.key("edit"), b.menu);
    let chosen = match self.native_menu(id, m.at, || m.items()) {
      NativeMenu::Answered(cmd) => cmd,
      NativeMenu::Unanswered => None,
      NativeMenu::Unavailable => {
        // Disabled while open, a drawn one closes.
        let off = b.disabled && self.menu_open(id);
        self.drawn_context_menu(id, m.at.or(off.then(Point::default)), || m.items())
      }
    };

    if let Some(cmd) = chosen {
      self.field_command(b.id, cmd);
    }
  }

  /// [Ui::selectable_text] in the theme's text, wrapping.
  pub fn selectable(&mut self, text: &str) -> Response {
    let t = self.theme();
    let style = FieldStyle { text: TextStyle::new(t.font, t.text), ..t.field() };
    self.selectable_text(text, &style)
  }
}

#[cfg(test)]
mod tests {
  use crate::app::{App, Event};
  use crate::harness::Harness;
  use crate::keys::Command;
  use crate::scene::Size;
  use crate::ui::{Axes, Cursor, Id, Style, Ui, px};

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
      let page = Style::new().w(px(200.0)).h(px(40.0)).scroll(Axes::VERTICAL);
      ui.id("page").col(page, |ui| _ = ui.id("t").selectable(&"line\n".repeat(20)));
    }
  }

  /// Two fields without ids, the first one disabled at will.
  #[derive(Default)]
  struct Two {
    texts: [String; 2],
    off: bool,
  }

  impl App for Two {
    fn view(&mut self, ui: &mut Ui) {
      let [a, b] = &mut self.texts;
      ui.disabled(self.off).field(a, "");
      ui.field(b, "");
    }
  }

  /// The second of two fields, and the first one's edit menu; open: whether it is.
  fn two() -> (Harness<Two>, Id, Id) {
    let mut h = Harness::new(Two::default(), Size::new(300.0, 200.0), 1.0);
    h.frame();
    let (first, second) = (h.rt.ui.last.nodes[1].id, h.rt.ui.last.nodes[3].id);
    (h, first, second)
  }

  fn right_click(h: &mut Harness<Two>, id: Id) {
    let r = h.rect(id).unwrap();
    h.right_click(r.x + 10.0, r.y + 5.0);
    h.frame();
    h.frame();
  }

  /// An edit menu open takes no place among the field's siblings: the next field keeps its id,
  /// and its editor with its undo.
  #[test]
  fn an_open_edit_menu_leaves_the_next_fields_id() {
    let (mut h, first, second) = two();
    h.click_id(second);
    h.type_text("moon");
    right_click(&mut h, first);
    assert!(h.rect(first.key("edit").key("menu")).is_some());
    assert!(h.rect(second).is_some(), "the second field keeps its id");
    h.key(crate::NamedKey::Escape, crate::Modifiers::empty());
    h.frame();
    h.click_id(second);
    h.key('z', crate::CMD);
    assert_eq!(h.app().texts[1], "");
  }

  /// A field disabled while its edit menu is open closes it, and drops a choice made just
  /// before.
  #[test]
  fn a_field_disabled_drops_its_edit_menu() {
    let (mut h, first, _) = two();
    h.rt.clipboard().write("moon");
    right_click(&mut h, first);
    h.app_mut().off = true;
    h.frame();
    h.frame();
    assert!(h.rect(first.key("edit").key("menu")).is_none());

    h.app_mut().off = false;
    h.frame();
    right_click(&mut h, first);
    h.click_menu_item("Paste");
    h.app_mut().off = true;
    h.frame();
    h.app_mut().off = false;
    h.frame();
    assert_eq!(h.app().texts[0], "");
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
    let page_down = Event::Command(Command::ScrollPageDown);
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
