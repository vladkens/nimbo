// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Containers drawn from the theme, each its headless behaviour plus its look: the scroll area
//! with overlay scrollbars and edge fades ([scroll]), virtualized lists ([list]) and columns
//! ([lazy]), menus
//! ([menu]) and the pop-up button ([popup]), tabs ([tabs]) and a resizable split ([split]).
//! They plug into the [super::Ui] as add-ons: a menu tells its items about itself through
//! [super::Ui::slot].

pub mod lazy;
pub mod list;
pub mod menu;
pub mod popup;
pub mod scroll;
pub mod split;
pub mod tabs;

pub use list::{List, ListResponse, Row, Selection};
pub use menu::TypeSelect;
pub use popup::PopUp;
pub use split::{Split, SplitBehavior};
pub use tabs::Tabs;
