// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Styled text: a paragraph of runs in colours, weights, italic, a design, lines and grounds of
//! their own ([AttributedString], as SwiftUI's `AttributedString` and `Text` concatenation), laid
//! out, wrapped and selected as one text ([Ui::text], [Ui::selectable_text]): syntax-coloured
//! code, a coloured diff, a word in bold in a sentence.
//!
//! ```ignore
//! let mut code = AttributedString::new();
//! code.push("fn ", Attributes::new().color(PINK).weight(Weight::Bold));
//! code.push("main", Attributes::new().color(BLUE)).push("() {}", Attributes::new());
//! ui.text(&code, TextStyle::new(Font::mono(12.0), t.text()));
//! ```
//!
//! A run sets what its [Attributes] say; the rest is the text's [TextStyle] (its font's size, its
//! colour, its lines), as SwiftUI's runs take the `Text`'s font and foreground where they set
//! none. A run in another font measures in it, and the paragraph's lines are as high as its
//! tallest font; runs that change only colours, lines or grounds wrap as the plain text does.

use std::fmt::{Display, Write};

use crate::geom::Color;
use crate::text::{Design, Font, Weight};
use crate::ui::{LinePattern, TextStyle};

/// What a run of an [AttributedString] sets over the text's own [TextStyle], as SwiftUI's
/// `AttributeContainer`: nothing by default. `const` builders, like [TextStyle].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Attributes {
  color: Option<Color>,
  /// A fill behind the run's glyphs, a line high (`backgroundColor`): a changed word in a diff.
  background: Option<Color>,
  weight: Option<Weight>,
  design: Option<Design>,
  italic: bool,
  underline: Option<LinePattern>,
  strikethrough: Option<LinePattern>,
}

impl Attributes {
  pub const fn new() -> Self {
    let (color, background, weight, design) = (None, None, None, None);
    let (underline, strikethrough) = (None, None);
    Self { color, background, weight, design, italic: false, underline, strikethrough }
  }

  /// The glyphs' colour, as `foregroundColor`.
  pub const fn color(self, color: Color) -> Self {
    Self { color: Some(color), ..self }
  }

  /// A fill behind the run, a line high, as `backgroundColor`.
  pub const fn background(self, color: Color) -> Self {
    Self { background: Some(color), ..self }
  }

  /// The text's font in this weight, as `Text.fontWeight`.
  pub const fn weight(self, weight: Weight) -> Self {
    Self { weight: Some(weight), ..self }
  }

  /// The text's font in this design: code in a sentence ([Design::Monospaced]).
  pub const fn design(self, design: Design) -> Self {
    Self { design: Some(design), ..self }
  }

  /// The text's font in italic ([Font::italic]).
  pub const fn italic(self) -> Self {
    Self { italic: true, ..self }
  }

  /// A line under the run, as [TextStyle::underline].
  pub const fn underline(self, pattern: LinePattern) -> Self {
    Self { underline: Some(pattern), ..self }
  }

  /// A line through the run, as [TextStyle::strikethrough].
  pub const fn strikethrough(self, pattern: LinePattern) -> Self {
    Self { strikethrough: Some(pattern), ..self }
  }

  /// The font a run in these takes in a text of `base`.
  pub(crate) fn font(&self, base: Font) -> Font {
    let f = Font { weight: self.weight.unwrap_or(base.weight), ..base };
    let f = Font { design: self.design.unwrap_or(base.design), ..f };
    Font { italic: base.italic || self.italic, ..f }
  }

  /// The style a run in these draws in, in a text of `base`, and its ground.
  pub(crate) fn style(&self, base: TextStyle) -> (TextStyle, Option<Color>) {
    let s = TextStyle {
      font: self.font(base.font),
      color: self.color.unwrap_or(base.color),
      underline: self.underline.or(base.underline),
      strikethrough: self.strikethrough.or(base.strikethrough),
      ..base
    };

    (s, self.background)
  }
}

/// A run of styled text: where it ends, as a byte offset of its text, and how it looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Run {
  pub end: u32,
  pub attrs: Attributes,
}

/// Text in runs of [Attributes], shown as one paragraph by [Ui::text](crate::Ui::text) and
/// [Ui::selectable_text](crate::Ui::selectable_text), as SwiftUI's `AttributedString`. An app
/// keeps one it built (a highlighted file) or builds it in the view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttributedString {
  text: String,
  runs: Vec<Run>,
}

impl AttributedString {
  pub const fn new() -> Self {
    Self { text: String::new(), runs: Vec::new() }
  }

  /// Appends `s` in `attrs`, as `+=` of SwiftUI's; returns the string, so pushes chain.
  pub fn push(&mut self, s: &str, attrs: Attributes) -> &mut Self {
    if s.is_empty() {
      return self;
    }

    self.text.push_str(s);
    let end = self.text.len() as u32;
    match self.runs.last_mut() {
      Some(r) if r.attrs == attrs => r.end = end,
      _ => self.runs.push(Run { end, attrs }),
    }

    self
  }

  /// Its characters, without the runs: what selecting it copies.
  pub fn as_str(&self) -> &str {
    &self.text
  }

  pub fn is_empty(&self) -> bool {
    self.text.is_empty()
  }

  /// Empties it, keeping its buffers.
  pub fn clear(&mut self) {
    self.text.clear();
    self.runs.clear();
  }

  pub(crate) fn runs(&self) -> &[Run] {
    &self.runs
  }
}

/// Plain text, one run.
impl From<&str> for AttributedString {
  fn from(s: &str) -> Self {
    let mut a = Self::new();
    a.push(s, Attributes::new());
    a
  }
}

/// What [Ui::text](crate::Ui::text) shows: anything `Display` in the text's style, or an
/// [AttributedString] in its runs (`&code`).
pub trait IntoText {
  /// Writes its text and its runs into the frame's.
  #[doc(hidden)]
  fn write(self, to: Sink);
}

/// Where [IntoText] writes: the frame's text, and its runs, their ends from where it starts.
#[doc(hidden)]
pub struct Sink<'t> {
  pub(crate) text: &'t mut String,
  pub(crate) runs: &'t mut Vec<Run>,
}

impl<T: Display> IntoText for T {
  fn write(self, to: Sink) {
    let _ = write!(to.text, "{self}");
  }
}

impl IntoText for &AttributedString {
  fn write(self, to: Sink) {
    to.text.push_str(&self.text);
    to.runs.extend_from_slice(&self.runs);
  }
}

/// What [Ui::selectable_text](crate::Ui::selectable_text) shows: a `&str` (or `&String`), or an
/// [AttributedString] (`&code`).
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum Selectable<'s> {
  Plain(&'s str),
  Styled(&'s AttributedString),
}

impl<'s> From<&'s str> for Selectable<'s> {
  fn from(s: &'s str) -> Self {
    Self::Plain(s)
  }
}

impl<'s> From<&'s String> for Selectable<'s> {
  fn from(s: &'s String) -> Self {
    Self::Plain(s)
  }
}

impl<'s> From<&'s AttributedString> for Selectable<'s> {
  fn from(s: &'s AttributedString) -> Self {
    Self::Styled(s)
  }
}

/// The pieces of `range` of a text in `base` with `runs` (ends from the text's start): each
/// one's range and its run's attributes; the text past the last run, and all of a text with
/// none, in the base's.
pub(crate) fn pieces(
  runs: &[Run],
  (a, b): (usize, usize),
) -> impl Iterator<Item = (usize, usize, Attributes)> + '_ {
  let first = runs.partition_point(|r| r.end as usize <= a);
  let mut at = a;
  let mut k = first;
  std::iter::from_fn(move || {
    if at >= b {
      return None;
    }

    let (end, attrs) = match runs.get(k) {
      Some(r) => ((r.end as usize).min(b), r.attrs),
      None => (b, Attributes::new()),
    };

    let piece = (at, end, attrs);
    (at, k) = (end, k + 1);
    Some(piece)
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::keys::CMD;
  use crate::ui::layout::Elide;
  use crate::ui::{Id, Kind, Style, Ui, px};
  use crate::{App, Harness, Modifiers, Size};

  const RED: Color = [1.0, 0.0, 0.0, 1.0];
  const BLUE: Color = [0.0, 0.0, 1.0, 1.0];
  const GREEN: Color = [0.0, 1.0, 0.0, 1.0];

  /// A styled text `t`, selectable or not, in a white box `w` wide.
  struct Para {
    w: f32,
    text: AttributedString,
    style: TextStyle,
    selectable: bool,
  }

  impl App for Para {
    fn view(&mut self, ui: &mut Ui) {
      ui.col(Style::new().w(px(self.w)).bg([1.0; 4]).pad(4.0), |ui| match self.selectable {
        true => _ = ui.id("t").selectable_text(&self.text, self.style),
        false => ui.id("t").text(&self.text, self.style),
      });
    }
  }

  fn para(w: f32, text: AttributedString, style: TextStyle, selectable: bool) -> Harness<Para> {
    let mut h = Harness::new(Para { w, text, style, selectable }, Size::new(400.0, 200.0), 1.0);
    h.frame();
    h
  }

  /// "Hello " and "World" around `middle` in `attrs`.
  fn hello(middle: &str, attrs: Attributes) -> AttributedString {
    let mut s = AttributedString::from("Hello ");
    s.push(middle, attrs).push(" World", Attributes::new());
    s
  }

  /// The lines text `t` took, and its box's width.
  fn lines(h: &Harness<Para>) -> (u32, f32) {
    let n = h.rt.ui.last.get(Id::of("t")).unwrap();
    let Kind::Text { lines: (a, b), .. } = n.kind else { panic!() };
    (b - a, n.rect.w)
  }

  /// A bold run measures bold: where the plain text fits one line, the bold one takes two, the
  /// paragraph breaking as one text; a run of another colour only wraps as the plain text.
  #[test]
  fn runs_wrap_as_one_paragraph() {
    let style = TextStyle::new(Font::ui(13.0), [0.0, 0.0, 0.0, 1.0]);
    let mut h = para(400.0, hello("WWWW", Attributes::new()), style, false);
    let (plain, bold) = (h.measure("Hello WWWW World", style.font), {
      let b = style.font.weight(Weight::Bold);
      h.measure("Hello ", style.font) + h.measure("WWWW", b) + h.measure(" World", style.font)
    });
    assert!(bold > plain + 2.0, "bold {bold}, plain {plain}");
    let w = (plain + bold) / 2.0 + 8.0;
    for (attrs, n) in
      [(Attributes::new().color(RED), 1), (Attributes::new().weight(Weight::Bold), 2)]
    {
      h.app_mut().w = w;
      h.app_mut().text = hello("WWWW", attrs);
      h.frame();
      assert_eq!(lines(&h).0, n, "{attrs:?}");
    }
  }

  /// Each run draws in its colour, a ground behind its run a line high.
  #[test]
  fn runs_paint_in_their_colours_and_grounds() {
    let style = TextStyle::new(Font::ui(40.0).weight(Weight::Bold), [0.0, 0.0, 0.0, 1.0]);
    let mut s = AttributedString::new();
    s.push("MM", Attributes::new().color(RED)).push("MM", Attributes::new().color(BLUE));
    s.push("MM", Attributes::new().background(GREEN));
    let mut h = para(400.0, s, style.nowrap(), false);
    let r = h.rect(Id::of("t")).unwrap();
    let m = h.measure("MM", style.font);
    let img = h.render();

    // Pixels of each colour along a run, below the middle of its capitals.
    let tone = |x0: f32| {
      let (mut red, mut blue) = (0, 0);
      for x in (x0 as usize)..(x0 + m) as usize {
        let p = img.pixel(x as f32, r.y + r.h * 0.6);
        red += (p[0] > 200 && p[2] < 80) as u32;
        blue += (p[2] > 200 && p[0] < 80) as u32;
      }

      (red, blue)
    };
    let (left, right) = (tone(r.x), tone(r.x + m));
    assert!(left.0 > 10 && left.1 == 0, "{left:?}");
    assert!(right.1 > 10 && right.0 == 0, "{right:?}");
    // Above the capitals, in the line's top: the ground.
    assert_eq!(img.pixel(r.x + 2.0 * m + m / 2.0, r.y + 2.0), [0, 255, 0, 255]);
    assert_eq!(img.pixel(r.x + m / 2.0, r.y + 2.0), [255; 4]);
  }

  /// Selectable styled text copies its characters, and a double click on its bold word selects
  /// that word: its caret stops follow the runs' fonts.
  #[test]
  fn selectable_styled_text_selects_by_its_runs() {
    let style = TextStyle::new(Font::ui(20.0), [0.0, 0.0, 0.0, 1.0]);
    let bold = Attributes::new().weight(Weight::Bold);
    let mut h = para(400.0, hello("WWWW", bold), style, true);
    let r = h.rect(Id::of("t")).unwrap();
    let x =
      r.x + h.measure("Hello ", style.font) + h.measure("WWWW", style.font.weight(Weight::Bold))
        - 3.0;
    h.press_with(x, r.y + r.h / 2.0, 2, Modifiers::empty());
    h.release(x, r.y + r.h / 2.0);
    h.frame();
    assert_eq!(h.selection(Id::of("t")), Some((6, 10)));
    h.key('a', CMD);
    h.key('c', CMD);
    assert_eq!(h.clipboard().read().as_deref(), Some("Hello WWWW World"));
  }

  /// One line of runs cut to fit ends in "…" within its box.
  #[test]
  fn a_styled_line_cut_to_fit_ends_in_an_ellipsis() {
    let style = TextStyle::new(Font::ui(13.0), [0.0, 0.0, 0.0, 1.0]).line_limit(1);
    let h = para(60.0, hello("WWWW", Attributes::new().weight(Weight::Bold)), style, false);
    let t = &h.rt.ui.last;
    let n = t.get(Id::of("t")).unwrap();
    let Kind::Text { lines: (a, b), .. } = n.kind else { panic!() };
    let l = t.lines[a as usize];
    assert_eq!((b - a, l.elide), (1, Elide::Tail));
    assert!(l.width <= n.rect.w + 0.01, "{} in {}", l.width, n.rect.w);
  }

  #[test]
  fn pushes_merge_alike_runs_and_skip_empty_ones() {
    let red = Attributes::new().color([1.0, 0.0, 0.0, 1.0]);
    let mut s = AttributedString::new();
    s.push("fn ", red).push("", Attributes::new()).push("main", red).push("()", Attributes::new());
    assert_eq!(s.as_str(), "fn main()");
    assert_eq!(s.runs().iter().map(|r| r.end).collect::<Vec<_>>(), [7, 9]);
  }

  #[test]
  fn pieces_split_a_range_at_the_runs() {
    let bold = Attributes::new().weight(Weight::Bold);
    let mut s = AttributedString::from("ab");
    s.push("cd", bold).push("ef", Attributes::new());
    let p: Vec<_> = pieces(s.runs(), (1, 5)).map(|(a, b, at)| (a, b, at == bold)).collect();
    assert_eq!(p, [(1, 2, false), (2, 4, true), (4, 5, false)]);
    // Past the runs, and with none: the base's.
    assert_eq!(pieces(&[], (0, 3)).count(), 1);
    assert_eq!(pieces(s.runs(), (6, 8)).map(|p| (p.0, p.1)).collect::<Vec<_>>(), [(6, 8)]);
  }

  #[test]
  fn a_run_takes_the_texts_style_where_it_sets_none() {
    let base = TextStyle::new(Font::mono(12.0), [1.0; 4]).underline(LinePattern::Dot);
    let attrs = Attributes::new().weight(Weight::Bold).italic().color([0.0, 0.0, 0.0, 1.0]);
    let (s, ground) = attrs.style(base);
    assert_eq!(
      (s.font, s.color, s.underline),
      (
        Font::mono(12.0).weight(Weight::Bold).italic(),
        [0.0, 0.0, 0.0, 1.0],
        Some(LinePattern::Dot)
      )
    );
    assert_eq!(ground, None);
    assert_eq!(Attributes::new().font(base.font), base.font);
  }
}
