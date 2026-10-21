// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The desktop's settings: the resources (`RESOURCE_MANAGER` on the root, as `xrdb` sets them)
//! for the scale from `Xft.dpi` and the cursor theme, light or dark from XSETTINGS'
//! `Net/ThemeName`.

use x11rb::NONE;
use x11rb::resource_manager::{self, Database};
use x11rb::rust_connection::RustConnection;

use super::property;
use crate::app::Appearance;

/// The root window's resources; none when it has none or the request fails.
pub(super) fn resources(x: &RustConnection) -> Database {
  let db = resource_manager::new_from_resource_manager(x).ok().flatten();
  db.unwrap_or_else(|| Database::new_from_data(b""))
}

/// `Xft.dpi` in the resources over 96, at least 1.
pub(super) fn dpi_scale(db: &Database) -> f32 {
  let dpi = db.get_value::<f32>("Xft.dpi", "").ok().flatten();
  dpi.map_or(1.0, |d| (d / 96.0).max(1.0))
}

/// Dark when the XSETTINGS manager's `Net/ThemeName` is a dark theme (`Adwaita-dark`,
/// `Breeze-Dark`); light without one.
pub(super) fn appearance(x: &RustConnection, owner: u32, prop: u32) -> Appearance {
  let data = match owner {
    NONE => None,
    w => property(x, w, prop, prop, false),
  };

  let theme = data.and_then(|d| xsettings_string(&d.value, "Net/ThemeName"));
  match theme {
    Some(t) if t.to_lowercase().contains("dark") => Appearance::DARK,
    _ => Appearance::LIGHT,
  }
}

/// A string setting in `_XSETTINGS_SETTINGS` (the XSETTINGS spec: a byte order, a serial, the
/// count, then each setting's type, name, serial and value, 4-byte aligned).
fn xsettings_string(d: &[u8], name: &str) -> Option<String> {
  let big = *d.first()? == 1;
  let u16_at = |i: usize| -> Option<u16> {
    let b = d.get(i..i + 2)?;
    Some(if big { u16::from_be_bytes([b[0], b[1]]) } else { u16::from_le_bytes([b[0], b[1]]) })
  };
  let u32_at = |i: usize| -> Option<u32> {
    let b: [u8; 4] = d.get(i..i + 4)?.try_into().ok()?;
    Some(if big { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
  };
  let pad = |n: usize| (n + 3) & !3;

  let count = u32_at(8)?;
  let mut i = 12;
  for _ in 0..count {
    let ty = *d.get(i)?;
    let len = u16_at(i + 2)? as usize;
    let key = d.get(i + 4..i + 4 + len)?;
    i += 4 + pad(len) + 4;
    match ty {
      // Integer.
      0 => i += 4,
      // String.
      1 => {
        let n = u32_at(i)? as usize;
        let value = d.get(i + 4..i + 4 + n)?;
        if key == name.as_bytes() {
          return String::from_utf8(value.to_vec()).ok();
        }

        i += 4 + pad(n);
      }
      // Colour.
      2 => i += 8,
      _ => return None,
    }
  }

  None
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn resources_give_the_dpi() {
    let db = Database::new_from_data(b"Xcursor.theme:\tbreeze_cursors\nXft.dpi:\t144\n");
    assert_eq!(dpi_scale(&db), 1.5);
    assert_eq!(dpi_scale(&Database::new_from_data(b"Xft.dpi: 72\n")), 1.0, "at least 1");
    assert_eq!(dpi_scale(&Database::new_from_data(b"")), 1.0);
  }

  #[test]
  fn xsettings_strings_are_found() {
    // Little-endian: an integer `Net/DoubleClickTime`, then the string `Net/ThemeName`.
    let mut d = vec![0, 0, 0, 0];
    d.extend(7u32.to_le_bytes());
    d.extend(2u32.to_le_bytes());
    let name = b"Net/DoubleClickTime";
    d.extend([0, 0]);
    d.extend((name.len() as u16).to_le_bytes());
    d.extend(name);
    d.push(0);
    d.extend(0u32.to_le_bytes());
    d.extend(400i32.to_le_bytes());
    let (name, value) = (b"Net/ThemeName", b"Breeze-Dark");
    d.extend([1, 0]);
    d.extend((name.len() as u16).to_le_bytes());
    d.extend(name);
    d.extend([0, 0, 0]);
    d.extend(0u32.to_le_bytes());
    d.extend((value.len() as u32).to_le_bytes());
    d.extend(value);
    d.push(0);
    assert_eq!(xsettings_string(&d, "Net/ThemeName").as_deref(), Some("Breeze-Dark"));
    assert_eq!(xsettings_string(&d, "Gtk/FontName"), None);
  }
}
