// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

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
