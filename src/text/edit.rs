// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The text editing model, pure logic with no OS: a UTF-8 buffer with a selection (anchor and
//! head, byte offsets on grapheme cluster boundaries), caret moves by character, word, line,
//! page and document, inserts and deletes, the clipboard through the [Clipboard] seam, undo and
//! redo that group a typing run into one step, and an input method's marked text. Moves by
//! visual line ask [Lines]: a text field's laid-out text implements it, [Mono] counts columns.
//! Grapheme cluster boundaries are here too, which the layout breaks words at; the UTF-16
//! helpers translate the ranges input methods speak in.
//!
//! The buffer's line break is `\n`: inserted `\r\n` and `\r` become `\n`, and a single-line
//! editor turns line breaks into spaces.

use std::ops::Range;

use crate::app::Clipboard;
use crate::keys::{Command, Delete, Move};

// MARK: Graphemes

const ZWJ: char = '\u{200d}';

/// Characters that join the cluster before them (Unicode's Grapheme_Extend and SpacingMark,
/// the common ranges): combining marks, Indic and Thai vowel signs, variation selectors, emoji
/// skin tones and tags, ZWNJ and ZWJ.
fn is_extend(c: char) -> bool {
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
  boundary(s, i, || s[..i].chars().rev().take_while(|&c| is_regional(c)).count())
}

/// [is_boundary], with `run` counting the regional indicators right before `i`: a walk through
/// the string keeps the count instead of counting back at every step.
fn boundary(s: &str, i: usize, run: impl FnOnce() -> usize) -> bool {
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
    return run().is_multiple_of(2);
  }

  true
}

fn next_char(s: &str, i: usize) -> usize {
  s[i..].chars().next().map_or(s.len(), |c| i + c.len_utf8())
}

/// The next grapheme boundary after `i` (the end at the end).
pub fn next_grapheme(s: &str, i: usize) -> usize {
  let i = i.min(s.len());
  // Inside a character: the end of that character, never a slice from the middle of it.
  let mut j = match s.is_char_boundary(i) {
    true => next_char(s, i),
    false => (i + 1..s.len()).find(|&j| s.is_char_boundary(j)).unwrap_or(s.len()),
  };

  while !is_boundary(s, j) {
    j = next_char(s, j);
  }

  j
}

fn prev_char(s: &str, i: usize) -> usize {
  s[..i].chars().next_back().map_or(0, |c| i - c.len_utf8())
}

/// The grapheme boundary before `i` (0 at the start).
pub fn prev_grapheme(s: &str, i: usize) -> usize {
  let mut j = prev_char(s, s.floor_char_boundary(i));
  while !is_boundary(s, j) {
    j = prev_char(s, j);
  }

  j
}

/// `i` clamped to `s` and moved back to a grapheme boundary.
pub fn snap(s: &str, i: usize) -> usize {
  let mut j = s.floor_char_boundary(i);
  while !is_boundary(s, j) {
    j = prev_char(s, j);
  }

  j
}

/// The grapheme clusters of `s` from the start, as byte ranges: [next_grapheme] in turn, in one
/// pass (a run of flags isn't counted again at each).
pub fn graphemes(s: &str) -> impl Iterator<Item = Range<usize>> + '_ {
  let (mut i, mut run) = (0, 0);
  std::iter::from_fn(move || {
    let start = i;
    while let Some(c) = s[i..].chars().next() {
      run = if is_regional(c) { run + 1 } else { 0 };
      i += c.len_utf8();
      if boundary(s, i, || run) {
        break;
      }
    }

    (i > start).then_some(start..i)
  })
}

// MARK: Words and lines

/// Whether the character at `i` is part of a word: letters, digits, `_`, marks on them, and an
/// apostrophe between letters ("don't").
fn word_at(s: &str, i: usize) -> bool {
  let Some(c) = s[i..].chars().next() else { return false };
  if c.is_alphanumeric() || c == '_' || is_extend(c) {
    return true;
  }

  let alnum = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
  matches!(c, '\'' | '’')
    && alnum(s[..i].chars().next_back())
    && alnum(s[i + c.len_utf8()..].chars().next())
}

/// The end of the word at or after `i`, past what separates them (Option-Right); by
/// graphemes, each one a word's by its first character, so an emoji sequence is passed whole.
pub fn word_right(s: &str, i: usize) -> usize {
  let mut j = snap(s, i);
  while j < s.len() && !word_at(s, j) {
    j = next_grapheme(s, j);
  }

  while j < s.len() && word_at(s, j) {
    j = next_grapheme(s, j);
  }

  j
}

/// The start of the word at or before `i` (Option-Left).
pub fn word_left(s: &str, i: usize) -> usize {
  let mut j = snap(s, i);
  while j > 0 && !word_at(s, prev_grapheme(s, j)) {
    j = prev_grapheme(s, j);
  }

  while j > 0 && word_at(s, prev_grapheme(s, j)) {
    j = prev_grapheme(s, j);
  }

  j
}

/// The start of the line between line breaks holding `i`.
pub fn para_start(s: &str, i: usize) -> usize {
  s[..i].rfind('\n').map_or(0, |p| p + 1)
}

/// The end of the line between line breaks holding `i`, before its `\n`.
pub fn para_end(s: &str, i: usize) -> usize {
  s[i..].find('\n').map_or(s.len(), |p| i + p)
}

/// The word, run of spaces or other grapheme holding the grapheme at `i` (a double click); at
/// the end, the last one's.
pub fn word_range(s: &str, i: usize) -> Range<usize> {
  let mut i = snap(s, i);
  if i == s.len() && i > 0 {
    i = prev_grapheme(s, i);
  }

  let blank = |j: usize| s[j..].starts_with([' ', '\t']);
  let class = |j: usize| match () {
    _ if word_at(s, j) => 1,
    _ if blank(j) => 2,
    _ => 0,
  };
  let c = class(i);
  if i >= s.len() || c == 0 {
    return i..next_grapheme(s, i);
  }

  // By graphemes, each one classed by its first character: an emoji's modifier is not a word's.
  let (mut a, mut b) = (i, i);
  while a > 0 && class(prev_grapheme(s, a)) == c {
    a = prev_grapheme(s, a);
  }

  while b < s.len() && class(b) == c {
    b = next_grapheme(s, b);
  }

  a..b
}

/// The line between line breaks at `i` with its `\n` (a triple click).
pub fn para_range(s: &str, i: usize) -> Range<usize> {
  let end = para_end(s, i);
  para_start(s, i)..(end + 1).min(s.len())
}

/// Where the text's visual lines are, for moves by line. A text field's laid-out text
/// implements it; [Mono] counts columns.
pub trait Lines {
  /// The visual line holding offset `i`, as a byte range without its line break. An offset at
  /// a soft wrap belongs to the line it starts.
  fn line(&self, s: &str, i: usize) -> Range<usize>;
  /// x of offset `i` from the start of its line, in points.
  fn x(&self, s: &str, i: usize) -> f32;
  /// The offset in `line` nearest to `x`.
  fn hit(&self, s: &str, line: Range<usize>, x: f32) -> usize;
  /// Visual lines a page moves by; 0 moves to the document's start or end.
  fn page(&self) -> usize {
    0
  }
}

/// A layout in columns: every grapheme is one column, lines wrap every `cols` graphemes (0
/// never wraps), `page` lines a page. For tests, and fields with no layout yet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mono {
  pub cols: usize,
  pub page: usize,
}

impl Mono {
  /// No wrapping: every line between line breaks is one visual line.
  pub const PLAIN: Self = Self { cols: 0, page: 0 };
}

impl Lines for Mono {
  fn line(&self, s: &str, i: usize) -> Range<usize> {
    let (ps, pe) = (para_start(s, i), para_end(s, i));
    if self.cols == 0 {
      return ps..pe;
    }

    let (mut start, mut j, mut n) = (ps, ps, 0);
    while j < pe {
      if n == self.cols {
        if i < j {
          return start..j;
        }

        (start, n) = (j, 0);
      }

      j = next_grapheme(s, j);
      n += 1;
    }

    start..pe
  }

  fn x(&self, s: &str, i: usize) -> f32 {
    let mut j = self.line(s, i).start;
    let mut n = 0;
    while j < i {
      j = next_grapheme(s, j);
      n += 1;
    }

    n as f32
  }

  fn hit(&self, s: &str, line: Range<usize>, x: f32) -> usize {
    let mut j = line.start;
    for _ in 0..x.round().max(0.0) as usize {
      if j >= line.end {
        break;
      }

      j = next_grapheme(s, j);
    }

    j.min(line.end)
  }

  fn page(&self) -> usize {
    self.page
  }
}

/// Whether `line` ends at a soft wrap rather than a line break or the end.
fn soft(s: &str, line: &Range<usize>) -> bool {
  line.end < s.len() && !s[line.end..].starts_with('\n')
}

/// The caret position at the end of `line`: before the last grapheme at a soft wrap, as the
/// wrap point itself shows at the start of the next line.
fn line_end(s: &str, line: Range<usize>) -> usize {
  match soft(s, &line) {
    true => prev_grapheme(s, line.end).max(line.start),
    false => line.end,
  }
}

// MARK: Editor

/// A selection: `anchor` stays where it began, `head` is the caret. Empty when they are equal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sel {
  pub anchor: usize,
  pub head: usize,
}

impl Sel {
  pub const fn caret(i: usize) -> Self {
    Self { anchor: i, head: i }
  }

  pub const fn new(anchor: usize, head: usize) -> Self {
    Self { anchor, head }
  }

  pub fn range(&self) -> Range<usize> {
    self.anchor.min(self.head)..self.anchor.max(self.head)
  }

  pub fn is_empty(&self) -> bool {
    self.anchor == self.head
  }
}

/// One undoable change: `removed` at `at` became `inserted`.
#[derive(Clone, Debug)]
struct Step {
  at: usize,
  removed: String,
  inserted: String,
  before: Sel,
  after: Sel,
}

/// Edits that grow the last undo step while they follow each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Run {
  Type,
  Back,
  Forward,
}

/// An input method's marked text: a range of the buffer, what marking replaced and the
/// selection before it, for the undo step its commit makes.
#[derive(Clone, Debug)]
struct Marked {
  range: Range<usize>,
  removed: String,
  before: Sel,
}

/// A text buffer with a selection and undo history. Single-line by default: line breaks
/// inserted become spaces and [Command::InsertNewline] / [Command::InsertTab] are left to the
/// app (submit, focus).
///
/// Marked text (an input method's composition, [Editor::set_marked]) is part of the buffer
/// until committed; any other edit, move or selection commits it first, as typed.
#[derive(Clone, Debug, Default)]
pub struct Editor {
  text: String,
  sel: Sel,
  /// The x that vertical moves keep, set by the first of them.
  goal: Option<f32>,
  pub multiline: bool,
  undo: Vec<Step>,
  redo: Vec<Step>,
  /// The run the last undo step belongs to, while it can grow.
  run: Option<Run>,
  marked: Option<Marked>,
}

impl Editor {
  /// A single-line editor with the caret at the end.
  pub fn new(text: &str) -> Self {
    let mut ed = Self::default();
    ed.set_text(text);
    ed
  }

  /// A multi-line editor with the caret at the end.
  pub fn multiline(text: &str) -> Self {
    let mut ed = Self { multiline: true, ..Self::default() };
    ed.set_text(text);
    ed
  }

  pub fn text(&self) -> &str {
    &self.text
  }

  pub fn sel(&self) -> Sel {
    self.sel
  }

  /// The caret: the selection's head.
  pub fn caret(&self) -> usize {
    self.sel.head
  }

  pub fn selected(&self) -> &str {
    &self.text[self.sel.range()]
  }

  /// Replaces the whole text, forgetting the history; the caret goes to the end.
  pub fn set_text(&mut self, s: &str) {
    self.text = self.clean(s);
    self.sel = Sel::caret(self.text.len());
    (self.goal, self.run, self.marked) = (None, None, None);
    self.undo.clear();
    self.redo.clear();
  }

  /// Selects `sel`, clamped and snapped to grapheme boundaries.
  pub fn select(&mut self, sel: Sel) {
    self.unmark();
    self.sel = Sel::new(snap(&self.text, sel.anchor), snap(&self.text, sel.head));
    (self.goal, self.run) = (None, None);
  }

  pub fn select_all(&mut self) {
    self.select(Sel::new(0, self.text.len()));
  }

  pub fn can_undo(&self) -> bool {
    !self.undo.is_empty()
  }

  pub fn can_redo(&self) -> bool {
    !self.redo.is_empty()
  }

  // MARK: Moves

  /// Moves the caret, extending the selection when `select`. Without it a selection collapses:
  /// Left and Up to its start, Right and Down to its end, word and line moves go from there.
  pub fn move_by(&mut self, m: Move, select: bool, l: &dyn Lines) {
    self.unmark();
    let s = &self.text;
    let r = self.sel.range();
    let collapse = !select && !self.sel.is_empty();
    let back = matches!(
      m,
      Move::Left
        | Move::WordLeft
        | Move::LineStart
        | Move::ParagraphStart
        | Move::Up
        | Move::PageUp
        | Move::DocStart
    );
    let from = match (collapse, back) {
      (true, true) => r.start,
      (true, false) => r.end,
      (false, _) => self.sel.head,
    };

    let page = match l.page() {
      0 => usize::MAX,
      n => n,
    };

    let mut goal = None;
    let to = match m {
      Move::Left | Move::Right if collapse => from,
      Move::Left => prev_grapheme(s, from),
      Move::Right => next_grapheme(s, from),
      Move::WordLeft => word_left(s, from),
      Move::WordRight => word_right(s, from),
      Move::LineStart => l.line(s, from).start,
      Move::LineEnd => line_end(s, l.line(s, from)),
      Move::ParagraphStart => para_start(s, from),
      Move::ParagraphEnd => para_end(s, from),
      Move::Up | Move::Down | Move::PageUp | Move::PageDown => {
        let x = self.goal.unwrap_or_else(|| l.x(s, from));
        goal = Some(x);
        let n = if matches!(m, Move::Up | Move::Down) { 1 } else { page };
        vertical(s, l, from, x, n, back)
      }
      Move::DocStart => 0,
      Move::DocEnd => s.len(),
    };

    self.sel = if select { Sel::new(self.sel.anchor, to) } else { Sel::caret(to) };
    (self.goal, self.run) = (goal, None);
  }

  // MARK: Edits

  /// Types `s` over the selection; consecutive typing is one undo step.
  pub fn insert(&mut self, s: &str) {
    self.insert_as(s, Some(Run::Type));
  }

  /// Replaces `r` (snapped to grapheme boundaries) with `s` as its own undo step, the caret
  /// after it: what an input method's `insertText:replacementRange:` does.
  pub fn replace(&mut self, r: Range<usize>, s: &str) {
    self.unmark();
    let r = snap(&self.text, r.start)..snap(&self.text, r.end.max(r.start));
    let s = self.clean(s);
    self.edit(r, &s, None);
  }

  fn insert_as(&mut self, s: &str, run: Option<Run>) {
    self.unmark();
    let s = self.clean(s);
    if s.is_empty() && self.sel.is_empty() {
      return;
    }

    self.edit(self.sel.range(), &s, run);
  }

  /// Deletes the selection, or what `d` reaches from the caret. Returns whether anything went.
  pub fn delete(&mut self, d: Delete, l: &dyn Lines) -> bool {
    self.unmark();
    if !self.sel.is_empty() {
      self.edit(self.sel.range(), "", None);
      return true;
    }

    let (s, i) = (&self.text, self.sel.head);
    let r = match d {
      Delete::Backward => prev_grapheme(s, i)..i,
      Delete::Forward => i..next_grapheme(s, i),
      Delete::WordBackward => word_left(s, i)..i,
      Delete::WordForward => i..word_right(s, i),
      Delete::ToLineStart => l.line(s, i).start.min(i)..i,
      Delete::ToLineEnd => i..line_end(s, l.line(s, i)).max(i),
      Delete::ToParagraphStart => para_start(s, i)..i,
      // At a line's end Ctrl-K joins the next line.
      Delete::ToParagraphEnd => match para_end(s, i) {
        e if e == i && i < s.len() => i..i + 1,
        e => i..e,
      },
    };

    if r.is_empty() {
      return false;
    }

    let run = match d {
      Delete::Backward => Some(Run::Back),
      Delete::Forward => Some(Run::Forward),
      _ => None,
    };

    self.edit(r, "", run);
    true
  }

  /// Puts the selection on the clipboard; false when nothing is selected.
  pub fn copy(&self, cb: &mut dyn Clipboard) -> bool {
    if self.sel.is_empty() {
      return false;
    }

    cb.write(self.selected());
    true
  }

  pub fn cut(&mut self, cb: &mut dyn Clipboard) -> bool {
    self.unmark();
    if !self.copy(cb) {
      return false;
    }

    self.edit(self.sel.range(), "", None);
    true
  }

  /// Inserts the clipboard's text over the selection, as its own undo step.
  pub fn paste(&mut self, cb: &dyn Clipboard) -> bool {
    let Some(s) = cb.read().filter(|s| !s.is_empty()) else { return false };
    self.insert_as(&s, None);
    true
  }

  pub fn undo(&mut self) -> bool {
    self.unmark();
    let Some(st) = self.undo.pop() else { return false };
    self.text.replace_range(st.at..st.at + st.inserted.len(), &st.removed);
    self.sel = st.before;
    (self.goal, self.run) = (None, None);
    self.redo.push(st);
    true
  }

  pub fn redo(&mut self) -> bool {
    self.unmark();
    let Some(st) = self.redo.pop() else { return false };
    self.text.replace_range(st.at..st.at + st.removed.len(), &st.inserted);
    self.sel = st.after;
    (self.goal, self.run) = (None, None);
    self.undo.push(st);
    true
  }

  // MARK: Marked text

  /// The marked text, while an input method composes.
  pub fn marked(&self) -> Option<Range<usize>> {
    self.marked.as_ref().map(|m| m.range.clone())
  }

  /// Marks `s`, an input method's composition, in place of the marked text, or `replace` (a
  /// range of the buffer), or the selection; `sel`, a range of `s`, becomes the selection. Not
  /// an undo step until committed. Empty, it ends marking with the text it replaced gone.
  pub fn set_marked(&mut self, s: &str, sel: Range<usize>, replace: Option<Range<usize>>) {
    let s = self.clean(s);
    let r = match (&self.marked, replace) {
      (Some(m), None) => m.range.clone(),
      (Some(m), Some(r)) if m.range.start <= r.start && r.end <= m.range.end => r,
      (Some(_), Some(r)) => {
        self.unmark();
        r
      }
      (None, Some(r)) => r,
      (None, None) => self.sel.range(),
    };
    // Char boundaries only: marked text may start or end inside a cluster (a combining mark or
    // ZWJ after it), and snapping back would leave part of it in place, duplicated.
    let (t, end) = (&self.text, r.end.max(r.start));
    let r = t.floor_char_boundary(r.start)..t.floor_char_boundary(end);
    let m = self.marked.get_or_insert_with(|| Marked {
      range: r.clone(),
      removed: self.text[r.clone()].to_owned(),
      before: self.sel,
    });
    m.range.end = m.range.end + s.len() - r.len();
    // `sel` is within `s`, which lands at the start of what it replaces.
    let start = r.start;
    self.text.replace_range(r, &s);
    let (a, b) = (start + snap(&s, sel.start), start + snap(&s, sel.end.max(sel.start)));
    self.sel = Sel::new(a, b);
    (self.goal, self.run) = (None, None);
    if s.is_empty() && self.marked.as_ref().is_some_and(|m| m.range.is_empty()) {
      self.commit("", None);
    }
  }

  /// Commits `s` in place of the marked text (one undo step from before marking), or of
  /// `replace`, or the selection, as typing: an input method's `insertText:`.
  pub fn commit(&mut self, s: &str, replace: Option<Range<usize>>) {
    let Some(m) = self.marked.take() else {
      match replace {
        Some(r) => self.replace(r, s),
        None => self.insert(s),
      }

      return;
    };
    if let Some(r) = replace.filter(|r| *r != m.range) {
      // Part of the marked text: spliced in at its characters, the whole committed in one step.
      if m.range.start <= r.start && r.end <= m.range.end {
        let t = &self.text;
        let (a, b) = (t.floor_char_boundary(r.start), t.floor_char_boundary(r.end.max(r.start)));
        let mut all = t[m.range.clone()].to_owned();
        all.replace_range(a - m.range.start..b - m.range.start, s);
        self.marked = Some(m);
        return self.commit(&all, None);
      }

      self.marked = Some(m);
      self.unmark();
      return self.replace(r, s);
    }

    let s = self.clean(s);
    self.text.replace_range(m.range.clone(), &s);
    let after = Sel::caret(settle(&self.text, m.range.start + s.len()));
    self.sel = after;
    (self.goal, self.run) = (None, None);
    if m.removed != s {
      self.redo.clear();
      let step =
        Step { at: m.range.start, removed: m.removed, inserted: s, before: m.before, after };
      self.undo.push(step);
    }
  }

  /// Drops the marked text and puts back what it replaced, with the selection from before:
  /// as if nothing was composed, no undo step.
  pub fn discard_marked(&mut self) {
    if let Some(m) = self.marked.take() {
      self.text.replace_range(m.range, &m.removed);
      (self.sel, self.goal, self.run) = (m.before, None, None);
    }
  }

  /// Keeps the marked text as it is (`unmarkText`).
  pub fn unmark(&mut self) {
    if let Some(m) = &self.marked {
      // The input method's selection in it stays: a cut or a paste then acts on it.
      let (s, sel) = (self.text[m.range.clone()].to_owned(), self.sel);
      self.commit(&s, None);
      if !sel.is_empty() {
        self.sel = Sel::new(snap(&self.text, sel.anchor), snap(&self.text, sel.head));
      }
    }
  }

  /// Runs an editing command; returns whether it was the editor's. Moves, deletes, undo and
  /// redo are taken even when there is nothing to do (a focused field keeps its keys); copy and
  /// cut with nothing selected, and a single-line editor's newline and tab, are not.
  pub fn apply(&mut self, cmd: Command, l: &dyn Lines, cb: &mut dyn Clipboard) -> bool {
    match cmd {
      Command::Move(m, select) => self.move_by(m, select, l),
      Command::Delete(d) => _ = self.delete(d, l),
      Command::InsertNewline if self.multiline => self.insert("\n"),
      Command::InsertTab if self.multiline => self.insert("\t"),
      Command::SelectAll => self.select_all(),
      Command::Copy => return self.copy(cb),
      Command::Cut => return self.cut(cb),
      Command::Paste => _ = self.paste(cb),
      Command::Undo => _ = self.undo(),
      Command::Redo => _ = self.redo(),
      _ => return false,
    }

    true
  }

  /// Line breaks as `\n`, or spaces when single-line.
  fn clean(&self, s: &str) -> String {
    if !s.contains('\r') && (self.multiline || !s.contains('\n')) {
      return s.to_owned();
    }

    let s = s.replace("\r\n", "\n").replace('\r', "\n");
    match self.multiline {
      true => s,
      false => s.replace('\n', " "),
    }
  }

  /// Replaces `r` with `s`, the caret after it, and records the step, growing the last one
  /// when this continues its run: typing at its end, Backspace at its start, Delete at its
  /// place.
  fn edit(&mut self, r: Range<usize>, s: &str, run: Option<Run>) {
    let before = self.sel;
    let removed = self.text[r.clone()].to_owned();
    self.text.replace_range(r.clone(), s);
    let after = Sel::caret(settle(&self.text, r.start + s.len()));
    self.sel = after;
    self.goal = None;
    self.redo.clear();
    if let Some(last) = self.undo.last_mut()
      && run.is_some()
      && run == self.run
    {
      let grown = match run {
        Some(Run::Type) if removed.is_empty() && last.at + last.inserted.len() == r.start => {
          last.inserted.push_str(s);
          true
        }
        Some(Run::Back) if last.inserted.is_empty() && r.end == last.at => {
          last.removed.insert_str(0, &removed);
          last.at = r.start;
          true
        }
        Some(Run::Forward) if last.inserted.is_empty() && r.start == last.at => {
          last.removed.push_str(&removed);
          true
        }
        _ => false,
      };

      if grown {
        last.after = after;
        return;
      }
    }
    self.undo.push(Step { at: r.start, removed, inserted: s.to_owned(), before, after });
    self.run = run;
  }
}

/// `i`, or the end of the grapheme it is inside after an edit joined clusters around it (a ZWJ
/// pasted between two emoji).
fn settle(s: &str, i: usize) -> usize {
  if is_boundary(s, i) { i } else { next_grapheme(s, i) }
}

/// The offset `n` visual lines up (`back`) or down from `from`, nearest to `x`; past the first
/// or last line it is the document's start or end.
fn vertical(s: &str, l: &dyn Lines, from: usize, x: f32, n: usize, back: bool) -> usize {
  let mut line = l.line(s, from);
  for _ in 0..n {
    line = match back {
      true if line.start == 0 => return 0,
      true => l.line(s, prev_grapheme(s, line.start)),
      false if line.end >= s.len() => return s.len(),
      false if soft(s, &line) => l.line(s, line.end),
      false => l.line(s, line.end + 1),
    };
  }

  let i = l.hit(s, line.clone(), x);
  match soft(s, &line) && i >= line.end {
    true => line_end(s, line),
    false => i,
  }
}

// MARK: UTF-16

/// Length of `s` in UTF-16 code units, as NSString counts.
pub fn utf16_len(s: &str) -> usize {
  s.chars().map(char::len_utf16).sum()
}

/// The UTF-16 offset of byte offset `i` (moved back to a char boundary).
pub fn to_utf16(s: &str, i: usize) -> usize {
  utf16_len(&s[..s.floor_char_boundary(i)])
}

/// The byte offset of UTF-16 offset `u`: inside a surrogate pair it is the char's start, past
/// the end the length.
pub fn from_utf16(s: &str, u: usize) -> usize {
  let mut n = 0;
  for (i, c) in s.char_indices() {
    n += c.len_utf16();
    if n > u {
      return i;
    }
  }

  s.len()
}

pub fn range_to_utf16(s: &str, r: Range<usize>) -> Range<usize> {
  to_utf16(s, r.start)..to_utf16(s, r.end)
}

pub fn range_from_utf16(s: &str, r: Range<usize>) -> Range<usize> {
  from_utf16(s, r.start)..from_utf16(s, r.end)
}

/// [from_utf16] for a shaper's many lookups (CoreText's and DirectWrite's string indices): a
/// table of the byte offsets by UTF-16 offset, none for ASCII, where the two are the same.
pub struct Utf16Bytes<'a> {
  s: &'a str,
  bytes: Vec<u32>,
}

impl<'a> Utf16Bytes<'a> {
  pub fn new(s: &'a str) -> Self {
    let bytes = match s.is_ascii() {
      true => Vec::new(),
      false => s
        .char_indices()
        .flat_map(|(i, c)| std::iter::repeat_n(i as u32, c.len_utf16()))
        .chain([s.len() as u32])
        .collect(),
    };

    Self { s, bytes }
  }

  /// The byte offset of UTF-16 offset `u`, as [from_utf16] gives it.
  pub fn at(&self, u: usize) -> u32 {
    match self.bytes.is_empty() {
      true => u.min(self.s.len()) as u32,
      false => self.bytes.get(u).copied().unwrap_or(self.s.len() as u32),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::MemClipboard;

  /// Cluster boundaries of `s` from the start.
  fn clusters(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
      let j = next_grapheme(s, i);
      out.push(&s[i..j]);
      i = j;
    }

    let walked: Vec<&str> = graphemes(s).map(|r| &s[r]).collect();
    assert_eq!(walked, out, "{s:?}: one pass, the same clusters");
    // Backward gives the same.
    let (mut back, mut j) = (Vec::new(), s.len());
    while j > 0 {
      let i = prev_grapheme(s, j);
      back.push(&s[i..j]);
      j = i;
    }

    back.reverse();
    assert_eq!(out, back, "{s:?}: backward");
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
      ("a🇺🇸🇩🇪🇫", &["a", "🇺🇸", "🇩🇪", "🇫"]),
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
    // Snapping goes back to the boundary before.
    assert_eq!((snap(s, 2), snap(s, 3), snap(s, 99)), (1, 1, 4));
  }

  /// From inside a character: the next boundary after it, without slicing into it.
  #[test]
  fn next_grapheme_from_inside_a_char() {
    assert_eq!(next_grapheme("éx", 1), 2);
    assert_eq!(next_grapheme("e\u{301}x", 2), 3);
    assert_eq!(next_grapheme("👍🏽!", 1), 8);
    assert_eq!(next_grapheme("é", 1), 2);
  }

  /// A long run of flags walks in one pass: counted back at each step, 10,000 flags took some
  /// 200 million steps.
  #[test]
  fn graphemes_walk_flags_in_one_pass() {
    let s = "🇺🇦".repeat(10_000);
    let t = std::time::Instant::now();
    assert_eq!(graphemes(&s).count(), 10_000);
    assert!(t.elapsed().as_millis() < 500, "{:?}", t.elapsed());
  }

  #[test]
  fn word_boundaries() {
    let s = "Hello, world! don't  foo_bar 42";
    let rights: Vec<usize> = std::iter::successors(Some(0), |&i| {
      let j = word_right(s, i);
      (j != i).then_some(j)
    })
    .collect();
    assert_eq!(rights, [0, 5, 12, 19, 28, 31]);
    let lefts: Vec<usize> = std::iter::successors(Some(s.len()), |&i| {
      let j = word_left(s, i);
      (j != i).then_some(j)
    })
    .collect();
    assert_eq!(lefts, [31, 29, 21, 14, 7, 0]);
    // From inside a word: its end or start.
    assert_eq!((word_right(s, 2), word_left(s, 2)), (5, 0));
    // Accented words stay whole.
    let s = "cafe\u{301} ok";
    assert_eq!((word_right(s, 0), word_left(s, 6)), (6, 0));
  }

  #[test]
  fn mono_layout_wraps_by_columns() {
    let l = Mono { cols: 3, page: 0 };
    let s = "abcdefg\nhi";
    assert_eq!(l.line(s, 0), 0..3);
    assert_eq!(l.line(s, 3), 3..6);
    assert_eq!(l.line(s, 7), 6..7);
    assert_eq!(l.line(s, 8), 8..10);
    assert_eq!(l.line(s, 10), 8..10);
    assert_eq!((l.x(s, 5), l.x(s, 6), l.x(s, 10)), (2.0, 0.0, 2.0));
    assert_eq!(l.hit(s, 3..6, 1.4), 4);
    assert_eq!(l.hit(s, 8..10, 9.0), 10);
    assert_eq!(Mono::PLAIN.line(s, 4), 0..7);
  }

  fn ed(s: &str, caret: usize) -> Editor {
    let mut e = Editor::multiline(s);
    e.select(Sel::caret(caret));
    e
  }

  fn mv(e: &mut Editor, m: Move, select: bool) -> usize {
    e.move_by(m, select, &Mono::PLAIN);
    e.caret()
  }

  #[test]
  fn moves_by_char_word_line_and_document() {
    let mut e = ed("a👍🏽b\ncafe\u{301} bar", 0);
    assert_eq!(mv(&mut e, Move::Right, false), 1);
    assert_eq!(mv(&mut e, Move::Right, false), 9); // over the emoji with its tone
    assert_eq!(mv(&mut e, Move::Left, false), 1);
    assert_eq!(mv(&mut e, Move::LineEnd, false), 10);
    assert_eq!(mv(&mut e, Move::Right, false), 11); // over the line break
    assert_eq!(mv(&mut e, Move::WordRight, false), 17);
    assert_eq!(mv(&mut e, Move::WordRight, false), 21);
    assert_eq!(mv(&mut e, Move::ParagraphStart, false), 11);
    assert_eq!(mv(&mut e, Move::Left, false), 10);
    assert_eq!(mv(&mut e, Move::DocEnd, false), 21);
    assert_eq!(mv(&mut e, Move::Right, false), 21);
    assert_eq!(mv(&mut e, Move::DocStart, false), 0);
    assert_eq!(mv(&mut e, Move::Left, false), 0);
  }

  #[test]
  fn selection_extends_and_collapses() {
    let mut e = ed("hello world", 6);
    mv(&mut e, Move::WordRight, true);
    assert_eq!((e.sel(), e.selected()), (Sel::new(6, 11), "world"));
    mv(&mut e, Move::Left, true);
    mv(&mut e, Move::Left, true);
    assert_eq!(e.selected(), "wor");
    // Past the anchor the selection flips.
    mv(&mut e, Move::WordLeft, true);
    mv(&mut e, Move::WordLeft, true);
    assert_eq!((e.sel(), e.selected()), (Sel::new(6, 0), "hello "));
    // Without Shift: Left goes to the start, Right to the end.
    let mut c = e.clone();
    assert_eq!((mv(&mut c, Move::Left, false), c.sel().is_empty()), (0, true));
    assert_eq!(mv(&mut e, Move::Right, false), 6);
    e.select(Sel::new(2, 8));
    assert_eq!(mv(&mut e, Move::WordRight, false), 11);
    e.select_all();
    assert_eq!(e.sel(), Sel::new(0, 11));
  }

  #[test]
  fn vertical_moves_keep_the_goal_column() {
    let mut e = ed("long line\nab\nanother one", 7);
    assert_eq!(mv(&mut e, Move::Down, false), 12); // end of "ab"
    assert_eq!(mv(&mut e, Move::Down, false), 20); // column 7 again
    assert_eq!(mv(&mut e, Move::Up, false), 12);
    assert_eq!(mv(&mut e, Move::Up, false), 7);
    // Past the first line: the start; past the last: the end.
    assert_eq!(mv(&mut e, Move::Up, false), 0);
    assert_eq!(mv(&mut e, Move::DocEnd, false), 24);
    assert_eq!(mv(&mut e, Move::Down, false), 24);
    // Another move forgets the goal.
    e.select(Sel::caret(1));
    mv(&mut e, Move::Down, false);
    mv(&mut e, Move::Right, false);
    assert_eq!(mv(&mut e, Move::Down, false), 15);
    // Up with a selection starts from its start; Shift-Down extends.
    e.select(Sel::new(16, 12));
    assert_eq!(mv(&mut e, Move::Up, false), 2);
    assert_eq!((mv(&mut e, Move::Down, true), e.sel().anchor), (12, 2));
  }

  #[test]
  fn vertical_moves_over_wrapped_lines() {
    // Visual lines: "abcd" "efgh" "ij" | "kl".
    let l = Mono { cols: 4, page: 2 };
    let mut e = ed("abcdefghij\nkl", 1);
    let down = |e: &mut Editor| {
      e.move_by(Move::Down, false, &l);
      e.caret()
    };
    assert_eq!(down(&mut e), 5);
    assert_eq!(down(&mut e), 9);
    assert_eq!(down(&mut e), 12);
    // Line ends at a wrap stay on their line; Line Start goes to the visual start.
    e.select(Sel::caret(5));
    e.move_by(Move::LineEnd, false, &l);
    assert_eq!(e.caret(), 7);
    e.move_by(Move::LineStart, false, &l);
    assert_eq!(e.caret(), 4);
    // A column past a wrapped line's end lands before the wrap.
    e.select(Sel::caret(13));
    e.move_by(Move::Up, false, &l);
    assert_eq!(e.caret(), 10);
    e.select(Sel::caret(3));
    e.move_by(Move::LineEnd, false, &l);
    e.move_by(Move::Down, false, &l);
    assert_eq!(e.caret(), 7);
    // A page is two lines.
    e.select(Sel::caret(0));
    e.move_by(Move::PageDown, false, &l);
    assert_eq!(e.caret(), 8);
    e.move_by(Move::PageUp, true, &l);
    assert_eq!(e.sel(), Sel::new(8, 0));
  }

  #[test]
  fn inserts_replace_the_selection() {
    let mut e = Editor::new("hello");
    e.select(Sel::new(1, 4));
    e.insert("ipp");
    assert_eq!((e.text(), e.caret()), ("hippo", 4));
    // Single-line: line breaks become spaces; multi-line keeps them as \n.
    e.insert("a\r\nb\rc\n");
    assert_eq!(e.text(), "hippa b c o");
    let mut m = Editor::multiline("");
    m.insert("a\r\nb\rc");
    assert_eq!(m.text(), "a\nb\nc");
    m.replace(1..3, "-");
    assert_eq!((m.text(), m.caret()), ("a-\nc", 2));
  }

  #[test]
  fn deletes() {
    let l = Mono::PLAIN;
    let mut e = ed("one two👍🏽\nthree", 15);
    assert!(e.delete(Delete::Backward, &l));
    assert_eq!(e.text(), "one two\nthree");
    e.select(Sel::caret(7));
    e.delete(Delete::WordBackward, &l);
    assert_eq!((e.text(), e.caret()), ("one \nthree", 4));
    e.delete(Delete::ToParagraphEnd, &l);
    assert_eq!(e.text(), "one three");
    e.delete(Delete::WordForward, &l);
    assert_eq!(e.text(), "one ");
    e.delete(Delete::ToLineStart, &l);
    assert_eq!((e.text(), e.caret()), ("", 0));
    assert!(!e.delete(Delete::Backward, &l) && !e.delete(Delete::Forward, &l));
    // A selection goes as a whole, whatever the delete.
    let mut e = ed("abcdef", 0);
    e.select(Sel::new(1, 3));
    e.delete(Delete::Forward, &l);
    assert_eq!(e.text(), "adef");
    e.delete(Delete::Forward, &l);
    assert_eq!(e.text(), "aef");
  }

  #[test]
  fn undo_groups_typing_runs() {
    let l = Mono::PLAIN;
    let mut e = Editor::new("");
    for c in ["h", "e", "l", "l", "o"] {
      e.insert(c);
    }

    e.move_by(Move::Left, false, &l);
    e.insert("!");
    e.move_by(Move::DocEnd, false, &l);
    for _ in 0..3 {
      e.delete(Delete::Backward, &l);
    }

    assert_eq!(e.text(), "hel");
    // The three backspaces, the "!", then the whole "hello".
    assert!(e.undo());
    assert_eq!((e.text(), e.caret()), ("hell!o", 6));
    assert!(e.undo());
    assert_eq!((e.text(), e.caret()), ("hello", 4));
    assert!(e.undo());
    assert_eq!((e.text(), e.caret()), ("", 0));
    assert!(!e.undo());
    assert!(e.redo() && e.redo());
    assert_eq!((e.text(), e.caret()), ("hell!o", 5));
    // A new edit drops what could be redone.
    e.insert("x");
    assert!(!e.can_redo());
    // Typing over a selection: undo brings it back, selected.
    let mut e = Editor::new("one two");
    e.select(Sel::new(4, 7));
    e.insert("t");
    e.insert("en");
    assert!(e.undo());
    assert_eq!((e.text(), e.sel()), ("one two", Sel::new(4, 7)));
    // Forward deletes group too.
    let mut e = ed("abcd", 0);
    e.delete(Delete::Forward, &l);
    e.delete(Delete::Forward, &l);
    e.undo();
    assert_eq!((e.text(), e.caret()), ("abcd", 0));
  }

  #[test]
  fn clipboard_and_commands() {
    let (l, mut cb) = (Mono::PLAIN, MemClipboard::default());
    let mut e = Editor::new("Hello");
    assert!(!e.apply(Command::Copy, &l, &mut cb));
    assert!(e.apply(Command::SelectAll, &l, &mut cb));
    assert!(e.apply(Command::Copy, &l, &mut cb));
    assert_eq!(cb.0.as_deref(), Some("Hello"));
    assert!(e.apply(Command::Cut, &l, &mut cb));
    assert_eq!(e.text(), "");
    e.apply(Command::Paste, &l, &mut cb);
    e.apply(Command::Paste, &l, &mut cb);
    assert_eq!(e.text(), "HelloHello");
    // Each paste is its own step.
    e.apply(Command::Undo, &l, &mut cb);
    assert_eq!(e.text(), "Hello");
    e.apply(Command::Move(Move::WordLeft, true), &l, &mut cb);
    e.apply(Command::Delete(Delete::Backward), &l, &mut cb);
    assert_eq!(e.text(), "");
    // Single-line editors leave Enter and Tab to the app; multi-line ones type them.
    assert!(!e.apply(Command::InsertNewline, &l, &mut cb));
    assert!(!e.apply(Command::Cancel, &l, &mut cb));
    let mut m = Editor::multiline("");
    assert!(
      m.apply(Command::InsertNewline, &l, &mut cb) && m.apply(Command::InsertTab, &l, &mut cb)
    );
    assert_eq!(m.text(), "\n\t");
  }

  #[test]
  fn word_and_paragraph_ranges() {
    let s = "one  two_3, cafe\u{301}!\nnext";
    assert_eq!(word_range(s, 1), 0..3);
    assert_eq!(word_range(s, 3), 3..5); // the spaces
    assert_eq!(word_range(s, 7), 5..10);
    assert_eq!(word_range(s, 10), 10..11); // the comma alone
    assert_eq!(word_range(s, 13), 12..18);
    assert_eq!(word_range(s, s.len()), 20..24); // past the end: the last word
    assert_eq!(para_range(s, 4), 0..20);
    assert_eq!(para_range(s, 21), 20..24);
  }

  #[test]
  fn marked_text_commits_as_one_undo_step() {
    let mut e = Editor::new("ab");
    // Composing: each mark replaces the last; the selection is inside it.
    e.set_marked("n", 1..1, None);
    e.set_marked("ni", 2..2, None);
    assert_eq!((e.text(), e.marked(), e.sel()), ("abni", Some(2..4), Sel::caret(4)));
    e.set_marked("にほ", 0..6, None);
    assert_eq!((e.text(), e.marked(), e.sel()), ("abにほ", Some(2..8), Sel::new(2, 8)));
    e.commit("日本", None);
    assert_eq!((e.text(), e.marked(), e.caret()), ("ab日本", None, 8));
    // One step back to before marking.
    assert!(e.undo());
    assert_eq!((e.text(), e.sel()), ("ab", Sel::caret(2)));
    assert!(e.redo());
    assert_eq!(e.text(), "ab日本");
    // Marking over a selection replaces it; an empty mark cancels, the selection gone.
    e.select(Sel::new(0, 2));
    e.set_marked("x", 1..1, None);
    assert_eq!(e.text(), "x日本");
    e.set_marked("", 0..0, None);
    assert_eq!((e.text(), e.marked()), ("日本", None));
    e.undo();
    assert_eq!(e.text(), "ab日本");
    // Any edit or move first keeps the marked text as typed.
    e.select(Sel::caret(e.text().len()));
    e.set_marked("q", 1..1, None);
    e.move_by(Move::DocStart, false, &Mono::PLAIN);
    assert_eq!((e.text(), e.marked(), e.caret()), ("ab日本q", None, 0));
    // A replacement range outside the marked text: commit it, then replace.
    e.select(Sel::caret(e.text().len()));
    e.set_marked("z", 1..1, None);
    e.commit("AB", Some(0..2));
    assert_eq!(e.text(), "AB日本qz");
  }

  #[test]
  fn a_mark_over_part_of_the_marked_text_selects_in_its_place() {
    let mut e = Editor::new("");
    e.set_marked("éx", 3..3, None);
    // The input method replaces "x" alone, its caret after the new "X".
    e.set_marked("X", 1..1, Some(2..3));
    assert_eq!((e.text(), e.marked(), e.sel()), ("éX", Some(0..3), Sel::caret(3)));
    e.select_all();
    assert_eq!(e.selected(), "éX");
  }

  #[test]
  fn an_edit_that_joins_clusters_leaves_the_caret_after_them() {
    // A ZWJ pasted between two emoji joins them: the caret goes after the pair, and Backspace
    // takes it whole.
    let mut e = Editor::new("👩👩");
    e.select(Sel::caret(4));
    e.insert("\u{200d}");
    assert_eq!(e.caret(), 11);
    assert!(e.delete(Delete::Backward, &Mono::PLAIN));
    assert_eq!(e.text(), "");
    // So does a commit.
    let mut e = Editor::new("👩👩");
    e.select(Sel::caret(4));
    e.set_marked("\u{200d}", 3..3, None);
    e.commit("\u{200d}", None);
    assert_eq!(e.caret(), 11);
    // A delete that makes a flag of two indicators.
    let mut e = Editor::new("🇺x🇸");
    e.select(Sel::caret(5));
    assert!(e.delete(Delete::Backward, &Mono::PLAIN));
    assert_eq!((e.text(), e.caret()), ("🇺🇸", 8));
  }

  #[test]
  fn a_commit_over_part_of_the_marked_text_keeps_the_rest() {
    let mut e = Editor::new("e");
    e.set_marked("\u{301}\u{302}", 4..4, None);
    e.commit("\u{303}", Some(3..5));
    assert_eq!((e.text(), e.marked()), ("e\u{301}\u{303}", None));
    assert!(e.undo());
    assert_eq!(e.text(), "e");
  }

  #[test]
  fn keeping_marked_text_keeps_its_selection() {
    let mut cb = MemClipboard::default();
    let mut e = Editor::new("");
    e.set_marked("abc", 0..2, None);
    assert!(e.cut(&mut cb));
    assert_eq!((e.text(), cb.0.as_deref()), ("c", Some("ab")));
  }

  #[test]
  fn a_double_click_beside_an_emoji_takes_the_word_alone() {
    let s = "👍🏽test";
    assert_eq!(word_range(s, 9), 8..12);
    assert_eq!(word_range(s, 0), 0..8);
  }

  #[test]
  fn keeping_a_joiner_leaves_the_caret_after_the_pair() {
    let mut e = Editor::new("👩👩");
    e.select(Sel::caret(4));
    e.set_marked("\u{200d}", 3..3, None);
    e.unmark();
    assert_eq!(e.caret(), 11);
  }

  #[test]
  fn word_moves_pass_emoji_sequences_whole() {
    let s = "👩\u{200d}💻 test";
    assert_eq!(word_right(s, 0), s.len());
    assert_eq!(word_left(s, s.len()), 12);
    assert_eq!(word_left(s, 12), 0);
  }

  #[test]
  fn marked_text_inside_a_cluster() {
    // A marked combining mark joins the letter before it; the next mark replaces only it.
    let mut e = Editor::new("e");
    e.set_marked("\u{301}", 2..2, None);
    e.set_marked("\u{301}\u{302}", 4..4, None);
    assert_eq!((e.text(), e.marked()), ("e\u{301}\u{302}", Some(1..5)));
    e.commit("\u{302}", None);
    assert_eq!((e.text(), e.caret()), ("e\u{302}", 3));
    // Marked text ending in ZWJ joins the emoji after it.
    let mut e = Editor::new("👩");
    e.select(Sel::caret(0));
    e.set_marked("👩\u{200d}", 7..7, None);
    e.set_marked("👨\u{200d}", 7..7, None);
    assert_eq!((e.text(), e.marked()), ("👨\u{200d}👩", Some(0..7)));
    e.commit("👨\u{200d}", None);
    assert_eq!(e.text(), "👨\u{200d}👩");
  }

  #[test]
  fn utf16_offsets() {
    let s = "aé👍🏽\n";
    // a: 1, é: 1, 👍: 2, 🏽: 2, \n: 1 code units; bytes 1, 2, 4, 4, 1.
    assert_eq!(utf16_len(s), 7);
    let pairs = [(0, 0), (1, 1), (3, 2), (7, 4), (11, 6), (12, 7)];
    for (b, u) in pairs {
      assert_eq!((to_utf16(s, b), from_utf16(s, u)), (u, b), "{b} {u}");
    }
    // Inside a surrogate pair or a char: the char's start.
    assert_eq!((from_utf16(s, 3), to_utf16(s, 2), from_utf16(s, 99)), (3, 1, 12));
    assert_eq!(range_to_utf16(s, 3..11), 2..6);
    assert_eq!(range_from_utf16(s, 2..6), 3..11);
    let table = Utf16Bytes::new(s);
    for u in 0..9 {
      assert_eq!(table.at(u), from_utf16(s, u) as u32, "{u}");
    }

    let ascii = Utf16Bytes::new("ab");
    assert_eq!((ascii.at(1), ascii.at(2), ascii.at(99), Utf16Bytes::new("").at(1)), (1, 2, 2, 0));
  }

  #[test]
  fn discarding_marked_text_puts_back_what_it_replaced() {
    let mut e = Editor::new("moon stars");
    e.select(Sel::new(0, 4));
    e.set_marked("ni", 2..2, None);
    assert_eq!(e.text(), "ni stars");
    e.discard_marked();
    let got = (e.text(), e.sel(), e.marked(), e.can_undo());
    assert_eq!(got, ("moon stars", Sel::new(0, 4), None, false));
  }
}
