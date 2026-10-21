// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The platform seam: everything that depends on the OS, one module per OS, picked by
//! `target_os` (no trait objects: one implementation per build). The rest of the crate (scene,
//! the CPU renderer, the harness) is portable and reaches the OS only through the items below.
//! `macos/` is AppKit, `windows/` Win32, `linux/` Wayland or X11 (picked at start).
//!
//! What a platform module provides (the `use` below fails to build when one is missing):
//!
//! - `pub fn run(cfg: &Config, app: Box<dyn App>)`: opens one window and runs the event loop until
//!   it closes or the app quits ([crate::Cx::quit]); macOS builds it on its app shell (`shell.rs`).
//!   Each window owns a [crate::Runtime] made with `Runtime::new(app, cfg.size, scale,
//!   Clock::real())`, and:
//!   - window: `cfg.title`, `cfg.size` in points (content area), `cfg.resizable`, the least content
//!     size `cfg.min()` (macOS: `contentMinSize`, Windows: `WM_GETMINMAXINFO`, Linux:
//!     `xdg_toplevel.set_min_size`, X11 `WM_NORMAL_HINTS`); minimal app menu where the OS has one
//!     (macOS: Quit);
//!   - frames: draw only while [crate::Runtime::needs_frame], paced by the display (≤ 120 Hz);
//!     idle, sleep until a wake: set `rt.waker().set_notify(f)`, `f` called from any thread to wake
//!     the loop; no frames while the window is hidden (minimized, covered), one when it shows
//!     again;
//!   - scale: the backing scale factor, given to [crate::Runtime::resize] with the size before each
//!     frame;
//!   - present: [crate::Runtime::frame], then `cpu::Renderer::sync(&mut rt.images)` and the CPU
//!     frame into the window: `prepare(scene, size, scale)`, `damage(&shown_tiles)`, `draw(scene,
//!     px, stride, &rects)` into a buffer the OS shows, keeping `tiles()` with each buffer (macOS:
//!     IOSurfaces as the layer's contents, `surface.rs`; Windows: a DIB section and BitBlt; Linux:
//!     `wl_shm` buffers, on X11 a MIT-SHM segment). Draw synchronously during a live resize so the
//!     content never stretches.
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

pub(crate) use imp::perf_cores;
pub use imp::{appearance, run};
