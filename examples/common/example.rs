// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! How every example starts: [run] opens its window ([run_with] then sets what the app sets on
//! [Main] for it, its menu bar), or with `--shot <file.png>` renders it in the harness instead
//! (in the dark appearance unless `--appearance light`, in English unless `--locale <tag>`).
//! Each example includes it with `#[path]`; `tests/example.rs` tests it.

use std::path::PathBuf;

use nimbo::{
  App, AppInfo, Appearance, Config, Event, Harness, Locale, Main, Size, Window, harness,
};

const USAGE: &str = "usage: [--shot <file.png>] [--scale <n>] [--size <w>x<h>] \
  [--appearance light|dark] [--locale <tag>|pseudo]";

#[derive(Debug, Default, PartialEq)]
pub struct Args {
  pub shot: Option<PathBuf>,
  pub scale: f32,
  /// The view size in points instead of the config's.
  pub size: Option<Size>,
  /// The OS appearance the harness reports; dark by default.
  pub appearance: Option<Appearance>,
  /// The language ([Locale]): the system's in a window, English in a shot unless given.
  pub locale: Option<Locale>,
}

impl Args {
  pub fn parse(args: &[String]) -> Result<Self, String> {
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

/// A finite number above zero: a scale or a size the harness can render.
fn positive(n: f32) -> bool {
  n.is_finite() && n > 0.0
}

/// With no `args` it opens a window and runs until it closes; with `--shot <file.png>` it runs
/// the app in the [Harness] at `cfg.size()` (or `--size`) and `--scale` (default 2), first
/// applying `script` (the state the shot shows), then writing the PNG. Unknown arguments print
/// the usage and exit with status 2. An example with arguments of its own takes them and passes
/// the rest.
pub fn run<A: App + 'static>(
  cfg: Config,
  app: A,
  args: &[String],
  script: impl FnOnce(&mut Harness<A>),
) {
  run_with(cfg, app, args, script, |_, _| {});
}

/// [run], with `start` running once the window opens (not in a shot): what the app sets on
/// [Main] for its window, as its menu bar ([Main::set_commands]).
pub fn run_with<A: App + 'static>(
  cfg: Config,
  app: A,
  args: &[String],
  script: impl FnOnce(&mut Harness<A>),
  start: impl FnOnce(&mut Main, &Window<A>),
) {
  let args = Args::parse(args).unwrap_or_else(|e| {
    eprintln!("{e}\n{USAGE}");
    std::process::exit(2);
  });
  let Some(path) = &args.shot else {
    return window(cfg, app, args.locale, start);
  };

  let mut h = shot(&cfg, app, &args);
  script(&mut h);
  if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
    std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
  }

  h.shot(path).unwrap_or_else(|e| panic!("{e}"));
  println!("{}: wrote {}", cfg.title(), path.display());
}

/// One window running `app`, the app named after the executable, quitting when it closes;
/// `start` runs once it is open.
fn window<A: App + 'static>(
  cfg: Config,
  app: A,
  locale: Option<Locale>,
  start: impl FnOnce(&mut Main, &Window<A>),
) {
  let exe = std::env::current_exe().ok();
  let stem = exe.as_ref().and_then(|p| Some(p.file_stem()?.to_str()?.to_owned()));
  // The loop runs once per process: the strings live as long as it.
  let id: &'static str = Box::leak(stem.unwrap_or_else(|| "nimbo".into()).into_boxed_str());
  nimbo::app(AppInfo::new(id, cfg.title()), |main| {
    if let Some(l) = locale {
      main.set_locale(l);
    }

    let w = main.open(&cfg, app);
    w.on_close(Main::quit);
    start(main, &w);
  });
}

/// The harness [run] shoots in, at the size and scale of `args`, in its language (English unless
/// `--locale` says otherwise); the app first hears the appearance, dark unless `--appearance`
/// says otherwise, as a window tells it the OS's.
pub fn shot<A: App>(cfg: &Config, app: A, args: &Args) -> Harness<A> {
  let mut h = Harness::new(app, args.size.unwrap_or(cfg.size()), args.scale);
  h.set_locale(args.locale.unwrap_or(Locale::Tag("en")), cfg.get_localizations());
  h.set_transparent(cfg.get_overlay().is_some());
  // A unified window's title bar as macOS's.
  h.set_titlebar(cfg.is_unified().then(|| harness::mac_titlebar(cfg.title(), false)));
  h.event(Event::Appearance(args.appearance.unwrap_or(Appearance::DARK)));
  h
}
