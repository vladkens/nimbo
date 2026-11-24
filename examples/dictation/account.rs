// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Account tab of the settings, its text fields on the form's plates: a name, an e-mail with
//! a placeholder, the organisation (disabled), the service's API key (secure) and a custom
//! vocabulary over several lines, with a note under them to select and copy; who is signed in.
//! Enter in a single-line field submits it; Tab moves between the fields. Input methods compose in
//! place (try Pinyin or Japanese).
//!
//! `cargo run --release --example dictation -- account` opens it alone in a window;
//! `-- account --shot account.png` renders it without one: the name typed, the e-mail empty, a
//! key, a word of the note selected and an input method composing in the vocabulary.

use nimbo::{
  Axis, FormRow, FormStyle, Harness, Icon, Id, Modifiers, Point, Section, Size, Style, TextField,
  TextStyle, Ui, grow, px,
};

/// The tab alone: as wide as the settings window, as high as its page.
pub const SIZE: Size = Size::new(680.0, 400.0);

pub const NAME: &str = "name";
pub const EMAIL: &str = "email";
pub const PASSWORD: &str = "password";
pub const NOTES: &str = "notes";
pub const OFF: &str = "off";
pub const ABOUT: &str = "about";

/// The tab's icon: a head and shoulders.
pub const PERSON: Icon =
  Icon::new("M8.25 8a3.75 3.75 0 1 0 7.5 0a3.75 3.75 0 1 0-7.5 0M4.5 20a7.5 7 0 0 1 15 0");

/// The note to select.
pub const TEXT: &str = "Nothing here leaves the app's memory: the demo saves nothing. This note \
  selects and copies as a field does: double-click a word, triple-click a line, Shift and the \
  arrows.";

/// The fields' width at the rows' right.
const FIELD_W: f32 = 240.0;

#[derive(Default)]
pub struct Account {
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

impl Account {
  pub fn new() -> Self {
    let off = "Example Inc.".to_owned();
    Self { off, ..Self::default() }
  }

  /// A single-line field's row: `title` beside the field, its changes and submits noted.
  fn field(&mut self, ui: &mut Ui, fs: &FormStyle, id: &'static str, title: &str, hint: &str) {
    let text = match id {
      NAME => &mut self.name,
      EMAIL => &mut self.email,
      _ => &mut self.password,
    };

    let (changes, submitted) = (&mut self.changes, &mut self.submitted);
    ui.form_row(fs, FormRow::new(title), |ui| {
      ui.col(Style::new().w(px(FIELD_W)), |ui| {
        // The first field takes the focus as the tab opens.
        let ui = if id == NAME { ui.id(id).autofocus() } else { ui.id(id) };
        let r = match id {
          PASSWORD => ui.secure_field(text, hint),
          _ => ui.text_field(text, hint),
        };

        if r.changed {
          changes.push(id);
        }

        // A password is not shown back.
        if r.submitted {
          let shown = if id == PASSWORD { "•••".to_owned() } else { text.clone() };
          *submitted = Some((id, shown));
        }
      });
    });
  }

  /// The tab's sections, in the settings' page.
  pub fn page(&mut self, ui: &mut Ui, fs: &FormStyle) {
    ui.form_section(fs, "Profile", |ui| {
      self.field(ui, fs, NAME, "Name", "");
      self.field(ui, fs, EMAIL, "E-mail", "name@example.com");
      ui.disabled(true).form_row(fs, FormRow::new("Organisation"), |ui| {
        ui.col(Style::new().w(px(FIELD_W)), |ui| ui.id(OFF).text_field(&mut self.off, ""));
      });
    });

    // The note in the footer's style.
    let note = |ui: &mut Ui, s: TextStyle| _ = ui.id(ABOUT).selectable_text(TEXT, s);
    ui.form_section(fs, Section::new("Transcription").footer(note), |ui| {
      self.field(ui, fs, PASSWORD, "API key", "Required");
      // A multi-line field fills the height its parent gives it; the row pads it as it does
      // the label.
      let row = FormRow::new("Custom vocabulary").caption("Names and terms it should know.");
      ui.form_row(fs, row, |ui| {
        let pads = [fs.pad_y, 0.0, fs.pad_y, 0.0];
        ui.col(Style::new().w(px(FIELD_W)).h(px(84.0 + 2.0 * fs.pad_y)).pads(pads), |ui| {
          let notes = TextField::new(&mut self.notes, "One a line").axis(Axis::Vertical);
          if notes.show(ui.id(NOTES)).changed {
            self.changes.push(NOTES);
          }
        });
      });
    });

    // Who is signed in: a tile of initials beside the button that ends it.
    ui.form_section(fs, "Signed In", |ui| {
      let row = FormRow::new("Alex Doe").caption("alex@example.com").tile("AD");
      ui.form_row(fs, row, |ui| ui.button("Sign Out…"));
    });
  }
}

impl nimbo::App for Account {
  fn view(&mut self, ui: &mut Ui) {
    let fs = FormStyle::plates(&ui.theme());
    ui.col(Style::new().grow().bg(fs.bg), |ui| {
      ui.form_scroll(&fs, Style::new().grow(), |ui| {
        ui.col(fs.page.w(grow()), |ui| self.page(ui, &fs));
      });
    });
  }
}

/// The shot: the name typed, the e-mail empty, a key, a word of the note double-clicked (it
/// stays selected, dimmed, as the vocabulary takes the focus) and an input method composing
/// there.
pub fn shot(h: &mut Harness<Account>) {
  h.frame();
  h.click_id(Id::of(NAME));
  h.type_text("Ada Lovelace");
  h.click_id(Id::of(PASSWORD));
  h.type_text("secret");

  let r = h.rect(Id::of(ABOUT)).unwrap();
  let p = Point::new(r.x + 30.0, r.y + 8.0);
  h.press_with(p.x, p.y, 2, Modifiers::empty());
  h.release(p.x, p.y);

  h.click_id(Id::of(NOTES));
  h.type_text("Lovelace\nBabbage ");
  h.ime_mark("にほん");
  h.frame();
}
