// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A window as an app asks for it ([Config]): its title, size, icon, an overlay's place
//! ([Anchor]), the content under the title bar, and that title bar as the platform reports it
//! ([TitleBar]).

use crate::geom::{Point, Rect, Size};
use crate::locale::Localizations;
#[cfg(feature = "png")]
use crate::png::Png;
#[cfg(doc)]
use crate::{AppInfo, Main, Ui, Window, locale};

/// Where an overlay ([Config::overlay]) shows: centred across the work area (the screen
/// less the menu bar, the Dock or the taskbar) of the screen with the pointer, a margin in points
/// from its top or bottom edge, or in its middle.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Anchor {
  Top(f32),
  Center,
  Bottom(f32),
}

impl Anchor {
  /// The top-left corner of a window of `size` in a work area of `area`, from the area's top
  /// left (y down), in whole points.
  pub fn offset(self, area: Size, size: Size) -> Point {
    let y = match self {
      Self::Top(margin) => margin,
      Self::Center => (area.h - size.h) / 2.0,
      Self::Bottom(margin) => area.h - size.h - margin,
    };

    Point::new(((area.w - size.w) / 2.0).round(), y.round())
  }
}

/// The title bar a unified window's content runs under ([Config::unified]), as the platform
/// reports it ([Ui::titlebar]), in points from the content's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct TitleBar {
  pub height: f32,
  /// The window's title ([Config::title]).
  pub title: &'static str,
  /// The window's own buttons over the content, macOS's traffic lights, all three; `None` while
  /// they don't show over it (in full screen they come down with the menu bar).
  pub buttons: Option<Rect>,
}

impl TitleBar {
  /// A title bar `height` high over a window titled `title`, the window's buttons at `buttons`:
  /// one a platform reports, or a test plays ([harness::mac_titlebar](crate::harness::mac_titlebar)
  /// plays macOS's).
  pub const fn new(height: f32, title: &'static str, buttons: Option<Rect>) -> Self {
    Self { height, title, buttons }
  }
}

/// How the title bar of a window whose content runs under it ([Config::unified]) shows a
/// toolbar, as SwiftUI's `.windowToolbarStyle` ([Ui::window_toolbar_style]); in any other window
/// it changes nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowToolbarStyle {
  /// The window's own: a toolbar's height while the view builds a toolbar at the window's top
  /// (nimbo's `ui.toolbar`), the title bar's alone otherwise.
  #[default]
  Automatic,
  /// A toolbar's height, the window's buttons centred in it, as AppKit's unified toolbar (macOS
  /// an empty `NSToolbar`): for a band of the view's own drawn at the window's top.
  Unified,
}

/// A window's title and initial content size in points; the harness renders at this size.
pub struct Config {
  pub(crate) title: &'static str,
  pub(crate) size: Size,
  /// The least content size the window resizes to, in points ([Config::min]).
  pub(crate) min_size: Size,
  /// The window can be resized (the default).
  pub(crate) resizable: bool,
  /// Opened without showing ([Config::hidden]) until [Window::front].
  pub(crate) hidden: bool,
  /// An overlay and where it shows ([Config::overlay]); `None` for a window.
  pub(crate) overlay: Option<Anchor>,
  /// The content runs under the title bar ([Config::unified]).
  pub(crate) unified: bool,
  /// The window's icon off macOS ([Config::icon]): its side and straight RGBA pixels, `side`²
  /// of them, as only the builder checks; or a PNG's ([Config::png_icon]).
  #[cfg_attr(target_os = "macos", expect(dead_code, reason = "a macOS window has no icon"))]
  pub(crate) icon: Option<(u32, &'static [u8])>,
  #[cfg(feature = "png")]
  #[cfg_attr(target_os = "macos", expect(dead_code, reason = "a macOS window has no icon"))]
  pub(crate) png_icon: Option<Png>,
  /// The app's catalogs ([Config::localizations]).
  pub(crate) localizations: Localizations,
}

impl Config {
  /// The least content size by default: room for a small form.
  pub const MIN_SIZE: Size = Size::new(320.0, 240.0);

  pub const fn new(title: &'static str, w: f32, h: f32) -> Self {
    let (size, min_size) = (Size::new(w, h), Self::MIN_SIZE);
    let (resizable, hidden, overlay, unified, icon) = (true, false, None, false, None);
    Self {
      title,
      size,
      min_size,
      resizable,
      hidden,
      overlay,
      unified,
      icon,
      #[cfg(feature = "png")]
      png_icon: None,
      localizations: &[],
    }
  }

  /// The app's translations ([locale]), a gettext `.po` file's text per language tag:
  /// `&[("ru", include_str!("../locales/ru.po"))]`. Only the language in use is read, as a
  /// string first shows. Every window shares them: the last window opened with any gives them, or
  /// [Main::set_localizations], which needs no window.
  pub const fn localizations(self, catalogs: Localizations) -> Self {
    Self { localizations: catalogs, ..self }
  }

  pub const fn title(&self) -> &'static str {
    self.title
  }

  /// The initial content size in points: the harness's (`Harness::new(app, CONFIG.size(), 2.0)`).
  pub const fn size(&self) -> Size {
    self.size
  }

  pub const fn is_resizable(&self) -> bool {
    self.resizable
  }

  pub const fn is_hidden(&self) -> bool {
    self.hidden
  }

  /// Where the overlay shows ([Config::overlay]); `None` for a window.
  pub const fn get_overlay(&self) -> Option<Anchor> {
    self.overlay
  }

  /// The least content size the user can resize the window to, [Config::MIN_SIZE] unless set.
  pub const fn min_size(self, w: f32, h: f32) -> Self {
    Self { min_size: Size::new(w, h), ..self }
  }

  /// The least content size as the platform applies it: never more than the initial size.
  pub const fn min(&self) -> Size {
    Size::new(self.min_size.w.min(self.size.w), self.min_size.h.min(self.size.h))
  }

  /// A window of a fixed size: a calculator, a dialog.
  pub const fn fixed(self) -> Self {
    Self { resizable: false, ..self }
  }

  /// The icon in the window's title bar and taskbar entry on Windows and X11: `side` × `side`
  /// pixels of straight RGBA, rows top-down (256 serves every size the system shows). macOS
  /// shows the bundle's icon; Wayland the one of the app's `.desktop` entry, found by the app id
  /// ([AppInfo::id]).
  pub const fn icon(self, side: u32, px: &'static [u8]) -> Self {
    assert!(px.len() == side as usize * side as usize * 4, "an icon is side² RGBA pixels");
    Self {
      icon: Some((side, px)),
      #[cfg(feature = "png")]
      png_icon: None,
      ..self
    }
  }

  /// [Config::icon] from a square PNG the app ships, decoded as the first window opens (not on
  /// macOS): `Png::icon(include_bytes!("icon.png"))` for the app's icon, its margin cut.
  #[cfg(feature = "png")]
  pub const fn png_icon(self, png: Png) -> Self {
    Self { png_icon: Some(png), icon: None, ..self }
  }

  /// The window's icon: its side and pixels, a PNG's decoded now.
  #[cfg_attr(target_os = "macos", expect(dead_code, reason = "a macOS window has no icon"))]
  pub(crate) fn window_icon(&self) -> Option<(u32, &'static [u8])> {
    let icon = self.icon;
    #[cfg(feature = "png")]
    let icon = icon.or_else(|| self.png_icon.map(|png| png.square()));
    icon
  }

  /// A window [Main::open] makes without showing it, until [Window::front].
  pub const fn hidden(self) -> Self {
    Self { hidden: true, ..self }
  }

  /// The title bar is part of the content, as in a macOS preferences window: the content runs
  /// under it and its size is the window's, the app draws the title in the band at the top
  /// ([Ui::titlebar]) and moves the window from its empty parts
  /// ([Ui::drags_window]); `Ui::toolbar_tabs` does both. A `Ui::toolbar` gives the title bar a
  /// toolbar's height, its items beside the window's buttons, as
  /// [WindowToolbarStyle::Unified] ([Ui::window_toolbar_style]) does for a band of the view's own:
  /// this window's content under its title bar is what that style shapes.
  ///
  /// Per OS: macOS a full-size content view under a transparent title bar, its title hidden (the
  /// Window menu and Mission Control still show it), the traffic lights over the content; with a
  /// toolbar, an empty unified `NSToolbar` that makes the bar 52 high with the lights centred in
  /// it, as Finder's and Notes', hidden in full screen. Windows and Linux keep their title bar:
  /// [Ui::titlebar] is `None` there.
  pub const fn unified(self) -> Self {
    Self { unified: true, ..self }
  }

  /// An overlay, a HUD over everything: no frame, transparent where the app draws nothing
  /// (so is its shot: [Harness::set_transparent](crate::Harness::set_transparent)), of a fixed
  /// size; it never activates the app or takes the keys, the mouse goes through it, it shows on
  /// every Space and over full-screen apps. [Window::front] shows it at `anchor` on the screen with
  /// the pointer (again at each call), [Window::hide] hides it. Size it to the widest the app
  /// draws, as it draws its own shape and shadow inside.
  ///
  /// Per OS: macOS a borderless non-activating `NSPanel` at the status bar level. Windows a
  /// layered, click-through, topmost tool window with no activation, shown on the current
  /// virtual desktop. Linux on X11 an override-redirect popup (above all, on every workspace);
  /// on Wayland, where an app can't place itself, a layer-shell surface (`libgtk-layer-shell`,
  /// if installed: wlroots compositors, KDE) on the overlay layer at the anchor, on the output
  /// the compositor picks; without it (GNOME) a borderless toplevel that the compositor places
  /// and may focus and stack as any window.
  pub const fn overlay(self, anchor: Anchor) -> Self {
    Self { overlay: Some(anchor), resizable: false, ..self }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// A HUD 200x40 in a 1000x600 work area: centred across, 20 points off the edge.
  #[test]
  fn anchors_place_in_the_work_area() {
    let (area, size) = (Size::new(1000.0, 600.0), Size::new(200.0, 40.5));
    assert_eq!(Anchor::Top(20.0).offset(area, size), Point::new(400.0, 20.0));
    assert_eq!(Anchor::Bottom(20.0).offset(area, size), Point::new(400.0, 540.0));
    assert_eq!(Anchor::Center.offset(area, size), Point::new(400.0, 280.0));
  }
}
