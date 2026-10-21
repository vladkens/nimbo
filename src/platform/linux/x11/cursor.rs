// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The window's cursors: the theme's (libxcb-cursor) by their CSS names or the legacy ones, else
//! the core cursor font's glyphs; each made once, on first use.

use std::ffi::CStr;

use super::xcb::{CursorContext, X};
use crate::ui::Cursor;

/// Cursors made on first use: the theme's, else the cursor font's.
pub struct Cursors {
  theme: Option<CursorContext>,
  font: u32,
  made: Vec<(Cursor, u32)>,
}

impl Cursors {
  pub fn new(theme: Option<CursorContext>) -> Self {
    Self { theme, font: 0, made: Vec::new() }
  }

  /// A theme can be loaded from (libxcb-cursor is there).
  pub fn themed(&self) -> bool {
    self.theme.is_some()
  }

  /// The X cursor for ours: the theme's by its CSS name or the legacy one, else the cursor
  /// font's glyph; the arrow is the parent's (the desktop's) without a theme.
  pub fn get(&mut self, x: &X, c: Cursor) -> u32 {
    if let Some(&(_, id)) = self.made.iter().find(|m| m.0 == c) {
      return id;
    }

    let (names, glyph): (&[&CStr], u16) = match c {
      Cursor::Inherit | Cursor::Arrow => (&[c"default", c"left_ptr"], 68),
      Cursor::Text => (&[c"text", c"xterm"], 152),
      Cursor::Pointer => (&[c"pointer", c"hand2"], 60),
      Cursor::ResizeX => (&[c"col-resize", c"sb_h_double_arrow"], 108),
      Cursor::ResizeY => (&[c"row-resize", c"sb_v_double_arrow"], 116),
      Cursor::Crosshair => (&[c"crosshair"], 34),
      Cursor::Grab => (&[c"grab", c"openhand", c"hand1"], 58),
      Cursor::Grabbing => (&[c"grabbing", c"closedhand", c"fleur"], 52),
      Cursor::NotAllowed => (&[c"not-allowed", c"crossed_circle"], 0),
    };

    let id = match (&self.theme, c) {
      (Some(t), _) => t.load(names).unwrap_or_else(|| x.font_cursor(&mut self.font, glyph)),
      (None, Cursor::Inherit | Cursor::Arrow) => 0,
      (None, _) => x.font_cursor(&mut self.font, glyph),
    };

    self.made.push((c, id));
    id
  }
}
