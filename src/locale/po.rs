// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A gettext catalog read from a `.po` file's text: each translated message by its English key,
//! and the header's `Plural-Forms` formula, evaluated for a count as C evaluates it
//! ([PO files](https://www.gnu.org/software/gettext/manual/html_node/PO-Files.html),
//! [Plural forms](https://www.gnu.org/software/gettext/manual/html_node/Plural-forms.html)).

use std::collections::HashMap;
use std::iter::Peekable;
use std::mem;
use std::str::Bytes;

/// The translations of one language: the forms of each message by its msgid (one form, or one
/// per plural form), and the formula that picks a form for a count.
#[derive(Debug, Default)]
pub(crate) struct Catalog {
  msgs: HashMap<String, Vec<String>>,
  plural: Option<String>,
}

/// The string of an entry a quoted line goes on with.
#[derive(Clone, Copy, PartialEq)]
enum Field {
  Ctxt,
  Id,
  Plural,
  Str(usize),
}

/// An entry as it is read: its context, msgid, msgstr forms and fuzzy flag.
#[derive(Default)]
struct Entry {
  ctxt: bool,
  id: String,
  strs: Vec<String>,
  fuzzy: bool,
}

impl Catalog {
  /// The catalog in `po`, read leniently: what it can't read is left out (`msgfmt -c` checks a
  /// file). Fuzzy and obsolete (`#~`) entries are left out as gettext leaves them, and so are
  /// those with a context (msgctxt), which `t!` has none of; the header is read fuzzy or not.
  pub fn parse(po: &str) -> Self {
    let mut cat = Self::default();
    let (mut e, mut field) = (Entry::default(), Field::Id);
    for line in po.lines().map(str::trim) {
      let (key, text) = match line.starts_with('"') {
        true => (None, line),
        false => {
          let (key, text) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
          (Some(key), text.trim())
        }
      };

      let starts = line.starts_with('#') || matches!(key, Some("msgctxt" | "msgid"));
      if starts && !e.strs.is_empty() {
        cat.add(mem::take(&mut e));
      }

      if let Some(flags) = line.strip_prefix("#,") {
        e.fuzzy |= flags.split(',').any(|f| f.trim() == "fuzzy");
      }

      if line.is_empty() || line.starts_with('#') {
        continue;
      }

      field = match key {
        None => field,
        Some("msgctxt") => Field::Ctxt,
        Some("msgid") => Field::Id,
        Some("msgid_plural") => Field::Plural,
        Some("msgstr") => Field::Str(0),
        Some(key) => match key.strip_prefix("msgstr[").and_then(|k| k.strip_suffix(']')) {
          Some(i) => Field::Str(i.parse().unwrap_or(usize::MAX)),
          None => continue,
        },
      };

      let s = unquote(text);
      match field {
        Field::Ctxt => e.ctxt = true,
        Field::Id => e.id += &s,
        Field::Plural => {}
        Field::Str(i) if i < 16 => {
          if e.strs.len() <= i {
            e.strs.resize(i + 1, String::new());
          }

          e.strs[i] += &s;
        }
        Field::Str(_) => {}
      }
    }

    cat.add(e);
    cat
  }

  /// Keeps `e` if it is translated and gettext would use it.
  fn add(&mut self, e: Entry) {
    if e.strs.is_empty() {
      return;
    }

    if e.id.is_empty() && !e.ctxt {
      let header = e.strs.first().map_or("", String::as_str);
      let forms = header.lines().find_map(|l| l.trim().strip_prefix("Plural-Forms:"));
      self.plural = forms.map(|f| f.trim().to_owned());
      return;
    }

    if e.ctxt || e.fuzzy || e.strs.iter().any(String::is_empty) {
      return;
    }

    self.msgs.insert(e.id, e.strs);
  }

  /// The messages translated.
  #[cfg(test)]
  pub fn len(&self) -> usize {
    self.msgs.len()
  }

  /// The translation of `id`; `None` when there is none.
  pub fn get(&self, id: &str) -> Option<&str> {
    self.msgs.get(id)?.first().map(String::as_str)
  }

  /// The form of plural message `id` for `n`, by the header's formula (English's, `n != 1`,
  /// without one); `None` when there is no translation or the formula picks no form it has.
  pub fn plural(&self, id: &str, n: u64) -> Option<&str> {
    let forms = self.msgs.get(id)?;
    let i = match &self.plural {
      Some(f) => plural_form(f, n)?,
      None => (n != 1) as usize,
    };

    forms.get(i).map(String::as_str)
  }
}

/// A quoted `.po` string's text with C's escapes undone (octal and hex ones as UTF-8 bytes);
/// the text as it is, unquoted.
fn unquote(s: &str) -> String {
  let s = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s);
  let mut out = Vec::with_capacity(s.len());
  let mut it = s.bytes().peekable();
  while let Some(c) = it.next() {
    if c != b'\\' {
      out.push(c);
      continue;
    }

    let Some(c) = it.next() else { break };
    out.push(match c {
      b'n' => b'\n',
      b't' => b'\t',
      b'r' => b'\r',
      b'a' => 0x07,
      b'b' => 0x08,
      b'f' => 0x0c,
      b'v' => 0x0b,
      b'0'..=b'7' => digits(&mut it, (c - b'0').into(), 8, 2),
      b'x' => digits(&mut it, 0, 16, 2),
      c => c,
    });
  }

  String::from_utf8_lossy(&out).into_owned()
}

/// The byte an escape's digits in `radix` make, `v` and at most `max` more of them from `it`.
fn digits(it: &mut Peekable<Bytes>, mut v: u32, radix: u32, max: usize) -> u8 {
  for _ in 0..max {
    let Some(d) = it.peek().and_then(|&d| char::from(d).to_digit(radix)) else { break };
    v = v * radix + d;
    it.next();
  }

  v as u8
}

// MARK: Plural forms

/// The form `Plural-Forms` (`nplurals=3; plural=EXPR;`) picks for `n`; `None` when its formula
/// doesn't read as C. Both sides of `?:`, `&&` and `||` are evaluated, so a division by zero is
/// 0 rather than an error: `n == 0 ? 0 : 1 / n` reads at 0.
pub(crate) fn plural_form(forms: &str, n: u64) -> Option<usize> {
  let expr = forms.split(';').find_map(|p| p.trim().strip_prefix("plural="))?;
  let mut e = Eval { s: expr.as_bytes(), at: 0, n: n as i64 };
  let v = e.ternary()?;
  e.skip();
  if e.at < e.s.len() {
    return None;
  }

  usize::try_from(v).ok()
}

/// C's binary operators by precedence, the loosest first; a longer one before its prefix.
const LEVELS: [&[&str]; 6] =
  [&["||"], &["&&"], &["==", "!="], &["<=", ">=", "<", ">"], &["+", "-"], &["*", "/", "%"]];

/// A formula evaluated as it is read, by recursive descent.
struct Eval<'a> {
  s: &'a [u8],
  at: usize,
  n: i64,
}

impl Eval<'_> {
  fn skip(&mut self) {
    while self.s.get(self.at).is_some_and(u8::is_ascii_whitespace) {
      self.at += 1;
    }
  }

  /// Takes `tok` if it is next.
  fn eat(&mut self, tok: &str) -> bool {
    self.skip();
    let next = self.s[self.at..].starts_with(tok.as_bytes());
    self.at += if next { tok.len() } else { 0 };
    next
  }

  fn ternary(&mut self) -> Option<i64> {
    let cond = self.binary(0)?;
    if !self.eat("?") {
      return Some(cond);
    }

    let yes = self.ternary()?;
    self.eat(":").then_some(())?;
    let no = self.ternary()?;
    Some(if cond != 0 { yes } else { no })
  }

  /// Operators of `LEVELS[level]` and tighter, left to right.
  fn binary(&mut self, level: usize) -> Option<i64> {
    let Some(ops) = LEVELS.get(level) else { return self.unary() };
    let mut a = self.binary(level + 1)?;
    while let Some(op) = ops.iter().copied().find(|op| self.eat(op)) {
      let b = self.binary(level + 1)?;
      a = match op {
        "||" => (a != 0 || b != 0) as i64,
        "&&" => (a != 0 && b != 0) as i64,
        "==" => (a == b) as i64,
        "!=" => (a != b) as i64,
        "<=" => (a <= b) as i64,
        ">=" => (a >= b) as i64,
        "<" => (a < b) as i64,
        ">" => (a > b) as i64,
        "+" => a.wrapping_add(b),
        "-" => a.wrapping_sub(b),
        "*" => a.wrapping_mul(b),
        "/" => a.checked_div(b).unwrap_or(0),
        _ => a.checked_rem(b).unwrap_or(0),
      };
    }

    Some(a)
  }

  fn unary(&mut self) -> Option<i64> {
    if self.eat("!") {
      return Some((self.unary()? == 0) as i64);
    }

    if self.eat("(") {
      let v = self.ternary()?;
      return self.eat(")").then_some(v);
    }

    if self.eat("n") {
      return Some(self.n);
    }

    let start = self.at;
    while self.s.get(self.at).is_some_and(u8::is_ascii_digit) {
      self.at += 1;
    }

    std::str::from_utf8(&self.s[start..self.at]).ok()?.parse().ok()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const RU: &str = "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && \
                    (n%100<10 || n%100>=20) ? 1 : 2);";

  #[test]
  fn russian_plurals() {
    let forms = [1, 2, 5, 11, 21, 111, 0, 14, 22].map(|n| plural_form(RU, n));
    assert_eq!(forms, [0, 1, 2, 2, 0, 2, 2, 2, 1].map(Some));
  }

  #[test]
  fn formulas() {
    assert_eq!(plural_form("nplurals=2; plural=(n != 1);", 1), Some(0));
    assert_eq!(plural_form("nplurals=2; plural=n != 1;", 3), Some(1));
    assert_eq!(plural_form("nplurals=1; plural=0;", 7), Some(0));
    assert_eq!(plural_form("nplurals=2; plural=n>1;", 0), Some(0));
    // Arabic's six, 3..10 and 11..99 by the hundreds.
    let ar = "nplurals=6; plural=n==0 ? 0 : n==1 ? 1 : n==2 ? 2 : n%100>=3 && n%100<=10 ? 3 \
              : n%100>=11 ? 4 : 5;";
    let forms = [0, 1, 2, 3, 11, 100, 102].map(|n| plural_form(ar, n));
    assert_eq!(forms, [0, 1, 2, 3, 4, 5, 5].map(Some));
    assert_eq!(plural_form("plural=!(n - 1) + 2 * 3 / 2 - 1;", 1), Some(3));
    assert_eq!(plural_form("plural=n == 0 ? 0 : 1 / n;", 0), Some(0));
    for bad in ["plural=n %", "plural=(n", "plural=n ? 1", "nplurals=2", "plural=n x"] {
      assert_eq!(plural_form(bad, 1), None, "{bad}");
    }
  }

  const PO: &str = r#"
# Translators: a comment.
msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"
"Plural-Forms: nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);\n"

#: src/main.rs:10
msgid "Keep the history"
msgstr "Хранить историю"

msgid ""
"Two "
"lines"
msgstr "Две\n"
"строки \"в кавычках\" \\ "

msgid	"Caf\303\251"
msgstr	"Кафе \xe2\x80\x94 \101"

msgid "{n} file"
msgid_plural "{n} files"
msgstr[0] "{n} файл"
msgstr[1] "{n} файла"
msgstr[2] "{n} файлов"

#, fuzzy, rust-format
msgid "Old"
msgstr "Старое"

msgctxt "menu"
msgid "Open"
msgstr "Открыть"

msgid "Untranslated"
msgstr ""

#~ msgid "Gone"
#~ msgstr "Нет"
msgid "Close"
msgstr "Закрыть"
#~ msgid "Gone too"
#~ msgstr "Тоже нет""#;

  #[test]
  fn parses_a_po_file() {
    let cat = Catalog::parse(PO);
    assert_eq!(cat.get("Keep the history"), Some("Хранить историю"));
    assert_eq!(cat.get("Two lines"), Some("Две\nстроки \"в кавычках\" \\ "));
    assert_eq!(cat.get("{n} file"), Some("{n} файл"));
    let forms = [1, 3, 5, 21].map(|n| cat.plural("{n} file", n));
    assert_eq!(forms, ["{n} файл", "{n} файла", "{n} файлов", "{n} файл"].map(Some));
    for gone in ["Old", "Open", "Untranslated", "Gone", ""] {
      assert_eq!(cat.get(gone), None, "{gone}");
    }

    assert_eq!(cat.get("Close"), Some("Закрыть"));
    assert_eq!(cat.get("Café"), Some("Кафе — A"));
    // The header outlasts the obsolete entries after the last one.
    assert_eq!(cat.plural("{n} file", 5), Some("{n} файлов"));
  }

  /// Without `Plural-Forms`, English's rule.
  #[test]
  fn plurals_without_a_header() {
    let cat = Catalog::parse("msgid \"a\"\nmsgid_plural \"b\"\nmsgstr[0] \"x\"\nmsgstr[1] \"y\"\n");
    assert_eq!([0, 1, 2].map(|n| cat.plural("a", n)), [Some("y"), Some("x"), Some("y")]);
    assert_eq!(cat.plural("b", 2), None);
  }
}
