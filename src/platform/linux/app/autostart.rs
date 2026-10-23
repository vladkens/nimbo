// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Launch at login on Linux: an XDG autostart entry, `$XDG_CONFIG_HOME/autostart/<id>.desktop`
//! (`~/.config` without it), which the session starts at login (GNOME, KDE, Xfce and the
//! others that follow the XDG Autostart spec). It runs `$APPIMAGE` when the app is one, else the
//! executable running now. Inside Flatpak this needs the Background portal instead: not done.

use std::path::PathBuf;

use crate::AppInfo;
use crate::autostart::{State, desktop_entry, entry_runs};

pub fn available() -> Result<(), String> {
  Ok(())
}

/// The entry's file.
fn entry(app: &AppInfo) -> PathBuf {
  crate::paths::config_dir("autostart").join(format!("{}.desktop", app.id))
}

pub fn state(app: &AppInfo) -> Result<State, String> {
  match std::fs::read_to_string(entry(app)) {
    Ok(text) if entry_runs(&text) => Ok(State::Enabled),
    Ok(_) => Ok(State::NotRegistered),
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::NotRegistered),
    Err(e) => Err(format!("{}: {e}", entry(app).display())),
  }
}

pub fn set(app: &AppInfo, on: bool) -> Result<State, String> {
  let path = entry(app);
  let err = |e: std::io::Error| format!("{}: {e}", path.display());
  if on {
    let exe = match std::env::var_os("APPIMAGE") {
      Some(p) => PathBuf::from(p),
      None => std::env::current_exe().map_err(|e| format!("the executable: {e}"))?,
    };

    if let Some(dir) = path.parent() {
      std::fs::create_dir_all(dir).map_err(err)?;
    }

    std::fs::write(&path, desktop_entry(app, &exe.to_string_lossy())).map_err(err)?;
  } else if let Err(e) = std::fs::remove_file(&path)
    && e.kind() != std::io::ErrorKind::NotFound
  {
    return Err(err(e));
  }

  state(app)
}

pub fn open_settings() {}
