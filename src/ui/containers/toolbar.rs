// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The toolbar: items in a band over the top of the window, as SwiftUI's `.toolbar` of
//! `ToolbarItem(placement:)`s, a sidebar toggle at the leading end and the view's actions at the
//! trailing one.
//!
//! ```ignore
//! ui.toolbar(|ui| {
//!   ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
//!     if ui.help("Hide Sidebar").icon_button(Icon::SIDEBAR).clicked { self.sidebar ^= true }
//!   });
//!   ui.toolbar_item(ToolbarItemPlacement::PrimaryAction, |ui| _ = ui.icon_button(Icon::PLUS));
//! });
//! ```
//!
//! Under a unified window's title bar ([crate::Config::unified]) the band is the title bar, as
//! in Finder, Notes and Messages: macOS gives it a toolbar's height (52, the traffic lights
//! centred in it), the leading items start [AFTER_BUTTONS] right of the lights and sit on their
//! middle, wherever the platform reports them ([crate::TitleBar::buttons]), and the band's empty
//! part moves the window. In full screen, where the lights come down with the menu bar, and in a
//! window with the OS's own title bar (Windows, Linux) the band is the content's top row under
//! it, as an app's header row: the items [LEAD] from its leading end, nothing moved by it.
//!
//! The band floats over its parent's top (the window's, built at the top level) and draws no
//! ground: the content runs under it, so a sidebar's ground goes up to the window's top as in
//! Notes; content leaves [Ui::toolbar_height] at its top for it.

use crate::ui::{Align, ControlSize, Float, Response, Style, Ui, grow, px};

/// Where a toolbar item sits ([Ui::toolbar_item]), as SwiftUI's `ToolbarItemPlacement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarItemPlacement {
  /// At the leading end, beside the window's buttons: a sidebar toggle, back and forward.
  Navigation,
  /// At the trailing end: the view's main actions, a compose or a share button; where SwiftUI's
  /// apps on macOS show it, though its documentation says the leading edge.
  PrimaryAction,
}

/// The band's height where the title bar is lower or none: macOS's unified toolbar's, measured on
/// macOS 27.
pub const TOOLBAR_HEIGHT: f32 = 52.0;
/// From the window's buttons to the first leading item, and the gap between items: macOS 27's
/// toolbar (the zoom button ends 79 in, the sidebar toggle's tile starts at 96, the next 8 after
/// it).
pub const AFTER_BUTTONS: f32 = 17.0;
const GAP: f32 = 8.0;
/// The ends' insets without the window's buttons: macOS 27's toolbar in full screen.
pub const LEAD: f32 = 10.0;
const TRAIL: f32 = 8.0;

/// The placement whose items the toolbar builds now.
struct Building(ToolbarItemPlacement);

impl Ui<'_> {
  /// A toolbar of the items `content` builds ([Ui::toolbar_item]), in a band
  /// [Ui::toolbar_height] high over the top of its parent: the title bar of a unified window
  /// (see [crate::ui::containers::toolbar]). Its widgets are [ControlSize::Large], as a toolbar's
  /// controls are (36-point tiles on macOS 27). `content` runs once per placement, building that
  /// placement's items; it reads their clicks as anywhere. The leading items are in `id.key(0)`,
  /// the trailing ones in `id.key(1)`.
  pub fn toolbar(&mut self, mut content: impl FnMut(&mut Self)) -> Response {
    self.st.toolbar = true;
    let (id, h) = (self.take_id(), self.toolbar_height());
    let buttons = self.titlebar().and_then(|b| b.buttons);
    // On the window's buttons' middle, past them; else on the band's, from its end.
    let (lead, mid) = match buttons {
      Some(b) => (b.x + b.w + AFTER_BUTTONS, b.y + b.h / 2.0),
      None => (LEAD, h / 2.0),
    };

    let pads = [(2.0 * mid - h).max(0.0), TRAIL, (h - 2.0 * mid).max(0.0), lead];
    let band = Style::new().w(grow()).h(px(h)).pads(pads).align(Align::Start, Align::Center);
    let group = Style::new().gap(GAP).align(Align::Start, Align::Center);
    // Only a band beside the window's buttons moves the window and takes the pointer: under the
    // OS's title bar or in full screen it lets the content have it.
    if buttons.is_some() {
      self.drags_window();
    }

    self.next_id = Some((id, false));
    let ui = self.float(Float::parent().z(1)).control_size(ControlSize::Large);
    ui.row(band, |ui| {
      for (i, p) in [ToolbarItemPlacement::Navigation, ToolbarItemPlacement::PrimaryAction]
        .into_iter()
        .enumerate()
      {
        if i == 1 {
          ui.spacer();
        }

        let was = ui.set_slot(Some(Building(p)));
        ui.key(i).row(group, &mut content);
        ui.set_slot(was);
      }
    })
  }

  /// An item of the toolbar being built ([Ui::toolbar]) at `placement`: what `content` builds,
  /// an icon button as a rule, with the items before it at that end. Outside a toolbar, nothing.
  pub fn toolbar_item(&mut self, placement: ToolbarItemPlacement, content: impl FnOnce(&mut Self)) {
    if self.slot::<Building>().is_some_and(|b| b.0 == placement) {
      content(self);
    }
  }

  /// The height of the band a toolbar takes over the top of the window ([Ui::toolbar]): the title
  /// bar's under a unified window's, at least [TOOLBAR_HEIGHT]. Content leaves it free at its
  /// top, or draws only its ground there.
  pub fn toolbar_height(&self) -> f32 {
    self.titlebar().map_or(TOOLBAR_HEIGHT, |b| b.height.max(TOOLBAR_HEIGHT))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::App;
  use crate::harness::{Harness, mac_titlebar};
  use crate::scene::{Point, Rect, Size};
  use crate::ui::{Cursor, Id};
  use crate::{Icon, TitleBar};

  /// A toolbar of a toggle at the leading end and a button at the trailing one, built in the
  /// other order, and a count of the toggle's clicks.
  #[derive(Default)]
  struct Bar(u32);

  impl App for Bar {
    fn view(&mut self, ui: &mut Ui) {
      ui.toolbar(|ui| {
        ui.toolbar_item(ToolbarItemPlacement::PrimaryAction, |ui| {
          _ = ui.id("add").icon_button(Icon::X)
        });
        ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
          if ui.id("toggle").icon_button(Icon::SEARCH).clicked {
            self.0 += 1;
          }
        });
        ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
          _ = ui.id("back").icon_button(Icon::RESET)
        });
      });
      // Outside the toolbar an item is nothing.
      ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
        _ = ui.id("stray").icon_button(Icon::X)
      });
    }
  }

  fn bar(titlebar: Option<TitleBar>) -> Harness<Bar> {
    let mut h = Harness::new(Bar::default(), Size::new(640.0, 400.0), 2.0);
    h.set_titlebar(titlebar);
    h.frame();
    h
  }

  fn mid(r: Rect) -> Point {
    Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0)
  }

  #[test]
  fn under_macos_title_bar_the_items_sit_beside_the_traffic_lights() {
    let h = bar(Some(mac_titlebar("Notes", false)));
    // The view built a toolbar: the bar has a toolbar's height now, as AppKit gives it.
    let lights = h.rt.titlebar().and_then(|b| b.buttons).unwrap();
    assert_eq!(
      (h.rt.titlebar().unwrap().height, lights),
      (52.0, Rect::new(19.0, 19.0, 60.0, 14.0))
    );
    let (toggle, back) = (h.rect(Id::of("toggle")).unwrap(), h.rect(Id::of("back")).unwrap());
    assert_eq!(toggle.x, lights.x + lights.w + AFTER_BUTTONS, "17 after the zoom button");
    assert_eq!(mid(toggle).y, mid(lights).y, "on the lights' middle");
    assert_eq!(back.x, toggle.x + toggle.w + GAP, "built after it, beside it");
    let add = h.rect(Id::of("add")).unwrap();
    assert_eq!((add.x + add.w, mid(add).y), (640.0 - TRAIL, mid(lights).y), "at the trailing end");
    assert!(h.rect(Id::of("stray")).is_none());
  }

  #[test]
  fn the_items_follow_the_lights_where_the_platform_puts_them() {
    let lights = Rect::new(20.0, 12.0, 52.0, 12.0);
    let h = bar(Some(TitleBar { height: 40.0, title: "", buttons: Some(lights) }));
    let toggle = h.rect(Id::of("toggle")).unwrap();
    assert_eq!((toggle.x, mid(toggle).y), (72.0 + AFTER_BUTTONS, 18.0));
    // In full screen there are none over the content: the items from the band's end.
    let mut h = bar(Some(TitleBar { height: 0.0, title: "", buttons: None }));
    let toggle = h.rect(Id::of("toggle")).unwrap();
    assert_eq!((toggle.x, mid(toggle).y), (LEAD, TOOLBAR_HEIGHT / 2.0));
    // Nor does the band move a window in full screen, or take the presses of the content under it.
    assert!(!h.rt.drags_window(Point::new(320.0, 26.0)));
    h.move_to(320.0, 26.0);
    h.frame();
    assert!(h.rt.ui.hover().is_empty());
  }

  #[test]
  fn the_empty_bar_moves_the_window_and_the_items_take_the_pointer() {
    let mut h = bar(Some(mac_titlebar("Notes", false)));
    let toggle = mid(h.rect(Id::of("toggle")).unwrap());
    let add = mid(h.rect(Id::of("add")).unwrap());
    assert!(!h.rt.drags_window(toggle) && !h.rt.drags_window(add), "the items are clicked");
    for p in [Point::new(90.0, 26.0), Point::new(320.0, 10.0), Point::new(320.0, 45.0)] {
      assert!(h.rt.drags_window(p), "the bar at {p:?} moves the window");
    }

    assert!(!h.rt.drags_window(Point::new(320.0, 60.0)), "under the bar, the content");
    h.move_to(toggle.x, toggle.y);
    h.frame();
    assert_eq!(h.rt.cursor(), Cursor::Pointer);
    h.click(toggle.x, toggle.y);
    assert_eq!(h.app().0, 1);
  }

  #[test]
  fn under_the_os_title_bar_the_band_is_a_row_that_moves_nothing() {
    let mut h = bar(None);
    let toggle = h.rect(Id::of("toggle")).unwrap();
    assert_eq!((toggle.x, mid(toggle).y), (LEAD, TOOLBAR_HEIGHT / 2.0));
    assert!(!h.rt.drags_window(Point::new(320.0, 26.0)));
    // The band takes no pointer of its own: the content under it has it.
    h.move_to(320.0, 26.0);
    h.frame();
    assert!(h.rt.ui.hover().is_empty());
  }
}
