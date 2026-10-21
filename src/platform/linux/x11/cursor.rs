// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The window's cursors: the theme's (x11rb's loader: `XCURSOR_THEME` or `Xcursor.theme`, the
//! size from `XCURSOR_SIZE`, `Xcursor.size` or `Xft.dpi`) by their CSS names or the legacy
//! ones, else the core cursor font's glyphs; each made once, on first use. The theme `core` is
//! the font's glyphs, made here: x11rb 0.14's loader takes a cursor's place in `cursorfont.h`'s
//! list for its glyph, half the glyph's number (`crosshair` 17, not 34), so it draws another.

use x11rb::NONE;
use x11rb::connection::Connection;
use x11rb::cursor::Handle as CursorTheme;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::xproto::{ConnectionExt as _, FontWrapper};
use x11rb::resource_manager::Database;
use x11rb::rust_connection::RustConnection;

use crate::ui::Cursor;

/// Cursors made on first use: the theme's, else the cursor font's.
pub struct Cursors {
  theme: Option<CursorTheme>,
  /// The theme is `core`: the font's arrow too, not the parent's.
  core: bool,
  made: Vec<(Cursor, u32)>,
}

impl Cursors {
  /// Cursors from the theme's loader, unless the theme the resources `db` (or `XCURSOR_THEME`)
  /// name is `core`: the font's then.
  pub fn new(theme: Option<CursorTheme>, db: &Database) -> Self {
    let env = std::env::var("XCURSOR_THEME").ok();
    let core = core_theme(env.as_deref(), db);
    Self { theme: theme.filter(|_| !core), core, made: Vec::new() }
  }

  /// The theme's loader is set up (none when its RENDER queries failed: the font's alone).
  pub fn themed(&self) -> bool {
    self.theme.is_some()
  }

  /// The X cursor for ours: the theme's by its CSS name or the legacy one, else the cursor
  /// font's glyph; the arrow is the parent's (the desktop's) when the theme can't load.
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
      (None, Cursor::Inherit | Cursor::Arrow) if !self.core => NONE,
      (None, _) => font_cursor(x, glyph),
    };

    self.made.push((c, id));
    id
  }
}

/// Whether the cursor theme is `core`, the cursor font's glyphs: `XCURSOR_THEME`, else
/// `Xcursor.theme`, as x11rb's loader picks it.
fn core_theme(env: Option<&str>, db: &Database) -> bool {
  env.or_else(|| db.get_string("Xcursor.theme", "")).is_some_and(|t| t.trim() == "core")
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_core_theme_is_the_fonts() {
    let db = Database::new_from_data(b"Xcursor.theme: core\n");
    assert!(core_theme(None, &db));
    assert!(!core_theme(Some("breeze_cursors"), &db), "the variable first");
    assert!(core_theme(Some("core"), &Database::new_from_data(b"")));
    assert!(!core_theme(None, &Database::new_from_data(b"Xcursor.theme: Adwaita\n")));
  }
}
