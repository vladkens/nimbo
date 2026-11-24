// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A modal dialog inside the window, a macOS 26 alert: a dimmed backdrop over all of it and a
//! compact card in the middle with an optional icon on top, a title, a message and two capsule
//! buttons of equal width, Cancel and the action, a primary one or a destructive one in red; they
//! stack full width, the action on top, when a label doesn't fit half the row. It asks before
//! something that can't be undone.
//!
//! ```ignore
//! if ui.icon_button(Icon::TRASH).clicked { self.ask = true }
//! let ask = Dialog::new("Delete the file?").message("It frees 740 MB.");
//! let ask = ask.button("Delete", ButtonRole::Destructive);
//! if ask.icon(Icon::TRASH).show(ui.id("delete"), &mut self.ask) == Some(Answer::Action) {
//!   self.delete();
//! }
//! ```
//!
//! Its keys, focus and fade are [Ui::dialog_behavior]: Return chooses the default button (the
//! action, or Cancel when the action is destructive), Escape cancels, Tab and Shift-Tab move
//! between the buttons, ringing the one focused (no ring until then), and focus stays in the
//! dialog; the backdrop takes the pointer, and a click on it does nothing, as on a macOS alert.
//! It fades in, rising 8 points, over 120 ms of the runtime's clock. [Ui::modal] is the
//! backdrop and the card alone, for a dialog of the app's own over the same behaviour.

use super::widgets::{Button, ButtonRole, Icon};
use super::{Enabled, Float, Id, Role, State, Style, TextStyle, Ui, grow, pct, px};
use crate::scene::alpha;
use crate::t;
use crate::text::Weight;
use crate::ui::UnitPoint;

/// Paint order of a dialog among overlays: over menus, under tooltips.
const MODAL_Z: i32 = 1500;
/// The button a dialog closed with ([Dialog::show]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
  /// Cancel, Escape, or Return in a destructive dialog.
  Cancel,
  /// The action: clicked, Space on it, or Return in a dialog that isn't destructive.
  Action,
}

/// A modal dialog in the theme's look ([super::Theme::dialog]); see the module's docs.
#[derive(Clone, Copy, Debug)]
pub struct Dialog<'s> {
  title: &'s str,
  message: &'s str,
  icon: Option<Icon>,
  action: &'s str,
  cancel: &'s str,
  /// The action's: [ButtonRole::Confirm] or [ButtonRole::Destructive].
  role: ButtonRole,
  width: f32,
}

impl<'s> Dialog<'s> {
  /// A dialog titled `title` with Cancel and "OK", the default, as SwiftUI's `.alert(title)`.
  pub fn new(title: &'s str) -> Self {
    let (icon, role) = (None, ButtonRole::Confirm);
    Self {
      title,
      message: "",
      icon,
      action: t!("OK").as_str(),
      cancel: t!("Cancel").as_str(),
      role,
      width: 260.0,
    }
  }

  /// Text under the title, wrapping.
  pub fn message(self, message: &'s str) -> Self {
    Self { message, ..self }
  }

  /// An icon over the title, 64 points: the accent's, or red in a destructive dialog.
  pub fn icon(self, icon: Icon) -> Self {
    Self { icon: Some(icon), ..self }
  }

  /// A button labelled `label` by its role, as SwiftUI's alert takes `Button(label, role:)`:
  /// [ButtonRole::Cancel] the button that keeps things, Cancel by default; any other the action,
  /// "OK" by default: [ButtonRole::Confirm] in the accent, the default button Return chooses;
  /// [ButtonRole::Destructive] in red ("Delete"), Cancel the default then, so Return keeps what
  /// is there.
  pub fn button(self, label: &'s str, role: ButtonRole) -> Self {
    match role {
      ButtonRole::Cancel => Self { cancel: label, ..self },
      role => Self { action: label, role, ..self },
    }
  }

  /// The card's width, 260 by default, a macOS alert's; at most the window's less 32.
  pub fn width(self, w: f32) -> Self {
    Self { width: w, ..self }
  }

  /// Shows it while `open`, as SwiftUI's `isPresented`: the answer once a button closed it
  /// (`open` is then false). Its buttons are `id.key("cancel")` and `id.key("action")`.
  pub fn show(self, ui: &mut Ui, open: &mut bool) -> Option<Answer> {
    // The values put for it reach its card, and end with it when it is closed.
    ui.group(|ui| self.build(ui, open))
  }

  fn build(self, ui: &mut Ui, open: &mut bool) -> Option<Answer> {
    let t = ui.theme();
    let id = ui.take_id();
    let (cancel, action) = (id.key("cancel"), id.key("action"));
    let destructive = self.role == ButtonRole::Destructive;
    let default = if destructive { 0 } else { 1 };
    let b = ui.dialog_behavior(id, open, &[cancel, action], default, 0);
    if let Some(i) = b.chosen {
      return Some(if i == 1 { Answer::Action } else { Answer::Cancel });
    }

    if !*open {
      return None;
    }

    let w = self.width.min(ui.size().w - 32.0).max(160.0);
    let tone = if destructive { t.danger } else { t.accent };
    let card = t.dialog().w(px(w));
    let (cs, act) = (t.dialog_button(ButtonRole::Cancel), t.dialog_button(self.role));
    // Side by side while both labels fit half the row with 16 at each side, as macOS's alerts;
    // else stacked.
    let half = (w - card.pad[1] - card.pad[3] - 8.0) / 2.0;
    let fits = |ui: &mut Ui, label: &str| ui.measure(label, cs.text) + 2.0 * 16.0 <= half;
    let stack = !(fits(ui, self.cancel) && fits(ui, self.action));
    ui.a11y_label(self.title).modal(id, b.shown, card, |ui| {
      // The icon at the card's padding: the margin in its view box brings its ink in line
      // with the text, 6 further in.
      if let Some(icon) = self.icon {
        ui.col(Style::new().pads([0.0, 0.0, 8.0, 0.0]), |ui| {
          ui.icon(icon.default_stroke(1.3), 64.0, tone)
        });
      }

      ui.col(Style::new().w(grow()).pads([0.0, 6.0, 16.0, 6.0]), |ui| {
        // Bold, as NSAlert's title: heavier than the theme's semibold.
        ui.text(self.title, TextStyle::new(t.bold.weight(Weight::Bold), t.text).line_h(16.0));
        if !self.message.is_empty() {
          ui.el(Style::new().h(px(10.0)));
          ui.text(self.message, t.body().line_h(16.0));
        }
      });

      let cancel_b = Button::new(self.cancel).look(cs.w(grow()));
      let action_b = Button::new(self.action).look(act.w(grow()));
      let s = Style::new().w(grow());
      match stack {
        false => {
          _ = ui.row(s.gap(8.0), |ui| {
            (cancel_b.show(ui.with_id(cancel)), action_b.show(ui.with_id(action)))
          })
        }
        true => {
          _ = ui.col(s.gap(6.0), |ui| {
            (action_b.show(ui.with_id(action)), cancel_b.show(ui.with_id(cancel)))
          })
        }
      }
    });
    None
  }
}

impl Ui<'_> {
  /// The backdrop of a modal dialog `id` over the whole window (`id.key("backdrop")`: it takes
  /// the pointer, so nothing under it is hovered, clicked or scrolled) and the card in the
  /// middle in `card`, built by `f`; `shown` (0 to 1, [Ui::dialog_behavior]'s) fades both in, the
  /// card rising into place. The card is a modal dialog to assistive technology. Built anywhere in
  /// the view, it is placed on the window and enables its widgets though a disabled scope is
  /// around it.
  pub fn modal<R>(&mut self, id: Id, shown: f32, card: Style, f: impl FnOnce(&mut Self) -> R) {
    let scrim = self.theme().scrim();
    let cover = Style::new().w(pct(100.0)).h(pct(100.0)).tab_focusable_quiet();
    let cover = cover.bg(alpha(scrim, scrim[3] * shown));
    self.a11y(id.key("card"), Role::Dialog).state(State::Modal, true);
    let at = Float::window()
      .attach(UnitPoint::CENTER)
      .anchor(UnitPoint::CENTER)
      .offset(0.0, (1.0 - shown) * 8.0);
    let card = card.opacity(shown);
    // The values put for it reach the card past the backdrop.
    self.group(|ui| {
      ui.with_id(id.key("backdrop")).float(Float::window().z(MODAL_Z)).el(cover);
      ui.with_id(id.key("card")).float(at.z(MODAL_Z + 1)).environment(Enabled(true)).col(card, f);
    });
  }
}

#[cfg(test)]
mod tests {
  use keyboard_types::{Modifiers, NamedKey};

  use super::*;
  use crate::app::{App, Cx, Event};
  use crate::harness::Harness;
  use crate::keys::{Binding, CMD, Command, Shortcut, bind};
  use crate::scene::Size;

  /// A dialog over an app that binds Cmd-Z and hears every key and command.
  #[derive(Default)]
  struct Under {
    open: bool,
    heard: usize,
  }

  impl App for Under {
    fn keymap(&self) -> &[Binding] {
      const KEYS: &[Binding] = &[bind(Shortcut::char('z').cmd(), Command::Action("undo"))];
      KEYS
    }

    fn event(&mut self, _: &mut Cx, ev: &Event) -> bool {
      let heard = matches!(ev, Event::Command(_) | Event::KeyDown(_) | Event::Text(_));
      self.heard += heard as usize;
      heard
    }

    fn view(&mut self, ui: &mut Ui) {
      Dialog::new("Sure?").show(ui.id("d"), &mut self.open);
    }
  }

  #[test]
  fn nothing_under_a_dialog_hears_its_keys() {
    let mut h = Harness::new(Under { open: true, heard: 0 }, Size::new(400.0, 300.0), 1.0);
    h.frame();
    let d = Id::of("d");

    // The app's binding, a command from elsewhere, a letter: none reaches the app.
    h.key('z', CMD);
    h.event(Event::Command(Command::Action("undo")));
    h.key('a', Modifiers::empty());
    assert_eq!(h.app().heard, 0);

    // Tab and Shift-Tab cycle the buttons, one step each, from a button too.
    let tab = |h: &mut Harness<Under>, mods| h.key(NamedKey::Tab, mods);
    tab(&mut h, Modifiers::empty());
    assert_eq!(h.focused(), Some(d.key("cancel")));
    tab(&mut h, Modifiers::empty());
    assert_eq!(h.focused(), Some(d.key("action")));
    tab(&mut h, Modifiers::SHIFT);
    assert_eq!(h.focused(), Some(d.key("cancel")));
    tab(&mut h, Modifiers::SHIFT);
    assert_eq!(h.focused(), Some(d.key("action")));
    h.key('a', Modifiers::empty());
    assert_eq!(h.app().heard, 0);

    // Closed, the app has its keys back.
    h.key(NamedKey::Escape, Modifiers::empty());
    h.frame();
    assert!(!h.app().open);
    h.key('z', CMD);
    assert_eq!(h.app().heard, 1);
  }

  /// Labelled by role: Cancel's own label, a destructive action, Return keeping things.
  struct Roles(bool, Option<Answer>);

  impl App for Roles {
    fn view(&mut self, ui: &mut Ui) {
      let d = Dialog::new("Discard?").button("Keep", ButtonRole::Cancel);
      let d = d.button("Discard", ButtonRole::Destructive);
      self.1 = d.show(ui.id("d"), &mut self.0).or(self.1);
    }
  }

  #[test]
  fn dialog_buttons_take_their_labels_by_role() {
    let mut h = Harness::new(Roles(true, None), Size::new(400.0, 300.0), 1.0);
    h.frame();
    h.frame();
    let tree = h.a11y();
    assert!(tree.contains(r#"button "Keep""#) && tree.contains(r#"button "Discard""#), "{tree}");
    h.key(NamedKey::Enter, Modifiers::empty());
    h.frame();
    assert_eq!(h.app().1, Some(Answer::Cancel), "Return keeps things");
  }

  /// The values put for a modal reach its card past the backdrop: its size, and enabled in a
  /// disabled container.
  #[test]
  fn a_modals_values_reach_its_card() {
    use std::time::Duration;

    use crate::ui::{ControlSize, UiState};
    let mut ui = Ui::new(UiState::default(), Size::new(200.0, 200.0), None, Duration::ZERO);
    let mut seen = None;
    ui.disabled(true).col(Style::new(), |ui| {
      ui.control_size(ControlSize::Small).modal(Id::of("d"), 1.0, Style::new(), |ui| {
        seen = Some((ui.environment_value::<ControlSize>(), ui.environment_value::<Enabled>()));
      });
    });
    assert_eq!(seen, Some((ControlSize::Small, Enabled(true))));
  }
}
