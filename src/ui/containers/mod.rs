// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Containers drawn from the theme, each its headless behaviour plus its look: virtualized lists
//! ([list]), menus ([menu]) and the picker ([popup]), tabs ([tabs]), a resizable split ([split])
//! and the window's toolbar ([toolbar]). The scroll area, which menus and lists scroll in, is
//! the behaviours' ([super::behavior], [super::Ui::scroll_area]), drawn as the environment says
//! ([super::ScrollIndicatorStyle]).
//! They plug into the [super::Ui] as add-ons: a menu tells its items about itself through
//! [super::Ui::slot].

pub mod list;
pub mod menu;
pub mod popup;
pub mod split;
pub mod tabs;
pub mod toolbar;

pub use list::{List, ListResponse, ListStyle, Row, Selection};
pub use menu::{MenuPlacement, TypeSelect};
pub use popup::{Picker, Tag};
pub use split::{Split, SplitBehavior};
pub use tabs::Tabs;
pub use toolbar::ToolbarItemPlacement;
