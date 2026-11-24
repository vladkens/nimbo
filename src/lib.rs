// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! nimbo: a small custom-drawn UI toolkit. Its own CPU renderer for drawing; the OS for the
//! window, input and text behind [platform] (AppKit and CoreText on macOS, Win32 and DirectWrite
//! on Windows, GTK 3 with fontconfig, FreeType and HarfBuzz on Linux), nothing else.
//!
//! An [App] gets [Event]s and builds each frame as an element tree on a [Ui] (laid out and painted
//! by [ui]; [App::view] says how often a view runs and what it may change) and/or paints shapes,
//! images and [text] through a [Frame]; the runtime drives it, fed by the window ([platform]) or
//! by the headless [Harness], both drawing with nimbo's CPU renderer. The scene, the renderer
//! and the laid-out tree are the crate's own, so they can change without breaking an app.
//! Its strings show in the user's language from the app's gettext catalogs ([locale], [t!]).
//! Examples start with [run_example], which opens a window, or with `--shot <file.png>` runs the
//! harness instead (in the dark appearance unless `--appearance light`, in English unless
//! `--locale <tag>`).
//!
//! An app with no window, or more than one, starts with [app()]: windows on demand ([Main::open])
//! and overlays, HUDs over every app ([Config::overlay]), the menu bar's commands
//! ([Main::set_commands]), work from other threads ([Proxy]), and what a tray app needs besides:
//! an icon with a menu ([tray]), global hotkeys ([hotkey]), one instance that takes messages from
//! a second launch ([instance], [Main::serve]), its folders and
//! the user's Downloads ([paths]), launch at login ([autostart]), its launcher entry on Linux
//! ([desktop]), alerts ([Main::alert]), its command line's console on Windows
//! ([attach_console]) and an uninstall that undoes what nimbo set up ([uninstall]); and to reach
//! other apps: links and files opened or shown ([open]), the OS's permissions ([permissions]),
//! where the user types ([focus]) and with which keyboard layout ([input_source]), keys posted to
//! the app in focus ([keys::post]), the clipboard, also taken and put back around a paste of the
//! app's own ([SystemClipboard]), and that paste itself, which gives the clipboard back ([paste]);
//! photos decoded by the OS at the size they show ([Bitmap]); and the hardware: how a microphone is
//! attached and the laptop's lid ([hardware]), and the system's changes, such as a microphone
//! plugged or the lid closed, as they come ([system], [Main::watch]).
//!
//! macOS 11 is the least it runs on, rustc's default deployment target, so an app sets none;
//! newer APIs are used where the system has them (the activation, the display link and menu
//! section headers on 14+, launch at login on 13+).
//!
//! What an app reads and the enums that may grow are `#[non_exhaustive]`, so a minor release can
//! add to them: a `match` on one keeps a `_` arm, and such a struct is read or changed field by
//! field, never built whole.
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(
  not(all(feature = "full", feature = "harness")),
  expect(
    dead_code,
    reason = "what a feature left off uses; the full build (clippy, tests) checks it"
  )
)]

pub mod activity;
pub mod app;
#[cfg(feature = "autostart")]
pub mod autostart;
#[cfg(feature = "bitmap")]
mod bitmap;
pub mod clipboard;
mod commands;
pub(crate) mod cpu;
#[cfg(feature = "autostart")]
pub mod desktop;
mod error;
mod event;
mod file_dialog;
pub mod focus;
pub(crate) mod geom;
#[cfg(feature = "system")]
pub mod hardware;
#[cfg(feature = "harness")]
pub mod harness;
#[cfg(feature = "hotkey")]
pub mod hotkey;
mod info;
#[cfg(feature = "system")]
pub mod input_source;
#[cfg(feature = "instance")]
pub mod instance;
pub mod keys;
pub mod locale;
mod menu;
pub mod open;
#[cfg(feature = "paste")]
pub mod paste;
pub mod paths;
#[cfg(feature = "permissions")]
pub mod permissions;
pub mod platform;
#[cfg(feature = "png")]
mod png;
mod runloop;
pub mod scene;
pub mod shell;
pub mod svg;
#[cfg(feature = "system")]
pub mod system;
pub mod text;
#[cfg(feature = "tray")]
pub mod tray;
pub mod ui;
#[cfg(feature = "autostart")]
pub mod uninstall;
mod window;

#[cfg(feature = "harness")]
use std::path::PathBuf;

pub use app::{
  App, Appearance, Clipboard, ColorScheme, Cx, Event, Ime, MemClipboard, MouseButton, Phase,
  WHEEL_LINE, Waker,
};
#[cfg(feature = "bitmap")]
pub use bitmap::Bitmap;
pub use clipboard::SystemClipboard;
pub use commands::{CommandGroupPlacement, Commands};
pub use error::Error;
pub use file_dialog::{FileDialog, FileDialogKind};
#[cfg(feature = "harness")]
pub use harness::Harness;
pub use keys::{
  Binding, CMD, Code, Command, Delete, EDIT_KEYS, Extent, Hotkey, Key, KeyEvent, Modifiers, Move,
  NamedKey, Os, SELECT_KEYS, Shortcut, bind, shortcut,
};
pub use locale::{Locale, Localizations, Localized};
pub use menu::{ButtonRole, Item, Menu};
#[cfg(feature = "png")]
pub use png::Png;
pub use scene::{Color, Frame, ImageId, Images, Point, Quad, Rect, Size, alpha, mix, rgb};
pub use shell::{Anchor, AppInfo, Main, Presence, Proxy, Window, WindowId, app, attach_console};
pub use text::edit::{Editor, Lines, Mono, Sel};
pub use text::{Design, Font, Weight};
pub use ui::{
  Align, Axes, Axis, Click, Clip, ContentMode, ControlSize, CoordinateSpace, Cursor, Dim, Ease,
  Enabled, EnvironmentValue, Float, Focus, FocusDirection, FocusRing, Icon, IconPaint, Id,
  ImageSource, LinePattern, NativeMenu, Press, Response, Scroll, Shadow, Style, TextInput,
  TextLines, TextStyle, Transition, TruncationMode, Ui, UnitPoint, grow, pct, px, rigid,
};
#[cfg(feature = "styled")]
pub use ui::{
  Answer, BorderedButtonStyle, BorderedProminentButtonStyle, BorderlessButtonStyle,
  ButtonToggleStyle, CheckboxToggleStyle, ControlBorder, Dialog, FormRow, FormStyle,
  InsetGroupedListStyle, InsetListStyle, LinearSliderStyle, MenuPickerStyle,
  RoundedBorderTextFieldStyle, Section, SegmentedPickerStyle, Sizes, Split, SwitchToggleStyle,
  Tabs, Theme, Tone, ToolbarItemPlacement, TooltipHelpStyle,
};
#[cfg(feature = "headless")]
pub use ui::{
  Behavior, Button, ButtonConfiguration, ButtonStyle, Content, DialogBehavior, EditMenu, Editing,
  FieldBehavior, FieldKind, FieldStyle, HelpConfiguration, HelpStyle, InlinePickerStyle, List,
  ListBehavior, ListConfiguration, ListLayout, ListResponse, ListRowConfiguration, ListRows,
  ListStyle, MenuPlacement, Picker, PickerConfiguration, PickerStyle, PlainButtonStyle,
  PlainHelpStyle, PlainListStyle, PlainSliderStyle, PlainTextFieldStyle, PlainToggleStyle, Row,
  ScrollBar, ScrollBehavior, ScrollerKnob, SectionHeader, Selection, Slider, SliderConfiguration,
  SliderStyle, SplitBehavior, Tag, TextField, TextFieldConfiguration, TextFieldStyle,
  ToggleConfiguration, ToggleStyle, TypeSelect,
};
pub use window::{Config, TitleBar, WindowToolbarStyle};

#[cfg(feature = "harness")]
const USAGE: &str = "usage: [--shot <file.png>] [--scale <n>] [--size <w>x<h>] \
  [--appearance light|dark] [--locale <tag>|pseudo]";

#[cfg(feature = "harness")]
#[derive(Debug, Default, PartialEq)]
struct Args {
  shot: Option<PathBuf>,
  scale: f32,
  /// The view size in points instead of the config's.
  size: Option<Size>,
  /// The OS appearance the harness reports; dark by default.
  appearance: Option<Appearance>,
  /// The language ([Locale]): the system's in a window, English in a shot unless given.
  locale: Option<Locale>,
}

#[cfg(feature = "harness")]
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
          let scheme = it.next().ok_or("--appearance needs light or dark")?;
          let scheme = scheme.parse().map_err(|e| format!("--appearance: {e}"))?;
          out.appearance = Some(Appearance { scheme, accent: None });
        }
        "--locale" => {
          let tag = it.next().ok_or("--locale needs a language tag")?;
          // Read once per process: it lives as long as it.
          out.locale = Some(Locale::Tag(Box::leak(tag.clone().into_boxed_str())));
        }
        _ => return Err(format!("unknown argument {arg}")),
      }
    }

    Ok(out)
  }
}

#[cfg(feature = "harness")]
/// A finite number above zero: a scale or a size the harness can render.
fn positive(n: f32) -> bool {
  n.is_finite() && n > 0.0
}

#[cfg(feature = "harness")]
/// The entry point of every example: [run_args] with the process's whole command line, exiting
/// with the usage on any argument it doesn't know; an app with its own arguments takes them and
/// passes the rest to [run_args].
pub fn run_example<A: App + 'static>(cfg: Config, app: A, script: impl FnOnce(&mut Harness<A>)) {
  let args: Vec<String> = std::env::args().skip(1).collect();
  run_args(cfg, app, script, &args);
}

#[cfg(feature = "harness")]
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
  let Some(path) = &args.shot else {
    if let Some(l) = args.locale {
      locale::set(l);
    }

    return shell::window(&cfg, Box::new(app));
  };

  let mut h = harness(&cfg, app, &args);
  script(&mut h);
  if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
    std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
  }

  h.shot(path).unwrap_or_else(|e| panic!("{e}"));
  println!("{}: wrote {}", cfg.title, path.display());
}

#[cfg(feature = "harness")]
/// The harness [run_args] shoots in, at the size and scale of `args`, in its language (English
/// unless `--locale` says otherwise); the app first hears the appearance, dark unless
/// `--appearance` says otherwise, as a window tells it the OS's.
fn harness<A: App>(cfg: &Config, app: A, args: &Args) -> Harness<A> {
  let mut h = Harness::new(app, args.size.unwrap_or(cfg.size), args.scale);
  h.set_locale(args.locale.unwrap_or(Locale::Tag("en")), cfg.localizations);
  h.set_transparent(cfg.overlay.is_some());
  // A unified window's title bar as macOS's.
  h.set_titlebar(cfg.unified.then(|| harness::mac_titlebar(cfg.title, false)));
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
    assert_eq!(parse("--locale ru").unwrap().locale, Some(Locale::Tag("ru")));
    assert!(parse("--locale").is_err());
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
