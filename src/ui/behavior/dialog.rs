// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A modal dialog's behaviour ([Ui::dialog_behavior]): its keys, its focus kept among its
//! buttons and its fade; [crate::Dialog] draws one, [Ui::modal] is its backdrop and card.

use keyboard_types::{Modifiers, NamedKey};

use crate::keys::Key;
use crate::ui::{Id, Transition, Ui};

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
}
