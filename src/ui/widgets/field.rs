// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The theme's text field look, as SwiftUI's `.roundedBorder` ([RoundedBorderTextFieldStyle]),
//! and selectable text: the text behaviours ([Ui::text_field_behavior],
//! [Ui::selectable_behavior]) in a box drawn from a [FieldStyle], the theme's
//! ([crate::ui::Theme::field]); the text in it is [Ui::field_text]'s, the edit menu
//! [Ui::edit_menu]'s.
//!
//! ```ignore
//! let r = ui.id("name").text_field(&mut self.name, "Your name");
//! if r.submitted { self.save() }
//! ui.id("key").secure_field(&mut self.key, "API key");
//! TextField::new(&mut self.notes, "Notes").axis(Axis::Vertical).show(ui.id("notes"));
//! ui.id("about").selectable_text(ABOUT, t.body());
//! ```
//!
//! The app owns the text; the field writes edits back and reports them in its
//! [Response](crate::ui::Response): `changed`, `submitted` (Enter, single line), `cancelled`
//! (Escape) and `blurred`. A single-line field scrolls sideways to keep the caret in view, a
//! multi-line one wraps at its width, fills the height its parent gives it and scrolls down.
//! Focused, the field is ringed and binds the edit menu's keys
//! ([EDIT_KEYS](crate::keys::EDIT_KEYS)); selectable text
//! binds select all and copy ([SELECT_KEYS]) and shows no ring, its selection dimmed while it
//! has no focus. A right-click opens the edit menu at the pointer ([Ui::edit_menu]): Cut, Copy,
//! Paste and Select All, or Copy and Select All in selectable text, the platform's own menu in a
//! window. The focused field is what input methods ask about ([crate::ui::TextInput]).

use crate::keys::SELECT_KEYS;
use crate::ui::{
  Align, Axes, Axis, FieldStyle, Response, Selectable, Style, TextFieldConfiguration,
  TextFieldStyle, TextStyle, Ui, grow,
};

/// The theme's text field ([crate::ui::Theme::field]), as SwiftUI's `.roundedBorder`: the
/// field's ground in a border with the control's corners, its focus border while focused,
/// dimmed when disabled. The shell's default.
#[derive(Clone, Copy, Debug, Default)]
pub struct RoundedBorderTextFieldStyle;

impl TextFieldStyle for RoundedBorderTextFieldStyle {
  fn make_body(&self, ui: &mut Ui<'_>, mut c: TextFieldConfiguration<'_>) {
    let style = ui.theme().field();
    let frame = field_frame(ui, &style, c.is_focused(), c.is_enabled());
    let axis = c.axis();
    let text = |ui: &mut Ui| c.text(ui, &style);
    match axis {
      Axis::Vertical => {
        let frame = frame.h(grow()).align(Align::Start, Align::Start);
        ui.col(frame.scroll(Axes::VERTICAL), text)
      }
      Axis::Horizontal => ui.row(frame.scroll(Axes::HORIZONTAL), text),
    };
  }
}

/// A field's box in `style`: its border the focus border while `focused`, if it has one; dimmed
/// unless `enabled`.
pub(super) fn field_frame(ui: &Ui, style: &FieldStyle, focused: bool, enabled: bool) -> Style {
  let border = match focused && style.focus_border[3] > 0.0 {
    true => style.focus_border,
    false => style.frame.border_color,
  };

  let frame = Style { border_color: border, ..style.frame };
  if enabled { frame } else { frame.opacity(ui.theme().disabled) }
}

impl Ui<'_> {
  /// Text the user can select and copy in `style`, as SwiftUI's `.textSelection(.enabled)` on a
  /// `Text`, selected in the theme's field colour ([crate::ui::Theme::field]): an I-beam over it,
  /// focus without a ring for the keys, its selection dimmed when the focus goes; its id from
  /// [Ui::id] or [Ui::key], else its place. A `&str` or an [AttributedString] (`&code`), whose
  /// characters it copies. [Ui::disabled] makes it plain text.
  pub fn selectable_text<'s>(
    &mut self,
    text: impl Into<Selectable<'s>>,
    style: TextStyle,
  ) -> Response {
    let style = FieldStyle { text: style, ..self.theme().field() };
    self.group(|ui| {
      let id = ui.take_id();
      let b = ui.selectable_behavior(id, text);
      ui.keymap(SELECT_KEYS).field_text(&b, "", &style, b.style(Style::new()));
      ui.edit_menu(&b);
      let (hovered, pressed, clicked) = (ui.hovered(id), ui.pressed(id), ui.clicked(id));
      Response { id, hovered, pressed, focused: b.focused, clicked, ..Response::default() }
    })
  }
}

#[cfg(test)]
mod tests {
  use crate::Harness;
  use crate::app::{App, Event};
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
      let body = ui.theme().body();
      ui.disabled(true).id("note").selectable_text("Plain text", body);
      if ui.id("b").button("B").clicked {
        self.clicks += 1;
      }

      ui.id("f").text_field(&mut self.text, "");
    }
  }

  /// Lines of selectable text in a scrolled page 40 high.
  struct Page;

  impl App for Page {
    fn view(&mut self, ui: &mut Ui) {
      let page = Style::new().w(px(200.0)).h(px(40.0)).scroll(Axes::VERTICAL);
      let body = ui.theme().body();
      ui.id("page").col(page, |ui| _ = ui.id("t").selectable_text(&"line\n".repeat(20), body));
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
      ui.disabled(self.off).text_field(a, "");
      ui.text_field(b, "");
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
    assert!(h.due_in().is_some_and(|d| d <= std::time::Duration::from_millis(16)));
  }

  #[test]
  fn disabled_selectable_text_is_plain_and_keeps_the_flag_to_itself() {
    let mut h = Harness::new(Off::default(), Size::new(300.0, 200.0), 1.0);
    h.frame();
    let note = h.rect(Id::of("note")).unwrap();
    h.move_to(note.x + 5.0, note.y + 5.0);
    assert_eq!(h.cursor(), Cursor::Arrow);
    h.click(note.x + 5.0, note.y + 5.0);
    assert_eq!(h.focus_id(), None);
    assert!(h.click_id(Id::of("b")));
    assert_eq!(h.app().clicks, 1);
    h.click_id(Id::of("f"));
    h.type_text("on");
    assert_eq!(h.app().text, "on");
  }

  /// Two selectable texts and a field over room to click in.
  #[derive(Default)]
  struct Texts {
    text: String,
  }

  impl App for Texts {
    fn view(&mut self, ui: &mut Ui) {
      let body = ui.theme().body();
      ui.col(Style::new().w(px(300.0)).h(px(200.0)).gap(10.0), |ui| {
        ui.id("a").selectable_text("Hello world", body);
        ui.id("b").selectable_text("Other words", body);
        ui.id("f").text_field(&mut self.text, "");
      });
    }
  }

  /// Selects all of `id` with a click and Cmd-A.
  fn select_all(h: &mut Harness<Texts>, id: Id) {
    h.click_id(id);
    h.key('a', crate::CMD);
    h.frame();
    let len = h.text(id).unwrap().len();
    assert_eq!(h.selection(id), Some((0, len)));
  }

  /// A click elsewhere in the window (on room, on other text) clears a selectable text's or a
  /// field's selection, the field losing its focus; the window losing its focus keeps them,
  /// shown.
  #[test]
  fn a_click_elsewhere_clears_the_selection_the_window_keeps_it() {
    let mut h = Harness::new(Texts { text: "typed".into() }, Size::new(300.0, 200.0), 1.0);
    h.frame();
    let (a, b, f) = (Id::of("a"), Id::of("b"), Id::of("f"));
    let collapsed = |h: &Harness<Texts>, id| h.selection(id).is_some_and(|(x, y)| x == y);
    let r = h.rect(a).unwrap();
    // Under the baseline, off the glyphs.
    let at = (r.x + 10.0, r.y + r.h - 2.0);

    select_all(&mut h, a);
    let shown = h.render().pixel(at.0, at.1);
    h.event(Event::Focus(false));
    h.frame();
    assert_eq!(h.selection(a), Some((0, 11)));
    let inactive = h.render().pixel(at.0, at.1);

    h.event(Event::Focus(true));
    h.frame();
    assert_eq!(h.render().pixel(at.0, at.1), shown);
    h.click(250.0, 190.0);
    h.frame();
    assert!(collapsed(&h, a) && h.focus_id().is_none());
    let plain = h.render().pixel(at.0, at.1);
    assert!(plain != inactive && inactive != shown, "{plain:?} {inactive:?} {shown:?}");

    select_all(&mut h, a);
    h.click_id(b);
    h.frame();
    assert!(collapsed(&h, a));

    // The field's text, every pixel of it.
    let fr = h.rect(f).unwrap();
    let look = |h: &mut Harness<Texts>| {
      let img = h.render();
      let xs = (0..fr.w as usize).map(|x| fr.x + x as f32);
      xs.flat_map(|x| (4..fr.h as usize - 4).map(move |y| (x, fr.y + y as f32)))
        .map(|(x, y)| img.pixel(x, y))
        .collect::<Vec<_>>()
    };
    select_all(&mut h, f);
    h.event(Event::Focus(false));
    h.frame();
    let inactive = look(&mut h);

    h.event(Event::Focus(true));
    h.frame();
    assert_eq!((h.selection(f), h.focus_id()), (Some((0, 5)), Some(f)));
    h.click(250.0, 190.0);
    h.frame();
    assert!(collapsed(&h, f) && h.focus_id().is_none());
    assert_ne!(look(&mut h), inactive, "the inactive window shows the field's selection");
  }
}
