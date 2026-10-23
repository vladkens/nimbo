// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The first app on the element tree with input: a count, − and + buttons and a reset, all
//! theme widgets (hover fades, pressed fills, Tab focus and Space). Clicks are read where the
//! buttons are built; the runtime runs the view once more right after a click, so the count
//! above them is never a frame behind.
//!
//! `cargo run --release --example counter [-- --shot <file.png>]`

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use nimbo::{App, Button, Config, Font, Id, Style, TextStyle, Ui, Variant, Weight};

#[derive(Default)]
pub struct Counter {
  pub count: i64,
}

pub const COUNT_ID: &str = "count";
pub const MINUS: &str = "minus";
pub const PLUS: &str = "plus";
pub const RESET: &str = "reset";

impl App for Counter {
  fn view(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    let count = TextStyle::new(Font::ui(64.0).weight(Weight::Semibold), t.text).nowrap();
    let big = t.button(Variant::Normal).height(36.0).min_w(64.0).font(Font::ui(22.0));
    ui.col(Style::new().grow().center().gap(16.0).bg(t.bg), |ui| {
      ui.text("COUNTER", t.section());
      ui.id(COUNT_ID).text(self.count, count);
      ui.row(Style::new().gap(10.0), |ui| {
        if Button::new("−").style(big).show(ui.id(MINUS)).clicked {
          self.count -= 1;
        }

        if Button::new("+").style(big).show(ui.id(PLUS)).clicked {
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
  nimbo::run(Config::new("counter", 360.0, 300.0), Counter::default(), |h| {
    h.frame();
    for _ in 0..3 {
      h.click_id(Id::of(PLUS));
    }

    h.wait(std::time::Duration::from_millis(500));
  });
}
