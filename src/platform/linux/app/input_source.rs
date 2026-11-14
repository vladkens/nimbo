// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The keyboard layouts on Linux ([crate::input_source]), from the desktop that switches them:
//! GNOME's GSettings `org.gnome.desktop.input-sources` (`sources`, the one in use first in
//! `mru-sources`, its `changed` signal) and Plasma's `org.kde.keyboard` on the session bus
//! (`getLayoutsList`, `getLayout`, `layoutChanged`, `layoutListChanged`). A layout's language
//! and name come from XKB's registry (`evdev.xml`: `languageList`, `description`), a variant's
//! own where the desktop names it: GNOME in the source (`us+dvorak`), Plasma in its settings
//! (`kxkbrc`'s `VariantList`). Other desktops (another X11 window manager, Sway, Hyprland) tell
//! none.
//!
//! What they miss: with GNOME's per-window sources (off by default) a source a window brings back
//! isn't in `mru-sources`, so the last one picked is told; Plasma 5 offers `org.kde.keyboard`
//! only with two layouts or more, so with one none is told.

use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::{CStr, c_int, c_ulong};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use super::super::dbus::{self, Gv, V};
use super::super::dl::fns;
use super::super::{Fp, P, S, connect, lib};
use crate::input_source::{InputSource, primary};
use crate::paths;
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
  fn g_signal_handler_disconnect(obj: P, id: c_ulong);
});

thread_local! {
  /// GNOME's `changed` handler while watched, and Plasma's two subscriptions.
  static WATCH: Cell<Option<(c_ulong, u32, u32)>> = const { Cell::new(None) };
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
      // GNOME saves the recent ones as the user switches: one removed since can lead.
      let (g, settings) = gnome()?;
      let all = sources(g, settings, c"sources");
      let mru = sources(g, settings, c"mru-sources");
      mru.into_iter().find(|m| all.iter().any(|s| s.id == m.id)).or_else(|| all.into_iter().next())
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
      // SAFETY: the settings live for good; the callback has `changed`'s signature.
      let id = unsafe { connect(settings, c"changed", changed as Fp, 0) };
      // GSettings tells a key's change only once the key was read with a handler connected.
      sources(g, settings, c"sources");
      sources(g, settings, c"mru-sources");
      (id, 0, 0)
    }
    Desktop::Plasma => {
      let bus = dbus::session()?;
      let sub = |m| {
        dbus::subscribe(bus, KDE, KDE_IFACE, m, Some(KDE_PATH), told as Fp, (null_mut(), null()))
      };
      (0, sub("layoutChanged"), sub("layoutListChanged"))
    }
  };

  WATCH.set(Some(watched));
  Ok(())
}

pub fn unwatch() {
  let Some((id, a, b)) = WATCH.take() else { return };
  if id != 0
    && let Some((g, settings)) = gnome()
  {
    // SAFETY: the handler connected by [watch].
    unsafe { (g.g_signal_handler_disconnect)(settings, id) };
  }

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
/// (`us` for US Dvorak too), so the long name tells them apart in the id; their variants from
/// Plasma's settings, in the same order.
fn plasma(bus: P) -> Vec<InputSource> {
  let Some(reply) = call(bus, "getLayoutsList") else { return Vec::new() };
  let list = reply.child(0);
  let set = kxkbrc();
  (0..list.len())
    .map(|i| (i, list.child(i)))
    .map(|(i, l)| (i, l.child(0).str(), l.child(2).str()))
    .map(|(i, short, long)| {
      let var = set.get(i).filter(|(l, v)| *l == short && !v.is_empty());
      let full = var.map_or_else(|| short.clone(), |(_, v)| format!("{short}({v})"));
      InputSource { id: format!("xkb:{short}:{long}"), ..xkb(&full, Some(long)) }
    })
    .collect()
}

/// Plasma's layouts and their variants as set (`[Layout]`'s `LayoutList=us,ru`,
/// `VariantList=,phonetic`), the layout empty past the list.
fn kxkbrc() -> Vec<(String, String)> {
  layout_lists(&std::fs::read_to_string(paths::config_dir("kxkbrc")).unwrap_or_default())
}

fn layout_lists(ini: &str) -> Vec<(String, String)> {
  let list = |key: &str| {
    let line = ini.lines().find_map(|l| l.trim().strip_prefix(key)?.strip_prefix('='));
    line.map_or_else(Vec::new, |l| l.split(',').map(|x| x.trim().to_owned()).collect())
  };
  let (layouts, variants) = (list("LayoutList"), list("VariantList"));
  layouts
    .into_iter()
    .enumerate()
    .map(|(i, l)| (l, variants.get(i).cloned().unwrap_or_default()))
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
  let language = entry.and_then(|e| e.0.clone());
  let named = given.filter(|g| !g.is_empty()).or_else(|| entry.map(|e| e.1.clone()));
  InputSource {
    id: format!("xkb:{name}"),
    name: named.unwrap_or_else(|| name.to_owned()),
    language,
  }
}

/// A layout's language and description.
type Entry = (Option<String>, String);

/// XKB's layouts and variants by name (`us`, `us(dvorak)`): the language they type (`en`, `uk` for
/// `ua`) and their description (`English (Dvorak)`).
fn registry() -> &'static HashMap<String, Entry> {
  static REGISTRY: OnceLock<HashMap<String, Entry>> = OnceLock::new();
  REGISTRY.get_or_init(|| {
    let files = ["/usr/share/X11/xkb/rules/evdev.xml", "/usr/share/X11/xkb/rules/base.xml"];
    files
      .iter()
      .find_map(|f| std::fs::read_to_string(f).ok())
      .map_or_else(HashMap::new, |x| layouts(&x))
  })
}

/// Each `<layout>`'s own `configItem`, and each of its variants', whose language is the layout's
/// unless it gives its own: the first of its `languageList`, an ISO 639-3 code (`shortDescription`
/// is only a label: `be` for Belgian).
fn layouts(xml: &str) -> HashMap<String, Entry> {
  let tag = |s: &str, t: &str| {
    let (open, close) = (format!("<{t}>"), format!("</{t}>"));
    let at = s.find(&open)? + open.len();
    Some(s[at..at + s[at..].find(&close)?].trim().replace("&amp;", "&"))
  };
  let mut out = HashMap::new();
  for block in xml.split("<layout>").skip(1) {
    let (own, variants) = block.split_once("<variantList>").unwrap_or((block, ""));
    let (Some(name), Some(desc)) = (tag(own, "name"), tag(own, "description")) else { continue };
    let lang = tag(own, "iso639Id").and_then(|c| language(&c));
    for v in variants.split("<variant>").skip(1) {
      let (Some(var), Some(var_desc)) = (tag(v, "name"), tag(v, "description")) else { continue };
      let var_lang = tag(v, "iso639Id").map_or_else(|| lang.clone(), |c| language(&c));
      out.insert(format!("{name}({var})"), (var_lang, var_desc));
    }

    out.insert(name, (lang, desc));
  }

  out
}

/// ISO 639-3 code `code` as a language tag's: its two letters where ISO 639-1 has them (`deu`,
/// `de`), else itself (`prs`); none for `und`.
fn language(code: &str) -> Option<String> {
  let two = ISO639_1.as_bytes().chunks(5).find(|c| &c[..3] == code.as_bytes());
  match two {
    Some(c) => Some(String::from_utf8_lossy(&c[3..]).into_owned()),
    None if code == "und" => None,
    None => primary(code),
  }
}

/// Each ISO 639-3 code with ISO 639-1's two letters, as five letters (`deude`), from the
/// [ISO 639-3 tables](https://iso639-3.sil.org/code_tables/download_tables).
const ISO639_1: &str = "\
  aaraaabkabafrafakaakamhamaraararganasmasavaavaveaeaymayazeazbakbabambmbelbebenbn\
  bisbibodbobosbsbrebrbulbgcatcacescschachchecechucuchvcvcorkwcoscocrecrcymcydanda\
  deudedivdvdzodzellelengenepoeoesteteuseueweeefaofofasfafijfjfinfifrafrfryfyfulff\
  glagdglegaglgglglvgvgrngngujguhaththauhahbsshhebheherhzhinhihmohohrvhrhunhuhyehy\
  iboigidoioiiiiiikuiuileieinaiaindidipkikislisitaitjavjvjpnjakalklkanknkaskskatka\
  kaukrkazkkkhmkmkikkikinrwkirkykomkvkonkgkorkokuakjkurkulaololatlalavlvlimlilinln\
  litltltzlblubluluglgmahmhmalmlmarmrmkdmkmlgmgmltmtmonmnmrimimsamsmyamynaunanavnv\
  nblnrndendndongnepnenldnlnnonnnobnbnornonyanyociocojiojoriorormomossospanpaplipi\
  polplporptpuspsquequrohrmronrorunrnrusrusagsgsansasinsislkskslvslsmesesmosmsnasn\
  sndsdsomsosotstspaessqisqsrdscsrpsrsswsssunsuswaswswesvtahtytamtatatttteltetgktg\
  tgltlthathtirtitontotsntntsotstuktkturtrtwitwuigugukrukurduruzbuzvenvevievivolvo\
  wlnwawolwoxhoxhyidyiyoryozhazazhozhzulzu";

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_registry_gives_a_layouts_language_and_name() {
    let xml = "<layoutList><layout><configItem><name>be</name><shortDescription>be</shortDescription>\
      <description>Belgian</description><languageList><iso639Id>deu</iso639Id><iso639Id>nld\
      </iso639Id></languageList></configItem><variantList><variant><configItem><name>oss</name>\
      <description>Belgian (alt.)</description></configItem></variant></variantList></layout>\
      <layout><configItem><name>us</name><shortDescription>en</shortDescription><description>\
      English (US)</description><languageList><iso639Id>eng</iso639Id></languageList>\
      </configItem><variantList><variant><configItem><name>rus</name><description>Russian (US, \
      phonetic)</description><languageList><iso639Id>rus</iso639Id></languageList></configItem>\
      </variant></variantList></layout><layout><configItem><name>brai</name><description>Braille\
      </description></configItem></layout></layoutList>";
    let got = layouts(xml);
    let lang = |s: &str| Some(s.to_owned());
    assert_eq!(got["be"], (lang("de"), "Belgian".into()));
    assert_eq!(got["be(oss)"], (lang("de"), "Belgian (alt.)".into()));
    assert_eq!(got["us"], (lang("en"), "English (US)".into()));
    assert_eq!(got["us(rus)"], (lang("ru"), "Russian (US, phonetic)".into()));
    assert_eq!(got["brai"], (None, "Braille".into()));
    assert_eq!(got.len(), 5);
  }

  #[test]
  fn an_iso_639_3_code_is_a_tags_language() {
    let cases = [("deu", Some("de")), ("zho", Some("zh")), ("prs", Some("prs")), ("und", None)];
    for (code, want) in cases {
      assert_eq!(language(code).as_deref(), want, "{code}");
    }
  }

  #[test]
  fn plasmas_settings_give_each_layouts_variant() {
    let ini = "[Layout]\nDisplayNames=,\nLayoutList=us,ru\nUse=true\nVariantList=,phonetic\n";
    let want = [("us".into(), String::new()), ("ru".into(), "phonetic".into())];
    assert_eq!(layout_lists(ini), want);
    assert!(layout_lists("").is_empty());
  }
}
