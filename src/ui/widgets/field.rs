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
  Align, Axes, Axis, FieldStyle, Response, Style, TextFieldConfiguration, TextFieldStyle,
  TextStyle, Ui, grow,
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
  /// [Ui::id] or [Ui::key], else its place. [Ui::disabled] makes it plain text.
  pub fn selectable_text(&mut self, text: &str, style: TextStyle) -> Response {
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
  use crate::app::{App, Event, Phase};
  use crate::keys::{CMD, Command};
  use crate::scene::{Point, Size};
  use crate::ui::{Axes, Cursor, Id, Style, Ui, px};
  use crate::{Harness, Modifiers, NamedKey};

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

  /// Two selectable texts and a field over room to click in; a button that takes the focus as
  /// it shows, `ok`.
  #[derive(Default)]
  struct Texts {
    text: String,
    ok: bool,
  }

  impl App for Texts {
    fn view(&mut self, ui: &mut Ui) {
      let body = ui.theme().body();
      ui.col(Style::new().w(px(300.0)).h(px(200.0)).gap(10.0), |ui| {
        ui.id("a").selectable_text("Hello world", body);
        ui.id("b").selectable_text("Other words", body);
        ui.id("f").text_field(&mut self.text, "");
        if self.ok {
          ui.id("ok").autofocus().button("OK");
        }
      });
    }
  }

  /// Selects all of `id` with a click and Cmd-A.
  fn select_all(h: &mut Harness<Texts>, id: Id) {
    h.click_id(id);
    h.key('a', CMD);
    h.frame();
    let len = h.text(id).unwrap().len();
    assert_eq!(h.selection(id), Some((0, len)));
  }

  /// A click elsewhere in the window (on room, on other text) clears a selectable text's or a
  /// field's selection, the field losing its focus; the window losing its focus keeps them,
  /// shown.
  #[test]
  fn a_click_elsewhere_clears_the_selection_the_window_keeps_it() {
    let mut h =
      Harness::new(Texts { text: "typed".into(), ok: false }, Size::new(300.0, 200.0), 1.0);
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

  /// A click elsewhere ends the selection where the page is, not at its head.
  #[test]
  fn a_selection_ended_elsewhere_leaves_the_page_where_it_is() {
    let mut h = Harness::new(Page, Size::new(300.0, 200.0), 1.0);
    let page = Id::of("page");
    let offset = |h: &Harness<Page>| h.scroll(page).unwrap().offset.y;
    h.frame();
    h.click(20.0, 10.0);
    h.key('a', CMD);
    h.frame();
    h.frame();
    assert!(offset(&h) > 0.0, "Cmd-A shows the head");

    let (pos, delta, mods) = (Point::new(20.0, 20.0), Point::new(0.0, 5000.0), Modifiers::empty());
    let (phase, momentum) = (Phase::None, Phase::None);
    h.event(Event::Scroll { pos, delta, precise: true, phase, momentum, mods });
    h.frame();
    assert_eq!(offset(&h), 0.0);
    h.click(150.0, 150.0);
    h.frame();
    h.frame();
    assert_eq!(offset(&h), 0.0);
  }

  /// Two fields and, unless `hidden` (its state `kept`), a menu whose item cuts the selection of
  /// the one `to`, and a menu in it; a field to `find` in it that takes the focus as it shows.
  struct Cuts {
    text: [String; 2],
    to: &'static str,
    hidden: bool,
    kept: bool,
    find: Option<String>,
  }

  impl Default for Cuts {
    fn default() -> Self {
      let text = ["hello world".into(), "other words".into()];
      Self { text, to: "f", hidden: false, kept: false, find: None }
    }
  }

  impl App for Cuts {
    fn view(&mut self, ui: &mut Ui) {
      ui.col(Style::new().w(px(300.0)).h(px(200.0)).gap(10.0), |ui| {
        let [a, b] = &mut self.text;
        ui.id("f").text_field(a, "");
        ui.id("g").text_field(b, "");
        if self.hidden {
          if self.kept {
            ui.retain(Id::of("actions"));
          }

          return;
        }

        let (to, find) = (Id::of(self.to), &mut self.find);
        ui.id("actions").menu("Actions", |ui| {
          if let Some(find) = find {
            ui.id("find").autofocus().text_field(find, "");
          }

          if ui.menu_item("Cut").clicked {
            ui.field_command(to, Command::Cut);
          }

          ui.id("more").menu("More", |ui| _ = ui.menu_item("Nothing"));
        });
      });
    }
  }

  /// A menu holding the focus keeps the field's selection for its item to cut, and gives the
  /// focus back.
  #[test]
  fn a_menus_item_cuts_the_selection_it_found() {
    let mut h = Harness::new(Cuts::default(), Size::new(300.0, 200.0), 1.0);
    let f = Id::of("f");
    h.frame();
    h.click_id(f);
    h.key('a', CMD);
    h.click_id(Id::of("actions"));
    h.frame();
    h.frame();
    assert!(h.click_menu_item("Cut"));
    h.frame();
    h.frame();
    assert_eq!((h.app().text[0].as_str(), h.focus_id()), ("", Some(f)));
  }

  /// A button taking the focus as it shows ends the selection before it, in the frames after.
  #[test]
  fn an_element_taking_the_focus_as_it_shows_ends_the_selection() {
    let mut h = Harness::new(Texts::default(), Size::new(300.0, 200.0), 1.0);
    let a = Id::of("a");
    h.frame();
    select_all(&mut h, a);
    h.app_mut().ok = true;
    h.redraw();
    let frames = (0..5).take_while(|_| h.frame()).count();
    assert_eq!((h.focus_id(), h.selection(a)), (Some(Id::of("ok")), Some((11, 11))));
    assert!(frames < 5, "the frames after show it, then stop");
  }

  /// A field in a menu taking the focus as it shows keeps the selection the menu's item cuts.
  #[test]
  fn a_field_in_a_menu_keeps_the_selection_for_its_item() {
    let cuts = Cuts { find: Some(String::new()), ..Cuts::default() };
    let mut h = Harness::new(cuts, Size::new(300.0, 200.0), 1.0);
    let f = Id::of("f");
    h.frame();
    h.click_id(f);
    h.key('a', CMD);
    h.click_id(Id::of("actions"));
    h.frame();
    h.frame();
    assert_eq!(h.focus_id(), Some(Id::of("find")));

    // A menu opened from that field and closed lends no more than it took.
    h.click_id(Id::of("more"));
    h.frame();
    h.frame();
    h.key(NamedKey::Escape, Modifiers::empty());
    h.frame();
    h.frame();
    assert_eq!(h.focus_id(), Some(Id::of("find")));

    assert!(h.click_menu_item("Cut"));
    h.frame();
    h.frame();
    assert_eq!(h.app().text[0], "");
  }

  /// A field that copies its selection as it loses the focus (unless `hidden`, its state kept;
  /// a label in its place when `swapped`), room to click in, and a button after it that takes
  /// the focus as it shows, `ok`.
  #[derive(Default)]
  struct Leaves {
    text: String,
    ok: bool,
    hidden: bool,
    swapped: bool,
  }

  impl App for Leaves {
    fn view(&mut self, ui: &mut Ui) {
      ui.col(Style::new().w(px(300.0)).h(px(200.0)), |ui| {
        if self.swapped {
          ui.id("f").label("Label");
          return;
        }

        if self.hidden {
          ui.retain(Id::of("f"));
          ui.retain(Id::of("f").key("edit"));
          return;
        }

        if ui.id("f").text_field(&mut self.text, "").blurred {
          ui.field_command(Id::of("f"), Command::Copy);
        }

        if self.ok {
          ui.id("ok").autofocus().button("OK");
        }
      });
    }
  }

  /// A command queued for a field as it loses the focus runs on its selection: the press held
  /// across frames, an element after it taking the focus.
  #[test]
  fn a_command_queued_on_blur_runs_on_the_selection() {
    let leaves = Leaves { text: "hello".into(), ..Leaves::default() };
    let mut h = Harness::new(leaves, Size::new(300.0, 200.0), 1.0);
    let f = Id::of("f");
    h.frame();
    h.click_id(f);
    h.key('a', CMD);
    h.frame();

    // A press held across frames.
    h.press(250.0, 190.0);
    h.frame();
    h.frame();
    h.release(250.0, 190.0);
    h.frame();
    assert_eq!(h.clipboard().read().as_deref(), Some("hello"));

    // A button after it taking the focus as it shows.
    h.clipboard().write("");
    h.click_id(f);
    h.key('a', CMD);
    h.app_mut().ok = true;
    h.redraw();
    while h.frame() {}
    assert_eq!(h.clipboard().read().as_deref(), Some("hello"));
    assert_eq!((h.focus_id(), h.selection(f)), (Some(Id::of("ok")), Some((5, 5))));
  }

  /// A focused field hidden, its state kept, or a label built in its place and a click
  /// elsewhere, ends its selection and the frames stop.
  #[test]
  fn a_field_hidden_with_the_focus_ends_its_selection() {
    for swapped in [false, true] {
      let leaves = Leaves { text: "hello".into(), ..Leaves::default() };
      let mut h = Harness::new(leaves, Size::new(300.0, 200.0), 1.0);
      let f = Id::of("f");
      h.frame();
      h.click_id(f);
      h.key('a', CMD);
      h.frame();

      // The label keeps the focus, which a click elsewhere takes.
      (h.app_mut().hidden, h.app_mut().swapped) = (!swapped, swapped);
      h.redraw();
      if swapped {
        h.frame();
        h.click(250.0, 190.0);
      }

      let frames = (0..5).take_while(|_| h.frame()).count();
      assert!(frames < 5, "the frames stop");

      (h.app_mut().hidden, h.app_mut().swapped) = (false, false);
      h.redraw();
      h.frame();
      assert_eq!(h.selection(f), Some((5, 5)), "swapped: {swapped}");
    }
  }

  /// A field hidden with its edit menu open, their states kept, ends its selection.
  #[test]
  fn a_field_hidden_with_its_edit_menu_ends_its_selection() {
    let leaves = Leaves { text: "hello".into(), ..Leaves::default() };
    let mut h = Harness::new(leaves, Size::new(300.0, 200.0), 1.0);
    let f = Id::of("f");
    h.frame();
    h.click_id(f);
    h.key('a', CMD);
    let r = h.rect(f).unwrap();
    h.right_click(r.x + 10.0, r.y + 5.0);
    h.frame();
    h.frame();
    assert!(h.menu_open(f.key("edit")));

    h.app_mut().hidden = true;
    h.redraw();
    let frames = (0..5).take_while(|_| h.frame()).count();
    assert!(frames < 5, "the frames stop");

    h.app_mut().hidden = false;
    h.redraw();
    h.frame();
    assert_eq!(h.selection(f), Some((5, 5)));
  }

  /// A click around an open menu closes it and keeps the field's selection.
  #[test]
  fn a_click_around_a_menu_keeps_the_selection() {
    let mut h = Harness::new(Cuts::default(), Size::new(300.0, 200.0), 1.0);
    let f = Id::of("f");
    h.frame();
    h.click_id(f);
    h.key('a', CMD);
    h.click_id(Id::of("actions"));
    h.frame();
    h.frame();

    h.click(250.0, 190.0);
    h.frame();
    h.frame();
    assert_eq!((h.selection(f), h.focus_id()), (Some((0, 11)), Some(f)));
    assert!(!h.menu_open(Id::of("actions")));
  }

  /// The window losing its focus with a menu open in a menu gives the focus back, the
  /// selection kept.
  #[test]
  fn a_menu_in_a_menu_gives_the_focus_back() {
    let mut h = Harness::new(Cuts::default(), Size::new(300.0, 200.0), 1.0);
    let f = Id::of("f");
    h.frame();
    h.click_id(f);
    h.key('a', CMD);
    h.click_id(Id::of("actions"));
    h.frame();
    h.frame();
    h.click_id(Id::of("more"));
    h.frame();
    h.frame();
    assert_eq!(h.focus_id(), Some(Id::of("more").key("menu")));

    h.event(Event::Focus(false));
    h.frame();
    h.frame();
    assert_eq!((h.selection(f), h.focus_id()), (Some((0, 11)), Some(f)));
  }

  /// A menu gone while open (its state kept) holds the focus no more: a click elsewhere ends
  /// the selection, and the next menu keeps another field's.
  #[test]
  fn a_menu_gone_while_open_lends_nothing() {
    let mut h = Harness::new(Cuts::default(), Size::new(300.0, 200.0), 1.0);
    let (f, g) = (Id::of("f"), Id::of("g"));
    h.frame();
    h.click_id(f);
    h.key('a', CMD);
    h.click_id(Id::of("actions"));
    h.frame();

    // The menu gone, the click elsewhere is the field's focus moving away.
    (h.app_mut().hidden, h.app_mut().kept) = (true, true);
    h.frame();
    h.frame();
    h.click(250.0, 190.0);
    h.frame();
    assert_eq!(h.selection(f), Some((11, 11)));

    // Back once its state is gone, it lends the other field's focus.
    h.app_mut().kept = false;
    h.frame();
    h.frame();
    (h.app_mut().hidden, h.app_mut().to) = (false, "g");
    h.frame();

    h.click_id(g);
    h.key('a', CMD);
    h.click_id(Id::of("actions"));
    h.frame();
    h.frame();
    assert!(h.click_menu_item("Cut"));
    h.frame();
    h.frame();
    assert_eq!(h.app().text, ["hello world", ""]);
  }
}
