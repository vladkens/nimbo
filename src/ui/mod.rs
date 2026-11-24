// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The element tree an app builds each frame it redraws ([crate::App::view], which says how
//! often that is and what a view may change), its flex layout
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
//! Widgets ([widgets]: label, button, text field, check box, switch, slider and the
//! like) and containers ([containers]: list, menu, picker, tabs, split; the scroll area they
//! scroll in is a behaviour's, [Ui::scroll_area]) do this for an app in the look of the theme
//! the app sets (`Ui::set_theme`):
//! `if ui.id("plus").button("+").clicked { .. }`; so do a settings form ([form]) and a modal
//! dialog ([dialog]).
//! What they read of where they are built, the theme, their size, whether they are enabled, is
//! the environment: values by type a container puts for what is built in it
//! ([Ui::environment]: `ui.disabled(true).col(..)`), the window for all ([Ui::set_environment]).
//!
//! The rule for every widget: a headless behaviour ([behavior]: `ui.button_behavior(id)` and
//! the like, hover, press, click, focus, keys and the value edited) plus drawing from the
//! theme. An app that wants its own look keeps the behaviour and draws the element itself.
//! Behaviours also record what their element is to assistive technology ([a11y]), so a look of
//! the app's own reads as the theme's does.

pub mod a11y;
pub mod behavior;
pub mod containers;
pub mod dialog;
mod environment;
pub mod form;
pub mod icon;
mod id;
mod image;
pub mod input;
mod layout;
mod lazy;
mod menus;
mod paint;
mod query;
pub mod store;
mod style;
mod textlayout;
pub mod theme;
pub mod widgets;

use std::any::Any;
use std::fmt::{Display, Write};
use std::hash::Hash;
use std::num::NonZeroU8;
use std::time::Duration;

pub use a11y::{A11yNode, A11yTree, Action, LabeledPairRole, Props, Role, State, Value};
pub use behavior::{
  Behavior, Button, ButtonConfiguration, ButtonRole, ButtonStyle, Content, DialogBehavior,
  EditMenu, Editing, FieldBehavior, FieldKind, FieldStyle, ListBehavior, ListRows, MenuPlacement,
  PlainButtonStyle, PlainSliderStyle, PlainToggleStyle, ScrollBar, ScrollBehavior, ScrollerKnob,
  SectionHeader, Selection, Slider, SliderConfiguration, SliderStyle, SplitBehavior, TextInput,
  ToggleConfiguration, ToggleStyle, TypeSelect,
};
pub use containers::{
  List, ListResponse, ListStyle, Picker, Row, Split, Tabs, Tag, ToolbarItemPlacement,
};
pub use dialog::{Answer, Dialog};
pub use environment::{Enabled, EnvironmentValue};
pub use form::{FormRow, FormStyle, Glass, Section, TabItem};
pub use icon::{Icon, IconPaint};
pub use id::Id;
pub use image::ImageSource;
pub(crate) use input::drags_window;
pub use input::{Click, Press, Scroll};
pub use layout::ContentMode;
pub(crate) use layout::{Flow, Kind, Measure, Node, Sys, Tree};
pub use menus::NativeMenu;
pub use store::{Ease, Transition};
pub use style::{
  Align, Axes, Axis, Clip, ControlSize, CoordinateSpace, Cursor, Dim, Float, Focus, FocusRing,
  LinePattern, Shadow, Style, TextLines, TextStyle, TruncationMode, UnitPoint, grow, pct, px,
  rigid,
};
pub use textlayout::Edit;
pub(crate) use textlayout::TextLayout;
pub use theme::{ControlBorder, Sizes, SwitchSize, Theme, Tone};
pub use widgets::{
  BorderedButtonStyle, BorderedProminentButtonStyle, BorderlessButtonStyle, ButtonToggleStyle,
  CheckboxToggleStyle, LinearSliderStyle, SwitchToggleStyle,
};

use self::input::{Deliver, Input, ScrollState};
use self::store::{Anim, Store};
use crate::app::{Appearance, Clipboard, ColorScheme, Event, MouseButton, Waker};
use crate::keys::{Binding, Os};
use crate::scene::{Frame, ImageId, Images, Point, Rect, Size};
use crate::text::{Font, Metrics, TextSystem};
use crate::{FileDialog, TitleBar, WindowToolbarStyle};

/// Which way focus moves among the focusable elements ([Ui::focus_next]): Tab and Shift-Tab,
/// as SwiftUI's `FocusDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusDirection {
  Next,
  Previous,
}

/// How long [UiState::flash] shows an element pressed.
const FLASH: Duration = Duration::from_millis(120);

// MARK: Builder

/// Paints a canvas element: gets the frame and the element's laid-out rect.
pub(crate) type Canvas<'a> = Box<dyn FnOnce(&mut Frame, Rect) + 'a>;

/// What a [Ui] keeps between frames: the tree being built and the last one laid out, pointer
/// state, the per-id store and timers, so a steady frame allocates nothing.
pub(crate) struct UiState {
  tree: Tree,
  /// The last frame's tree, laid out: ids and rects for hit testing.
  pub last: Tree,
  /// Text measures the layout keeps across frames.
  kept: layout::Kept,
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
  /// Values the whole window shares, one per type ([Ui::set_environment]).
  env: Vec<Box<dyn Any>>,
  /// The shell's defaults under the window's values, one per type
  /// ([UiState::set_environment_default]).
  env_defaults: Vec<Box<dyn Any>>,
  /// Values scoped to the elements being built ([Ui::environment]).
  scoped: environment::Scoped,
  /// The focus ring the view set this frame ([Ui::set_focus_ring]), over the default, which the
  /// shell puts after each view ([Ui::set_default_focus_ring]).
  own_ring: Option<Option<FocusRing>>,
  ring: Option<FocusRing>,
  /// The focus and pointer the app set on controls whose behaviours have read them, by the id of
  /// the root each is for, until it is built ([Ui::take_control_modifiers]); cleared each frame.
  control_mods: Vec<(Id, Option<bool>, Option<Cursor>)>,
  /// The OS's look ([Ui::appearance]).
  appearance: Appearance,
  /// What the last frame cost ([Ui::last_frame]).
  pub(crate) stats: FrameStats,
  /// The content size the app asked the window for ([Ui::set_window_size]), until the platform
  /// takes it ([crate::Runtime::take_resize]).
  pub(crate) resize: Option<Size>,
  /// The last size asked: asking it again is nothing new, so a view may ask every frame.
  asked: Option<Size>,
  /// The file panel asked for ([Ui::file_dialog]), until the platform takes it.
  pub(crate) file_dialog: Option<crate::FileDialog>,
  /// The platform shows context menus ([crate::Runtime::set_native_menus]); the one asked for
  /// ([Ui::native_menu]) until it takes it; the press that opened one last.
  pub(crate) native_menus: bool,
  pub(crate) context_menu: Option<(Id, Point, crate::Menu<usize>)>,
  pub(crate) menu_seq: u32,
  /// The title bar over the content ([Ui::titlebar]).
  pub(crate) titlebar: Option<TitleBar>,
  /// The view asked for a toolbar's title bar ([Ui::window_toolbar_style]): the platform gives
  /// the title bar a toolbar's height.
  pub(crate) toolbar: bool,
  /// The images [Ui::image] and [Ui::rgba_image] added: their pixels' hash or a bitmap's
  /// identity, the image, and whether a view built it since the last frame.
  pub(crate) pictures: Vec<(u64, ImageId, bool)>,
  /// The element that takes key presses before any binding ([Ui::grab_keys]), as the view
  /// built last asked.
  pub(crate) grab: Option<Id>,
  /// The element ringed in place of the focused one, as the view built last asked: a field
  /// whose edit menu has the focus.
  pub(crate) ring_on: Option<Id>,
  /// Whose keys bindings and text fields follow: the platform's, or the one a test plays.
  pub(crate) os: Os,
}

/// What the runtime's last frame cost, for a status bar or a frame meter ([Ui::last_frame]).
/// More figures may come in a minor release, so it is read, never built.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
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
  /// The dark appearance until the platform (or the harness, `--appearance`) reports the OS's.
  fn default() -> Self {
    Self {
      tree: Tree::default(),
      last: Tree::default(),
      kept: layout::Kept::default(),
      stack: Vec::new(),
      input: Input::default(),
      store: Store::default(),
      due: None,
      after: None,
      animating: false,
      waker: Waker::default(),
      clipboard: Box::default(),
      env: Vec::new(),
      env_defaults: Vec::new(),
      scoped: environment::Scoped::default(),
      own_ring: None,
      ring: None,
      control_mods: Vec::new(),
      appearance: Appearance::DARK,
      stats: FrameStats::default(),
      resize: None,
      asked: None,
      file_dialog: None,
      native_menus: false,
      context_menu: None,
      menu_seq: 0,
      titlebar: None,
      toolbar: false,
      pictures: Vec::new(),
      grab: None,
      ring_on: None,
      os: Os::CURRENT,
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

  /// Takes the OS's new look, which the theme follows unless the app set one (`Ui::theme`).
  pub(crate) fn set_appearance(&mut self, a: Appearance) {
    self.appearance = a;
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
  pub fn focus_next(&mut self, dir: FocusDirection) -> bool {
    let t = &self.last;
    let all = || t.nodes.iter().filter(|n| n.style.focus != Focus::None).map(|n| n.id);
    let n = all().count();
    if n == 0 {
      return false;
    }

    let at = self.input.focus.and_then(|f| all().position(|id| id == f));
    let next = match (at, dir) {
      (Some(i), FocusDirection::Next) => (i + 1) % n,
      (Some(i), FocusDirection::Previous) => (i + n - 1) % n,
      (None, FocusDirection::Next) => 0,
      (None, FocusDirection::Previous) => n - 1,
    };

    self.input.focus = all().nth(next);
    if let Some(id) = self.input.focus {
      self.reveal_in_scrolls(id);
    }

    true
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
/// callbacks borrow the app's data: `fn view<'a>(&'a mut self, ui: &mut Ui<'a>)`. The view
/// may run several times for a frame, or not at all: it changes the app's state only in answer
/// to what a pass reports ([crate::App::view]).
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
  /// A control's id whose root is yet to be built, inside what its style builds around it: an
  /// element there never takes it by its place.
  pending_root: Option<Id>,
  next_float: Option<Float>,
  next_keymap: Option<&'static [Binding]>,
  /// The next element's tooltip ([Ui::help]), and the elements with one built, by node.
  next_help: Option<String>,
  pub(crate) helps: Vec<(u32, String)>,
  /// The next element takes focus when it first shows ([Ui::autofocus]).
  next_autofocus: bool,
  /// The next element moves the window when pressed ([Ui::drags_window]).
  next_drag: bool,
  /// The next element takes no pointer input of its own ([Ui::allows_hit_testing]).
  next_no_hit: bool,
  /// The next element's focus and pointer, over its style's ([Ui::focusable],
  /// [Ui::pointer_style]).
  next_focus: Option<bool>,
  next_cursor: Option<Cursor>,
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
  pub(crate) fn new(mut st: UiState, size: Size, mouse: Option<Point>, now: Duration) -> Self {
    st.tree.clear(size);
    st.stack.clear();
    st.stack.push((0, 0));
    st.control_mods.clear();
    st.animating = false;
    st.input.moves = false;
    (st.grab, st.ring_on, st.toolbar, st.own_ring) = (None, None, false, None);
    st.input.now = now;
    st.scoped.clear();
    let (next_id, next_float, next_keymap) = (None, None, None);
    let canvases = Vec::new();
    Self {
      st,
      canvases,
      next_id,
      pending_root: None,
      next_float,
      next_keymap,
      next_help: None,
      helps: Vec::new(),
      next_autofocus: false,
      next_drag: false,
      next_no_hit: false,
      next_focus: None,
      next_cursor: None,
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

  /// The OS's look, for an app drawing in colours of its own: light or dark and the accent.
  pub fn appearance(&self) -> Appearance {
    self.st.appearance
  }

  /// Light or dark as the content is drawn, as SwiftUI's `colorScheme`: the environment's (a
  /// theme the app set puts its own), else the OS's.
  pub fn color_scheme(&self) -> ColorScheme {
    self.environment_value()
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

  /// The title bar the content runs under, for a window that draws its own
  /// ([crate::Config::unified]): its height, the window's title and where the window's buttons
  /// are; `None` for any other.
  pub fn titlebar(&self) -> Option<TitleBar> {
    self.st.titlebar
  }

  /// How the title bar shows a toolbar this frame ([WindowToolbarStyle]), as SwiftUI's
  /// `.windowToolbarStyle`: [WindowToolbarStyle::Unified] gives it a toolbar's height for a band
  /// the view draws at the window's top (nimbo's `ui.toolbar` asks it itself). Only a window whose
  /// content runs under its title bar ([crate::Config::unified]) has one to change. Ask it each
  /// frame; [WindowToolbarStyle::Automatic] takes back nothing asked in the frame.
  pub fn window_toolbar_style(&mut self, style: WindowToolbarStyle) {
    self.st.toolbar |= style == WindowToolbarStyle::Unified;
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
  #[cfg(test)]
  pub(crate) fn tree(&self) -> &Tree {
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

  /// The size of `s` in `style` wrapped at `width`, as the layout breaks and lays it out: its
  /// widest line, to fit a box to its wrapped text (a tooltip), as the layout sizes one by the
  /// text's width before it wraps; and its lines' height, for items of known heights
  /// ([Ui::virtual_col]) that are paragraphs. Zero in a [Ui] the runtime didn't build.
  pub fn measure_wrapped(&mut self, s: &str, style: TextStyle, width: f32) -> Size {
    let Some(text) = self.text.as_mut() else { return Size::default() };
    let mut m = Sys { text, images: &self.images, scale: 1.0 };
    layout::wrapped_size(s, style, width, &mut m, &mut self.st.kept)
  }

  /// The distance between baselines of text in `style`, as the layout lays it out: its own
  /// ([TextStyle::line_h]) or its font's. 0 in a [Ui] the runtime didn't build.
  pub fn line_height(&mut self, style: TextStyle) -> f32 {
    style.line_h.unwrap_or_else(|| self.metrics(style.font).line_height())
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

  /// Whether the next element takes pointer input of its own, as SwiftUI's `.allowsHitTesting`:
  /// `false` leaves it to what is under it, named or not, so a part a widget names off its own
  /// id (`ui.with_id(id.key("box")).allows_hit_testing(false)`) has an identity, a rect and a
  /// record for assistive technology but no hover, press or click, and a click on it is its
  /// parent's. Unlike SwiftUI's, the elements inside it keep theirs, and a scroll container, a
  /// focusable element and one with [Ui::help] keep the pointer, which they need to work (a
  /// badge with a tip shows it). `true`, the default, changes nothing.
  pub fn allows_hit_testing(&mut self, yes: bool) -> &mut Self {
    self.next_no_hit = !yes;
    self
  }

  /// Whether the next element takes focus, as SwiftUI's `.focusable`: `true` by Tab, as AppKit's
  /// controls take it, unless its style asks for focus of its own kind ([Style::focusable] and
  /// the like); `false` none, whatever its style asks, so Tab passes it by.
  pub fn focusable(&mut self, yes: bool) -> &mut Self {
    self.next_focus = Some(yes);
    self
  }

  /// The pointer's shape over the next element, as SwiftUI's `.pointerStyle`, whatever its style
  /// says ([Style::cursor]): [Cursor::Pointer], the pointing hand, over a control;
  /// [Cursor::Inherit] the parent's.
  pub fn pointer_style(&mut self, cursor: Cursor) -> &mut Self {
    self.next_cursor = Some(cursor);
    self
  }

  /// Takes [Ui::focusable] and [Ui::pointer_style] set for the next element off it for control
  /// `id`'s root, the element built under `id`, for a behaviour to read before the widget builds
  /// anything around its root; what is set after still wins. Disabled (`off`), the root takes no
  /// focus and the parent's pointer, whatever its style asks. Whether the control takes focus:
  /// `false` when the app said not.
  fn take_control_modifiers(&mut self, id: Id, off: bool) -> bool {
    let (focus, cursor) = (self.next_focus.take(), self.next_cursor.take());
    self.take_kept_modifiers(id);
    if off || focus.is_some() || cursor.is_some() {
      let kept = if off { (Some(false), Some(Cursor::Inherit)) } else { (focus, cursor) };
      self.st.control_mods.push((id, kept.0, kept.1));
    }

    focus != Some(false)
  }

  /// The focus and pointer kept for control `id`'s root ([Ui::take_control_modifiers]), taken.
  fn take_kept_modifiers(&mut self, id: Id) -> (Option<bool>, Option<Cursor>) {
    let mods = &mut self.st.control_mods;
    match mods.iter().position(|m| m.0 == id) {
      Some(i) => {
        let (_, focus, cursor) = mods.swap_remove(i);
        (focus, cursor)
      }
      None => (None, None),
    }
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

  /// The next element shows `tip` under the pointer resting on it, a tooltip as macOS places one
  /// (its left edge at the pointer, 19 below it, after `Theme::tip_delay`, at once within
  /// `Theme::tip_warm` of another), and reads it as its description to assistive technology, as
  /// SwiftUI's `.help`: any element, a text (read where it stands), a widget, a container or a
  /// canvas (read as a group, its children as ever). It takes the hover for the tip alone: a
  /// primary press on it is the interactive element's around it (a button whose label has a
  /// tip), a secondary one opens its own context menu if it has one. The tip is `id.key("tip")`
  /// of the element's id.
  pub fn help(&mut self, tip: &str) -> &mut Self {
    self.next_help = Some(tip.to_owned());
    self
  }

  /// The tip [Ui::help] gave the next element, taken for a widget to put on a part of its own
  /// with [Ui::help]: a mark in a box, a button built after others.
  pub fn take_help(&mut self) -> Option<String> {
    self.next_help.take()
  }

  /// The tooltips this frame asked for with [Ui::help] so far: each element built with one and
  /// its tip, to show the one the pointer rests on in a design system's own look, at the end of
  /// the view. Taken: the shell shows those left after the view (nimbo's styled tooltip), so
  /// a view that takes them draws them all. Each tip is its element's description for assistive
  /// technology, set here, so it is read however it is drawn; one with no record gets one to
  /// carry it: in a control (a button's label), the control's, unless it has one; else a text
  /// as itself, any other a group, whose children are read as ever.
  pub fn take_tooltips(&mut self) -> Vec<(Id, String)> {
    let mut out = Vec::with_capacity(self.helps.len());
    for (i, tip) in std::mem::take(&mut self.helps) {
      let n = &self.st.tree.nodes[i as usize];
      let (id, text) = match n.kind {
        Kind::Text { range, .. } => (n.id, Some(self.st.tree.str(range.0, range.1).to_owned())),
        _ => (n.id, None),
      };

      // A node of its own in a control would take its hits from assistive technology.
      if self.a11y_of(id).is_none()
        && let Some(c) = self.a11y_control_around(i)
      {
        _ = c.description_or(&tip);
        out.push((id, tip));
        continue;
      }

      match (self.a11y_of(id), text) {
        (Some(a), _) => _ = a.description(&tip),
        (None, Some(text)) => {
          _ = self.a11y_label(&text).a11y(id, Role::StaticText).description(&tip)
        }
        (None, None) => _ = self.a11y(id, Role::Group).description(&tip),
      }

      out.push((id, tip));
    }

    out
  }

  /// Disables the next widget ([widgets]) when `yes`: dimmed, not focusable, and its clicks
  /// do nothing; on a container, every widget built in it, as SwiftUI's `.disabled`. Puts
  /// [Enabled] `false`, so it only disables: `ui.environment(Enabled(true))` enables a part.
  pub fn disabled(&mut self, yes: bool) -> &mut Self {
    if yes {
      self.environment(Enabled(false));
    }

    self
  }

  /// Sizes the next widget, or every widget built in the next container, as SwiftUI's
  /// `.controlSize`: a button's height and font, a switch's track, a badge's pill, a spinner's
  /// size; the environment's [ControlSize], [ControlSize::Regular] outside one.
  pub fn control_size(&mut self, size: ControlSize) -> &mut Self {
    self.environment(size)
  }

  /// Focuses the next widget, or the next element if it is focusable, when it first shows:
  /// when the last frame did not have it. A field that opens for an edit, the first field of a
  /// form; the view sees the focus at once.
  pub fn autofocus(&mut self) -> &mut Self {
    self.next_autofocus = true;
    self
  }

  /// Makes the next element move the window when pressed: the header of a window that draws
  /// its own title bar ([crate::Config::unified]). It is a layer over the content, as AppKit's
  /// title bar is: where it shows, the pointer reaches only what is built inside it and the
  /// overlays over it (an open menu); the content under it gets no hover, cursor or press there,
  /// and a press moves the window. A double-click zooms or minimizes the window as the OS's
  /// setting says; the app never sees the press. The wheel scrolls the content under it.
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
  /// frame did not have it, unless it is disabled ([Ui::disabled]) or not focusable
  /// ([Ui::focusable]).
  pub fn take_autofocus(&mut self, id: Id) {
    let off = !self.environment_value::<Enabled>().0 || self.next_focus == Some(false);
    if std::mem::take(&mut self.next_autofocus) && !off && self.st.last.find(id).is_none() {
      self.st.input.focus = Some(id);
    }
  }

  // MARK: Elements

  /// A container laying its children out left to right.
  pub fn row<R>(&mut self, style: Style, f: impl FnOnce(&mut Self) -> R) -> Response {
    self.container(Flow::Row, style, f)
  }

  /// A container laying its children out top to bottom.
  pub fn col<R>(&mut self, style: Style, f: impl FnOnce(&mut Self) -> R) -> Response {
    self.container(Flow::Column, style, f)
  }

  /// A container laying its children out in `cols` equal columns, left to right and wrapping
  /// to the next row, `style.gap` apart both ways: a keypad, a form, a gallery. A child takes
  /// one cell, or [Ui::span] columns; growing children fill their cell, others sit in it by
  /// `style.align`. Rows are as high as their highest child and share the height left over
  /// when the grid is taller.
  pub fn grid<R>(&mut self, cols: u8, style: Style, f: impl FnOnce(&mut Self) -> R) -> Response {
    let cols = NonZeroU8::new(cols).unwrap_or(NonZeroU8::MIN);
    let (i, r) = self.open(Flow::Grid(cols), style);
    f(self);
    self.close(i);
    r
  }

  /// A box with no children: a divider, a swatch, a fixed gap.
  pub fn el(&mut self, style: Style) -> Response {
    self.push(Kind::Box, style, Flow::Row).1
  }

  /// Takes the free space along the parent's axis.
  pub fn spacer(&mut self) -> Response {
    self.el(Style::new().grow())
  }

  /// The axis the container being built, the next element's parent, lays its children along:
  /// horizontal in a row, vertical in a column (the window's root is one), as SwiftUI's
  /// `Divider` follows its stack; `None` in a grid, which lays them along both.
  pub fn parent_axis(&self) -> Option<Axis> {
    let parent = self.st.stack.last().map_or(0, |s| s.0);
    match self.st.tree.nodes[parent as usize].flow {
      Flow::Row => Some(Axis::Horizontal),
      Flow::Column => Some(Axis::Vertical),
      Flow::Grid(_) => None,
    }
  }

  /// How many children the container being built has so far, the next element's place among
  /// them: 0 before its first (a section's first row, with no line above it).
  pub fn child_count(&self) -> usize {
    self.st.stack.last().map_or(0, |s| s.1 as usize)
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
    self.push(kind, Style::new(), Flow::Row);
  }

  /// A box whose `paint` draws into the frame within its laid-out rect (a plot). It draws in
  /// its parent's layer: start one with `f.layer()` for order across primitive kinds.
  pub fn canvas(&mut self, style: Style, paint: impl FnOnce(&mut Frame, Rect) + 'a) -> Response {
    self.canvases.push(Some(Box::new(paint)));
    self.push(Kind::Canvas(self.canvases.len() as u32 - 1), style, Flow::Row).1
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
    self.canvas_container(Flow::Row, style, Box::new(paint), f)
  }

  /// [Ui::canvas_row] with the children top to bottom.
  pub fn canvas_col<R>(
    &mut self,
    style: Style,
    paint: impl FnOnce(&mut Frame, Rect) + 'a,
    f: impl FnOnce(&mut Self) -> R,
  ) -> Response {
    self.canvas_container(Flow::Column, style, Box::new(paint), f)
  }

  fn canvas_container<R>(
    &mut self,
    flow: Flow,
    style: Style,
    paint: Canvas<'a>,
    f: impl FnOnce(&mut Self) -> R,
  ) -> Response {
    self.canvases.push(Some(paint));
    let (i, r) = self.node(Kind::Canvas(self.canvases.len() as u32 - 1), style, flow);
    self.st.stack.push((i, 0));
    self.st.scoped.open(i);
    f(self);
    self.close(i);
    r
  }

  fn container<R>(&mut self, flow: Flow, style: Style, f: impl FnOnce(&mut Self) -> R) -> Response {
    let (i, r) = self.open(flow, style);
    f(self);
    self.close(i);
    r
  }

  /// Starts a container; out of line, so each `row` / `col` call site stays small.
  #[inline(never)]
  fn open(&mut self, flow: Flow, style: Style) -> (u32, Response) {
    let (i, r) = self.node(Kind::Box, style, flow);
    self.st.stack.push((i, 0));
    self.st.scoped.open(i);
    (i, r)
  }

  fn close(&mut self, i: u32) {
    self.st.scoped.close(i);
    self.st.stack.pop();
    let tree = &mut self.st.tree;
    tree.nodes[i as usize].end = tree.nodes.len() as u32;
  }

  /// Adds an element with no children: the values put for it ([Ui::environment]) go with it.
  #[inline(always)]
  fn push(&mut self, kind: Kind, style: Style, flow: Flow) -> (u32, Response) {
    let (i, r) = self.node(kind, style, flow);
    self.st.scoped.close(i);
    (i, r)
  }

  #[inline(always)]
  fn node(&mut self, kind: Kind, mut style: Style, flow: Flow) -> (u32, Response) {
    let (parent, count) = self.st.stack.last_mut().unwrap();
    let tree = &mut self.st.tree;
    let (id, named) = self
      .next_id
      .take()
      .unwrap_or_else(|| (tree.nodes[*parent as usize].id.nth(*count as usize), false));
    let id = match self.pending_root {
      Some(r) if r == id && !named => id.key("around"),
      Some(r) if r == id => {
        self.pending_root = None;
        id
      }
      _ => id,
    };

    *count += 1;
    let parent = *parent;
    // The modifiers over its style, before what they decide: focus, the pointer, hit testing; a
    // control's root those its behaviour took where none are set since.
    let (mut focus, mut cursor) = (self.next_focus.take(), self.next_cursor.take());
    if !self.st.control_mods.is_empty() {
      let (f, c) = self.take_kept_modifiers(id);
      (focus, cursor) = (focus.or(f), cursor.or(c));
    }

    match focus {
      // Made not focusable while it has the focus, it gives the focus up.
      Some(false) if self.st.input.focus == Some(id) => {
        style.focus = Focus::None;
        self.st.set_focus(None);
      }
      Some(false) => style.focus = Focus::None,
      Some(true) if style.focus == Focus::None => style.focus = Focus::Tab,
      _ => {}
    }

    if let Some(cursor) = cursor {
      style.cursor = cursor;
    }

    let scrolls = !style.scroll.is_empty();
    let focus = style.focus != Focus::None;
    if self.next_autofocus {
      // Only a focusable element takes it; the flag goes with this one either way.
      match focus {
        true => self.take_autofocus(id),
        false => self.next_autofocus = false,
      }
    }

    let (drag, help) = (std::mem::take(&mut self.next_drag), self.next_help.take());
    // Taken whatever the element is: the modifier is the next element's only. What needs the
    // pointer to work, a scroll container, a focusable element, a tip, takes it anyway.
    let off = std::mem::take(&mut self.next_no_hit);
    let hit = style.hit || named || style.cursor != Cursor::Inherit;
    let own = (hit && !off) || scrolls || focus;
    let hit = own || help.is_some();
    // Its style's `hit` turned off too: no hover as an interactive child's ancestor.
    style.hit = hit;
    let tree = &mut self.st.tree;
    let i = tree.nodes.len() as u32;
    self.helps.extend(help.map(|tip| (i, tip)));
    tree.drags.extend(drag.then_some(i));
    tree.tips.extend((hit && !own).then_some(i));

    let float = self.next_float.take();
    if float.is_some() {
      tree.floats.push(i);
    }

    if let Some(map) = self.next_keymap.take() {
      tree.keymaps.push((i, map));
    }

    tree.nodes.push(Node::new(id, kind, style, flow, float, parent, i + 1));
    if self.next_span != 1 {
      tree.nodes[i as usize].span = std::mem::replace(&mut self.next_span, 1);
    }

    match hit {
      true => (i, self.interactive(i, id, scrolls)),
      false => (i, Response { id, ..Response::default() }),
    }
  }

  /// Indexes an interactive node for hit testing (and a scroll container for the layout) and
  /// reads its input; out of line, so plain elements build fast.
  #[inline(never)]
  fn interactive(&mut self, i: u32, id: Id, scrolls: bool) -> Response {
    self.st.tree.hits.push(i);
    if scrolls {
      self.st.tree.scrolls.push(i);
      // Keeps its offset alive; the layout reads and clamps it.
      self.state::<ScrollState>(id);
    }

    self.st.response(id)
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

  /// Asks for the next frame at the display's refresh, as an animation does while it moves: a
  /// view that changes every frame (a spinner), as SwiftUI's `TimelineView(.animation)`.
  pub fn request_frame(&mut self) {
    self.st.animating = true;
  }

  /// Asks for a frame `dt` from now (a timer); the app checks [Ui::now] when it comes.
  pub fn request_frame_in(&mut self, dt: Duration) {
    self.st.request_frame_at(self.now + dt);
  }

  /// Shows the system's file panel over the window once this frame is built: from the person's
  /// click, never by itself. The answer comes as [crate::Event::FileDialog]. Asked while one is
  /// open, it is answered `None` at once on macOS and Windows and replaces it on Linux, where a
  /// portal that went away never answers.
  pub fn file_dialog(&mut self, dialog: FileDialog) {
    self.st.file_dialog = Some(dialog);
  }

  // MARK: Frame

  /// Lays the tree out for the window, rects snapped to pixels at `scale`, with scroll
  /// offsets from the store (clamped to the new content, and kept so). Texts' least widths and
  /// wrapped lines are kept across frames per font and text: `m` measures the same each frame.
  pub(crate) fn layout(&mut self, m: &mut dyn Measure, scale: f32) {
    let UiState { tree, store, kept, .. } = &mut self.st;
    for &i in &tree.scrolls {
      let n = &mut tree.nodes[i as usize];
      n.scroll = store.peek::<ScrollState>(n.id).map_or([0.0; 2], |s| s.offset);
    }

    let fit_h = layout::layout(tree, self.size, scale, m, kept);
    for &i in &tree.scrolls {
      let n = &tree.nodes[i as usize];
      let (s, max) = (store.raw::<ScrollState>(n.id), input::max_scroll(n));
      // Wheel momentum still to run stops at the new ends too.
      let to = |ax: usize, pend: f32| {
        let off = n.scroll[ax];
        ((off + f64::from(pend)).clamp(0.0, max[ax]) - off) as f32
      };
      let (x, y) = (to(0, s.pending.x), to(1, s.pending.y));
      (s.offset, s.pending) = (n.scroll, Point::new(x, y));
    }

    if let Some([w, lo, hi]) = self.fit {
      self.st.ask_size(Size::new(w, fit_h.ceil().clamp(lo, hi.max(lo))), self.size);
    }
  }

  /// Whether content built only in part ([Tree::partial]) doesn't cover what the layout just
  /// showed of it (its viewport grew, its offset was clamped), or the layout moved what the view
  /// drew from ([Ui::settle]): the view should run again before this frame is painted.
  pub(crate) fn partial_short(&self) -> bool {
    let t = &self.st.tree;
    let moved = t.settle.iter().any(|&(id, seen)| t.get(id).map(|n| (n.rect, n.content)) != seen);
    moved
      || t.partial.iter().any(|&(i, b, lo, hi)| {
        let (n, b) = (&t.nodes[i as usize], &t.nodes[b as usize]);
        // The block's top in the container's content: 0 for the container itself.
        // From the positions, in `f64`: the block may be millions of points tall.
        let top = if std::ptr::eq(n, b) { 0.0 } else { b.pos[1] - n.pos[1] + n.scroll[1] };
        let (off, view) = (n.scroll[1], f64::from(n.rect.h));
        top + lo > off + 0.5 || top + hi < off + view - 0.5
      })
  }

  /// Hit tests the pointer on the tree just laid out; returns whether the hover changed, so
  /// the view should run again before this frame is painted.
  pub(crate) fn rehover(&mut self) -> bool {
    self.st.hover_at(self.mouse, true)
  }

  /// Whether the tree just laid out differs from the last frame's in what a view reads from
  /// it ([Ui::rect], hover): an overlay opened or closed (a menu, a dialog, a tooltip) or the
  /// window changed size. What the view drew from the last frame is then a frame behind, so
  /// the runtime builds one more.
  pub(crate) fn shifted(&self) -> bool {
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
  pub(crate) fn paint(mut self, f: &mut Frame) -> UiState {
    let st = &self.st;
    let ring = st.own_ring.unwrap_or(st.ring).filter(|_| st.window_focused()).and_then(|r| {
      let i = st.tree.find(st.ring_on.or(st.input.focus)?)?;
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
  pub(crate) fn finish(mut self) -> UiState {
    self.st.input.ended = None;
    std::mem::swap(&mut self.st.tree, &mut self.st.last);
    self.st.store.gc(&self.st.last);
    if self.st.input.focus.is_some_and(|f| self.st.last.find(f).is_none()) {
      self.st.input.focus = None;
    }

    self.st
  }

  /// Keeps this tree as the last for a pass run again on its layout ([Ui::partial_short]),
  /// keeping the state and focus of what it left out: the pass after builds them.
  pub(crate) fn relayout(mut self) -> UiState {
    self.st.input.ended = None;
    std::mem::swap(&mut self.st.tree, &mut self.st.last);
    self.st
  }

  /// Drops this tree, keeping the last frame's: a pass that only delivers input to the view,
  /// or one run again.
  pub(crate) fn discard(mut self) -> UiState {
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

  /// What a widget reads of the stack it is built in, and the elements built so far.
  #[test]
  fn stacks_tell_their_axis_and_children() {
    let mut ui = Ui::new(UiState::default(), Size::new(10.0, 10.0), None, Duration::ZERO);
    let at = ui.st.tree.nodes.len();
    ui.row(Style::new(), |ui| {
      assert_eq!((ui.parent_axis(), ui.child_count()), (Some(Axis::Horizontal), 0));
      ui.el(Style::new());
      assert_eq!(ui.child_count(), 1);
      ui.col(Style::new(), |ui| assert_eq!(ui.parent_axis(), Some(Axis::Vertical)));
      ui.grid(2, Style::new(), |ui| assert_eq!(ui.parent_axis(), None));
    });
    assert_eq!(ui.st.tree.nodes.len() - at, 4);
  }

  /// `allows_hit_testing(false)` is the next element's, whatever it is: a plain one built after
  /// it takes input as ever.
  #[test]
  fn no_hit_testing_ends_with_the_next_element() {
    let mut ui = Ui::new(UiState::default(), Size::new(10.0, 10.0), None, Duration::ZERO);
    ui.allows_hit_testing(false).el(Style::new());
    ui.id("b").el(Style::new());
    let hit = |ui: &Ui, i: usize| ui.st.tree.nodes[i].style.hit;
    let n = ui.st.tree.nodes.len();
    assert!(!hit(&ui, n - 2) && hit(&ui, n - 1));
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
}
