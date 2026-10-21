// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What a menu-bar app needs on macOS besides its windows ([super::shell]): the status item and
//! its menu ([tray]), global hotkeys ([hotkey]: an event tap with Carbon's hotkeys as the
//! fallback) and launch at login ([autostart]: SMAppService).

pub mod autostart;
pub mod hotkey;
pub mod tray;
