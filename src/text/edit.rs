// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Grapheme cluster boundaries: where the layout breaks a word too wide for its line, between
//! characters a person reads as one.

const ZWJ: char = '\u{200d}';

/// Characters that join the cluster before them (Unicode's Grapheme_Extend and SpacingMark,
/// the common ranges): combining marks, Indic and Thai vowel signs, variation selectors, emoji
/// skin tones and tags, ZWNJ and ZWJ.
pub(crate) fn is_extend(c: char) -> bool {
  matches!(c as u32,
    0x0300..=0x036f | 0x0483..=0x0489 | 0x0591..=0x05bd | 0x05bf | 0x05c1..=0x05c2
    | 0x05c4..=0x05c5 | 0x05c7 | 0x0610..=0x061a | 0x064b..=0x065f | 0x0670 | 0x06d6..=0x06dc
    | 0x06df..=0x06e4 | 0x06e7..=0x06e8 | 0x06ea..=0x06ed | 0x0900..=0x0903 | 0x093a..=0x093c
    | 0x093e..=0x094f | 0x0951..=0x0957 | 0x0962..=0x0963 | 0x0981..=0x0983 | 0x09bc
    | 0x09be..=0x09cd | 0x09d7 | 0x0e31 | 0x0e34..=0x0e3a | 0x0e47..=0x0e4e | 0x1ab0..=0x1aff
    | 0x1dc0..=0x1dff | 0x200c..=0x200d | 0x20d0..=0x20ff | 0x302a..=0x302f | 0x3099..=0x309a
    | 0xfe00..=0xfe0f | 0xfe20..=0xfe2f | 0x1f3fb..=0x1f3ff | 0xe0020..=0xe007f | 0xe0100..=0xe01ef)
}

/// Line breaks and other controls: clusters of their own (`\r\n` is one).
fn is_control(c: char) -> bool {
  c.is_control() || c == '\u{2028}' || c == '\u{2029}'
}

fn is_regional(c: char) -> bool {
  ('\u{1f1e6}'..='\u{1f1ff}').contains(&c)
}

/// Hangul jamo and syllables: L, V, T, LV, LVT.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hangul {
  L,
  V,
  T,
  Lv,
  Lvt,
}

fn hangul(c: char) -> Option<Hangul> {
  match c as u32 {
    0x1100..=0x115f | 0xa960..=0xa97c => Some(Hangul::L),
    0x1160..=0x11a7 | 0xd7b0..=0xd7c6 => Some(Hangul::V),
    0x11a8..=0x11ff | 0xd7cb..=0xd7fb => Some(Hangul::T),
    s @ 0xac00..=0xd7a3 if (s - 0xac00) % 28 == 0 => Some(Hangul::Lv),
    0xac00..=0xd7a3 => Some(Hangul::Lvt),
    _ => None,
  }
}

/// Whether byte offset `i` of `s` is a grapheme cluster boundary, after Unicode's rules
/// (UAX #29) simplified: no break in `\r\n`, before extending characters and ZWJ, after ZWJ
/// (emoji sequences), inside Hangul syllables, or inside a pair of regional indicators (a
/// flag).
pub fn is_boundary(s: &str, i: usize) -> bool {
  if i == 0 || i >= s.len() {
    return i <= s.len();
  }

  if !s.is_char_boundary(i) {
    return false;
  }

  let (Some(a), Some(b)) = (s[..i].chars().next_back(), s[i..].chars().next()) else {
    return true;
  };
  if a == '\r' && b == '\n' {
    return false;
  }

  if is_control(a) || is_control(b) {
    return true;
  }

  if is_extend(b) || a == ZWJ {
    return false;
  }

  if let (Some(x), Some(y)) = (hangul(a), hangul(b)) {
    use Hangul::*;
    let joins = match x {
      L => y != T,
      V | Lv => y == V || y == T,
      T | Lvt => y == T,
    };

    if joins {
      return false;
    }
  }

  if is_regional(a) && is_regional(b) {
    // A flag is a pair: break after an even run of indicators.
    let run = s[..i].chars().rev().take_while(|&c| is_regional(c)).count();
    return run % 2 == 0;
  }

  true
}

fn next_char(s: &str, i: usize) -> usize {
  s[i..].chars().next().map_or(s.len(), |c| i + c.len_utf8())
}

/// The next grapheme boundary after `i` (the end at the end).
pub fn next_grapheme(s: &str, i: usize) -> usize {
  let mut j = next_char(s, i.min(s.len()));
  while !is_boundary(s, j) {
    j = next_char(s, j);
  }

  j
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Cluster boundaries of `s` from the start.
  fn clusters(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
      let j = next_grapheme(s, i);
      out.push(&s[i..j]);
      i = j;
    }

    out
  }

  #[test]
  fn grapheme_clusters() {
    let family = "👨\u{200d}👩\u{200d}👧";
    let cases: &[(&str, &[&str])] = &[
      ("abc", &["a", "b", "c"]),
      ("e\u{301}x", &["e\u{301}", "x"]),
      ("a\u{301}\u{302}", &["a\u{301}\u{302}"]),
      ("👍🏽!", &["👍🏽", "!"]),
      ("❤\u{fe0f}", &["❤\u{fe0f}"]),
      (family, &[family]),
      ("🇺🇸🇩🇪", &["🇺🇸", "🇩🇪"]),
      ("🇺🇸🇩", &["🇺🇸", "🇩"]),
      ("a\r\nb", &["a", "\r\n", "b"]),
      ("\n\u{301}", &["\n", "\u{301}"]),
      ("\u{1100}\u{1161}\u{11a8}가", &["\u{1100}\u{1161}\u{11a8}", "가"]),
      ("नमस्ते", &["न", "म", "स्", "ते"]),
      // Avagraha and khanda ta are letters among the marks.
      ("कऽ", &["क", "ऽ"]),
      ("কৎ", &["ক", "ৎ"]),
      (
        "🏴\u{e0067}\u{e0062}\u{e0073}\u{e0063}\u{e0074}\u{e007f}",
        &["🏴\u{e0067}\u{e0062}\u{e0073}\u{e0063}\u{e0074}\u{e007f}"],
      ),
      ("", &[]),
    ];
    for (s, want) in cases {
      assert_eq!(clusters(s), *want, "{s:?}");
    }
    // Inside a char or a cluster: not a boundary.
    let s = "xe\u{301}";
    assert!(!is_boundary(s, 2) && !is_boundary(s, 3) && is_boundary(s, 4) && !is_boundary(s, 9));
  }
}
