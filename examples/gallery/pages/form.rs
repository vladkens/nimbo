// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The form blocks, built live in the form's look on its ground: a section, rows (a composed label,
//! a tile, a footer), a disclosure, a pane's footer and the toolbar's tabs.

use nimbo::{
  ControlSize, FormRow, FormStyle, Icon, LinePattern, Section, Slider, Style, Tag, TextStyle, Tone,
  Ui, grow,
};

use super::{Gallery, Shown};

impl Gallery {
  /// A form block in the form's look, on its ground.
  pub(super) fn form(&mut self, ui: &mut Ui, name: &str) {
    let fs = FormStyle::plates(&ui.theme());
    let (on, level, tab) = (&mut self.on, &mut self.level, &mut self.tab);
    Self::variants(ui, Shown::Plain, |ui| {
      let row = |ui: &mut Ui, on: &mut bool, level: &mut f32| match name {
        "Form row" => {
          let row = FormRow::new("Open at login").caption("Starts with your Mac, in the menu bar.");
          ui.form_row(&fs, row, |ui| ui.toggle(on, ()))
        }

        "Composed label" => {
          let label = |ui: &mut Ui, s: TextStyle| {
            ui.text("gigaam-v3", s.line_limit(1));
            let ui = ui.help("Best in Russian").control_size(ControlSize::Small);
            ui.badge("Recommended", Tone::Accent);
          };
          let caption = |ui: &mut Ui, s: TextStyle| {
            ui.row(Style::new().w(grow()), |ui| {
              ui.help("Errors per 100 words").text("WER 4.7%", s.underline(LinePattern::Dot));
              ui.text(" · 274 MB", s);
            });
          };
          ui.form_row(&fs, FormRow::new(label).caption(caption), |ui| ui.button("Use"))
        }
        "Row tile" => {
          let row = FormRow::new("Data folder").caption("~/Library/Application Support");
          ui.form_row(&fs, row.tile(Icon::FOLDER), |ui| ui.button("Show"))
        }

        "Row footer" => {
          let (name, off) = (format!("Turn off below {level:.0}%"), !*on);
          let footer = |ui: &mut Ui, _: TextStyle| {
            let label = |v: f32| [10.0, 50.0, 90.0].contains(&v).then(|| format!("{v}%"));
            let s = Slider::new(level, 10.0..=90.0).step(10.0).tick_label(label);
            s.show(ui.disabled(off).a11y_label("Level"));
          };
          let row = FormRow::new(&name).caption("Only on battery.").footer(footer);
          ui.form_row(&fs, row, |ui| ui.toggle(on, ()))
        }
        "Disclosure" => {
          let row = FormRow::new("Other models").caption("2 more");
          ui.form_disclosure(&fs, row, |ui| ui.form_row(&fs, FormRow::new("whisper-tiny"), |_| {}))
        }
        _ => ui.form_row(&fs, FormRow::new("Row"), |ui| ui.toggle(on, ())),
      };

      let page = Style::new().w(grow()).gap(10.0).pad(10.0).radius(12.0).bg(fs.bg);
      ui.col(page, |ui| match name {
        "Section" => {
          let head = |ui: &mut Ui, s: TextStyle| {
            ui.text("Storage", s);
            ui.spacer();
            ui.text("640 MB used", fs.note);
          };
          let section = Section::new(head).footer("Kept until you clear it.");
          _ = ui.form_section(&fs, section, |ui| row(ui, on, level));
        }

        "Toolbar tabs" => {
          let tabs =
            [Tag::new(0, "General").icon(Icon::GEAR), Tag::new(1, "About").icon(Icon::INFO)];
          _ = ui.toolbar_tabs(&fs, tab, &tabs);
        }
        "Footer" => {
          ui.form_section(&fs, "Section", |ui| row(ui, on, level));
          ui.help("Help on these settings").form_footer(&fs, |ui| _ = ui.button("Reset All…"));
        }
        _ => _ = ui.form_section(&fs, "", |ui| row(ui, on, level)),
      });
    });
  }
}
