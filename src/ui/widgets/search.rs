// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The search field: a single-line text field with a magnifier, and a ⊗ that clears it while
//! it has text, as macOS's search fields; its look from the theme ([crate::ui::Theme::search]).
//!
//! ```ignore
//! if ui.id("search").search(&mut self.query, "Search models").changed { self.filter() }
//! ```
//!
//! A click on the ⊗ (`id.key("clear")`) clears the text and focuses the field, ready for the
//! next search; Escape clears it too, before the app's bindings, and goes on to them only when
//! the field is empty already, so an Escape that cleared it doesn't also close a dialog. While
//! an input method composes, Escape is the input method's.

use keyboard_types::NamedKey;

use super::icon::{self, Icon};
use super::shortcut::clear_mark;
use crate::keys::Key;
use crate::ui::behavior::FieldState;
use crate::ui::{Cursor, FieldBehavior, FieldStyle, Float, Id, Response, Style, Ui, grow, px};

/// The ⊗'s side, and how far in it is from the field's right edge.
const MARK: f32 = 14.0;
const INSET: f32 = 8.0;

impl Ui<'_> {
  /// A search field's input: [Ui::text_field_behavior]'s on one line, and a click on
  /// `id.key("clear")` (the ⊗ a look draws while there is text) or Escape clears the text,
  /// `changed`, an input method's composition with it. The ⊗ focuses the field; Escape is the
  /// field's before any binding while there is text and nothing composing, else it goes on.
  pub fn search_behavior(&mut self, id: Id, text: &mut String) -> FieldBehavior {
    let off = self.next_disabled || self.scope_off;
    let mut cleared = !off && self.clicked(id.key("clear")) && !text.is_empty();
    if cleared {
      self.focus(Some(id));
    }

    if let Some(k) = self.key_down(id) {
      let composing = self.state::<FieldState>(id).ed.marked().is_some();
      match k.key == Key::Named(NamedKey::Escape) && k.mods.is_empty() {
        true if !text.is_empty() && !composing => cleared = true,
        _ => self.pass(id),
      }
    }

    if cleared {
      text.clear();
      self.state::<FieldState>(id).ed.set_text("");
    }

    let mut b = self.text_field_behavior(id, text, false, false);
    if b.focused && !text.is_empty() {
      self.grab_keys(id);
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
    // The frame's size goes to the box the marks sit in, which the field fills, its text
    // clipped short of them.
    let f = style.frame;
    let wrap = Style { size: f.size, min: f.min, max: f.max, ..Style::new() };
    let (min, max) = (Style::new().min, Style::new().max);
    let frame = Style { size: [grow(); 2], min, max, ..f }.clip_content();
    let mut style = FieldStyle { frame, ..*style };
    if clear {
      style.frame.pad[1] = style.frame.pad[1].max(INSET + MARK + 6.0);
    }

    let muted = self.dim(t.muted, b.disabled);
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
  use crate::app::{App, Cx, Event};
  use crate::harness::Harness;
  use crate::keys::{Binding, Command, bind};
  use crate::scene::{Rect, Size};
  use crate::ui::{Cursor, Id, Style, Ui, pct};
  use crate::{Modifiers, NamedKey};

  const NONE: Modifiers = Modifiers::empty();

  /// A search field, the changes it reported, and the app's own Escape: what the field gives
  /// back. `narrow`: half the window wide, at most 100, and 60% of it high.
  #[derive(Default)]
  struct Find {
    text: String,
    changes: u32,
    cancels: u32,
    off: bool,
    narrow: bool,
  }

  impl App for Find {
    fn keymap(&self) -> &[Binding] {
      const KEYS: &[Binding] = &[bind("escape", Command::Cancel)];
      KEYS
    }

    fn event(&mut self, _: &mut Cx, ev: &Event) -> bool {
      let cancel = matches!(ev, Event::Command(Command::Cancel));
      self.cancels += cancel as u32;
      cancel
    }

    fn view(&mut self, ui: &mut Ui) {
      let t = ui.theme();
      let frame = t.search().frame;
      let style = match self.narrow {
        true => t.search().frame(frame.w(pct(50.0)).max_w(100.0).h(pct(60.0))),
        false => t.search().hint("Search"),
      };

      let r = ui.disabled(self.off).search_field(Id::of("s"), &mut self.text, &style);
      if r.changed {
        self.changes += 1;
      }

      ui.el(Style::new().w(crate::ui::grow()).h(crate::ui::px(20.0)));
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
  fn escape_clears_before_the_apps_binding_then_goes_on() {
    let mut h = find("");
    h.click_id(Id::of("s"));
    h.key('m', NONE);
    h.type_text("oon");
    assert_eq!(h.app().text, "moon");
    assert!(h.key(NamedKey::Escape, NONE));
    h.frame();
    assert_eq!((h.app().text.as_str(), h.app().changes, h.app().cancels), ("", 3, 0));
    assert_eq!(h.text(Id::of("s").key("text")), Some("Search"));
    assert_eq!(h.focused(), Some(Id::of("s")));
    // Empty, it is the app's.
    assert!(h.key(NamedKey::Escape, NONE));
    assert_eq!((h.app().changes, h.app().cancels), (3, 1));
  }

  #[test]
  fn clearing_drops_a_composition() {
    let mut h = find("");
    h.click_id(Id::of("s"));
    h.type_text("moon");
    h.ime_mark("ni");
    h.frame();
    // Escape is the input method's while it composes.
    h.key(NamedKey::Escape, NONE);
    assert_eq!(h.app().text, "moon");
    h.click_id(Id::of("s").key("clear"));
    h.frame();
    assert_eq!(h.app().text, "");
    assert_eq!(h.editor(Id::of("s")).map(|e| (e.text(), e.marked())), Some(("", None)));
    h.ime_commit("日");
    h.frame();
    assert_eq!(h.app().text, "日");
  }

  #[test]
  fn a_long_text_stays_clear_of_the_marks() {
    let mut h = find(&"moon ".repeat(20));
    h.click_id(Id::of("s"));
    let field = h.rect(Id::of("s")).unwrap();
    // The field's ground, above the text.
    let ground = h.render().pixel(field.x + field.w / 2.0, field.y + 3.0);
    let y = field.y + field.h / 2.0;
    // Scrolled to the end, then back to the start: nothing of it over the magnifier's side or
    // the ⊗'s.
    for key in [NamedKey::End, NamedKey::Home] {
      h.key(key, NONE);
      h.frame();
      let img = h.render();
      for x in [3.0, 6.0, field.w - 3.0, field.w - 5.0] {
        assert_eq!(img.pixel(field.x + x, y - 2.0), ground, "{key:?} at {x}");
      }
    }
  }

  #[test]
  fn the_marks_stay_in_a_field_of_its_own_width() {
    let mut h = find("moon");
    h.app_mut().narrow = true;
    h.frame();
    let (field, x) = (h.rect(Id::of("s")).unwrap(), clear(&h).unwrap());
    assert_eq!((field.w, field.h), (100.0, 60.0));
    assert!(x.x > field.x + field.w / 2.0 && x.x + x.w <= field.x + field.w - 8.0, "{x:?}");
  }

  #[test]
  fn a_disabled_search_has_no_clear_mark_and_takes_no_input() {
    let mut h = find("moon");
    h.app_mut().off = true;
    h.frame();
    assert!(clear(&h).is_none());
    h.click_id(Id::of("s"));
    assert_eq!(h.focused(), None);
    h.key(NamedKey::Escape, NONE);
    assert_eq!((h.app().text.as_str(), h.app().changes), ("moon", 0));
  }
}
