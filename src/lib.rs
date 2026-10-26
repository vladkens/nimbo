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
//! An app with no window, or more than one, starts with [app]: windows on demand ([Main::open])
//! and overlays, HUDs over every app ([Config::overlay]), work from other threads ([Proxy]), and
//! what a tray app needs besides: an icon with a menu ([tray]), global hotkeys ([hotkey]), its
//! folders ([paths]), launch at login ([autostart]) and alerts ([Main::alert]); and to reach
//! other apps: links and files opened or shown ([open]), the OS's permissions ([permissions]),
//! where the user types ([focus]), keys posted to the app in focus ([keys::post]) and the
//! clipboard, also taken and put back around a paste of the app's own ([SystemClipboard]).
//!
//! macOS 14 is the least it runs on (`NSApplication`'s `activate`, `NSView`'s display link): build
//! apps with `MACOSX_DEPLOYMENT_TARGET=14.0`.

pub mod activity;
pub mod app;
pub mod autostart;
pub mod clipboard;
pub mod cpu;
mod file_dialog;
pub mod focus;
pub mod harness;
pub mod hotkey;
pub mod keys;
pub mod open;
pub mod paths;
pub mod permissions;
pub mod platform;
mod png;
pub mod scene;
pub mod shell;
pub mod svg;
pub mod text;
pub mod tray;
pub mod ui;

use std::path::PathBuf;

pub use app::{
  App, Appearance, Clipboard, Clock, Cx, Event, Ime, MemClipboard, MouseButton, Phase, Runtime,
  WHEEL_LINE, Waker,
};
pub use clipboard::SystemClipboard;
pub use file_dialog::FileDialog;
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
pub use shell::{Anchor, AppInfo, Main, Presence, Proxy, Window, WindowId, app};
pub use text::edit::{Editor, Lines, Mono, Sel};
pub use text::{Font, Weight};
pub use ui::{
  Align, Answer, Behavior, Button, ButtonStyle, Choice, Click, Clip, ControlSize, Cursor, Dialog,
  Dim, Ease, FieldBehavior, FieldStyle, Float, Focus, FocusRing, FormRow, FormStyle, Icon, Id,
  List, ListResponse, PopUp, Press, Response, Row, Scroll, Select, Selection, Shadow, Sizes, Split,
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
  /// An overlay and where it shows ([Config::overlay]); `None` for a window.
  pub overlay: Option<Anchor>,
  /// The content runs under the title bar ([Config::unified]).
  pub unified: bool,
  /// The window's icon off macOS ([Config::icon]): its side and straight RGBA pixels.
  pub icon: Option<(u32, &'static [u8])>,
}

impl Config {
  /// The least content size by default: room for a small form.
  pub const MIN_SIZE: Size = Size::new(320.0, 240.0);

  pub const fn new(title: &'static str, w: f32, h: f32) -> Self {
    let (size, min_size) = (Size::new(w, h), Self::MIN_SIZE);
    let (resizable, hidden, overlay, unified, icon) = (true, false, None, false, None);
    Self { title, size, min_size, resizable, hidden, overlay, unified, icon }
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

  /// The icon in the window's title bar and taskbar entry on Windows and X11: `side` × `side`
  /// pixels of straight RGBA, rows top-down (256 serves every size the system shows). macOS
  /// shows the bundle's icon; Wayland the one of the app's `.desktop` entry, found by the app id
  /// ([AppInfo::id]).
  pub const fn icon(self, side: u32, px: &'static [u8]) -> Self {
    assert!(px.len() == side as usize * side as usize * 4, "an icon is side² RGBA pixels");
    Self { icon: Some((side, px)), ..self }
  }

  /// A window [Main::open] makes without showing it, until [Window::front].
  pub const fn hidden(self) -> Self {
    Self { hidden: true, ..self }
  }

  /// The title bar is part of the content, as in a macOS preferences window: the content runs
  /// under it and its size is the window's, the app draws the title in the band at the top
  /// ([Ui::titlebar]) and moves the window from its empty parts
  /// ([Ui::drags_window]); [Ui::toolbar_tabs] does both.
  ///
  /// Per OS: macOS a full-size content view under a transparent title bar, its title hidden (the
  /// Window menu and Mission Control still show it), the traffic lights over the content.
  /// Windows and Linux keep their title bar: [Ui::titlebar] is `None` there.
  pub const fn unified(self) -> Self {
    Self { unified: true, ..self }
  }

  /// An overlay, a HUD over everything: no frame, transparent where the app draws nothing
  /// ([cpu::Renderer::transparent]), of a fixed size; it never activates the app or takes the
  /// keys, the mouse goes through it, it shows on every Space and over full-screen apps.
  /// [Window::front] shows it at `anchor` on the screen with the pointer (again at each call),
  /// [Window::hide] hides it. Size it to the widest the app draws, as it draws its own shape and
  /// shadow inside.
  ///
  /// Per OS: macOS a borderless non-activating `NSPanel` at the status bar level. Windows a
  /// layered, click-through, topmost tool window with no activation, shown on the current
  /// virtual desktop. Linux on X11 an override-redirect popup (above all, on every workspace);
  /// on Wayland, where an app can't place itself, a layer-shell surface (`libgtk-layer-shell`,
  /// if installed: wlroots compositors, KDE) on the overlay layer at the anchor, on the output
  /// the compositor picks; without it (GNOME) a borderless toplevel that the compositor places
  /// and may focus and stack as any window.
  pub const fn overlay(self, anchor: Anchor) -> Self {
    Self { overlay: Some(anchor), resizable: false, ..self }
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
  /// The OS appearance the harness reports; dark by default.
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

/// The entry point of every example: [run_args] with the process's arguments.
pub fn run<A: App + 'static>(cfg: Config, app: A, script: impl FnOnce(&mut Harness<A>)) {
  let args: Vec<String> = std::env::args().skip(1).collect();
  run_args(cfg, app, script, &args);
}

/// With no `args` it opens a window and runs until it closes; with `--shot <file.png>` it runs
/// the app in the [Harness] at `cfg.size` (or `--size`) and `--scale` (default 2), first applying
/// `script` (the state the shot shows), then writing the PNG. Unknown arguments print the usage
/// and exit with status 2. For an app that parses its own command line and passes the rest on:
///
/// ```no_run
/// struct Settings;
/// impl nimbo::App for Settings {}
///
/// // `myapp settings --shot settings.png`: the app takes `settings`, the rest opens the window
/// // or renders its shot.
/// let args: Vec<String> = std::env::args().skip(1).collect();
/// if let Some((command, rest)) = args.split_first()
///   && command == "settings"
/// {
///   nimbo::run_args(nimbo::Config::new("Settings", 640.0, 480.0), Settings, |_| {}, rest);
/// }
/// ```
pub fn run_args<A: App + 'static>(
  cfg: Config,
  app: A,
  script: impl FnOnce(&mut Harness<A>),
  args: &[String],
) {
  let args = Args::parse(args).unwrap_or_else(|e| {
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
  h.cpu.transparent = cfg.overlay.is_some();
  // A unified window's title bar as high as macOS 26's.
  h.rt.set_titlebar(cfg.unified.then_some((32.0, cfg.title)));
  h.event(Event::Appearance(args.appearance.unwrap_or(Appearance::DARK)));
  h
}

#[cfg(test)]
mod tests {
  use super::*;

  /// An icon is `side`² RGBA pixels: other sizes are refused where the config is made.
  #[test]
  #[should_panic(expected = "side² RGBA")]
  fn an_icon_of_the_wrong_size_is_refused() {
    let _ = Config::new("a", 1.0, 1.0).icon(2, &[0; 4]);
  }

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

  /// An overlay's shot is transparent where it draws nothing, a window's black.
  #[test]
  fn overlay_shots_are_transparent() {
    struct Nothing;
    impl App for Nothing {}

    let args = parse("").unwrap();
    let cfg = Config::new("a", 10.0, 10.0);
    let corner = |cfg: &Config| harness(cfg, Nothing, &args).render().pixel(1.0, 1.0);
    assert_eq!(corner(&cfg), [0, 0, 0, 255]);
    assert_eq!(corner(&cfg.overlay(Anchor::Center)), [0; 4]);
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

  /// The arguments given, not the process's (the test runner's), pick the shot.
  #[test]
  fn run_args_shoots_with_the_arguments_given() {
    struct Blank;
    impl App for Blank {}

    let dir = std::env::temp_dir().join(format!("nimbo-run-args-{}", std::process::id()));
    let path = dir.join("shot.png");
    let args = ["--shot", path.to_str().unwrap(), "--scale", "1", "--size", "40x30"];
    let args: Vec<String> = args.map(String::from).to_vec();
    let mut scripted = false;
    run_args(Config::new("a", 100.0, 100.0), Blank, |_| scripted = true, &args);
    let png = std::fs::read(&path).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(scripted);
    assert_eq!(&png[1..4], b"PNG");
    // IHDR's width and height, big-endian: `--size` at `--scale`.
    let size = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
    assert_eq!((size(16), size(20)), (40, 30));
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
