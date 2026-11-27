// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! nimbo: a small custom-drawn UI toolkit. Its own CPU renderer for drawing; the OS for the
//! window, input and text behind `platform` (AppKit and CoreText on macOS, Win32 and DirectWrite
//! on Windows, GTK 3 with fontconfig, FreeType and HarfBuzz on Linux), nothing else.
//!
//! An [App] gets [Event]s and builds each frame as an element tree on a [Ui] (laid out and
//! painted by nimbo; [App::view] says how often a view runs and what it may change) and/or paints
//! shapes, images and text through a [Frame]; the runtime drives it, fed by the window
//! (`platform`) or by the headless [Harness], both drawing with nimbo's CPU renderer. The scene,
//! the renderer and the laid-out tree are the crate's own, so they can change without breaking an
//! app. Its strings show in the user's language from the app's gettext catalogs ([t!], below).
//! The examples (`examples/`) open a window, or with `--shot <file.png>` render it in the harness
//! instead (in the dark appearance unless `--appearance light`, in English unless `--locale
//! <tag>`).
//!
//! What an app builds with is at the crate's root, one name each, as SwiftUI's one namespace
//! (`nimbo::Button`, `nimbo::Icon`). A module is a service of its own that an app names as it
//! calls it (`tray::Tray`, `hotkey::Hotkeys`, `paste::text`), the clipboard ([clipboard]), what
//! assistive technology reads ([a11y]) and the test harness's parts ([harness]).
//!
//! An app with no window, or more than one, starts with [app()]: windows on demand ([Main::open])
//! and overlays, HUDs over every app ([Config::overlay]), the menu bar's commands
//! ([Main::set_commands]), work from other threads ([Proxy]), and what a tray app needs besides:
//! an icon with a menu ([tray]), global hotkeys ([hotkey]), one instance that takes messages from
//! a second launch ([instance], [Main::serve]), its folders and
//! the user's Downloads ([paths]), launch at login ([autostart]), its launcher entry on Linux
//! ([desktop]), alerts ([Main::alert]), its command line's console on Windows
//! ([attach_console]) and an uninstall that undoes what nimbo set up ([uninstall]); and to reach
//! other apps: links and files opened or shown ([open]), the OS's permissions ([permissions]),
//! where the user types ([focus]) and with which keyboard layout ([input_source]), keys posted to
//! the app in focus ([paste::post]), the clipboard, also taken and put back around a paste of the
//! app's own ([clipboard::SystemClipboard]), and that paste itself, which gives the clipboard back
//! ([paste]); photos decoded by the OS at the size they show ([Bitmap]); and the hardware: how a
//! microphone is attached and the laptop's lid ([hardware]), and the system's changes, such as a
//! microphone plugged or the lid closed, as they come ([system], [Main::watch]).
//!
//! macOS 11 is the least it runs on, rustc's default deployment target, so an app sets none;
//! newer APIs are used where the system has them (the activation, the display link and menu
//! section headers on 14+, launch at login on 13+).
//!
//! What an app reads and the enums that may grow are `#[non_exhaustive]`, so a minor release can
//! add to them: a `match` on one keeps a `_` arm, and such a struct is read or changed field by
//! field, never built whole. What an app gives (a style, a font, a window's config) keeps its
//! fields private: it starts from a constructor or a constant and changes by `const` builders
//! named as the field (`Style::new().pad(8.0)`), so a field added breaks nothing; where an app
//! reads one back, a getter, `is_` before a flag and `get_` before a name a builder holds
//! ([TextStyle::get_color]). The theme, its sizes and a form's style, read far more than built,
//! read by the field's name and change by `with_` it (`Theme::PLATES_DARK.with_text(WHITE)`).
//!
//! # Building a frame
//!
//! ```ignore
//! ui.row(Style::new().pad(8.0).gap(8.0).bg(PANEL), |ui| {
//!   ui.text("Title", BOLD);
//!   ui.spacer();
//!   ui.id("save").row(BUTTON, |ui| ui.text("Save", LABEL));
//! });
//! ```
//!
//! A text styled in runs (colours, weights, italic, code, lines, grounds) is an
//! [AttributedString], shown and wrapped as one paragraph by the same [Ui::text].
//!
//! Containers take their children in a closure, so the code nests like the tree. Every element
//! gets an [Id]: its index among its siblings hashed with its parent's id, a key hashed the same
//! way ([Ui::key], stable when lists change), or a frame-wide name ([Ui::id], found again with
//! [Id::of]). The last frame's tree with every element's rect is kept for hit testing
//! (`input`): named elements get hover, press and clicks, which the view reads by id. State the
//! app should not hold (scroll offsets, animations) lives in a store keyed by id ([Ui::state]).
//!
//! ```ignore
//! let plus = Id::of("plus");
//! let fade = ui.animate(plus, if ui.hovered(plus) { 1.0 } else { 0.0 }, FADE);
//! if ui.with_id(plus).row(BUTTON.bg(mix(RAISED, HOVER, fade)), |ui| ui.text("+", LABEL)).clicked {
//!   self.count += 1;
//! }
//! ```
//!
//! Widgets (label, button, text field, check box, switch, slider and the
//! like) and containers (list, menu, picker, tabs, split; the scroll area they
//! scroll in is a behaviour's, [Ui::scroll_area]) do this for an app in the look of the theme
//! the app sets (`Ui::set_theme`):
//! `if ui.id("plus").button("+").clicked { .. }`; so do a settings form ([FormRow], [Section]) and
//! a modal dialog ([Dialog]).
//! What they read of where they are built, the theme, their size, whether they are enabled, is
//! the environment: values by type a container puts for what is built in it
//! ([Ui::environment]: `ui.disabled(true).col(..)`), the window for all ([Ui::set_environment]).
//!
//! The rule for every widget: a headless behaviour (`ui.button_behavior(id)` and
//! the like, hover, press, click, focus, keys and the value edited) plus drawing from the
//! theme. An app that wants its own look keeps the behaviour and draws the element itself.
//! Behaviours also record what their element is to assistive technology ([a11y]), so a look of
//! the app's own reads as the theme's does.
//!
//! # Keys
//!
//! A key press travels in this order:
//!
//! 0. A focused element that grabs the keys ([Ui::grab_keys]: a hotkey recorder recording, a modal
//!    dialog) gets the key itself first.
//! 1. Bindings of the focused element, then of its ancestors ([Ui::keymap]): a match becomes a
//!    [Command] for that element, which reads it with [Ui::command].
//! 2. The app's bindings ([App::keymap]): a match goes to `App::event` as [Event::Command].
//! 3. The key itself: the focused element ([Ui::key_down]), then `App::event` as [Event::KeyDown].
//! 4. Nobody used it: AppKit's `interpretKeyEvents:` turns it into typed text (`insertText:`,
//!    [Event::Text]) or a command (`doCommandBySelector:`, [Event::Command]), with the system's
//!    bindings: arrows, Option and Cmd moves, Emacs-style Ctrl keys. Text and commands go to the
//!    focused element, then the app; an unused Tab or Shift-Tab moves focus. A key with Cmd that
//!    nobody used goes on to the menu bar (Cmd-Q), then to step 4.
//!
//! The menu bar's Edit items ([Commands]) run the command their key would: the focused
//! element, then those above it that bind the command, then the app.
//!
//! # Strings in the user's language
//!
//! An app marks its strings with [t!] and [tn!]; they are looked up in gettext catalogs the app
//! ships, keyed by the English text itself, as SwiftUI's `Text("…")` and gettext key them.
//!
//! - `t!("Keep the history")` is a [Localized], `const` and `Copy`, translated where it shows: a
//!   text ([Ui::text]), a [Content], `format!("{KEEP}")`, a label as `&t!("Cancel")` (a `&str`). A
//!   string not marked never is (SwiftUI's `Text(verbatim:)`): names, paths, what the user typed.
//! - `t!("{chosen} is not on disk", chosen, size = f.size)` is a `String` translated now, its
//!   `{name}`s filled (`{{` and `}}` for braces), in the order the translation puts them.
//! - `tn!("{n} file", "{n} files", count)` is a `String` in the form the catalog's `Plural-Forms`
//!   picks for `count` (English's without one: 1, else the other), `{n}` the count; more `{name}`s
//!   follow as in `t!`.
//!
//! The catalogs, one `.po` file per language, come with the window's [Config]
//! ([Config::localizations]), or before any window, for a tray
//! menu, from [Main::set_localizations]; only the one in use is read. The
//! language is the system's ([Locale::System]) or the app's choice ([Main::set_locale]),
//! found as the tag (`pt-BR`), then its language (`pt`); a language alone also finds its first
//! longer tag (`pt` finds `pt-BR`, as Apple's bundles); with no catalog, the English text.
//! nimbo's own strings (OK and Cancel, the text menu, an icon button's name) come in its own
//! catalogs (`locales/`, `make i18n`), in the language the app's give.
//! `pseudo` is a pseudo language (`--locale pseudo` in an example): every marked string accented
//! and a third longer, so unmarked and cut strings show. GNU gettext (0.24 or later, for Rust)
//! keeps the catalogs; a comment `// Translators: …` before a string goes to its translator:
//!
//! ```sh
//! xgettext --language=Rust --keyword='t!:1' --keyword='tn!:1,2' --add-comments=Translators \
//!   --from-code=UTF-8 -o locales/messages.pot src/*.rs
//! msginit -l ru_RU.UTF-8 -i locales/messages.pot -o locales/ru.po  # a new language
//! msgmerge --update locales/ru.po locales/messages.pot              # the code changed
//! msgfmt -c --statistics -o /dev/null locales/ru.po                 # forms, {placeholders}
//! ```
//!
//! An English string changed becomes fuzzy in the catalogs and shows in English until a
//! translator confirms it; one removed becomes obsolete (`#~`).

pub mod activity;
mod app;
pub mod autostart;
mod bitmap;
pub mod clipboard;
mod commands;
pub(crate) mod cpu;
pub mod desktop;
mod error;
mod event;
mod file_dialog;
pub mod focus;
pub(crate) mod geom;
pub mod hardware;
pub mod harness;
pub mod hotkey;
mod info;
pub mod input_source;
pub mod instance;
mod keys;
mod locale;
mod menu;
pub mod open;
pub mod paste;
pub mod paths;
pub mod permissions;
pub(crate) mod platform;
mod png;
mod runloop;
mod scene;
mod shell;
mod svg;
pub mod system;
mod text;
pub mod tray;
mod ui;
pub mod uninstall;
mod window;

pub use app::{App, Appearance, ColorScheme, Cx, Event, Ime, MouseButton, Phase, Waker};
pub use bitmap::Bitmap;
pub use commands::{CommandGroupPlacement, Commands};
pub use error::Error;
pub use file_dialog::{FileDialog, FileDialogKind};
pub use harness::run::Harness;
pub use keys::{
  Binding, CMD, CTRL, Code, Command, Hotkey, Key, KeyEvent, Modifiers, NamedKey, Os, Shortcut,
  ShortcutError,
};
pub use locale::{Locale, Localizations, Localized};
pub use menu::{ButtonRole, Item, Menu};
pub use png::Png;
pub use scene::{Color, Frame, ImageId, Images, Point, Quad, Rect, Size, alpha, mix, rgb};
pub use shell::{Anchor, AppInfo, Main, Presence, Proxy, Window, WindowId, app, attach_console};
pub use text::{Design, Font, Metrics, Weight};
pub use ui::{
  Align, Answer, AttributedString, Attributes, Axes, Axis, Behavior, BorderedButtonStyle,
  BorderedProminentButtonStyle, BorderlessButtonStyle, Button, ButtonConfiguration, ButtonStyle,
  ButtonToggleStyle, CheckboxToggleStyle, Click, Clip, Content, ContentMode, ControlBorder,
  ControlSize, CoordinateSpace, Cursor, Dialog, DialogBehavior, Dim, Ease, EditMenu, Editing,
  Enabled, EnvironmentValue, FieldBehavior, FieldKind, FieldStyle, Float, Focus, FocusDirection,
  FocusRing, FormRow, FormStyle, FrameStats, Glass, HelpConfiguration, HelpStyle, Icon, IconPaint,
  Id, ImageSource, InlinePickerStyle, InsetGroupedListStyle, InsetListStyle, IntoText, LinePattern,
  LinearSliderStyle, List, ListBehavior, ListConfiguration, ListLayout, ListResponse,
  ListRowConfiguration, ListRows, ListStyle, MenuPickerStyle, MenuPlacement, NativeMenu, Picker,
  PickerConfiguration, PickerStyle, PlainButtonStyle, PlainHelpStyle, PlainListStyle,
  PlainSliderStyle, PlainTextFieldStyle, PlainToggleStyle, Press, Response,
  RoundedBorderTextFieldStyle, Row, Scroll, ScrollBar, ScrollBehavior, ScrollerKnob, Section,
  SectionHeader, SegmentedPickerStyle, Selectable, Selection, Shadow, Sizes, Slider,
  SliderConfiguration, SliderStyle, Split, SplitBehavior, Style, SwitchSize, SwitchToggleStyle,
  Tabs, Tag, TextField, TextFieldConfiguration, TextFieldStyle, TextInput, TextLines, TextStyle,
  Theme, ToggleConfiguration, ToggleStyle, Tone, ToolbarItemPlacement, TooltipHelpStyle,
  Transition, TruncationMode, TypeSelect, Ui, UnitPoint, grow, pct, px, rigid,
};
pub use window::{Config, TitleBar, WindowToolbarStyle};

/// What an element is to assistive technology: its role, value, states and actions, which a
/// behaviour records for its control and a look of the app's own refines ([Ui::a11y]).
pub mod a11y {
  pub use crate::ui::a11y::{Access, Action, LabeledPairRole, Role, State, Value};
}

/// What [t!] and [tn!] expand to.
#[doc(hidden)]
pub mod __private {
  pub use crate::locale::{format, plural};
}

#[cfg(test)]
mod tests {
  use super::*;

  /// An icon is `side`² RGBA pixels: other sizes are refused where the config is made.
  #[test]
  #[should_panic(expected = "side² RGBA")]
  fn an_icon_of_the_wrong_size_is_refused() {
    let _ = Config::new("a", 1.0, 1.0).icon(2, &[0; 4]);
  }

  #[test]
  fn min_size_is_capped_by_the_size() {
    assert_eq!(Config::new("a", 880.0, 640.0).min(), Config::MIN_SIZE);
    assert_eq!(Config::new("a", 200.0, 100.0).min(), Size::new(200.0, 100.0));
    assert_eq!(
      Config::new("a", 880.0, 640.0).min_size(720.0, 480.0).min(),
      Size::new(720.0, 480.0)
    );
  }
}
