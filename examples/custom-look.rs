// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A whole look of the app's own over the headless layer: Sift, a small triage queue in a dense,
//! flat, dark-first look with one accent, its button, toggle, slider, text field, picker, list
//! and tooltip styles drawn from a palette of its own, an environment value that follows the
//! colour scheme, and a focus ring of its own. Nothing comes from nimbo's theme or its System
//! Settings look: focus, the pointing hand, the keys and what assistive technology reads are the
//! behaviours', the same under any look.
//!
//! `cargo run --release --example custom-look [-- --shot <file.png>]`

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "common/example.rs"]
mod example;

use std::time::Duration;

use nimbo::{
  Align, App, ButtonConfiguration, ButtonStyle, Color, Config, EnvironmentValue, FieldStyle,
  FocusRing, Font, HelpConfiguration, HelpStyle, Id, List, ListConfiguration, ListLayout,
  ListStyle, PickerConfiguration, PickerStyle, Point, Rect, Selection, Slider, SliderConfiguration,
  SliderStyle, Style, Tag, TextFieldConfiguration, TextFieldStyle, TextStyle, ToggleConfiguration,
  ToggleStyle, Transition, Ui, Weight, alpha, grow, mix, px, rgb,
};

/// The look's colours: a dark slate first, a paper-grey light one beside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
  /// The working area.
  pub ground: Color,
  /// The bars and the queue beside the working area.
  pub panel: Color,
  /// Fields and pressed-in places.
  pub card: Color,
  pub ink: Color,
  pub muted: Color,
  pub line: Color,
  /// The one accent: the primary action, what is on, selected or focused.
  pub accent: Color,
}

impl Palette {
  pub const DARK: Self = Self {
    ground: rgb(0x23262b),
    panel: rgb(0x1c1e22),
    card: rgb(0x191b1e),
    ink: rgb(0xd4d7dc),
    muted: rgb(0x80868f),
    line: rgb(0x34383f),
    accent: rgb(0xe2a24c),
  };
  pub const LIGHT: Self = Self {
    ground: rgb(0xf6f6f4),
    panel: rgb(0xeaeae6),
    card: rgb(0xffffff),
    ink: rgb(0x2b2e33),
    muted: rgb(0x7a7f87),
    line: rgb(0xd4d4cf),
    accent: rgb(0xb06f12),
  };
}

/// The colour scheme's palette, unless a container puts its own.
impl EnvironmentValue for Palette {
  fn default_value(ui: &Ui) -> Self {
    if ui.color_scheme().is_dark() { Self::DARK } else { Self::LIGHT }
  }
}

/// Hover and on-off fades: 120 ms.
const FADE: Transition = Transition::linear(1000.0 / 120.0);
/// A control's opacity while disabled.
const OFF: f32 = 0.4;
/// Every control's height: tight, as in a tool.
const H: f32 = 24.0;

fn text(size: f32, color: Color) -> TextStyle {
  TextStyle::new(Font::ui(size), color).line_limit(1)
}

/// How far `id`'s hover has faded in.
fn hot(ui: &mut Ui, id: Id, on: bool) -> f32 {
  ui.animate(id.key("hot"), on as u8 as f32, FADE)
}

fn opacity(enabled: bool) -> f32 {
  if enabled { 1.0 } else { OFF }
}

/// A flat block of the accent, lighter under the pointer and darker while held.
pub struct Solid;

impl ButtonStyle for Solid {
  fn make_body(&self, ui: &mut Ui, mut c: ButtonConfiguration) {
    let p = ui.environment_value::<Palette>();
    let h = hot(ui, c.id(), c.is_hovered());
    let bg =
      if c.is_pressed() { mix(p.accent, p.panel, 0.25) } else { mix(p.accent, p.ink, 0.15 * h) };
    let s = Style::new().h(px(H)).min_w(64.0).pad_xy(12.0, 0.0).center().radius(3.0).bg(bg);
    let label = text(12.0, p.panel).font(Font::ui(12.0).weight(Weight::Semibold));
    ui.row(s.opacity(opacity(c.is_enabled())), |ui| c.label(ui, label));
  }
}

/// A hairline box, tinted under the pointer: the quieter actions.
pub struct Hairline;

impl ButtonStyle for Hairline {
  fn make_body(&self, ui: &mut Ui, mut c: ButtonConfiguration) {
    let p = ui.environment_value::<Palette>();
    let k = if c.is_pressed() { 0.12 } else { 0.06 * hot(ui, c.id(), c.is_hovered()) };
    let s = Style::new().h(px(H)).pad_xy(10.0, 0.0).center().radius(3.0).border(1.0, p.line);
    let s = s.bg(alpha(p.ink, k)).opacity(opacity(c.is_enabled()));
    ui.row(s, |ui| c.label(ui, text(12.0, p.ink)));
  }
}

/// A small square box before the label, filled with the accent and ticked while on.
pub struct Tick;

impl ToggleStyle for Tick {
  fn make_body(&self, ui: &mut Ui, mut c: ToggleConfiguration) {
    let p = ui.environment_value::<Palette>();
    let k = ui.animate(c.id().key("on"), c.is_on() as u8 as f32, FADE);
    let edge = mix(p.line, p.muted, 0.6 * hot(ui, c.id(), c.is_hovered()));
    // A canvas paints past its container's opacity: each colour takes it.
    let a = opacity(c.is_enabled());
    let (edge, card) = (alpha(mix(edge, p.accent, k), a), alpha(p.card, a));
    let (fill, mark) = (alpha(p.accent, k * a), alpha(p.panel, k * a));

    ui.row(Style::new().h(px(H)).gap(8.0).center(), |ui| {
      ui.canvas(Style::new().w(px(14.0)).h(px(14.0)), move |f, r| {
        f.rect(r, edge, 3.0);
        f.rect(Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0), card, 2.0);
        f.rect(r, fill, 3.0);

        let at = |x: f32, y: f32| Point::new(r.x + x, r.y + y);
        f.line(at(3.5, 7.5), at(6.0, 10.0), 1.6, mark);
        f.line(at(6.0, 10.0), at(10.5, 4.5), 1.6, mark);
      });
      c.label(ui, text(12.0, alpha(p.ink, a)));
    });
  }
}

/// A hairline rail filled with the accent up to a narrow block, a tick under each step.
pub struct Rail;

impl SliderStyle for Rail {
  fn make_body(&self, ui: &mut Ui, c: SliderConfiguration) {
    let p = ui.environment_value::<Palette>();
    let h = hot(ui, c.id(), c.is_hovered() || c.is_held());
    let a = opacity(c.is_enabled());
    let (rail, fill, tick) = (alpha(p.line, a), alpha(p.accent, a), alpha(p.muted, 0.7 * a));
    let block = alpha(mix(p.ink, p.accent, h), a);

    let (k, ticks) = (c.fraction(), c.steps().map(|(_, x)| x).collect::<Vec<_>>());
    let s = Style::new().w(grow()).min_w(120.0).h(px(H)).pad_xy(4.0, 0.0);
    c.root(ui).canvas(s, move |f, r| {
      let (x0, w, y) = (r.x + 4.0, r.w - 8.0, r.y + r.h / 2.0 - 1.0);
      f.rect(Rect::new(x0, y, w, 2.0), rail, 1.0);
      f.rect(Rect::new(x0, y, w * k, 2.0), fill, 1.0);
      for x in &ticks {
        f.rect(Rect::new(x0 + w * x - 0.5, y + 6.0, 1.0, 4.0), tick, 0.0);
      }

      f.rect(Rect::new(x0 + w * k - 3.0, y - 6.0, 6.0, 14.0), block, 2.0);
    });
  }
}

/// A sunken box, its hairline edge taking the accent while focused, in place of a ring.
pub struct Inset;

impl TextFieldStyle for Inset {
  fn make_body(&self, ui: &mut Ui, mut c: TextFieldConfiguration) {
    let p = ui.environment_value::<Palette>();
    let edge = match c.is_focused() {
      true => p.accent,
      false => mix(p.line, p.muted, 0.5 * hot(ui, c.id(), c.is_hovered())),
    };

    let s = Style::new().w(grow()).h(px(H)).pad_xy(8.0, 0.0).radius(3.0).bg(p.card);
    let s = s.border(1.0, edge).align(Align::Start, Align::Center).focusable_quiet();
    let s = s.scroll(c.axis().into()).opacity(opacity(c.is_enabled()));
    let field = FieldStyle::new(text(12.0, p.ink)).placeholder(p.muted).caret(p.accent);
    ui.row(s, |ui| c.text(ui, &field.selection(alpha(p.accent, 0.3))));
  }
}

/// The options joined in one hairline box, the selected one underlined with the accent.
pub struct Joined;

impl PickerStyle for Joined {
  fn make_body(&self, ui: &mut Ui, mut c: PickerConfiguration) {
    let p = ui.environment_value::<Palette>();
    let b = c.segments(ui);
    let frame = Style::new().h(px(H)).radius(3.0).border(1.0, p.line).clip();
    ui.row(frame.opacity(opacity(!b.disabled)), |ui| {
      for i in 0..c.len() {
        let (id, on) = (c.item_id(i), c.selection() == Some(i));
        let h = hot(ui, id, !b.disabled && ui.hovered(id));
        let bg = if on { alpha(p.accent, 0.14) } else { alpha(p.ink, 0.05 * h) };
        let seg = Style::new().h(grow()).pad_xy(12.0, 0.0).bg(bg);
        let (ink, bar) = if on { (p.ink, p.accent) } else { (p.muted, [0.0; 4]) };

        ui.with_id(id).col(b.pointer(seg), |ui| {
          ui.el(Style::new().h(px(2.0)));
          ui.row(Style::new().w(grow()).h(grow()).center(), |ui| {
            ui.text(c.label(i), text(12.0, ink))
          });
          ui.el(Style::new().w(grow()).h(px(2.0)).bg(bar));
        });
      }
    });
  }
}

/// Tight rows straight on the panel, the selected one a bar of the accent, stronger while the
/// list has focus.
pub struct Queue;

impl ListStyle for Queue {
  fn layout(&self, _: &Ui) -> ListLayout {
    let mut l = ListLayout::default();
    l.frame = l.frame.pad(2.0);
    l
  }

  fn make_body<'a>(&self, ui: &mut Ui<'a>, mut c: ListConfiguration<'_, 'a>) {
    let p = ui.environment_value::<Palette>();
    let frame = c.frame().bg(p.panel);
    ui.scroll_area(frame.opacity(opacity(c.is_enabled())), |ui| {
      c.rows(ui, |ui, mut r| {
        let fill = match (r.is_selected(), r.is_hovered()) {
          (true, _) => alpha(p.accent, if r.is_focused() { 0.28 } else { 0.14 }),
          (false, true) => alpha(p.ink, 0.05),
          _ => [0.0; 4],
        };

        ui.row(r.frame().radius(2.0).bg(fill), |ui| r.content(ui));
      })
    });
  }
}

/// A panel-coloured slip with a hairline edge.
pub struct Slip;

impl HelpStyle for Slip {
  fn make_body(&self, ui: &mut Ui, c: HelpConfiguration) {
    let p = ui.environment_value::<Palette>();
    let s = Style::new().pad_xy(7.0, 4.0).radius(2.0).bg(p.panel).border(1.0, p.line);
    ui.row(s.max_w(240.0).opacity(c.opacity()), |ui| ui.text(c.text(), text(11.0, p.ink)));
  }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
  Bug,
  Feature,
  Chore,
}

pub const KINDS: [Tag<Kind>; 3] =
  [Tag::new(Kind::Bug, "Bug"), Tag::new(Kind::Feature, "Feature"), Tag::new(Kind::Chore, "Chore")];

/// An issue in the queue.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
  pub title: String,
  pub kind: Kind,
  /// 1, the most urgent, to 4.
  pub priority: u8,
  pub blocked: bool,
  pub review: bool,
}

/// A queue of issues and the one being written.
pub struct Sift {
  pub queue: Vec<Issue>,
  pub title: String,
  pub kind: Kind,
  pub priority: f32,
  pub blocked: bool,
  pub review: bool,
  pub sel: Selection,
}

pub const TITLE: &str = "title";
pub const KIND: &str = "kind";
pub const PRIORITY: &str = "priority";
pub const BLOCKED: &str = "blocked";
pub const REVIEW: &str = "review";
pub const FILE: &str = "file";
pub const CLEAR: &str = "clear";
pub const CLOSE: &str = "close";
pub const QUEUE: &str = "queue";

fn issue(title: &str, kind: Kind, priority: u8, blocked: bool) -> Issue {
  Issue { title: title.into(), kind, priority, blocked, review: kind != Kind::Chore }
}

impl Default for Sift {
  fn default() -> Self {
    let queue = vec![
      issue("Crash on an empty search", Kind::Bug, 1, false),
      issue("Export the log as CSV", Kind::Feature, 2, false),
      issue("Scrollbar too faint in dark", Kind::Bug, 3, true),
      issue("Bump the toolchain", Kind::Chore, 4, false),
    ];
    let (title, sel) = (String::new(), Selection::default());
    Self { queue, title, kind: Kind::Bug, priority: 2.0, blocked: false, review: true, sel }
  }
}

impl Sift {
  /// The bar over the window: the app's name and what the queue holds.
  fn bar(&self, ui: &mut Ui, p: Palette, top: bool) {
    let s = Style::new().w(grow()).h(px(if top { 34.0 } else { 22.0 })).pad_xy(12.0, 0.0);
    let s = s.gap(8.0).align(Align::Start, Align::Center).bg(p.panel);
    if !top {
      ui.el(Style::new().w(grow()).h(px(1.0)).bg(p.line));
    }

    ui.row(s, |ui| {
      let blocked = self.queue.iter().filter(|i| i.blocked).count();
      match top {
        true => {
          ui.text("Sift", text(13.0, p.ink).font(Font::ui(13.0).weight(Weight::Bold)));
          ui.text("triage", text(12.0, p.muted));
        }

        false => {
          let open = format!("{} open · {blocked} blocked", self.queue.len());
          ui.id("status").text(&open, text(11.0, p.muted));
        }
      }
    });
    if top {
      ui.el(Style::new().w(grow()).h(px(1.0)).bg(p.line));
    }
  }

  fn form(&mut self, ui: &mut Ui, p: Palette) {
    let label =
      |ui: &mut Ui, s: &str| ui.row(Style::new().w(px(64.0)), |ui| ui.text(s, text(12.0, p.muted)));
    let row = Style::new().w(grow()).gap(10.0).align(Align::Start, Align::Center);
    ui.row(row, |ui| {
      label(ui, "Title");
      ui.id(TITLE).a11y_label("Title").text_field(&mut self.title, "What is wrong or wanted");
    });

    ui.row(row, |ui| {
      label(ui, "Kind");
      ui.id(KIND).a11y_label("Kind").picker(&mut self.kind, &KINDS);
    });

    ui.row(row, |ui| {
      ui.row(Style::new().w(px(64.0)), |ui| {
        ui.help("1 is the most urgent").text("Priority", text(12.0, p.muted))
      });
      let slider = Slider::new(&mut self.priority, 1.0..=4.0).step(1.0);
      slider.show(ui.id(PRIORITY).a11y_label("Priority"));
      ui.text(format_args!("P{}", self.priority), text(12.0, p.ink).font(Font::mono(12.0)));
    });

    // A chore goes in unreviewed: no review to ask for.
    let chore = self.kind == Kind::Chore;
    ui.row(row, |ui| {
      ui.el(Style::new().w(px(64.0)));
      ui.id(BLOCKED).toggle(&mut self.blocked, "Blocked");
      ui.disabled(chore).id(REVIEW).toggle(&mut self.review, "Needs review");
    });
  }

  fn actions(&mut self, ui: &mut Ui) {
    let tip = "Adds it to the queue";
    if ui.disabled(self.title.trim().is_empty()).help(tip).id(FILE).button("File").clicked {
      let priority = self.priority as u8;
      let review = self.review && self.kind != Kind::Chore;
      let (title, kind, blocked) = (std::mem::take(&mut self.title), self.kind, self.blocked);
      self.queue.push(Issue { title, kind, priority, blocked, review });
    }

    ui.spacer();
    // The quieter actions in a style of their own, put on their row.
    ui.button_style(&Hairline).row(Style::new().gap(6.0), |ui| {
      if ui.id(CLEAR).button("Clear").clicked {
        let (queue, sel) = (std::mem::take(&mut self.queue), std::mem::take(&mut self.sel));
        *self = Sift { queue, sel, ..Sift::default() };
      }

      let at = self.sel.keys().next().map(|k| k as usize);
      if ui.disabled(at.is_none()).id(CLOSE).button("Close").clicked {
        self.queue.remove(at.unwrap_or_default());
        self.sel.clear();
      }
    });
  }

  fn queue(&mut self, ui: &mut Ui, p: Palette) {
    let (list, queue) = (List::new(self.queue.len(), 24.0), &self.queue);
    let list = list.selection(&mut self.sel);
    list.show(ui.a11y_label("Queue").id(QUEUE), |ui, r| {
      let it = &queue[r.index];
      let kind = KINDS.iter().find(|t| t.value() == it.kind).map_or("", |t| t.label());
      let s = Style::new().w(grow()).gap(8.0).pad_xy(6.0, 0.0).align(Align::Start, Align::Center);
      ui.row(s, |ui| {
        let mark = text(11.0, p.accent).font(Font::mono(11.0));
        ui.text(
          format_args!("P{}", it.priority),
          if it.priority == 1 { mark } else { mark.color(p.muted) },
        );
        ui.row(Style::new().w(grow()).clip(), |ui| ui.text(&it.title, text(12.0, p.ink)));
        let note = if it.blocked { "blocked" } else { kind };
        ui.text(note, text(11.0, p.muted));
      });
    });
  }
}

impl App for Sift {
  fn view(&mut self, ui: &mut Ui) {
    let p = ui.environment_value::<Palette>();
    ui.help_style(&Slip);
    ui.set_focus_ring(Some(FocusRing::new(alpha(p.accent, 0.6)).width(1.5).gap(1.0)));

    // The look, window-wide: on the root, so every control built in it reads it.
    let ui = ui.button_style(&Solid).toggle_style(&Tick).slider_style(&Rail);
    let ui = ui.text_field_style(&Inset).picker_style(&Joined).list_style(&Queue);
    ui.col(Style::new().grow().bg(p.ground), |ui| {
      self.bar(ui, p, true);
      ui.row(Style::new().grow(), |ui| {
        ui.col(Style::new().w(px(250.0)).h(grow()), |ui| self.queue(ui, p));
        ui.el(Style::new().w(px(1.0)).h(grow()).bg(p.line));
        ui.col(Style::new().grow().pad(14.0).gap(10.0), |ui| {
          self.form(ui, p);
          ui.spacer();
          ui.row(Style::new().w(grow()).gap(6.0), |ui| self.actions(ui));
        });
      });
      self.bar(ui, p, false);
    });
  }
}

fn main() {
  // The shot: a chore being filed (no review to ask for), an issue selected, the pointer resting
  // on File, its tip showing.
  let args: Vec<String> = std::env::args().skip(1).collect();
  example::run(Config::new("Sift", 640.0, 360.0), Sift::default(), &args, |h| {
    h.frame();
    h.click_id(Id::of(KIND).key(("item", 2usize)));
    let r = h.rect(Id::of(QUEUE).key(("row", 1u64))).unwrap();
    h.click(r.x + 20.0, r.y + r.h / 2.0);
    h.click_id(Id::of(TITLE));
    h.type_text("Pin the formatter's version");
    let r = h.rect(Id::of(FILE)).unwrap();
    h.move_to(r.x + 30.0, r.y + 10.0);
    h.wait(Duration::from_millis(400));
  });
}
