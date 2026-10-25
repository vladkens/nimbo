// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The element tree an app builds each frame it redraws ([crate::App::view]), its flex layout
//! ([layout]) and the paint pass that turns it into the frame's scene ([paint]).
//!
//! ```ignore
//! ui.row(Style::new().pad(8.0).gap(8.0).bg(PANEL), |ui| {
//!   ui.text("Title", BOLD);
//!   ui.spacer();
//!   ui.id("save").row(BUTTON, |ui| ui.text("Save", LABEL));
//! });
//! ```
//!
//! Containers take their children in a closure, so the code nests like the tree. Every element
//! gets an [Id]: its index among its siblings hashed with its parent's id, a key hashed the same
//! way ([Ui::key], stable when lists change), or a frame-wide name ([Ui::id], found again with
//! [Id::of]). The last frame's tree with every element's rect is kept for hit testing
//! ([input]): named elements get hover, press and clicks, which the view reads by id. State the
//! app should not hold (scroll offsets, animations) lives in a store keyed by id ([store]).
//!
//! ```ignore
//! let plus = Id::of("plus");
//! let fade = ui.animate(plus, if ui.hovered(plus) { 1.0 } else { 0.0 }, FADE);
//! if ui.with_id(plus).row(BUTTON.bg(mix(RAISED, HOVER, fade)), |ui| ui.text("+", LABEL)).clicked {
//!   self.count += 1;
//! }
//! ```
//!
//! Widgets ([widgets]: label, button, text field, check box, switch, slider, select and the
//! like) and containers ([containers]: scroll area, list, menu, pop-up button, tabs, split) do
//! this for an app in the look of the [Theme] set with [Ui::set_theme]:
//! `if ui.id("plus").button("+").clicked { .. }`; so do a settings form ([form]) and a modal
//! dialog ([dialog]).
//!
//! The rule for every widget: a headless behaviour ([behavior]: `ui.button_behavior(id)` and
//! the like, hover, press, click, focus, keys and the value edited) plus drawing from the
//! theme. An app that wants its own look keeps the behaviour and draws the element itself.

pub mod behavior;
pub mod containers;
pub mod dialog;
pub mod form;
pub mod input;
mod layout;
mod paint;
pub mod store;
mod style;
mod textlayout;
pub mod theme;
pub mod widgets;

use std::any::Any;
use std::fmt::{Display, Write};
use std::hash::{Hash, Hasher};
use std::time::Duration;

pub use behavior::{Behavior, FieldBehavior};
pub use containers::{
  List, ListResponse, PopUp, Row, Selection, Split, SplitBehavior, Tabs, TypeSelect,
};
pub use dialog::{Answer, Dialog, DialogBehavior};
pub use form::{FormRow, FormStyle, Glass, Tile};
pub(crate) use input::drags_window;
pub use input::{Click, Press, Scroll, hit};
pub(crate) use layout::Sys;
pub use layout::{Kind, Line, Measure, Node, Tree};
pub use store::{Ease, Transition};
pub use style::{
  Align, Clip, Cursor, Dim, Float, Focus, FocusRing, Shadow, Style, TextStyle, grow, pct, px, rigid,
};
pub use textlayout::{Edit, Stop, TextLayout};
pub use theme::{ButtonStyle, ControlSize, Sizes, SwitchSize, Theme, Tone, Variant};
pub use widgets::{Button, Choice, FieldStyle, Icon, Select, TextInput};

use self::input::{Deliver, Input, ScrollState};
use self::store::{Anim, Store};
use crate::app::{Appearance, Clipboard, Event, MouseButton, Waker};
use crate::keys::{Binding, Command, KeyEvent};
use crate::scene::{Frame, ImageId, Images, Point, Rect, Size};
use crate::text::{Font, FxHasher, Metrics, TextSystem};

/// How long [UiState::flash] shows an element pressed.
const FLASH: Duration = Duration::from_millis(120);

// MARK: Ids

/// An element's id, stable across frames while the app builds the same element at the same place
/// (or with the same key or name).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Id(pub u64);

impl Id {
  /// The implicit root: the window, a column.
  pub const ROOT: Self = Self(0);

  /// A frame-wide id from a name: what [Ui::id] gives an element.
  pub fn of(name: impl Hash) -> Self {
    hash((2u8, name))
  }

  /// A child's id from a key unique among its siblings: what [Ui::key] gives an element.
  pub fn key(self, key: impl Hash) -> Self {
    hash((self.0, 1u8, key))
  }

  /// The id of the `i`-th child with no key or name.
  pub fn nth(self, i: usize) -> Self {
    hash((self.0, 0u8, i))
  }
}

fn hash(v: impl Hash) -> Id {
  // A seed: FxHash of zeros from a zero state is zero, the root's id.
  let mut h = IdHasher(FxHasher::default());
  h.write_u64(0x9e37_79b9_7f4a_7c15);
  v.hash(&mut h);
  Id(h.finish())
}

/// FxHash with every integer fed in as 64 bits, so a key doesn't depend on its integer's type:
/// `id.key(3)`, `id.key(3usize)` and `id.key(3u8)` are one id, and so are `-1i32` and `-1i64`.
struct IdHasher(FxHasher);

impl Hasher for IdHasher {
  fn finish(&self) -> u64 {
    self.0.finish()
  }

  fn write(&mut self, bytes: &[u8]) {
    self.0.write(bytes);
  }

  fn write_u8(&mut self, v: u8) {
    self.0.write_u64(v as u64);
  }

  fn write_u16(&mut self, v: u16) {
    self.0.write_u64(v as u64);
  }

  fn write_u32(&mut self, v: u32) {
    self.0.write_u64(v as u64);
  }

  fn write_u64(&mut self, v: u64) {
    self.0.write_u64(v);
  }

  fn write_usize(&mut self, v: usize) {
    self.0.write_u64(v as u64);
  }

  fn write_i8(&mut self, v: i8) {
    self.0.write_u64(v as i64 as u64);
  }

  fn write_i16(&mut self, v: i16) {
    self.0.write_u64(v as i64 as u64);
  }

  fn write_i32(&mut self, v: i32) {
    self.0.write_u64(v as i64 as u64);
  }

  fn write_i64(&mut self, v: i64) {
    self.0.write_u64(v as u64);
  }

  fn write_isize(&mut self, v: isize) {
    self.0.write_u64(v as i64 as u64);
  }
}

// MARK: Builder

/// Paints a canvas element: gets the frame and the element's laid-out rect.
pub type Canvas<'a> = Box<dyn FnOnce(&mut Frame, Rect) + 'a>;

/// What a [Ui] keeps between frames: the tree being built and the last one laid out, pointer
/// state, the per-id store and timers, so a steady frame allocates nothing.
pub struct UiState {
  tree: Tree,
  /// The last frame's tree, laid out: ids and rects for hit testing.
  pub last: Tree,
  /// Open containers: node index and the number of children so far.
  stack: Vec<(u32, u32)>,
  pub(crate) input: Input,
  store: Store,
  /// When the last frame's view asked for the next ([Ui::request_frame_in]), on the runtime's
  /// clock: each frame asks again for what it still needs (a caret's next blink).
  pub(crate) due: Option<Duration>,
  /// When an event handler asked for a frame ([crate::Cx::after]); kept until it comes.
  pub(crate) after: Option<Duration>,
  /// An animation in the frame built last has not reached its target.
  pub(crate) animating: bool,
  pub(crate) waker: Waker,
  pub(crate) clipboard: Box<dyn Clipboard>,
  ring: Option<FocusRing>,
  theme: Theme,
  /// The OS's look, which the theme follows unless `pinned`.
  appearance: Appearance,
  /// The app set its theme ([Ui::set_theme]).
  pinned: bool,
  /// What the last frame cost ([Ui::last_frame]).
  pub(crate) stats: FrameStats,
  /// The content size the app asked the window for ([Ui::set_window_size]), until the platform
  /// takes it ([crate::Runtime::take_resize]).
  pub(crate) resize: Option<Size>,
  /// The last size asked: asking it again is nothing new, so a view may ask every frame.
  asked: Option<Size>,
  /// The file panel asked for ([Ui::file_dialog]), until the platform takes it.
  pub(crate) file_dialog: Option<crate::FileDialog>,
  /// The height of the title bar over the content and the window's title ([Ui::titlebar]).
  pub(crate) titlebar: Option<(f32, &'static str)>,
  /// The images [Ui::rgba_image] added: their pixels' hash, the image, and whether a view built
  /// it since the last frame.
  pub(crate) pictures: Vec<(u64, ImageId, bool)>,
  /// The element that takes key presses before any binding ([Ui::grab_keys]), as the view
  /// built last asked.
  pub(crate) grab: Option<Id>,
}

/// What the runtime's last frame cost, for a status bar or a frame meter ([Ui::last_frame]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameStats {
  /// Time in the app's view (every pass), the layout and painting the tree (canvases too).
  pub view: Duration,
  pub layout: Duration,
  pub paint: Duration,
  /// Times the view ran: 2 when the hover moved under the new layout.
  pub passes: u32,
  /// Frames built so far: frames per second from two readings a second apart.
  pub count: u64,
}

impl FrameStats {
  /// View, layout and paint.
  pub fn total(&self) -> Duration {
    self.view + self.layout + self.paint
  }
}

impl Default for UiState {
  /// The dark theme and its focus ring, following the OS's appearance once the platform (or the
  /// harness, `--appearance`) reports it.
  fn default() -> Self {
    Self {
      tree: Tree::default(),
      last: Tree::default(),
      stack: Vec::new(),
      input: Input::default(),
      store: Store::default(),
      due: None,
      after: None,
      animating: false,
      waker: Waker::default(),
      clipboard: Box::default(),
      ring: Some(Theme::PLATES_DARK.focus_ring()),
      theme: Theme::PLATES_DARK,
      appearance: Appearance::DARK,
      pinned: false,
      stats: FrameStats::default(),
      resize: None,
      asked: None,
      file_dialog: None,
      titlebar: None,
      pictures: Vec::new(),
      grab: None,
    }
  }
}

impl UiState {
  /// The OS's look as last reported ([Event::Appearance]).
  pub fn appearance(&self) -> Appearance {
    self.appearance
  }

  /// Asks the window for a content size of `size` points when it differs from the last one
  /// asked and from the window's `now`.
  pub(crate) fn ask_size(&mut self, size: Size, now: Size) {
    if self.asked == Some(size) {
      return;
    }

    self.asked = Some(size);
    self.resize = (size != now).then_some(size);
  }

  /// Takes the OS's new look; the theme follows unless the app set one.
  pub(crate) fn set_appearance(&mut self, a: Appearance) {
    self.appearance = a;
    if !self.pinned {
      self.retheme(Theme::system(a));
    }
  }

  /// Draws with `theme`; the focus ring follows unless the app set its own.
  fn retheme(&mut self, theme: Theme) {
    if self.ring == Some(self.theme.focus_ring()) {
      self.ring = Some(theme.focus_ring());
    }

    self.theme = theme;
  }

  /// Whether the pointer is over `id` or an element inside it.
  pub fn hovered(&self, id: Id) -> bool {
    self.input.hover.contains(&id)
  }

  /// The interactive elements under the pointer, innermost first.
  pub fn hover(&self) -> &[Id] {
    &self.input.hover
  }

  /// Whether a button went down on `id` and is held with the pointer still over it, or `id`
  /// flashes pressed ([UiState::flash]).
  pub fn pressed(&self, id: Id) -> bool {
    self.held(id) && self.hovered(id)
      || self.input.flash.is_some_and(|(f, until)| f == id && self.input.now < until)
  }

  /// Shows `id` pressed for a moment from `now`, as a click would: a key on a keypad typed on
  /// the keyboard, a button activated by Space. A frame follows when it ends, beside the
  /// timers ([UiState::next_due]).
  pub fn flash(&mut self, id: Id, now: Duration) {
    self.input.flash = Some((id, now + FLASH));
  }

  /// Whether a button went down on `id` and is held, wherever the pointer is now (drags).
  pub fn held(&self, id: Id) -> bool {
    self.input.press.is_some_and(|p| p.id == id)
  }

  pub fn press(&self) -> Option<Press> {
    self.input.press
  }

  /// The click being delivered, if it is on `id`.
  pub fn click(&self, id: Id) -> Option<Click> {
    self.input.click.filter(|c| c.id == id)
  }

  /// Whether `id` was clicked with the left button.
  pub fn clicked(&self, id: Id) -> bool {
    self.click(id).is_some_and(|c| c.button == MouseButton::Left)
  }

  pub fn focused(&self, id: Id) -> bool {
    self.input.focus == Some(id)
  }

  pub fn focus(&self) -> Option<Id> {
    self.input.focus
  }

  pub fn set_focus(&mut self, id: Option<Id>) {
    self.input.focus = id;
  }

  /// Whether the window is the key window, which gets the keys.
  pub fn window_focused(&self) -> bool {
    !self.input.blurred
  }

  /// Moves focus to the next focusable element of the last frame in tree order, or the previous
  /// one, wrapping around; from none, to the first or the last. Returns whether there is one.
  pub fn focus_next(&mut self, forward: bool) -> bool {
    let t = &self.last;
    let all = || t.nodes.iter().filter(|n| n.style.focus != Focus::None).map(|n| n.id);
    let n = all().count();
    if n == 0 {
      return false;
    }

    let at = self.input.focus.and_then(|f| all().position(|id| id == f));
    let next = match (at, forward) {
      (Some(i), true) => (i + 1) % n,
      (Some(i), false) => (i + n - 1) % n,
      (None, true) => 0,
      (None, false) => n - 1,
    };

    self.input.focus = all().nth(next);
    if let Some(id) = self.input.focus {
      self.reveal_in_scrolls(id);
    }

    true
  }

  /// Scrolls every scroll container around `id` the least that shows it, innermost first, by
  /// the last frame's rects, up to an overlay placed on the window, which their offsets don't
  /// move; an overlay on its parent skips that parent's, which doesn't move it either.
  fn reveal_in_scrolls(&mut self, id: Id) {
    let t = &self.last;
    let Some(mut i) = t.find(id) else { return };
    let mut r = t.nodes[i as usize].rect;
    while i != 0 && !t.nodes[i as usize].float.is_some_and(|f| f.window) {
      let float = t.nodes[i as usize].float.is_some();
      i = t.nodes[i as usize].parent;
      let n = &t.nodes[i as usize];
      if float || !(n.style.scroll[0] || n.style.scroll[1]) {
        continue;
      }
      // In the container's content, then its offset after showing it.
      let c = Rect::new(r.x - n.rect.x + n.scroll.x, r.y - n.rect.y + n.scroll.y, r.w, r.h);
      let off = reveal_offset(n.scroll, n.rect, c);
      let s = self.store.raw::<ScrollState>(n.id);
      (s.offset, s.pending) = (off, Point::default());
      r = Rect::new(r.x - (off.x - n.scroll.x), r.y - (off.y - n.scroll.y), r.w, r.h);
    }
  }

  /// The pointer's shape: from the element a button is held on, else the innermost one under
  /// the pointer, or the nearest ancestor that sets one; the arrow when none does.
  pub fn cursor(&self) -> Cursor {
    let from = self.input.press.map(|p| p.id).or_else(|| self.input.hover.first().copied());
    let mut at = from.and_then(|id| self.last.find(id));
    while let Some(i) = at {
      let n = &self.last.nodes[i as usize];
      if n.style.cursor != Cursor::Inherit {
        return n.style.cursor;
      }

      at = (i != 0).then_some(n.parent);
    }

    Cursor::Arrow
  }

  /// Starts delivering `ev` to `id` in a view pass.
  pub(crate) fn deliver(&mut self, id: Id, ev: Event) {
    let (used, seen) = Default::default();
    self.input.deliver = Some(Deliver { target: id, ev, used, seen });
  }

  /// Ends a delivery: whether the view used it, and whether it read it at all.
  pub(crate) fn delivered(&mut self) -> (bool, bool) {
    self.input.deliver.take().map_or((false, false), |d| (d.used.get(), d.seen.get()))
  }

  /// What `pick` finds in the event being delivered to `id`, which is then marked used and
  /// seen; when it finds nothing, the event is left as it was.
  fn take<'s, T>(&'s self, id: Id, pick: impl FnOnce(&'s Event) -> Option<T>) -> Option<T> {
    let d = self.input.deliver.as_ref().filter(|d| d.target == id)?;
    let found = pick(&d.ev)?;
    d.used.set(true);
    d.seen.set(true);
    Some(found)
  }

  /// Asks for a frame at `at` on the runtime's clock (the earliest request wins), from the
  /// view.
  pub fn request_frame_at(&mut self, at: Duration) {
    self.due = Some(self.due.map_or(at, |d| d.min(at)));
  }

  /// The next frame a timer asked for, or the end of a flash: the earliest.
  pub(crate) fn next_due(&self) -> Option<Duration> {
    let flash = self.input.flash.map(|(_, until)| until);
    [self.due, self.after, flash].into_iter().flatten().min()
  }

  fn response(&self, id: Id) -> Response {
    Response {
      id,
      hovered: self.hovered(id),
      pressed: self.pressed(id),
      focused: self.focused(id),
      clicked: self.clicked(id),
      changed: false,
      submitted: false,
      cancelled: false,
      blurred: false,
    }
  }
}

/// The offset of a scroll container showing `viewport` at `off` that moves the least to show
/// `r`, a rect of its content.
fn reveal_offset(mut off: Point, viewport: Rect, r: Rect) -> Point {
  let fit = |off: f32, lo: f32, len: f32, view: f32| match () {
    _ if lo < off => lo,
    _ if lo + len > off + view => (lo + len - view).min(lo),
    _ => off,
  };
  off.x = fit(off.x, r.x, r.w, viewport.w);
  off.y = fit(off.y, r.y, r.h, viewport.h);
  off
}

/// What an interactive element got from the pointer, as its builder returns it. Hover and press
/// are known before building, from the last frame: query them first with [Ui::hovered] and
/// [Ui::pressed] to pick the element's style. [Ui::click] has the details of a click.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Response {
  pub id: Id,
  pub hovered: bool,
  pub pressed: bool,
  pub focused: bool,
  /// Clicked with the left button; seen once.
  pub clicked: bool,
  /// The value a widget edits changed: a text field's text.
  pub changed: bool,
  /// Enter in a single-line text field.
  pub submitted: bool,
  /// Escape in a text field.
  pub cancelled: bool,
  /// A text field lost focus since the view last built it.
  pub blurred: bool,
}

/// The element tree builder an app's `view` gets each frame it redraws. `'a` lets canvas
/// callbacks borrow the app's data: `fn view<'a>(&'a mut self, ui: &mut Ui<'a>)`.
///
/// Input reaches the view as state: [Ui::hovered] and [Ui::pressed] hold while they last, a
/// click is seen once ([Ui::clicked], or [Response::clicked] from the element's builder). The
/// runtime runs the view once more right after a click, without drawing, so the app changes
/// its state before the frame that shows it, wherever in the view the click is read.
pub struct Ui<'a> {
  pub(crate) st: UiState,
  canvases: Vec<Option<Canvas<'a>>>,
  /// The next element's id, and whether it is named (interactive).
  next_id: Option<(Id, bool)>,
  next_float: Option<Float>,
  next_keymap: Option<&'static [Binding]>,
  /// The next widget is disabled ([Ui::disabled]).
  next_disabled: bool,
  /// Every widget built is disabled ([Ui::disabled_scope]).
  scope_off: bool,
  /// The next element takes focus when it first shows ([Ui::autofocus]).
  next_autofocus: bool,
  /// The next element moves the window when pressed ([Ui::drags_window]).
  next_drag: bool,
  /// The runtime's text system, lent while the view runs ([Ui::measure]).
  pub(crate) text: Option<TextSystem>,
  /// The runtime's images, lent while the view runs ([Ui::rgba_image]).
  pub(crate) images: Images,
  /// Columns the next element takes in a grid ([Ui::span]).
  next_span: u8,
  /// The width and the least and most height of a window as high as its content
  /// ([Ui::fit_window_height]).
  fit: Option<[f32; 3]>,
  /// Build-time state of add-ons, one per type ([Ui::slot]).
  slots: Vec<Box<dyn Any>>,
  size: Size,
  mouse: Option<Point>,
  now: Duration,
}

impl<'a> Ui<'a> {
  /// Starts a frame's tree in `st` for a window of `size`.
  pub fn new(mut st: UiState, size: Size, mouse: Option<Point>, now: Duration) -> Self {
    st.tree.clear(size);
    st.stack.clear();
    st.stack.push((0, 0));
    st.animating = false;
    st.input.moves = false;
    st.grab = None;
    st.input.now = now;
    let (next_id, next_float, next_keymap) = (None, None, None);
    let canvases = Vec::new();
    let (next_disabled, next_autofocus) = (false, false);
    Self {
      st,
      canvases,
      next_id,
      next_float,
      next_keymap,
      next_disabled,
      scope_off: false,
      next_autofocus,
      next_drag: false,
      text: None,
      images: Images::default(),
      next_span: 1,
      fit: None,
      slots: Vec::new(),
      size,
      mouse,
      now,
    }
  }

  /// The theme widgets draw with ([theme]), a copy.
  pub fn theme(&self) -> Theme {
    self.st.theme
  }

  /// Sets the theme, kept until changed, over the OS's appearance, and its focus ring
  /// ([Ui::set_focus_ring] after this overrides the ring).
  pub fn set_theme(&mut self, theme: Theme) {
    self.st.theme = theme;
    self.st.ring = Some(theme.focus_ring());
    self.st.pinned = true;
  }

  /// Follows the OS's appearance again ([Theme::system]), the default: light or dark with
  /// its accent, switching when it changes.
  pub fn follow_system(&mut self) {
    self.st.pinned = false;
    self.st.retheme(Theme::system(self.st.appearance));
  }

  /// The OS's look, for an app drawing in colours of its own: light or dark and the accent.
  pub fn appearance(&self) -> Appearance {
    self.st.appearance
  }

  /// What the last frame cost: view, layout and paint times and the frames built so far.
  pub fn last_frame(&self) -> FrameStats {
    self.st.stats
  }

  /// The window's size in points.
  pub fn size(&self) -> Size {
    self.size
  }

  /// The build-time state of type `T` that an add-on put with [Ui::set_slot]: what a container
  /// being built (a menu, a document) tells the elements built inside it. Gone after the frame.
  pub fn slot<T: 'static>(&mut self) -> Option<&mut T> {
    self.slots.iter_mut().find_map(|s| s.downcast_mut())
  }

  /// Puts `v` as the build-time state of type `T`, or clears it with `None`; returns the one it
  /// replaces, which a nested container keeps and puts back when it ends.
  pub fn set_slot<T: 'static>(&mut self, v: Option<T>) -> Option<T> {
    let old = self.slots.iter().position(|s| s.is::<T>()).map(|i| self.slots.swap_remove(i));
    if let Some(v) = v {
      self.slots.push(Box::new(v));
    }

    old.and_then(|s| s.downcast().ok()).map(|s| *s)
  }

  /// Asks the window for a content size of `w` × `h` points: a preferences window whose height
  /// follows its tab. Asking the size asked last is nothing new, so a view may ask every frame;
  /// the platform resizes the window after the frame (macOS keeps its top edge in place), and
  /// a fixed one ([crate::Config::fixed]) too.
  pub fn set_window_size(&mut self, w: f32, h: f32) {
    self.st.ask_size(Size::new(w, h), self.size);
  }

  /// The height of the title bar the content runs under and the window's title, for a window
  /// that draws its own title bar ([crate::Config::unified]); `None` for any other.
  pub fn titlebar(&self) -> Option<(f32, &'static str)> {
    self.st.titlebar
  }

  /// Asks the window for `w` points wide and as high as its content, between `min_h` and
  /// `max_h`: the height its elements take at their fit size, a scroll area at its content's
  /// whole height (so nothing scrolls while it fits; past `max_h` it scrolls), the text wrapped
  /// at the window's width. Measured by this frame's layout, the window follows after it as for
  /// [Ui::set_window_size]: no per-OS heights for fonts of other metrics. Ask it each frame.
  pub fn fit_window_height(&mut self, w: f32, min_h: f32, max_h: f32) {
    self.fit = Some([w, min_h, max_h]);
  }

  /// The pointer, `None` when it is outside the window.
  pub fn mouse(&self) -> Option<Point> {
    self.mouse
  }

  pub fn now(&self) -> Duration {
    self.now
  }

  /// An element's rect in the last frame.
  pub fn rect(&self, id: Id) -> Option<Rect> {
    self.st.last.rect(id)
  }

  /// The tree built so far this frame.
  pub fn tree(&self) -> &Tree {
    &self.st.tree
  }

  /// Wakes the runtime from another thread: background work that has a result to show.
  pub fn waker(&self) -> &Waker {
    &self.st.waker
  }

  /// The advance width of `s` on one line in `style`'s font, in points, as the layout measures
  /// it: to size things by their text while building (a display shrinking its font to fit, a
  /// column as wide as its digits). 0 in a [Ui] the runtime didn't build.
  pub fn measure(&mut self, s: &str, style: TextStyle) -> f32 {
    self.text.as_mut().map_or(0.0, |t| t.measure(s, style.font))
  }

  /// The widest line of `s` in `style` wrapped at `width`, as the layout breaks it: to fit a box
  /// to its wrapped text (a tooltip), as the layout sizes one by the text's width before it
  /// wraps. 0 in a [Ui] the runtime didn't build.
  pub fn measure_wrapped(&mut self, s: &str, style: TextStyle, width: f32) -> f32 {
    let Some(text) = self.text.as_mut() else { return 0.0 };
    let mut m = Sys { text, images: &self.images, scale: 1.0 };
    layout::wrapped_width(s, style, width, &mut m)
  }

  /// Vertical metrics of `font` as the layout uses them: to set a mark beside text by its
  /// baseline, cap or x-height. Zeros in a [Ui] the runtime didn't build.
  pub fn metrics(&mut self, font: Font) -> Metrics {
    self.text.as_mut().map_or(Metrics::default(), |t| t.metrics(font))
  }

  // MARK: Ids

  /// Keys the next element: its id is the parent's hashed with `key`, so it stays when the
  /// siblings before it change. A key gives identity only; add [Style::hit] for input.
  pub fn key(&mut self, key: impl Hash) -> &mut Self {
    let parent = self.st.stack.last().unwrap().0;
    self.next_id = Some((self.st.tree.nodes[parent as usize].id.key(key), false));
    self
  }

  /// Names the next element with a frame-wide id, [Id::of] `name`; a named element takes
  /// pointer input.
  pub fn id(&mut self, name: impl Hash) -> &mut Self {
    self.with_id(Id::of(name))
  }

  /// Names the next element with a ready id, as [Ui::id] does.
  pub fn with_id(&mut self, id: Id) -> &mut Self {
    self.next_id = Some((id, true));
    self
  }

  /// Places the next element out of the flow, as an overlay.
  pub fn float(&mut self, float: Float) -> &mut Self {
    self.next_float = Some(float);
    self
  }

  /// Gives the next element key bindings, tried while it or an element inside it has focus,
  /// innermost first: a match comes to it as a command ([Ui::command]).
  pub fn keymap(&mut self, map: &'static [Binding]) -> &mut Self {
    self.next_keymap = Some(map);
    self
  }

  /// The next element takes `n` columns of the grid it is in ([Ui::grid]): the keypad's wide 0.
  pub fn span(&mut self, n: u8) -> &mut Self {
    self.next_span = n;
    self
  }

  /// Disables the next widget ([widgets]) when `yes`: dimmed, not focusable, and its clicks
  /// do nothing. Plain elements ignore it.
  pub fn disabled(&mut self, yes: bool) -> &mut Self {
    self.next_disabled = yes;
    self
  }

  /// Focuses the next widget, or the next element if it is focusable, when it first shows:
  /// when the last frame did not have it. A field that opens for an edit, the first field of a
  /// form; the view sees the focus at once.
  pub fn autofocus(&mut self) -> &mut Self {
    self.next_autofocus = true;
    self
  }

  /// Makes the next element move the window when pressed where no interactive element inside it
  /// is: the header of a window that draws its own title bar ([crate::Config::unified]). A
  /// double-click zooms or minimizes the window as the OS's setting says; the app never sees
  /// the press. The element takes pointer input.
  pub fn drags_window(&mut self) -> &mut Self {
    self.next_drag = true;
    self
  }

  /// The id the next element gets, taken for a widget to name its element with: the one set
  /// by [Ui::id] or [Ui::key], else its position among its siblings.
  pub fn take_id(&mut self) -> Id {
    let id = match self.next_id.take() {
      Some((id, _)) => id,
      None => {
        let &(parent, count) = self.st.stack.last().unwrap();
        self.st.tree.nodes[parent as usize].id.nth(count as usize)
      }
    };

    self.take_autofocus(id);
    id
  }

  /// Applies [Ui::autofocus] to `id`, for widgets given their id: focuses it when the last
  /// frame did not have it, unless it is disabled ([Ui::disabled], read without taking it).
  pub fn take_autofocus(&mut self, id: Id) {
    let off = self.next_disabled || self.scope_off;
    if std::mem::take(&mut self.next_autofocus) && !off && self.st.last.find(id).is_none() {
      self.st.input.focus = Some(id);
    }
  }

  /// Disables every widget `f` builds when `yes` ([Ui::disabled] for each): a form row that
  /// doesn't apply, with its label and control dimmed.
  pub fn disabled_scope<R>(&mut self, yes: bool, f: impl FnOnce(&mut Self) -> R) -> R {
    let was = self.scope_off;
    self.scope_off |= yes;
    let r = f(self);
    self.scope_off = was;
    r
  }

  /// Whether widgets built now are disabled by a [Ui::disabled_scope].
  pub fn scope_disabled(&self) -> bool {
    self.scope_off
  }

  /// Whether the next widget is disabled ([Ui::disabled], or inside a disabled
  /// [Ui::disabled_scope]), for the widget to read once.
  pub fn take_disabled(&mut self) -> bool {
    std::mem::take(&mut self.next_disabled) || self.scope_off
  }

  // MARK: Elements

  /// A container laying its children out left to right.
  pub fn row<R>(&mut self, style: Style, f: impl FnOnce(&mut Self) -> R) -> Response {
    self.container(0, style, f)
  }

  /// A container laying its children out top to bottom.
  pub fn col<R>(&mut self, style: Style, f: impl FnOnce(&mut Self) -> R) -> Response {
    self.container(1, style, f)
  }

  /// A container laying its children out in `cols` equal columns, left to right and wrapping
  /// to the next row, `style.gap` apart both ways: a keypad, a form, a gallery. A child takes
  /// one cell, or [Ui::span] columns; growing children fill their cell, others sit in it by
  /// `style.align`. Rows are as high as their highest child and share the height left over
  /// when the grid is taller.
  pub fn grid<R>(&mut self, cols: u8, style: Style, f: impl FnOnce(&mut Self) -> R) -> Response {
    let (i, r) = self.open(0, style);
    self.st.tree.nodes[i as usize].cols = cols.max(1);
    f(self);
    self.close(i);
    r
  }

  /// A box with no children: a divider, a swatch, a fixed gap.
  pub fn el(&mut self, style: Style) -> Response {
    self.push(Kind::Box, style, 0).1
  }

  /// Takes the free space along the parent's axis.
  pub fn spacer(&mut self) -> Response {
    self.el(Style::new().grow())
  }

  /// Text, measured and wrapped by the layout. Takes anything `Display`, so
  /// `format_args!("{n}")` formats straight into the frame's text buffer. Returns nothing, so it
  /// fits `match` arms beside other statements; named text (`ui.id("x").text(..)`) still takes
  /// the pointer: read it with [Ui::clicked] and the like.
  pub fn text(&mut self, s: impl Display, style: TextStyle) {
    let text = &mut self.st.tree.text;
    let start = text.len() as u32;
    let _ = write!(text, "{s}");
    let range = (start, text.len() as u32);
    let kind = Kind::Text { range, style, lines: (0, 0), baseline: 0.0, line_h: 0.0 };
    self.push(kind, Style::new(), 0);
  }

  /// An image; a fit size is its pixel size over the backing scale.
  pub fn image(&mut self, image: ImageId, style: Style) -> Response {
    self.push(Kind::Image(image), style, 0).1
  }

  /// A box whose `paint` draws into the frame within its laid-out rect (a plot). It draws in
  /// its parent's layer: start one with `f.layer()` for order across primitive kinds.
  pub fn canvas(&mut self, style: Style, paint: impl FnOnce(&mut Frame, Rect) + 'a) -> Response {
    self.canvases.push(Some(Box::new(paint)));
    self.push(Kind::Canvas(self.canvases.len() as u32 - 1), style, 0).1
  }

  /// A canvas with children laid out left to right over what it paints: a plot's hit areas
  /// (each with its own cursor), axes and readouts. `paint` gets the whole box, before the
  /// children are drawn.
  pub fn canvas_row<R>(
    &mut self,
    style: Style,
    paint: impl FnOnce(&mut Frame, Rect) + 'a,
    f: impl FnOnce(&mut Self) -> R,
  ) -> Response {
    self.canvas_container(0, style, Box::new(paint), f)
  }

  /// [Ui::canvas_row] with the children top to bottom.
  pub fn canvas_col<R>(
    &mut self,
    style: Style,
    paint: impl FnOnce(&mut Frame, Rect) + 'a,
    f: impl FnOnce(&mut Self) -> R,
  ) -> Response {
    self.canvas_container(1, style, Box::new(paint), f)
  }

  fn canvas_container<R>(
    &mut self,
    axis: u8,
    style: Style,
    paint: Canvas<'a>,
    f: impl FnOnce(&mut Self) -> R,
  ) -> Response {
    self.canvases.push(Some(paint));
    let (i, r) = self.push(Kind::Canvas(self.canvases.len() as u32 - 1), style, axis);
    self.st.stack.push((i, 0));
    f(self);
    self.close(i);
    r
  }

  fn container<R>(&mut self, axis: u8, style: Style, f: impl FnOnce(&mut Self) -> R) -> Response {
    let (i, r) = self.open(axis, style);
    f(self);
    self.close(i);
    r
  }

  /// Starts a container; out of line, so each `row` / `col` call site stays small.
  #[inline(never)]
  fn open(&mut self, axis: u8, style: Style) -> (u32, Response) {
    let (i, r) = self.push(Kind::Box, style, axis);
    self.st.stack.push((i, 0));
    (i, r)
  }

  fn close(&mut self, i: u32) {
    self.st.stack.pop();
    let tree = &mut self.st.tree;
    tree.nodes[i as usize].end = tree.nodes.len() as u32;
  }

  #[inline(always)]
  fn push(&mut self, kind: Kind, style: Style, axis: u8) -> (u32, Response) {
    let (parent, count) = self.st.stack.last_mut().unwrap();
    let tree = &mut self.st.tree;
    let (id, named) = self
      .next_id
      .take()
      .unwrap_or_else(|| (tree.nodes[*parent as usize].id.nth(*count as usize), false));
    *count += 1;
    let parent = *parent;
    let scrolls = style.scroll[0] || style.scroll[1];
    let focus = style.focus != Focus::None;
    if self.next_autofocus {
      // Only a focusable element takes it; the flag goes with this one either way.
      match focus {
        true => self.take_autofocus(id),
        false => self.next_autofocus = false,
      }
    }

    let drag = std::mem::take(&mut self.next_drag);
    let hit = style.hit || named || scrolls || focus || drag || style.cursor != Cursor::Inherit;
    let tree = &mut self.st.tree;
    let i = tree.nodes.len() as u32;
    tree.drags.extend(drag.then_some(i));

    let float = self.next_float.take();
    if float.is_some() {
      tree.floats.push(i);
    }

    if let Some(map) = self.next_keymap.take() {
      tree.keymaps.push((i, map));
    }

    tree.nodes.push(Node::new(id, kind, style, axis, float, parent, i + 1));
    if self.next_span != 1 {
      tree.nodes[i as usize].span = std::mem::replace(&mut self.next_span, 1);
    }

    match hit {
      true => (i, self.interactive(i, id, scrolls)),
      false => (i, Response { id, ..Response::default() }),
    }
  }

  /// Marks a node interactive, indexes it for hit testing (and a scroll container for the
  /// layout) and reads its input; out of line, so plain elements build fast.
  #[inline(never)]
  fn interactive(&mut self, i: u32, id: Id, scrolls: bool) -> Response {
    self.st.tree.nodes[i as usize].style.hit = true;
    self.st.tree.hits.push(i);
    if scrolls {
      self.st.tree.scrolls.push(i);
      // Keeps its offset alive; the layout reads and clamps it.
      self.state::<ScrollState>(id);
    }

    self.st.response(id)
  }

  // MARK: Input

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

  /// Shows `id` pressed for a moment ([UiState::flash]).
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
  pub fn focus_next(&mut self, forward: bool) -> bool {
    self.st.focus_next(forward)
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

  /// Rings the focused box ([Style::focusable]) while the window is key; `None` leaves
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

  /// A modifier key going down (`true`) or up while `id` has focus ([Event::Modifier]): a
  /// shortcut recorder taking one alone. Seen once; reading it uses it.
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

  // MARK: State

  /// The `T` kept for `id` between frames, created with its default. It lives while `id` is
  /// built or its state used each frame, or an ancestor is retained ([Ui::retain]); see
  /// [store] for the rule.
  pub fn state<T: Default + 'static>(&mut self, id: Id) -> &mut T {
    let UiState { store, stack, tree, .. } = &mut self.st;
    store.get(id, stack.iter().map(|&(i, _)| tree.nodes[i as usize].id))
  }

  /// Keeps this frame the state of `id` and of every element that was inside it, though they
  /// are not built (a hidden tab).
  pub fn retain(&mut self, id: Id) {
    self.st.store.retain(id);
  }

  /// A value moving toward `target` by `tr` (Sublime's `target` + `speed`), kept per `id` (use
  /// `id.key("name")` for several on one element). Frames follow while it moves.
  pub fn animate(&mut self, id: Id, target: f32, tr: Transition) -> f32 {
    let now = self.now;
    let (v, moving) = self.state::<Anim>(id).step(target, tr, now);
    self.st.animating |= moving;
    v
  }

  /// Asks for a frame `dt` from now (a timer); the app checks [Ui::now] when it comes.
  pub fn request_frame_in(&mut self, dt: Duration) {
    self.st.request_frame_at(self.now + dt);
  }

  // MARK: Scrolling

  /// A scroll container's offset, content and viewport as of the last frame and the input
  /// since: what a scrollbar draws.
  pub fn scroll(&self, id: Id) -> Option<Scroll> {
    self.st.scroll(id)
  }

  /// Scrolls `id` to `offset`, clamped by the layout; stops wheel momentum.
  pub fn scroll_to(&mut self, id: Id, offset: Point) {
    let s = self.state::<ScrollState>(id);
    (s.offset, s.pending) = (offset, Point::default());
  }

  /// Scrolls `id` the least that shows `r` (in its content: (0, 0) is its top-left corner at
  /// offset 0), using the last frame's viewport: keeps a caret or a selected row visible.
  pub fn reveal(&mut self, id: Id, r: Rect) {
    let Some(sc) = self.scroll(id) else { return };
    self.scroll_to(id, reveal_offset(sc.offset, sc.viewport, r));
  }

  // MARK: Frame

  /// Lays the tree out for the window, rects snapped to pixels at `scale`, with scroll
  /// offsets from the store (clamped to the new content, and kept so).
  pub fn layout(&mut self, m: &mut dyn Measure, scale: f32) {
    let UiState { tree, store, .. } = &mut self.st;
    for &i in &tree.scrolls {
      let n = &mut tree.nodes[i as usize];
      n.scroll = store.peek::<ScrollState>(n.id).map_or(Point::default(), |s| s.offset);
    }

    let fit_h = layout::layout(tree, self.size, scale, m);
    for &i in &tree.scrolls {
      let n = &tree.nodes[i as usize];
      let (s, max) = (store.raw::<ScrollState>(n.id), input::max_scroll(n));
      // Wheel momentum still to run stops at the new ends too.
      let to = |off: f32, pend: f32, max: f32| (off + pend).clamp(0.0, max) - off;
      let (x, y) = (to(n.scroll.x, s.pending.x, max[0]), to(n.scroll.y, s.pending.y, max[1]));
      (s.offset, s.pending) = (n.scroll, Point::new(x, y));
    }

    if let Some([w, lo, hi]) = self.fit {
      self.st.ask_size(Size::new(w, fit_h.ceil().clamp(lo, hi.max(lo))), self.size);
    }
  }

  /// Whether content built only in part ([Tree::partial]) doesn't cover what the layout just
  /// showed of it (its viewport grew, its offset was clamped), or the layout moved what the view
  /// drew from ([Ui::settle]): the view should run again before this frame is painted.
  pub fn partial_short(&self) -> bool {
    let t = &self.st.tree;
    let moved = t.settle.iter().any(|&(id, seen)| t.get(id).map(|n| (n.rect, n.content)) != seen);
    moved
      || t.partial.iter().any(|&(i, lo, hi)| {
        let n = &t.nodes[i as usize];
        lo > n.scroll.y + 0.5 || hi < n.scroll.y + n.rect.h - 0.5
      })
  }

  /// Hit tests the pointer on the tree just laid out; returns whether the hover changed, so
  /// the view should run again before this frame is painted.
  pub fn rehover(&mut self) -> bool {
    self.st.hover_at(self.mouse, true)
  }

  /// Whether the tree just laid out differs from the last frame's in what a view reads from
  /// it ([Ui::rect], hover): an overlay opened or closed (a menu, a dialog, a tooltip) or the
  /// window changed size. What the view drew from the last frame is then a frame behind, so
  /// the runtime builds one more.
  pub fn shifted(&self) -> bool {
    let (now, last) = (&self.st.tree, &self.st.last);
    if last.nodes.is_empty() {
      return false;
    }

    let same = |(&a, &b): (&u32, &u32)| now.nodes[a as usize].id == last.nodes[b as usize].id;
    let floats =
      now.floats.len() == last.floats.len() && now.floats.iter().zip(&last.floats).all(same);
    now.nodes[0].rect != last.nodes[0].rect || !floats
  }

  /// Paints the laid-out tree, then keeps it as the last frame's.
  pub fn paint(mut self, f: &mut Frame) -> UiState {
    let st = &self.st;
    let ring = st.ring.filter(|_| st.window_focused()).and_then(|r| {
      let i = st.tree.find(st.input.focus?)?;
      // Boxes get it, not selectable text.
      let n = &st.tree.nodes[i as usize];
      let boxed = !matches!(n.kind, Kind::Text { .. });
      let ringed = !matches!(n.style.focus, Focus::None | Focus::Quiet | Focus::TabQuiet);
      (ringed && boxed).then_some((i as usize, r))
    });
    paint::paint(&self.st.tree, &mut self.canvases, ring, f);
    self.finish()
  }

  /// Ends the frame without painting; the tree becomes the last frame's and the store drops
  /// what the frame did not use. Focus on an element the frame did not build clears.
  pub fn finish(mut self) -> UiState {
    self.st.input.ended = None;
    std::mem::swap(&mut self.st.tree, &mut self.st.last);
    self.st.store.gc(&self.st.last);
    if self.st.input.focus.is_some_and(|f| self.st.last.find(f).is_none()) {
      self.st.input.focus = None;
    }

    self.st
  }

  /// Drops this tree, keeping the last frame's: a pass that only delivers input to the view,
  /// or one run again.
  pub fn discard(mut self) -> UiState {
    self.st.input.ended = None;
    self.st
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// What a container being built (a menu, here) tells the elements inside it, nested.
  #[test]
  fn slots_hold_build_state_by_type() {
    struct Menu(u32);

    let mut ui = Ui::new(UiState::default(), Size::new(10.0, 10.0), None, Duration::ZERO);
    assert!(ui.slot::<Menu>().is_none());
    assert!(ui.set_slot(Some(Menu(1))).is_none());

    // A nested one keeps the outer and puts it back when it ends.
    let outer = ui.set_slot(Some(Menu(2)));
    ui.slot::<Menu>().unwrap().0 += 1;
    assert_eq!(ui.slot::<Menu>().map(|m| m.0), Some(3));
    ui.set_slot(outer);
    assert_eq!(ui.slot::<Menu>().map(|m| m.0), Some(1));
    ui.set_slot::<Menu>(None);
    assert!(ui.slot::<Menu>().is_none());
  }

  /// A delivered event is read as its own kind only: asked as another, it stays unused and
  /// unseen; read, it is both.
  #[test]
  fn events_are_taken_by_their_kind() {
    let id = Id::of("field");
    let mut st = UiState::default();
    st.deliver(id, Event::Text("a".into()));
    let ui = Ui::new(st, Size::new(10.0, 10.0), None, Duration::ZERO);
    assert_eq!((ui.command(id), ui.key_down(id), ui.modifier_key(id)), (None, None, None));
    assert_eq!((ui.ime(id), ui.typed(Id::of("other"))), (None, None));
    let mut st = ui.finish();
    assert_eq!(st.delivered(), (false, false));

    st.deliver(id, Event::Text("a".into()));
    let ui = Ui::new(st, Size::new(10.0, 10.0), None, Duration::ZERO);
    assert_eq!(ui.typed(id), Some("a"));
    assert_eq!(ui.finish().delivered(), (true, true));
  }

  #[test]
  fn integer_keys_hash_alike_whatever_their_type() {
    let list = Id::of("list");
    assert_eq!(list.key(3), list.key(3usize));
    assert_eq!(list.key(3u8), list.key(3u64));
    assert_eq!(list.key(-1i32), list.key(-1i64));
    assert_eq!(list.key(("row", 5u32)), list.key(("row", 5u64)));
    assert_ne!(list.key(3), list.key(4));
    assert_ne!(list.key("a"), list.key("b"));
    assert_ne!(list.key(3), list.nth(3));
    assert_ne!(Id::of("a"), Id::ROOT);
  }
}
