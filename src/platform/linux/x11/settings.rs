// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The desktop's settings: the scale from `Xft.dpi` in the resources, light or dark from
//! XSETTINGS' `Net/ThemeName`.

use super::xcb::*;
use crate::app::Appearance;

/// `Xft.dpi` in the root window's resources over 96, at least 1.
pub(super) fn dpi_scale(x: &X, root: u32) -> f32 {
  let res = x.property(root, ATOM_RESOURCE_MANAGER, ATOM_STRING, false);
  let text = res.map(|p| String::from_utf8_lossy(&p.data).into_owned()).unwrap_or_default();
  resource(&text, "Xft.dpi")
    .and_then(|v| v.parse::<f32>().ok())
    .map_or(1.0, |d| (d / 96.0).max(1.0))
}

/// A resource's value in `xrdb` text (`name:\tvalue` lines).
fn resource<'a>(text: &'a str, name: &str) -> Option<&'a str> {
  text.lines().find_map(|l| {
    let (k, v) = l.split_once(':')?;
    (k.trim() == name).then(|| v.trim())
  })
}

/// Dark when the XSETTINGS manager's `Net/ThemeName` is a dark theme (`Adwaita-dark`,
/// `Breeze-Dark`); light without one.
pub(super) fn appearance(x: &X, owner: u32, prop: u32) -> Appearance {
  let data = match owner {
    0 => None,
    w => x.property(w, prop, prop, false).map(|p| p.data),
  };

  let theme = data.and_then(|d| xsettings_string(&d, "Net/ThemeName"));
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
    let text = "Xcursor.theme:\tbreeze_cursors\nXft.dpi:\t144\nXft.hinting:\t1\n";
    assert_eq!(resource(text, "Xft.dpi"), Some("144"));
    assert_eq!(resource(text, "Xft.rgba"), None);
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
