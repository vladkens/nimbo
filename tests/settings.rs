// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Harness tests for `examples/settings.rs`: the toolbar turns pages and the window follows
//! their height; switches flip, a row disabled by another setting ignores clicks; a select opens
//! over its button and chooses by letters typed, closed and open; the shortcut recorder records
//! a chord and a modifier key alone, Escape and a click elsewhere stop it, Backspace clears it,
//! the reset puts the default back; the slider drags and steps; the dialog asks before clearing
//! the cache and resetting all: Return chooses its default, Escape cancels, Tab and Space the
//! action, its backdrop dims the page and takes the pointer; a row's switch keeps its width and
//! place beside a long description that wraps.

#[path = "../examples/settings.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod settings;

use std::time::Duration;

use nimbo::{
  Appearance, Code, Event, Harness, Hotkey, Id, Key, Modifiers, NamedKey, Rect, Size, Theme,
};
use settings::*;

type H = Harness<Settings>;

fn rect(h: &H, name: &str) -> Rect {
  h.rect(Id::of(name)).unwrap_or_else(|| panic!("no {name}"))
}

fn key(h: &mut H, k: impl Into<Key>) {
  h.key(k, Modifiers::empty());
}

/// Turns to `page` by a click on its tab, laid out and resized.
fn turn(h: &mut H, page: Page) {
  let i = PAGES.iter().position(|p| p.0 == page).unwrap();
  h.click_id(Id::of(TABS).key(i));
  h.wait(Duration::from_millis(50));
  assert_eq!(h.app().page, page);
}

fn hotkey(h: &H) -> Option<String> {
  h.app().hotkey.map(|k| k.to_string())
}

/// The window in the light appearance, its first page laid out.
fn start() -> H {
  let mut h = Harness::new(Settings::default(), Size::new(WIDTH, 560.0), 2.0);
  h.event(Event::Appearance(Appearance::LIGHT));
  h.wait(Duration::from_millis(50));
  h
}

#[test]
fn general_switches_select_and_recorder() {
  let mut h = start();
  // The switch that is on is in the accent.
  let r = rect(&h, LOGIN);
  let accent = Theme::PLATES_LIGHT.accent.map(|c| (c * 255.0).round() as u8);
  assert_eq!(h.render().pixel(r.x + 6.0, r.y + r.h / 2.0), accent);

  // Switches flip; one disabled by another setting doesn't.
  h.click_id(Id::of(LOGIN));
  assert!(!h.app().login);
  h.click_id(Id::of(UPDATES));
  h.frame();
  h.click_id(Id::of(DOWNLOAD));
  assert!(!h.app().updates && !h.app().download);

  // The select: letters typed while it has focus choose an option; open, its menu is over it
  // and letters move the highlight, Enter picks.
  let launch = Id::of(LAUNCH);
  h.rt.ui.set_focus(Some(launch));
  key(&mut h, 'o');
  assert_eq!(h.app().launch, Launch::Window);
  h.click_id(launch);
  h.frame();
  h.frame();
  let (menu, button) = (h.rect(launch.key("menu")).expect("open"), rect(&h, LAUNCH));
  assert!(menu.y < button.y && menu.y + menu.h > button.y + button.h, "{menu:?} over {button:?}");
  key(&mut h, 's');
  key(&mut h, NamedKey::Enter);
  h.frame();
  assert_eq!(h.app().launch, Launch::Hidden);
  assert!(h.rect(launch.key("menu")).is_none());

  // The recorder: a click records the next chord, then a modifier alone.
  let rec = Id::of(HOTKEY);
  h.click_id(rec);
  assert_eq!(h.focused(), Some(rec));
  h.key('k', Modifiers::CONTROL | Modifiers::SHIFT);
  assert_eq!(hotkey(&h).as_deref(), Some("ctrl+shift+k"));
  assert_ne!(h.focused(), Some(rec));
  h.click_id(rec);
  h.event(Event::Modifier { key: Code::AltRight, down: true });
  h.event(Event::Modifier { key: Code::AltRight, down: false });
  assert_eq!(h.app().hotkey, Some(Hotkey::Modifier(Code::AltRight)));
  // A plain key is taken and records nothing; a modifier with another one between isn't alone.
  h.click_id(rec);
  assert!(h.key('a', Modifiers::empty()));
  for (key, down) in [(Code::ShiftLeft, true), (Code::ShiftRight, true)] {
    h.event(Event::Modifier { key, down });
  }

  for (key, down) in [(Code::ShiftLeft, false), (Code::ShiftRight, false)] {
    h.event(Event::Modifier { key, down });
  }

  // Escape stops it unchanged, Backspace clears it.
  key(&mut h, NamedKey::Escape);
  assert_eq!(hotkey(&h).as_deref(), Some("right_option"));
  assert_ne!(h.focused(), Some(rec));
  h.click_id(rec);
  key(&mut h, NamedKey::Backspace);
  assert_eq!(h.app().hotkey, None);
  // A click elsewhere stops it; the reset puts the default back.
  h.click_id(rec);
  h.click_id(Id::of(LOGIN));
  key(&mut h, NamedKey::F5);
  assert_eq!(h.app().hotkey, None);
  h.frame();
  h.click_id(Id::of(RESET));
  assert_eq!(hotkey(&h).as_deref(), Some(DEFAULT_HOTKEY));
}

#[test]
fn appearance_row_width_slider_and_look() {
  let mut h = start();
  // Appearance: the switch keeps its width at the row's right edge, its description wraps
  // beside it onto lines of their own.
  turn(&mut h, Page::Appearance);
  let (sw, row) = (rect(&h, MOTION), rect(&h, MOTION_ROW));
  let caption = h.rect(Id::of(MOTION_ROW).key("caption")).unwrap();
  assert_eq!(sw.w, Theme::PLATES_LIGHT.sizes.switch.track[0]);
  assert!((row.x + row.w - 10.0 - (sw.x + sw.w)).abs() < 0.5, "{sw:?} in {row:?}");
  assert!(caption.h >= 28.0 && caption.x + caption.w < sw.x, "{caption:?}");
  // The slider takes a click between frames, drags to its ends and steps by a twentieth.
  let s = rect(&h, TEXT_SIZE);
  h.click(s.x + s.w / 2.0, s.y + s.h / 2.0);
  assert_eq!(h.app().text_size, 15.0);
  h.frame();
  h.press(s.x + 2.0, s.y + s.h / 2.0);
  h.frame();
  assert_eq!(h.app().text_size, 10.0);
  h.move_to(s.x + s.w + 20.0, s.y + s.h / 2.0);
  h.frame();
  h.release(s.x + s.w + 20.0, s.y + s.h / 2.0);
  assert_eq!(h.app().text_size, 20.0);
  h.rt.ui.set_focus(Some(Id::of(TEXT_SIZE)));
  key(&mut h, NamedKey::ArrowLeft);
  assert_eq!(h.app().text_size, 19.5);
  // The look applies at once.
  h.click_id(Id::of(LOOK).key(2));
  h.wait(Duration::from_millis(50));
  assert!(h.render().pixel(4.0, h.rt.size.h - 4.0)[0] < 64);
}

#[test]
fn advanced_asks_in_a_dialog() {
  let mut h = start();
  h.app_mut().login = false;
  h.rt.redraw();
  // Advanced: the dialog asks before clearing the cache, over a dimmed page.
  turn(&mut h, Page::Advanced);
  let confirm = Id::of(CONFIRM);
  let clear = rect(&h, CLEAR_CACHE);
  let ground = h.render().pixel(4.0, h.rt.size.h - 4.0);
  h.click_id(Id::of(CLEAR_CACHE));
  h.wait(Duration::from_millis(200));
  assert!(h.rect(confirm.key("card")).is_some());
  let dimmed = h.render().pixel(4.0, h.rt.size.h - 4.0);
  assert!(dimmed[0] < ground[0], "{dimmed:?} over {ground:?}");
  // A click on the page under it does nothing; Return chooses Cancel (the action is
  // destructive) and closes it.
  h.click(clear.x + 2.0, clear.y + 2.0);
  assert_eq!(h.app().ask, Some(Ask::ClearCache));
  key(&mut h, NamedKey::Enter);
  h.frame();
  assert_eq!((h.app().ask, h.app().cache), (None, 128));
  assert!(h.rect(confirm.key("card")).is_none());
  // Tab focuses the action, then Cancel, then the action again; Alt-Space goes on to the
  // window menu; Space chooses.
  h.click_id(Id::of(CLEAR_CACHE));
  h.frame();
  key(&mut h, NamedKey::Tab);
  assert_eq!(h.focused(), Some(confirm.key("action")));
  key(&mut h, NamedKey::Tab);
  assert_eq!(h.focused(), Some(confirm.key("cancel")));
  key(&mut h, NamedKey::Tab);
  assert_eq!(h.focused(), Some(confirm.key("action")));
  h.key(' ', Modifiers::ALT);
  assert_eq!(h.app().ask, Some(Ask::ClearCache));
  key(&mut h, ' ');
  h.frame();
  assert_eq!((h.app().ask, h.app().cache), (None, 0));
  // Reset All: Escape cancels, Return resets (the action is the default), keeping the page.
  h.click_id(Id::of(RESET_ALL));
  h.frame();
  key(&mut h, NamedKey::Escape);
  h.frame();
  assert_eq!((h.app().ask, h.app().resets), (None, 0));
  h.click_id(Id::of(RESET_ALL));
  h.frame();
  key(&mut h, NamedKey::Enter);
  h.frame();
  let a = h.app();
  assert_eq!((a.resets, a.cache, a.page, a.login), (1, 128, Page::Advanced, true));
}

#[test]
fn the_window_follows_the_page() {
  // It takes the page's height.
  let mut h = start();
  let general = h.rt.size.h;
  assert!((400.0..720.0).contains(&general), "{general}");
  turn(&mut h, Page::About);
  assert!(h.rt.size.h < general, "{} vs {general}", h.rt.size.h);
}
