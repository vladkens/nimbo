// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Light or dark and the accent colour: the XDG settings portal's `org.freedesktop.appearance`
//! (`color-scheme`: 1 dark, 2 light, 0 no preference; `accent-color`, sRGB in 0..1, out of
//! range when unset) read over GIO's D-Bus, its changes by the portal's `SettingChanged`
//! ([Settings portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Settings.html),
//! as of 2026-10). Without a portal or a preference, GtkSettings: its dark preference (Plasma
//! sets it with its dark colours, its theme staying `Breeze`) or a dark theme's name
//! (`Adwaita-dark`, `Breeze-Dark`, from XSETTINGS or GSettings), followed as they change.

use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use super::dbus::{self, Gv, V};
use super::dl::fns;
use super::{Fp, P, S, connect, each, lib};
use crate::app::{Appearance, Event};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const NAMESPACE: &str = "org.freedesktop.appearance";
/// How long the first read waits for the portal (started on demand by the bus).
const TIMEOUT_MS: c_int = 250;

fns!(Gtk {
  fn g_free(p: P);
  fn g_timeout_add(ms: c_uint, f: Fp, data: P) -> c_uint;
  fn gtk_settings_get_default() -> P;
});

/// GTK's settings, and `g_object_get` (property names and where each value goes, then null),
/// which `fns!` can't declare.
struct Api {
  f: Gtk,
  g_object_get: unsafe extern "C" fn(P, S, ...),
}

fn api() -> Option<&'static Api> {
  static API: OnceLock<Option<Api>> = OnceLock::new();
  API
    .get_or_init(|| {
      let lib = lib()?;
      // SAFETY: gobject.h's declaration.
      let g_object_get = unsafe { lib.sym("g_object_get\0") }?;
      Some(Api { f: Gtk::load(&lib)?, g_object_get })
    })
    .as_ref()
}

/// The OS's light or dark setting and accent now: the portal's, else GtkSettings' (light
/// before GTK opened a display).
pub fn appearance() -> Appearance {
  let Some(a) = api() else { return Appearance::LIGHT };
  let (dark, accent) = portal();
  Appearance { dark: dark.unwrap_or_else(|| gtk_dark(a)), accent }
}

/// Follows the portal's and GtkSettings' changes, each sent as [Event::Appearance] (the
/// runtime drops repeats).
pub fn watch() {
  let Some(a) = api() else { return };
  // SAFETY: GTK's calls; the callbacks have the signals' signatures.
  unsafe {
    let settings = (a.f.gtk_settings_get_default)();
    if !settings.is_null() {
      connect(settings, c"notify::gtk-theme-name", on_changed as Fp, 0);
      connect(settings, c"notify::gtk-application-prefer-dark-theme", on_changed as Fp, 0);
    }
  }

  if let Ok(bus) = dbus::session() {
    let cb = on_portal as Fp;
    dbus::subscribe(bus, PORTAL, SETTINGS, "SettingChanged", Some(PATH), cb, (null_mut(), null()));
  }
}

unsafe extern "C" fn on_changed(_: P, _: P, _: P) {
  unsafe { deliver(null_mut()) };
}

/// `SettingChanged(namespace, key, value)`: one in the appearance namespace.
unsafe extern "C" fn on_portal(_: P, _: S, _: S, _: S, _: S, args: P, _: P) {
  if Gv::borrowed(args).child(0).str() == NAMESPACE {
    unsafe { deliver(null_mut()) };
  }
}

/// Sends the appearance to every window now; while one is busy (a paste waits), again in a
/// moment.
unsafe extern "C" fn deliver(_: P) -> c_int {
  let look = appearance();
  let missed = each(|w| {
    w.send(Event::Appearance(look));
  });

  if missed {
    let f = &api().expect("loaded before its callbacks").f;
    unsafe { (f.g_timeout_add)(100, deliver as Fp, null_mut()) };
  }

  0
}

/// The portal's dark (`None` for no preference) and accent, by `ReadAll` of the appearance
/// namespace (`(a{sa{sv}})`: the namespaces, each a dictionary of its keys); nothing without a
/// portal.
fn portal() -> (Option<bool>, Option<[f32; 4]>) {
  let args = V::Tuple(vec![V::Array("s", vec![V::s(NAMESPACE)])]);
  let read = |bus| dbus::call(bus, PORTAL, PATH, SETTINGS, "ReadAll", args, TIMEOUT_MS);
  let Some(ns) = dbus::session().and_then(read).ok().and_then(|r| r.child(0).get(NAMESPACE)) else {
    return (None, None);
  };

  let scheme = ns.get("color-scheme").and_then(|v| dark_scheme(v.u32()));
  let accent = ns.get("accent-color").filter(|v| v.ty() == "(ddd)");
  (scheme, accent.and_then(|v| accent_rgb([0, 1, 2].map(|i| v.child(i).f64()))))
}

/// 1 is dark, 2 light, the rest no preference.
fn dark_scheme(n: u32) -> Option<bool> {
  match n {
    1 => Some(true),
    2 => Some(false),
    _ => None,
  }
}

/// An accent's channels in 0..1, as opaque sRGB; out of range is no accent.
fn accent_rgb(c: [f64; 3]) -> Option<[f32; 4]> {
  let ok = c.iter().all(|v| (0.0..=1.0).contains(v));
  ok.then(|| [c[0] as f32, c[1] as f32, c[2] as f32, 1.0])
}

/// GtkSettings' dark preference, or a dark theme's name.
fn gtk_dark(a: &Api) -> bool {
  let settings = unsafe { (a.f.gtk_settings_get_default)() };
  if settings.is_null() {
    return false;
  }

  let (mut dark, mut theme): (c_int, *mut c_char) = (0, null_mut());
  let (d, t) = (c"gtk-application-prefer-dark-theme", c"gtk-theme-name");
  // SAFETY: an int and a string property, then the null that ends the list; the string is ours.
  let name = unsafe {
    let end = null::<c_char>();
    (a.g_object_get)(settings, d.as_ptr(), &mut dark, t.as_ptr(), &mut theme, end);
    let name = match theme.is_null() {
      true => String::new(),
      false => CStr::from_ptr(theme).to_string_lossy().into_owned(),
    };

    (a.f.g_free)(theme.cast::<c_void>());
    name
  };

  dark != 0 || dark_theme(&name)
}

fn dark_theme(name: &str) -> bool {
  name.to_lowercase().contains("dark")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_portal_and_themes_say_dark() {
    assert_eq!(dark_scheme(1), Some(true));
    assert_eq!(dark_scheme(2), Some(false));
    assert_eq!(dark_scheme(0), None);
    assert!(dark_theme("Adwaita-dark") && dark_theme("Breeze-Dark"));
    assert!(!dark_theme("Breeze") && !dark_theme("Yaru"));
  }

  #[test]
  fn accents_in_range_only() {
    assert_eq!(accent_rgb([0.2, 0.4, 1.0]), Some([0.2, 0.4, 1.0, 1.0]));
    assert_eq!(accent_rgb([-1.0, -1.0, -1.0]), None, "unset");
    assert_eq!(accent_rgb([0.5, 2.0, 0.0]), None);
  }
}
