// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What a view reads of the input through its [Ui]: the pointer on an element (hover, press,
//! clicks) and the focus, as of the last frame and the input since, and the keys, text and
//! commands delivered to the focused element, each seen once.

use super::{Click, Cursor, FocusDirection, FocusRing, Id, Press, Ui};
use crate::app::{Clipboard, Event};
use crate::keys::{Command, KeyEvent};

impl Ui<'_> {
  /// Whether the pointer is over `id` or an element inside it, as of the last frame.
  pub fn hovered(&self, id: Id) -> bool {
    self.st.hovered(id)
  }

  /// Whether a button is held on `id` with the pointer over it.
  pub fn pressed(&self, id: Id) -> bool {
    self.st.pressed(id)
  }

  /// Whether a button went down on `id` and is held, wherever the pointer is (drags).
  pub fn held(&self, id: Id) -> bool {
    self.st.held(id)
  }

  /// The button held down, where and on what it went down.
  pub fn press(&self) -> Option<Press> {
    self.st.press()
  }

  /// A click on `id` (any button), seen once.
  pub fn click(&self, id: Id) -> Option<Click> {
    self.st.click(id)
  }

  /// Whether `id` was clicked with the left button; seen once.
  pub fn clicked(&self, id: Id) -> bool {
    self.st.clicked(id)
  }

  /// Shows `id` pressed for a moment, as a click would: a key on a keypad typed on the keyboard,
  /// a button activated by Space.
  pub fn flash(&mut self, id: Id) {
    self.st.flash(id, self.now);
  }

  pub fn focused(&self, id: Id) -> bool {
    self.st.focused(id)
  }

  /// The focused element.
  pub fn focus_id(&self) -> Option<Id> {
    self.st.focus()
  }

  /// Moves focus to `id`, or clears it. Focus clears by itself when its element is gone.
  pub fn focus(&mut self, id: Option<Id>) {
    self.st.set_focus(id);
  }

  /// Moves focus as Tab (forward) or Shift-Tab do.
  pub fn focus_next(&mut self, dir: FocusDirection) -> bool {
    self.st.focus_next(dir)
  }

  /// Whether the window is the key window, which gets the keys.
  pub fn window_focused(&self) -> bool {
    self.st.window_focused()
  }

  /// The pointer's cursor, as of the last frame and the input since.
  pub fn cursor(&self) -> Cursor {
    self.st.cursor()
  }

  /// Whether `id` is focused in the key window: when to draw a focus ring or a caret.
  pub fn focus_visible(&self, id: Id) -> bool {
    self.focused(id) && self.window_focused()
  }

  /// Rings the focused box ([super::Style::focusable]) while the window is key; `None` leaves
  /// focus for the widgets to show. Kept until changed.
  pub fn set_focus_ring(&mut self, ring: Option<FocusRing>) {
    self.st.ring = ring;
  }

  // MARK: Keys

  /// A command for `id`, seen once: from its bindings ([Ui::keymap]), or a key it did not use
  /// turned into one by text input while it has focus. Reading it uses it; [Ui::pass] gives it
  /// back.
  pub fn command(&self, id: Id) -> Option<Command> {
    self.st.take(id, |e| if let &Event::Command(c) = e { Some(c) } else { None })
  }

  /// Text typed while `id` has focus, seen once. Reading it uses it.
  pub fn typed(&self, id: Id) -> Option<&str> {
    self.st.take(id, |e| if let Event::Text(s) = e { Some(s.as_str()) } else { None })
  }

  /// A key pressed while `id` has focus, before bindings of the app and text input, seen once.
  /// Reading it uses it: what it would type or do is then not typed or done.
  pub fn key_down(&self, id: Id) -> Option<KeyEvent> {
    self.st.take(id, |e| if let &Event::KeyDown(k) = e { Some(k) } else { None })
  }

  /// [Ui::key_down] unless something built before (a tree over its list) used it already.
  pub fn key_unused(&self, id: Id) -> Option<KeyEvent> {
    let used = self.st.input.deliver.as_ref().is_some_and(|d| d.target == id && d.used.get());
    if used { None } else { self.key_down(id) }
  }

  /// A modifier key going down or up while `id` has focus ([Event::Modifier]): a shortcut
  /// recorder taking one alone. Seen once; reading it uses it.
  pub fn modifier_key(&self, id: Id) -> Option<(crate::Code, bool)> {
    self.st.take(id, |e| match *e {
      Event::Modifier { key, down } => Some((key, down)),
      _ => None,
    })
  }

  /// Gives back the key, text or command `id` read: it goes on as if unused (a single-line
  /// field passing Enter on to the app).
  pub fn pass(&self, id: Id) {
    if let Some(d) = self.st.input.deliver.as_ref().filter(|d| d.target == id) {
      d.used.set(false);
    }
  }

  /// The clipboard: the system's in a window, an in-memory one in the harness.
  pub fn clipboard(&mut self) -> &mut dyn Clipboard {
    &mut *self.st.clipboard
  }
}
