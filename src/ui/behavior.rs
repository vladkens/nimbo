// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Headless controls: what a widget does (hover, press, click, focus, keys, the value it edits)
//! without how it looks, for an app that draws its own. Every styled widget ([super::widgets])
//! is one of these plus drawing from the theme; the rule for a new widget is the same: its input
//! in a `*_behavior` here, its look over the [Behavior] in the widget.
//!
//! ```ignore
//! let b = ui.button_behavior(Id::of("play"));
//! let fill = if b.pressed { DOWN } else if b.hovered { OVER } else { REST };
//! ui.with_id(b.id).canvas(b.style(Style::new().w(px(32.0)).h(px(32.0))), move |f, r| {
//!   f.quad(Quad::new(r, fill).radius(16.0));
//! });
//! if b.clicked { self.play() }
//! ```
//!
//! A behaviour reads the input for its id before the element is built, as of the last frame and
//! the input since ([super::Ui::hovered] and the like), applies it and returns a [Behavior]. The
//! app then builds the element under that id with [Behavior::style], which makes it take focus by
//! Tab while enabled, as AppKit's controls do, and show the pointing hand over it.
//! [super::Ui::disabled] before the call disables it: no input is read, no hand. Hover fades are
//! [super::Ui::hot]. What each one covers:
//!
//! - [Ui::button_behavior]: click, Space while focused (flashes it pressed);
//! - [Ui::pick_behavior] (tabs): one of `n` items by a click, the arrows step;
//! - [Ui::split_behavior]: a divider's drag within both panes' minimums;
//! - [Ui::menu_popup] and [Ui::menu_item_behavior]: a menu's open state, keys and items;
//! - [Ui::pop_up_behavior]: a pop-up button, opened by a click, Space, Enter or the arrows, letters
//!   typed choosing an option ([super::containers::TypeSelect]);
//! - [super::List] with [super::List::unstyled] rows: selection, keys and scrolling, rows drawn by
//!   the app from [super::Row];
//! - [Ui::text_field_behavior] and [Ui::selectable_behavior]: text editing on the editing model
//!   ([Editor]), drawn by [Ui::field_text] ([super::widgets]).
//!
//! The containers' behaviours live beside their looks in [super::containers].
//!
//! A text field's behaviour keeps its editor (selection, undo, marked text) in the store under
//! its id and writes edits back to the app's text, reporting [FieldBehavior::changed]. A
//! single-line field reports Enter as [FieldBehavior::submitted] (and passes it on); both kinds
//! report Escape as [FieldBehavior::cancelled], passed on too, and losing focus as
//! [FieldBehavior::blurred]: an edit in place saves on Enter or blur and drops its draft on
//! Escape. Keys come as commands ([crate::keys]), typed text and input method events ([Ime]);
//! the mouse places the caret, drags a selection (scrolling at the edges), double-clicks a word,
//! triple-clicks a line, and Shift-click extends. The caret blinks on timers, so a window sleeps
//! between blinks and stops when the field loses focus.
//!
//! An input method's composition is marked text at the caret, apart from the value: the app's
//! text changes only when it is committed, and a value the app sets meanwhile waits until the
//! composition ends (dropped when the user's commit changed the text). Enter during a
//! composition only completes it. A password takes no input method, shows a dot per character,
//! keeps its text off the clipboard and moves by words as by lines.

use std::ops::Range;
use std::time::Duration;

use keyboard_types::Modifiers;

use super::input::ScrollState;
use super::textlayout::{Edit, TextLayout};
use super::{Cursor, Focus, Id, Style, Ui, UiState};
use crate::app::{Ime, MouseButton};
use crate::keys::{Command, Delete, Key, Move};
use crate::scene::{Point, Rect};
use crate::text::edit::{
  Editor, Lines, Mono, Sel, graphemes, para_range, range_from_utf16, snap, word_range,
};

/// The caret shows this long after it moves, then hides as long, and so on.
const BLINK: Duration = Duration::from_millis(500);
/// Frames while a drag selects outside the element, scrolling toward the pointer.
const AUTOSCROLL: Duration = Duration::from_millis(16);
/// How far past the element's edge a drag reaches per frame, sideways.
const AUTOSCROLL_X: f32 = 16.0;
/// What a password shows for each character.
pub(crate) const DOT: &str = "•";

/// What a control got this frame, read before it is built; see the module's docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Behavior {
  pub id: Id,
  /// Disabled by [Ui::disabled]: no input was read and the rest is false.
  pub disabled: bool,
  pub hovered: bool,
  /// A button held on it with the pointer over it, or Space flashing it.
  pub pressed: bool,
  /// A button went down on it and is held, wherever the pointer is (drags).
  pub held: bool,
  /// Focused in the key window: when to draw a focus ring of its own.
  pub focused: bool,
  /// Clicked with the left button, or activated by Space.
  pub clicked: bool,
  /// The value it edits changed, written back already.
  pub changed: bool,
  /// Chosen or highlighted: the selected tab, a menu's highlighted item.
  pub on: bool,
}

impl Behavior {
  /// The control's box: takes focus by Tab and shows the pointing hand while enabled; disabled,
  /// neither, whatever `s` asked for, so Tab passes it by.
  pub const fn style(&self, s: Style) -> Style {
    match self.disabled {
      true => Style { focus: Focus::None, cursor: Cursor::Inherit, ..s },
      false => s.tab_focusable().cursor(Cursor::Pointer),
    }
  }

  /// A part of a control that doesn't take focus itself (a tab, a menu's item): the pointing
  /// hand while enabled.
  pub const fn pointer(&self, s: Style) -> Style {
    if self.disabled { s } else { s.cursor(Cursor::Pointer) }
  }
}

impl Ui<'_> {
  /// Whether Space was pressed on `id` (focused), which flashes it pressed; another key goes on
  /// unused, Space with Cmd, Ctrl or Alt too (Alt-Space opens the window menu on Windows).
  pub(super) fn activated(&mut self, id: Id) -> bool {
    match self.key_down(id) {
      Some(k) if k.key == Key::Char(' ') && (k.mods - Modifiers::SHIFT).is_empty() => {
        self.flash(id);
        true
      }
      Some(_) => {
        self.pass(id);
        false
      }
      None => false,
    }
  }

  /// The disabled flag and autofocus for `id`; a disabled behaviour, or `None` to read input.
  /// Disabled while focused, it gives focus up: its keys no longer route through it.
  pub(super) fn begin(&mut self, id: Id) -> Option<Behavior> {
    // Autofocus reads the disabled flag, so before it is taken.
    self.take_autofocus(id);
    let off = self.take_disabled();
    if off && self.focused(id) {
      self.focus(None);
    }

    off.then_some(Behavior { id, disabled: true, ..Behavior::default() })
  }

  /// Pointer state of `id`, without keys.
  pub(super) fn pointer_state(&self, id: Id) -> Behavior {
    Behavior {
      id,
      hovered: self.hovered(id),
      pressed: self.pressed(id),
      held: self.held(id),
      focused: self.focus_visible(id),
      clicked: self.clicked(id),
      ..Behavior::default()
    }
  }

  /// A button: hover, press, and a click or Space while focused.
  pub fn button_behavior(&mut self, id: Id) -> Behavior {
    if let Some(b) = self.begin(id) {
      return b;
    }

    let mut b = self.pointer_state(id);
    b.clicked |= self.activated(id);
    b
  }
}

// MARK: Text

/// A field's or selectable text's state in the store, under its id.
#[derive(Default)]
pub(crate) struct FieldState {
  pub ed: Editor,
  /// The text element: the field's inner one, or the selectable text itself.
  pub text_id: Id,
  /// Takes typing and input methods (a field), or only selects and copies.
  pub editable: bool,
  /// A password: no input method, dots, nothing copied.
  pub secure: bool,
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
  focused: bool,
}

/// What a text field or selectable text got this frame, read before it is built; see the
/// module's docs. [Ui::field_text] draws its text from it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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
  /// A password ([super::FieldStyle::password]).
  pub password: bool,
  /// The selection, marked text, caret and reveal to draw, in offsets of the text shown;
  /// [Ui::field_text] fills in the colours.
  pub edit: Edit,
}

impl Ui<'_> {
  /// A text field editing `text`, one line or many, or a password: typing, input methods,
  /// moves and selection by keys and mouse, the clipboard and undo; see the module's docs.
  /// Build its box under `id` with [FieldBehavior::style] and its text with [Ui::field_text].
  pub fn text_field_behavior(
    &mut self,
    id: Id,
    text: &mut String,
    multiline: bool,
    password: bool,
  ) -> FieldBehavior {
    let disabled = self.begin(id).is_some();
    let mut st = std::mem::take(self.state::<FieldState>(id));
    (st.text_id, st.editable, st.secure) = (id.key("text"), true, password);
    st.ed.multiline = multiline && !password;
    // A reveal asked for stays until a laid-out frame had it (a view pass that only delivers
    // input is not laid out); a later one waits for its own.
    let t = &self.st.last;
    let laid = t.find(st.text_id).and_then(|i| t.edit(i)).map(|k| t.edits[k].reveal);
    if laid.is_some_and(|r| r == st.reveal) {
      st.reveal = 0;
    }

    take_value(&mut st, text);
    let sel = st.ed.sel();
    let (submitted, cancelled) = match disabled {
      true => (false, false),
      false => self.edit_input(id, &mut st),
    };

    let focused = self.focused(id);
    if !focused {
      st.ed.unmark();
    }

    let changed = write_back(&mut st, text);
    let blurred = st.focused && !focused;
    if changed || st.ed.sel() != sel || focused && !st.focused {
      st.reveals = st.reveals.wrapping_add(1).max(1);
      (st.blink, st.reveal) = (self.now, st.reveals);
    }

    st.focused = focused;
    let mut b = self.field_state(id, st.text_id, disabled);
    (b.changed, b.submitted, b.cancelled, b.blurred) = (changed, submitted, cancelled, blurred);
    (b.password, b.edit) = (password, shown_edit(&st));
    b.edit.caret = b.focused && st.ed.sel().is_empty() && self.blink(st.blink);
    b.edit.reveal = st.reveal;
    *self.state::<FieldState>(id) = st;
    b
  }

  /// Text the user selects with the mouse and the keyboard (Shift-arrows) and copies: a double
  /// click selects a word, a triple click a line; Cmd-A and Cmd-C come from its bindings
  /// ([crate::keys::SELECT_KEYS]). Build it under `id` with [Ui::field_text].
  pub fn selectable_behavior(&mut self, id: Id, text: &str) -> FieldBehavior {
    let disabled = self.begin(id).is_some();
    let mut st = std::mem::take(self.state::<FieldState>(id));
    (st.text_id, st.editable, st.secure, st.ed.multiline) = (id, false, false, true);
    if st.app != text {
      st.ed.set_text(text);
      st.ed.select(Sel::caret(0));
      synced(&mut st, text);
    }

    let (submitted, cancelled) = match disabled {
      true => (false, false),
      false => self.edit_input(id, &mut st),
    };

    let mut b = self.field_state(id, id, disabled);
    (b.submitted, b.cancelled, b.edit) = (submitted, cancelled, shown_edit(&st));
    *self.state::<FieldState>(id) = st;
    b
  }

  /// A text behaviour's pointer and focus, for `id` drawing its text in `text_id`.
  fn field_state(&self, id: Id, text_id: Id, disabled: bool) -> FieldBehavior {
    let (hovered, focused) = (!disabled && self.hovered(id), !disabled && self.focus_visible(id));
    FieldBehavior { id, text_id, disabled, hovered, focused, ..FieldBehavior::default() }
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
    let viewport = self.st.last.rect(id);
    let lay = TextLayout::of(&self.st.last, st.text_id).filter(|l| shows(l, st));
    let lay = lay.map(|mut l| {
      let rows = viewport.map_or(0.0, |r| r.h / l.line_h) as usize;
      l.page = rows.saturating_sub(1).max(1);
      l
    });
    let mono = Mono::PLAIN;
    let lines: &dyn Lines = match &lay {
      Some(l) if !st.secure => l,
      _ => &mono,
    };

    let multiline = st.ed.multiline;
    let (mut submitted, mut cancelled) = (false, false);
    if let Some(c) = self.command(id) {
      let c = if st.secure { by_lines(c) } else { c };
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
          if multiline =>
        {
          let line_h = lay.map_or(0.0, |l| l.line_h);
          if let Some(n) = self.st.last.get(id) {
            page(self.st.store.raw::<ScrollState>(id), n.rect.h, c, line_h);
          }

          true
        }
        Command::Copy | Command::Cut if st.secure => false,
        Command::Copy | Command::SelectAll | Command::Move(..) if !st.editable => {
          st.ed.apply(c, lines, &mut *self.st.clipboard)
        }
        _ if !st.editable => false,
        c => st.ed.apply(c, lines, &mut *self.st.clipboard),
      };

      if !used {
        self.pass(id);
      }
    }

    if st.editable {
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

    (submitted, cancelled)
  }
}

impl Ui<'_> {
  /// Gives the next element the text element's id: named (taking the pointer) for selectable
  /// text, not in a field, whose box takes it.
  pub(crate) fn text_id(&mut self, b: &FieldBehavior) -> &mut Self {
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

/// The app's `text` into the editor when the app changed it: at once, or held while an input
/// method composes.
fn take_value(st: &mut FieldState, text: &str) {
  if text == st.app {
    return;
  }

  match st.ed.marked() {
    Some(_) => st.held = Some(text.to_owned()),
    None => {
      st.ed.set_text(text);
      st.held = None;
      synced(st, text);
    }
  }
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
  match (press, st.drag.is_some()) {
    (Some((p, clicks, shift)), false) => start_drag(st, lay, p, clicks, shift),
    (Some(_), true) => {
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

/// Page Up and Down, Home and End in a multi-line field of height `h`: scrolls without moving
/// the caret.
fn page(s: &mut ScrollState, h: f32, c: Command, line_h: f32) {
  let page = (h - line_h).max(1.0);
  s.offset.y = match c {
    Command::ScrollPageUp => s.offset.y - page,
    Command::ScrollPageDown => s.offset.y + page,
    Command::ScrollToTop => 0.0,
    _ => f32::MAX,
  };
  s.pending = Point::default();
}

/// An input method's event on the editor; a password types its commits, nothing else.
fn apply_ime(st: &mut FieldState, ime: &Ime) {
  let s = st.ed.text();
  let replace = |r: &Option<Range<usize>>| r.clone().map(|r| range_from_utf16(s, r));
  match ime {
    Ime::Commit { text, .. } if st.secure => st.ed.insert(text),
    _ if st.secure => {}
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
  match st.secure {
    true => l.text.len() == graphemes(st.ed.text()).count() * DOT.len(),
    false => l.text == st.ed.text(),
  }
}

/// Offset `i` of the editor's text in the text shown: a dot per grapheme for a password.
fn shown(st: &FieldState, i: usize) -> u32 {
  match st.secure {
    true => (graphemes(&st.ed.text()[..i]).count() * DOT.len()) as u32,
    false => i as u32,
  }
}

/// Offset `i` of the text shown in the editor's text.
fn real(st: &FieldState, i: usize) -> usize {
  match st.secure {
    true => graphemes(st.ed.text()).nth(i / DOT.len()).map_or(st.ed.text().len(), |g| g.start),
    false => snap(st.ed.text(), i),
  }
}

/// The selection and marked text to draw, in the text shown.
fn shown_edit(st: &FieldState) -> Edit {
  let sel = st.ed.sel();
  let mut e = Edit::new((shown(st, sel.anchor), shown(st, sel.head)), [0.0; 4], [0.0; 4]);
  if let Some(m) = st.ed.marked() {
    e.marked = (shown(st, m.start), shown(st, m.end));
  }

  e
}

/// A press at `p`: the caret there (Shift extends the selection to it), or the word or line
/// there for a double or triple click (all of a password); a drag goes on from it.
fn start_drag(st: &mut FieldState, lay: &TextLayout, p: Point, clicks: u32, shift: bool) {
  let i = real(st, lay.hit(p, clicks >= 2));
  let s = st.ed.text();
  let r = match clicks {
    0 | 1 => i..i,
    _ if st.secure => 0..s.len(),
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
  let i = real(st, lay.hit(p, clicks >= 2));
  let s = st.ed.text();
  let sel = match clicks {
    0 | 1 => Sel::new(a.start, i),
    _ if st.secure => Sel::new(0, s.len()),
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

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::NamedKey;
  use crate::app::{App, Cx, Event};
  use crate::harness::Harness;
  use crate::scene::{Color, Quad, Size};
  use crate::ui::px;

  const REST: Color = [0.2, 0.2, 0.2, 1.0];
  const OVER: Color = [0.0, 0.0, 1.0, 1.0];
  const DOWN: Color = [1.0, 0.0, 0.0, 1.0];

  /// A round button the app draws itself, on the button's behaviour.
  #[derive(Default)]
  struct Round {
    plays: usize,
    off: bool,
  }

  impl App for Round {
    fn view(&mut self, ui: &mut Ui) {
      let b = ui.disabled(self.off).button_behavior(Id::of("play"));
      let fill = match (b.pressed, b.hovered) {
        (true, _) => DOWN,
        (_, true) => OVER,
        _ => REST,
      };

      let s = b.style(Style::new().w(px(40.0)).h(px(40.0)));
      ui.with_id(b.id).canvas(s, move |f, r| f.quad(Quad::new(r, fill).radius(20.0)));
      if b.clicked {
        self.plays += 1;
      }
    }
  }

  #[test]
  fn a_custom_drawn_button_on_the_behaviour() {
    let mut h = Harness::new(Round::default(), Size::new(100.0, 100.0), 1.0);
    let px = |h: &mut Harness<Round>| h.render().pixel(20.0, 20.0);
    assert_eq!(px(&mut h), [51, 51, 51, 255]);
    // Hovered, held, clicked: the app's colours, one play.
    h.move_to(20.0, 20.0);
    assert_eq!(px(&mut h), [0, 0, 255, 255]);
    h.press(20.0, 20.0);
    assert_eq!(px(&mut h), [255, 0, 0, 255]);
    h.release(20.0, 20.0);
    assert_eq!(h.app().plays, 1);
    // Tab focuses it, Space plays and flashes it pressed.
    h.leave();
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focused(), Some(Id::of("play")));
    h.key(Key::Char(' '), Modifiers::empty());
    assert_eq!(h.app().plays, 2);
    assert_eq!(px(&mut h), [255, 0, 0, 255]);
    // Space again before the flash ends: pressed until the second one ends, then at rest.
    h.wait(Duration::from_millis(80));
    h.key(Key::Char(' '), Modifiers::empty());
    h.wait(Duration::from_millis(60));
    assert_eq!(px(&mut h), [255, 0, 0, 255]);
    h.wait(Duration::from_millis(100));
    assert_eq!((px(&mut h), h.app().plays), ([51, 51, 51, 255], 3));
    // Alt-Space is the window menu's on Windows: it goes on unused.
    assert!(!h.key(Key::Char(' '), Modifiers::ALT));
    assert_eq!(h.app().plays, 3);
    // Disabled while focused: focus goes, no input, and Tab passes it by.
    h.app_mut().off = true;
    h.rt.redraw();
    h.frame();
    assert_eq!(h.focused(), None);
    h.click(20.0, 20.0);
    assert_eq!(h.app().plays, 3);
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focused(), None);
  }

  /// A button, and a timer a second away that the app sets on T.
  struct Alarm;

  impl App for Alarm {
    fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
      let Event::KeyDown(k) = ev else { return false };
      if k.key != Key::Char('t') {
        return false;
      }

      cx.after(Duration::from_secs(1));
      true
    }

    fn view(&mut self, ui: &mut Ui) {
      let b = ui.button_behavior(Id::of("ok"));
      ui.with_id(b.id).el(b.style(Style::new().w(px(20.0)).h(px(20.0))));
    }
  }

  #[test]
  fn a_flash_keeps_the_apps_timer() {
    let mut h = Harness::new(Alarm, Size::new(100.0, 100.0), 1.0);
    h.frame();
    h.key(NamedKey::Tab, Modifiers::empty());
    h.key(Key::Char('t'), Modifiers::empty());
    h.key(Key::Char(' '), Modifiers::empty());
    // The flash's frame comes first, then the timer's, which it did not replace.
    assert_eq!(h.rt.due_in(), Some(Duration::from_millis(120)));
    h.wait(Duration::from_millis(200));
    assert_eq!(h.rt.due_in(), Some(Duration::from_millis(800)));
    assert!(h.wait(Duration::from_millis(800)) > 0);
    assert!(!h.rt.needs_frame() && h.rt.due_in().is_none());
  }

  /// A disabled control whose own style asks for focus, then an enabled one.
  struct Pair;

  impl App for Pair {
    fn view(&mut self, ui: &mut Ui) {
      let sq = Style::new().w(px(20.0)).h(px(20.0));
      let a = ui.disabled(true).button_behavior(Id::of("a"));
      ui.with_id(a.id).el(a.style(sq.tab_focusable().cursor(Cursor::Pointer)));
      let c = ui.button_behavior(Id::of("c"));
      ui.with_id(c.id).el(c.style(sq));
    }
  }

  #[test]
  fn tab_passes_a_disabled_control_by_whatever_its_style() {
    let mut h = Harness::new(Pair, Size::new(100.0, 100.0), 1.0);
    h.frame();
    h.move_to(10.0, 10.0);
    assert_eq!(h.rt.cursor(), Cursor::Arrow);
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focused(), Some(Id::of("c")));
  }

  /// A field the app draws itself: its own box over the behaviour, the text from it.
  #[derive(Default)]
  struct Own {
    text: String,
    seen: Vec<FieldBehavior>,
  }

  impl App for Own {
    fn view(&mut self, ui: &mut Ui) {
      let b = ui.text_field_behavior(Id::of("own"), &mut self.text, false, false);
      let style = crate::ui::Theme::PLATES_DARK.field();
      let frame =
        b.style(Style::new().w(px(120.0)).h(px(24.0)).bg(if b.focused { OVER } else { REST }));
      ui.with_id(b.id).row(frame, |ui| ui.field_text(&b, &style, Style::new()));
      self.seen.push(b);
    }
  }

  #[test]
  fn a_custom_drawn_field_on_the_behaviour() {
    let mut h = Harness::new(Own::default(), Size::new(200.0, 100.0), 1.0);
    h.frame();
    h.click(10.0, 10.0);
    assert_eq!(h.rt.cursor(), Cursor::Text);
    h.type_text("hi");
    assert_eq!(h.app().text, "hi");
    let b = *h.app().seen.last().unwrap();
    assert!(b.focused && b.changed && !b.password);
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
    let st =
      FieldState { ed: Editor::new("ae\u{301}👍🏽"), secure: true, ..FieldState::default() };
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
    // Held again, then committed: the commit wins.
    st.ed.set_marked("y", 1..1, None);
    text = "other".into();
    take_value(&mut st, &text);
    st.ed.commit("Y", None);
    assert!(write_back(&mut st, &mut text));
    assert_eq!(text, "appY");
  }
}
