// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The first app on the element tree with input: a count, − and + buttons and a reset, theme
//! widgets (hover fades, pressed fills, Tab focus and Space), − and + in a button style of the
//! app's own, big keys of the theme's colours. Clicks are read where the
//! buttons are built; the runtime runs the view once more right after a click, so the count
//! above them is never a frame behind. Off macOS the window carries an icon made at compile
//! time, a blue disc.
//!
//! `cargo run --release --example counter [-- --shot <file.png>]`

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use nimbo::{
  App, ButtonConfiguration, ButtonStyle, Config, Font, Id, Style, TextStyle, Ui, Weight, mix, px,
  rigid,
};

#[derive(Default)]
pub struct Counter {
  pub count: i64,
}

/// The window's icon on Windows and X11: a blue disc, 64 pixels square.
pub const ICON: [u8; 64 * 64 * 4] = {
  let mut px = [0; 64 * 64 * 4];
  let mut i = 0;
  while i < 64 * 64 {
    let (x, y) = ((i % 64) as i32 * 2 - 63, (i / 64) as i32 * 2 - 63);
    if x * x + y * y <= 60 * 60 {
      (px[i * 4], px[i * 4 + 1], px[i * 4 + 2], px[i * 4 + 3]) = (0x0a, 0x84, 0xff, 0xff);
    }

    i += 1;
  }

  px
};

/// A big key, 36 high and 64 wide at least, in the theme's push button colours: its fill
/// hovered and held, the label in 22 points.
struct Key;

impl ButtonStyle for Key {
  fn make_body(&self, ui: &mut Ui, mut c: ButtonConfiguration) {
    let t = ui.theme();
    let hot = ui.ctl_hot(c.id(), c.is_hovered());
    let bg = if c.is_pressed() { t.pressed() } else { mix(t.raised(), t.hover(), hot) };
    let s = Style::new().w(rigid()).h(px(36.0)).min_w(64.0).pad_xy(12.0, 0.0).gap(5.0).center();
    let s = s.radius(t.radius()).border(t.control_border().width(), t.line()).bg(bg);
    ui.row(s, |ui| c.label(ui, TextStyle::new(Font::ui(22.0), t.text()).line_limit(1)));
  }
}

pub const COUNT_ID: &str = "count";
pub const MINUS: &str = "minus";
pub const PLUS: &str = "plus";
pub const RESET: &str = "reset";

impl App for Counter {
  fn view(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    let count = TextStyle::new(Font::ui(64.0).weight(Weight::Semibold), t.text()).nowrap();
    ui.col(Style::new().grow().center().gap(16.0).bg(t.bg()), |ui| {
      ui.text("COUNTER", t.section());
      ui.id(COUNT_ID).text(self.count, count);
      ui.button_style(&Key).row(Style::new().gap(10.0), |ui| {
        if ui.id(MINUS).button("−").clicked {
          self.count -= 1;
        }

        if ui.id(PLUS).button("+").clicked {
          self.count += 1;
        }
      });
      if ui.disabled(self.count == 0).id(RESET).button("Reset").clicked {
        self.count = 0;
      }
    });
  }
}

fn main() {
  // The shot: counted to 3, the pointer resting on + after the fade.
  let cfg = Config::new("counter", 360.0, 300.0).icon(64, &ICON);
  nimbo::run_example(cfg, Counter::default(), |h| {
    h.frame();
    for _ in 0..3 {
      h.click_id(Id::of(PLUS));
    }

    h.wait(std::time::Duration::from_millis(500));
  });
}
