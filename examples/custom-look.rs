// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A whole look of the app's own over the headless layer: a brew journal whose button, toggle,
//! slider, text field, picker, list and tooltip styles draw from a palette of its own, an
//! environment value that follows the colour scheme. The styles go on the root container, the
//! window's tooltips on the window; a row of quiet buttons puts its own button style. Nothing
//! comes from nimbo's theme or its System Settings look: focus, the pointing hand, the keys and
//! what assistive technology reads are the behaviours', the same under any look.
//!
//! `cargo run --release --example custom-look [-- --shot <file.png>]`

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::time::Duration;

use nimbo::{
  Align, App, ButtonConfiguration, ButtonStyle, Color, Config, EnvironmentValue, FieldStyle,
  FocusRing, Font, HelpConfiguration, HelpStyle, Id, List, ListConfiguration, ListLayout,
  ListStyle, PickerConfiguration, PickerStyle, Rect, Selection, Slider, SliderConfiguration,
  SliderStyle, Style, Tag, TextFieldConfiguration, TextFieldStyle, TextStyle, ToggleConfiguration,
  ToggleStyle, Transition, Ui, Weight, alpha, grow, mix, px, rgb,
};

/// The look's colours: a light roast in a light scheme, a dark one in a dark scheme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
  pub ground: Color,
  /// Fields, the list, the selected tile.
  pub card: Color,
  pub ink: Color,
  pub muted: Color,
  pub line: Color,
  /// Fills, with the card's colour for what is on them.
  pub accent: Color,
}

impl Palette {
  pub const LIGHT: Self = Self {
    ground: rgb(0xf3ece2),
    card: rgb(0xfffaf3),
    ink: rgb(0x3b2a20),
    muted: rgb(0x8c7b6e),
    line: rgb(0xe0d3c2),
    accent: rgb(0xb5562b),
  };
  pub const DARK: Self = Self {
    ground: rgb(0x1e1814),
    card: rgb(0x2a221c),
    ink: rgb(0xf1e6d8),
    muted: rgb(0xa29384),
    line: rgb(0x46392f),
    accent: rgb(0xe08a54),
  };
}

/// The colour scheme's palette, unless a container puts its own.
impl EnvironmentValue for Palette {
  fn default_value(ui: &Ui) -> Self {
    if ui.color_scheme().is_dark() { Self::DARK } else { Self::LIGHT }
  }
}

/// Hover and on-off fades: 140 ms.
const FADE: Transition = Transition::linear(7.0);
/// A control's opacity while disabled.
const OFF: f32 = 0.4;

fn text(size: f32, color: Color) -> TextStyle {
  TextStyle::new(Font::ui(size), color).line_limit(1)
}

/// How far `id`'s hover has faded in.
fn hot(ui: &mut Ui, id: Id, on: bool) -> f32 {
  ui.animate(id.key("hot"), on as u8 as f32, FADE)
}

/// A capsule filled with the accent, darker under the pointer and while held.
pub struct Bean;

impl ButtonStyle for Bean {
  fn make_body(&self, ui: &mut Ui, mut c: ButtonConfiguration) {
    let p = ui.environment_value::<Palette>();
    let k = if c.is_pressed() { 0.3 } else { 0.15 * hot(ui, c.id(), c.is_hovered()) };
    let s = Style::new().h(px(30.0)).min_w(76.0).pad_xy(16.0, 0.0).center().radius(15.0);
    let s = s.bg(mix(p.accent, p.ink, k)).opacity(if c.is_enabled() { 1.0 } else { OFF });
    let label = TextStyle { font: Font::ui(13.0).weight(Weight::Semibold), ..text(13.0, p.card) };
    ui.row(s, |ui| c.label(ui, label));
  }
}

/// The capsule's outline alone, tinted under the pointer: the row of quieter actions.
pub struct Quiet;

impl ButtonStyle for Quiet {
  fn make_body(&self, ui: &mut Ui, mut c: ButtonConfiguration) {
    let p = ui.environment_value::<Palette>();
    let k = if c.is_pressed() { 0.12 } else { 0.06 * hot(ui, c.id(), c.is_hovered()) };
    let s = Style::new().h(px(30.0)).pad_xy(14.0, 0.0).center().radius(15.0).border(1.5, p.line);
    let s = s.bg(alpha(p.ink, k)).opacity(if c.is_enabled() { 1.0 } else { OFF });
    ui.row(s, |ui| c.label(ui, text(13.0, p.ink)));
  }
}

/// A squared track before the label, its block sliding to the end and both taking the accent's
/// colours as it turns on.
pub struct Notch;

impl ToggleStyle for Notch {
  fn make_body(&self, ui: &mut Ui, mut c: ToggleConfiguration) {
    let p = ui.environment_value::<Palette>();
    let k = ui.animate(c.id().key("on"), c.is_on() as u8 as f32, FADE);
    let off = mix(p.line, p.muted, 0.4 * hot(ui, c.id(), c.is_hovered()));
    let a = if c.is_enabled() { 1.0 } else { OFF };
    let (track, block) = (alpha(mix(off, p.accent, k), a), alpha(mix(p.muted, p.card, k), a));
    ui.row(Style::new().gap(10.0).center(), |ui| {
      ui.canvas(Style::new().w(px(36.0)).h(px(20.0)), move |f, r| {
        f.rect(r, track, 6.0);
        f.rect(Rect::new(r.x + 3.0 + (r.w - 20.0) * k, r.y + 3.0, 14.0, 14.0), block, 4.0);
      });
      c.label(ui, text(13.0, alpha(p.ink, a)));
    });
  }
}

/// A groove filled with the accent up to a bar for the knob, a dot under it at each step.
pub struct Groove;

impl SliderStyle for Groove {
  fn make_body(&self, ui: &mut Ui, c: SliderConfiguration) {
    let p = ui.environment_value::<Palette>();
    let h = hot(ui, c.id(), c.is_hovered() || c.is_held());
    let a = if c.is_enabled() { 1.0 } else { OFF };
    let (groove, fill, dot) = (alpha(p.line, a), alpha(p.accent, a), alpha(p.muted, 0.6 * a));
    let knob = alpha(mix(p.ink, p.accent, h), a);

    let (k, dots) = (c.fraction(), c.steps().map(|(_, x)| x).collect::<Vec<_>>());
    let s = Style::new().w(grow()).min_w(160.0).h(px(26.0)).pad_xy(4.0, 0.0);
    c.root(ui).canvas(s, move |f, r| {
      let (x0, w, y) = (r.x + 4.0, r.w - 8.0, r.y + r.h / 2.0 - 3.0);
      f.rect(Rect::new(x0, y, w, 6.0), groove, 3.0);
      f.rect(Rect::new(x0, y, w * k, 6.0), fill, 3.0);
      for x in &dots {
        f.rect(Rect::new(x0 + w * x - 1.5, y + 13.0, 3.0, 3.0), dot, 1.5);
      }

      f.rect(Rect::new(x0 + w * k - 4.0, y - 6.0, 8.0, 18.0), knob, 3.0);
    });
  }
}

/// A card with a hairline edge that takes the accent while focused, in place of a ring.
pub struct Inkwell;

impl TextFieldStyle for Inkwell {
  fn make_body(&self, ui: &mut Ui, mut c: TextFieldConfiguration) {
    let p = ui.environment_value::<Palette>();
    let edge = match c.is_focused() {
      true => p.accent,
      false => mix(p.line, p.muted, 0.5 * hot(ui, c.id(), c.is_hovered())),
    };

    let s = Style::new().w(grow()).h(px(30.0)).pad_xy(10.0, 0.0).radius(8.0).bg(p.card);
    let s = s.border(1.5, edge).align(Align::Start, Align::Center).focusable_quiet();
    let s = s.scroll(c.axis().into()).opacity(if c.is_enabled() { 1.0 } else { OFF });
    let field = FieldStyle::new(text(13.0, p.ink)).placeholder(p.muted).caret(p.accent);
    ui.row(s, |ui| c.text(ui, &field.selection(alpha(p.accent, 0.25))));
  }
}

/// The options as tiles in a tray, the selected one a card with its label in the accent.
pub struct Tiles;

impl PickerStyle for Tiles {
  fn make_body(&self, ui: &mut Ui, mut c: PickerConfiguration) {
    let p = ui.environment_value::<Palette>();
    let b = c.segments(ui);
    let tray = Style::new().pad(3.0).gap(2.0).radius(10.0).bg(alpha(p.line, 0.7));
    ui.row(tray.opacity(if b.disabled { OFF } else { 1.0 }), |ui| {
      for i in 0..c.len() {
        let (id, on) = (c.item_id(i), c.selection() == Some(i));
        let h = hot(ui, id, !b.disabled && ui.hovered(id));
        let bg = if on { p.card } else { alpha(p.card, 0.5 * h) };
        let tile = Style::new().h(px(26.0)).pad_xy(14.0, 0.0).center().radius(7.0).bg(bg);
        let ink = if on { p.accent } else { p.muted };
        ui.with_id(id).row(b.pointer(tile), |ui| ui.text(c.label(i), text(13.0, ink)));
      }
    });
  }
}

/// Rows on a card, the selected one tinted with the accent, stronger while the list has focus.
pub struct Ledger;

impl ListStyle for Ledger {
  fn layout(&self, _: &Ui) -> ListLayout {
    let mut l = ListLayout::default();
    l.frame = l.frame.pad(5.0);
    l
  }

  fn make_body<'a>(&self, ui: &mut Ui<'a>, mut c: ListConfiguration<'_, 'a>) {
    let p = ui.environment_value::<Palette>();
    let frame = c.frame().radius(10.0).bg(p.card).border(1.0, p.line);
    ui.scroll_area(frame.opacity(if c.is_enabled() { 1.0 } else { OFF }), |ui| {
      c.rows(ui, |ui, mut r| {
        let fill = match (r.is_selected(), r.is_hovered()) {
          (true, _) => alpha(p.accent, if r.is_focused() { 0.22 } else { 0.12 }),
          (false, true) => alpha(p.ink, 0.05),
          _ => [0.0; 4],
        };

        ui.row(r.frame().radius(6.0).bg(fill), |ui| r.content(ui));
      })
    });
  }
}

/// A slip of ink with the ground's colour for text.
pub struct Note;

impl HelpStyle for Note {
  fn make_body(&self, ui: &mut Ui, c: HelpConfiguration) {
    let p = ui.environment_value::<Palette>();
    let s = Style::new().pad_xy(8.0, 5.0).radius(4.0).bg(p.ink).max_w(240.0);
    ui.row(s.opacity(c.opacity()), |ui| {
      ui.text(c.text(), TextStyle::new(Font::ui(11.0), p.ground))
    });
  }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Method {
  PourOver,
  Press,
  ColdBrew,
}

pub const METHODS: [Tag<Method>; 3] = [
  Tag::new(Method::PourOver, "Pour-over"),
  Tag::new(Method::Press, "Press"),
  Tag::new(Method::ColdBrew, "Cold brew"),
];

/// A recipe being written, and the names of those saved.
pub struct Brew {
  pub name: String,
  pub method: Method,
  /// Grams of water for each gram of coffee.
  pub ratio: f32,
  pub bloom: bool,
  pub iced: bool,
  pub saved: Vec<String>,
  pub sel: Selection,
}

pub const NAME: &str = "name";
pub const METHOD: &str = "method";
pub const RATIO: &str = "ratio";
pub const BLOOM: &str = "bloom";
pub const ICED: &str = "iced";
pub const SAVE: &str = "save";
pub const CLEAR: &str = "clear";
pub const DELETE: &str = "delete";
pub const SAVED: &str = "saved";

impl Default for Brew {
  fn default() -> Self {
    let saved = ["Morning V60", "Sunday press", "Iced pour-over"].map(String::from).into();
    let (name, sel) = (String::new(), Selection::default());
    Self { name, method: Method::PourOver, ratio: 16.0, bloom: true, iced: false, saved, sel }
  }
}

impl Brew {
  fn form(&mut self, ui: &mut Ui, p: Palette) {
    ui.id(NAME).text_field(&mut self.name, "Recipe name");
    ui.id(METHOD).a11y_label("Method").picker(&mut self.method, &METHODS);
    ui.row(Style::new().w(grow()), |ui| {
      ui.help("Grams of water for each gram of coffee").text("Ratio", text(13.0, p.ink));
      ui.spacer();
      ui.text(format_args!("1:{}", self.ratio), text(13.0, p.muted));
    });
    let slider = Slider::new(&mut self.ratio, 10.0..=18.0).step(1.0);
    slider.show(ui.id(RATIO).a11y_label("Ratio"));
    // Cold brew steeps for hours: no bloom to set.
    let cold = self.method == Method::ColdBrew;
    ui.disabled(cold).id(BLOOM).toggle(&mut self.bloom, "Bloom first");
    ui.id(ICED).toggle(&mut self.iced, "Over ice");
  }

  fn actions(&mut self, ui: &mut Ui) {
    let tip = "Adds it to the recipes";
    if ui.disabled(self.name.is_empty()).help(tip).id(SAVE).button("Save").clicked {
      self.saved.push(std::mem::take(&mut self.name));
    }

    ui.spacer();
    // The quieter actions in a style of their own, put on their row.
    ui.button_style(&Quiet).row(Style::new().gap(8.0), |ui| {
      if ui.id(CLEAR).button("Clear").clicked {
        *self = Brew { saved: std::mem::take(&mut self.saved), ..Brew::default() };
      }

      let at = self.sel.keys().next().map(|k| k as usize);
      if ui.disabled(at.is_none()).id(DELETE).button("Delete").clicked {
        self.saved.remove(at.unwrap_or_default());
        self.sel.clear();
      }
    });
  }
}

impl App for Brew {
  fn view(&mut self, ui: &mut Ui) {
    let p = ui.environment_value::<Palette>();
    ui.help_style(&Note);
    ui.set_focus_ring(Some(FocusRing::new(alpha(p.accent, 0.5)).width(2.0).gap(2.0)));

    // The look, window-wide: on the root, so every control built in it reads it.
    let ui = ui.button_style(&Bean).toggle_style(&Notch).slider_style(&Groove);
    let ui = ui.text_field_style(&Inkwell).picker_style(&Tiles).list_style(&Ledger);
    ui.col(Style::new().grow().pad(20.0).gap(16.0).bg(p.ground), |ui| {
      ui.row(Style::new().w(grow()).align(Align::Start, Align::End), |ui| {
        ui.text("Brew", TextStyle::new(Font::ui(26.0).weight(Weight::Bold), p.ink));
        ui.spacer();
        ui.text(format_args!("{} recipes", self.saved.len()), text(12.0, p.muted));
      });

      ui.row(Style::new().grow().gap(20.0), |ui| {
        ui.col(Style::new().w(px(190.0)).h(grow()), |ui| {
          let (list, saved) = (List::new(self.saved.len(), 32.0), &self.saved);
          let list = list.selection(&mut self.sel);
          list.show(ui.id(SAVED), |ui, r| ui.text(&saved[r.index], text(13.0, p.ink)));
        });
        ui.col(Style::new().grow().gap(14.0), |ui| {
          self.form(ui, p);
          ui.spacer();
          ui.row(Style::new().w(grow()).gap(8.0), |ui| self.actions(ui));
        });
      });
    });
  }
}

fn main() {
  // The shot: a cold brew being named (no bloom to set), a recipe selected, the pointer resting
  // on Save, its tip showing.
  nimbo::run_example(Config::new("Brew", 600.0, 420.0), Brew::default(), |h| {
    h.frame();
    h.click_id(Id::of(METHOD).key(("item", 2usize)));
    let r = h.rect(Id::of(SAVED).key(("row", 1u64))).unwrap();
    h.click(r.x + 20.0, r.y + r.h / 2.0);
    h.click_id(Id::of(NAME));
    h.type_text("Overnight jar");
    let r = h.rect(Id::of(SAVE)).unwrap();
    h.move_to(r.x + 40.0, r.y + 10.0);
    h.wait(Duration::from_millis(400));
  });
}
