// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What reaches an app from the system: input and window events, the appearance, IME
//! composition, and the waker that asks for the next frame.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use crate::geom::{Color, Point};
use crate::keys::{Command, KeyEvent};
#[cfg(feature = "system")]
use crate::system::Change;
use crate::ui::{Action, Id, Role};
use crate::{Code, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MouseButton {
  Left,
  Right,
  Middle,
  Other(u8),
}

/// A gesture phase, as AppKit reports it for trackpad scrolls and their momentum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Phase {
  /// A mouse wheel, or no gesture.
  #[default]
  None,
  MayBegin,
  Began,
  Changed,
  Ended,
  Cancelled,
}

/// Input in view coordinates (points, y down), independent of NSEvent. More kinds may come in a
/// minor release: a `match` on it ends with `_ =>`.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Event {
  /// Pointer moved, with or without a button held (drag).
  MouseMove {
    pos: Point,
    mods: Modifiers,
  },
  MouseDown {
    pos: Point,
    button: MouseButton,
    clicks: u32,
    mods: Modifiers,
  },
  MouseUp {
    pos: Point,
    button: MouseButton,
    mods: Modifiers,
  },
  /// The pointer left the view.
  MouseExit,
  /// `delta` in points, positive y when content should move down (toward the top of the
  /// document). Trackpads set `precise` and report `phase`, then `momentum` after lift-off.
  Scroll {
    pos: Point,
    delta: Point,
    precise: bool,
    phase: Phase,
    momentum: Phase,
    mods: Modifiers,
  },
  KeyDown(KeyEvent),
  KeyUp(KeyEvent),
  /// Text typed: a key no one used, as AppKit's text input turns it into text (`insertText:`,
  /// dead keys composed). Never empty, no control characters.
  Text(String),
  /// A key's command: from a binding, or a key no one used as AppKit's text input turns it into
  /// one (`doCommandBySelector:`: arrows, Backspace, Enter, Ctrl-A).
  Command(Command),
  /// Modifier keys changed.
  Mods(Modifiers),
  /// A modifier key went down or up, by its place ([Code::AltRight], [Code::Fn]): what a
  /// shortcut recorder needs to take one alone (`right_option`). After the [Event::Mods] it
  /// changed; to the focused element first ([crate::Ui::modifier_key]), then the app. Autorepeats
  /// send none.
  Modifier {
    key: Code,
    down: bool,
  },
  /// A trackpad pinch: `delta` is the change in magnification (0.1 is 10% larger).
  Magnify {
    pos: Point,
    delta: f32,
    phase: Phase,
    mods: Modifiers,
  },
  /// The window became or stopped being the key window.
  Focus(bool),
  /// An input method composing or committing text, for the focused text field.
  Ime(Ime),
  /// The OS's light or dark setting or accent colour changed, or is first known (before the
  /// first frame): the theme follows it unless the app set one (`Ui::set_theme`).
  Appearance(Appearance),
  /// A change in the system the app watches ([crate::Main::watch]), to read again
  /// ([crate::system]); redraws when the app uses it.
  #[cfg(feature = "system")]
  System(Change),
  /// Files dragged over the window, the pointer at `pos`; the hover follows it, so
  /// [crate::Cx::hovered] tells where they would go.
  DragOver {
    pos: Point,
    paths: Vec<PathBuf>,
  },
  /// The dragged files left the window, or the drag ended elsewhere.
  DragExit,
  /// Files dropped on the window at `pos`.
  Drop {
    pos: Point,
    paths: Vec<PathBuf>,
  },
  /// The file picked in the panel the window asked for ([crate::Ui::file_dialog]); `None` when
  /// cancelled.
  FileDialog(Option<PathBuf>),
  /// The item chosen in the context menu of `on` the platform showed ([crate::Ui::native_menu]),
  /// by its action's number ([crate::Menu]); `None` when dismissed. Delivered to `on` in a view
  /// pass, then to the app when no view took it.
  Menu {
    on: Id,
    item: Option<usize>,
  },
  /// Assistive technology acting on the element `id` (VoiceOver's press, a slider's increment,
  /// a field's new text): the runtime takes it through the input paths, then the app hears it,
  /// the press as a click ([crate::Cx::clicked]). `role` is the node's, as the client saw it: a
  /// node of `id` with another role refuses it.
  A11y {
    id: Id,
    role: Role,
    action: Action,
  },
}

/// Light or dark, as SwiftUI's `ColorScheme`: the OS's ([Appearance]) or a theme's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColorScheme {
  #[default]
  Light,
  Dark,
}

impl ColorScheme {
  /// [ColorScheme::Dark] when `dark`, else Light: from a platform's flag.
  pub const fn of(dark: bool) -> Self {
    if dark { ColorScheme::Dark } else { ColorScheme::Light }
  }

  /// The other one.
  pub const fn other(self) -> Self {
    Self::of(!self.is_dark())
  }

  pub const fn is_dark(self) -> bool {
    matches!(self, ColorScheme::Dark)
  }
}

/// `light` or `dark`.
impl std::str::FromStr for ColorScheme {
  type Err = String;

  fn from_str(s: &str) -> Result<Self, String> {
    match s {
      "light" => Ok(ColorScheme::Light),
      "dark" => Ok(ColorScheme::Dark),
      _ => Err(format!("{s:?} is not light or dark")),
    }
  }
}

/// The OS's look: its [ColorScheme], and the accent colour the user picked, if the OS has one
/// ([crate::platform::appearance]).
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Appearance {
  pub scheme: ColorScheme,
  /// sRGB; `None` where the OS has none or the user left it to apps (macOS Multicolor).
  pub accent: Option<Color>,
}

impl Appearance {
  pub const LIGHT: Self = Self { scheme: ColorScheme::Light, accent: None };
  pub const DARK: Self = Self { scheme: ColorScheme::Dark, accent: None };
}

/// What an input method does to the focused text field (macOS `NSTextInputClient`, Windows
/// IMM32, GTK's input method context). Ranges are UTF-16 code units of the field's text, as
/// AppKit and IMM32 count.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Ime {
  /// Shows `text` as marked text (the composition: underlined at the caret, not yet part of the
  /// value) in place of the marked text, or `replace` (a range of the field's text), or the
  /// selection; `sel` is the selection within `text` (the input method's caret or converted
  /// clause). Empty, it ends composing with nothing typed.
  Mark { text: String, sel: Range<usize>, replace: Option<Range<usize>> },
  /// Commits `text` in place of the marked text, or `replace`, or the selection.
  Commit { text: String, replace: Option<Range<usize>> },
  /// Keeps the marked text as typed (a click elsewhere, the input method switched off).
  Unmark,
}

/// Points per line for mouse wheels, which report lines rather than points.
pub const WHEEL_LINE: f32 = 18.0;

// MARK: Waker

type Notify = Box<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct WakerInner {
  woken: AtomicBool,
  notify: OnceLock<Notify>,
}

/// Requests a redraw from any thread (background loading, timers of other threads). Cheap to
/// clone; repeated wakes before the next frame coalesce into one.
#[derive(Clone, Default)]
pub struct Waker(Arc<WakerInner>);

impl Waker {
  pub fn wake(&self) {
    if !self.0.woken.swap(true, Ordering::AcqRel)
      && let Some(notify) = self.0.notify.get()
    {
      notify();
    }
  }

  /// Sets how a wake reaches the event loop (the window hops to the main thread).
  pub(crate) fn set_notify(&self, f: Notify) {
    let _ = self.0.notify.set(f);
  }

  pub(crate) fn take(&self) -> bool {
    self.0.woken.swap(false, Ordering::AcqRel)
  }
}
