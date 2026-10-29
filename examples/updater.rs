// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The update's window ([nimbo::update::dialog]) in each of its states, about a fake release with
//! no network: `cargo run --release --example updater [-- <state>] [--shot <file.png>]`, the
//! state one of available (the default), checking, up-to-date, downloading, restarting, failed.
//! Its buttons walk the states as an app's updater would: Install downloads, Retry checks.

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use nimbo::update::Release;
use nimbo::update::dialog::{self, About, Act, State};
use nimbo::{App, Png, Ui};

pub const ICON: Png = Png::icon(include_bytes!("../docs/brand/mark-iris.png"));

/// The window's state and what the person chose in it.
pub struct Updater {
  pub state: State,
  pub auto: bool,
  pub acts: Vec<Act>,
}

/// The state a name stands for, `None` for an unknown one.
pub fn state(name: &str) -> Option<State> {
  let release = Release::test("1.2.0");
  Some(match name {
    "available" => State::Available(release),
    "checking" => State::Checking,
    "up-to-date" => State::UpToDate,
    "downloading" => State::Downloading(release, 42),
    "restarting" => State::Restarting(release),
    "failed" => State::Failed(Some(release), "the download stopped: the network is down".into()),
    _ => return None,
  })
}

impl App for Updater {
  fn view(&mut self, ui: &mut Ui) {
    let about = About { name: "Counter", version: "1.1.0", icon: Some(ICON) };
    let Some(act) = dialog::view(ui, &about, &self.state, self.auto) else { return };
    self.acts.push(act);
    match act {
      Act::Install => self.state = state("downloading").unwrap(),
      Act::Retry => self.state = State::Checking,
      Act::Auto(on) => self.auto = on,
      Act::Later | Act::Skip | Act::Notes | Act::Close => {}
    }
  }
}

fn main() {
  let mut args: Vec<String> = std::env::args().skip(1).collect();
  let name = match args.first() {
    Some(a) if !a.starts_with("--") => args.remove(0),
    _ => "available".into(),
  };

  let Some(state) = state(&name) else {
    eprintln!("unknown state {name}");
    std::process::exit(2);
  };

  let app = Updater { state, auto: true, acts: Vec::new() };
  nimbo::run_args(dialog::config("Software Update"), app, |h| _ = h.frame(), &args);
}
