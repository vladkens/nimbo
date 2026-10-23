// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What an app needs on Linux besides its windows: GTK's loop with no window and windows on
//! demand ([shell]), a StatusNotifierItem with its menu ([tray]), hotkeys through the
//! GlobalShortcuts portal ([hotkey]), an XDG autostart entry ([autostart]), links and files opened
//! ([open]) and shortcuts posted to the app in focus on X11 ([post]); the tray, the portal and the
//! file manager speak D-Bus through GIO ([super::dbus]).

pub mod autostart;
pub mod hotkey;
mod menu;
pub mod open;
pub mod post;
pub mod shell;
pub mod tray;
