// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/files/` through the harness, a module per part: [browser] alone at its
//! size, the [sidebar] and the [panels] (with files dropped on the list) on the whole window.

#[path = "../../examples/files/main.rs"]
#[expect(dead_code, reason = "its main and what these tests don't use")]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[cfg_attr(windows, expect(unused_attributes, reason = "its windows_subsystem"))]
mod files;

#[path = "../common/mod.rs"]
mod common;

mod browser;
mod panels;
mod sidebar;
