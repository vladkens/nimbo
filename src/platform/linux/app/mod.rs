// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What an app needs on Linux besides its windows: GTK's loop with no window and windows on
//! demand ([shell]), a StatusNotifierItem with its menu ([tray]), hotkeys through the
//! GlobalShortcuts portal ([hotkey]) and an XDG autostart entry ([autostart]); the tray and the
//! portal speak D-Bus through GIO ([dbus]).

pub mod autostart;
mod dbus;
pub mod hotkey;
mod menu;
pub mod shell;
pub mod tray;
