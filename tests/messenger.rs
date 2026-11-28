// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/messenger.rs` through the harness, the contacts answering at once on
//! their thread: Return sends and the bot answers ("typing…" meanwhile), Shift-Return breaks the
//! line and the composer grows; a chat not open counts its answers unread, a muted one doesn't;
//! Herald posts by itself on the window's clock, unread until its chat is read; people and bots
//! marked apart; the archive's thousands of messages open at once, build lazily, scroll with the
//! wheel and the trackpad and stay at the bottom until scrolled up; the search opens a chat at the
//! message found; a message's menu copies and deletes; emoji from the system's picker, IME, files
//! from the open panel and dropped on the chat (lit while dragged over) go into the composer; the
//! contact's card exports the chat through the save panel and clears it after asking; the
//! window's keys; the chat list's selection in the accent wherever the focus is; the sidebar's
//! toggle in macOS's title bar, its slide, its drag between its bounds and the bar that moves the
//! window; a closed window's chats for the next. The tray's and the menu bar's menus, the settings
//! window, Russian and what assistive technology reads.

#[path = "../examples/messenger.rs"]
#[expect(dead_code, reason = "its main and what this file doesn't use")]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[cfg_attr(windows, expect(unused_attributes, reason = "its windows_subsystem"))]
mod messenger;

#[expect(dead_code, reason = "this file reads by its lines' starts")]
mod common;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use common::{diff, mid, px};
use messenger::*;
use nimbo::a11y::Role;
use nimbo::harness::mac_titlebar;
use nimbo::{
  Appearance, CMD, CTRL, Event, FileDialogKind, Harness, Id, Key, Locale, Modifiers, NamedKey,
  Phase, Point, Rect, Size, Theme, character_palette,
};

type H = Harness<Messenger>;

/// Asserts that what assistive technology reads has a line starting with each of `lines`.
fn reads(dump: &str, lines: &[&str]) {
  for l in lines {
    assert!(dump.lines().any(|d| d.trim_start().starts_with(l)), "no {l:?} in\n{dump}");
  }
}

/// The messenger with contacts that answer at once, under no title bar, in the light
/// appearance, Alice's chat open.
fn messenger() -> H {
  answering_in(Duration::ZERO)
}

fn answering_in(delay: Duration) -> H {
  let mut app = Messenger::default();
  app.delay = delay;
  let mut h = Harness::new(app, Size::new(900.0, 600.0), 1.0);
  h.event(Event::Appearance(Appearance::LIGHT));
  h.frame();
  h
}

fn rect(h: &H, id: Id) -> Rect {
  h.rect(id).unwrap_or_else(|| panic!("no {id:?} in\n{}", h.a11y()))
}

fn key(h: &mut H, k: NamedKey, mods: Modifiers) {
  h.key(Key::Named(k), mods);
  h.frame();
}

/// Frames until every bot has answered.
fn settle(h: &mut H) {
  for _ in 0..500 {
    h.redraw();
    h.frame();
    if h.app().pending() == 0 {
      return;
    }

    std::thread::sleep(Duration::from_millis(2));
  }

  panic!("the bots never answered");
}

/// Types `text` into the composer and sends it with Return.
fn say(h: &mut H, text: &str) {
  let open = h.app().open;
  h.click_id(field(open));
  h.type_text(text);
  key(h, NamedKey::Enter, Modifiers::empty());
}

fn last(h: &H, chat: usize) -> &Msg {
  h.app().chats[chat].msgs.last().unwrap()
}

/// The open chat's scroll area.
fn area(h: &H) -> Id {
  Id::of(MESSAGES).key(h.app().open)
}

// MARK: Sending

#[test]
fn return_sends_and_the_bot_answers_typing_meanwhile() {
  let mut h = answering_in(Duration::from_millis(200));
  h.app_mut().open_chat(ABACUS_AT);
  h.frame();
  say(&mut h, "12 * 7 + 1");
  let c = &h.app().chats[ABACUS_AT];
  assert_eq!((c.draft.as_str(), c.msgs.last().unwrap().text.as_str()), ("", "12 * 7 + 1"));
  assert!(c.msgs.last().unwrap().mine);
  reads(&h.a11y(), &["static text \"typing…\""]);

  settle(&mut h);
  assert_eq!((last(&h, ABACUS_AT).text.as_str(), last(&h, ABACUS_AT).mine), ("= 85", false));
  reads(&h.a11y(), &["static text \"bot\""]);
}

/// A person answers in a few words, "typing…" meanwhile, and shows online.
#[test]
fn a_person_answers_too() {
  let mut h = answering_in(Duration::from_millis(200));
  reads(&h.a11y(), &["static text \"online\""]);
  say(&mut h, "Sending it now");
  reads(&h.a11y(), &["static text \"typing…\""]);
  settle(&mut h);
  let m = last(&h, ALICE_AT);
  assert!(!m.mine && answer(Contact::Person, "Sending it now", 7) == m.text, "{}", m.text);
}

#[test]
fn shift_return_breaks_the_line_and_the_composer_grows() {
  let mut h = messenger();
  h.click_id(field(0));
  let one = rect(&h, field(0)).h;
  for line in ["one", "two", "three"] {
    h.type_text(line);
    key(&mut h, NamedKey::Enter, Modifiers::SHIFT);
  }

  h.type_text("four");
  h.frame();
  assert_eq!(h.app().chats[0].draft, "one\ntwo\nthree\nfour");
  let four = rect(&h, field(0)).h;
  assert!(four > one * 2.5, "{one} -> {four}");

  // Up to six lines, then it scrolls.
  for _ in 0..6 {
    key(&mut h, NamedKey::Enter, Modifiers::SHIFT);
    h.type_text("more");
  }

  h.frame();
  let most = rect(&h, field(0)).h;
  assert!(most < one * 6.0 && most > four, "{most}");
  assert_eq!(h.app().chats[0].msgs.len(), 6, "nothing sent");
}

#[test]
fn send_is_off_until_there_is_something_to_send() {
  let mut h = messenger();
  let n = h.app().chats[0].msgs.len();
  h.click_id(Id::of(SEND));
  h.click_id(field(0));
  h.type_text("   ");
  key(&mut h, NamedKey::Enter, Modifiers::empty());
  assert_eq!(h.app().chats[0].msgs.len(), n);
  reads(&h.a11y(), &["button \"Send\" disabled"]);

  h.type_text("hi");
  h.frame();
  h.click_id(Id::of(SEND));
  assert_eq!(last(&h, 0).text, "hi");
}

#[test]
fn an_answer_to_a_chat_not_open_counts_unread_unless_muted() {
  let mut h = messenger();
  // Sent, and another chat opened before the answer comes.
  let ask = |h: &mut H, text: &str| {
    h.app_mut().open_chat(ECHO_AT);
    h.app_mut().chats[ECHO_AT].draft = text.into();
    h.app_mut().send();
    h.app_mut().open_chat(ABACUS_AT);
    settle(h);
  };
  ask(&mut h, "olleh");
  assert_eq!((last(&h, ECHO_AT).text.as_str(), h.app().chats[ECHO_AT].unread), ("hello", 1));
  reads(&h.a11y(), &["row \"Echo, bot, hello, 1 unread\""]);

  // Opened, it is read.
  h.app_mut().open_chat(ECHO_AT);
  assert_eq!(h.app().chats[ECHO_AT].unread, 0);

  h.app_mut().chats[ECHO_AT].muted = true;
  ask(&mut h, "abc");
  assert_eq!(h.app().chats[ECHO_AT].unread, 0, "muted");
}

#[test]
fn an_answer_in_the_background_is_unread_until_the_window_comes_back() {
  let mut h = messenger();
  h.event(Event::Focus(false));
  assert!(!h.app().focused, "what the app's loop reads, the window drawn or not");
  h.app_mut().chats[0].draft = "abc".into();
  h.app_mut().send();
  settle(&mut h);
  assert_eq!(h.app().chats[0].unread, 1);

  h.event(Event::Focus(true));
  h.frame();
  let left = h.app().chats[HERALD_AT].unread + h.app().chats[ORACLE_AT].unread;
  assert_eq!((h.app().chats[0].unread, h.app().unread()), (0, left), "Herald's and Oracle's left");
}

// MARK: Herald

/// Herald posts its news by itself every [NEWS_EVERY] on the window's clock: unread, counted in
/// its row and in the app's count, until its chat is open in the focused window; muted, it
/// counts nothing.
#[test]
fn herald_posts_by_itself_and_counts_unread() {
  let mut h = messenger();
  let (n, unread) = (h.app().chats[HERALD_AT].msgs.len(), h.app().unread());
  h.advance(NEWS_EVERY - Duration::from_millis(10));
  h.frame();
  assert_eq!(h.app().chats[HERALD_AT].msgs.len(), n, "not yet");

  h.advance(Duration::from_millis(10));
  h.frame();
  let c = &h.app().chats[HERALD_AT];
  assert_eq!((c.msgs.len(), c.unread, h.app().unread()), (n + 1, 2, unread + 1));
  assert_eq!(last(&h, HERALD_AT).text, NEWS[3]);
  let row = format!("row \"Herald, bot, {}, 2 unread\"", NEWS[3]);
  reads(&h.a11y(), &[&row]);

  // Read as it opens, and what comes while it is open is read.
  h.app_mut().open_chat(HERALD_AT);
  h.wait(NEWS_EVERY);
  let c = &h.app().chats[HERALD_AT];
  assert_eq!((c.msgs.len(), c.unread), (n + 2, 0));

  h.app_mut().open_chat(ALICE_AT);
  h.app_mut().chats[HERALD_AT].muted = true;
  h.advance(NEWS_EVERY);
  h.frame();
  let c = &h.app().chats[HERALD_AT];
  assert_eq!((c.msgs.len(), c.unread), (n + 3, 0), "muted");
}

// MARK: People and bots

/// Item `i`'s row in the chat list.
fn row(i: usize) -> Id {
  Id::of(CHATS).key(("row", i as u64))
}

/// A bot is marked in its row (read as one too) and its card; a person isn't, and shows
/// whether they are online. A bot's avatar is a rounded square, a person's a circle: the corner
/// of its box is the bot's colour, the ground's for a person.
#[test]
fn people_and_bots_are_marked_apart() {
  let mut h = messenger();
  reads(
    &h.a11y(),
    &["row \"Echo, bot, reward\"", "row \"Ken Tanaka, Will do after the tests pass.\""],
  );
  let img = h.render();
  let corner = |i: usize| {
    let r = rect(&h, row(i));
    img.pixel(r.x + 10.0 + 4.0, r.y + r.h / 2.0 - 22.0 + 4.0)
  };
  let tint = |i: usize| px(h.app().chats[i].tint);
  assert!(diff(corner(ECHO_AT), tint(ECHO_AT)) < 8, "a rounded square: {:?}", corner(ECHO_AT));
  assert!(diff(corner(KEN_AT), tint(KEN_AT)) > 60, "a circle: {:?}", corner(KEN_AT));

  h.app_mut().profile = true;
  h.frame();
  reads(&h.a11y(), &["static text \"online\"", "static text \"@alice\"", "static text \"Bio\""]);
  h.app_mut().open_chat(KEN_AT);
  h.frame();
  reads(&h.a11y(), &["static text \"last seen recently\""]);
  h.app_mut().open_chat(ECHO_AT);
  h.frame();
  reads(&h.a11y(), &["static text \"bot\"", "static text \"Description\""]);
}

/// The selected chat's row is in the accent wherever the focus is, the list's or the
/// composer's, as Messages' and Mail's: the colour doesn't flicker as you type.
#[test]
fn the_selection_keeps_its_colour_as_the_focus_moves() {
  let mut h = messenger();
  let at = |h: &mut H| {
    let r = rect(h, row(ALICE_AT));
    h.render().pixel(r.x + r.w / 2.0, r.y + 3.0)
  };
  h.click_id(row(ALICE_AT));
  h.frame();
  assert_eq!(h.focus_id().map(|f| f == field(ALICE_AT)), Some(false), "the list's");
  let listed = at(&mut h);
  assert!(diff(listed, px(Theme::system(Appearance::LIGHT).accent())) < 8, "{listed:?}");

  h.click_id(field(ALICE_AT));
  h.wait(Duration::from_millis(300));
  assert_eq!(h.focus_id(), Some(field(ALICE_AT)));
  assert_eq!(at(&mut h), listed);
}

/// The sidebar's edge drags it wider and narrower between its bounds, as Messages': the chat
/// beside it takes the rest.
#[test]
fn the_sidebar_drags_between_its_bounds() {
  let mut h = messenger();
  let edge = |h: &H| rect(h, Id::of(SPLIT).key("handle"));
  let side = |h: &H| rect(h, row(ALICE_AT)).w;
  let drag = |h: &mut H, by: f32| {
    let (x, y) = mid(edge(h));
    h.move_to(x, y);
    h.press(x, y);
    h.move_to(x + by, y);
    h.frame();
    h.release(x + by, y);
    h.frame();
  };

  let (x, w) = (mid(edge(&h)).0, side(&h));
  drag(&mut h, 40.0);
  assert_eq!((mid(edge(&h)).0, side(&h)), (x + 40.0, w + 40.0), "dragged");

  drag(&mut h, 500.0);
  let most = mid(edge(&h)).0;
  assert!((most - SIDE_MAX).abs() <= 4.0, "at its widest: {most}");
  drag(&mut h, -500.0);
  let least = mid(edge(&h)).0;
  assert!((least - SIDE_MIN).abs() <= 4.0, "at its narrowest: {least}");
  assert!(h.app().side > 0.0 && h.app().side * 900.0 < SIDE_MIN + 4.0, "kept: {}", h.app().side);
}

// MARK: The lazy column

#[test]
fn the_archive_builds_lazily_at_its_bottom() {
  let mut h = messenger();
  h.app_mut().open_chat(ARCHIVE_AT);
  h.frame();
  h.frame();
  let bubbles = h.find_all(Role::Group).len();
  assert!(bubbles > 3 && bubbles < 60, "{bubbles} of {ARCHIVE} built");
  let s = h.scroll(area(&h)).unwrap();
  assert!(s.offset.y > 0.0 && s.offset.y >= s.max().y - 1.0, "at the bottom: {s:?}");
  let id = Id::of(BUBBLE).key(last(&h, ARCHIVE_AT).id);
  let (r, a) = (rect(&h, id), rect(&h, area(&h)));
  assert!(r.y + r.h <= a.y + a.h + 0.5 && r.y >= a.y, "the last message shows");
}

/// The archive's 3,000 messages open about as fast as a chat of a few (all of them took over a
/// second, against 4 ms): relative, as tests in parallel slow both alike, the best of three,
/// after a first window loads the fonts.
#[test]
fn the_archive_opens_as_fast_as_a_short_chat() {
  let mut warm = messenger();
  warm.app_mut().open_chat(ARCHIVE_AT);
  warm.frame();

  let mut h = messenger();
  let mut open = |c: usize| {
    let t = std::time::Instant::now();
    h.app_mut().open_chat(c);
    h.frame();
    t.elapsed()
  };
  let (mut short, mut archive) = (Duration::MAX, Duration::MAX);
  for _ in 0..3 {
    short = short.min(open(ABACUS_AT));
    archive = archive.min(open(ARCHIVE_AT));
  }

  assert!(archive < short * 5 + Duration::from_millis(30), "{archive:?} against {short:?}");
  let s = h.scroll(area(&h)).unwrap();
  assert!(s.offset.y >= s.max().y - 1.0, "at the bottom: {s:?}");
}

/// A mouse wheel's notch: the chat glides up by it.
fn wheel(h: &mut H, dy: f32) {
  let (x, y) = mid(rect(h, area(h)));
  let (pos, delta, mods) = (Point::new(x, y), Point::new(0.0, dy), Modifiers::empty());
  let (phase, momentum) = (Phase::None, Phase::None);
  h.event(Event::Scroll { pos, delta, precise: false, phase, momentum, mods });
  h.wait(Duration::from_millis(500));
}

/// From its bottom, a wheel's notch and a trackpad's slow start in steps of 2 points each
/// scroll the chat up, the last message moving down by just that; a message coming then
/// leaves it there.
#[test]
fn the_wheel_and_the_trackpad_scroll_the_chat_up_from_its_bottom() {
  let mut h = messenger();
  h.app_mut().open_chat(ARCHIVE_AT);
  h.frame();
  let last = Id::of(BUBBLE).key(last(&h, ARCHIVE_AT).id);
  let y = rect(&h, last).y;
  wheel(&mut h, 40.0);
  assert_eq!(rect(&h, last).y, y + 40.0, "the wheel");

  for _ in 0..10 {
    let (x, y) = mid(rect(&h, area(&h)));
    let (pos, delta, mods) = (Point::new(x, y), Point::new(0.0, 2.0), Modifiers::empty());
    let (phase, momentum) = (Phase::Changed, Phase::None);
    h.event(Event::Scroll { pos, delta, precise: true, phase, momentum, mods });
    h.frame();
  }

  assert_eq!(rect(&h, last).y, y + 60.0, "the trackpad");

  // Scrolled up so little, a message coming leaves what shows where it is.
  let mut m = h.app().chats[ARCHIVE_AT].msgs[0].clone();
  (m.id, m.mine) = (u64::MAX, false);
  h.app_mut().chats[ARCHIVE_AT].msgs.push(m);
  h.redraw();
  h.frame();
  assert_eq!(rect(&h, last).y, y + 60.0, "a message came");
}

/// A trackpad's scroll of `dy` points over the messages.
fn swipe(h: &mut H, dy: f32) {
  let (x, y) = mid(rect(h, area(h)));
  let (pos, delta, mods) = (Point::new(x, y), Point::new(0.0, dy), Modifiers::empty());
  let (phase, momentum) = (Phase::Changed, Phase::None);
  h.event(Event::Scroll { pos, delta, precise: true, phase, momentum, mods });
  h.frame();
  h.frame();
}

#[test]
fn an_answer_keeps_the_bottom_unless_scrolled_up_and_a_message_sent_brings_it_back() {
  let mut h = answering_in(Duration::from_millis(100));
  h.app_mut().open_chat(ARCHIVE_AT);
  h.frame();
  let bottom = |h: &H| {
    let s = h.scroll(area(h)).unwrap();
    s.offset.y >= s.max().y - 1.0
  };
  say(&mut h, "first");
  settle(&mut h);
  h.frame();
  assert!(bottom(&h), "kept at the bottom");

  // Scrolled up as the bot thinks: its answer leaves the view where it is.
  say(&mut h, "second");
  swipe(&mut h, 600.0);
  let up = h.scroll(area(&h)).unwrap().offset.y;
  assert!(!bottom(&h));
  settle(&mut h);
  h.frame();
  assert_eq!(last(&h, ARCHIVE_AT).text, "Kept. That makes 3004 messages here.");
  assert_eq!(h.scroll(area(&h)).unwrap().offset.y, up, "scrolled up, it stays");

  // A message sent brings it back.
  say(&mut h, "third");
  h.frame();
  assert!(bottom(&h), "sent, back at the bottom");
}

// MARK: Search

#[test]
fn the_search_finds_across_chats_and_opens_the_message() {
  let mut h = messenger();
  h.app_mut().chats[ARCHIVE_AT].msgs[100].text = "a needle in the archive".into();
  h.app_mut().chats[ORACLE_AT].msgs[2].text = "another needle".into();
  h.click_id(Id::of(SEARCH));
  h.type_text("NEEDLE");
  h.frame();
  assert_eq!(h.app().hits(), [(ORACLE_AT, 2), (ARCHIVE_AT, 100)]);
  reads(&h.a11y(), &["list \"Found\""]);
  assert!(h.find(Role::List, "Chats").is_none());

  let row = Id::of(HITS).key(("row", 1u64));
  h.click_id(row);
  h.frame();
  h.frame();
  assert_eq!(h.app().open, ARCHIVE_AT);
  let id = Id::of(BUBBLE).key(h.app().chats[ARCHIVE_AT].msgs[100].id);
  let (r, a) = (rect(&h, id), rect(&h, area(&h)));
  assert!(r.y >= a.y - 0.5 && r.y + r.h <= a.y + a.h + 0.5, "{r:?} in {a:?}");

  h.app_mut().query = "zzz".into();
  h.frame();
  reads(&h.a11y(), &["static text \"No messages found\""]);
}

// MARK: Messages

#[test]
fn a_message_menu_copies_and_deletes() {
  let mut h = messenger();
  let m = last(&h, 0).clone();
  let (x, y) = mid(rect(&h, Id::of(BUBBLE).key(m.id)));
  h.right_click(x, y);
  h.frame();
  h.click_menu_item("Copy");
  h.frame();
  assert_eq!(h.clipboard().read().as_deref(), Some(m.text.as_str()));

  h.right_click(x, y);
  h.frame();
  h.click_menu_item("Delete");
  h.frame();
  let c = &h.app().chats[0];
  assert!(c.msgs.iter().all(|k| k.id != m.id));
  assert_eq!(c.msgs.len(), 5);
}

#[test]
fn an_undo_in_one_chat_never_brings_another_chats_text() {
  let mut h = messenger();
  h.click_id(field(0));
  h.type_text("secret");
  for _ in 0..6 {
    key(&mut h, NamedKey::Backspace, Modifiers::empty());
  }

  h.key(Key::Char(']'), CMD);
  h.frame();
  assert_eq!(h.focus_id(), Some(field(1)), "the focus follows to the chat opened");
  h.key(Key::Char('z'), CMD);
  h.frame();
  assert_eq!(h.app().chats[1].draft, "", "nothing of the first chat's");
}

/// The emoji button asks for the system's picker for the composer, focused or not, and what is
/// picked comes into it; where the OS has no picker there is no button. IME's text comes too.
#[test]
fn emoji_from_the_system_picker_and_ime_go_into_the_composer() {
  let mut h = messenger();
  if character_palette::available().is_err() {
    assert!(h.rect(Id::of(EMOJI)).is_none(), "no button without a picker");
    return;
  }

  reads(&h.a11y(), &["button \"Emoji\""]);
  assert!(!h.character_palette(None), "not asked before a click");
  h.click_id(Id::of(EMOJI));
  h.frame();
  assert!(h.character_palette(Some("😀")), "asked, with the field to type into");
  h.frame();
  assert_eq!((h.app().chats[0].draft.as_str(), h.focus_id()), ("😀", Some(field(0))));

  h.ime_mark("ni");
  h.frame();
  h.ime_commit("你");
  h.frame();
  assert_eq!(h.app().chats[0].draft, "😀你");
}

// MARK: Files

fn sunset() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/images/sunset.jpg")
}

#[test]
fn a_file_from_the_open_panel_sends_with_its_picture() {
  let mut h = messenger();
  assert!(h.file_dialog(None).is_none(), "no panel before a click");
  h.click_id(Id::of(ATTACH));
  assert!(h.file_dialog(None).is_some(), "cancelled");
  h.frame();
  assert!(h.app().chats[0].files.is_empty());

  h.click_id(Id::of(ATTACH));
  let asked = h.file_dialog(Some(&sunset())).expect("the open panel");
  assert_eq!(asked.kind(), FileDialogKind::Open);
  h.frame();
  let files = &h.app().chats[0].files;
  assert!(files.len() == 1 && files[0].image.is_some(), "the picture decoded");
  reads(&h.a11y(), &["static text \"sunset.jpg\"", "button \"Send\""]);

  key(&mut h, NamedKey::Enter, Modifiers::empty());
  h.click_id(Id::of(SEND));
  let m = last(&h, 0).clone();
  assert!(m.mine && m.files.len() == 1 && m.text.is_empty());
  h.frame();
  let r = rect(&h, Id::of(BUBBLE).key(m.id));
  assert!(r.h > 120.0, "the thumbnail in its bubble: {r:?}");
}

#[test]
fn files_dragged_over_the_chat_light_it_and_drop_into_the_composer() {
  let mut h = messenger();
  let zone = rect(&h, Id::of(DROP));
  let edge = |h: &mut H| h.render().pixel(zone.x + 1.0, zone.y + zone.h / 2.0);
  let idle = edge(&mut h);
  let paths = vec![PathBuf::from("/tmp/notes.txt")];
  let (x, y) = mid(zone);
  h.event(Event::DragOver { pos: Point::new(x, y), paths: paths.clone() });
  h.wait(Duration::from_millis(300));
  assert!(h.app().dragging);
  assert!(diff(edge(&mut h), idle) > 40, "lit");
  reads(&h.a11y(), &["static text \"Drop files to send them\""]);

  // Dropped on the sidebar: nothing.
  h.event(Event::Drop { pos: Point::new(100.0, 300.0), paths: paths.clone() });
  h.wait(Duration::from_millis(300));
  assert!(!h.app().dragging && h.app().chats[0].files.is_empty());
  assert!(diff(edge(&mut h), idle) < 4, "dark again");

  h.event(Event::DragOver { pos: Point::new(x, y), paths: paths.clone() });
  h.event(Event::Drop { pos: Point::new(x, y), paths });
  h.frame();
  assert_eq!(h.app().chats[0].files.len(), 1);
  assert_eq!(h.app().chats[0].files[0].name(), "notes.txt");

  // Its ✕ takes it back.
  let x = h.find(Role::Button, "Delete").unwrap().id;
  h.click_id(x);
  assert!(h.app().chats[0].files.is_empty());
}

// MARK: Profile

#[test]
fn the_profile_exports_the_chat_through_the_save_panel() {
  let mut h = messenger();
  h.click_id(Id::of(PROFILE));
  h.frame();
  reads(
    &h.a11y(),
    &["static text \"Designs the windows you look at.\"", "static text \"Messages\""],
  );
  h.click_id(Id::of(EXPORT));
  let out = std::env::temp_dir().join(format!("nimbo-messenger-{}.txt", std::process::id()));
  let asked = h.file_dialog(Some(&out)).expect("the save panel");
  assert_eq!((asked.kind(), asked.get_name()), (FileDialogKind::Save, Some("Alice Moreau.txt")));
  let text = std::fs::read_to_string(&out).unwrap();
  _ = std::fs::remove_file(&out);
  assert_eq!(text.lines().count(), 6);
  assert!(text.starts_with("[9:00] Alice Moreau: "), "{text}");
}

/// The menu bar's commands reach the window past its dialog: while it asks, they do nothing, so
/// it clears the chat it asked about.
#[test]
fn the_menu_waits_while_the_clear_dialog_asks() {
  let mut h = messenger();
  h.app_mut().profile = true;
  h.frame();
  h.click_id(Id::of(CLEAR));
  h.wait(Duration::from_millis(300));
  h.app_mut().run("next");
  h.frame();
  assert_eq!(h.app().open, 0);

  let clear = h.find(Role::Button, "Clear").unwrap().id;
  h.click_id(clear);
  h.wait(Duration::from_millis(300));
  assert!(h.app().chats[0].msgs.is_empty());
  assert_eq!(h.app().chats[1].msgs.len(), 3, "the next chat's kept");
}

/// A panel answers for the chat it was asked from, though another opened meanwhile (Wayland's
/// chooser is no sheet).
#[test]
fn a_panel_answers_for_the_chat_that_asked() {
  let mut h = messenger();
  h.click_id(Id::of(PROFILE));
  h.frame();
  h.click_id(Id::of(EXPORT));
  h.app_mut().open_chat(1);
  h.frame();
  let out = std::env::temp_dir().join(format!("nimbo-messenger-asked-{}.txt", std::process::id()));
  h.file_dialog(Some(&out)).expect("the save panel");
  let text = std::fs::read_to_string(&out).unwrap();
  _ = std::fs::remove_file(&out);
  assert_eq!(text, h.app().transcript(0), "Echo's");

  h.click_id(Id::of(ATTACH));
  h.app_mut().open_chat(2);
  h.frame();
  h.file_dialog(Some(&sunset())).expect("the open panel");
  h.frame();
  assert_eq!((h.app().chats[1].files.len(), h.app().chats[2].files.len()), (1, 0));
}

/// At the window's least width the profile goes over the chat's trailing edge, inside the
/// window, and the composer keeps its width.
#[test]
fn a_narrow_window_lays_the_profile_over_the_chat() {
  let mut h = Harness::new(Messenger::default(), Size::new(560.0, 380.0), 1.0);
  h.frame();
  let w = rect(&h, field(0)).w;
  h.click_id(Id::of(PROFILE));
  h.frame();
  h.frame();
  let p = rect(&h, Id::of(PANEL));
  assert!(p.x + p.w <= 560.0 && p.w == PANEL_W, "{p:?}");
  assert_eq!(rect(&h, field(0)).w, w);

  // Scrolled down to its buttons, below the window's bottom.
  let (pos, delta, mods) =
    (Point::new(mid(p).0, 300.0), Point::new(0.0, -400.0), Modifiers::empty());
  let (phase, momentum) = (Phase::None, Phase::None);
  h.event(Event::Scroll { pos, delta, precise: true, phase, momentum, mods });
  h.wait(Duration::from_millis(300));
  h.click_id(Id::of(EXPORT));
  assert!(h.file_dialog(None).is_some(), "its buttons work over the chat");
}

#[test]
fn clear_history_asks_first() {
  let mut h = messenger();
  h.app_mut().profile = true;
  h.frame();
  h.click_id(Id::of(CLEAR));
  h.wait(Duration::from_millis(300));
  reads(&h.a11y(), &["static text \"Clear the history?\""]);
  key(&mut h, NamedKey::Escape, Modifiers::empty());
  h.wait(Duration::from_millis(300));
  assert_eq!(h.app().chats[0].msgs.len(), 6, "cancelled");

  h.click_id(Id::of(CLEAR));
  h.wait(Duration::from_millis(300));
  let clear = h.find(Role::Button, "Clear").unwrap().id;
  h.click_id(clear);
  h.wait(Duration::from_millis(300));
  assert!(h.app().chats[0].msgs.is_empty());
  reads(&h.a11y(), &["button \"Clear History…\" disabled"]);
}

// MARK: Keys and the title bar

#[test]
fn the_window_keys_turn_chats_and_panels() {
  let mut h = messenger();
  h.key(Key::Char(']'), CMD);
  h.key(Key::Char(']'), CMD);
  assert_eq!(h.app().open, 2);
  h.key(Key::Char('['), CMD);
  assert_eq!(h.app().open, 1);
  assert_eq!(h.app().chats[1].unread, 0, "read as it opens");
  h.key(Key::Char('i'), CMD);
  assert!(h.app().profile);
  h.key(Key::Char('s'), CMD | CTRL);
  assert!(!h.app().sidebar);

  h.key(Key::Char('f'), CMD);
  h.wait(Duration::from_millis(300));
  assert!(h.app().sidebar);
  assert_eq!(h.focus_id(), Some(Id::of(SEARCH)));
}

/// macOS 27's toolbar: from the zoom button to the first leading item.
const AFTER_BUTTONS: f32 = 17.0;

#[test]
fn the_sidebar_toggle_trails_it_and_follows_it_to_the_traffic_lights() {
  let mut h = messenger();
  h.set_titlebar(Some(mac_titlebar("Messenger", false)));
  h.frame();
  let lights = mac_titlebar("Messenger", true).buttons.unwrap();
  let after = lights.x + lights.w + AFTER_BUTTONS;
  let toggle = |h: &H| rect(h, Id::of(TOGGLE));
  let side = rect(&h, Id::of(CHATS));

  let at = toggle(&h);
  assert_eq!(mid(at).1, lights.y + lights.h / 2.0);
  assert_eq!(at.x + at.w, side.x + side.w - 8.0, "by the divider");
  h.click_id(Id::of(TOGGLE));
  h.wait(Duration::from_millis(60));
  let half = h.rect(Id::of(CHATS)).map_or(0.0, |r| r.w);
  assert!(half > 0.0 && half < side.w, "sliding: {half}");
  h.wait(Duration::from_millis(150));
  assert!(h.rect(Id::of(CHATS)).is_none());
  assert_eq!(toggle(&h).x, after, "right of the lights");

  h.click_id(Id::of(TOGGLE));
  h.wait(Duration::from_millis(200));
  assert_eq!(rect(&h, Id::of(CHATS)).w, side.w, "back at its share");
}

#[test]
fn the_empty_title_bar_moves_the_window_and_the_panel_goes_up_to_it() {
  let mut h = messenger();
  h.set_titlebar(Some(mac_titlebar("Messenger", false)));
  h.frame();
  let (t, p) = (mid(rect(&h, Id::of(TOGGLE))), mid(rect(&h, Id::of(PROFILE))));
  assert!(!h.drags_window(t.0, t.1) && !h.drags_window(p.0, p.1));
  assert!(h.drags_window(700.0, 20.0) && h.drags_window(150.0, 5.0));
  let img = h.render();
  let panel = Theme::PLATES_LIGHT.panel();
  assert!(diff(img.pixel(5.0, 5.0), px(panel)) < 3, "{:?}", img.pixel(5.0, 5.0));
  assert!(diff(img.pixel(5.0, 40.0), px(panel)) < 3);
  assert!(rect(&h, Id::of(SEARCH)).y >= 52.0, "the sidebar's content under the bar");
}

#[test]
fn under_the_os_title_bar_the_toolbar_is_the_top_row() {
  let h = messenger();
  let (at, side) = (rect(&h, Id::of(TOGGLE)), rect(&h, Id::of(CHATS)));
  assert_eq!((at.x + at.w, mid(at).1), (side.x + side.w - 8.0, 26.0));
  assert!(!h.drags_window(700.0, 26.0));
}

// MARK: The app

/// A window is a view over the messenger the app keeps: closed with an answer on its way and a
/// draft, the next one, opened before the closed one is gone, has both, at the chat's bottom.
#[test]
fn a_closed_window_leaves_everything_to_the_next() {
  let mut app = Messenger::default();
  app.delay = Duration::from_millis(100);
  let m = Rc::new(RefCell::new(app));
  let mut h = Harness::new(Shown::new(m.clone()), Size::new(900.0, 600.0), 1.0);
  h.frame();
  {
    let mut m = m.borrow_mut();
    m.open_chat(ECHO_AT);
    m.chats[ECHO_AT].draft = "olleh".into();
    m.send();
    m.chats[ORACLE_AT].draft = "a draft".into();
  }

  let mut next = Harness::new(Shown::new(m.clone()), Size::new(900.0, 600.0), 1.0);
  drop(h);
  for _ in 0..500 {
    next.redraw();
    next.frame();
    if m.borrow().pending() == 0 {
      break;
    }

    std::thread::sleep(Duration::from_millis(2));
  }

  next.frame();
  let s = next.scroll(Id::of(MESSAGES).key(ECHO_AT)).unwrap();
  assert!(s.offset.y >= s.max().y - 1.0, "at the bottom: {s:?}");
  let m = m.borrow();
  assert_eq!((m.chats[ECHO_AT].msgs.last().unwrap().text.as_str(), m.pending()), ("hello", 0));
  assert_eq!(m.chats[ORACLE_AT].draft, "a draft");
}

/// With no window open, the app takes an answer in (as the bots' wake has it do): unread.
#[test]
fn an_answer_with_no_window_open_is_taken_in_unread() {
  let mut app = Messenger::default();
  app.delay = Duration::from_millis(20);
  let m = Rc::new(RefCell::new(app));
  let mut h = Harness::new(Shown::new(m.clone()), Size::new(900.0, 600.0), 1.0);
  h.frame();
  m.borrow_mut().open_chat(ECHO_AT);
  m.borrow_mut().chats[ECHO_AT].draft = "olleh".into();
  m.borrow_mut().send();

  drop(h);
  for _ in 0..500 {
    m.borrow_mut().take_answers(false);
    if m.borrow().pending() == 0 {
      break;
    }

    std::thread::sleep(Duration::from_millis(2));
  }

  let m = m.borrow();
  let echo = &m.chats[ECHO_AT];
  assert_eq!((echo.msgs.last().unwrap().text.as_str(), echo.unread), ("hello", 1));
}

#[test]
fn the_seeded_chats_are_the_same_each_run() {
  let (a, b) = (chats().0, chats().0);
  let sum = |c: &[Chat]| {
    c.iter().map(|c| c.msgs.iter().map(|m| m.text.len()).sum::<usize>()).sum::<usize>()
  };
  assert_eq!(sum(&a), sum(&b));
  assert_eq!(a[ARCHIVE_AT].msgs.len(), ARCHIVE);
  let lens: Vec<usize> = a[ARCHIVE_AT].msgs.iter().map(|m| m.text.len()).collect();
  assert!(lens.iter().min().unwrap() * 5 < *lens.iter().max().unwrap(), "mixed heights");
}

#[test]
fn the_bots_answer_in_their_ways() {
  let bot = |b: Bot, text: &str, n: usize| answer(Contact::Bot(b), text, n);
  assert_eq!(bot(Bot::Echo, "abc", 0), "cba");
  assert_eq!(bot(Bot::Abacus, "2+3*4", 0), "= 20");
  assert_eq!(bot(Bot::Abacus, "hi", 0), "Send me a sum, like 2 + 2.");
  assert_eq!(sum("10 / 4 - 1"), Some(1.5));
  assert_eq!(sum("1 +"), None);
  assert_ne!(bot(Bot::Oracle, "a", 0), bot(Bot::Oracle, "a", 1));
  assert_ne!(answer(Contact::Person, "a", 0), answer(Contact::Person, "a", 1));
}

#[test]
fn the_tray_menu_counts_the_unread() {
  let labels = |n| {
    let m = tray_menu(n);
    m.items().iter().map(|i| (i.label().to_owned(), i.is_enabled())).collect::<Vec<_>>()
  };
  let l = labels(3);
  assert_eq!(l[0], ("3 unread".into(), false));
  assert_eq!(l[1], ("Open Messenger".into(), true));
  assert_eq!(tray_menu(0).items()[4].action(), Some(&TrayAction::Quit));
}

#[test]
fn the_chat_menu_shows_the_window_keys() {
  let m = chat_menu();
  let keys: Vec<String> =
    m.items().iter().filter_map(|i| i.shortcut()).map(|s| s.to_string()).collect();
  assert_eq!(keys.len(), KEYS.len());
  assert!(m.items().iter().any(|i| i.action() == Some(&"find") && i.shortcut().is_some()));
}

#[test]
fn it_reads_to_assistive_technology() {
  let mut h = messenger();
  reads(
    &h.a11y(),
    &[
      "text field \"Search\" value \"\"",
      "list \"Chats\"",
      "row \"Alice Moreau, The one with the profile open, if you have it 🙏\" selected",
      "row \"Oracle, bot, A small binary brings great joy., 2 unread\"",
      "button \"Sidebar\" desc \"Hide Sidebar\"",
      "button \"Profile\"",
      "button \"Attach Files\"",
      "text area \"Message\" value \"\"",
      "button \"Send\" disabled",
    ],
  );
  let first = &h.app().chats[0].msgs[3];
  let who = if first.mine { "You" } else { "Alice Moreau" };
  let name = format!("group \"{who}, {}\"", clock(first.at));
  reads(&h.a11y(), &[&name]);
  h.frame();
}

#[test]
fn in_russian() {
  let mut h = messenger();
  h.set_locale(Locale::Tag("ru"), &[("ru", RU)]);
  h.frame();
  reads(&h.a11y(), &["text field \"Поиск\" value \"\"", "button \"Отправить\" disabled"]);
  assert_eq!(tray_menu(5).items()[0].label(), "5 непрочитанных");
  assert_eq!(chat_menu().items()[0].label(), "Найти");
  h.set_locale(Locale::Tag("en"), &[("ru", RU)]);
}

// MARK: Settings

fn settings() -> Harness<Settings> {
  let mut h = Harness::new(Settings::default(), Size::new(560.0, 360.0), 1.0);
  h.event(Event::Appearance(Appearance::LIGHT));
  h.frame();
  h
}

#[test]
fn settings_toggle_login_pick_a_language_and_reset_the_shortcut() {
  let mut h = settings();
  h.click_id(Id::of(LOGIN));
  assert_eq!(h.app().login, Some(true));
  h.frame();
  reads(
    &h.a11y(),
    &["pop up button \"Language\" value \"System Default\"", "button \"Reset\" disabled"],
  );

  h.app_mut().hotkey = None;
  h.frame();
  h.click_id(Id::of(RESET));
  assert_eq!(h.app().hotkey, nimbo::Hotkey::parse(DEFAULT_HOTKEY).ok());

  let tab = h.find(Role::Tab, "About").unwrap().id;
  h.click_id(tab);
  h.frame();
  reads(&h.a11y(), &["static text \"Chats with bots, made with nimbo.\""]);
}

#[test]
fn settings_open_again_on_what_they_chose() {
  let chosen = nimbo::Hotkey::parse("ctrl+option+k").ok();
  set_hotkey(chosen, true);
  chose_lang(Some("ru"));
  let mut h = Harness::new(settings_app(Some(false), None), Size::new(560.0, 360.0), 1.0);
  h.frame();
  assert_eq!((h.app().hotkey, h.app().lang), (chosen, Some("ru")));
  reads(&h.a11y(), &["button \"Reset\" desc"]);
  h.click_id(Id::of(RESET));
  assert_eq!(h.app().hotkey, nimbo::Hotkey::parse(DEFAULT_HOTKEY).ok());
}

thread_local! {
  /// The app's hotkeys as Settings left them, on this test's thread.
  static PAUSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The app's hotkeys pause while the recorder has the keys, so their chord reaches it, and work
/// again when it loses them: to another control, or with the window at once.
#[test]
fn the_recorder_pauses_the_apps_hotkeys() {
  let mut app = Settings::default();
  app.pause_hotkeys = |paused| PAUSED.set(paused);
  let mut h = Harness::new(app, Size::new(560.0, 360.0), 1.0);
  h.frame();
  assert!(!PAUSED.get());

  h.click_id(Id::of(HOTKEY));
  h.frame();
  assert!(PAUSED.get(), "recording");
  h.event(Event::Focus(false));
  assert!(!PAUSED.get(), "the window lost the keys");

  h.event(Event::Focus(true));
  h.frame();
  assert!(PAUSED.get());
  h.set_focus(Some(Id::of(LOGIN)));
  h.frame();
  assert!(!PAUSED.get(), "another control");
}

#[test]
fn settings_without_login_hide_its_row() {
  let mut h = settings();
  h.app_mut().login = None;
  h.frame();
  assert!(h.rect(Id::of(LOGIN)).is_none());
  assert!(h.find(Role::Switch, "Open at login").is_none());
}
