// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! nimbo: a small custom-drawn UI toolkit. Its own CPU renderer for drawing; the OS for the
//! window, input and text behind [platform] (AppKit and CoreText on macOS, Win32 and DirectWrite
//! on Windows, GTK 3 with fontconfig, FreeType and HarfBuzz on Linux), nothing else.
//!
//! An [App] gets [Event]s and builds each frame as an element tree on a [Ui] (laid out and painted
//! by [ui]) and/or paints a [Scene] and [text] through a [Frame]; a [Runtime] drives it, fed by
//! the window ([platform]) or by the headless [Harness], both drawing with the CPU renderer
//! ([cpu]). Examples start with [run], which opens a window, or with `--shot <file.png>` runs the
//! harness instead (in the dark appearance unless `--appearance light`).
//!
//! An app with no window, or more than one, starts with [app]: windows on demand ([Main::open]),
//! work from other threads ([Proxy]), and what a tray app needs besides: an icon with a menu
//! ([tray]), global hotkeys ([hotkey]), its folders ([paths]) and launch at login ([autostart]).
//!
//! macOS 14 is the least it runs on (`NSApplication`'s `activate`, `NSView`'s display link): build
//! apps with `MACOSX_DEPLOYMENT_TARGET=14.0`.

pub mod app;
pub mod autostart;
pub mod cpu;
pub mod harness;
pub mod hotkey;
pub mod keys;
pub mod paths;
pub mod platform;
mod png;
pub mod scene;
pub mod shell;
pub mod text;
pub mod tray;
pub mod ui;

use std::path::PathBuf;

pub use app::{
  App, Appearance, Clipboard, Clock, Cx, Event, Ime, MemClipboard, MouseButton, Phase, Runtime,
  WHEEL_LINE, Waker,
};
pub use harness::Harness;
/// Keys' W3C UI Events names: [Code] a key's place, [NamedKey] a key that types nothing,
/// [Modifiers] the modifier keys held.
pub use keyboard_types::{self, Code, Modifiers, NamedKey};
pub use keys::{
  Binding, CMD, Command, Delete, EDIT_KEYS, Hotkey, Key, KeyEvent, Move, SELECT_KEYS, Shortcut,
  bind, shortcut,
};
pub use scene::{
  Color, Frame, ImageId, Images, Point, Quad, Rect, Scene, Segment, Size, alpha, mix, rgb,
};
pub use shell::{AppInfo, Main, Presence, Proxy, Window, WindowId, app};
pub use text::edit::{Editor, Lines, Mono, Sel};
pub use text::{Font, Weight};
pub use ui::{
  Align, Answer, Behavior, Button, ButtonStyle, Choice, Click, ControlSize, Cursor, Dialog, Dim,
  Ease, FieldBehavior, FieldStyle, Float, Focus, FocusRing, FormRow, FormStyle, Icon, Id, List,
  ListResponse, PopUp, Press, Response, Row, Scroll, Select, Selection, Shadow, Sizes, Split,
  Style, Tabs, TextInput, TextLayout, TextStyle, Theme, Tile, Tone, Transition, TypeSelect, Ui,
  Variant, grow, pct, px, rigid,
};

/// A window's title and initial content size in points; the harness renders at this size.
pub struct Config {
  pub title: &'static str,
  pub size: Size,
  /// The least content size the window resizes to, in points ([Config::min]).
  pub min_size: Size,
  /// The window can be resized (the default).
  pub resizable: bool,
  /// Opened without showing ([Config::hidden]) until [Window::front].
  pub hidden: bool,
}

impl Config {
  /// The least content size by default: room for a small form.
  pub const MIN_SIZE: Size = Size::new(320.0, 240.0);

  pub const fn new(title: &'static str, w: f32, h: f32) -> Self {
    let (size, min_size) = (Size::new(w, h), Self::MIN_SIZE);
    Self { title, size, min_size, resizable: true, hidden: false }
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

  /// A window [Main::open] makes without showing it, until [Window::front].
  pub const fn hidden(self) -> Self {
    Self { hidden: true, ..self }
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
  let Some(path) = &args.shot else { return shell::window(&cfg, Box::new(app)) };

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

    // The app hears the first one, the default dark too, and the theme follows it.
    let cfg = Config::new("a", 100.0, 100.0);
    let seen = |s: &str| {
      let h = harness(&cfg, Seen::default(), &parse(s).unwrap());
      (h.app().0, h.rt.ui.appearance())
    };
    assert_eq!(seen(""), (Some(Appearance::DARK), Appearance::DARK));
    assert_eq!(seen("--appearance light"), (Some(Appearance::LIGHT), Appearance::LIGHT));
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
