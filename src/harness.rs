// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The headless harness ([crate::Harness]): runs an app with no window on the same renderer,
//! offscreen, at a given size and scale, on virtual time and an in-memory clipboard. Scripted
//! input drives it; it renders PNG shots ([crate::Png]). Here, what a test reads from it and the
//! input it makes for it.

pub(crate) mod run;

use std::path::Path;

use crate::cpu::Pixels;
use crate::keys::{self, Key, KeyEvent};
use crate::scene::{Rect, Size};
use crate::ui::{Id, Role};
use crate::{Code, Error, Modifiers, TitleBar, png};

/// macOS's title bar over a unified window's content, measured on macOS 27: 32 high with the
/// traffic lights (three of 14, 9 apart) 9 from the top-left corner; with a toolbar
/// ([crate::Ui::toolbar]) 52 high, the lights 19 in, centred on it.
pub fn mac_titlebar(title: &'static str, toolbar: bool) -> TitleBar {
  let (height, at) = if toolbar { (52.0, 19.0) } else { (32.0, 9.0) };
  TitleBar { height, title, buttons: Some(Rect::new(at, at, 60.0, 14.0)) }
}

/// A node of the last frame as assistive technology reads it ([crate::Harness::find]).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Element {
  pub id: Id,
  pub role: Role,
  pub name: String,
  /// In window points, clipped to the scroll areas around it.
  pub rect: Rect,
}

// MARK: US keyboard

/// The event AppKit reports for `key` with `mods` on a US keyboard. A character may be given
/// shifted or not: `Char('1')` or `Char('!')` with Shift is `'!'`. How a test makes a
/// [KeyEvent] for [crate::Harness::key_event] or [crate::Event::KeyDown].
pub fn us_key(key: Key, mods: Modifiers) -> KeyEvent {
  let (key, code, mods) = match key {
    Key::Char(c) => match keys::us_code(c) {
      Some((code, shifted)) => {
        let mods = if shifted { mods | Modifiers::SHIFT } else { mods };
        let c = keys::us_char(code, mods.shift()).unwrap_or(c);
        (Key::Char(c), code, mods)
      }
      None => (key, Code::Unidentified, mods),
    },
    k => (k, k.code().unwrap_or(Code::Unidentified), mods),
  };

  let text = match key {
    Key::Char(c) if (mods - Modifiers::SHIFT).is_empty() => Some(c),
    _ => None,
  };

  KeyEvent { key, text, code, mods, repeat: false }
}

/// Offscreen frame pixels with their backing scale.
#[non_exhaustive]
pub struct Image {
  scale: f32,
  pixels: Pixels,
}

impl Image {
  /// Pixels per point.
  pub fn scale(&self) -> f32 {
    self.scale
  }

  /// The size in points.
  pub fn size(&self) -> Size {
    Size::new(self.pixels.w as f32 / self.scale, self.pixels.h as f32 / self.scale)
  }

  /// RGBA at a point (in points, like events).
  pub fn pixel(&self, x: f32, y: f32) -> [u8; 4] {
    let px = ((x * self.scale) as usize).min(self.pixels.w - 1);
    let py = ((y * self.scale) as usize).min(self.pixels.h - 1);
    let i = (py * self.pixels.w + px) * 4;
    let [b, g, r, a] = self.pixels.data[i..i + 4].try_into().unwrap();
    [r, g, b, a]
  }

  /// Writes a PNG, tagged sRGB.
  pub fn save_png(&self, path: &Path) -> Result<(), Error> {
    png::write(path, &self.pixels).map_err(Error::System)
  }
}
