// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The search field: a single-line text field with a magnifier, and a ⊗ that clears it while
//! it has text, as macOS's search fields; its look from the theme ([crate::ui::Theme::search]).
//!
//! ```ignore
//! if ui.id("search").search(&mut self.query, "Search models").changed { self.filter() }
//! ```
//!
//! A click on the ⊗ (`id.key("clear")`) clears the text and focuses the field, ready for the
//! next search; Escape clears it too, and goes on to the app only when the field is empty
//! already, so an Escape that cleared it doesn't also close a dialog.

use super::icon::{self, Icon};
use super::shortcut::clear_mark;
use crate::ui::{Cursor, FieldBehavior, FieldStyle, Float, Id, Response, Style, Ui, px};

/// The ⊗'s side, and how far in it is from the field's right edge.
const MARK: f32 = 14.0;
const INSET: f32 = 8.0;

impl Ui<'_> {
  /// A search field's input: [Ui::text_field_behavior]'s on one line, and a click on
  /// `id.key("clear")` (the ⊗ a look draws while there is text) or Escape clears the text,
  /// `changed`; the ⊗ focuses the field, and an Escape that cleared it goes no further.
  pub fn search_behavior(&mut self, id: Id, text: &mut String) -> FieldBehavior {
    let off = self.next_disabled || self.scope_off;
    let cleared = !off && self.clicked(id.key("clear")) && !text.is_empty();
    if cleared {
      text.clear();
      self.focus(Some(id));
    }

    let mut b = self.text_field_behavior(id, text, false, false);
    if b.cancelled && !text.is_empty() {
      text.clear();
      b.changed = true;
      // Taken again: the field gave it back to the app.
      _ = self.command(id);
    }

    b.changed |= cleared;
    b
  }

  /// A search field over `text` ([Ui::search_behavior]) in `style`'s look: its box with a
  /// magnifier at the left in the muted colour and, while there is text, a ⊗ at the right
  /// (darker under the pointer), the text kept clear of it. [Ui::disabled] dims it, with no ⊗.
  pub fn search_field(&mut self, id: Id, text: &mut String, style: &FieldStyle) -> Response {
    let t = self.theme();
    let b = self.search_behavior(id, text);
    let clear = !text.is_empty() && !b.disabled;
    let mut style = *style;
    if clear {
      style.frame.pad[1] = style.frame.pad[1].max(INSET + MARK + 6.0);
    }

    let (muted, wrap) = (self.dim(t.muted, b.disabled), Style::new().w(style.frame.size[0]));
    let mut r = Response::default();
    // Its own id: the field may have taken its place's.
    self.next_id = Some((id.key("wrap"), false));
    self.row(wrap, |ui| {
      r = ui.field_box(&b, &style);
      // System Settings' magnifier: its ink 13 wide, 10 in from the edge.
      let at = Float::parent().attach(0.0, 0.5).anchor(0.0, 0.5).offset(INSET, 0.0).clip();
      let s = Style::new().w(px(17.0)).h(px(17.0));
      ui.float(at).canvas(s, move |f, r| icon::draw(f, Icon::SEARCH, r, muted, 2.0));
      if clear {
        let x = id.key("clear");
        let at = Float::parent().attach(1.0, 0.5).anchor(1.0, 0.5).offset(-INSET, 0.0).clip();
        let s = Style::new().w(px(MARK)).h(px(MARK)).cursor(Cursor::Pointer);
        let hot = ui.ctl_hot(x, ui.hovered(x));
        let mark = crate::scene::mix(t.faint, t.muted, hot);
        ui.with_id(x).float(at).canvas(s, move |f, r| clear_mark(f, r, mark, t.field));
      }
    });
    r
  }

  /// A [Ui::search_field] in the theme's look ([crate::ui::Theme::search]), `hint` shown while
  /// it is empty; its id from [Ui::id] or [Ui::key], else its place.
  pub fn search(&mut self, text: &mut String, hint: &'static str) -> Response {
    let id = self.take_id();
    let style = self.theme().search().hint(hint);
    self.search_field(id, text, &style)
  }
}

#[cfg(test)]
mod tests {
  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::{Rect, Size};
  use crate::ui::{Cursor, Id, Ui};
  use crate::{Modifiers, NamedKey};

  /// A search field and the changes it reported.
  #[derive(Default)]
  struct Find {
    text: String,
    changes: u32,
    off: bool,
  }

  impl App for Find {
    fn view(&mut self, ui: &mut Ui) {
      if ui.disabled(self.off).id("s").search(&mut self.text, "Search").changed {
        self.changes += 1;
      }
    }
  }

  fn find(text: &str) -> Harness<Find> {
    let find = Find { text: text.into(), ..Find::default() };
    let mut h = Harness::new(find, Size::new(300.0, 100.0), 2.0);
    h.frame();
    h
  }

  fn clear(h: &Harness<Find>) -> Option<Rect> {
    h.rect(Id::of("s").key("clear"))
  }

  #[test]
  fn the_clear_mark_shows_with_text_and_clears_it() {
    let mut h = find("");
    assert!(clear(&h).is_none(), "no ⊗ while empty");
    h.click_id(Id::of("s"));
    h.type_text("moon");
    h.frame();
    assert_eq!((h.app().text.as_str(), h.app().changes), ("moon", 1));
    let x = clear(&h).expect("a ⊗ with text");
    let field = h.rect(Id::of("s")).unwrap();
    assert!(x.x + x.w <= field.x + field.w - 8.0 && x.w == 14.0, "{x:?} in {field:?}");
    h.move_to(x.x + 7.0, x.y + 7.0);
    assert_eq!(h.rt.cursor(), Cursor::Pointer);

    // Clicked from outside the field: cleared, `changed`, and the field focused for the next.
    h.click(5.0, 95.0);
    assert_eq!(h.focused(), None);
    h.click_id(Id::of("s").key("clear"));
    h.frame();
    assert_eq!((h.app().text.as_str(), h.app().changes), ("", 2));
    assert_eq!(h.focused(), Some(Id::of("s")));
    assert!(clear(&h).is_none());
    assert_eq!(h.text(Id::of("s").key("text")), Some("Search"), "the hint again");
  }

  #[test]
  fn the_magnifier_and_the_margin_by_the_mark_are_the_fields() {
    let mut h = find("moon");
    let (field, x) = (h.rect(Id::of("s")).unwrap(), clear(&h).unwrap());
    let y = field.y + field.h / 2.0;
    // The magnifier, 8 in: the I-beam, and a click focuses the field.
    h.move_to(field.x + 16.0, y);
    assert_eq!(h.rt.cursor(), Cursor::Text);
    h.click(field.x + 16.0, y);
    assert_eq!(h.focused(), Some(Id::of("s")));
    // Just left of the ⊗: the field's, nothing cleared.
    h.click(x.x - 2.0, y);
    h.frame();
    assert_eq!((h.app().text.as_str(), h.app().changes), ("moon", 0));
  }

  #[test]
  fn escape_clears_then_goes_on() {
    let mut h = find("");
    h.click_id(Id::of("s"));
    h.type_text("moon");
    assert!(h.key(NamedKey::Escape, Modifiers::empty()), "the field's while it clears");
    h.frame();
    assert_eq!((h.app().text.as_str(), h.app().changes), ("", 2));
    assert_eq!(h.text(Id::of("s").key("text")), Some("Search"));
    assert_eq!(h.focused(), Some(Id::of("s")));
    assert!(!h.key(NamedKey::Escape, Modifiers::empty()), "the app's when empty");
    assert_eq!(h.app().changes, 2);
  }

  #[test]
  fn a_disabled_search_has_no_clear_mark_and_takes_no_input() {
    let mut h = find("moon");
    h.app_mut().off = true;
    h.frame();
    assert!(clear(&h).is_none());
    h.click_id(Id::of("s"));
    assert_eq!(h.focused(), None);
    assert_eq!((h.app().text.as_str(), h.app().changes), ("moon", 0));
  }
}
