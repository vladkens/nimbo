// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Below the widgets: the renderer and raw input, a page each, painted straight into the frame
//! at a fixed size: [shapes] (every scene primitive), [text] (fonts, scripts, subpixel
//! positions), [input] (pointer, keys, clipboard, cursor) and [pace] (frame pacing).
//!
//! `cargo run --release --example canvas [-- <page>]` opens the page (shapes when none is named)
//! in a window; `--shot <file.png>` after it renders the page without one.

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

pub mod input;
pub mod pace;
pub mod shapes;
pub mod text;

use nimbo::{Config, Size};

use self::input::Input;
use self::pace::Pace;
use self::shapes::Shapes;
use self::text::Text;

const USAGE: &str = "usage: canvas [shapes|text|input|pace] [--shot <file.png>] [options]";

fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  // A page's name first, shapes without one.
  let (page, rest) = match args.split_first() {
    Some((page, rest)) if !page.starts_with('-') => (page.as_str(), rest),
    _ => ("shapes", &args[..]),
  };

  let cfg = |title, size: Size| Config::new(title, size.w, size.h);
  match page {
    "shapes" => nimbo::run_args(cfg("shapes", shapes::SIZE), Shapes::default(), |_| {}, rest),
    "text" => nimbo::run_args(cfg("text", text::SIZE), Text::default(), |_| {}, rest),
    "input" => nimbo::run_args(cfg("input", input::SIZE), Input::default(), |_| {}, rest),
    "pace" => nimbo::run_args(cfg("pace", pace::SIZE).fixed(), Pace::default(), |_| {}, rest),
    _ => {
      eprintln!("unknown page {page}\n{USAGE}");
      std::process::exit(2);
    }
  }
}
