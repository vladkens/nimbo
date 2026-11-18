// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Light or dark and the accent colour: the XDG settings portal's `org.freedesktop.appearance`
//! (`color-scheme`: 1 dark, 2 light, 0 no preference; `accent-color`, sRGB in 0..1, out of
//! range when unset) read over GIO's D-Bus, its changes by the portal's `SettingChanged`
//! ([Settings portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Settings.html),
//! as of 2026-10). Without a portal or a preference, GtkSettings: its dark preference (Plasma
//! sets it with its dark colours, its theme staying `Breeze`) or a dark theme's name
//! (`Adwaita-dark`, `Breeze-Dark`, from XSETTINGS or GSettings), followed as they change. The
//! user's languages are gettext's variables.
//!
//! The text scale ([zoom]) is GtkSettings' too, followed the same way.

use std::cell::Cell;
use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use super::dbus::{self, Gv, V};
use super::dl::fns;
use super::{Fp, P, S, connect, each, lib};
use crate::app::{Appearance, ColorScheme, Event};

thread_local! {
  /// [zoom], read when GTK opened its display and as it changes.
  static ZOOM: Cell<f32> = const { Cell::new(1.0) };
}

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
  Appearance { scheme: ColorScheme::of(dark.unwrap_or_else(|| gtk_dark(a))), accent }
}

/// The user's preferred languages, the most preferred first, as tags, as gettext picks them:
/// `LANGUAGE` (a list by `:`), else the first of `LC_ALL`, `LC_MESSAGES` and `LANG` that is set;
/// none in the `C` locale, which turns translations off, `LANGUAGE` too.
pub fn languages() -> Vec<String> {
  let var = |k| std::env::var(k).ok().filter(|v: &String| !v.is_empty());
  let locale = ["LC_ALL", "LC_MESSAGES", "LANG"].into_iter().find_map(var);
  tags(&var("LANGUAGE").unwrap_or_default(), locale.as_deref())
}

/// The tags of a `LANGUAGE` list, or of the locale's name without one; none when the locale is
/// `C` or `POSIX`, or unset. `C` in the list is English, the untranslated text.
fn tags(list: &str, locale: Option<&str>) -> Vec<String> {
  let Some(locale) = locale.map(tag).filter(|l| !matches!(l.as_str(), "" | "C" | "POSIX")) else {
    return Vec::new();
  };

  let english = |t: String| if matches!(t.as_str(), "C" | "POSIX") { "en".into() } else { t };
  let list: Vec<_> = list.split(':').map(tag).filter(|t| !t.is_empty()).map(english).collect();
  if list.is_empty() { vec![locale] } else { list }
}

/// A locale name's tag: `ru_RU.UTF-8` is `ru-RU`, `sr_RS@latin` `sr-Latn-RS` (gettext's script
/// modifiers); other modifiers (`@euro`) are dropped.
fn tag(name: &str) -> String {
  let (name, modifier) = name.split_once('@').unwrap_or((name, ""));
  let name = name.split('.').next().unwrap_or_default();
  let script = match modifier {
    "latin" => "Latn",
    "cyrillic" => "Cyrl",
    _ => "",
  };

  let (lang, region) = name.split_once('_').unwrap_or((name, ""));
  let parts = [lang, script, region].into_iter().filter(|p| !p.is_empty());
  parts.collect::<Vec<_>>().join("-")
}

/// Follows the portal's and GtkSettings' changes, each sent as [Event::Appearance] (the
/// runtime drops repeats).
pub fn watch() {
  let Some(a) = api() else { return };
  ZOOM.set(xft_zoom(a));
  // SAFETY: GTK's calls; the callbacks have the signals' signatures.
  unsafe {
    let settings = (a.f.gtk_settings_get_default)();
    if !settings.is_null() {
      connect(settings, c"notify::gtk-theme-name", on_changed as Fp, 0);
      connect(settings, c"notify::gtk-application-prefer-dark-theme", on_changed as Fp, 0);
      connect(settings, c"notify::gtk-xft-dpi", on_dpi as Fp, 0);
    }
  }

  if let Ok(bus) = dbus::session() {
    let cb = on_portal as Fp;
    dbus::subscribe(bus, PORTAL, SETTINGS, "SettingChanged", Some(PATH), cb, (null_mut(), null()));
  }
}

unsafe extern "C" fn on_changed(_: P, _: P, _: P) {
  super::app::tray::repaint();
  unsafe { deliver(null_mut()) };
}

unsafe extern "C" fn on_dpi(_: P, _: P, _: P) {
  let z = api().map_or(1.0, xft_zoom);
  if ZOOM.replace(z) != z {
    unsafe { refit(null_mut()) };
  }
}

/// Fits every window to the zoom now ([super::Win::fit]); while one is busy, again in a moment
/// (till then it keeps its own).
unsafe extern "C" fn refit(_: P) -> c_int {
  if each(|w| w.fit()) {
    let f = &api().expect("loaded before its callbacks").f;
    unsafe { (f.g_timeout_add)(100, refit as Fp, null_mut()) };
  }

  0
}

/// `SettingChanged(namespace, key, value)`: one in the appearance namespace.
unsafe extern "C" fn on_portal(_: P, _: S, _: S, _: S, _: S, args: P, _: P) {
  if Gv::borrowed(args).child(0).str() == NAMESPACE {
    super::app::tray::repaint();
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

/// How much bigger than its points everything is drawn: the desktop's text scale (GNOME's and
/// Cinnamon's text scaling factor, the DPI Xfce and Plasma on X11 set), as Chromium and Firefox
/// take it, over GDK's whole scale. 1 when the desktop sets none.
pub fn zoom() -> f32 {
  ZOOM.get()
}

/// GtkSettings' `gtk-xft-dpi` as a zoom ([dpi_zoom]).
fn xft_zoom(a: &Api) -> f32 {
  let settings = unsafe { (a.f.gtk_settings_get_default)() };
  if settings.is_null() {
    return 1.0;
  }

  let mut dpi: c_int = -1;
  // SAFETY: an int property, then the null that ends the list.
  unsafe { (a.g_object_get)(settings, c"gtk-xft-dpi".as_ptr(), &mut dpi, null::<c_char>()) };
  let scale = std::env::var("GDK_DPI_SCALE").ok().and_then(|s| s.trim().parse().ok());
  dpi_zoom(dpi, scale.unwrap_or(1.0))
}

/// `gtk-xft-dpi` (1024 × the DPI, GDK's whole scale left out; -1 unset) times `GDK_DPI_SCALE`,
/// as GTK takes its fonts' resolution (`settings_update_resolution`, gtk-3-24), over 96 DPI; to
/// the hundredth within GNOME's 0.5–3 (Xft rounds 1.2 × 96 down to 115.199).
fn dpi_zoom(dpi: c_int, scale: f32) -> f32 {
  let scale = if scale > 0.0 { scale } else { 1.0 };
  match dpi {
    ..=0 => 1.0,
    _ => ((dpi as f32 * scale / (96.0 * 1024.0)).clamp(0.5, 3.0) * 100.0).round() / 100.0,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn languages_from_the_variables() {
    let tags = |list, locale| tags(list, locale).join(" ");
    assert_eq!(tags("ru_RU:de", Some("en_US.UTF-8")), "ru-RU de");
    assert_eq!(tags("C.UTF-8:ru", Some("ru_RU.UTF-8")), "en ru");
    assert_eq!(tags("", Some("sr_RS@latin")), "sr-Latn-RS");
    assert_eq!(tags("sr@latin", Some("de_DE@euro")), "sr-Latn");
    assert_eq!(tags("", Some("uz_UZ.UTF-8@cyrillic")), "uz-Cyrl-UZ");
    assert_eq!(tags("", Some("C.UTF-8")), "");
    assert_eq!(tags("POSIX", None), "");
    assert_eq!(tags("ru", Some("C")), "");
    assert_eq!(tags("ru", None), "");
  }

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

  #[test]
  fn the_text_scale_zooms() {
    assert_eq!(dpi_zoom(-1, 1.0), 1.0, "unset");
    assert_eq!(dpi_zoom(96 * 1024, 1.0), 1.0);
    assert_eq!(dpi_zoom(144 * 1024, 1.0), 1.5);
    assert_eq!(dpi_zoom(117964, 1.0), 1.2, "Cinnamon's 1.2");
    assert_eq!(dpi_zoom(1000 * 1024, 1.0), 3.0);
    assert_eq!(dpi_zoom(192 * 1024, 0.5), 1.0, "GDK_DPI_SCALE");
    assert_eq!(dpi_zoom(96 * 1024, 0.0), 1.0, "GTK ignores 0");
  }
}
