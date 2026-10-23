// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/input.rs` through the harness: the square dragged by the mouse and kept
//! inside the window, moved by the arrow keys, grown by the wheel, amber while Shift is held; its
//! cursor; Cmd-C and Cmd-V through the app's bindings and the in-memory clipboard; the focus bar;
//! and frames built only when the input changed something.

#[path = "../examples/input.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod input;

mod common;

use common::{mid, px};
use input::*;
use nimbo::harness::Image;
use nimbo::{
  Appearance, CMD, Code, Color, Command, Cursor, Event, Harness, Key, KeyEvent, Modifiers,
  NamedKey, Phase, Point, Size,
};

const NONE: Modifiers = Modifiers::empty();

/// The example, its first frame built.
fn input() -> Harness<Input> {
  let mut h = Harness::new(Input::default(), SIZE, 2.0);
  h.render();
  h
}

/// The square is drawn where the app has it, in `fill`, with the ground just outside.
fn check_square(img: &Image, app: &Input, fill: Color) {
  let sq = app.square();
  let (x, y) = mid(sq);
  assert_eq!(img.pixel(x, y), px(fill), "centre of {sq:?}");
  assert_eq!(img.pixel(sq.x - 1.0, y), px(app.bg()), "left of {sq:?}");
  assert_eq!(img.pixel(x, sq.y + sq.h + 1.0), px(app.bg()), "below {sq:?}");
}

fn wheel(h: &mut Harness<Input>, at: (f32, f32), dy: f32) -> bool {
  h.event(Event::Scroll {
    pos: Point::new(at.0, at.1),
    delta: Point::new(0.0, dy),
    precise: false,
    phase: Phase::None,
    momentum: Phase::None,
    mods: NONE,
  })
}

#[test]
fn the_square_and_the_focus_bar() {
  let mut h = Harness::new(Input::default(), SIZE, 2.0);
  let img = h.render();
  check_square(&img, h.app(), BLUE);
  // The focus bar along the top, until the window loses the keyboard.
  assert_eq!(img.pixel(SIZE.w / 2.0, 1.0), px(GREEN));
  assert!(!h.frame(), "a frame without changes");
}

/// Over the square: an open hand and a border; off it, the arrow. A move that changes nothing
/// builds no frame.
#[test]
fn hover_shows_an_open_hand_and_a_border() {
  let mut h = input();
  let (x, y) = mid(h.app().square());
  assert!(h.move_to(x, y));
  assert_eq!(h.rt.cursor(), Cursor::Grab);
  let img = h.render();
  let sq = h.app().square();
  assert_ne!(img.pixel(sq.x + 0.5, y), px(BLUE), "a hover border");
  assert!(!h.move_to(x + 1.0, y));
  assert!(!h.frame());
  assert!(h.move_to(400.0, 280.0));
  assert_eq!(h.rt.cursor(), Cursor::Arrow);
}

/// Grabbed off its centre, the square follows the pointer by the same offset, under a closed
/// hand. The cursor is the element's under the pointer, as the next frame builds it.
#[test]
fn a_drag_moves_the_square_by_the_pointer() {
  let mut h = input();
  let (x, y) = mid(h.app().square());
  assert!(h.press(x - 10.0, y + 5.0));
  assert!(h.frame());
  assert_eq!(h.rt.cursor(), Cursor::Grabbing);
  assert!(h.move_to(x + 90.0, y + 45.0));
  assert!(h.release(x + 90.0, y + 45.0));
  assert_eq!(h.app().pos, Point::new(START.x + 100.0, START.y + 40.0));
  assert!(h.frame());
  assert_eq!(h.rt.cursor(), Cursor::Grab);
  check_square(&h.render(), h.app(), BLUE);
}

/// Dragged past the window's corners it stays inside, under the focus bar.
#[test]
fn a_drag_keeps_the_square_inside_the_window() {
  let mut h = input();
  let (x, y) = mid(h.app().square());
  h.press(x, y);
  h.move_to(-500.0, -500.0);
  h.release(-500.0, -500.0);
  assert_eq!(h.app().pos, Point::new(0.0, BAR));
  h.press(SIDE / 2.0, BAR + SIDE / 2.0);
  h.move_to(5000.0, 5000.0);
  h.release(5000.0, 5000.0);
  assert_eq!(h.app().pos, Point::new(SIZE.w - SIDE, SIZE.h - SIDE));
}

/// The window shrinks under the square to its least size: the square comes back inside, off the
/// pointer, which stayed where the square was.
#[test]
fn a_smaller_window_brings_the_square_back_inside() {
  let mut h = input();
  h.app_mut().pos = Point::new(SIZE.w - SIDE, SIZE.h - SIDE);
  h.render();
  h.move_to(SIZE.w - SIDE / 2.0, SIZE.h - SIDE / 2.0);
  assert!(h.app().hover);
  h.rt.resize(Size::new(320.0, 240.0), 2.0);
  let img = h.render();
  assert_eq!(h.app().pos, Point::new(320.0 - SIDE, 240.0 - SIDE));
  assert_eq!(img.pixel(320.0 - SIDE / 2.0, 240.0 - SIDE / 2.0), px(BLUE));
  assert!(!h.app().hover);
  h.rt.resize(SIZE, 2.0);
  h.leave();
  assert_eq!(h.rt.mouse, None);
}

/// Arrows move it a step; a key nobody binds or uses is unused, as is the text it types.
#[test]
fn arrows_move_the_square_a_step() {
  let mut h = input();
  assert!(h.key(NamedKey::ArrowLeft, NONE));
  assert!(h.key(NamedKey::ArrowUp, NONE));
  assert!(h.key(NamedKey::ArrowRight, NONE));
  assert!(h.key(NamedKey::ArrowRight, NONE));
  assert!(h.key(NamedKey::ArrowDown, NONE));
  assert!(h.key(NamedKey::ArrowDown, NONE));
  assert_eq!(h.app().pos, Point::new(START.x + STEP, START.y + STEP));
  check_square(&h.render(), h.app(), BLUE);
  assert!(!h.key(Key::Char('x'), NONE));
  assert!(!h.frame());
}

/// The wheel grows it about its centre, down to its least side.
#[test]
fn the_wheel_resizes_the_square_about_its_centre() {
  let mut h = input();
  let at = mid(h.app().square());
  assert!(wheel(&mut h, at, 18.0));
  assert_eq!(h.app().side, SIDE + 18.0);
  assert_eq!(mid(h.app().square()), at);
  check_square(&h.render(), h.app(), BLUE);
  assert!(wheel(&mut h, at, -500.0));
  assert_eq!(h.app().side, SIDES.0);
  assert!(!wheel(&mut h, at, -18.0), "no smaller");
  wheel(&mut h, at, SIDE - SIDES.0);
  assert_eq!(h.app().side, SIDE);
}

/// The square moving or shrinking under a pointer that stays: the hover follows it at once,
/// without a move of the mouse, and the cursor with the frame that shows it.
#[test]
fn the_hover_follows_the_square_under_a_still_pointer() {
  let mut h = input();
  let edge = (START.x + SIDE - 4.0, START.y + SIDE / 2.0);
  h.move_to(edge.0, edge.1);
  h.frame();
  assert!(h.app().hover && h.rt.cursor() == Cursor::Grab);
  assert!(h.key(NamedKey::ArrowLeft, NONE));
  assert!(!h.app().hover, "moved away by an arrow");
  let img = h.render();
  assert_eq!(h.rt.cursor(), Cursor::Arrow);
  assert_eq!(img.pixel(h.app().pos.x + 0.5, edge.1), px(BLUE), "no hover border");
  assert!(h.key(NamedKey::ArrowRight, NONE));
  h.frame();
  assert!(h.app().hover && h.rt.cursor() == Cursor::Grab, "back under the pointer");
  h.rt.clipboard().write("300,200");
  assert!(h.key(Key::Char('v'), CMD));
  h.frame();
  assert!(!h.app().hover && h.rt.cursor() == Cursor::Arrow, "pasted away");
  h.rt.clipboard().write(&format!("{},{}", START.x, START.y));
  assert!(h.key(Key::Char('v'), CMD));
  assert!(h.app().hover);
  assert!(wheel(&mut h, edge, -20.0));
  h.frame();
  assert!(!h.app().hover && h.rt.cursor() == Cursor::Arrow, "shrunk away by the wheel");
  wheel(&mut h, edge, 20.0);
  h.leave();
  assert!(!h.app().hover);
}

/// Shift held: amber, until it is let go.
#[test]
fn shift_turns_the_square_amber() {
  let mut h = input();
  assert!(h.event(Event::Mods(Modifiers::SHIFT)));
  check_square(&h.render(), h.app(), AMBER);
  assert!(!h.event(Event::Mods(Modifiers::SHIFT)), "the same modifiers again");
  assert!(h.event(Event::Mods(NONE)));
  check_square(&h.render(), h.app(), BLUE);
}

/// Cmd-C copies the position through the app's binding; Cmd-V moves the square to a pasted one,
/// and leaves it for text that isn't a position.
#[test]
fn cmd_c_and_cmd_v_copy_and_paste_the_position() {
  let mut h = input();
  assert!(h.key(Key::Char('c'), CMD));
  let copied = h.rt.clipboard().read();
  assert_eq!(copied, Some(format!("{},{}", START.x, START.y)));
  h.rt.clipboard().write(" 200, 120 ");
  assert!(h.key(Key::Char('v'), CMD));
  assert_eq!(h.app().pos, Point::new(200.0, 120.0));
  check_square(&h.render(), h.app(), BLUE);
  h.rt.clipboard().write("hello");
  assert!(!h.key(Key::Char('v'), CMD));
  assert_eq!(h.app().pos, Point::new(200.0, 120.0));
  // A binding's command reaches the app as a command, not as the key.
  let k = KeyEvent { key: Key::Char('v'), text: None, code: Code::KeyV, mods: CMD, repeat: false };
  h.rt.clipboard().write("10,50");
  assert!(h.event(Event::KeyDown(k)));
  assert_eq!(h.app().pos, Point::new(10.0, 50.0));
  assert!(h.event(Event::Command(Command::Paste)));
}

#[test]
fn the_focus_bar_goes_with_the_keyboard() {
  let mut h = input();
  assert!(h.event(Event::Focus(false)));
  let img = h.render();
  assert_eq!(img.pixel(SIZE.w / 2.0, 1.0), px(h.app().bg()));
}

#[test]
fn the_light_appearance() {
  let mut h = input();
  h.event(Event::Appearance(Appearance::LIGHT));
  check_square(&h.render(), h.app(), BLUE);
  assert_eq!(h.render().pixel(5.0, 300.0), px(LIGHT_BG));
}

#[test]
fn positions_parse_from_the_clipboard_text() {
  assert_eq!(parse_point("3,4"), Some(Point::new(3.0, 4.0)));
  assert_eq!(parse_point("3;4"), None);
  assert_eq!(parse_point("NaN,60"), None);
  assert_eq!(parse_point("10,inf"), None);
}
