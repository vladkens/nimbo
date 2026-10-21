// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The platform seam: everything that depends on the OS, one module per OS, picked by
//! `target_os` (no trait objects: one implementation per build). The rest of the crate (scene,
//! the CPU renderer, the harness) is portable and reaches the OS only through the items below.
//! `macos/` is AppKit + CoreText, `windows/` Win32 + DirectWrite, `linux/` Wayland or X11 (picked
//! at start) + fontconfig, FreeType and HarfBuzz (loaded at run time).
//!
//! What a platform module provides (the `use` below fails to build when one is missing):
//!
//! - `pub fn run(cfg: &Config, app: Box<dyn App>)`: opens one window and runs the event loop until
//!   it closes or the app quits ([crate::Cx::quit]); macOS builds it on its app shell (`shell.rs`).
//!   Each window owns a [crate::Runtime] made with `Runtime::new(app, cfg.size, scale,
//!   Clock::real(), Box::new(<its clipboard>))`, and:
//!   - window: `cfg.title`, `cfg.size` in points (content area), `cfg.resizable`, the least content
//!     size `cfg.min()` (macOS: `contentMinSize`, Windows: `WM_GETMINMAXINFO`, Linux:
//!     `xdg_toplevel.set_min_size`, X11 `WM_NORMAL_HINTS`); minimal app menu where the OS has one
//!     (macOS: Quit);
//!   - input: OS events to [crate::Event] in points, y down from the view's top left: mouse (moves,
//!     buttons with click counts, exit), scroll (`precise` and phases for trackpads, wheel lines ×
//!     [crate::WHEEL_LINE]), pinch, modifiers (none once the focus goes: their changes then go to
//!     another window), focus (key window); keys as [crate::KeyEvent] whose `code` is the macOS
//!     virtual key code (`kVK_*`, [crate::keys]): translate the OS scan code to it, so
//!     [crate::Key::named] and bindings work unchanged; `key` from the layout (with Cmd/Ctrl, a
//!     non-Latin layout's character is the ASCII-capable layout's, so shortcuts work: macOS
//!     `keyboard.rs`), `text` the printable character it types; a modifier key going down or up
//!     also sends [crate::Event::Modifier] (macOS: `flagsChanged:`, the side from the
//!     device-dependent flags);
//!   - text input: a key nobody used ([crate::Runtime::event] returns false) goes to the OS's text
//!     input and comes back as [crate::Event::Text] (typed) or [crate::Event::Command] (editing
//!     keys; [crate::keys::standard] is the US mapping);
//!   - cursors: show [crate::Runtime::cursor] (a [crate::Cursor]) while the pointer is in the view;
//!   - clipboard: plain text, an [crate::Clipboard]; a reader of ours never blocks the loop;
//!   - frames: draw only while [crate::Runtime::needs_frame], paced by the display (≤ 120 Hz);
//!     idle, sleep until a wake: set `rt.waker().set_notify(f)`, `f` called from any thread to wake
//!     the loop; no frames while the window is hidden (minimized, covered), one when it shows
//!     again;
//!   - scale: the backing scale factor, given to [crate::Runtime::resize] with the size before each
//!     frame (a change refills the glyph atlas);
//!   - present: [crate::Runtime::frame], then `cpu::Renderer::sync(&mut rt.text, &mut rt.images)`
//!     and the CPU frame into the window: `prepare(scene, size, scale)`, `damage(&shown_tiles)`,
//!     `draw(scene, px, stride, &rects)` into a buffer the OS shows, keeping `tiles()` with each
//!     buffer (macOS: IOSurfaces as the layer's contents, `surface.rs`; Windows: a DIB section and
//!     BitBlt; Linux: `wl_shm` buffers, on X11 a MIT-SHM segment). Draw synchronously during a live
//!     resize so the content never stretches.
//!   - appearance: send `Event::Appearance(appearance())` ([crate::Event::Appearance]) to the
//!     runtime right after making it, before the first frame, and again whenever the OS's light or
//!     dark setting or accent colour changes (macOS: `viewDidChangeEffectiveAppearance`,
//!     `NSSystemColorsDidChangeNotification`).
//! - `pub fn appearance() -> Appearance` ([crate::Appearance]): `dark`, and `accent` in sRGB or
//!   `None` when the OS has none or the user left it to apps (macOS: `effectiveAppearance` matched
//!   against Aqua / DarkAqua, `controlAccentColor` unless Multicolor). Stubs return
//!   [crate::Appearance::LIGHT]. TODO Windows: `AppsUseLightTheme` (HKCU
//!   `Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`), re-read on `WM_SETTINGCHANGE`
//!   with "ImmersiveColorSet", the accent from `UISettings` or `DwmGetColorizationColor`, and the
//!   title bar dark with `DWMWA_USE_IMMERSIVE_DARK_MODE`. TODO Linux: the XDG portal's
//!   `org.freedesktop.appearance` `color-scheme` (1 dark, else light) and `accent-color` through
//!   `org.freedesktop.portal.Settings.Read`, changes by its `SettingChanged` signal; light without
//!   a portal.
//! - `pub fn perf_cores() -> Option<usize>`: the performance cores, to size the CPU renderer's
//!   threads ([crate::cpu::pool]); `None` falls back to `available_parallelism` capped at 4.
//! - `pub mod text`, the text backend under [crate::text] (its atlas and line cache are portable):
//!   - `pub struct Face: Clone + PartialEq`: a font at a point size; equal faces are one.
//!   - `pub fn face(font: Font) -> Face`: the system UI font or its monospaced one, at the size and
//!     weight.
//!   - `pub fn metrics(face: &Face) -> Metrics`: ascent, descent, leading in points.
//!   - `pub fn is_color(face: &Face) -> bool`: colour glyphs (emoji), drawn as RGBA.
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
//!     colour face.
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

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
compile_error!("nimbo runs on macOS, Windows and Linux");

#[cfg(any(test, not(target_os = "macos")))]
use std::time::{Duration, Instant};

pub use imp::{appearance, run};
pub(crate) use imp::{perf_cores, text};

/// The points of `px` pixels at `scale`: the least that the renderer's pixels (`points × scale`,
/// cut down: [crate::cpu::Renderer::pixel_size]) give back whole at a fractional scale, where
/// `px / scale` alone can lose the last row (454 px at 1.75).
#[cfg(any(test, not(target_os = "macos")))]
pub(crate) fn points(px: u32, scale: f32) -> f32 {
  let mut p = px as f32 / scale;
  while ((p * scale) as u32) < px {
    p = p.next_up();
  }

  p
}

/// Frames an interval apart from the start of each, so the time spent drawing one doesn't add to
/// the gap: 8 ms frames at 60 Hz still come every 16.7 ms, not every 25 (40 fps). For the loops
/// paced by a timer; macOS has its display link.
#[cfg(any(test, not(target_os = "macos")))]
pub(crate) struct Pace {
  pub interval: Duration,
  /// When the last frame started.
  last: Option<Instant>,
}

#[cfg(any(test, not(target_os = "macos")))]
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
