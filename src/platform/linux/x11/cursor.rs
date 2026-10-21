// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The window's cursors: the theme's (x11rb's loader: `XCURSOR_THEME` or `Xcursor.theme`, the
//! size from `XCURSOR_SIZE`, `Xcursor.size` or `Xft.dpi`) by their CSS names or the legacy
//! ones, else the core cursor font's glyphs; each made once, on first use.

use x11rb::NONE;
use x11rb::connection::Connection;
use x11rb::cursor::Handle as CursorTheme;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::xproto::{ConnectionExt as _, FontWrapper};
use x11rb::rust_connection::RustConnection;

use crate::ui::Cursor;

/// Cursors made on first use: the theme's, else the cursor font's.
pub struct Cursors {
  theme: Option<CursorTheme>,
  made: Vec<(Cursor, u32)>,
}

impl Cursors {
  pub fn new(theme: Option<CursorTheme>) -> Self {
    Self { theme, made: Vec::new() }
  }

  /// The theme's loader is set up (none when its RENDER queries failed: the font's alone).
  pub fn themed(&self) -> bool {
    self.theme.is_some()
  }

  /// The X cursor for ours: the theme's by its CSS name or the legacy one, else the cursor
  /// font's glyph; the arrow is the parent's (the desktop's) without a theme.
  pub fn get(&mut self, x: &RustConnection, c: Cursor) -> u32 {
    if let Some(&(_, id)) = self.made.iter().find(|m| m.0 == c) {
      return id;
    }

    let (names, glyph): (&[&str], u16) = match c {
      Cursor::Inherit | Cursor::Arrow => (&["default", "left_ptr"], 68),
      Cursor::Text => (&["text", "xterm"], 152),
      Cursor::Pointer => (&["pointer", "hand2"], 60),
      Cursor::ResizeX => (&["col-resize", "sb_h_double_arrow"], 108),
      Cursor::ResizeY => (&["row-resize", "sb_v_double_arrow"], 116),
      Cursor::Crosshair => (&["crosshair"], 34),
      Cursor::Grab => (&["grab", "openhand", "hand1"], 58),
      Cursor::Grabbing => (&["grabbing", "closedhand", "fleur"], 52),
      Cursor::NotAllowed => (&["not-allowed", "crossed_circle"], 0),
    };

    let themed = |t: &CursorTheme| {
      names.iter().find_map(|n| t.load_cursor(x, n).ok().filter(|&id| id != NONE))
    };
    let id = match (&self.theme, c) {
      (Some(t), _) => themed(t).unwrap_or_else(|| font_cursor(x, glyph)),
      (None, Cursor::Inherit | Cursor::Arrow) => NONE,
      (None, _) => font_cursor(x, glyph),
    };

    self.made.push((c, id));
    id
  }
}

/// A cursor of the core cursor font (`<X11/cursorfont.h>`), black on white; none when it fails.
fn font_cursor(x: &RustConnection, glyph: u16) -> u32 {
  let make = || -> Result<u32, ReplyOrIdError> {
    let (font, id) = (FontWrapper::open_font(x, b"cursor")?, x.generate_id()?);
    let (f, w) = (font.font(), u16::MAX);
    x.create_glyph_cursor(id, f, f, glyph, glyph + 1, 0, 0, 0, w, w, w)?;
    Ok(id)
  };
  make().unwrap_or(NONE)
}
