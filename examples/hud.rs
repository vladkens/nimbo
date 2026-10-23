// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A HUD in an overlay ([nimbo::Config::overlay]): a recording pill at the bottom centre of the
//! screen with the pointer, over every app and Space, never taking the focus and letting the
//! mouse through: a red dot, a level meter of bars that follow a made-up level, and the time.
//! The pill and its shadow are drawn inside the overlay, transparent around them.
//!
//! `cargo run --release --example hud` shows it for [SHOW], hides it for [HIDDEN] and shows it
//! again, at the pointer's screen then, until it quits after [RUN]; `-- --shot hud.png` renders
//! it in the harness, transparent around the pill.

// A Windows app, not a console one: no terminal opens with it.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::time::Duration;

use nimbo::{
  Anchor, App, AppInfo, Color, Config, Font, Frame, Main, Presence, Rect, Weight, alpha, rgb,
};

pub const INFO: AppInfo =
  AppInfo { id: "com.example.nimbo-hud", name: "Nimbo HUD", presence: Presence::Accessory };
/// The overlay: the pill and a margin around it for its shadow, 44 points above the Dock or the
/// taskbar.
pub const CONFIG: Config = Config::new("HUD", PILL_W + 2.0 * MARGIN, PILL_H + 2.0 * MARGIN)
  .overlay(Anchor::Bottom(44.0))
  .hidden();
pub const PILL_W: f32 = 168.0;
pub const PILL_H: f32 = 40.0;
pub const MARGIN: f32 = 16.0;
pub const BARS: usize = 12;
pub const SHOW: Duration = Duration::from_secs(6);
pub const HIDDEN: Duration = Duration::from_secs(1);
pub const RUN: Duration = Duration::from_secs(10);

pub const FILL: Color = rgb(0x262628);
pub const DOT: Color = rgb(0xff453a);
const TEXT: Color = rgb(0xf2f2f7);

/// The pill: where it is in the overlay.
pub fn pill() -> Rect {
  Rect::new(MARGIN, MARGIN, PILL_W, PILL_H)
}

/// A level from 0 to 1 that wanders like speech, at `t` seconds.
pub fn level(t: f32) -> f32 {
  let s = (t * 3.1).sin() * 0.5 + (t * 7.3).sin() * 0.3 + (t * 13.7).sin() * 0.2;
  (s * 0.5 + 0.5).clamp(0.0, 1.0)
}

/// Recording since the first frame: the meter moves on each frame.
#[derive(Default)]
pub struct Hud {
  start: Option<Duration>,
}

impl App for Hud {
  fn paint(&mut self, f: &mut Frame) {
    let t = f.now.saturating_sub(*self.start.get_or_insert(f.now));
    let p = pill();
    let r = p.h / 2.0;
    f.shadow(Rect::new(p.x, p.y + 2.0, p.w, p.h), r, 6.0, alpha([0.0, 0.0, 0.0, 1.0], 0.35));
    f.rect(p, FILL, r);
    f.rect(Rect::new(p.x + 14.0, p.y + 15.0, 10.0, 10.0), DOT, 5.0);

    // The bars, the newest on the right, each the level a tick earlier than its neighbour's.
    for i in 0..BARS {
      let ago = (BARS - 1 - i) as f32 / 30.0;
      let h = 4.0 + 20.0 * level(t.as_secs_f32() - ago);
      let x = p.x + 34.0 + i as f32 * 6.0;
      f.rect(Rect::new(x, p.y + (p.h - h) / 2.0, 3.0, h), TEXT, 1.5);
    }

    let s = t.as_secs();
    let time = format!("{}:{:02}", s / 60, s % 60);
    f.text(p.x + 116.0, p.y + 25.0, &time, Font::mono(13.0).weight(Weight::Medium), TEXT);
    f.animate();
  }
}

fn start(main: &mut Main) {
  let hud = main.open(&CONFIG, Hud::default());
  hud.on_close(Main::quit);
  hud.front();
  let (id, proxy) = (hud.id(), main.proxy());
  let hud = move |main: &mut Main| main.window::<Hud>(id);
  proxy.after(SHOW, move |main| _ = hud(main).map(|w| w.hide()));
  proxy.after(SHOW + HIDDEN, move |main| _ = hud(main).map(|w| w.front()));
  proxy.after(RUN, Main::quit);
}

/// The shot: a second and a half into the recording.
pub fn shot(h: &mut nimbo::Harness<Hud>) {
  h.frame();
  h.wait(Duration::from_millis(1500));
}

fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  match args.is_empty() {
    true => nimbo::app(INFO, start),
    false => nimbo::run_args(CONFIG, Hud::default(), shot, &args),
  }
}
