// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! nimbo: a small custom-drawn UI toolkit. Its own CPU renderer for drawing; the OS for the
//! window behind [platform] (AppKit on macOS, Win32 on Windows, Wayland or X11 on Linux), nothing
//! else.
//!
//! An [App] gets [Event]s and paints a [Scene] through a [Frame]; a [Runtime] drives it, fed by
//! the window ([platform]) or by the headless [Harness], both drawing with the CPU renderer
//! ([cpu]). Examples start with [run], which opens a window, or with `--shot <file.png>` runs the
//! harness instead (in the dark appearance unless `--appearance light`).

pub mod app;
pub mod cpu;
pub mod harness;
pub mod platform;
mod png;
pub mod scene;

use std::path::PathBuf;

pub use app::{App, Appearance, Clock, Cx, Event, Runtime, Waker};
pub use harness::Harness;
pub use scene::{
  Color, Frame, ImageId, Images, Point, Quad, Rect, Scene, Segment, Size, alpha, mix, rgb,
};

/// A window's title and initial content size in points; the harness renders at this size.
pub struct Config {
  pub title: &'static str,
  pub size: Size,
  /// The least content size the window resizes to, in points ([Config::min]).
  pub min_size: Size,
  /// The window can be resized (the default).
  pub resizable: bool,
}

impl Config {
  /// The least content size by default: room for a small form.
  pub const MIN_SIZE: Size = Size::new(320.0, 240.0);

  pub const fn new(title: &'static str, w: f32, h: f32) -> Self {
    let (size, min_size) = (Size::new(w, h), Self::MIN_SIZE);
    Self { title, size, min_size, resizable: true }
  }

  /// The least content size the user can resize the window to, [Config::MIN_SIZE] unless set.
  pub const fn min_size(self, w: f32, h: f32) -> Self {
    Self { min_size: Size::new(w, h), ..self }
  }

  /// The least content size as the platform applies it: never more than the initial size.
  pub const fn min(&self) -> Size {
    Size::new(self.min_size.w.min(self.size.w), self.min_size.h.min(self.size.h))
  }

  /// A window of a fixed size: a calculator, a dialog.
  pub const fn fixed(self) -> Self {
    Self { resizable: false, ..self }
  }
}

const USAGE: &str = "usage: [--shot <file.png>] [--scale <n>] [--size <w>x<h>] \
  [--appearance light|dark]";

#[derive(Debug, Default, PartialEq)]
struct Args {
  shot: Option<PathBuf>,
  scale: f32,
  /// The view size in points instead of the config's.
  size: Option<Size>,
  /// The OS appearance the harness reports; dark by default, as before the theme followed it.
  appearance: Option<Appearance>,
}

impl Args {
  fn parse(args: &[String]) -> Result<Self, String> {
    let mut out = Self { scale: 2.0, ..Self::default() };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
      match arg.as_str() {
        "--shot" => out.shot = Some(it.next().ok_or("--shot needs a path")?.into()),
        "--scale" => {
          let v = it.next().ok_or("--scale needs a number")?;
          out.scale = v.parse().map_err(|e| format!("--scale: {e}"))?;
          if !positive(out.scale) {
            return Err(format!("--scale: {v} is not a positive number"));
          }
        }
        "--size" => {
          let v = it.next().ok_or("--size needs <w>x<h>")?;
          let (w, h) = v.split_once('x').ok_or("--size needs <w>x<h>")?;
          let num = |s: &str| match s.parse::<f32>() {
            Ok(n) if positive(n) => Ok(n),
            Ok(_) => Err(format!("--size: {s} is not a positive number")),
            Err(e) => Err(format!("--size: {e}")),
          };
          out.size = Some(Size::new(num(w)?, num(h)?));
        }
        "--appearance" => {
          out.appearance = Some(match it.next().map(String::as_str) {
            Some("light") => Appearance::LIGHT,
            Some("dark") => Appearance::DARK,
            _ => return Err("--appearance needs light or dark".into()),
          });
        }
        _ => return Err(format!("unknown argument {arg}")),
      }
    }

    Ok(out)
  }
}

/// A finite number above zero: a scale or a size the harness can render.
fn positive(n: f32) -> bool {
  n.is_finite() && n > 0.0
}

/// The entry point of every example. With no arguments it opens a window; with `--shot` it runs
/// the app in the [Harness] at `cfg.size` and `--scale` (default 2), first applying `script` (the
/// state the shot shows), then writing the PNG.
pub fn run<A: App + 'static>(cfg: Config, app: A, script: impl FnOnce(&mut Harness<A>)) {
  let argv: Vec<String> = std::env::args().skip(1).collect();
  let args = Args::parse(&argv).unwrap_or_else(|e| {
    eprintln!("{e}\n{USAGE}");
    std::process::exit(2);
  });
  let Some(path) = &args.shot else { return platform::run(&cfg, Box::new(app)) };

  let mut h = harness(&cfg, app, &args);
  script(&mut h);
  if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
    std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
  }

  h.shot(path).unwrap_or_else(|e| panic!("{e}"));
  println!("{}: wrote {}", cfg.title, path.display());
}

/// The harness [run] shoots in, at the size and scale of `args`; the app first hears the
/// appearance, dark unless `--appearance` says otherwise, as a window tells it the OS's.
fn harness<A: App>(cfg: &Config, app: A, args: &Args) -> Harness<A> {
  let mut h = Harness::new(app, args.size.unwrap_or(cfg.size), args.scale);
  h.event(Event::Appearance(args.appearance.unwrap_or(Appearance::DARK)));
  h
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn shots_tell_the_appearance() {
    #[derive(Default)]
    struct Seen(Option<Appearance>);
    impl App for Seen {
      fn event(&mut self, _cx: &mut Cx, ev: &Event) -> bool {
        if let Event::Appearance(a) = ev {
          self.0 = Some(*a);
        }

        true
      }
    }

    let cfg = Config::new("a", 100.0, 100.0);
    let seen = |s: &str| harness(&cfg, Seen::default(), &parse(s).unwrap()).app().0;
    assert_eq!(seen(""), Some(Appearance::DARK));
    assert_eq!(seen("--appearance light"), Some(Appearance::LIGHT));
  }

  fn parse(s: &str) -> Result<Args, String> {
    Args::parse(&s.split_whitespace().map(String::from).collect::<Vec<_>>())
  }

  #[test]
  fn args() {
    let a = parse("--shot a.png --scale 1").unwrap();
    assert_eq!(a, Args { shot: Some("a.png".into()), scale: 1.0, ..Args::default() });
    assert_eq!(parse("").unwrap(), Args { scale: 2.0, ..Args::default() });
    assert_eq!(parse("--size 1280x800").unwrap().size, Some(Size::new(1280.0, 800.0)));
    assert_eq!(parse("--appearance light").unwrap().appearance, Some(Appearance::LIGHT));
    assert!(parse("--appearance blue").is_err());
    assert!(parse("--size 10").is_err());
    for bad in ["0", "-1", "inf", "NaN"] {
      assert!(parse(&format!("--scale {bad}")).is_err(), "--scale {bad}");
      assert!(parse(&format!("--size {bad}x10")).is_err(), "--size {bad}x10");
      assert!(parse(&format!("--size 10x{bad}")).is_err(), "--size 10x{bad}");
    }

    assert!(parse("--shot").is_err());
    assert!(parse("--what").is_err());
  }

  #[test]
  fn min_size_is_capped_by_the_size() {
    assert_eq!(Config::new("a", 880.0, 640.0).min(), Config::MIN_SIZE);
    assert_eq!(Config::new("a", 200.0, 100.0).min(), Size::new(200.0, 100.0));
    assert_eq!(
      Config::new("a", 880.0, 640.0).min_size(720.0, 480.0).min(),
      Size::new(720.0, 480.0)
    );
  }
}
