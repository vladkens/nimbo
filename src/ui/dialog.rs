// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A modal dialog inside the window, a macOS 26 alert: a dimmed backdrop over all of it and a
//! compact card in the middle with an optional icon on top, a title, a message and two capsule
//! buttons of equal width, Cancel and the action, a primary one or a destructive one in red; they
//! stack full width, the action on top, when a label doesn't fit half the row. It asks before
//! something that can't be undone.
//!
//! ```ignore
//! if ui.icon_button(Icon::TRASH).clicked { self.ask = true }
//! let ask = Dialog::new("Delete the file?").message("It frees 740 MB.").destructive("Delete");
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

use keyboard_types::{Modifiers, NamedKey};

use super::theme::Variant;
use super::widgets::{Button, Icon};
use super::{Float, Id, Role, State, Style, TextStyle, Transition, Ui, grow, pct, px};
use crate::keys::Key;
use crate::scene::alpha;
use crate::text::Weight;

/// Paint order of a dialog among overlays: over menus, under tooltips.
const MODAL_Z: i32 = 1500;
/// How fast a dialog fades in: 0 to 1 in 120 ms.
const DIALOG_IN: Transition = Transition::smooth(1000.0 / 120.0);

/// A modal dialog ([Ui::dialog_behavior]): how far it has faded in, the button chosen this
/// frame (it closed then) and the one with focus.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DialogBehavior {
  pub id: Id,
  /// 0 to 1 as it opens, on the runtime's clock.
  pub shown: f32,
  /// Clicked, Space on the focused button, Return (the default) or Escape (the cancel).
  pub chosen: Option<usize>,
  pub focus: Option<usize>,
}

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
  destructive: bool,
  width: f32,
}

impl<'s> Dialog<'s> {
  /// A dialog titled `title` with Cancel and "OK", the default.
  pub fn new(title: &'s str) -> Self {
    let (icon, destructive) = (None, false);
    Self { title, message: "", icon, action: "OK", cancel: "Cancel", destructive, width: 260.0 }
  }

  /// Text under the title, wrapping.
  pub fn message(self, message: &'s str) -> Self {
    Self { message, ..self }
  }

  /// An icon over the title, 64 points: the accent's, or red in a destructive dialog.
  pub fn icon(self, icon: Icon) -> Self {
    Self { icon: Some(icon), ..self }
  }

  /// The action, in the accent: the default button, which Return chooses.
  pub fn action(self, label: &'s str) -> Self {
    Self { action: label, destructive: false, ..self }
  }

  /// A destructive action, in red ("Delete"): Cancel is the default then, so Return keeps what
  /// is there.
  pub fn destructive(self, label: &'s str) -> Self {
    Self { action: label, destructive: true, ..self }
  }

  /// Cancel's label.
  pub fn cancel(self, label: &'s str) -> Self {
    Self { cancel: label, ..self }
  }

  /// The card's width, 260 by default, a macOS alert's; at most the window's less 32.
  pub fn width(self, w: f32) -> Self {
    Self { width: w, ..self }
  }

  /// Shows it while `open`: the answer once a button closed it (`open` is then false). Its
  /// buttons are `id.key("cancel")` and `id.key("action")`.
  pub fn show(self, ui: &mut Ui, open: &mut bool) -> Option<Answer> {
    let t = ui.theme();
    let id = ui.take_id();
    let (cancel, action) = (id.key("cancel"), id.key("action"));
    let default = if self.destructive { 0 } else { 1 };
    let b = ui.dialog_behavior(id, open, &[cancel, action], default, 0);
    if let Some(i) = b.chosen {
      return Some(if i == 1 { Answer::Action } else { Answer::Cancel });
    }

    if !*open {
      return None;
    }

    let w = self.width.min(ui.size().w - 32.0).max(160.0);
    let tone = if self.destructive { t.danger } else { t.accent };
    let card = t.dialog().w(px(w));
    let lead = if self.destructive { Variant::Danger } else { Variant::Primary };
    let (cs, act) = (t.dialog_button(Variant::Normal), t.dialog_button(lead));
    // Side by side while both labels fit half the row with 16 at each side, as macOS's alerts;
    // else stacked.
    let half = (w - card.pad[1] - card.pad[3] - 8.0) / 2.0;
    let fits = |ui: &mut Ui, label: &str| ui.measure(label, cs.text) + 2.0 * 16.0 <= half;
    let stack = !(fits(ui, self.cancel) && fits(ui, self.action));
    ui.a11y_label(self.title).modal(id, b.shown, card, |ui| {
      // The icon at the card's padding: the margin in its view box brings its ink in line
      // with the text, 6 further in.
      if let Some(icon) = self.icon {
        ui.col(Style::new().pads([0.0, 0.0, 8.0, 0.0]), |ui| ui.icon_stroke(icon, 64.0, tone, 1.3));
      }

      ui.col(Style::new().w(grow()).pads([0.0, 6.0, 16.0, 6.0]), |ui| {
        // Bold, as NSAlert's title: heavier than the theme's semibold.
        ui.text(self.title, TextStyle::new(t.bold.weight(Weight::Bold), t.text).line_h(16.0));
        if !self.message.is_empty() {
          ui.el(Style::new().h(px(10.0)));
          ui.text(self.message, t.body().line_h(16.0));
        }
      });
      let cancel_b = Button::new(self.cancel).style(cs.w(grow()));
      let action_b = Button::new(self.action).style(act.w(grow()));
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
  /// A modal dialog `id` over the window while `open`, its buttons `buttons` (the app builds
  /// them, with [super::Behavior::style] for the Tab focus and its ring): opening, it takes
  /// focus to its backdrop, which draws no ring (assistive technology sees the default button
  /// focused then), and keeps it there and among the buttons: Tab and Shift-Tab move from the
  /// `default` button, ringing the one they reach, and cycle them; nothing else gets focus.
  /// Return chooses the default, Escape the `cancel` one, Space the focused one, a click any;
  /// choosing closes it (`open` false) and gives focus back where it was. Keys go nowhere else
  /// while it is open. The backdrop ([Ui::modal]) takes the pointer and does nothing with a
  /// click, as a macOS alert. `shown` fades in from 0 as it opens.
  pub fn dialog_behavior(
    &mut self,
    id: Id,
    open: &mut bool,
    buttons: &[Id],
    default: usize,
    cancel: usize,
  ) -> DialogBehavior {
    /// Open last frame, and the focus before it opened.
    #[derive(Default)]
    struct Modal {
      open: bool,
      prev: Option<Id>,
    }

    let backdrop = id.key("backdrop");
    let inside = |f: Option<Id>| f.is_some_and(|f| f == backdrop || buttons.contains(&f));
    let mut m = std::mem::take(self.state::<Modal>(id));
    let mut b = DialogBehavior { id, ..DialogBehavior::default() };
    if !*open || buttons.is_empty() {
      // Closed by the app: focus goes back.
      if m.open && inside(self.focus_id()) {
        self.focus(m.prev);
      }

      return b;
    }

    let shown = id.key("shown");
    if !m.open {
      m = Modal { open: true, prev: self.focus_id().filter(|&f| !inside(Some(f))) };
      self.focus(Some(backdrop));
      // What scrolls under it stops: a trackpad's gesture, a wheel's momentum.
      self.st.halt_scrolling();
      // From nothing, whatever an earlier opening left.
      self.animate(shown, 0.0, Transition::linear(0.0));
    }

    if !inside(self.focus_id()) {
      self.focus(Some(backdrop));
    }

    let n = buttons.len();
    let at = self.focus_id().and_then(|f| buttons.iter().position(|&b| b == f));
    if let Some(f) = self.focus_id()
      && let Some(k) = self.key_down(f)
    {
      match k.key {
        Key::Named(NamedKey::Escape) => b.chosen = Some(cancel),
        Key::Named(NamedKey::Enter) => b.chosen = Some(default),
        Key::Named(NamedKey::Tab) => {
          let step = if k.mods.contains(Modifiers::SHIFT) { n - 1 } else { 1 };
          let next = (at.unwrap_or(default.min(n - 1)) + step) % n;
          self.focus(Some(buttons[next]));
        }
        // Alt-Space opens the window menu on Windows.
        Key::Char(' ') if k.mods.contains(Modifiers::ALT) => self.pass(f),
        Key::Char(' ') if at.is_some() && (k.mods - Modifiers::SHIFT).is_empty() => {
          self.flash(f);
          b.chosen = at;
        }
        // The rest stops here: nothing under the dialog hears it.
        _ => {}
      }
    }

    // So do commands and text for the focused element (a menu's, an input method's).
    if let Some(f) = self.focus_id() {
      _ = (self.command(f), self.typed(f));
    }

    if let Some(i) = buttons.iter().position(|&id| self.clicked(id)) {
      b.chosen = Some(i);
    }

    if let Some(i) = b.chosen {
      b.chosen = Some(i.min(n - 1));
      *open = false;
      if inside(self.focus_id()) {
        self.focus(m.prev);
      }

      *self.state::<Modal>(id) = Modal::default();
      return b;
    }

    // Keys go to the dialog before the app's bindings: nothing under it hears them.
    if let Some(f) = self.focus_id() {
      self.grab_keys(f);
    }

    // The backdrop holds the keys; a screen reader is led to the default button.
    self.a11y_focus(backdrop, buttons[default.min(n - 1)]);

    b.focus = self.focus_id().and_then(|f| buttons.iter().position(|&b| b == f));
    b.shown = self.animate(shown, 1.0, DIALOG_IN);
    *self.state::<Modal>(id) = m;
    b
  }

  /// The backdrop of a modal dialog `id` over the whole window (`id.key("backdrop")`: it takes
  /// the pointer, so nothing under it is hovered, clicked or scrolled) and the card in the
  /// middle in `card`, built by `f`; `shown` (0 to 1, [Ui::dialog_behavior]'s) fades both in, the
  /// card rising into place. The card is a modal dialog to assistive technology. Built anywhere in
  /// the view, it is placed on the window and enables its widgets though a disabled scope is
  /// around it.
  pub fn modal<R>(&mut self, id: Id, shown: f32, card: Style, f: impl FnOnce(&mut Self) -> R) {
    let scrim = self.theme().scrim();
    let off = std::mem::replace(&mut self.scope_off, false);
    let cover = Style::new().w(pct(100.0)).h(pct(100.0)).tab_focusable_quiet();
    let cover = cover.bg(alpha(scrim, scrim[3] * shown));
    self.a11y(id.key("card"), Role::Dialog).state(State::Modal, true);
    self.with_id(id.key("backdrop")).float(Float::window().z(MODAL_Z)).el(cover);
    let at = Float::window().attach(0.5, 0.5).anchor(0.5, 0.5).offset(0.0, (1.0 - shown) * 8.0);
    self.with_id(id.key("card")).float(at.z(MODAL_Z + 1)).col(card.opacity(shown), f);
    self.scope_off = off;
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::{App, Cx, Event};
  use crate::harness::Harness;
  use crate::keys::{Binding, CMD, Command, bind};
  use crate::scene::Size;

  /// A dialog over an app that binds Cmd-Z and hears every key and command.
  #[derive(Default)]
  struct Under {
    open: bool,
    heard: usize,
  }

  impl App for Under {
    fn keymap(&self) -> &[Binding] {
      const KEYS: &[Binding] = &[bind("cmd+z", Command::Action("undo"))];
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
}
