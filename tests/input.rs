// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests through the harness of the app below: the square dragged by the mouse and kept inside the
//! window, moved by the arrow keys, grown by the wheel, amber while Shift is held; its cursor;
//! Cmd-C and Cmd-V through the app's bindings and the in-memory clipboard; the focus bar; and
//! frames built only when the input changed something.

#[expect(dead_code, reason = "what the tests don't read")]
mod app {

  //! Input without text: a square dragged with the mouse or moved [STEP] points by the arrow keys,
  //! the wheel grows or shrinks it, the cursor an open hand over it and a closed one while
  //! dragging. Cmd-C (Ctrl-C on Windows and Linux) copies its position as `x,y`, Cmd-V moves it to
  //! a position pasted as `x,y`; it turns amber while Shift is held. A bar along the top shows
  //! that the window has the keyboard focus.

  use nimbo::{
    App, Binding, Color, Command, Cursor, Cx, Event, Float, Frame, Key, MouseButton, NamedKey,
    Point, Quad, Rect, Shortcut, Size, Style, Ui, mix, px, rgb,
  };

  pub const SIZE: Size = Size::new(480.0, 320.0);

  /// The square at the start: its top-left corner and side.
  pub const START: Point = Point::new(40.0, 60.0);
  pub const SIDE: f32 = 80.0;
  /// The side the wheel keeps it within.
  pub const SIDES: (f32, f32) = (24.0, 200.0);
  /// Points an arrow key moves it.
  pub const STEP: f32 = 10.0;
  /// The focus bar's height.
  pub const BAR: f32 = 4.0;
  /// The border drawn while the pointer is over it.
  pub const HOVER_BORDER: f32 = 2.0;

  pub const DARK_BG: Color = rgb(0x1c1c1e);
  pub const LIGHT_BG: Color = rgb(0xf2f2f7);
  pub const BLUE: Color = rgb(0x3b82f6);
  pub const AMBER: Color = rgb(0xf5a524);
  pub const GREEN: Color = rgb(0x30a46c);
  pub const WHITE: Color = rgb(0xffffff);

  /// Copy and paste, as the edit menu binds them: Cmd on macOS, Ctrl elsewhere.
  const KEYS: &[Binding] = &[
    Binding::new(Shortcut::char('c').cmd(), Command::Copy),
    Binding::new(Shortcut::char('v').cmd(), Command::Paste),
  ];

  pub struct Input {
    /// The light appearance; dark by default.
    pub light: bool,
    /// The square's top-left corner, in points.
    pub pos: Point,
    pub side: f32,
    /// Where the square was grabbed, from its corner, while it is dragged.
    pub grab: Option<Point>,
    pub hover: bool,
    pub shift: bool,
    /// The window has the keyboard focus.
    pub focused: bool,
  }

  impl Default for Input {
    fn default() -> Self {
      let (pos, side) = (START, SIDE);
      Self { light: false, pos, side, grab: None, hover: false, shift: false, focused: true }
    }
  }

  impl Input {
    pub fn square(&self) -> Rect {
      Rect::new(self.pos.x, self.pos.y, self.side, self.side)
    }

    pub fn bg(&self) -> Color {
      if self.light { LIGHT_BG } else { DARK_BG }
    }

    /// The square's fill: amber while Shift is held.
    pub fn fill(&self) -> Color {
      if self.shift { AMBER } else { BLUE }
    }

    /// Moves the square's corner to `p`, kept inside the window below the focus bar.
    fn move_to(&mut self, p: Point, size: Size) {
      let x = p.x.clamp(0.0, (size.w - self.side).max(0.0));
      let y = p.y.clamp(BAR, (size.h - self.side).max(BAR));
      self.pos = Point::new(x, y);
    }

    /// An event, without the hover: the square moved, grown, copied or pasted, Shift, the focus.
    fn handle(&mut self, cx: &mut Cx, ev: &Event) -> bool {
      match ev {
        Event::KeyDown(k) => {
          let (dx, dy) = match k.key {
            Key::Named(NamedKey::ArrowLeft) => (-STEP, 0.0),
            Key::Named(NamedKey::ArrowRight) => (STEP, 0.0),
            Key::Named(NamedKey::ArrowUp) => (0.0, -STEP),
            Key::Named(NamedKey::ArrowDown) => (0.0, STEP),
            _ => return false,
          };

          self.move_to(Point::new(self.pos.x + dx, self.pos.y + dy), cx.size());
        }
        Event::Command(Command::Copy) => {
          let text = format!("{},{}", self.pos.x.round(), self.pos.y.round());
          cx.clipboard().write(&text);
        }
        Event::Command(Command::Paste) => {
          let Some(text) = cx.clipboard().read() else { return false };
          let Some(p) = parse_point(&text) else { return false };
          self.move_to(p, cx.size());
        }
        Event::Mods(m) => {
          let was = std::mem::replace(&mut self.shift, m.shift());
          return was != m.shift();
        }
        Event::Focus(on) => self.focused = *on,
        Event::Appearance(a) => self.light = !a.scheme.is_dark(),
        ev => return self.pointer(cx, ev),
      }

      true
    }

    /// The hover from where the pointer is: after it moves, and after the square moves or grows
    /// under a pointer that stays (arrows, a paste, the wheel). Returns whether it changed.
    fn track(&mut self, cx: &mut Cx) -> bool {
      let was = self.hover;
      self.hover = cx.mouse().is_some_and(|p| self.square().contains(p));
      self.hover != was
    }

    fn pointer(&mut self, cx: &mut Cx, ev: &Event) -> bool {
      match *ev {
        Event::MouseMove { pos, .. } => match self.grab {
          Some(g) => {
            self.move_to(Point::new(pos.x - g.x, pos.y - g.y), cx.size());
            true
          }
          None => false,
        },
        Event::MouseDown { pos, button: MouseButton::Left, .. } if self.square().contains(pos) => {
          self.grab = Some(Point::new(pos.x - self.pos.x, pos.y - self.pos.y));
          true
        }
        Event::MouseUp { button: MouseButton::Left, .. } if self.grab.is_some() => {
          self.grab = None;
          true
        }
        Event::Scroll { delta, .. } => {
          let side = (self.side + delta.y).clamp(SIDES.0, SIDES.1);
          let grow = side - self.side;
          self.side = side;

          // Grown or shrunk about its centre.
          let corner = Point::new(self.pos.x - grow / 2.0, self.pos.y - grow / 2.0);
          self.move_to(corner, cx.size());
          grow != 0.0
        }
        _ => false,
      }
    }
  }

  impl App for Input {
    fn keymap(&self) -> &[Binding] {
      KEYS
    }

    fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
      // Back inside a window that shrank since, before the event sees it.
      self.move_to(self.pos, cx.size());
      let used = self.handle(cx, ev);
      self.track(cx) || used
    }

    /// An element over the square for its cursor, out of the flow: an open hand, a closed one
    /// while dragging; the arrow elsewhere.
    fn view(&mut self, ui: &mut Ui) {
      let sq = self.square();
      let cursor = if self.grab.is_some() { Cursor::Grabbing } else { Cursor::Grab };
      let style = Style::new().w(px(sq.w)).h(px(sq.h)).cursor(cursor);
      ui.float(Float::window().offset(sq.x, sq.y)).el(style);
    }

    fn paint(&mut self, f: &mut Frame) {
      // Kept inside a window that shrank, its hover from where the pointer is now; the cursor
      // follows at the next event.
      self.move_to(self.pos, f.size());
      self.hover = f.hover(self.square());
      f.rect(Rect::new(0.0, 0.0, f.size().w, f.size().h), self.bg(), 0.0);
      if self.focused {
        f.rect(Rect::new(0.0, 0.0, f.size().w, BAR), GREEN, 0.0);
      }

      let sq = self.square();
      let mut quad = Quad::new(sq, self.fill()).radius(8.0);
      if self.hover || self.grab.is_some() {
        quad = quad.border(HOVER_BORDER, mix(self.fill(), WHITE, 0.6));
      }

      f.quad(quad);
    }
  }

  /// A position written as `x,y`, spaces allowed around each number; finite numbers only (`NaN`
  /// parses as one, and no clamp would bring it back).
  pub fn parse_point(s: &str) -> Option<Point> {
    let (x, y) = s.trim().split_once(',')?;
    let num = |t: &str| t.trim().parse::<f32>().ok().filter(|v| v.is_finite());
    Some(Point::new(num(x)?, num(y)?))
  }
}

use app::*;
#[expect(dead_code, reason = "this file uses a part of the helpers")]
mod common;

use common::{mid, px};
use nimbo::harness::{Image, us_key};
use nimbo::{
  Appearance, CMD, Color, Command, Cursor, Event, Harness, Key, Modifiers, NamedKey, Phase, Point,
  Size,
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
  assert_eq!(h.cursor(), Cursor::Grab);
  let img = h.render();
  let sq = h.app().square();
  assert_ne!(img.pixel(sq.x + 0.5, y), px(BLUE), "a hover border");
  assert!(!h.move_to(x + 1.0, y));
  assert!(!h.frame());
  assert!(h.move_to(400.0, 280.0));
  assert_eq!(h.cursor(), Cursor::Arrow);
}

/// Grabbed off its centre, the square follows the pointer by the same offset, under a closed
/// hand. The cursor is the element's under the pointer, as the next frame builds it.
#[test]
fn a_drag_moves_the_square_by_the_pointer() {
  let mut h = input();
  let (x, y) = mid(h.app().square());
  assert!(h.press(x - 10.0, y + 5.0));
  assert!(h.frame());
  assert_eq!(h.cursor(), Cursor::Grabbing);
  assert!(h.move_to(x + 90.0, y + 45.0));
  assert!(h.release(x + 90.0, y + 45.0));
  assert_eq!(h.app().pos, Point::new(START.x + 100.0, START.y + 40.0));
  assert!(h.frame());
  assert_eq!(h.cursor(), Cursor::Grab);
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

  h.resize(Size::new(320.0, 240.0), 2.0);
  let img = h.render();
  assert_eq!(h.app().pos, Point::new(320.0 - SIDE, 240.0 - SIDE));
  assert_eq!(img.pixel(320.0 - SIDE / 2.0, 240.0 - SIDE / 2.0), px(BLUE));
  assert!(!h.app().hover);

  h.resize(SIZE, 2.0);
  h.leave();
  assert_eq!(h.mouse(), None);
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
  assert!(h.app().hover && h.cursor() == Cursor::Grab);

  assert!(h.key(NamedKey::ArrowLeft, NONE));
  assert!(!h.app().hover, "moved away by an arrow");
  let img = h.render();
  assert_eq!(h.cursor(), Cursor::Arrow);
  assert_eq!(img.pixel(h.app().pos.x + 0.5, edge.1), px(BLUE), "no hover border");

  assert!(h.key(NamedKey::ArrowRight, NONE));
  h.frame();
  assert!(h.app().hover && h.cursor() == Cursor::Grab, "back under the pointer");

  h.clipboard().write("300,200");
  assert!(h.key(Key::Char('v'), CMD));
  h.frame();
  assert!(!h.app().hover && h.cursor() == Cursor::Arrow, "pasted away");
  h.clipboard().write(&format!("{},{}", START.x, START.y));
  assert!(h.key(Key::Char('v'), CMD));
  assert!(h.app().hover);

  assert!(wheel(&mut h, edge, -20.0));
  h.frame();
  assert!(!h.app().hover && h.cursor() == Cursor::Arrow, "shrunk away by the wheel");
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
  let copied = h.clipboard().read();
  assert_eq!(copied, Some(format!("{},{}", START.x, START.y)));

  h.clipboard().write(" 200, 120 ");
  assert!(h.key(Key::Char('v'), CMD));
  assert_eq!(h.app().pos, Point::new(200.0, 120.0));
  check_square(&h.render(), h.app(), BLUE);

  h.clipboard().write("hello");
  assert!(!h.key(Key::Char('v'), CMD));
  assert_eq!(h.app().pos, Point::new(200.0, 120.0));

  // A binding's command reaches the app as a command, not as the key.
  let k = us_key(Key::Char('v'), CMD);
  h.clipboard().write("10,50");
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
