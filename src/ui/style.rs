// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! How an element looks and sits: its [Style] (size, padding, alignment, fills, borders,
//! focus, cursor), a text element's [TextStyle], and [Float] for overlays. All `const`, so a
//! look is a set of constants.

use crate::scene::{Color, Point};
use crate::text::Font;

/// How an element is sized along one axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Dim {
  /// Wraps its content (text, children and padding).
  #[default]
  Fit,
  /// Takes a share of the parent's free space by weight, at least its content's minimum;
  /// across the parent's axis it fills the parent.
  Grow(f32),
  /// Fixed, in points.
  Px(f32),
  /// Percent (0..100) of the parent's inner size, less the gaps along the parent's axis.
  Pct(f32),
  /// Wraps its content as [Dim::Fit] does, but never shrinks below it when the parent is too
  /// small: the others give way (a form row's controls beside its wrapping label).
  Rigid,
}

pub const fn px(v: f32) -> Dim {
  Dim::Px(v)
}

pub const fn pct(v: f32) -> Dim {
  Dim::Pct(v)
}

pub const fn grow() -> Dim {
  Dim::Grow(1.0)
}

pub const fn rigid() -> Dim {
  Dim::Rigid
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
  #[default]
  Start,
  Center,
  End,
}

/// The pointer's shape over an element ([Style::cursor]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Cursor {
  /// The parent's; the arrow at the root.
  #[default]
  Inherit,
  Arrow,
  /// The I-beam, over text.
  Text,
  /// The pointing hand, over links.
  Pointer,
  /// Left-right arrows, over a vertical divider.
  ResizeX,
  /// Up-down arrows, over a horizontal divider.
  ResizeY,
  Crosshair,
  /// An open hand, over something to drag.
  Grab,
  /// A closed hand, while dragging.
  Grabbing,
  NotAllowed,
}

/// How an element takes focus ([Style::focusable], [Style::tab_focusable]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
  #[default]
  None,
  /// By Tab, and by a click on it or on an element inside it (text fields, lists).
  Click,
  /// By Tab only: a click on it leaves focus where it is, as AppKit's buttons do, so clicking
  /// a button keeps the text field being typed in focused.
  Tab,
  /// As [Focus::Click], without the focus ring: the element shows focus itself (a list draws
  /// its selection in the accent).
  Quiet,
  /// As [Focus::Tab], without the focus ring: toolbar tabs ring their selected tab.
  TabQuiet,
}

/// How the focused element is ringed when the window is key ([Ui::set_focus_ring]): a border
/// `width` wide, `gap` outside the element, following its corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusRing {
  pub color: Color,
  pub width: f32,
  pub gap: f32,
}

impl FocusRing {
  /// AppKit's look: 3 points hugging the element.
  pub const fn new(color: Color) -> Self {
    Self { color, width: 3.0, gap: 0.0 }
  }

  pub const fn width(self, width: f32) -> Self {
    Self { width, ..self }
  }

  pub const fn gap(self, gap: f32) -> Self {
    Self { gap, ..self }
  }
}

/// A soft shadow under the element's shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
  pub offset: Point,
  pub blur: f32,
  pub color: Color,
}

impl Shadow {
  /// No shadow: a transparent colour.
  pub const NONE: Self = Self::new(0.0, 0.0, [0.0; 4]);

  pub const fn new(dy: f32, blur: f32, color: Color) -> Self {
    Self { offset: Point::new(0.0, dy), blur, color }
  }
}

/// An element's box: size, spacing, alignment of its children, and decoration. Built with
/// `const` methods, so a theme is a set of constants: `const CARD: Style = Style::new().pad(8.0)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
  /// Width and height.
  pub size: [Dim; 2],
  pub min: [f32; 2],
  pub max: [f32; 2],
  /// Top, right, bottom, left, as in CSS.
  pub pad: [f32; 4],
  /// Between children along the container's axis.
  pub gap: f32,
  /// Of the children within the inner box, horizontally and vertically.
  pub align: [Align; 2],
  pub bg: Color,
  /// Border widths inside the box: top, right, bottom, left.
  pub border: [f32; 4],
  pub border_color: Color,
  /// Corner radii: top-left, top-right, bottom-right, bottom-left.
  pub radii: [f32; 4],
  /// Drawn when its colour isn't transparent (not an `Option`: four bytes keep a laid-out node
  /// in five cache lines).
  pub shadow: Shadow,
  /// Clips the children to the box (overflow hidden); the box can then shrink below its
  /// content.
  pub clip: bool,
  /// Multiplies the alpha of everything the element and its children draw, except canvases.
  pub opacity: f32,
  /// Scrolls its children when they overflow, horizontally and vertically; implies `clip`.
  pub scroll: [bool; 2],
  /// Takes pointer input (hover, press, click, wheel) and hides what is under it from the
  /// pointer. Named elements ([Ui::id]), scroll containers, focusable elements and those with
  /// a cursor always do.
  pub hit: bool,
  /// Takes focus by Tab, and when clicked unless [Focus::Tab].
  pub focus: Focus,
  /// The pointer's shape over it and the elements inside it that don't set their own.
  pub cursor: Cursor,
}

impl Default for Style {
  fn default() -> Self {
    Self::new()
  }
}

impl Style {
  pub const fn new() -> Self {
    Self {
      size: [Dim::Fit; 2],
      min: [0.0; 2],
      max: [f32::INFINITY; 2],
      pad: [0.0; 4],
      gap: 0.0,
      align: [Align::Start; 2],
      bg: [0.0; 4],
      border: [0.0; 4],
      border_color: [0.0; 4],
      radii: [0.0; 4],
      shadow: Shadow::NONE,
      clip: false,
      opacity: 1.0,
      scroll: [false; 2],
      hit: false,
      focus: Focus::None,
      cursor: Cursor::Inherit,
    }
  }

  pub const fn w(self, w: Dim) -> Self {
    Self { size: [w, self.size[1]], ..self }
  }

  pub const fn h(self, h: Dim) -> Self {
    Self { size: [self.size[0], h], ..self }
  }

  /// Grows on both axes.
  pub const fn grow(self) -> Self {
    Self { size: [grow(), grow()], ..self }
  }

  pub const fn min_w(self, v: f32) -> Self {
    Self { min: [v, self.min[1]], ..self }
  }

  pub const fn max_w(self, v: f32) -> Self {
    Self { max: [v, self.max[1]], ..self }
  }

  pub const fn min_h(self, v: f32) -> Self {
    Self { min: [self.min[0], v], ..self }
  }

  pub const fn max_h(self, v: f32) -> Self {
    Self { max: [self.max[0], v], ..self }
  }

  pub const fn pad(self, v: f32) -> Self {
    Self { pad: [v; 4], ..self }
  }

  /// Horizontal and vertical padding.
  pub const fn pad_xy(self, x: f32, y: f32) -> Self {
    Self { pad: [y, x, y, x], ..self }
  }

  /// Top, right, bottom, left.
  pub const fn pads(self, pad: [f32; 4]) -> Self {
    Self { pad, ..self }
  }

  pub const fn gap(self, gap: f32) -> Self {
    Self { gap, ..self }
  }

  /// Aligns the children horizontally and vertically.
  pub const fn align(self, x: Align, y: Align) -> Self {
    Self { align: [x, y], ..self }
  }

  pub const fn center(self) -> Self {
    Self { align: [Align::Center; 2], ..self }
  }

  pub const fn bg(self, bg: Color) -> Self {
    Self { bg, ..self }
  }

  pub const fn border(self, w: f32, color: Color) -> Self {
    Self { border: [w; 4], border_color: color, ..self }
  }

  /// Top, right, bottom, left.
  pub const fn borders(self, border: [f32; 4], color: Color) -> Self {
    Self { border, border_color: color, ..self }
  }

  pub const fn radius(self, r: f32) -> Self {
    Self { radii: [r; 4], ..self }
  }

  pub const fn radii(self, radii: [f32; 4]) -> Self {
    Self { radii, ..self }
  }

  pub const fn shadow(self, shadow: Shadow) -> Self {
    Self { shadow, ..self }
  }

  pub const fn clip(self) -> Self {
    Self { clip: true, ..self }
  }

  pub const fn opacity(self, opacity: f32) -> Self {
    Self { opacity, ..self }
  }

  /// Scrolls vertically on overflow (and clips); the children keep their heights.
  pub const fn scroll_y(self) -> Self {
    Self { scroll: [self.scroll[0], true], clip: true, ..self }
  }

  /// Scrolls horizontally on overflow (and clips); the children keep their widths.
  pub const fn scroll_x(self) -> Self {
    Self { scroll: [true, self.scroll[1]], clip: true, ..self }
  }

  /// Scrolls both ways.
  pub const fn scroll(self) -> Self {
    Self { scroll: [true; 2], clip: true, ..self }
  }

  /// Takes pointer input without a name: a popup's background that must not let clicks
  /// through.
  pub const fn hit(self) -> Self {
    Self { hit: true, ..self }
  }

  /// Takes focus when clicked and in Tab order (tree order), and takes pointer input.
  pub const fn focusable(self) -> Self {
    Self { focus: Focus::Click, ..self }
  }

  /// Takes focus as [Style::focusable] does, with no ring drawn around it ([Focus::Quiet]).
  pub const fn focusable_quiet(self) -> Self {
    Self { focus: Focus::Quiet, ..self }
  }

  /// Takes focus by Tab only (buttons): a click on it leaves focus where it is.
  pub const fn tab_focusable(self) -> Self {
    Self { focus: Focus::Tab, ..self }
  }

  /// Takes focus by Tab only, with no ring drawn around it ([Focus::TabQuiet]).
  pub const fn tab_focusable_quiet(self) -> Self {
    Self { focus: Focus::TabQuiet, ..self }
  }

  /// The pointer's shape over the element; implies [Style::hit].
  pub const fn cursor(self, cursor: Cursor) -> Self {
    Self { cursor, ..self }
  }
}

/// Font, colour and wrapping of a text element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
  pub font: Font,
  pub color: Color,
  /// Wraps at spaces to the width the layout gives it; a word longer than that overflows.
  pub wrap: bool,
  /// A single line that doesn't fit its box is cut and ends in "…"; it can then shrink to
  /// that. Implies no wrapping.
  pub ellipsis: bool,
  /// Distance between baselines in points; 0 takes the font's.
  pub line_h: f32,
  /// A line through the middle of the lowercase letters, in the text's colour (done items).
  pub strike: bool,
}

impl TextStyle {
  pub const fn new(font: Font, color: Color) -> Self {
    Self { font, color, wrap: true, ellipsis: false, line_h: 0.0, strike: false }
  }

  /// Struck through: a line over each laid-out line of the text.
  pub const fn strike(self) -> Self {
    Self { strike: true, ..self }
  }

  pub const fn color(self, color: Color) -> Self {
    Self { color, ..self }
  }

  pub const fn font(self, font: Font) -> Self {
    Self { font, ..self }
  }

  pub const fn nowrap(self) -> Self {
    Self { wrap: false, ..self }
  }

  /// One line, cut with "…" when it doesn't fit (labels, tabs, list rows).
  pub const fn ellipsis(self) -> Self {
    Self { wrap: false, ellipsis: true, ..self }
  }

  pub const fn line_h(self, line_h: f32) -> Self {
    Self { line_h, ..self }
  }
}

/// Places an element out of the flow (an overlay: popup, tooltip, crosshair), relative to its
/// parent or the window, painted above the whole tree without its ancestors' clips. Grow and
/// percent sizes are relative to the target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Float {
  /// Relative to the window rather than the parent.
  pub window: bool,
  /// Keeps its ancestors' clips (an overlay scrollbar inside a scrolled panel); overlays
  /// otherwise draw and take the pointer outside them.
  pub clip: bool,
  /// The target's point to attach to, as fractions of its size: (0, 0) top-left, (1, 1)
  /// bottom-right.
  pub attach: [f32; 2],
  /// The element's own point placed there, as fractions of its size.
  pub anchor: [f32; 2],
  pub offset: Point,
  /// Paint order among overlays, higher on top; equal ones keep tree order.
  pub z: i32,
}

impl Float {
  /// At the parent's top-left corner.
  pub const fn parent() -> Self {
    let offset = Point::new(0.0, 0.0);
    Self { window: false, clip: false, attach: [0.0; 2], anchor: [0.0; 2], offset, z: 0 }
  }

  /// At the window's top-left corner.
  pub const fn window() -> Self {
    Self { window: true, ..Self::parent() }
  }

  pub const fn attach(self, x: f32, y: f32) -> Self {
    Self { attach: [x, y], ..self }
  }

  pub const fn anchor(self, x: f32, y: f32) -> Self {
    Self { anchor: [x, y], ..self }
  }

  pub const fn offset(self, x: f32, y: f32) -> Self {
    Self { offset: Point::new(x, y), ..self }
  }

  pub const fn z(self, z: i32) -> Self {
    Self { z, ..self }
  }

  /// Clipped by its ancestors' clips, as if in the flow.
  pub const fn clip(self) -> Self {
    Self { clip: true, ..self }
  }
}
