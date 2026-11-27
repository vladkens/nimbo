// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Text editing: the behaviours of a text field and of selectable text
//! ([Ui::text_field_behavior], [Ui::selectable_behavior]) on the editing model ([Editor]), their
//! text drawn from them in a [FieldStyle] ([Ui::field_text]), and the focused field as an input
//! method sees it ([TextInput]).
//!
//! A text field's behaviour keeps its editor (selection, undo, marked text) in the store under
//! its id and writes edits back to the app's text, reporting [FieldBehavior::changed]. A
//! single-line field reports Enter as [FieldBehavior::submitted] (and passes it on); both kinds
//! report Escape as [FieldBehavior::cancelled], passed on too, and losing focus as
//! [FieldBehavior::blurred]: an edit in place saves on Enter or blur and drops its draft on
//! Escape. Keys come as commands ([crate::keys]), typed text and input method events ([Ime]);
//! the mouse places the caret, drags a selection (scrolling at the edges), double-clicks a word,
//! triple-clicks a line, and Shift-click extends. A secondary click (the right button) focuses it
//! and asks for its edit menu ([FieldBehavior::menu], opened by [Ui::edit_menu]), whose choice
//! comes back as a command ([Ui::field_command]); the field keeps its focus look while the menu
//! is open. The focus moving away (a click elsewhere in the window, Tab) ends the selection at
//! its head; the window losing its focus keeps it, half as strong. The caret blinks on timers, so a
//! window sleeps between blinks and stops when the field loses focus.
//!
//! An input method's composition is marked text at the caret, apart from the value: the app's
//! text changes only when it is committed, and a value the app sets meanwhile waits until the
//! composition ends (dropped when the user's commit changed the text). Enter during a
//! composition only completes it. A password takes no input method, shows a dot per character,
//! keeps its text off the clipboard and moves by words as by lines.

use std::ops::Range;
use std::time::Duration;

use crate::app::{Ime, MouseButton};
use crate::keys::{Command, Delete, Move};
use crate::menu::{Item, Menu};
use crate::scene::{Color, Point, Rect, alpha};
use crate::t;
use crate::text::edit::{
  Editor, Lines, Mono, Sel, graphemes, para_range, range_from_utf16, snap, word_range,
};
use crate::ui::input::ScrollState;
use crate::ui::textlayout::{Edit, Hit};
use crate::ui::{
  Align, Cursor, EnvironmentValue, Focus, Id, NativeMenu, Role, Style, TextInput, TextLayout,
  TextStyle, Tree, Ui, UiState, grow,
};

/// The caret shows this long after it moves, then hides as long, and so on.
const BLINK: Duration = Duration::from_millis(500);
/// Frames while a drag selects outside the element, scrolling toward the pointer.
const AUTOSCROLL: Duration = Duration::from_millis(16);
/// How far past the element's edge a drag reaches per frame, sideways.
const AUTOSCROLL_X: f32 = 16.0;
/// What a password shows for each character.
const DOT: &str = "•";

/// What a text field takes, as SwiftUI's `TextField`, `TextField(axis: .vertical)` and
/// `SecureField`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldKind {
  /// One line, scrolling sideways; Enter submits.
  #[default]
  SingleLine,
  /// Lines wrapped at its width, Enter a line break; give the frame a height, it scrolls past it.
  MultiLine,
  /// One line of secret text, shown as dots: no input method, nothing copied.
  Secure,
}

/// What text in a field's behaviour does ([FieldBehavior::editing]): a field of a
/// [FieldKind] takes typing and input methods; selectable text only selects and copies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Editing {
  #[default]
  Selectable,
  Field(FieldKind),
}

impl FieldState {
  /// Takes typing and input methods.
  pub fn editable(&self) -> bool {
    matches!(self.editing, Editing::Field(_))
  }

  /// A password: no input method, dots, nothing copied.
  pub fn secure(&self) -> bool {
    self.editing == Editing::Field(FieldKind::Secure)
  }
}

/// A field's or selectable text's state in the store, under its id.
#[derive(Default)]
pub(super) struct FieldState {
  pub ed: Editor,
  /// The text element: the field's inner one, or the selectable text itself.
  pub text_id: Id,
  /// A field of a kind, or text that only selects and copies.
  pub editing: Editing,
  /// The app's text as the field last had it from the app or wrote it, and the editor's text
  /// then (line breaks made `\n`, or spaces on one line).
  app: String,
  synced: String,
  /// A value the app set while an input method composed, applied when the composition ends.
  held: Option<String>,
  /// Drag-selecting since a press: what the press selected (a word or a line for double and
  /// triple clicks), and its clicks.
  drag: Option<(Range<usize>, u32)>,
  /// When the caret last moved: it shows from then for [BLINK], then blinks.
  blink: Duration,
  /// Scroll the caret into view in the next layout: the request's number, 0 once laid out.
  reveal: u32,
  /// Requests made so far.
  reveals: u32,
  /// It has the focus (its edit menu's while open), the window's or not.
  focused: bool,
  /// The edit menu as last opened ([Ui::edit_menu]).
  pub menu: EditMenu,
  /// A command from the edit menu, run as the field is built next ([Ui::field_command]).
  queued: Option<Command>,
}

impl FieldState {
  /// Clears the text as one edit that undoes, a composition dropped first, and the app's
  /// `text` with it, at `now`: the caret shows and scrolls into view. A search field's ⊗ and
  /// Escape.
  pub(super) fn clear(&mut self, text: &mut String, now: Duration) {
    self.ed.discard_marked();
    if !self.ed.text().is_empty() {
      self.ed.replace(0..self.ed.text().len(), "");
    }

    text.clear();
    self.held = None;
    synced(self, text);
    self.blink = now;
    reveal(self);
  }
}

/// What a text field or selectable text got this frame, read before it is built; see the
/// module's docs. [Ui::field_text] draws its text from it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct FieldBehavior {
  pub id: Id,
  /// The text element: `id.key("text")` in a field, `id` itself for selectable text.
  pub text_id: Id,
  /// Disabled by [Ui::disabled]: no input was read, nothing is drawn selected.
  pub disabled: bool,
  pub hovered: bool,
  /// Focused in the key window: when to draw the ring, the caret and a bright selection.
  pub focused: bool,
  /// The text changed and was written back.
  pub changed: bool,
  /// Enter in a single-line field.
  pub submitted: bool,
  /// Escape.
  pub cancelled: bool,
  /// Lost focus since the view last built it.
  pub blurred: bool,
  /// A field of a kind, or selectable text.
  pub editing: Editing,
  /// The edit menu a secondary click asked for, or that is open.
  pub menu: EditMenu,
  /// The selection, marked text, caret and reveal to draw, in offsets of the text shown;
  /// [Ui::field_text] fills in the colours.
  pub(crate) edit: Edit,
}

/// A text field's or selectable text's edit menu as a secondary click (the right button) opened
/// it: where, and what it can do then ([Ui::edit_menu] draws it).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct EditMenu {
  /// Where the click was, in the frame it came; `None` after.
  pub at: Option<Point>,
  /// A field, which has Cut and Paste; selectable text only copies.
  pub editable: bool,
  /// Cut and Copy: a selection, not in a password.
  pub cut: bool,
  pub copy: bool,
  /// The clipboard has text.
  pub paste: bool,
  /// There is text.
  pub select_all: bool,
}

impl EditMenu {
  /// The menu: Cut, Copy, Paste (Copy alone in selectable text), a line, Select All, each
  /// enabled when it can act, running its command; no shortcut labels, as AppKit's, Win32's
  /// and GTK's text menus show none.
  pub fn items(&self) -> Menu<Command> {
    let item = |cmd: Command, label: &str, on: bool| Item::button(cmd, label).enabled(on);
    let cut = item(Command::Cut, &t!("Cut"), self.cut);
    let paste = item(Command::Paste, &t!("Paste"), self.paste);
    let copy = item(Command::Copy, &t!("Copy"), self.copy);
    let mut items = if self.editable { vec![cut, copy, paste] } else { vec![copy] };
    items.extend([Item::Divider, item(Command::SelectAll, &t!("Select All"), self.select_all)]);
    Menu::new(items)
  }
}

/// Draws edit menu `id` asked for at a point where the platform shows none (the harness), and
/// returns what was chosen in it: the styled layer's context menu, which the shell puts
/// ([UiState::set_environment_default]); none without it.
#[derive(Clone, Copy)]
pub(crate) struct DrawnEditMenu(pub(crate) DrawMenu);

pub(crate) type DrawMenu = fn(&mut Ui<'_>, Id, Option<Point>, EditMenu) -> Option<Command>;

impl EnvironmentValue for DrawnEditMenu {
  fn default_value(_: &Ui) -> Self {
    Self(|_, _, _, _| None)
  }
}

impl Ui<'_> {
  /// The edit menu of a field or selectable text over its behaviour `b` ([FieldBehavior::menu]):
  /// [EditMenu::items] as `b.id.key("edit")`'s context menu, the platform's ([Ui::native_menu])
  /// or else drawn in the shell's look; the one chosen runs in the field ([Ui::field_command]).
  pub fn edit_menu(&mut self, b: &FieldBehavior) {
    let (id, m) = (b.id.key("edit"), b.menu);
    let chosen = match self.native_menu(id, m.at, || m.items()) {
      NativeMenu::Answered(cmd) => cmd,
      NativeMenu::Unanswered => None,
      NativeMenu::Unavailable => {
        // Disabled while open, a drawn one closes.
        let off = b.disabled && self.menu_open(id);
        let at = m.at.or(off.then(Point::default));
        (self.environment_value::<DrawnEditMenu>().0)(self, id, at, m)
      }
    };

    if let Some(cmd) = chosen {
      self.field_command(b.id, cmd);
    }
  }

  /// Runs `cmd` in the text field or selectable text `id` as it is built next, as if its keys
  /// sent it (an edit menu's item); asks for that frame.
  pub fn field_command(&mut self, id: Id, cmd: Command) {
    self.state::<FieldState>(id).queued = Some(cmd);
    self.request_frame_in(Duration::ZERO);
  }

  /// A text field editing `text`, one line or many, or a password: typing, input methods,
  /// moves and selection by keys and mouse, the clipboard and undo; see the module's docs.
  /// Build its box under `id` with [FieldBehavior::style] and its text with [Ui::field_text].
  pub fn text_field_behavior(
    &mut self,
    id: Id,
    text: &mut String,
    kind: FieldKind,
  ) -> FieldBehavior {
    // A password is a secure field, its text never exposed; one of many lines a text area.
    let role = match kind {
      FieldKind::Secure => Role::SecureTextField,
      FieldKind::MultiLine => Role::TextArea,
      FieldKind::SingleLine => Role::TextField,
    };

    let password = kind == FieldKind::Secure;

    let disabled = self.begin(id, role).is_some();
    let mut st = std::mem::take(self.state::<FieldState>(id));
    (st.text_id, st.editing) = (id.key("text"), Editing::Field(kind));
    st.ed.multiline = kind == FieldKind::MultiLine;
    self.revealed(&mut st);
    // A value from the app reveals the caret too.
    let (sel, set) = (st.ed.sel(), take_value(&mut st, text));
    let (submitted, cancelled) = match disabled {
      true => (false, false),
      false => self.edit_input(id, &mut st),
    };

    // A new text from assistive technology, as one edit over a value the app set while
    // composing, which it supersedes.
    if let Some(s) = self.a11y_value(id) {
      st.ed.discard_marked();
      if let Some(h) = st.held.take() {
        st.ed.set_text(&h);
        synced(&mut st, &h);
      }

      st.ed.replace(0..st.ed.text().len(), s);
    }

    // The edit menu takes the focus while open (or a native one waits for its answer); the field
    // keeps its look, ring and all.
    let menu_open = self.menu_open(id.key("edit"));
    let focused = self.focused(id) || menu_open;
    if menu_open {
      self.st.ring_on = Some(id);
    }

    if !focused {
      st.ed.unmark();
    }

    let changed = write_back(&mut st, text);
    let blurred = st.focused && !focused;
    if blurred {
      collapse(&mut st);
    }

    if changed || set || st.ed.sel() != sel || focused && !st.focused {
      st.blink = self.now;
      reveal(&mut st);
    }

    // The text shown, a composition in it, as the ranges of the focused field count it.
    st.focused = focused;
    if !password {
      self.a11y(id, role).text(st.ed.text());
    }

    let mut b = self.field_state(id, &mut st, disabled);
    (b.changed, b.submitted, b.cancelled, b.blurred) = (changed, submitted, cancelled, blurred);
    (b.editing, b.edit) = (st.editing, shown_edit(&st));
    b.edit.caret = b.focused && st.ed.sel().is_empty() && self.blink(st.blink);
    b.edit.reveal = st.reveal;
    *self.state::<FieldState>(id) = st;
    b
  }

  /// Text the user selects with the mouse and the keyboard (Shift-arrows) and copies: a double
  /// click selects a word, a triple click a line; Cmd-A and Cmd-C come from its bindings
  /// (`SELECT_KEYS`). Build it under `id` with [Ui::field_text].
  pub fn selectable_behavior(&mut self, id: Id, text: &str) -> FieldBehavior {
    let disabled = self.begin(id, Role::StaticText).is_some();
    let mut st = std::mem::take(self.state::<FieldState>(id));
    (st.text_id, st.editing, st.ed.multiline) = (id, Editing::Selectable, true);
    if st.app != text {
      st.ed.set_text(text);
      st.ed.select(Sel::caret(0));
      synced(&mut st, text);
    }

    self.revealed(&mut st);
    let sel = st.ed.sel();
    let (submitted, cancelled) = match disabled {
      true => (false, false),
      false => self.edit_input(id, &mut st),
    };

    let focused = self.focused(id) || self.menu_open(id.key("edit"));
    if st.focused && !focused {
      collapse(&mut st);
    }

    st.focused = focused;

    // The selection's head in view, as it moves (Shift-Down in a scrolled page).
    if st.ed.sel() != sel {
      reveal(&mut st);
    }

    let mut b = self.field_state(id, &mut st, disabled);
    (b.submitted, b.cancelled, b.edit) = (submitted, cancelled, shown_edit(&st));
    b.edit.reveal = st.reveal;
    *self.state::<FieldState>(id) = st;
    b
  }

  /// A reveal asked for stays until a laid-out frame had it (a view pass that only delivers
  /// input is not laid out); a later one waits for its own.
  fn revealed(&self, st: &mut FieldState) {
    let t = &self.st.last;
    let laid = t.find(st.text_id).and_then(|i| t.edit(i)).map(|k| t.edits[k].reveal);
    if laid.is_some_and(|r| r == st.reveal) {
      st.reveal = 0;
    }
  }

  /// A text behaviour's pointer, focus (its edit menu's too) and edit menu, the menu's point
  /// taken.
  fn field_state(&self, id: Id, st: &mut FieldState, disabled: bool) -> FieldBehavior {
    let menu_open = self.menu_open(id.key("edit"));
    let focused = !disabled && (self.focused(id) || menu_open) && self.window_focused();
    let (hovered, text_id, menu) = (!disabled && self.hovered(id), st.text_id, st.menu);
    // Disabled, an edit menu's choice is dropped.
    st.menu.at = None;
    st.queued = st.queued.filter(|_| !disabled);
    FieldBehavior { id, text_id, disabled, hovered, focused, menu, ..FieldBehavior::default() }
  }

  /// Whether a caret that last moved at `since` shows now; asks for the frame that toggles it.
  fn blink(&mut self, since: Duration) -> bool {
    let t = self.now.saturating_sub(since);
    let (n, into) = (t.as_nanos() / BLINK.as_nanos(), t.as_nanos() % BLINK.as_nanos());
    self.request_frame_in(BLINK - Duration::from_nanos(into as u64));
    n.is_multiple_of(2)
  }

  /// Applies the commands, text, input method events and mouse input for `id` to its editor,
  /// hit testing on the last frame's layout of its text; returns whether Enter submitted it
  /// and whether Escape cancelled it.
  fn edit_input(&mut self, id: Id, st: &mut FieldState) -> (bool, bool) {
    let viewport = visible(&self.st.last, id);
    let lay = TextLayout::of(&self.st.last, st.text_id).filter(|l| shows(l, st));
    let lay = lay.map(|mut l| {
      let rows = viewport.map_or(0.0, |r| r.h / l.line_h) as usize;
      l.page = rows.saturating_sub(1).max(1);
      l
    });
    let mono = Mono::PLAIN;
    let lines: &dyn Lines = match &lay {
      Some(l) if !st.secure() => l,
      _ => &mono,
    };

    let multiline = st.ed.multiline;
    let (mut submitted, mut cancelled) = (false, false);
    if let Some(c) = self.command(id).or(st.queued.take()) {
      let c = if st.secure() { by_lines(c) } else { c };
      let used = match c {
        // Enter while composing completes the composition, and only that.
        Command::InsertNewline if st.ed.marked().is_some() => {
          st.ed.unmark();
          true
        }
        Command::InsertNewline if !multiline => {
          // Submits, and goes on to the app (a dialog's default button).
          submitted = true;
          false
        }
        Command::Cancel => {
          // Cancels, and goes on to the app too (a dialog's Cancel).
          cancelled = true;
          false
        }

        Command::InsertTab | Command::InsertBacktab => false,
        Command::ScrollPageUp
        | Command::ScrollPageDown
        | Command::ScrollToTop
        | Command::ScrollToBottom
          if multiline && st.editable() =>
        {
          let line_h = lay.map_or(0.0, |l| l.line_h);
          if let Some(n) = self.st.last.get(id) {
            page(self.st.store.raw::<ScrollState>(id), n.rect.h, c, line_h);
          }

          true
        }
        Command::Copy | Command::Cut if st.secure() => false,
        Command::Copy | Command::SelectAll | Command::Move(..) if !st.editable() => {
          st.ed.apply(c, lines, &mut *self.st.clipboard)
        }
        _ if !st.editable() => false,
        c => st.ed.apply(c, lines, &mut *self.st.clipboard),
      };

      if !used {
        self.pass(id);
      }
    }

    if st.editable() {
      if let Some(t) = self.typed(id) {
        st.ed.insert(t);
      }

      if let Some(ime) = self.ime(id) {
        apply_ime(st, ime);
      }
    }

    // The mouse: a press places the caret (or selects a word or line), a drag extends.
    let press = self.st.input.press.filter(|p| p.id == id && p.button == MouseButton::Left);
    let click = self.st.input.click.filter(|c| c.id == id && c.button == MouseButton::Left);
    let press = press.map(|p| (p.pos, p.clicks, p.mods.shift()));
    let click = click.map(|c| (c.pos, c.clicks, c.mods.shift()));
    // A press released before a view saw it held: the drag from it to where it went up.
    let released = self.released(id).filter(|(p, _)| p.button == MouseButton::Left);
    let click = match released {
      Some((p, at)) if st.drag.is_none() => Some((p.pos, p.clicks, p.mods.shift())).zip(Some(at)),
      _ => click.map(|c| (c, c.0)),
    };

    let outside = match lay {
      Some(lay) => drag(st, &lay, press, click, self.mouse, viewport),
      None => {
        st.drag = None;
        false
      }
    };

    if outside {
      self.request_frame_in(AUTOSCROLL);
    }

    // A secondary click focuses it and asks for its edit menu there ([Ui::context_press]).
    if let Some(pos) = self.context_press(id) {
      self.focus(Some(id));
      let (editable, copy) = (st.editable(), !st.ed.sel().is_empty() && !st.secure());
      let (paste, select_all) =
        (editable && self.st.clipboard.has_text(), !st.ed.text().is_empty());
      st.menu =
        EditMenu { at: Some(pos), editable, cut: copy && editable, copy, paste, select_all };
    }

    (submitted, cancelled)
  }
}

impl Ui<'_> {
  /// Gives the next element the text element's id: named (taking the pointer) for selectable
  /// text, not in a field, whose box takes it.
  fn text_id(&mut self, b: &FieldBehavior) -> &mut Self {
    self.next_id = Some((b.text_id, b.text_id == b.id));
    self
  }
}

impl FieldBehavior {
  /// The field's box, or selectable text's element: takes focus by a click or Tab, the I-beam
  /// over it; disabled, neither.
  pub const fn style(&self, s: Style) -> Style {
    match self.disabled {
      true => Style { focus: Focus::None, cursor: Cursor::Inherit, ..s },
      false => s.focusable().cursor(Cursor::Text),
    }
  }
}

impl UiState {
  /// The editor of the text field or selectable text `id`.
  pub fn editor(&self, id: Id) -> Option<&Editor> {
    self.store.peek::<FieldState>(id).map(|s| &s.ed)
  }
}

/// The focus moved away (a click elsewhere, Tab), not the window's: the selection ends at its
/// head, as AppKit's does when its field resigns.
fn collapse(st: &mut FieldState) {
  let head = st.ed.sel().head;
  st.ed.select(Sel::caret(head));
}

/// Asks the next layout to show the caret, as a new request.
fn reveal(st: &mut FieldState) {
  st.reveals = st.reveals.wrapping_add(1).max(1);
  st.reveal = st.reveals;
}

/// The app's `text` into the editor when the app changed it: at once (returns true), or held
/// while an input method composes.
fn take_value(st: &mut FieldState, text: &str) -> bool {
  // Back to the value the field has: a value held meanwhile is no more.
  let (new, composing) = (text != st.app, st.ed.marked().is_some());
  match (new, composing) {
    (false, _) => st.held = None,
    (true, true) => st.held = Some(text.to_owned()),
    (true, false) => {
      st.ed.set_text(text);
      st.held = None;
      synced(st, text);
    }
  }

  new && !composing
}

/// The editor and the app agree: on `app`, and the editor's text now.
fn synced(st: &mut FieldState, app: &str) {
  st.app.clear();
  st.app.push_str(app);
  st.synced.clear();
  st.synced.push_str(st.ed.text());
}

/// Writes the editor's text back to the app's `text` when it changed, not while composing;
/// returns whether it did. A value the app set while composing applies now, unless the commit
/// changed the text.
fn write_back(st: &mut FieldState, text: &mut String) -> bool {
  if st.ed.marked().is_some() {
    return false;
  }

  let held = st.held.take();
  if st.ed.text() != st.synced {
    text.clear();
    text.push_str(st.ed.text());
    synced(st, text);
    return true;
  }

  if let Some(h) = held {
    st.ed.set_text(&h);
    synced(st, &h);
  }

  false
}

/// A press (`press`: where, clicks, Shift) placing the caret or selecting a word or line, held
/// and dragged to `mouse`, or a press that came and went between views (`click`: the press and
/// where it was released); returns whether the drag is past the edges of `viewport`, where
/// frames go on scrolling it.
fn drag(
  st: &mut FieldState,
  lay: &TextLayout,
  press: Option<(Point, u32, bool)>,
  click: Option<((Point, u32, bool), Point)>,
  mouse: Option<Point>,
  viewport: Option<Rect>,
) -> bool {
  // A press the view sees first starts the drag, which goes on to the pointer at once.
  if let Some((p, clicks, shift)) = press.filter(|_| st.drag.is_none()) {
    start_drag(st, lay, p, clicks, shift);
  }

  match (press, st.drag.is_some()) {
    (Some(_), _) => {
      let (Some(m), Some(v)) = (mouse, viewport) else { return false };
      // Past the edges the selection reaches a step further each frame, which the reveal of
      // its head scrolls to.
      let p = Point::new(
        m.x.clamp(v.x - AUTOSCROLL_X, v.x + v.w + AUTOSCROLL_X),
        m.y.clamp(v.y - lay.line_h, v.y + v.h + lay.line_h),
      );
      drag_to(st, lay, p);
      return !v.contains(m);
    }
    (None, true) => {
      if let Some(p) = click.map(|c| c.1).or(mouse) {
        drag_to(st, lay, p);
      }

      st.drag = None;
    }
    (None, false) => {
      if let Some(((p, clicks, shift), at)) = click {
        start_drag(st, lay, p, clicks, shift);
        if at != p {
          drag_to(st, lay, at);
        }

        st.drag = None;
      }
    }
  }

  false
}

/// The part of element `id` of the last tree that shows, inside the clips of its ancestors (a
/// scrolled page): where a drag past the edges scrolls.
fn visible(t: &Tree, id: Id) -> Option<Rect> {
  let mut i = t.find(id)? as usize;
  let n = &t.nodes[i];
  let mut r = n.style.clip_rect(n.rect).unwrap_or(n.rect);
  while i != 0 {
    i = t.nodes[i].parent as usize;
    if let Some(c) = t.nodes[i].style.clip_rect(t.nodes[i].rect) {
      r = r.intersect(c);
    }
  }

  Some(r)
}

/// Page Up and Down, Home and End in a multi-line field of height `h`: scrolls without moving
/// the caret.
fn page(s: &mut ScrollState, h: f32, c: Command, line_h: f32) {
  let page = (h - line_h).max(1.0);
  s.offset[1] = match c {
    Command::ScrollPageUp => s.offset[1] - f64::from(page),
    Command::ScrollPageDown => s.offset[1] + f64::from(page),
    Command::ScrollToTop => 0.0,
    _ => f64::MAX,
  };
  s.pending = Point::default();
}

/// An input method's event on the editor; a password types its commits, nothing else.
fn apply_ime(st: &mut FieldState, ime: &Ime) {
  let s = st.ed.text();
  let replace = |r: &Option<Range<usize>>| r.clone().map(|r| range_from_utf16(s, r));
  match ime {
    Ime::Commit { text, .. } if st.secure() => st.ed.insert(text),
    _ if st.secure() => {}
    // An empty preedit with nothing marked (some report one after every commit) is no edit.
    Ime::Mark { text, .. } if text.is_empty() && st.ed.marked().is_none() => {}
    Ime::Mark { text, sel, replace: r } => {
      let (sel, r) = (range_from_utf16(text, sel.clone()), replace(r));
      st.ed.set_marked(text, sel, r);
    }
    Ime::Commit { text, replace: r } => {
      let r = replace(r);
      st.ed.commit(text, r);
    }
    Ime::Unmark => st.ed.unmark(),
  }
}

/// A password's command: words move and delete as lines do, as AppKit's secure fields keep a
/// password's words to themselves.
fn by_lines(c: Command) -> Command {
  match c {
    Command::Move(Move::WordLeft, s) => Command::Move(Move::LineStart, s),
    Command::Move(Move::WordRight, s) => Command::Move(Move::LineEnd, s),
    Command::Delete(Delete::WordBackward) => Command::Delete(Delete::ToLineStart),
    Command::Delete(Delete::WordForward) => Command::Delete(Delete::ToLineEnd),
    c => c,
  }
}

/// Whether the last frame's layout shows the editor's text as it is now (dots for a password).
fn shows(l: &TextLayout, st: &FieldState) -> bool {
  match st.secure() {
    true => l.text.len() == graphemes(st.ed.text()).count() * DOT.len(),
    false => l.text == st.ed.text(),
  }
}

/// Offset `i` of the editor's text in the text shown: a dot per grapheme for a password.
fn shown(st: &FieldState, i: usize) -> u32 {
  match st.secure() {
    true => (graphemes(&st.ed.text()[..i]).count() * DOT.len()) as u32,
    false => i as u32,
  }
}

/// Offset `i` of the text shown in the editor's text.
fn real(st: &FieldState, i: usize) -> usize {
  match st.secure() {
    true => graphemes(st.ed.text()).nth(i / DOT.len()).map_or(st.ed.text().len(), |g| g.start),
    false => snap(st.ed.text(), i),
  }
}

/// The selection and marked text to draw, in the text shown.
fn shown_edit(st: &FieldState) -> Edit {
  let sel = st.ed.sel();
  let mut e = Edit { sel: (shown(st, sel.anchor), shown(st, sel.head)), ..Edit::default() };
  if let Some(m) = st.ed.marked() {
    e.marked = (shown(st, m.start), shown(st, m.end));
  }

  e
}

/// A press at `p`: the caret there (Shift extends the selection to it), or the word or line
/// there for a double or triple click (all of a password); a drag goes on from it.
fn start_drag(st: &mut FieldState, lay: &TextLayout, p: Point, clicks: u32, shift: bool) {
  let i = real(st, lay.hit(p, if clicks >= 2 { Hit::Over } else { Hit::Nearest }));
  let s = st.ed.text();
  let r = match clicks {
    0 | 1 => i..i,
    _ if st.secure() => 0..s.len(),
    2 => word_range(s, i),
    _ => para_range(s, i),
  };

  let anchor = match (shift, clicks) {
    (true, 0 | 1) => {
      let a = st.ed.sel().anchor;
      st.ed.select(Sel::new(a, i));
      a..a
    }
    _ => {
      st.ed.select(Sel::new(r.start, r.end));
      r
    }
  };

  st.drag = Some((anchor, clicks));
}

/// A drag to `p`: from what the press selected to there, by words or lines after a double or
/// triple click.
fn drag_to(st: &mut FieldState, lay: &TextLayout, p: Point) {
  let Some((a, clicks)) = st.drag.clone() else { return };
  let i = real(st, lay.hit(p, if clicks >= 2 { Hit::Over } else { Hit::Nearest }));
  let s = st.ed.text();
  let sel = match clicks {
    0 | 1 => Sel::new(a.start, i),
    _ if st.secure() => Sel::new(0, s.len()),
    _ => {
      let w = if clicks == 2 { word_range(s, i) } else { para_range(s, i) };
      match w.start < a.start {
        true => Sel::new(a.end, w.start),
        false => Sel::new(a.start, w.end.max(a.end)),
      }
    }
  };

  st.ed.select(sel);
}

// MARK: Look

impl Ui<'_> {
  /// The text element of a field or selectable text, as its behaviour `b` has it, `node` its
  /// style: the text (a dot per character for a password), or `prompt` while it is empty, with
  /// the selection (half as strong in a window that isn't key and in unfocused selectable text,
  /// none in an unfocused field), the caret and the marked text's underline.
  pub fn field_text(&mut self, b: &FieldBehavior, prompt: &str, style: &FieldStyle, node: Style) {
    let st = std::mem::take(self.st.store.raw::<FieldState>(b.id));
    let mut e = b.edit;
    e.caret_color = style.caret;
    e.sel_color = match (b.focused, st.focused || !st.editable()) {
      _ if b.disabled => [0.0; 4],
      (true, _) => style.selection,
      (false, true) => alpha(style.selection, style.selection[3] * 0.5),
      (false, false) => [0.0; 4],
    };

    let s = st.ed.text();
    let (ts, empty) = (style.text, s.is_empty() && !prompt.is_empty());
    match (empty, st.secure()) {
      (true, _) => {
        e.placeholder = true;
        let ts = ts.color(style.placeholder);
        self.text_id(b).edit_text(prompt, ts, e, node);
      }
      (false, true) => {
        let dots = DOT.repeat(graphemes(s).count());
        self.text_id(b).edit_text(&dots, ts, e, node);
      }
      (false, false) => _ = self.text_id(b).edit_text(s, ts, e, node),
    }

    *self.st.store.raw::<FieldState>(b.id) = st;
  }
}

// MARK: Input methods

impl UiState {
  /// The focused text field for an input method; `None` unless an editable field that takes
  /// input methods (not a password) has focus.
  pub fn text_input(&self) -> Option<TextInput<'_>> {
    let id = self.input.focus?;
    let st = self.store.peek::<FieldState>(id).filter(|s| s.editable() && !s.secure())?;
    let lay = TextLayout::of(&self.last, st.text_id).filter(|l| l.text == st.ed.text());
    Some(TextInput { ed: &st.ed, lay })
  }
}

/// How a field's text looks ([Ui::field_text]): the text, the prompt's colour, the selection's
/// and the caret's; and the box a look draws around it (the theme's: [crate::ui::Theme::field]).
/// `const` builders, like [Style].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldStyle {
  /// The box: size, padding, background, border, radius.
  pub(crate) frame: Style,
  pub(crate) text: TextStyle,
  /// The prompt's colour, shown while the text is empty.
  pub(crate) placeholder: Color,
  pub(crate) selection: Color,
  pub(crate) caret: Color,
  /// The border's colour while focused; transparent keeps the frame's.
  pub(crate) focus_border: Color,
}

impl FieldStyle {
  /// A box growing along its parent, with `text`'s font and colour on one line (a multi-line
  /// field wraps it: [crate::ui::TextFieldConfiguration::text]).
  pub const fn new(text: TextStyle) -> Self {
    Self {
      frame: Style::new().w(grow()).pad_xy(6.0, 4.0).align(Align::Start, Align::Center),
      text: text.nowrap(),
      placeholder: alpha(text.color, 0.4),
      selection: [0.25, 0.47, 0.95, 0.45],
      caret: text.color,
      focus_border: [0.0; 4],
    }
  }

  pub const fn frame(self, frame: Style) -> Self {
    Self { frame, ..self }
  }

  pub const fn placeholder(self, placeholder: Color) -> Self {
    Self { placeholder, ..self }
  }

  pub const fn selection(self, selection: Color) -> Self {
    Self { selection, ..self }
  }

  pub const fn caret(self, caret: Color) -> Self {
    Self { caret, ..self }
  }

  pub const fn focus_border(self, focus_border: Color) -> Self {
    Self { focus_border, ..self }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::App;
  use crate::scene::Size;
  use crate::ui::{Theme, px};
  use crate::{Harness, Modifiers, NamedKey};

  const REST: Color = [0.2, 0.2, 0.2, 1.0];
  const OVER: Color = [0.0, 0.0, 1.0, 1.0];

  /// A field the app draws itself: its own box over the behaviour, the text from it.
  #[derive(Default)]
  struct Own {
    text: String,
    seen: Vec<FieldBehavior>,
  }

  impl App for Own {
    fn view(&mut self, ui: &mut Ui) {
      let b = ui.text_field_behavior(Id::of("own"), &mut self.text, FieldKind::SingleLine);
      let style = Theme::PLATES_DARK.field();
      let frame =
        b.style(Style::new().w(px(120.0)).h(px(24.0)).bg(if b.focused { OVER } else { REST }));
      ui.with_id(b.id).row(frame, |ui| ui.field_text(&b, "", &style, Style::new()));
      self.seen.push(b);
    }
  }

  #[test]
  fn a_custom_drawn_field_on_the_behaviour() {
    let mut h = Harness::new(Own::default(), Size::new(200.0, 100.0), 1.0);
    h.frame();
    h.click(10.0, 10.0);
    assert_eq!(h.cursor(), Cursor::Text);
    h.type_text("hi");
    assert_eq!(h.app().text, "hi");
    let b = *h.app().seen.last().unwrap();
    assert!(b.focused && b.changed && b.editing == Editing::Field(FieldKind::SingleLine));
    assert_eq!(h.render().pixel(100.0, 20.0), [0, 0, 255, 255]);
    assert_eq!(h.text(Id::of("own").key("text")), Some("hi"));

    // The app's own value goes in; Escape cancels and goes on.
    h.app_mut().text = "set".into();
    h.frame();
    assert_eq!(h.editor(Id::of("own")).unwrap().text(), "set");
    assert!(!h.key(NamedKey::Escape, Modifiers::empty()));
    assert!(h.app().seen.iter().any(|b| b.cancelled));
  }

  #[test]
  fn password_offsets_map_dots_to_graphemes() {
    let st = FieldState {
      ed: Editor::new("ae\u{301}👍🏽"),
      editing: Editing::Field(FieldKind::Secure),
      ..FieldState::default()
    };
    // a, é (3 bytes), 👍🏽 (8): dots of 3 bytes each.
    assert_eq!((shown(&st, 0), shown(&st, 1), shown(&st, 4), shown(&st, 12)), (0, 3, 6, 9));
    assert_eq!((real(&st, 0), real(&st, 3), real(&st, 6), real(&st, 9)), (0, 1, 4, 12));
    let plain = FieldState { ed: Editor::new("ae\u{301}"), ..FieldState::default() };
    assert_eq!((shown(&plain, 2), real(&plain, 2)), (2, 1));
  }

  #[test]
  fn an_app_value_with_crlf_is_taken_once() {
    let mut st = FieldState::default();
    st.ed.multiline = true;
    let mut text = "a\r\nb".to_owned();
    take_value(&mut st, &text);
    st.ed.select_all();
    // The next view: the same text from the app keeps the selection; typing replaces it.
    take_value(&mut st, &text);
    st.ed.insert("c");
    assert!(write_back(&mut st, &mut text));
    assert_eq!(text, "c");
  }

  #[test]
  fn a_value_set_while_composing_waits() {
    let mut st = FieldState::default();
    let mut text = "ab".to_owned();
    take_value(&mut st, &text);
    st.ed.set_marked("x", 1..1, None);

    // The app sets a value: held, the composition stays, nothing written back.
    text = "app".into();
    take_value(&mut st, &text);
    assert!(!write_back(&mut st, &mut text));
    assert_eq!((st.ed.text(), text.as_str()), ("abx", "app"));

    // Cancelled: the app's value applies, not as a change.
    st.ed.set_marked("", 0..0, None);
    assert!(!write_back(&mut st, &mut text));
    assert_eq!((st.ed.text(), st.held.as_deref()), ("app", None));

    // Set, then set back: nothing waits.
    st.ed.set_marked("z", 1..1, None);
    text = "other".into();
    take_value(&mut st, &text);
    text = "app".into();
    take_value(&mut st, &text);
    st.ed.set_marked("", 0..0, None);
    assert!(!write_back(&mut st, &mut text));
    assert_eq!((st.ed.text(), text.as_str()), ("app", "app"));

    // Held again, then committed: the commit wins.
    st.ed.set_marked("y", 1..1, None);
    text = "other".into();
    take_value(&mut st, &text);
    st.ed.commit("Y", None);
    assert!(write_back(&mut st, &mut text));
    assert_eq!(text, "appY");
  }
}
