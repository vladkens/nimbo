// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/canvas/` through the harness, a module per page, each page alone at its
//! size: [shapes], [text] and [input]. Pace has none: `make pace` runs it in a window.

#[path = "../../examples/canvas/main.rs"]
#[expect(dead_code, reason = "its main and what these tests don't use")]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[cfg_attr(windows, expect(unused_attributes, reason = "its windows_subsystem"))]
mod canvas;

#[path = "../common/mod.rs"]
#[expect(dead_code, reason = "these tests use a part of the helpers")]
mod common;

mod input;
mod shapes;
mod text;
