// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The presentation blocks, built live: an alert, a context menu, the menu bar, tooltips and a help
//! style of the gallery's own.

use nimbo::{
  Answer, ButtonRole, Dialog, HelpConfiguration, HelpStyle, Icon, Item, Menu, Style, TextStyle, Ui,
};

use super::{DIALOG, Gallery, Shown};

/// The menu bar the "Menu bar" block's line makes.
const MENU_BAR: &str = "File, Edit, View, Track, Window and Help in the menu bar (macOS)";

/// What the context menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
  Copy,
  Delete,
}

/// A help style of the gallery's own: the tip on the accent in small white text.
struct Note;

impl HelpStyle for Note {
  fn make_body(&self, ui: &mut Ui, c: HelpConfiguration) {
    let t = ui.theme();
    let s = Style::new().pad_xy(8.0, 4.0).radius(6.0).bg(t.accent()).opacity(c.opacity());
    ui.row(s, |ui| ui.text(c.text(), TextStyle::new(t.small(), t.on_accent())));
  }
}

impl Gallery {
  /// The live presentation block `name`.
  pub(super) fn presentation(&mut self, ui: &mut Ui, name: &str) {
    let t = ui.theme();
    match name {
      "Alert" => {
        Self::variants(ui, Shown::Plain, |ui| {
          if ui.button("Delete…").clicked {
            self.ask = true;
          }

          ui.text(format_args!("Deleted {} times", self.deleted), t.muted().nowrap());
        });
        let d = Dialog::new("Delete the file?").message("It moves to the Trash.").icon(Icon::TRASH);
        let d = d.button("Delete", ButtonRole::Destructive);
        if d.show(ui.id(DIALOG), &mut self.ask) == Some(Answer::Action) {
          self.deleted += 1;
        }
      }
      "Context menu" => Self::variants(ui, Shown::Plain, |ui| {
        let r = ui.button("Right-click me");
        let menu = || {
          let delete = Item::button(Action::Delete, "Delete").role(ButtonRole::Destructive);
          Menu::new(vec![Item::button(Action::Copy, "Copy"), Item::Divider, delete])
        };
        if ui.context_menu(r.id, menu) == Some(Action::Copy) {
          self.copied += 1;
        }
      }),

      "Menu bar" => Self::variants(ui, Shown::Alone, |ui| ui.text(MENU_BAR, t.label())),
      "Tooltip" => Self::variants(ui, Shown::Plain, |ui| _ = ui.help("Saves it").button("Save")),
      // The window's tips in the gallery's style while the page shows.
      "Custom help style" => Self::variants(ui, Shown::Alone, |ui| {
        ui.help_style(&Note);
        ui.help("Saves it").button("Save");
        ui.help("Opens it").button("Open");
      }),
      _ => unreachable!("{name} is not a presentation block"),
    }
  }
}
