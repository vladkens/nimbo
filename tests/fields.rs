// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/fields.rs` through the harness: typing, moves by word and line, selection by
//! Shift and the mouse (click, drag, double and triple click), the clipboard, undo and redo, Enter
//! and Tab; an input method's composition kept out of the value until committed, a commit and a new
//! composition back to back, a value the app sets meanwhile held, Enter completing it; the
//! password's dots, clipboard and input method; the placeholder, the disabled field, selectable
//! text, and the caret's blink timers only while a field has focus (the first one from the
//! start). Assistive technology reads the fields' text as shown, a composition in it, never the
//! password's, and sets it, over a value the app set while composing.

#[path = "../examples/fields.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod fields;

mod common;

use std::time::Duration;

use common::reads;
use fields::*;
use nimbo::ui::Action;
use nimbo::{CMD, Command, Event, Harness, Id, Key, Modifiers, NamedKey, Sel};

const NONE: Modifiers = Modifiers::empty();

fn text_of(id: &str) -> Id {
  Id::of(id).key("text")
}

fn sel(h: &Harness<Fields>, id: &str) -> Sel {
  h.editor(Id::of(id)).unwrap().sel()
}

fn clipboard(h: &mut Harness<Fields>) -> Option<String> {
  h.rt.clipboard().read()
}

/// The fields, laid out once.
fn fields() -> Harness<Fields> {
  let mut h = Harness::new(Fields::new(), SIZE, 2.0);
  h.frame();
  h
}

/// The first field has the focus from the start, its caret blinking on timers.
#[test]
fn the_first_field_has_the_focus_from_the_start() {
  let mut h = fields();
  assert!(!h.frame(), "a frame without changes");
  assert_eq!(h.focused(), Some(Id::of(NAME)));
  assert_eq!(h.rt.due_in(), Some(Duration::from_millis(500)));
}

#[test]
fn typing_word_moves_undo_and_the_clipboard() {
  let h = &mut fields();
  let name = Id::of(NAME);
  h.click_id(name);
  assert_eq!(h.focused(), Some(name));
  assert!(h.rt.text_input().is_some(), "a field takes input methods");
  assert!(h.type_text("Hello world"));
  assert_eq!(h.app().name, "Hello world");
  assert_eq!(h.app().changes, [NAME]);
  h.frame();
  assert_eq!(h.text(text_of(NAME)), Some("Hello world"));
  // The caret blinks on timers while the field has focus.
  assert!(h.rt.due_in().is_some_and(|d| d <= Duration::from_millis(500)));

  // Alt-Left a word, Cmd-Left the line's start, Shift-Alt-Right selects a word.
  h.key(NamedKey::ArrowLeft, Modifiers::ALT);
  assert_eq!(sel(h, NAME), Sel::caret(6));
  h.key(NamedKey::ArrowLeft, CMD);
  assert_eq!(sel(h, NAME), Sel::caret(0));
  h.key(NamedKey::ArrowRight, Modifiers::ALT | Modifiers::SHIFT);
  assert_eq!(sel(h, NAME), Sel::new(0, 5));
  h.type_text("Howdy");
  assert_eq!(h.app().name, "Howdy world");
  h.key(NamedKey::Backspace, NONE);
  h.key(NamedKey::ArrowRight, NONE);
  h.key(NamedKey::Delete, Modifiers::ALT);
  assert_eq!(h.app().name, "Howd ");

  // Undo steps back through the delete, the backspace and the typing; redo forward.
  h.key(Key::Char('z'), CMD);
  assert_eq!(h.app().name, "Howd world");
  h.key(Key::Char('z'), CMD);
  assert_eq!(h.app().name, "Howdy world");
  h.key(Key::Char('z'), CMD | Modifiers::SHIFT);
  assert_eq!(h.app().name, "Howd world");

  // Select all, copy, cut, paste twice.
  h.key(Key::Char('a'), CMD);
  h.key(Key::Char('c'), CMD);
  assert_eq!(clipboard(h).as_deref(), Some("Howd world"));
  h.key(Key::Char('x'), CMD);
  assert_eq!(h.app().name, "");
  h.key(Key::Char('v'), CMD);
  h.key(Key::Char('v'), CMD);
  assert_eq!(h.app().name, "Howd worldHowd world");

  // Enter submits and goes on; Tab moves to the next field.
  assert!(!h.key(NamedKey::Enter, NONE));
  assert_eq!(h.app().submitted, Some((NAME, "Howd worldHowd world".to_owned())));
  h.key(NamedKey::Tab, NONE);
  assert_eq!(h.focused(), Some(Id::of(EMAIL)));
}

#[test]
fn the_mouse_selects() {
  let h = &mut fields();
  let name = Id::of(NAME);
  h.click_id(name);
  h.key(Key::Char('a'), CMD);
  h.type_text("alpha beta gamma");
  h.frame();
  let t = h.rect(text_of(NAME)).unwrap();
  let y = t.y + t.h / 2.0;
  // A click at the text's start puts the caret there.
  h.click(t.x + 1.0, y);
  assert_eq!(sel(h, NAME), Sel::caret(0));
  // A double click selects the word under it, a triple click the line.
  let w = h.rect(text_of(NAME)).unwrap().w;
  let mid = t.x + w / 2.0;
  h.press_with(mid, y, 2, NONE);
  h.release(mid, y);
  assert_eq!(h.editor(name).unwrap().selected(), "beta");
  h.press_with(mid, y, 3, NONE);
  h.release(mid, y);
  assert_eq!(h.editor(name).unwrap().selected(), "alpha beta gamma");
  // A drag from the start to the end selects what it went over, frames between.
  h.press(t.x + 1.0, y);
  h.frame();
  h.move_to(t.x + w + 4.0, y);
  h.frame();
  h.release(t.x + w + 4.0, y);
  assert_eq!(sel(h, NAME), Sel::new(0, 16));
  // A drag that comes and goes between frames (press, move, release) selects all the same.
  h.click(t.x + 1.0, y);
  h.press(t.x + 1.0, y);
  h.move_to(mid, y);
  h.release(mid, y);
  let s = sel(h, NAME);
  assert!(s.anchor == 0 && (6..=11).contains(&s.head), "{s:?}");
  // Shift-click extends from the anchor.
  h.click(t.x + 1.0, y);
  h.press_with(mid, y, 1, Modifiers::SHIFT);
  h.release(mid, y);
  let s = sel(h, NAME);
  assert!(s.anchor == 0 && (6..=11).contains(&s.head), "{s:?}");
}

/// The caret scrolled into view: at the end of a long line, then at its start, the second asked
/// for by a key between the frames.
#[test]
fn the_caret_stays_in_view() {
  let h = &mut fields();
  let name = Id::of(NAME);
  h.click_id(name);
  h.key(Key::Char('a'), CMD);
  h.type_text(&"a long name that does not fit the field ".repeat(3));
  h.frame();
  let off = |h: &Harness<Fields>| h.rt.ui.last.get(name).unwrap().scroll[0];
  assert!(off(h) > 0.0, "the end in view");
  h.key(NamedKey::ArrowLeft, CMD);
  h.frame();
  assert_eq!(off(h), 0.0, "the start in view");
  // A long value from the app puts the caret at its end, in view.
  h.app_mut().name = "set by the app, a name far too long for the field to show".into();
  h.frame();
  h.frame();
  assert!(off(h) > 0.0, "the app's value's end in view");
  // As long as the last one, but wider: the caret's offset is the same, and still in view.
  h.app_mut().name = "i".repeat(40);
  h.frame();
  assert_eq!(off(h), 0.0);
  h.app_mut().name = "W".repeat(40);
  h.frame();
  assert!(off(h) > 0.0, "the wider value's end in view");
}

#[test]
fn input_methods_compose_apart_from_the_value() {
  let h = &mut fields();
  let notes = Id::of(NOTES);
  h.click_id(notes);
  h.type_text("ab");
  // Composing: marked text in the editor at the caret, underlined, not in the app's value.
  assert!(h.ime_mark("n"));
  assert!(h.ime_mark("ni"));
  assert_eq!(h.app().notes, "ab");
  assert_eq!(h.editor(notes).unwrap().text(), "abni");
  assert_eq!(h.rt.text_input().unwrap().marked(), Some(2..4));
  assert!(h.rt.text_input().unwrap().caret_rect().is_some(), "where the candidates go");
  let img = h.render();
  let t = h.rect(text_of(NOTES)).unwrap();
  let r = h.rt.text_input().unwrap().rect(2..4).unwrap().0;
  let under = (r.y + r.h - 1.5..r.y + r.h + 0.5).step_by_half().any(|y| {
    let p = img.pixel(r.x + r.w / 2.0, y);
    p[0] > 0xa0
  });
  assert!(under, "the composition is underlined at {r:?} in {t:?}");
  // The commit is the value; a commit and a new composition back to back are an end and a
  // start.
  assert!(h.ime_commit("你"));
  assert_eq!(h.app().notes, "ab你");
  h.ime_mark("h");
  assert!(h.ime_commit("好"));
  h.ime_mark("m");
  assert_eq!((h.app().notes.as_str(), h.editor(notes).unwrap().text()), ("ab你好", "ab你好m"));
  // A value the app sets while composing waits for the end; a commit makes it stale.
  h.app_mut().notes = "from the app".into();
  h.frame();
  assert_eq!(h.editor(notes).unwrap().text(), "ab你好m");
  h.ime_commit("吗");
  h.frame();
  assert_eq!(h.app().notes, "ab你好吗");
  // Cancelled (an empty mark), the composition leaves the app's value to apply.
  h.ime_mark("x");
  h.app_mut().notes = "from the app".into();
  h.frame();
  h.ime_mark("");
  h.frame();
  assert_eq!(h.editor(notes).unwrap().text(), "from the app");
  assert_eq!(h.app().notes, "from the app");
  // Enter during a composition only completes it: no line break.
  h.ime_mark("ok");
  assert!(h.event(Event::Command(Command::InsertNewline)));
  assert_eq!(h.app().notes, "from the appok");
  h.key(NamedKey::Enter, NONE);
  assert_eq!(h.app().notes, "from the appok\n", "then Enter breaks the line");
  // An empty preedit with nothing composed (GTK reports one after each commit) leaves a
  // selection alone.
  h.key(NamedKey::ArrowLeft, Modifiers::SHIFT);
  h.ime_mark("");
  assert_eq!(h.app().notes, "from the appok\n");
  h.key(NamedKey::ArrowRight, NONE);
  // Losing focus keeps a composition as typed.
  h.ime_mark("z");
  h.click_id(Id::of(EMAIL));
  assert_eq!(h.app().notes, "from the appok\nz");
}

#[test]
fn a_password_shows_dots_and_keeps_its_secret() {
  let h = &mut fields();
  let pw = Id::of(PASSWORD);
  h.click_id(pw);
  assert!(h.rt.text_input().is_none(), "no input method in a password");
  h.type_text("sécret");
  h.frame();
  assert_eq!(h.app().password, "sécret");
  assert_eq!(h.text(text_of(PASSWORD)), Some("••••••"));
  // Nothing copied or cut; word moves go to the ends.
  let before = clipboard(h);
  h.key(Key::Char('a'), CMD);
  h.key(Key::Char('c'), CMD);
  h.key(Key::Char('x'), CMD);
  assert_eq!((clipboard(h), h.app().password.as_str()), (before, "sécret"));
  h.key(NamedKey::ArrowLeft, Modifiers::ALT);
  assert_eq!(sel(h, PASSWORD), Sel::caret(0));
  // A click lands between dots, in the text's offsets: after the second character.
  h.frame();
  let t = h.rect(text_of(PASSWORD)).unwrap();
  let dot = t.w / 6.0;
  h.click(t.x + dot * 2.0 + 0.5, t.y + t.h / 2.0);
  assert_eq!(sel(h, PASSWORD), Sel::caret(3));
}

#[test]
fn placeholder_disabled_and_selectable() {
  let h = &mut fields();
  h.frame();
  assert_eq!(h.text(text_of(EMAIL)), Some("name@example.com"));
  // Disabled: no focus by a click or Tab, its text unchanged.
  let off = Id::of(OFF);
  h.click_id(off);
  assert_ne!(h.focused(), Some(off));
  h.click_id(Id::of(NOTES));
  h.key(NamedKey::Tab, NONE);
  assert_ne!(h.focused(), Some(off));
  // Selectable text: a word by a double click, copied; typing changes nothing.
  let about = Id::of(ABOUT);
  let r = h.rect(about).unwrap();
  h.press_with(r.x + 20.0, r.y + 8.0, 2, NONE);
  h.release(r.x + 20.0, r.y + 8.0);
  assert_eq!(h.focused(), Some(about));
  assert_eq!(h.editor(about).unwrap().selected(), "Selectable");
  assert!(h.rt.text_input().is_none(), "selectable text takes no input method");
  h.key(Key::Char('c'), CMD);
  assert_eq!(clipboard(h).as_deref(), Some("Selectable"));
  h.type_text("x");
  assert_eq!(h.editor(about).unwrap().text(), TEXT);
  // Without a focused field the window sleeps: no caret, no timer.
  h.click(SIZE.w - 4.0, SIZE.h - 4.0);
  h.wait(Duration::from_millis(600));
  assert_eq!((h.focused(), h.rt.due_in()), (None, None));
  assert!(!h.frame());
}

/// Steps of half a point over a range, for pixel probes.
trait Halves {
  fn step_by_half(self) -> impl Iterator<Item = f32>;
}

impl Halves for std::ops::Range<f32> {
  fn step_by_half(self) -> impl Iterator<Item = f32> {
    let n = ((self.end - self.start) * 2.0).ceil() as usize;
    (0..n).map(move |k| self.start + k as f32 * 0.5)
  }
}

/// Fields read their label and text, a password only that it is one; selectable text reads as
/// text.
#[test]
fn fields_read_their_text_but_never_the_password() {
  let h = &mut fields();
  h.type_text("Ann");
  h.click_id(Id::of(PASSWORD));
  h.type_text("sécret");
  h.frame();
  let dump = h.a11y();
  let lines = [
    "text field \"Name\" value \"Ann\"",
    "text field \"E-mail\" value \"\"",
    "secure text field \"Password\" focused",
    "text area \"Notes\" value \"\"",
    "text field \"Disabled\" value \"Read only\" disabled",
  ];
  reads(&dump, &lines);
  reads(&dump, &[&format!("static text {TEXT:?}")]);
  assert!(!dump.contains("sécret") && !dump.contains('•'), "{dump}");
}

/// Assistive technology sets a field's text, and a password's without ever reading it; a
/// disabled field refuses it.
#[test]
fn assistive_technology_sets_a_fields_text() {
  let h = &mut fields();
  assert!(h.act(Id::of(EMAIL), Action::SetValue("ann@example.com".into())));
  assert_eq!(h.app().email, "ann@example.com");
  reads(&h.a11y(), &["text field \"E-mail\" value \"ann@example.com\""]);
  assert!(h.act(Id::of(PASSWORD), Action::SetValue("sécret".into())));
  assert_eq!(h.app().password, "sécret");
  let dump = h.a11y();
  assert!(!dump.contains("sécret") && !dump.contains('•'), "{dump}");
  assert!(!h.act(Id::of(OFF), Action::SetValue("x".into())));
  assert_eq!(h.app().off, "Read only");
}

/// A text set by assistive technology while the app's value waits on a composition replaces
/// that value, even when it is the text the field had: field "old", composing, the app sets
/// "held", then "old" is set.
#[test]
fn an_assistive_value_supersedes_one_held() {
  let h = &mut fields();
  let name = Id::of(NAME);
  h.type_text("old");
  h.ime_mark("ni");
  h.app_mut().name = "held".into();
  h.frame();
  assert!(h.act(name, Action::SetValue("old".into())));
  h.frame();
  assert_eq!((h.app().name.as_str(), h.editor(name).unwrap().text()), ("old", "old"));
  reads(&h.a11y(), &["text field \"Name\" value \"old\" focused"]);
}

/// A field reads the text it shows, a composition in it, so its value, its length and the
/// selection's offsets agree; a password composing still reads no text.
#[test]
fn a_composition_reads_as_the_field_shows_it() {
  let h = &mut fields();
  h.type_text("old");
  h.ime_mark("ni");
  assert_eq!(h.app().name, "old");
  assert_eq!(h.rt.text_input().unwrap().selected(), 5..5);
  reads(&h.a11y(), &["text field \"Name\" value \"oldni\" focused"]);
  h.click_id(Id::of(PASSWORD));
  h.ime_mark("xy");
  let dump = h.a11y();
  reads(&dump, &["secure text field \"Password\" focused"]);
  assert!(!dump.contains("xy"), "{dump}");
}
