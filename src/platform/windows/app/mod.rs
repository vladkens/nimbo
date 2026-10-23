// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app platform on Windows: the loop with no window and the windows on demand ([shell], over
//! a hidden window of its own for posts, session end and the tray's messages), the notification
//! area icon ([tray]), global hotkeys on a thread of their own ([hotkey]), launch at login
//! through the registry ([autostart]), links and files opened ([open]) and shortcuts posted to the
//! app in front ([post]).

pub mod autostart;
pub mod hotkey;
pub mod open;
pub mod post;
pub mod shell;
pub mod tray;
