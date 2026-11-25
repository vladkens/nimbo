// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What a label shows ([Content]): a text, an icon or what a closure builds, built in the text
//! style its look gives it ([Content::build]), so a look of the app's own draws a label as the
//! theme's does.

use std::fmt;

use crate::locale::Localized;
use crate::ui::icon::Icon;
use crate::ui::{Style, TextStyle, Ui, px};

/// What a label shows, as SwiftUI's labels take a string or a view: a text, an icon, or what a
/// closure builds in the label's text style (dimmed in a disabled form row, as SwiftUI passes a
/// foreground style down); from a `&str`, a `&String`, a [Localized] (`t!("…")`, or a reference to
/// one), an [Icon] or `FnOnce(&mut Ui, TextStyle)`, whose parameters need their types written;
/// `()` shows nothing, as SwiftUI's empty label `{}` (a toggle in a form row, named by the row). A
/// closure's texts take the style or one made from it (`s.underline(LinePattern::Dot)` on a term
/// with [Ui::help]); a canvas of its own reads `Enabled` ([Ui::environment_value]).
#[derive(Default)]
#[non_exhaustive]
pub enum Content<'s> {
  #[default]
  None,
  Text(&'s str),
  Icon(Icon),
  Built(Build<'s>),
}

/// What a [Content::Built] runs: it builds the content in its style.
pub(crate) type Build<'s> = Box<dyn FnOnce(&mut Ui, TextStyle) + 's>;

impl Content<'_> {
  /// Shows nothing: none, or an empty text.
  pub fn is_empty(&self) -> bool {
    matches!(self, Content::None | Content::Text(""))
  }

  /// Builds it in `style` under the id set for it ([Ui::with_id]): a text, or an icon as high as
  /// its line in its colour (stroked 1.6 unless it has a width), neither taking the pointer; a
  /// closure's elements as it names them.
  pub fn build(self, ui: &mut Ui, style: TextStyle) {
    let id = ui.take_id();
    match self {
      Content::None => {}
      Content::Text(s) => ui.with_id(id).allows_hit_testing(false).text(s, style),
      Content::Icon(i) => {
        let (size, color) = (ui.line_height(style), style.color);
        let s = Style::new().w(px(size)).h(px(size));
        let i = i.default_stroke(1.6);
        ui.with_id(id).allows_hit_testing(false).canvas(s, move |f, r| i.draw(f, r, color));
      }
      Content::Built(f) => f(ui, style),
    }
  }
}

impl<'s> From<&'s str> for Content<'s> {
  fn from(s: &'s str) -> Self {
    Content::Text(s)
  }
}

impl<'s> From<&'s String> for Content<'s> {
  fn from(s: &'s String) -> Self {
    Content::Text(s)
  }
}

impl From<Localized> for Content<'_> {
  fn from(s: Localized) -> Self {
    Content::Text(s.as_str())
  }
}

impl From<&Localized> for Content<'_> {
  fn from(s: &Localized) -> Self {
    Content::Text(s.as_str())
  }
}

/// Nothing: [Content::None].
impl From<()> for Content<'_> {
  fn from((): ()) -> Self {
    Content::None
  }
}

impl From<Icon> for Content<'_> {
  fn from(i: Icon) -> Self {
    Content::Icon(i)
  }
}

impl<'s, F: FnOnce(&mut Ui, TextStyle) -> R + 's, R> From<F> for Content<'s> {
  fn from(f: F) -> Self {
    Content::Built(Box::new(move |ui, s| _ = f(ui, s)))
  }
}

/// Its text, `…` for one built.
impl fmt::Debug for Content<'_> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      Content::None => f.write_str(""),
      Content::Text(s) => f.write_str(s),
      Content::Icon(i) => write!(f, "{:?}", i.name()),
      Content::Built(_) => f.write_str("…"),
    }
  }
}
