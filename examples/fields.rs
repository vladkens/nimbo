// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Text fields and selectable text: a name, an e-mail with a placeholder, a password, notes over
//! several lines, a disabled field and a paragraph to select and copy. Enter in a single-line
//! field shows what it submitted; Tab moves between the fields. Input methods compose in place
//! (try Pinyin or Japanese).
//!
//! `cargo run --release --example fields` opens it in a window; `-- --shot fields.png` renders
//! it without one: the name typed, the e-mail empty, a password, a word of the paragraph
//! selected and an input method composing in the notes.

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

use nimbo::{App, Config, Id, Point, Size, Style, Ui, px};

pub const SIZE: Size = Size::new(460.0, 340.0);

pub const NAME: &str = "name";
pub const EMAIL: &str = "email";
pub const PASSWORD: &str = "password";
pub const NOTES: &str = "notes";
pub const OFF: &str = "off";
pub const ABOUT: &str = "about";

/// The paragraph to select.
pub const TEXT: &str = "Selectable text takes the pointer and the keys as a field does, but \
  only selects and copies: double-click a word, triple-click a line, Shift and the arrows.";

#[derive(Default)]
pub struct Fields {
  pub name: String,
  pub email: String,
  pub password: String,
  pub notes: String,
  pub off: String,
  /// What Enter submitted last, and from which field.
  pub submitted: Option<(&'static str, String)>,
  /// Fields whose text changed, in order.
  pub changes: Vec<&'static str>,
}

impl Fields {
  pub fn new() -> Self {
    let off = "Read only".to_owned();
    Self { off, ..Self::default() }
  }
}

impl App for Fields {
  fn view(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    let row =
      Style::new().w(nimbo::grow()).gap(12.0).align(nimbo::Align::Start, nimbo::Align::Center);
    let label = Style::new().w(px(72.0));
    ui.col(Style::new().grow().pad(20.0).gap(12.0).bg(t.bg), |ui| {
      let single: [(&'static str, &str, &'static str); 3] =
        [(NAME, "Name", ""), (EMAIL, "E-mail", "name@example.com"), (PASSWORD, "Password", "")];
      for (id, title, hint) in single {
        ui.row(row, |ui| {
          ui.row(label, |ui| ui.label(title));
          let text = match id {
            NAME => &mut self.name,
            EMAIL => &mut self.email,
            _ => &mut self.password,
          };

          // The first field takes the focus as the window opens.
          let ui = if id == NAME { ui.id(id).autofocus() } else { ui.id(id) };
          let r = match id {
            PASSWORD => ui.password_field(text, "Required"),
            _ => ui.field(text, hint),
          };

          if r.changed {
            self.changes.push(id);
          }

          // A password is not shown back.
          if r.submitted {
            let shown = if id == PASSWORD { "•••".to_owned() } else { text.clone() };
            self.submitted = Some((id, shown));
          }
        });
      }

      ui.row(row.align(nimbo::Align::Start, nimbo::Align::Start), |ui| {
        ui.row(label.pads([4.0, 0.0, 0.0, 0.0]), |ui| ui.label("Notes"));
        let style = t.field().multiline().hint("A few lines");
        let style =
          style.frame(style.frame.h(px(84.0)).align(nimbo::Align::Start, nimbo::Align::Start));
        if ui.text_field(Id::of(NOTES), &mut self.notes, &style).changed {
          self.changes.push(NOTES);
        }
      });

      ui.row(row, |ui| {
        ui.row(label, |ui| ui.label("Disabled"));
        ui.disabled(true).id(OFF).field(&mut self.off, "");
      });

      ui.id(ABOUT).selectable(TEXT);
      if let Some((id, text)) = &self.submitted {
        ui.text(format_args!("Submitted {id}: {text}"), t.label().color(t.muted));
      }
    });
  }
}

#[allow(dead_code)]
fn main() {
  nimbo::run(Config::new("fields", SIZE.w, SIZE.h), Fields::new(), |h| {
    h.frame();
    h.click_id(Id::of(NAME));
    h.type_text("Ada Lovelace");
    h.click_id(Id::of(PASSWORD));
    h.type_text("secret");
    // A word of the paragraph, double-clicked: it stays selected, dimmed, as the notes take
    // the focus and an input method composes there.
    let r = h.rect(Id::of(ABOUT)).unwrap();
    let p = Point::new(r.x + 30.0, r.y + 8.0);
    h.press_with(p.x, p.y, 2, nimbo::Modifiers::empty());
    h.release(p.x, p.y);
    h.click_id(Id::of(NOTES));
    h.type_text("Notes over lines: ");
    h.ime_mark("にほん");
    h.frame();
  });
}
