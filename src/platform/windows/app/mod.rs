// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app platform on Windows: the loop with no window and the windows on demand ([shell], over
//! a hidden window of its own for posts, session end and the tray's messages), the notification
//! area icon ([tray]), global hotkeys on a thread of their own ([hotkey]) and launch at login
//! through the registry ([autostart]).

pub mod autostart;
pub mod hotkey;
pub mod shell;
pub mod tray;
