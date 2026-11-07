// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! How an element looks and sits: its [Style] (size, padding, alignment, fills, borders,
//! focus, cursor), a text element's [TextStyle], and [Float] for overlays. All `const`, so a
//! look is a set of constants.

use crate::scene::{Color, Point, Rect};
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

/// What an element clips its children to ([Style::clip], [Style::clip_content]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Clip {
  #[default]
  None,
  /// Its box (overflow hidden).
  Box,
  /// Its box inside the padding (a point short of it, for a caret at the edge): a search
  /// field's text stops short of the marks there.
  Content,
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
  /// Clips the children to the box, or inside its padding; the box can then shrink below its
  /// content.
  pub clip: Clip,
  /// Multiplies the alpha of everything the element and its children draw, except canvases.
  pub opacity: f32,
  /// The axes it scrolls its children along when they overflow; implies `clip`.
  pub scroll: Axes,
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
      clip: Clip::None,
      opacity: 1.0,
      scroll: Axes::NONE,
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
    Self { clip: Clip::Box, ..self }
  }

  /// Clips the children inside the padding, not at the box.
  pub const fn clip_content(self) -> Self {
    Self { clip: Clip::Content, ..self }
  }

  /// Where its children are clipped, given its box `r`; `None` when they aren't. Inside the
  /// padding is a point short of it, for a caret at the text's edge.
  pub fn clip_rect(&self, r: Rect) -> Option<Rect> {
    let [top, right, bottom, left] = self.pad.map(|p| (p - 1.0).max(0.0));
    match self.clip {
      Clip::None => None,
      Clip::Box => Some(r),
      Clip::Content => {
        let (w, h) = ((r.w - left - right).max(0.0), (r.h - top - bottom).max(0.0));
        Some(Rect::new(r.x + left, r.y + top, w, h))
      }
    }
  }

  /// Its clip, at least the box: a scroll container clips.
  const fn clipped(self) -> Clip {
    match self.clip {
      Clip::None => Clip::Box,
      c => c,
    }
  }

  pub const fn opacity(self, opacity: f32) -> Self {
    Self { opacity, ..self }
  }

  /// Scrolls along `axes` on overflow (and clips), as SwiftUI's `ScrollView(axes)`: the
  /// children keep their size along them.
  pub const fn scroll(self, axes: Axes) -> Self {
    Self { scroll: axes, clip: self.clipped(), ..self }
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
  /// How it takes lines: wrapping, one line as wide as it is, or at most some cut with "…".
  pub lines: TextLines,
  /// Distance between baselines in points; `None` takes the font's ([crate::Ui::line_height]).
  pub line_h: Option<f32>,
  /// A line under each laid-out line, in the text's colour ([TextStyle::underline]).
  pub underline: Option<LinePattern>,
  /// A line through the middle of the lowercase letters ([TextStyle::strikethrough]).
  pub strikethrough: Option<LinePattern>,
}

/// A direction, as SwiftUI's `Axis`: along x or along y.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
  Horizontal,
  Vertical,
}

impl Axis {
  /// Its place in a `[x, y]` pair.
  pub(crate) const fn index(self) -> usize {
    self as usize
  }
}

/// A set of axes, as SwiftUI's `Axis.Set`: what a scroll area scrolls along
/// ([Style::scroll]). `Axes::HORIZONTAL | Axes::VERTICAL` is both, [Axes::ALL].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Axes(u8);

impl Axes {
  pub const NONE: Self = Self(0);
  pub const HORIZONTAL: Self = Self(1);
  pub const VERTICAL: Self = Self(2);
  pub const ALL: Self = Self(3);

  pub const fn contains(self, axis: Axis) -> bool {
    self.0 & (1 << axis as u8) != 0
  }

  pub const fn is_empty(self) -> bool {
    self.0 == 0
  }

  /// [Axes::contains] of the axis at `[x, y]` index `ax`.
  pub(crate) const fn at(self, ax: usize) -> bool {
    self.0 & (1 << ax) != 0
  }
}

impl std::ops::BitOr for Axes {
  type Output = Self;

  fn bitor(self, other: Self) -> Self {
    Self(self.0 | other.0)
  }
}

/// How a text takes lines, as SwiftUI's `lineLimit`, `truncationMode` and `fixedSize`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextLines {
  /// Wraps at spaces onto as many lines as it needs at the width the layout gives it; a word
  /// longer than that breaks between characters.
  #[default]
  Wrap,
  /// One line (a line per line break) as wide as the text, never cut: a button's label, as
  /// SwiftUI's `.fixedSize()`.
  Fixed,
  /// At most this many lines, wrapping; where the text doesn't fit them it is cut with "…" in
  /// the [TruncationMode], and it can then shrink to that, as `.lineLimit(n)` with
  /// `.truncationMode(mode)`. Over more than one line it is cut at the end.
  Limit(u16, TruncationMode),
}

/// Where a text cut to fit loses what doesn't, shown by "…", as SwiftUI's
/// `Text.TruncationMode`: a path keeps its file name with [TruncationMode::Middle].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TruncationMode {
  Head,
  Middle,
  #[default]
  Tail,
}

/// How a text's line is drawn ([TextStyle::underline], [TextStyle::strikethrough]), as
/// SwiftUI's `Text.LineStyle.Pattern`: dots and dashes as long as the line is thick, and as far
/// apart, a dash three dots long.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinePattern {
  Solid,
  /// A term with more on hover ([crate::Ui::help]).
  Dot,
  Dash,
  DashDot,
  DashDotDot,
}

impl LinePattern {
  /// The pattern's marks and gaps in line thicknesses, from its start; empty for a solid line.
  pub(crate) const fn marks(self) -> &'static [f32] {
    match self {
      LinePattern::Solid => &[],
      LinePattern::Dot => &[1.0, 1.0],
      LinePattern::Dash => &[3.0, 1.0],
      LinePattern::DashDot => &[3.0, 1.0, 1.0, 1.0],
      LinePattern::DashDotDot => &[3.0, 1.0, 1.0, 1.0, 1.0, 1.0],
    }
  }
}

impl TextStyle {
  pub const fn new(font: Font, color: Color) -> Self {
    let (underline, strikethrough, line_h) = (None, None, None);
    Self { font, color, lines: TextLines::Wrap, line_h, underline, strikethrough }
  }

  /// A line under the text in `pattern`, as SwiftUI's `Text.underline(pattern:)`: a sixth of an
  /// em under the baseline.
  pub const fn underline(self, pattern: LinePattern) -> Self {
    Self { underline: Some(pattern), ..self }
  }

  /// A line through the text in `pattern` (done items), as SwiftUI's
  /// `Text.strikethrough(pattern:)`: at half the x-height.
  pub const fn strikethrough(self, pattern: LinePattern) -> Self {
    Self { strikethrough: Some(pattern), ..self }
  }

  pub const fn color(self, color: Color) -> Self {
    Self { color, ..self }
  }

  pub const fn font(self, font: Font) -> Self {
    Self { font, ..self }
  }

  /// One line as wide as the text ([TextLines::Fixed]), as SwiftUI's `.fixedSize()`.
  pub const fn fixed_size(self) -> Self {
    Self { lines: TextLines::Fixed, ..self }
  }

  /// At most `n` lines, cut with "…" where it doesn't fit ([TextLines::Limit]), at its end
  /// unless [TextStyle::truncation] says otherwise, as SwiftUI's `.lineLimit(n)`: one for labels,
  /// tabs, list rows.
  pub const fn line_limit(self, n: u16) -> Self {
    let mode = match self.lines {
      TextLines::Limit(_, mode) => mode,
      _ => TruncationMode::Tail,
    };

    Self { lines: TextLines::Limit(n, mode), ..self }
  }

  /// Where a text cut to fit loses what doesn't, as SwiftUI's `.truncationMode`; one line unless
  /// [TextStyle::line_limit] said more.
  pub const fn truncation(self, mode: TruncationMode) -> Self {
    let n = if let TextLines::Limit(n, _) = self.lines { n } else { 1 };
    Self { lines: TextLines::Limit(n, mode), ..self }
  }

  /// Distance between baselines in points, as SwiftUI's `.lineSpacing` sets it in effect.
  pub const fn line_h(self, line_h: f32) -> Self {
    Self { line_h: Some(line_h), ..self }
  }
}

/// Places an element out of the flow (an overlay: popup, tooltip, crosshair), relative to its
/// parent or the window, painted above the whole tree without its ancestors' clips. Grow and
/// percent sizes are relative to the target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Float {
  /// Placed in its parent's space, or the window's.
  pub space: CoordinateSpace,
  /// Keeps its ancestors' clips (an overlay scrollbar inside a scrolled panel); overlays
  /// otherwise draw and take the pointer outside them.
  pub clip: bool,
  /// The target's point to attach to.
  pub attach: UnitPoint,
  /// The element's own point placed there.
  pub anchor: UnitPoint,
  pub offset: Point,
  /// Paint order among overlays, higher on top; equal ones keep tree order.
  pub z: i32,
}

/// Where an overlay is placed ([Float]), as SwiftUI's `CoordinateSpace`: in its parent, or in
/// the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinateSpace {
  Local,
  Global,
}

/// A point in a box as fractions of its size, as SwiftUI's `UnitPoint`: (0, 0) the top-left
/// corner, (1, 1) the bottom-right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitPoint {
  pub x: f32,
  pub y: f32,
}

impl UnitPoint {
  pub const TOP_LEADING: Self = Self::new(0.0, 0.0);
  pub const TOP: Self = Self::new(0.5, 0.0);
  pub const TOP_TRAILING: Self = Self::new(1.0, 0.0);
  pub const LEADING: Self = Self::new(0.0, 0.5);
  pub const CENTER: Self = Self::new(0.5, 0.5);
  pub const TRAILING: Self = Self::new(1.0, 0.5);
  pub const BOTTOM_LEADING: Self = Self::new(0.0, 1.0);
  pub const BOTTOM: Self = Self::new(0.5, 1.0);
  pub const BOTTOM_TRAILING: Self = Self::new(1.0, 1.0);

  pub const fn new(x: f32, y: f32) -> Self {
    Self { x, y }
  }

  /// Its fraction along the axis at `[x, y]` index `ax`.
  pub(crate) const fn at(self, ax: usize) -> f32 {
    if ax == 0 { self.x } else { self.y }
  }
}

impl Float {
  /// At the parent's top-left corner.
  pub const fn parent() -> Self {
    let (offset, at) = (Point::new(0.0, 0.0), UnitPoint::TOP_LEADING);
    let space = CoordinateSpace::Local;
    Self { space, clip: false, attach: at, anchor: at, offset, z: 0 }
  }

  /// At the window's top-left corner.
  pub const fn window() -> Self {
    Self { space: CoordinateSpace::Global, ..Self::parent() }
  }

  /// The target's point it attaches to.
  pub const fn attach(self, at: UnitPoint) -> Self {
    Self { attach: at, ..self }
  }

  /// Its own point placed there.
  pub const fn anchor(self, at: UnitPoint) -> Self {
    Self { anchor: at, ..self }
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
