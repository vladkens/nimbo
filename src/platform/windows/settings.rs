// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Light or dark and the accent colour: Settings › Personalization › Colors' app mode, the
//! registry's `AppsUseLightTheme` (HKCU, 0 dark; missing before Windows 10 1809: light), and the
//! accent, `AccentColorMenu` (0xAABBGGRR): the Settings colour, where `DwmGetColorizationColor`
//! is the title bar's blend (0x006FC4 for the default 0x0078D4, measured on Windows 11 24H2).
//! Both change with `WM_SETTINGCHANGE` "ImmersiveColorSet" (the accent also with
//! `WM_DWMCOLORIZATIONCOLORCHANGED`); the window re-reads them then. The
//! title bar is drawn dark by DWM on request (`DWMWA_USE_IMMERSIVE_DARK_MODE`), as a Win32 app
//! gets a light one otherwise:
//! [Support Dark and Light themes in Win32 apps](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-windows-themes),
//! [DWMWINDOWATTRIBUTE](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute)
//! (as of 2026-10).

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
  WM_DWMCOLORIZATIONCOLORCHANGED, WM_SETTINGCHANGE,
};
use windows_sys::core::BOOL;

use super::wide;
use crate::app::Appearance;
use crate::scene::rgb;

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const ACCENT: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent";

/// The dark title bar's attribute before Windows 10 20H1 (build 19041): undocumented, as 20 is
/// documented from Windows 11 only, though Windows 10 takes it from 20H1 on.
const DARK_MODE_BEFORE_20H1: u32 = 19;

/// The OS's light or dark app mode and its accent colour now.
pub fn appearance() -> Appearance {
  read(dword(PERSONALIZE, "AppsUseLightTheme"), dword(ACCENT, "AccentColorMenu"))
}

/// The appearance from `AppsUseLightTheme` (0 dark, else light; none, as before 1809, light) and
/// `AccentColorMenu` (0xAABBGGRR; none: no accent).
fn read(apps_light: Option<u32>, accent: Option<u32>) -> Appearance {
  let accent = accent.map(|c| rgb((c & 0xff) << 16 | (c & 0xff00) | (c >> 16 & 0xff)));
  Appearance { dark: apps_light == Some(0), accent }
}

/// A DWORD value under HKCU; `None` when it or its key is missing.
pub(super) fn dword(key: &str, name: &str) -> Option<u32> {
  let (key, name) = (wide(key), wide(name));
  let (mut v, mut size) = (0u32, size_of::<u32>() as u32);
  let p = (&raw mut v).cast();
  let err = unsafe {
    RegGetValueW(
      HKEY_CURRENT_USER,
      key.as_ptr(),
      name.as_ptr(),
      RRF_RT_REG_DWORD,
      null_mut(),
      p,
      &mut size,
    )
  };

  (err == 0).then_some(v)
}

/// Whether a message says the app mode or accent may have changed: `WM_SETTINGCHANGE` whose
/// `lParam` names "ImmersiveColorSet", or `WM_DWMCOLORIZATIONCOLORCHANGED`.
pub(super) fn changed(msg: u32, lp: LPARAM) -> bool {
  match msg {
    WM_DWMCOLORIZATIONCOLORCHANGED => true,
    WM_SETTINGCHANGE if lp != 0 => {
      let p = lp as *const u16;
      // SAFETY: lParam is a NUL-terminated string, read up to the first unit that differs, so
      // never past its NUL.
      wide("ImmersiveColorSet").into_iter().enumerate().all(|(i, c)| unsafe { *p.add(i) } == c)
    }
    _ => false,
  }
}

/// Asks DWM for a dark or a light title bar; it redraws the frame itself.
pub(super) fn title_bar(hwnd: HWND, dark: bool) {
  let on = BOOL::from(dark);
  let set = |attr: u32| unsafe {
    DwmSetWindowAttribute(hwnd, attr, (&raw const on).cast(), size_of::<BOOL>() as u32)
  };

  if set(DWMWA_USE_IMMERSIVE_DARK_MODE as u32) < 0 {
    set(DARK_MODE_BEFORE_20H1);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn apps_use_light_theme_maps_to_dark() {
    assert!(read(Some(0), None).dark, "0 is dark");
    assert!(!read(Some(1), None).dark, "1 is light");
    assert!(!read(None, None).dark, "missing (before 1809) is light");
    assert_eq!(read(Some(1), None), Appearance::LIGHT);
  }

  #[test]
  fn the_accent_reads_as_rgb() {
    let a = read(Some(0), Some(0xff_d4_78_00));
    assert_eq!(a, Appearance { dark: true, accent: Some(rgb(0x0078d4)) });
  }

  #[test]
  fn only_the_color_set_change_counts() {
    let set = wide("ImmersiveColorSet");
    let other = wide("ImmersiveColorSetX");
    let short = wide("Immersive");
    assert!(changed(WM_SETTINGCHANGE, set.as_ptr() as LPARAM));
    assert!(!changed(WM_SETTINGCHANGE, other.as_ptr() as LPARAM));
    assert!(!changed(WM_SETTINGCHANGE, short.as_ptr() as LPARAM));
    assert!(!changed(WM_SETTINGCHANGE, 0));
    assert!(changed(WM_DWMCOLORIZATIONCOLORCHANGED, 0));
  }

  /// A DWORD reads back from a key of the test's own, deleted after; a missing value or key
  /// reads as none. The system's own values may be missing on a valid profile.
  #[test]
  fn the_registry_is_read() {
    use windows_sys::Win32::System::Registry::{REG_DWORD, RegDeleteKeyW, RegSetKeyValueW};

    let key = format!(r"Software\nimbo-test-{}", std::process::id());
    let (k, name, v) = (wide(&key), wide("Value"), 7u32);
    let p = (&raw const v).cast();
    let set =
      unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), name.as_ptr(), REG_DWORD, p, 4) };
    let (value, missing) = (dword(&key, "Value"), dword(&key, "Missing"));
    unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, k.as_ptr()) };

    assert_eq!(set, 0);
    assert_eq!((value, missing), (Some(7), None));
    assert_eq!(dword(&key, "Value"), None, "the key is gone");
  }
}
