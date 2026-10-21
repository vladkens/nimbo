// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Containers drawn from the theme, each its headless behaviour plus its look: the scroll area
//! with overlay scrollbars and edge fades ([scroll]), virtualized lists ([list]), menus ([menu])
//! and the pop-up button ([popup]). They plug into the [super::Ui] as add-ons: a menu tells its
//! items about itself through [super::Ui::slot].

pub mod list;
pub mod menu;
pub mod popup;
pub mod scroll;

pub use list::{List, ListResponse, Row, Selection};
pub use menu::TypeSelect;
pub use popup::PopUp;
