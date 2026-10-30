// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The platform seam: everything that depends on the OS, one module per OS, picked by
//! `target_os` (no trait objects: one implementation per build). The rest of the crate (scene,
//! layout, widgets, the CPU renderer, the harness) is portable and reaches the OS only through
//! the items below. `macos/` is AppKit + CoreText, `windows/` Win32 + DirectWrite, `linux/`
//! GTK 3 (on Wayland or X11) + fontconfig, FreeType and HarfBuzz (all loaded at run time).
//!
//! What a platform module provides (the `use` below fails to build when one is missing):
//!
//! - `pub mod shell`, the loop and the windows ([crate::shell] over it, which keeps each window's
//!   app type and close callback, and the quit hook; ids are `u64`, all on the main thread unless
//!   noted):
//!   - `run(info: &AppInfo, start: impl FnOnce(&mut Main))`: sets the app up, runs `start`, then
//!     the loop until `quit` (not when the last window closes); `quit` in `start` skips the loop;
//!     at the end `crate::shell::finish(close)`: the quit hook, then the windows close;
//!   - `open(cfg: &Config, app: Box<dyn App>, ty: TypeId) -> u64`: a window, kept with
//!     `crate::shell::opened(id, ty)` where the platform keeps it, shown in front unless
//!     `cfg.hidden`; `front(id)`, `hide(id)` (off screen, no frames, its app kept: `front` shows it
//!     again), `close(id)` (its runtime dropped, then its close callback from
//!     `crate::shell::closed(id)`), `with_app(id, f: &mut dyn FnMut(&mut dyn App)) -> bool` (not
//!     while the window is busy up the stack; redraws);
//!   - an overlay (`cfg.overlay`, [crate::Config::overlay]): no frame, transparent (the renderer's
//!     `transparent`, the buffer's alpha composited), click-through, never key or active, above
//!     full-screen apps on every Space or workspace, not in the Dock, taskbar or window cycling;
//!     `front` places it at its [crate::Anchor] in the work area of the screen with the pointer
//!     (where the OS lets an app place itself) and shows it without activating anything; a size the
//!     app asks for keeps it at its anchor;
//!   - `set_presence(Presence)`, `terminating() -> bool`, `quit()`; a second launch, where the OS
//!     tells (macOS), runs `crate::shell::reopened`; a window shown or brought to the front comes
//!     with `crate::shell::token()`, the activation token of the command being served, where the OS
//!     takes one (Linux: `gtk_window_set_startup_id`);
//!   - `post(delay: Duration, f: Box<dyn FnOnce() + Send>)` from any thread, run by the loop also
//!     in modal loops (a menu open, a live resize); `is_main() -> bool`.
//!
//!   [crate::run] is one window whose close quits ([crate::shell] `window`). A window's app that
//!   quits ([crate::Cx::quit]) quits the app. Each window owns a [crate::Runtime] made with
//!   `Runtime::new(app, cfg.size, scale, Clock::real(), Box::new(<its clipboard>))`, and:
//!   - window: `cfg.title`, `cfg.size` in points (content area), `cfg.resizable`, the least content
//!     size `cfg.min()` (macOS: `contentMinSize`, Windows: `WM_GETMINMAXINFO`, Linux: the drawing
//!     area's size request); minimal app menu where the OS has one (macOS: Quit);
//!   - title bar: where `cfg.unified` and the OS draws the content under it (macOS: a full-size
//!     content view under a transparent title bar, its title hidden),
//!     [crate::Runtime::set_titlebar] with its height and `cfg.title`; a left press where
//!     [crate::Runtime::drags_window] moves the window (macOS: `performWindowDragWithEvent:`, a
//!     double-click as `AppleActionOnDoubleClick` says) and never reaches the app;
//!   - window size from the app: after each event and frame, [crate::Runtime::take_resize] is a
//!     content size the app asked for ([crate::Ui::set_window_size]); resize the window to it,
//!     resizable or not, lowering the least size with it (macOS: `setFrame:display:` keeping the
//!     top edge; Windows: `SetWindowPos` keeping the top-left corner; Linux: `gtk_window_resize`, a
//!     fixed window's area's size request);
//!   - input: OS events to [crate::Event] in points, y down from the view's top left: mouse (moves,
//!     buttons with click counts, exit), scroll (`precise` and phases for trackpads, wheel lines ×
//!     [crate::WHEEL_LINE]), pinch, modifiers (none once the focus goes: their changes then go to
//!     another window), focus (key window); keys as [crate::KeyEvent] whose `code` is the key's
//!     place, a W3C [crate::Code] from the OS's own code (macOS `kVK_*`, Windows' virtual keys and
//!     scan codes, evdev; set 1 `scan_code` is shared), so [crate::Key::named] and bindings work
//!     unchanged; `mods` the modifier keys as named on the keyboard (Ctrl is `Modifiers::CONTROL`
//!     everywhere, [crate::CMD] the command key); `key` from the layout (with Cmd/Ctrl, a non-Latin
//!     layout's character is the ASCII-capable layout's, so shortcuts work: macOS `keyboard.rs`),
//!     `text` the printable character it types; a modifier key going down or up also sends
//!     [crate::Event::Modifier] (macOS: `flagsChanged:`, the side from the device-dependent flags);
//!   - text input: a key nobody used ([crate::Runtime::event] returns false) goes to the OS's text
//!     input and comes back as [crate::Event::Text] (typed) or [crate::Event::Command] (editing
//!     keys: macOS's from AppKit, Windows' and Linux's from [crate::keys::Os::command]);
//!   - cursors: show [crate::Runtime::cursor] (a [crate::Cursor]) while the pointer is in the view;
//!   - clipboard: plain text, an [crate::Clipboard]; a reader of ours never blocks the loop;
//!   - frames: draw only while [crate::Runtime::needs_frame], paced by the display (≤ 120 Hz);
//!     idle, sleep until the next timer ([crate::Runtime::due_in]) or a wake: set
//!     `rt.waker().set_notify(f)`, `f` called from any thread to wake the loop; no frames while the
//!     window is hidden (minimized, covered), one when it shows again;
//!   - scale: the backing scale factor, given to [crate::Runtime::resize] with the size before each
//!     frame (a change refills the glyph atlas);
//!   - present: [crate::Runtime::frame], then `cpu::Renderer::sync(&mut rt.text, &mut rt.images)`
//!     and the CPU frame into the window: `prepare(scene, size, scale)`, `damage(&shown_tiles)`,
//!     `draw(scene, px, stride, &rects)` into a buffer the OS shows, keeping `tiles()` with each
//!     buffer (macOS: IOSurfaces as the layer's contents, `surface.rs`; Windows: a DIB section and
//!     BitBlt; Linux: a cairo image surface GTK paints). Draw synchronously during a live resize so
//!     the content never stretches.
//!   - appearance: send `Event::Appearance(appearance())` ([crate::Event::Appearance]) to the
//!     runtime right after making it, before the first frame, and again whenever the OS's light or
//!     dark setting or accent colour changes (macOS: `viewDidChangeEffectiveAppearance`,
//!     `NSSystemColorsDidChangeNotification`); the runtime drops repeats. The theme follows it
//!     unless the app set one.
//!   - context menus: `Runtime::set_native_menus(true)` when the window opens; after an event or a
//!     frame, outside the window's borrow, `Runtime::take_context_menu()` gives `(on, at, Menu)` to
//!     show as the OS's menu at `at` (macOS `popUpContextMenu` with the press kept, Windows
//!     `TrackPopupMenuEx` on the release, Linux a `GtkMenu` from the press's `GdkEvent`); its
//!     answer, an item's id or none, goes back as `Event::Menu { on, item }`.
//! - `pub mod tray` ([crate::tray]): `Tray: Clone` with `new(icon, tooltip)`, `detached(icon,
//!   tooltip)` (nothing on screen, for tests), `set_icon`, `set_tooltip`, `set_menu(&Menu)` (held
//!   while the menu is open, the last one set when it closes: `tray::State` holds it), `state() ->
//!   &State` (the app's callbacks too), and for tests `describe() -> Vec<String>`, `opening()`,
//!   `closing()`, `choose(path: &[usize])`; `available() -> Result<(), String>`.
//! - `pub mod hotkey` ([crate::hotkey]): `Hotkeys` with `new(on: OnEdge)` (`on` runs on a thread of
//!   its own), `set(&mut self, Option<Hotkey>) -> Result<(), String>` (`None`: no hotkey; `Err`
//!   when the OS can't take it), `start(&mut self) -> Result<(), String>` (the listener),
//!   `escape(&mut self, on: bool)` (Escape taken from every app as `Edge::Escape`), and
//!   `status(&self, &mut Status)` filling the listener, fallback, secure input, counts and, while
//!   Escape is on, why it is not taken (`escape_error`). Besides, `modifier_alone() -> bool` (a
//!   modifier key alone can be the hotkey) and `start_asks() -> bool` (`start` shows the desktop's
//!   dialog).
//! - `pub mod autostart` ([crate::autostart]): `available()`, `state(&AppInfo)`, `set(&AppInfo,
//!   on)`, `open_settings()`.
//! - `pub mod desktop` ([crate::desktop]): `install(&AppInfo, &Launcher) -> Result<bool, String>`
//!   and `uninstall(&AppInfo) -> Result<(), String>`; nothing but on Linux.
//!
//!   The tray, hotkeys and autostart live in `app/` on each OS (on Windows and Linux with the
//!   shell), sized apart from the window layer (`scripts/size.py`).
//! - `pub mod open` ([crate::open]): `url(&str)`, `path(&Path)` and `reveal(&Path)` (an absolute
//!   path), each `-> Result<(), String>` without waiting for the app it starts; `file_manager() ->
//!   &'static str`.
//! - `pub(crate) mod instance` ([crate::instance]), one file for every Unix
//!   (`linux/app/instance.rs`) and Windows' (`windows/app/instance.rs`): `lock_file(&Path) ->
//!   io::Result<File>`, `Server` with `bind(dir, lock)` and `serve(on: impl Fn(&str) + Send)`, a
//!   thread that calls `on` with each line, both let go on drop, and `send(dir, line, deadline:
//!   Instant)`, `NotFound` or `ConnectionRefused` when nothing listens.
//! - `pub mod uninstall` ([crate::uninstall]): `steps(&AppInfo) -> Vec<Step>`, what the OS keeps
//!   for the app besides launch at login, undone (macOS its permissions and defaults, Linux the
//!   launcher entry and the portal's token), and `busy(&io::Error) -> bool`, an error a removal may
//!   pass when tried again.
//! - `pub mod activity` ([crate::activity]), from any thread: `Token: Send`, `begin(&str) ->
//!   Token`, `end(Token)`.
//! - `pub mod post` ([crate::keys::post]): `post(Shortcut) -> Result<(), String>`, the shortcut as
//!   key events to the app in focus, its character (lowercase) found on the layout in use.
//! - `pub fn clipboard() -> Option<impl Clipboard>`: the system clipboard outside a window
//!   ([crate::SystemClipboard]), `None` while the platform can't reach it (no app running).
//! - `pub mod paste` ([crate::clipboard], called on the main thread, which a read may leave for a
//!   while: macOS): `change_count() -> Option<u64>`, `readable() -> bool`, `snapshot() ->
//!   Option<Snapshot>`, `restore(Snapshot) -> bool`, `offer(&str, Option<Box<dyn FnMut()>>) ->
//!   Option<u64>` (the text marked for clipboard managers to skip, handed out at a read that runs
//!   the callback when there is one) and `holds(u64) -> bool` (nothing was written since that
//!   offer).
//! - `pub mod focus` ([crate::focus]): `current() -> Option<Focused>`, `hidden() -> bool` (the OS
//!   hides the focus: `current` is always `None`) and `Window: Clone + Debug + PartialEq + Send +
//!   Sync`, another app's window by identity.
//! - `shell::alert(title: &str, message: &str)` ([crate::Main::alert]): a modal alert on the main
//!   thread, returning once dismissed.
//! - Windows only, `downloads() -> Option<PathBuf>` (the Known Folder, [crate::paths::downloads])
//!   and `attach_console()` ([crate::attach_console]), in `app/`.
//! - Linux only, `startup_complete(token: &str)`: the launcher's busy cursor stops for a launch
//!   whose command ([crate::Main::serve]) brought no window forward.
//! - macOS only, `pub mod permissions` ([crate::permissions]): `accessibility(prompt: bool) ->
//!   bool`, `secure_input() -> bool`, `microphone() -> Access`, `request_microphone(Box<dyn
//!   FnOnce(bool) + Send>)`.
//!
//! - `pub mod hardware` ([crate::hardware]), from any thread: `transport(id: &str) -> Transport`,
//!   an audio device's by the id cpal gives (macOS the CoreAudio UID, its transport type to
//!   `hardware::core_audio`; Windows the IMMDevice endpoint id, its PnP enumerator to
//!   `hardware::pnp`; Linux `Unknown`), and `lid_closed() -> Option<bool>` (macOS IOPMrootDomain's
//!   `AppleClamshellState`; `None` without a lid, on Windows and Linux).
//!
//!   `activity`, `hardware`, `open`, `post`, `paste`, `focus`, `permissions` and `uninstall` live
//!   in `app/` on each OS too.
//! - `pub fn appearance() -> Appearance` ([crate::Appearance]): `dark`, and `accent` in sRGB or
//!   `None` when the OS has none or the user left it to apps (macOS: `effectiveAppearance` matched
//!   against Aqua / DarkAqua, `controlAccentColor` unless Multicolor; on the main thread only,
//!   [crate::Appearance::LIGHT] elsewhere). Windows: `AppsUseLightTheme` (HKCU
//!   `Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`), re-read on `WM_SETTINGCHANGE`
//!   with "ImmersiveColorSet", the accent from `AccentColorMenu`, and the title bar dark with
//!   `DWMWA_USE_IMMERSIVE_DARK_MODE`. Linux: the XDG portal's `org.freedesktop.appearance`
//!   `color-scheme` and `accent-color`, changes by its `SettingChanged` signal; else GtkSettings'
//!   dark preference or theme name.
//! - `pub fn perf_cores() -> Option<usize>`: the performance cores, to size the CPU renderer's
//!   threads ([crate::cpu::pool]); `None` falls back to `available_parallelism` capped at 4.
//! - `pub mod text`, the text backend under [crate::text] (its atlas and line cache are portable):
//!   - `pub struct Face: Clone + PartialEq`: a font at a point size; equal faces are one.
//!   - `pub fn face(font: Font) -> Face`: the system UI font or its monospaced one, at the size and
//!     weight.
//!   - `pub fn metrics(face: &Face) -> Metrics`: ascent, descent, leading in points.
//!   - `pub fn is_color(face: &Face) -> bool`: colour glyphs (emoji), drawn as RGBA.
//!   - `pub fn color_glyph(face: &Face, id: u16) -> Option<bool>`: how a colour face's glyph draws:
//!     `None` as coverage (a monochrome glyph, tinted like text), `Some(true)` as colour with
//!     layers in the text's colour (`raster.text`, so a bitmap per colour), `Some(false)` as colour
//!     alone.
//!   - `pub fn shape(face: &Face, s: &str, index: impl FnMut(Face) -> u16) -> Line`: shapes one
//!     line with kerning, ligatures and font fallback; each run's face (after fallback) is given to
//!     `index` for its number; glyph ids, pen x and the offset up from the baseline in points, and
//!     per glyph the byte offset in `s` where its cluster starts; `width` the advance.
//!   - `pub fn glyph_box(face: &Face, id: u16, scale: f32, subpixel: u8) -> Option<GlyphBox>`: the
//!     pixel box of a glyph at `scale`, drawn `subpixel` quarters of a pixel right of the pen,
//!     padded one pixel each side; `None` for no ink.
//!   - `pub fn rasterize(face: &Face, id: u16, subpixel: u8, b: &GlyphBox, raster: Raster, color:
//!     bool) -> Option<Vec<u8>>`: the glyph's pixels in that box, rows top-down: coverage (1 byte,
//!     with `raster.smooth` / `raster.dark` where the OS smooths) or premultiplied sRGB RGBA for a
//!     colour glyph.
//!
//!   Process-wide state (a DirectWrite factory, a FreeType library) lives in the backend:
//!   [crate::text::TextSystem] is made often and must stay cheap.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as imp;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;

/// One instance ([crate::instance]): Linux's Unix socket on macOS too, a named pipe on Windows.
#[cfg_attr(unix, path = "linux/app/instance.rs")]
#[cfg_attr(windows, path = "windows/app/instance.rs")]
pub(crate) mod instance;

/// Linux's, for its files in the user's folders: on every Unix for [crate::desktop]'s tests.
#[cfg(any(all(test, unix), target_os = "linux"))]
#[path = "linux/app/write.rs"]
pub(crate) mod write;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
compile_error!("nimbo runs on macOS, Windows and Linux");

#[cfg(any(test, target_os = "windows"))]
use std::time::{Duration, Instant};

pub use imp::appearance;
#[cfg(target_os = "macos")]
pub(crate) use imp::permissions;
#[cfg(target_os = "linux")]
pub(crate) use imp::startup_complete;
pub(crate) use imp::{
  activity, autostart, clipboard, desktop, focus, hardware, hotkey, open, paste, perf_cores, post,
  shell, text, tray, uninstall,
};
#[cfg(target_os = "windows")]
pub(crate) use imp::{attach_console, downloads};
#[cfg(any(test, target_os = "windows", target_os = "linux"))]
use keyboard_types::Code;

/// The points of `px` pixels at `scale`: the least that the renderer's pixels (`points × scale`,
/// cut down: [crate::cpu::Renderer::pixel_size]) give back whole at a fractional scale, where
/// `px / scale` alone can lose the last row (454 px at 1.75).
#[cfg(any(test, target_os = "windows"))]
pub(crate) fn points(px: u32, scale: f32) -> f32 {
  let mut p = px as f32 / scale;
  while ((p * scale) as u32) < px {
    p = p.next_up();
  }

  p
}

/// The key at a PC keyboard's scan code `scan` (set 1, as Windows reports it without the extended
/// bit); evdev codes (<linux/input-event-codes.h>) are the same up to F12's 88.
#[cfg(any(test, target_os = "windows", target_os = "linux"))]
pub(crate) fn scan_code(scan: u16) -> Code {
  use Code::*;
  #[rustfmt::skip]
  const SET1: [Code; 89] = [
    Unidentified, Escape, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, // 0x00
    Digit7, Digit8, Digit9, Digit0, Minus, Equal, Backspace, Tab,
    KeyQ, KeyW, KeyE, KeyR, KeyT, KeyY, KeyU, KeyI, // 0x10
    KeyO, KeyP, BracketLeft, BracketRight, Enter, ControlLeft, KeyA, KeyS,
    KeyD, KeyF, KeyG, KeyH, KeyJ, KeyK, KeyL, Semicolon, // 0x20
    Quote, Backquote, ShiftLeft, Backslash, KeyZ, KeyX, KeyC, KeyV,
    KeyB, KeyN, KeyM, Comma, Period, Slash, ShiftRight, NumpadMultiply, // 0x30
    AltLeft, Space, CapsLock, F1, F2, F3, F4, F5,
    F6, F7, F8, F9, F10, NumLock, ScrollLock, Numpad7, // 0x40
    Numpad8, Numpad9, NumpadSubtract, Numpad4, Numpad5, Numpad6, NumpadAdd, Numpad1,
    Numpad2, Numpad3, Numpad0, NumpadDecimal, // 0x50
    Unidentified, Unidentified, IntlBackslash, F11, F12,
  ];
  SET1.get(scan as usize).copied().unwrap_or(Unidentified)
}

/// Frames an interval apart from the start of each, so the time spent drawing one doesn't add to
/// the gap: 8 ms frames at 60 Hz still come every 16.7 ms, not every 25 (40 fps). For Windows'
/// loop, paced by a timer; macOS has its display link, Linux GDK's frame clock.
#[cfg(any(test, target_os = "windows"))]
pub(crate) struct Pace {
  pub interval: Duration,
  /// When the last frame started.
  last: Option<Instant>,
}

#[cfg(any(test, target_os = "windows"))]
impl Pace {
  pub fn new(interval: Duration) -> Self {
    Self { interval, last: None }
  }

  /// A frame starts at `now`: call before building it.
  pub fn start(&mut self, now: Instant) {
    self.last = Some(now);
  }

  /// The time from `now` until the next frame is due: zero when it is (or none was drawn yet).
  pub fn left(&self, now: Instant) -> Duration {
    let next = self.last.map(|t| t + self.interval);
    next.map_or(Duration::ZERO, |t| t.saturating_duration_since(now))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scene::Size;

  /// 8 ms frames at 60 Hz: the next is due 8.7 ms after one ends, and a second holds 60 of them.
  #[test]
  fn pace_counts_from_the_frame_start() {
    let (draw, interval) = (Duration::from_millis(8), Duration::from_micros(16_667));
    let t0 = Instant::now();
    let mut pace = Pace::new(interval);
    assert_eq!(pace.left(t0), Duration::ZERO);

    pace.start(t0);
    assert_eq!(pace.left(t0 + draw), interval - draw);
    assert_eq!(pace.left(t0 + interval), Duration::ZERO);

    let (mut now, mut frames) = (t0, 0);
    pace = Pace::new(interval);
    while now < t0 + Duration::from_secs(1) {
      let left = pace.left(now);
      if left.is_zero() {
        pace.start(now);
        now += draw;
        frames += 1;
      } else {
        now += left;
      }
    }

    assert_eq!(frames, 60);
  }

  /// Set 1 as Windows and evdev number it: xev's keycodes on a US layout less 8.
  #[test]
  fn scan_codes() {
    let scans = [0x01, 0x02, 0x10, 0x1E, 0x2C, 0x39, 0x47, 0x53, 0x56, 0x57, 0x58, 0x59];
    let want = [
      Code::Escape,
      Code::Digit1,
      Code::KeyQ,
      Code::KeyA,
      Code::KeyZ,
      Code::Space,
      Code::Numpad7,
      Code::NumpadDecimal,
      Code::IntlBackslash,
      Code::F11,
      Code::F12,
      Code::Unidentified,
    ];
    assert_eq!(scans.map(scan_code), want);
    let typing = (0..89).filter(|&s| crate::keys::us_char(scan_code(s), false).is_some()).count();
    assert_eq!(typing, 48);
  }

  #[test]
  fn points_give_their_pixels_back() {
    for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 3.0] {
      for px in 1..4000 {
        let p = points(px, scale);
        assert_eq!(crate::cpu::Renderer::pixel_size(Size::new(p, p), scale).0, px as usize);
      }
    }
  }
}
