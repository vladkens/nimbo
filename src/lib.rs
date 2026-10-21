// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! nimbo: a small custom-drawn UI toolkit, with its own CPU renderer for drawing.

pub mod cpu;
mod png;
pub mod scene;

pub use scene::{
  Color, Frame, ImageId, Images, Point, Quad, Rect, Scene, Segment, Size, alpha, mix, rgb,
};
