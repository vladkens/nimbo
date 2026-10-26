// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What [crate::uninstall::system] undoes on Linux besides launch at login: the launcher entry,
//! an autostart entry the user turned off (launch at login's state doesn't count it), and the
//! RemoteDesktop portal's approval ([super::remote]). Ported from wavo-app's `src/cli.rs`
//! (vladkens, MIT).

use std::{fs, io};

use crate::AppInfo;
use crate::uninstall::{Step, remove_file};

pub fn steps(app: &AppInfo) -> Vec<Step> {
  let off = super::autostart::set(app, false).map(drop);
  let entry = super::desktop::uninstall(app).and(off);
  let dir = crate::paths::config_dir(app.id);
  let token = remove_file(&dir.join(super::remote::TOKEN));
  // The folder made for the token, unless the app keeps more in it.
  _ = fs::remove_dir(&dir);
  vec![
    Step { what: "launcher and autostart entries removed", result: entry },
    Step { what: "remote desktop approval removed", result: token },
  ]
}

/// A folder not empty yet, as a file came into it while it was emptied.
pub fn busy(e: &io::Error) -> bool {
  e.kind() == io::ErrorKind::DirectoryNotEmpty
}
