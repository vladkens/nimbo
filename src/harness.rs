// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The headless harness: runs an app with no window on the same renderer, offscreen, at a
//! given size and scale, on virtual time. Scripted events drive it; it renders PNG shots
//! ([crate::png]).

use std::path::Path;
use std::time::Duration;

use crate::app::{App, Clock, Event, Mods, MouseButton, Runtime};
use crate::cpu::{self, Pixels};
use crate::keys::{self, Key, KeyEvent};
use crate::scene::{Point, Size};

/// A display refresh at 120 Hz, the step of [Harness::wait].
pub const TICK: Duration = Duration::from_nanos(8_333_333);

pub struct Harness<A> {
  pub rt: Runtime<A>,
  pub cpu: cpu::Renderer,
}

impl<A: App> Harness<A> {
  pub fn new(app: A, size: Size, scale: f32) -> Self {
    let clock = Clock::Virtual(Duration::ZERO);
    let rt = Runtime::new(app, size, scale, clock);
    Self { rt, cpu: cpu::Renderer::new() }
  }

  pub fn app(&self) -> &A {
    &self.rt.app
  }

  /// The app to change directly; the next [Self::frame] builds a frame.
  pub fn app_mut(&mut self) -> &mut A {
    self.rt.redraw();
    &mut self.rt.app
  }

  // MARK: Input

  pub fn event(&mut self, ev: Event) -> bool {
    self.rt.event(ev)
  }

  pub fn move_to(&mut self, x: f32, y: f32) -> bool {
    self.event(Event::MouseMove { pos: Point::new(x, y), mods: Mods::NONE })
  }

  /// A left click at (x, y): move, press, release. Returns whether any of them was used.
  pub fn click(&mut self, x: f32, y: f32) -> bool {
    let moved = self.move_to(x, y);
    let down = self.press(x, y);
    let up = self.release(x, y);
    moved | down | up
  }

  /// Presses the left button at (x, y).
  pub fn press(&mut self, x: f32, y: f32) -> bool {
    self.press_with(x, y, 1, Mods::NONE)
  }

  /// Presses the left button at (x, y) as the `clicks`-th click in a row, with `mods` held.
  pub fn press_with(&mut self, x: f32, y: f32, clicks: u32, mods: Mods) -> bool {
    let (pos, button) = (Point::new(x, y), MouseButton::Left);
    self.event(Event::MouseDown { pos, button, clicks, mods })
  }

  /// Releases the left button at (x, y).
  pub fn release(&mut self, x: f32, y: f32) -> bool {
    self.event(Event::MouseUp {
      pos: Point::new(x, y),
      button: MouseButton::Left,
      mods: Mods::NONE,
    })
  }

  /// Presses and releases `key` with `mods` as on a US keyboard (Shift-1 is `'!'`, Shift-A
  /// `'A'`); returns whether the press was used. Unused, it goes through text input as in a
  /// window: [Self::key_event].
  pub fn key(&mut self, key: Key, mods: Mods) -> bool {
    self.key_event(us_key(key, mods))
  }

  /// Presses and releases a key as a window does: the key goes through bindings to the focused
  /// element and the app; unused, AppKit's text input would make it text or a command, here
  /// [keys::standard] does. Returns whether any of it was used.
  pub fn key_event(&mut self, k: KeyEvent) -> bool {
    let mut used = self.event(Event::KeyDown(k));
    if !used {
      used = match (k.text, keys::standard(&k)) {
        (Some(c), _) => self.event(Event::Text(c.to_string())),
        (None, Some(cmd)) => self.event(Event::Command(cmd)),
        (None, None) => false,
      };
    }

    self.event(Event::KeyUp(KeyEvent { repeat: false, ..k }));
    used
  }

  /// The pointer leaves the view.
  pub fn leave(&mut self) -> bool {
    self.event(Event::MouseExit)
  }

  /// Moves virtual time on without building frames.
  pub fn advance(&mut self, dt: Duration) {
    self.rt.advance(dt);
  }

  /// Lets `dt` of virtual time pass as a window would: a display refresh every [TICK], each
  /// building a frame when one is needed (an animation). Returns the frames built.
  pub fn wait(&mut self, dt: Duration) -> usize {
    let (end, mut frames) = (self.rt.now() + dt, 0);
    while self.rt.now() < end {
      self.advance(TICK.min(end - self.rt.now()));
      frames += self.frame() as usize;
    }

    frames
  }

  // MARK: Output

  /// Builds a frame if anything asked for one; returns whether it did.
  pub fn frame(&mut self) -> bool {
    if !self.rt.needs_frame() {
      return false;
    }

    self.rt.frame();
    self.cpu.sync(&mut self.rt.images);
    true
  }

  /// Renders the current state offscreen and reads the pixels back.
  pub fn render(&mut self) -> Image {
    self.frame();
    let (size, scale) = (self.rt.size, self.rt.scale);
    let pixels = self.cpu.snapshot(&self.rt.scene, size, scale);
    Image { scale, pixels }
  }

  pub fn shot(&mut self, path: &Path) -> Result<(), String> {
    self.render().save_png(path)
  }
}

// MARK: US keyboard

/// The key code typing `c` on a US keyboard and whether it takes Shift.
fn us_char(c: char) -> Option<(u16, bool)> {
  if c == '\0' {
    return None;
  }

  let at = |s: &str| s.chars().position(|k| k == c).map(|i| i as u16);
  at(keys::US).map(|i| (i, false)).or_else(|| at(keys::US_SHIFT).map(|i| (i, true)))
}

/// The event AppKit reports for `key` with `mods` on a US keyboard. A character may be given
/// shifted or not: `Char('1')` or `Char('!')` with Shift is `'!'`.
pub fn us_key(key: Key, mods: Mods) -> KeyEvent {
  let (key, code, mods) = match key {
    Key::Char(c) => match us_char(c) {
      Some((code, shifted)) => {
        let shift = mods.shift || shifted;
        let c = keys::us_char(code, shift).unwrap_or(c);
        (Key::Char(c), code, Mods { shift, ..mods })
      }
      None => (key, 0xffff, mods),
    },
    k => (k, k.code().unwrap_or(0xffff), mods),
  };

  let text = match key {
    Key::Char(c) if !(mods.cmd || mods.ctrl || mods.alt) => Some(c),
    _ => None,
  };

  KeyEvent { key, text, code, mods, repeat: false }
}

/// Offscreen frame pixels with their backing scale.
pub struct Image {
  pub scale: f32,
  pub pixels: Pixels,
}

impl Image {
  /// RGBA at a point (in points, like events).
  pub fn pixel(&self, x: f32, y: f32) -> [u8; 4] {
    let px = ((x * self.scale) as usize).min(self.pixels.w - 1);
    let py = ((y * self.scale) as usize).min(self.pixels.h - 1);
    let i = (py * self.pixels.w + px) * 4;
    let [b, g, r, a] = self.pixels.data[i..i + 4].try_into().unwrap();
    [r, g, b, a]
  }

  /// Writes a PNG, tagged sRGB.
  pub fn save_png(&self, path: &Path) -> Result<(), String> {
    crate::png::write(path, &self.pixels)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn us_keyboard() {
    let ev = us_key(Key::Char('1'), Mods::SHIFT);
    assert_eq!((ev.key, ev.text, ev.code, ev.mods), (Key::Char('!'), Some('!'), 18, Mods::SHIFT));
    let ev = us_key(Key::Char('A'), Mods::NONE);
    assert_eq!((ev.key, ev.text, ev.code, ev.mods.shift), (Key::Char('A'), Some('A'), 0, true));
    let ev = us_key(Key::Char('z'), Mods::CMD);
    assert_eq!((ev.key, ev.text, ev.code), (Key::Char('z'), None, 6));
    assert_eq!(us_key(Key::Char(' '), Mods::NONE).code, 49);
    assert_eq!(us_key(Key::Tab, Mods::SHIFT).code, 48);
    assert_eq!(us_char('é'), None);
  }
}
