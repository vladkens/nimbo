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

use super::dl::fns;
use super::{Fp, P, S, connect, each, lib};
use crate::app::{Appearance, Event};

const PORTAL: &CStr = c"org.freedesktop.portal.Desktop";
const PATH: &CStr = c"/org/freedesktop/portal/desktop";
const SETTINGS: &CStr = c"org.freedesktop.portal.Settings";
const NAMESPACE: &CStr = c"org.freedesktop.appearance";
/// `G_BUS_TYPE_SESSION`.
const SESSION: c_int = 2;
/// How long the first read waits for the portal (started on demand by the bus).
const TIMEOUT_MS: c_int = 250;

fns!(Gio {
  fn g_bus_get_sync(bus: c_int, cancel: P, err: *mut P) -> P;
  fn g_dbus_connection_call_sync(c: P, name: S, path: S, iface: S, method: S, args: P,
    reply: P, flags: c_int, ms: c_int, cancel: P, err: *mut P) -> P;
  fn g_dbus_connection_signal_subscribe(c: P, sender: S, iface: S, member: S, path: S, arg0: S,
    flags: c_int, cb: Fp, data: P, free: Fp) -> c_uint;
  fn g_variant_new_strv(v: *const S, n: isize) -> P;
  fn g_variant_new_tuple(items: *const P, n: usize) -> P;
  fn g_variant_get_child_value(v: P, i: usize) -> P;
  fn g_variant_lookup_value(dict: P, key: S, ty: P) -> P;
  fn g_variant_get_variant(v: P) -> P;
  fn g_variant_get_type_string(v: P) -> S;
  fn g_variant_get_uint32(v: P) -> u32;
  fn g_variant_get_double(v: P) -> f64;
  fn g_variant_unref(v: P);
  fn g_error_free(e: P);
  fn g_object_unref(obj: P);
  fn g_free(p: P);
  fn g_timeout_add(ms: c_uint, f: Fp, data: P) -> c_uint;
  fn gtk_settings_get_default() -> P;
});

/// GIO's functions, and `g_object_get` (property names and where each value goes, then null),
/// which `fns!` can't declare.
struct Api {
  f: Gio,
  g_object_get: unsafe extern "C" fn(P, S, ...),
}

fn api() -> Option<&'static Api> {
  static API: OnceLock<Option<Api>> = OnceLock::new();
  API
    .get_or_init(|| {
      let lib = lib()?;
      // SAFETY: gobject.h's declaration.
      let g_object_get = unsafe { lib.sym("g_object_get\0") }?;
      Some(Api { f: Gio::load(&lib)?, g_object_get })
    })
    .as_ref()
}

/// The OS's light or dark setting and accent now: the portal's, else GtkSettings' (light
/// before GTK opened a display).
pub fn appearance() -> Appearance {
  let Some(a) = api() else { return Appearance::LIGHT };
  let (dark, accent) = portal(a);
  Appearance { dark: dark.unwrap_or_else(|| gtk_dark(a)), accent }
}

/// Follows the portal's and GtkSettings' changes, each sent as [Event::Appearance] (the
/// runtime drops repeats).
pub fn watch() {
  let Some(a) = api() else { return };
  let f = &a.f;
  // SAFETY: GIO's and GTK's calls; the callbacks have the signals' signatures.
  unsafe {
    let settings = (f.gtk_settings_get_default)();
    if !settings.is_null() {
      connect(settings, c"notify::gtk-theme-name", on_changed as Fp, 0);
      connect(settings, c"notify::gtk-application-prefer-dark-theme", on_changed as Fp, 0);
    }

    // The connection is kept for the subscription.
    let bus = bus(a);
    if !bus.is_null() {
      let (iface, path, ns) = (SETTINGS.as_ptr(), PATH.as_ptr(), NAMESPACE.as_ptr());
      let (signal, cb) = (c"SettingChanged".as_ptr(), on_portal as Fp);
      let sub = f.g_dbus_connection_signal_subscribe;
      sub(bus, PORTAL.as_ptr(), iface, signal, path, ns, 0, cb, null_mut(), null());
    }
  }
}

unsafe extern "C" fn on_changed(_: P, _: P, _: P) {
  unsafe { deliver(null_mut()) };
}

/// `SettingChanged(namespace, key, value)` in the appearance namespace (the subscription's
/// first argument).
unsafe extern "C" fn on_portal(_: P, _: S, _: S, _: S, _: S, _: P, _: P) {
  unsafe { deliver(null_mut()) };
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

/// The session bus, a new reference; null without one.
fn bus(a: &Api) -> P {
  let mut err = null_mut();
  let bus = unsafe { (a.f.g_bus_get_sync)(SESSION, null_mut(), &mut err) };
  if bus.is_null() && !err.is_null() {
    unsafe { (a.f.g_error_free)(err) };
  }

  bus
}

/// The portal's dark (`None` for no preference) and accent, by `ReadAll` of the appearance
/// namespace; nothing without a portal.
fn portal(a: &Api) -> (Option<bool>, Option<[f32; 4]>) {
  let f = &a.f;
  let bus = bus(a);
  if bus.is_null() {
    return (None, None);
  }

  // SAFETY: GIO's calls; each variant made or returned here is unreferenced once read.
  unsafe {
    let ns = [NAMESPACE.as_ptr()];
    let args = [(f.g_variant_new_strv)(ns.as_ptr(), 1)];
    let args = (f.g_variant_new_tuple)(args.as_ptr(), 1);
    let (name, iface) = (PORTAL.as_ptr(), SETTINGS.as_ptr());
    let mut err = null_mut();
    let call = f.g_dbus_connection_call_sync;
    let reply = call(
      bus,
      name,
      PATH.as_ptr(),
      iface,
      c"ReadAll".as_ptr(),
      args,
      null_mut(),
      0,
      TIMEOUT_MS,
      null_mut(),
      &mut err,
    );
    (f.g_object_unref)(bus);
    if reply.is_null() {
      if !err.is_null() {
        (f.g_error_free)(err);
      }

      return (None, None);
    }

    // `(a{sa{sv}})`: the namespaces, each a dictionary of its keys.
    let all = (f.g_variant_get_child_value)(reply, 0);
    let ns = (f.g_variant_lookup_value)(all, NAMESPACE.as_ptr(), null_mut());
    let out = match ns.is_null() {
      true => (None, None),
      false => {
        let scheme = value(f, ns, c"color-scheme").map(|v| read(f, v, |v| scheme(f, v)));
        let accent = value(f, ns, c"accent-color").and_then(|v| read(f, v, |v| accent(f, v)));
        (f.g_variant_unref)(ns);
        (scheme.flatten(), accent)
      }
    };

    (f.g_variant_unref)(all);
    (f.g_variant_unref)(reply);
    out
  }
}

/// A key's value in a namespace's dictionary, unboxed from its variants.
unsafe fn value(f: &Gio, dict: P, key: &CStr) -> Option<P> {
  let mut v = unsafe { (f.g_variant_lookup_value)(dict, key.as_ptr(), null_mut()) };
  while !v.is_null() && unsafe { type_of(f, v) } == "v" {
    let inner = unsafe { (f.g_variant_get_variant)(v) };
    unsafe { (f.g_variant_unref)(v) };
    v = inner;
  }

  (!v.is_null()).then_some(v)
}

/// Reads `v` with `r`, then lets it go.
unsafe fn read<T>(f: &Gio, v: P, r: impl FnOnce(P) -> T) -> T {
  let out = r(v);
  unsafe { (f.g_variant_unref)(v) };
  out
}

unsafe fn type_of(f: &Gio, v: P) -> String {
  unsafe { CStr::from_ptr((f.g_variant_get_type_string)(v)) }.to_string_lossy().into_owned()
}

/// `color-scheme`, a `u`.
fn scheme(f: &Gio, v: P) -> Option<bool> {
  if unsafe { type_of(f, v) } != "u" {
    return None;
  }

  dark_scheme(unsafe { (f.g_variant_get_uint32)(v) })
}

/// `accent-color`, a `(ddd)`.
fn accent(f: &Gio, v: P) -> Option<[f32; 4]> {
  if unsafe { type_of(f, v) } != "(ddd)" {
    return None;
  }

  let c = [0, 1, 2].map(|i| unsafe {
    let d = (f.g_variant_get_child_value)(v, i);
    read(f, d, |d| (f.g_variant_get_double)(d))
  });
  accent_rgb(c)
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
