// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The text blocks, built live: text styles, localized lines, truncation, underlines, selectable
//! text, a link, icons, a label's content and a photo.

use nimbo::{
  Content, ContentMode, Icon, Id, LinePattern, Style, TextStyle, TruncationMode, Ui, px, t, tn,
};

use super::{Gallery, Shown};

impl Gallery {
  /// The live text block `name`.
  pub(super) fn text(&mut self, ui: &mut Ui, name: &str) {
    let t = ui.theme();
    match name {
      "Text" => Self::variants(ui, Shown::Alone, |ui| {
        ui.text("Title", TextStyle::new(t.bold, t.text));
        ui.text("Body", t.label());
        ui.text("Secondary", t.muted());
      }),
      "Localized text" => Self::variants(ui, Shown::Alone, |ui| {
        let name = "parakeet";
        ui.col(Style::new().gap(4.0), |ui| {
          ui.text(t!("Keep the history"), t.label());
          ui.text(t!("{name} is not on disk", name), t.label());
          ui.text(tn!("{n} file", "{n} files", 3), t.muted());
        });
      }),

      "Line limit" => Self::variants(ui, Shown::Alone, |ui| {
        let path = "/Users/alex/Library/Application Support/Example/cache.db";
        ui.col(Style::new().w(px(300.0)).gap(4.0), |ui| {
          for mode in [TruncationMode::Head, TruncationMode::Middle, TruncationMode::Tail] {
            ui.text(path, t.body().truncation(mode));
          }
        });
      }),
      "Underline" => Self::variants(ui, Shown::Alone, |ui| {
        let patterns = [
          (LinePattern::Solid, "solid"),
          (LinePattern::Dot, "dot"),
          (LinePattern::Dash, "dash"),
          (LinePattern::DashDot, "dashDot"),
          (LinePattern::DashDotDot, "dashDotDot"),
        ];
        for (p, n) in patterns {
          ui.text(n, t.label().underline(p));
        }

        ui.text("struck", t.label().strikethrough(LinePattern::Solid));
      }),
      "Selectable text" => Self::variants(ui, Shown::Plain, |ui| {
        ui.col(Style::new().w(px(300.0)), |ui| _ = ui.selectable("Select this text and copy it."));
      }),
      "Link" => Self::variants(ui, Shown::Plain, |ui| _ = ui.link("Learn more…")),
      "Image" => Self::variants(ui, Shown::Alone, |ui| {
        for icon in [Icon::GEAR, Icon::FOLDER, Icon::TRASH, Icon::INFO] {
          ui.icon(icon.stroke(1.6), 20.0, t.text);
        }
      }),

      // What a label shows, a text and an icon, built in a text style as a look would.
      "Label content" => Self::variants(ui, Shown::Alone, |ui| {
        let id = Id::of("label content");
        let labels: [Content; 3] = [Icon::FOLDER.into(), "Data folder".into(), Icon::INFO.into()];
        for (k, label) in labels.into_iter().enumerate() {
          label.build(ui.with_id(id.key(k)), t.label());
        }
      }),

      "Photo" => Self::variants(ui, Shown::Alone, |ui| {
        // Stretched to its box, then in its aspect: filling a square, fitting it.
        if let Some(photo) = &self.photo {
          ui.image(photo, Style::new().w(px(120.0)).h(px(80.0)));
          let square = Style::new().w(px(80.0)).h(px(80.0)).bg(t.panel);
          ui.aspect_ratio(ContentMode::Fill).image(photo, square);
          ui.aspect_ratio(ContentMode::Fit).image(photo, square);
        }
      }),
      _ => unreachable!("{name} is not a text block"),
    }
  }
}
