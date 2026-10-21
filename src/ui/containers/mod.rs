// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Containers drawn from the theme, each its headless behaviour plus its look: the scroll area
//! with overlay scrollbars and edge fades ([scroll]) and virtualized lists ([list]). They plug
//! into the [super::Ui] as add-ons.

pub mod list;
pub mod scroll;

pub use list::{List, ListResponse, Row, Selection};
