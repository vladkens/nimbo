// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Input without text: a square dragged with the mouse or moved [STEP] points by the arrow keys,
//! the wheel grows or shrinks it, the cursor an open hand over it and a closed one while
//! dragging. Cmd-C (Ctrl-C on Windows and Linux) copies its position as `x,y`, Cmd-V moves it to
//! a position pasted as `x,y`; it turns amber while Shift is held. A bar along the top shows
//! that the window has the keyboard focus.
//!
//! `cargo run --release --example input` opens it in a window; `-- --shot input.png` renders it
//! without one.

use nimbo::{
  App, Binding, Color, Command, Config, Cursor, Cx, Event, Frame, Key, MouseButton, Point, Quad,
  Rect, Size, bind, mix, rgb,
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
const KEYS: &[Binding] = &[bind("cmd-c", Command::Copy), bind("cmd-v", Command::Paste)];

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

  /// The cursor for where the pointer is: a closed hand while dragging, an open one over the
  /// square.
  fn cursor(&self) -> Cursor {
    match (self.grab, self.hover) {
      (Some(_), _) => Cursor::Grabbing,
      (None, true) => Cursor::Grab,
      (None, false) => Cursor::Arrow,
    }
  }

  fn pointer(&mut self, cx: &mut Cx, ev: &Event) -> bool {
    let used = match *ev {
      Event::MouseMove { pos, .. } => {
        let was = self.hover;
        self.hover = self.square().contains(pos);
        match self.grab {
          Some(g) => {
            self.move_to(Point::new(pos.x - g.x, pos.y - g.y), cx.size);
            true
          }
          None => self.hover != was,
        }
      }
      Event::MouseDown { pos, button: MouseButton::Left, .. } if self.square().contains(pos) => {
        self.grab = Some(Point::new(pos.x - self.pos.x, pos.y - self.pos.y));
        true
      }
      Event::MouseUp { pos, button: MouseButton::Left, .. } if self.grab.is_some() => {
        self.grab = None;
        self.hover = self.square().contains(pos);
        true
      }
      Event::MouseExit => std::mem::take(&mut self.hover),
      Event::Scroll { delta, .. } => {
        let side = (self.side + delta.y).clamp(SIDES.0, SIDES.1);
        let grow = side - self.side;
        self.side = side;
        // Grown or shrunk about its centre.
        let corner = Point::new(self.pos.x - grow / 2.0, self.pos.y - grow / 2.0);
        self.move_to(corner, cx.size);
        grow != 0.0
      }
      _ => false,
    };

    cx.set_cursor(self.cursor());
    used
  }
}

impl App for Input {
  fn keymap(&self) -> &[Binding] {
    KEYS
  }

  fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
    match ev {
      Event::KeyDown(k) => {
        let (dx, dy) = match k.key {
          Key::Left => (-STEP, 0.0),
          Key::Right => (STEP, 0.0),
          Key::Up => (0.0, -STEP),
          Key::Down => (0.0, STEP),
          _ => return false,
        };

        self.move_to(Point::new(self.pos.x + dx, self.pos.y + dy), cx.size);
      }
      Event::Command(Command::Copy) => {
        let text = format!("{},{}", self.pos.x.round(), self.pos.y.round());
        cx.clipboard().write(&text);
      }
      Event::Command(Command::Paste) => {
        let Some(text) = cx.clipboard().read() else { return false };
        let Some(p) = parse_point(&text) else { return false };
        self.move_to(p, cx.size);
      }
      Event::Mods(m) => {
        let was = std::mem::replace(&mut self.shift, m.shift);
        return was != m.shift;
      }
      Event::Focus(on) => self.focused = *on,
      Event::Appearance(a) => self.light = !a.dark,
      ev => return self.pointer(cx, ev),
    }

    true
  }

  fn paint(&mut self, f: &mut Frame) {
    f.rect(Rect::new(0.0, 0.0, f.size.w, f.size.h), self.bg(), 0.0);
    if self.focused {
      f.rect(Rect::new(0.0, 0.0, f.size.w, BAR), GREEN, 0.0);
    }

    let sq = self.square();
    let mut quad = Quad::new(sq, self.fill()).radius(8.0);
    if self.hover || self.grab.is_some() {
      quad = quad.border(HOVER_BORDER, mix(self.fill(), WHITE, 0.6));
    }

    f.quad(quad);
  }
}

/// A position written as `x,y`, spaces allowed around each number.
pub fn parse_point(s: &str) -> Option<Point> {
  let (x, y) = s.trim().split_once(',')?;
  Some(Point::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
}

#[allow(dead_code)]
fn main() {
  nimbo::run(Config::new("input", SIZE.w, SIZE.h), Input::default(), |_| {});
}
