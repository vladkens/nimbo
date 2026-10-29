// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A console app that updates itself in place ([nimbo::update]), with no window: it prints its
//! version, the release a feed offers, or downloads and installs that release and ends, as the
//! new version starts in its place. Its first call is [update::finish], which is the installer
//! when the update started it and else prints the error of the installer that started this run.
//! It runs once ([instance]): a second run is refused.
//!
//! `UPDATE_VERSION=1.0.0 cargo build --release --example update` sets the version (the crate's
//! without it). The release's file is what people download: on macOS the example bundled as
//! `Update.app`, signed with the identity v1 has, in its DMG (`scripts/pack.py app` and `dmg`); on
//! Windows the per-user NSIS setup of `pack.py setup`, with the config's `name` "app"
//! so it installs `%LOCALAPPDATA%\Programs\app\app.exe` (Windows asks to elevate an
//! `update.exe`); on Linux the AppImage of `pack.py appimage`. `scripts/pack.py feed` makes a feed
//! of them, `update.txt`, and `feed --public-key` prints the key in hex. Then `update` prints the
//! version, `update --check <feed url> <key>` what the feed offers, `update --update <feed url>
//! <key>` updates, printing the download's percentage.

use std::env;
use std::io::{self, ErrorKind};
use std::process::ExitCode;

use nimbo::instance::{self, Claim};
use nimbo::update::{self, Feed};
use nimbo::{AppInfo, Presence};

pub const INFO: AppInfo =
  AppInfo { id: "com.example.nimbo-update", name: "Update", presence: Presence::Regular };
pub const VERSION: &str = match option_env!("UPDATE_VERSION") {
  Some(v) => v,
  None => env!("CARGO_PKG_VERSION"),
};
const USAGE: &str = "usage: update [--check | --update <feed url> <key>]";

/// What the arguments ask: the version; the release the feed offers; that release installed.
fn run(args: &[String]) -> io::Result<()> {
  let feed = |url, key| Feed { url, key };
  match args {
    [] => {
      println!("{VERSION}");
      Ok(())
    }
    [flag, url, key] if flag == "--check" => {
      match update::check(&feed(url, key), VERSION)? {
        Some(r) => println!("{} at {}", r.version, r.url),
        None => println!("{VERSION} is the latest"),
      }

      Ok(())
    }
    [flag, url, key] if flag == "--update" => {
      let Some(release) = update::check(&feed(url, key), VERSION)? else {
        println!("{VERSION} is the latest");
        return Ok(());
      };

      println!("{VERSION} -> {}: downloading {}", release.version, release.url);
      let staged = update::download(&release, |done, total| {
        println!("{}%", done * 100 / total.max(1));
      })?;
      update::install(staged)?;
      println!("installed {}, it starts as this one ends", release.version);
      Ok(())
    }
    _ => Err(io::Error::new(ErrorKind::InvalidInput, USAGE)),
  }
}

fn main() -> ExitCode {
  // First, before the arguments: the installer runs here and exits.
  if let Some(e) = update::finish() {
    eprintln!("{e}");
  }

  let _instance = match instance::claim(&INFO) {
    Ok(Claim::Primary(instance)) => instance,
    Ok(Claim::Running) => {
      eprintln!("update: another instance runs");
      return ExitCode::FAILURE;
    }
    Err(e) => {
      eprintln!("update: instance: {e}");
      return ExitCode::FAILURE;
    }
  };

  match run(&env::args().skip(1).collect::<Vec<_>>()) {
    Ok(()) => ExitCode::SUCCESS,
    // The library's errors say they are the update's.
    Err(e) => {
      eprintln!("{e}");
      ExitCode::FAILURE
    }
  }
}
