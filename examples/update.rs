// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A console app that updates itself in place ([nimbo::update]), with no window: it prints its
//! version, the release a feed offers, or downloads and installs that release and ends, as the
//! new version starts in its place. It runs once ([instance]), so the relaunched version waits
//! for this one to end and removes what the update left.
//!
//! `UPDATE_VERSION=1.0.0 cargo build --release --example update` sets the version (the crate's
//! without it); `scripts/update-feed.py` makes a feed. Then `update` prints the version,
//! `update --check <feed url> <key>` what the feed offers, `update --update <feed url> <key>`
//! updates.

use std::io::{self, ErrorKind};
use std::process::ExitCode;
use std::time::Duration;
use std::{env, thread};

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
/// How long a relaunched version stays: the update's leftovers go on a thread of their own,
/// tried again for up to 5 s while a file stays busy.
const TIDY: Duration = Duration::from_secs(6);

/// What the arguments ask: the version; the release the feed offers; that release installed.
fn run(args: &[String]) -> io::Result<()> {
  let feed = |url, key| Feed { url, key };
  match args {
    [] => {
      println!("{VERSION}");
      if env::var_os("NIMBO_RELAUNCH").is_some() {
        thread::sleep(TIDY);
      }

      Ok(())
    }
    [flag, url, key] if flag == "--check" => {
      match update::check(&feed(url, key), VERSION)? {
        Some(r) => println!("{} at {}\n{}", r.version, r.url, r.notes),
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
      update::install(update::download(&release)?)?;
      println!("installed {}, it starts as this one ends", release.version);
      Ok(())
    }
    _ => Err(io::Error::new(ErrorKind::InvalidInput, USAGE)),
  }
}

fn main() -> ExitCode {
  // Held to the end: the relaunched version waits for it to go.
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
