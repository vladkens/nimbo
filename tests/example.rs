// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/common/example.rs`, how every example starts: its arguments, and the shot it
//! renders in the harness with the appearance, the transparency and the size they give.

#[path = "../examples/common/example.rs"]
mod example;

use example::Args;
use nimbo::{Anchor, App, Appearance, Config, Cx, Event, Locale, Size, Ui};

#[test]
fn shots_tell_the_appearance() {
  #[derive(Default)]
  struct Seen(Option<Appearance>, Option<Appearance>);
  impl App for Seen {
    fn event(&mut self, _cx: &mut Cx, ev: &Event) -> bool {
      if let Event::Appearance(a) = ev {
        self.0 = Some(*a);
      }

      true
    }

    fn view(&mut self, ui: &mut Ui) {
      self.1 = Some(ui.appearance());
    }
  }

  // The app hears the first one, the default dark too, and the theme follows it.
  let cfg = Config::new("a", 100.0, 100.0);
  let seen = |s: &str| {
    let mut h = example::shot(&cfg, Seen::default(), &parse(s).unwrap());
    h.frame();
    (h.app().0, h.app().1.unwrap())
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
  let corner = |cfg: &Config| example::shot(cfg, Nothing, &args).render().pixel(1.0, 1.0);
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
fn run_shoots_with_the_arguments_given() {
  struct Blank;
  impl App for Blank {}

  let dir = std::env::temp_dir().join(format!("nimbo-example-run-{}", std::process::id()));
  let path = dir.join("shot.png");
  let args = ["--shot", path.to_str().unwrap(), "--scale", "1", "--size", "40x30"];
  let args: Vec<String> = args.map(String::from).to_vec();
  let mut scripted = false;
  example::run(Config::new("a", 100.0, 100.0), Blank, &args, |_| scripted = true);
  let png = std::fs::read(&path).unwrap();
  std::fs::remove_dir_all(&dir).unwrap();
  assert!(scripted);
  assert_eq!(&png[1..4], b"PNG");
  // IHDR's width and height, big-endian: `--size` at `--scale`.
  let size = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
  assert_eq!((size(16), size(20)), (40, 30));
}
