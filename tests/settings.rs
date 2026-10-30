// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Harness tests for `examples/settings.rs`: the toolbar turns pages and the window follows
//! their height; switches flip, a row disabled by another setting ignores clicks; a select opens
//! over its button and chooses by letters typed, closed and open; the shortcut recorder records
//! a chord and a modifier key alone, Escape and a click elsewhere stop it, Backspace clears it,
//! the reset puts the default back; the slider drags and steps; the dialog asks before clearing
//! the cache and resetting all: Return chooses its default, Escape cancels, Tab and Space the
//! action, its backdrop dims the page and takes the pointer; a row's switch keeps its width and
//! place beside a long description that wraps. A row's warning takes its ⓘ's place, in the
//! theme's warning colour, and shows in one tooltip with the ⓘ's tip under it; a disabled row's
//! ⓘ stays enabled and shows its tip. Assistive technology reads the controls by their
//! rows with their values, and the dialog as modal, its default button focused until it gives
//! the focus back, and the focus only while the window has it; its actions flip a switch, step
//! the slider, choose in a select, move the focus, and reach only the dialog while it is open.

#[path = "../examples/settings.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod settings;

mod common;

use std::time::Duration;

use common::{mid, px, reads};
use nimbo::ui::Action;
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

/// The mark of row `row`: its ⓘ, or the warning in its place.
fn mark(h: &H, row: &str) -> Rect {
  h.rect(Id::of(row).key("info")).unwrap_or_else(|| panic!("no mark in {row}"))
}

/// Rests the pointer on the mark of row `row` until its tooltip shows; the tooltip's text.
fn tip(h: &mut H, row: &str) -> String {
  let (x, y) = mid(mark(h, row));
  h.move_to(x, y);
  h.wait(Duration::from_millis(400));
  let tip = Id::of(row).key("info").key("tip");
  assert!(h.rect(tip).is_some(), "no tooltip on the mark of {row}");
  h.text(tip.nth(0)).unwrap_or_default().to_owned()
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
  assert_eq!(h.render().pixel(r.x + 6.0, r.y + r.h / 2.0), px(Theme::PLATES_LIGHT.accent));

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
  // Where the platform grabs no modifier alone (Linux) it records nothing, and Escape stops it.
  let lone = match nimbo::hotkey::modifier_alone() {
    true => "right_option",
    false => {
      key(&mut h, NamedKey::Escape);
      "ctrl+shift+k"
    }
  };

  assert_eq!(hotkey(&h).as_deref(), Some(lone));
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
  assert_eq!(hotkey(&h).as_deref(), Some(lone));
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

#[test]
fn a_switch_reads_its_row_and_its_state() {
  let mut h = start();
  reads(&h.a11y(), &["switch \"Open at login\" on"]);
  h.click_id(Id::of(LOGIN));
  h.frame();
  reads(&h.a11y(), &["switch \"Open at login\" off"]);
}

#[test]
fn a_select_reads_its_value_and_its_open_menu() {
  let mut h = start();
  let caption = "desc \"Applies the next time the app starts.\"";
  reads(&h.a11y(), &[&format!("pop up button \"Language\" value \"System Default\" {caption}")]);
  h.click_id(Id::of(LANG));
  h.wait(Duration::from_millis(50));
  let lines = [
    &format!("pop up button \"Language\" value \"System Default\" expanded {caption}"),
    "menu focused active \"System Default\"",
    "menu item \"System Default\" selected",
    "menu item \"English\"",
  ];
  reads(&h.a11y(), &lines);
}

#[test]
fn the_slider_reads_its_number() {
  let mut h = start();
  turn(&mut h, Page::Appearance);
  reads(&h.a11y(), &["slider \"Text size\" 13 in 10..20"]);
  h.rt.ui.set_focus(Some(Id::of(TEXT_SIZE)));
  key(&mut h, NamedKey::ArrowRight);
  h.frame();
  reads(&h.a11y(), &["slider \"Text size\" 13.5 in 10..20 focused"]);
}

#[test]
fn the_recorder_reads_its_hotkey_and_the_icon_its_label() {
  let h = start();
  let shown = Hotkey::parse(DEFAULT_HOTKEY).unwrap().label();
  let lines = [
    &format!("button \"Show the app\" value \"{shown}\""),
    "button \"Reset\" disabled desc \"Back to the default shortcut\"",
  ];
  reads(&h.a11y(), &lines);
}

#[test]
fn the_dialog_reads_modal_with_its_buttons() {
  let mut h = start();
  turn(&mut h, Page::Advanced);
  h.click_id(Id::of(CLEAR_CACHE));
  h.wait(Duration::from_millis(200));
  let dump = h.a11y();
  reads(&dump, &["dialog \"Clear the cache?\" modal"]);
  // Its buttons inside it, the default one focused.
  let buttons = "\n  button \"Cancel\" focused\n  button \"Clear\"\n";
  assert!(dump.contains(buttons), "{dump}");
}

/// Opening, the dialog takes the focus, its default button focused to assistive technology;
/// closed, it gives the focus back to the button that opened it.
#[test]
fn the_dialog_takes_the_focus_and_gives_it_back() {
  let mut h = start();
  turn(&mut h, Page::Advanced);
  let (clear, confirm) = (Id::of(CLEAR_CACHE), Id::of(CONFIRM));
  assert!(h.act(clear, Action::Focus));
  key(&mut h, ' ');
  h.frame();
  assert_eq!(h.app().ask, Some(Ask::ClearCache));
  assert_eq!(h.focused(), Some(confirm.key("backdrop")));
  reads(&h.a11y(), &["button \"Cancel\" focused"]);
  key(&mut h, NamedKey::Escape);
  h.frame();
  assert_eq!((h.app().ask, h.focused()), (None, Some(clear)));
  reads(&h.a11y(), &["button \"Clear…\" focused"]);
}

/// Assistive technology presses a switch as a click does, the pointer left alone.
#[test]
fn an_assistive_press_flips_a_switch() {
  let mut h = start();
  assert!(h.act(Id::of(LOGIN), Action::Press));
  assert!(!h.app().login);
  // The pointer and the hover stay where they were: nowhere.
  assert!(h.rt.mouse.is_none() && h.rt.ui.hover().is_empty());
  reads(&h.a11y(), &["switch \"Open at login\" off"]);
}

/// Increment and decrement step the slider as the arrows do.
#[test]
fn assistive_steps_move_the_slider() {
  let mut h = start();
  turn(&mut h, Page::Appearance);
  let size = Id::of(TEXT_SIZE);
  assert!(h.act(size, Action::Increment));
  assert_eq!(h.app().text_size, 13.5);
  h.act(size, Action::Decrement);
  h.act(size, Action::Decrement);
  assert_eq!(h.app().text_size, 12.5);
  reads(&h.a11y(), &["slider \"Text size\" 12.5 in 10..20"]);
}

/// A press opens the select's menu, a press on an item chooses it and closes the menu.
#[test]
fn assistive_presses_choose_in_a_select() {
  let mut h = start();
  let lang = Id::of(LANG);
  assert!(h.act(lang, Action::Press));
  reads(&h.a11y(), &["menu focused active \"System Default\"", "menu item \"English\""]);
  assert!(h.act(lang.key(("item", 1)), Action::Press));
  assert_eq!(h.app().lang, 1);
  assert!(h.rect(lang.key("menu")).is_none());
  let caption = "desc \"Applies the next time the app starts.\"";
  reads(&h.a11y(), &[&format!("pop up button \"Language\" value \"English\" {caption}")]);
}

/// Focus moves to an enabled control, not to a disabled one.
#[test]
fn an_assistive_focus_moves_the_focus() {
  let mut h = start();
  assert!(h.act(Id::of(LANG), Action::Focus));
  assert_eq!(h.focused(), Some(Id::of(LANG)));
  assert!(!h.act(Id::of(RESET), Action::Focus));
  assert_eq!(h.focused(), Some(Id::of(LANG)));
}

/// The focus reads only while the window has it: leaving and coming back change the tree, so
/// a screen reader is told of the focus on return.
#[test]
fn the_focus_reads_only_in_the_focused_window() {
  let mut h = start();
  assert!(h.act(Id::of(LANG), Action::Focus));
  let focused = h.a11y();
  assert!(focused.contains(" focused"), "{focused}");
  h.event(Event::Focus(false));
  let away = h.a11y();
  assert!(!away.contains(" focused"), "{away}");
  h.event(Event::Focus(true));
  assert_eq!(h.a11y(), focused);
}

/// An open dialog leaves the page behind it out of the tree, and a press there is refused; its
/// own buttons are pressed.
#[test]
fn a_press_behind_a_modal_dialog_is_refused() {
  let mut h = start();
  turn(&mut h, Page::Advanced);
  assert!(h.act(Id::of(CLEAR_CACHE), Action::Press));
  assert_eq!(h.app().ask, Some(Ask::ClearCache));
  let dump = h.a11y();
  assert!(dump.starts_with("dialog \"Clear the cache?\" modal\n"), "{dump}");
  assert!(!dump.contains("Reset"), "{dump}");
  assert!(!h.act(Id::of(RESET_ALL), Action::Press));
  assert_eq!(h.app().ask, Some(Ask::ClearCache));
  assert!(h.act(Id::of(CONFIRM).key("action"), Action::Press));
  assert_eq!((h.app().ask, h.app().cache), (None, 0));
}

/// A row's warning takes its ⓘ's place, as big, in the theme's warning colour with the "!" cut
/// out of it, in both appearances; it reads as a warning, and goes with what it warns of.
#[test]
fn a_warning_marks_its_row_in_the_warning_colour() {
  let mut h = start();
  let looks = [(Appearance::LIGHT, Theme::PLATES_LIGHT), (Appearance::DARK, Theme::PLATES_DARK)];
  for (look, t) in looks {
    h.event(Event::Appearance(look));
    h.wait(Duration::from_millis(50));
    let (r, img) = (mark(&h, LOGIN_ROW), h.render());
    // In its 24-unit view box: the fill either side of the "!"'s dot, and the ground through
    // the middle of its stem.
    let at = |x: f32, y: f32| img.pixel(r.x + x / 24.0 * r.w, r.y + y / 24.0 * r.h);
    assert_eq!(at(7.0, 17.0), px(t.warning));
    assert_eq!(at(17.0, 17.0), px(t.warning));
    let ground = (-2..=2).any(|i| at(12.0 + i as f32 * 0.2, 11.0) == px(t.plate));
    assert!(ground, "the plate through the \"!\": {:?}", at(12.0, 11.0));
    let info = mark(&h, LAUNCH_ROW);
    assert_eq!((r.w, r.h), (info.w, info.h), "as big as the ⓘ");
  }

  let warning = format!("button \"Warning\" desc {LOGIN_WARNING:?}");
  reads(&h.a11y(), &[&warning, &format!("button \"Info\" desc {TIP_LAUNCH:?}")]);
  h.click_id(Id::of(LOGIN));
  h.frame();
  assert!(h.rect(Id::of(LOGIN_ROW).key("info")).is_none());
}

/// A row's warning shows its text on a rest; with the ⓘ's tip too, both in one tooltip, the
/// warning first, read so; without the warning, the ⓘ and its tip alone.
#[test]
fn a_warning_and_a_tip_show_in_one_tooltip() {
  let mut h = start();
  assert_eq!(tip(&mut h, LOGIN_ROW), LOGIN_WARNING);
  let both = format!("{HOTKEY_WARNING}\n\n{TIP_HOTKEY}");
  assert_eq!(tip(&mut h, HOTKEY_ROW), both);
  reads(&h.a11y(), &[&format!("button \"Warning\" desc {both:?}")]);
  // Another shortcut: nothing to warn of.
  h.app_mut().hotkey = Hotkey::parse("ctrl+shift+k").ok();
  h.rt.redraw();
  h.frame();
  assert_eq!(tip(&mut h, HOTKEY_ROW), TIP_HOTKEY);
  reads(&h.a11y(), &[&format!("button \"Info\" desc {TIP_HOTKEY:?}")]);
}

/// A disabled row's ⓘ stays enabled: as dark as an enabled row's, it reads enabled beside the
/// row's disabled switch and shows its tip on a rest.
#[test]
fn a_disabled_rows_info_shows_its_tip() {
  let mut h = start();
  h.click_id(Id::of(UPDATES));
  h.frame();
  assert!(!h.app().updates);
  let img = h.render();
  let darkest = |r: Rect| {
    let px = (0..(r.w * 2.0) as i32).flat_map(|x| (0..(r.h * 2.0) as i32).map(move |y| (x, y)));
    px.map(|(x, y)| img.pixel(r.x + x as f32 / 2.0, r.y + y as f32 / 2.0)[0]).min().unwrap()
  };
  let (off, on) = (darkest(mark(&h, DOWNLOAD_ROW)), darkest(mark(&h, LAUNCH_ROW)));
  assert!(off.abs_diff(on) <= 8, "the ⓘ at {off} in a disabled row, {on} in an enabled one");
  let info = format!("button \"Info\" desc {TIP_DOWNLOAD:?}");
  reads(&h.a11y(), &[&info, "switch \"Download them in the background\" off disabled"]);
  assert_eq!(tip(&mut h, DOWNLOAD_ROW), TIP_DOWNLOAD);
}
