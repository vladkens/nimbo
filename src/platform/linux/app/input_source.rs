// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The keyboard layouts on Linux ([crate::input_source]), from the desktop that switches them:
//! GNOME's GSettings `org.gnome.desktop.input-sources` (`sources`, the one in use first in
//! `mru-sources`, its `changed` signal) and Plasma's `org.kde.keyboard` on the session bus
//! (`getLayoutsList`, `getLayout`, `layoutChanged`, `layoutListChanged`). A layout's language
//! and name come from XKB's registry (`evdev.xml`: `shortDescription`, `description`), a
//! variant's own where GNOME names it (`us+dvorak`); Plasma names only the layout, its variant in
//! the name it gives. Other desktops (another X11 window manager, Sway, Hyprland) tell none.
//!
//! What they miss: with GNOME's per-window sources (off by default) a source a window brings back
//! isn't in `mru-sources`, so the last one picked is told; Plasma 5 offers `org.kde.keyboard`
//! only with two layouts or more, so with one none is told.

use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::{CStr, c_int};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use super::super::dbus::{self, Gv, V};
use super::super::dl::fns;
use super::super::{Fp, P, S, connect, lib};
use crate::input_source::{InputSource, primary};
use crate::system::{Change, notice};

const SCHEMA: &CStr = c"org.gnome.desktop.input-sources";
const KDE: &str = "org.kde.keyboard";
const KDE_PATH: &str = "/Layouts";
const KDE_IFACE: &str = "org.kde.KeyboardLayouts";
/// How long a call to Plasma waits.
const TIMEOUT_MS: c_int = 250;

fns!(Gio {
  fn g_settings_schema_source_get_default() -> P;
  fn g_settings_schema_source_lookup(src: P, id: S, recursive: c_int) -> P;
  fn g_settings_schema_unref(schema: P);
  fn g_settings_new(id: S) -> P;
  fn g_settings_get_value(settings: P, key: S) -> P;
});

thread_local! {
  /// GNOME's settings while watched, and Plasma's two subscriptions.
  static WATCH: Cell<Option<(P, u32, u32)>> = const { Cell::new(None) };
}

/// Who switches the layouts here.
#[derive(Clone, Copy, PartialEq)]
enum Desktop {
  Gnome,
  Plasma,
}

fn desktop() -> Option<Desktop> {
  let names = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
  let has = |n: &str| names.split(':').any(|d| d.eq_ignore_ascii_case(n));
  match () {
    _ if has("KDE") => Some(Desktop::Plasma),
    _ if (has("GNOME") || has("Unity")) && gnome().is_some() => Some(Desktop::Gnome),
    _ => None,
  }
}

pub fn available() -> bool {
  desktop().is_some()
}

pub fn current() -> Option<InputSource> {
  match desktop()? {
    Desktop::Gnome => {
      let (g, settings) = gnome()?;
      let mru = sources(g, settings, c"mru-sources");
      mru.into_iter().next().or_else(|| sources(g, settings, c"sources").into_iter().next())
    }
    Desktop::Plasma => {
      let bus = dbus::session().ok()?;
      let i = call(bus, "getLayout")?.child(0).u32() as usize;
      plasma(bus).into_iter().nth(i)
    }
  }
}

pub fn enabled() -> Vec<InputSource> {
  match desktop() {
    Some(Desktop::Gnome) => gnome().map_or_else(Vec::new, |(g, s)| sources(g, s, c"sources")),
    Some(Desktop::Plasma) => dbus::session().map_or_else(|_| Vec::new(), plasma),
    None => Vec::new(),
  }
}

/// Starts telling [Change::InputSource] (on the main thread, where both call back).
pub fn watch() -> Result<(), String> {
  if WATCH.get().is_some() {
    return Ok(());
  }

  let watched = match desktop().ok_or("no desktop that tells its keyboard layouts")? {
    Desktop::Gnome => {
      let (g, settings) = gnome().ok_or("no GSettings")?;
      // SAFETY: the settings live while watched; the callback has `changed`'s signature.
      unsafe { connect(settings, c"changed", changed as Fp, 0) };
      // GSettings tells a key's change only once the key was read with a handler connected.
      sources(g, settings, c"sources");
      sources(g, settings, c"mru-sources");
      (settings, 0, 0)
    }
    Desktop::Plasma => {
      let bus = dbus::session()?;
      let sub = |m| {
        dbus::subscribe(bus, KDE, KDE_IFACE, m, Some(KDE_PATH), told as Fp, (null_mut(), null()))
      };
      (null_mut(), sub("layoutChanged"), sub("layoutListChanged"))
    }
  };

  WATCH.set(Some(watched));
  Ok(())
}

pub fn unwatch() {
  let Some((_, a, b)) = WATCH.take() else { return };
  if let Ok(bus) = dbus::session() {
    for id in [a, b].into_iter().filter(|&id| id != 0) {
      dbus::unsubscribe(bus, id);
    }
  }
}

unsafe extern "C" fn changed(_: P, _: S, _: P) {
  notice(Change::InputSource);
}

unsafe extern "C" fn told(_: P, _: S, _: S, _: S, _: S, _: P, _: P) {
  notice(Change::InputSource);
}

/// GIO and GNOME's input sources settings, made once; `None` without the schema (`g_settings_new`
/// aborts on a missing one).
fn gnome() -> Option<(&'static Gio, P)> {
  static GNOME: OnceLock<Option<(Gio, usize)>> = OnceLock::new();
  let (g, settings) = GNOME
    .get_or_init(|| {
      let g = Gio::load(&lib()?)?;
      // SAFETY: GIO's calls; the schema looked up before the settings are made, kept for good.
      unsafe {
        let schema = (g.g_settings_schema_source_lookup)(
          (g.g_settings_schema_source_get_default)(),
          SCHEMA.as_ptr(),
          1,
        );
        if schema.is_null() {
          return None;
        }

        (g.g_settings_schema_unref)(schema);
        let settings = (g.g_settings_new)(SCHEMA.as_ptr());
        (!settings.is_null()).then_some((g, settings as usize))
      }
    })
    .as_ref()?;
  Some((g, *settings as P))
}

/// GNOME's `a(ss)` list `key`: XKB layouts (`('xkb', 'us+dvorak')`); an input method
/// (`('ibus', 'mozc-jp')`) without a language.
fn sources(g: &Gio, settings: P, key: &CStr) -> Vec<InputSource> {
  // SAFETY: the value is a new reference, owned by `Gv`.
  let list = Gv(unsafe { (g.g_settings_get_value)(settings, key.as_ptr()) });
  (0..list.len())
    .map(|i| list.child(i))
    .map(|s| (s.child(0).str(), s.child(1).str()))
    .map(|(kind, name)| match kind.as_str() {
      "xkb" => xkb(&name, None),
      _ => InputSource { id: format!("{kind}:{name}"), name, language: None },
    })
    .collect()
}

/// Plasma's layouts: `(short name, display name, long name)`, the short name the layout's alone
/// (`us` for US Dvorak too), so the long name tells them apart in the id.
fn plasma(bus: P) -> Vec<InputSource> {
  let Some(reply) = call(bus, "getLayoutsList") else { return Vec::new() };
  let list = reply.child(0);
  (0..list.len())
    .map(|i| list.child(i))
    .map(|l| (l.child(0).str(), l.child(2).str()))
    .map(|(short, long)| InputSource {
      id: format!("xkb:{short}:{long}"),
      ..xkb(&short, Some(long))
    })
    .collect()
}

fn call(bus: P, method: &str) -> Option<Gv> {
  dbus::call(bus, KDE, KDE_PATH, KDE_IFACE, method, V::Tuple(vec![]), TIMEOUT_MS).ok()
}

/// XKB layout `name` (`us`, `us+dvorak`, `ru(phonetic)`): its language and name from the
/// registry, the variant's else the layout's, the name given when there is one.
fn xkb(name: &str, given: Option<String>) -> InputSource {
  let full = match name.split_once('+') {
    Some((layout, variant)) => format!("{layout}({variant})"),
    None => name.to_owned(),
  };

  let layout = name.split(['+', '(']).next().unwrap_or(name);
  let entry = registry().get(&full).or_else(|| registry().get(layout));
  let language = entry.and_then(|(short, _)| primary(short));
  let named = given.filter(|g| !g.is_empty()).or_else(|| entry.map(|e| e.1.clone()));
  InputSource {
    id: format!("xkb:{name}"),
    name: named.unwrap_or_else(|| name.to_owned()),
    language,
  }
}

/// XKB's layouts and variants by name (`us`, `us(dvorak)`): their short description, the language
/// they type (`en`, `uk` for `ua`), and their description (`English (Dvorak)`).
fn registry() -> &'static HashMap<String, (String, String)> {
  static REGISTRY: OnceLock<HashMap<String, (String, String)>> = OnceLock::new();
  REGISTRY.get_or_init(|| {
    let files = ["/usr/share/X11/xkb/rules/evdev.xml", "/usr/share/X11/xkb/rules/base.xml"];
    files
      .iter()
      .find_map(|f| std::fs::read_to_string(f).ok())
      .map_or_else(HashMap::new, |x| layouts(&x))
  })
}

/// Each `<layout>`'s own `configItem`, and each of its variants', whose language is the layout's
/// unless it gives its own.
fn layouts(xml: &str) -> HashMap<String, (String, String)> {
  let tag = |s: &str, t: &str| {
    let (open, close) = (format!("<{t}>"), format!("</{t}>"));
    let at = s.find(&open)? + open.len();
    Some(s[at..at + s[at..].find(&close)?].trim().replace("&amp;", "&"))
  };
  let mut out = HashMap::new();
  for block in xml.split("<layout>").skip(1) {
    let (own, variants) = block.split_once("<variantList>").unwrap_or((block, ""));
    let (Some(name), Some(desc)) = (tag(own, "name"), tag(own, "description")) else { continue };
    let short = tag(own, "shortDescription").unwrap_or_default();
    for v in variants.split("<variant>").skip(1) {
      let (Some(var), Some(var_desc)) = (tag(v, "name"), tag(v, "description")) else { continue };
      let var_short = tag(v, "shortDescription").unwrap_or_else(|| short.clone());
      out.insert(format!("{name}({var})"), (var_short, var_desc));
    }

    out.insert(name, (short, desc));
  }

  out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_registry_gives_a_layouts_language_and_name() {
    let xml = "<layoutList><layout><configItem><name>ua</name><shortDescription>uk</shortDescription>\
      <description>Ukrainian</description></configItem><variantList><variant><configItem>\
      <name>phonetic</name><description>Ukrainian (phonetic)</description></configItem></variant>\
      </variantList></layout><layout><configItem><name>us</name><shortDescription>en</shortDescription>\
      <description>English (US)</description></configItem></layout></layoutList>";
    let got = layouts(xml);
    assert_eq!(got["ua"], ("uk".into(), "Ukrainian".into()));
    assert_eq!(got["ua(phonetic)"], ("uk".into(), "Ukrainian (phonetic)".into()));
    assert_eq!(got["us"], ("en".into(), "English (US)".into()));
    assert_eq!(got.len(), 3);
  }
}
