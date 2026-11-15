// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app's strings in the user's language: marked with [t!](crate::t) and [tn!](crate::tn),
//! looked up in gettext catalogs the app ships, keyed by the English text itself, as SwiftUI's
//! `Text("…")` and gettext key them.
//!
//! - `t!("Keep the history")` is a [Localized], `const` and `Copy`, translated where it shows: a
//!   text ([crate::Ui::text]), a [crate::Content], `format!("{KEEP}")`, a label as `&t!("Cancel")`
//!   (a `&str`). A string not marked never is (SwiftUI's `Text(verbatim:)`): names, paths, what the
//!   user typed.
//! - `t!("{chosen} is not on disk", chosen, size = f.size)` is a `String` translated now, its
//!   `{name}`s filled (`{{` and `}}` for braces), in the order the translation puts them.
//! - `tn!("{n} file", "{n} files", count)` is a `String` in the form the catalog's `Plural-Forms`
//!   picks for `count` (English's without one: 1, else the other), `{n}` the count; more `{name}`s
//!   follow as in `t!`.
//!
//! The catalogs, one `.po` file per language, come with the window's [crate::Config]
//! ([Config::localizations](crate::Config::localizations)); only the one in use is read. The
//! language is the system's ([Locale::System]) or the app's choice ([crate::Main::set_locale]),
//! found as the tag (`pt-BR`), then its language (`pt`); with no catalog, the English text.
//! nimbo's own strings (OK and Cancel, the text menu, an icon button's name) come in its own
//! catalogs (`locales/`, `make i18n`), in the language the app's give.
//! `pseudo` is a pseudo language (`--locale pseudo` in an example): every marked string accented
//! and a third longer, so unmarked and cut strings show. GNU gettext (0.24 or later, for Rust)
//! keeps the catalogs; a comment `// Translators: …` before a string goes to its translator:
//!
//! ```sh
//! xgettext --language=Rust --keyword='t!:1' --keyword='tn!:1,2' --add-comments=Translators \
//!   --from-code=UTF-8 -o locales/messages.pot src/*.rs
//! msginit -l ru_RU.UTF-8 -i locales/messages.pot -o locales/ru.po  # a new language
//! msgmerge --update locales/ru.po locales/messages.pot              # the code changed
//! msgfmt -c --statistics -o /dev/null locales/ru.po                 # forms, {placeholders}
//! ```
//!
//! An English string changed becomes fuzzy in the catalogs and shows in English until a
//! translator confirms it; one removed becomes obsolete (`#~`).

mod po;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::{self, Display, Write};
use std::iter;
use std::ops::Deref;
use std::sync::{Mutex, MutexGuard, PoisonError};

use po::Catalog;

use crate::platform;

/// The catalogs an app ships: a language tag (`ru`, `pt-BR`) and its `.po` file's text, as
/// `("ru", include_str!("../locales/ru.po"))`.
pub type Localizations = &'static [(&'static str, &'static str)];

/// nimbo's own catalogs, for its strings (OK, Cancel, Copy, an icon button's name), each used
/// with the app's of the same language (`make i18n` updates them from the code).
const OWN: Localizations = &[("ru", include_str!("../../locales/ru.po"))];

/// The language the app's strings show in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Locale {
  /// The first of the user's preferred languages the app has a catalog for, stopping at
  /// English (macOS: `NSLocale.preferredLanguages`, with the app's own language from System
  /// Settings first; Windows: `GetUserPreferredUILanguages`; Linux: `LANGUAGE`, then `LC_ALL`,
  /// `LC_MESSAGES` or `LANG`).
  #[default]
  System,
  /// A language tag the app picked (`ru`, `pt-BR`; `en` for the English texts).
  Tag(&'static str),
}

impl Locale {
  /// The language chosen now: the harness's on this thread ([crate::Harness::set_locale]), else
  /// the app's ([crate::Main::set_locale]).
  pub fn current() -> Self {
    THREAD.with_borrow(|s| s.as_ref().map(|s| s.locale)).unwrap_or_else(|| global().locale)
  }
}

/// A string to translate where it shows, made by [t!](crate::t): its English text is the key.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Localized(&'static str);

/// As the text it shows, so a tree of labels reads the same whether a label was marked.
impl fmt::Debug for Localized {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    fmt::Debug::fmt(self.as_str(), f)
  }
}

impl Localized {
  pub const fn new(key: &'static str) -> Self {
    Self(key)
  }

  /// The English text, untranslated.
  pub const fn key(self) -> &'static str {
    self.0
  }

  /// The translation in the language chosen now, the English text without one, kept as long
  /// as the program: where a `&'static str` is wanted (a field's hint); `&t!("…")` is a `&str`
  /// for the statement, as a label takes it.
  pub fn as_str(self) -> &'static str {
    lang().text(self.0)
  }
}

/// [Localized::as_str], so `&t!("Cancel")` passes as a `&str`.
impl Deref for Localized {
  type Target = str;

  fn deref(&self) -> &str {
    self.as_str()
  }
}

impl Display for Localized {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.write_str(self.as_str())
  }
}

/// [t!](crate::t) with arguments: `key` translated, its `{name}`s filled from `args`.
#[doc(hidden)]
pub fn format(key: &'static str, args: &[(&str, &dyn Display)]) -> String {
  fill(lang().text(key), args)
}

/// [tn!](crate::tn): the form of `one` / `other` for `n`, its `{name}`s filled from `args`.
#[doc(hidden)]
pub fn plural(one: &str, other: &str, n: u64, args: &[(&str, &dyn Display)]) -> String {
  let lang = lang();
  let english = if n == 1 { one } else { other };
  if lang.pseudo.is_some() {
    return fill(&pseudo(english), args);
  }

  let form = lang.app.as_ref().and_then(|c| c.plural(one, n));
  let form = form.or_else(|| lang.own.as_ref()?.plural(one, n));
  fill(form.unwrap_or(english), args)
}

// MARK: State

/// The language chosen, the catalogs to find it in, and the one in use once a string asked.
struct State {
  locale: Locale,
  catalogs: Localizations,
  lang: Option<&'static Lang>,
}

/// A language as it translates: the app's catalog and nimbo's own of the same language, or
/// the pseudo language's strings made so far, or none (English). Made the first time a string
/// asks after a language is chosen, and kept as long as the program, so that its strings are
/// `&'static str`: a catalog's text per language the app chooses.
#[derive(Default)]
struct Lang {
  app: Option<Catalog>,
  own: Option<Catalog>,
  pseudo: Option<Mutex<HashMap<&'static str, &'static str>>>,
}

static STATE: Mutex<State> = Mutex::new(State::new(Locale::System, &[]));

thread_local! {
  /// The harness's language on its thread, over the process's: tests run side by side.
  static THREAD: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn global() -> MutexGuard<'static, State> {
  STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The language in use on this thread.
fn lang() -> &'static Lang {
  let local = THREAD.with_borrow_mut(|s| s.as_mut().map(State::lang));
  local.unwrap_or_else(|| global().lang())
}

/// The app's language from now on ([crate::Main::set_locale]).
pub(crate) fn set(locale: Locale) {
  let mut s = global();
  if s.locale != locale {
    *s = State::new(locale, s.catalogs);
  }
}

/// The catalogs of a window's config: the app's from now on, unless it has none.
pub(crate) fn provide(catalogs: Localizations) {
  let mut s = global();
  if !catalogs.is_empty() && !same(s.catalogs, catalogs) {
    *s = State::new(s.locale, catalogs);
  }
}

/// The harness's `locale` and `catalogs` on this thread ([crate::Harness::set_locale]).
pub(crate) fn set_thread(locale: Locale, catalogs: Localizations) {
  THREAD.set(Some(State::new(locale, catalogs)));
}

impl State {
  const fn new(locale: Locale, catalogs: Localizations) -> Self {
    Self { locale, catalogs, lang: None }
  }

  /// The language in use, its catalogs read the first time this language is chosen with
  /// these catalogs: the languages made are kept, so that tests and a language picker read
  /// each one once.
  fn lang(&mut self) -> &'static Lang {
    static MADE: Mutex<Vec<(Locale, Localizations, &'static Lang)>> = Mutex::new(Vec::new());
    let (locale, catalogs) = (self.locale, self.catalogs);
    self.lang.get_or_insert_with(|| {
      let mut made = MADE.lock().unwrap_or_else(PoisonError::into_inner);
      if let Some(m) = made.iter().find(|m| m.0 == locale && same(m.1, catalogs)) {
        return m.2;
      }

      let lang: &'static Lang = Box::leak(Box::new(Lang::find(locale, catalogs)));
      made.push((locale, catalogs, lang));
      lang
    })
  }
}

/// The same catalogs: the same tags and `.po` texts, wherever the lists are.
fn same(a: Localizations, b: Localizations) -> bool {
  let eq = |(x, y): (&(&str, &str), &(&str, &str))| x.0 == y.0 && std::ptr::eq(x.1, y.1);
  a.len() == b.len() && a.iter().zip(b).all(eq)
}

impl Lang {
  /// The first of the languages `locale` wants that `catalogs` has, with nimbo's own catalog of
  /// it; English, the keys' language, ends the search.
  fn find(locale: Locale, catalogs: Localizations) -> Self {
    let wanted = match locale {
      Locale::System => platform::languages(),
      Locale::Tag(tag) => vec![tag.to_owned()],
    };

    for want in &wanted {
      if want == "pseudo" {
        return Self { pseudo: Some(Mutex::default()), ..Self::default() };
      }

      if let Some(po) = po_of(catalogs, want) {
        let own = po_of(OWN, want).map(Catalog::parse);
        return Self { app: Some(Catalog::parse(po)), own, pseudo: None };
      }

      if want.split('-').next().is_some_and(|l| l.eq_ignore_ascii_case("en")) {
        break;
      }
    }

    Self::default()
  }

  /// `key` in this language: the app's, else nimbo's translation, else the key.
  fn text(&'static self, key: &'static str) -> &'static str {
    if let Some(made) = &self.pseudo {
      let mut made = made.lock().unwrap_or_else(PoisonError::into_inner);
      return made.entry(key).or_insert_with(|| pseudo(key).leak());
    }

    let found = self.app.as_ref().and_then(|c| c.get(key));
    found.or_else(|| self.own.as_ref()?.get(key)).unwrap_or(key)
  }
}

/// The `.po` text of `catalogs` for `tag`, else for a shorter one (`zh-Hant-TW`, `zh-Hant`,
/// `zh`).
fn po_of(catalogs: Localizations, tag: &str) -> Option<&'static str> {
  let mut tag = tag;
  loop {
    if let Some((_, po)) = catalogs.iter().find(|c| c.0.eq_ignore_ascii_case(tag)) {
      return Some(po);
    }

    tag = tag.rsplit_once('-')?.0;
  }
}

// MARK: Text

/// `s` with each `{name}` of `args` filled, `{{` and `}}` as braces; other braces kept.
fn fill(s: &str, args: &[(&str, &dyn Display)]) -> String {
  let mut out = String::with_capacity(s.len());
  let mut rest = s;
  while let Some(i) = rest.find(['{', '}']) {
    out += &rest[..i];
    rest = &rest[i..];
    if rest.starts_with("{{") || rest.starts_with("}}") {
      out += &rest[..1];
      rest = &rest[2..];
      continue;
    }

    let arg = rest.strip_prefix('{').and_then(|r| r.split_once('}'));
    let arg = arg.and_then(|(name, tail)| Some((args.iter().find(|a| a.0 == name)?.1, tail)));
    match arg {
      Some((v, tail)) => {
        let _ = write!(out, "{v}");
        rest = tail;
      }
      None => {
        out += &rest[..1];
        rest = &rest[1..];
      }
    }
  }

  out + rest
}

/// The pseudo language's `a` to `z` and `A` to `Z`.
const LOWER: &str = "åƀçđéƒĝĥîĵķĺɱñöþǫŕšţûṽŵẋýž";
const UPPER: &str = "ÅƁÇĐÉƑĜĤÎĴҜĹṀÑÖÞǪŔŠŢÛṼŴẊÝŽ";

/// `s` in the pseudo language: its letters accented, in brackets and about 35% longer with
/// tildes, its `{placeholders}` kept: `[Ҝééþ ţĥé ĥîšţöŕý ~~~]`.
fn pseudo(s: &str) -> String {
  let mut out = String::from("[");
  let mut inside = false;
  for c in s.chars() {
    inside |= c == '{';
    let accented = match c {
      'a'..='z' if !inside => LOWER.chars().nth(c as usize - 'a' as usize),
      'A'..='Z' if !inside => UPPER.chars().nth(c as usize - 'A' as usize),
      _ => None,
    };

    out.push(accented.unwrap_or(c));
    inside &= c != '}';
  }

  // The brackets and the space are 3 of the added third.
  let tildes = (s.chars().count() * 35).div_ceil(100).saturating_sub(3).max(1);
  out.push(' ');
  out.extend(iter::repeat_n('~', tildes));
  out + "]"
}

// MARK: Macros

/// A string of the app's to translate ([crate::locale]): `t!("Keep the history")` is a
/// [Localized], translated where it shows; with arguments, `t!("{name} is gone", name, size =
/// f.size)` is a `String` translated now, each `{name}` filled by its `Display`.
#[macro_export]
macro_rules! t {
  ($key:literal $(,)?) => {
    $crate::locale::Localized::new($key)
  };
  ($key:literal, $($args:tt)+) => {
    $crate::locale::format($key, &$crate::__t_args!([] $($args)+))
  };
}

/// A string with a count, translated now ([crate::locale]): `tn!("{n} file", "{n} files",
/// count)`, the English singular and plural, the form the language's `Plural-Forms` picks for
/// `count` (any integer), `{n}` filled with it; more arguments as [t!](crate::t) takes them.
#[macro_export]
macro_rules! tn {
  ($one:literal, $other:literal, $n:expr $(, $($args:tt)*)?) => {{
    let n = $n;
    let args = $crate::__t_args!([("n", &n as &dyn ::core::fmt::Display),] $($($args)*)?);
    $crate::locale::plural($one, $other, n as u64, &args)
  }};
}

/// The arguments of [t!](crate::t) and [tn!](crate::tn) as `(name, &dyn Display)` pairs:
/// `name` alone or `name = expr`.
#[doc(hidden)]
#[macro_export]
macro_rules! __t_args {
  ([$($out:tt)*] $(,)?) => {
    [$($out)*]
  };
  ([$($out:tt)*] $name:ident = $v:expr $(, $($rest:tt)*)?) => {
    $crate::__t_args!(
      [$($out)* (::core::stringify!($name), &$v as &dyn ::core::fmt::Display),] $($($rest)*)?
    )
  };
  ([$($out:tt)*] $name:ident $(, $($rest:tt)*)?) => {
    $crate::__t_args!(
      [$($out)* (::core::stringify!($name), &$name as &dyn ::core::fmt::Display),] $($($rest)*)?
    )
  };
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{App, Harness, Id, Size, Ui};

  const RU: &str = r#"
msgid ""
msgstr "Plural-Forms: nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);\n"

msgid "Keep the history"
msgstr "Хранить историю"

msgid "{chosen} is not on disk: {fallback} transcribes instead."
msgstr "{chosen} нет на диске: вместо него распознаёт {fallback}."

msgid "{n} dictation"
msgid_plural "{n} dictations"
msgstr[0] "{n} диктовка"
msgstr[1] "{n} диктовки"
msgstr[2] "{n} диктовок"
"#;
  const CATALOGS: Localizations =
    &[("de", "msgid \"Keep the history\"\nmsgstr \"Verlauf\""), ("ru", RU)];
  const KEEP: Localized = t!("Keep the history");

  #[test]
  fn translates_on_this_thread() {
    set_thread(Locale::Tag("ru"), CATALOGS);
    assert_eq!(Locale::current(), Locale::Tag("ru"));
    assert_eq!(KEEP.to_string(), "Хранить историю");
    assert_eq!(format!("{KEEP}!"), "Хранить историю!");
    let (chosen, model) = ("parakeet", ("whisper",));
    let s =
      t!("{chosen} is not on disk: {fallback} transcribes instead.", chosen, fallback = model.0);
    assert_eq!(s, "parakeet нет на диске: вместо него распознаёт whisper.");
    let forms = [1, 2, 5, 11, 21, 111].map(|n| tn!("{n} dictation", "{n} dictations", n));
    let want =
      ["1 диктовка", "2 диктовки", "5 диктовок", "11 диктовок", "21 диктовка", "111 диктовок"];
    assert_eq!(forms, want);
    assert_eq!(t!("Not in the catalog").to_string(), "Not in the catalog");
    let s: &str = &KEEP;
    assert_eq!((s, KEEP.as_str()), ("Хранить историю", "Хранить историю"));
    assert_eq!(format!("{KEEP:?}"), "\"Хранить историю\"");
  }

  /// nimbo's own strings, in the language the app's catalogs give.
  #[test]
  fn own_strings_follow_the_app() {
    set_thread(Locale::Tag("ru-RU"), CATALOGS);
    assert_eq!([t!("Cancel"), t!("Copy")].map(Localized::as_str), ["Отменить", "Скопировать"]);
    set_thread(Locale::Tag("ru"), &[("de", "")]);
    assert_eq!(t!("Cancel").as_str(), "Cancel");
    for (tag, po) in OWN {
      let entries = po.matches("\nmsgid ").count() - 1;
      assert_eq!(Catalog::parse(po).len(), entries, "{tag}: each translated");
    }
  }

  /// The tag, then shorter ones, then the English key.
  #[test]
  fn falls_back_to_the_language_then_english() {
    for (tag, want) in [("ru-RU", "Хранить историю"), ("DE", "Verlauf"), ("fr", "Keep the history")]
    {
      set_thread(Locale::Tag(tag), CATALOGS);
      assert_eq!(KEEP.to_string(), want, "{tag}");
    }

    set_thread(Locale::Tag("ru"), &[]);
    assert_eq!(KEEP.to_string(), "Keep the history");
    assert_eq!(tn!("{n} dictation", "{n} dictations", 1usize), "1 dictation");
    assert_eq!(tn!("{n} dictation", "{n} dictations", 0), "0 dictations");
  }

  #[test]
  fn fills_placeholders() {
    let (a, b) = (1, "two");
    let args: [(&str, &dyn Display); 2] = [("a", &a), ("b", &b)];
    assert_eq!(fill("{b} {a}{a}", &args), "two 11");
    assert_eq!(fill("{{a}} }} {{", &args), "{a} } {");
    assert_eq!(fill("{c} {a", &args), "{c} {a");
    assert_eq!(t!("{x} of {y}", x = 1 + 1, y = "z"), "2 of z");
    assert_eq!(tn!("{n} of {y}", "{n} of {y}s", 3u8, y = "z"), "3 of zs");
  }

  #[test]
  fn pseudo_is_longer_and_keeps_placeholders() {
    assert_eq!(pseudo("Keep the history"), "[Ҝééþ ţĥé ĥîšţöŕý ~~~]");
    assert_eq!(pseudo("Hi {name}"), "[Ĥî {name} ~]");
    assert_eq!([LOWER, UPPER].map(|s| s.chars().count()), [26, 26]);
    set_thread(Locale::Tag("pseudo"), CATALOGS);
    assert_eq!(t!("{n} left", n = 3), "[3 ĺéƒţ ~]");
  }

  /// What the system prefers, as tags: printed for a look on each OS.
  #[test]
  fn system_languages_are_tags() {
    let langs = platform::languages();
    println!("system languages: {langs:?}");
    assert!(langs.iter().all(|l| !l.is_empty() && !l.contains(['_', '.', '@'])), "{langs:?}");
  }

  struct Shows;

  impl App for Shows {
    fn view(&mut self, ui: &mut Ui) {
      let t = ui.theme();
      ui.id("keep").text(KEEP, t.label());
    }
  }

  /// A window shows the catalog's string in its language, the English key without a catalog,
  /// and the next language set redraws it.
  #[test]
  fn a_window_shows_its_language() {
    let mut h = Harness::new(Shows, Size::new(200.0, 60.0), 1.0);
    h.set_locale(Locale::Tag("ru"), CATALOGS);
    h.frame();
    assert_eq!(h.text(Id::of("keep")), Some("Хранить историю"));
    h.set_locale(Locale::Tag("ru"), &[]);
    assert!(h.frame());
    assert_eq!(h.text(Id::of("keep")), Some("Keep the history"));
  }
}
