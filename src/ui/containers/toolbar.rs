// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The toolbar: items in a band over the top of the window or of a column, as SwiftUI's `.toolbar`
//! of `ToolbarItem(placement:)`s: navigation at the leading end, the view's actions at the
//! trailing one, a sidebar's toggle by its trailing edge.
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
//! it, as an app's header row: the items [LEAD] from its leading end, nothing moved by it. So
//! is a band lower in the window, built in a container under other content.
//!
//! The band floats over its parent's top (the window's, built at the top level) and draws no
//! ground: the content runs under it, so a sidebar's ground goes up to the window's top as in
//! Notes; content leaves [Ui::toolbar_height] at its top for it. Under a unified window's title
//! bar the band is a layer over the content, as AppKit's title bar is: there the pointer reaches
//! only its items (fields and menus too) and an overlay over it (an open menu); the content under
//! it (rows scrolled under the bar, a ground, a capsule the content draws) gets no hover, cursor
//! or press, and a press there moves the window. The wheel scrolls the content under it all the
//! same. What should take presses in the bar is an item: a chat's avatar and name are a
//! [ToolbarItemPlacement::Principal] or [ToolbarItemPlacement::Navigation] one.
//!
//! A toolbar belongs to the container it is built in, as SwiftUI's `.toolbar` on a
//! `NavigationSplitView`'s column: built in a column it is that column's part of the bar.
//! A navigation split's sidebar column ([crate::Split::navigation], SwiftUI's
//! `NavigationSplitView`; an `HSplitView`'s pane is no such column) has its items at its
//! trailing edge, by the divider, as macOS 26 puts the sidebar toggle (measured on
//! macOS 27 with SwiftUI's: 8 from the column's edge); a column too narrow for them has them
//! right of the window's buttons, so the toggle follows the sidebar as it slides out. With the
//! sidebar hidden, a toolbar at the window's leading edge has them there too.

use crate::scene::Rect;
use crate::ui::{Align, ControlSize, Float, Id, Response, Style, Ui, grow, px};

/// Where a toolbar item sits ([Ui::toolbar_item]), as SwiftUI's `ToolbarItemPlacement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarItemPlacement {
  /// At the leading end, beside the window's buttons: a sidebar toggle, back and forward.
  Navigation,
  /// At the trailing end: the view's main actions, a compose or a share button; where SwiftUI's
  /// apps on macOS show it, though its documentation says the leading edge.
  PrimaryAction,
  /// In the centre of the bar: what the bar is about, a title, a location field, a chat's
  /// avatar and name.
  Principal,
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

/// A navigation split's sidebar column is being built ([crate::Split::navigation]): a toolbar
/// in it is the sidebar's part of the bar, its items at its trailing edge.
pub(crate) struct Sidebar;

impl Ui<'_> {
  /// A toolbar of the items `content` builds ([Ui::toolbar_item]), in a band
  /// [Ui::toolbar_height] high over the top of its parent: the title bar of a unified window, or
  /// a column's part of it (see [crate::ui::containers::toolbar]). Its widgets are
  /// [ControlSize::Large], as a toolbar's controls are (36-point tiles on macOS 27). `content` runs
  /// once per placement, building that placement's items; it reads their clicks as anywhere. The
  /// leading items are in `id.key(0)`, the trailing ones in `id.key(1)`, the principal ones in
  /// `id.key(2).key(0)`, in the room between them (`id.key(2)`): centred in the bar where there is
  /// room, else beside the others and cut short, never over them.
  pub fn toolbar(&mut self, mut content: impl FnMut(&mut Self)) -> Response {
    let (id, h) = (self.take_id(), self.toolbar_height());
    // Where the band was; in a navigation split's sidebar its items trail. Only a band at the
    // window's top is beside its buttons and gives the title bar a toolbar's height; lower down
    // it is the content's header row. Not laid out yet, it is taken as at the top: the view
    // runs again on where it is ([Ui::settle]) before the frame shows.
    let at = self.rect(id).unwrap_or(Rect::new(0.0, 0.0, self.size().w, h));
    self.st.toolbar |= at.y <= 0.0;
    let buttons = self.titlebar().and_then(|b| b.buttons).filter(|_| at.y <= 0.0);
    let column = self.slot::<Sidebar>().is_some();
    // Laid out elsewhere (a column sliding), the view runs again on where it is now.
    self.settle(id);
    // On the window's buttons' middle, past them; else on the band's, from its end.
    let (lead, mid) = match buttons {
      Some(b) => ((b.x + b.w + AFTER_BUTTONS - at.x).max(LEAD), b.y + b.h / 2.0),
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
        // The room between the leading and trailing items, the principal ones in it; a column's
        // items trail, by its divider, the room before them.
        if i == (!column) as usize {
          ui.toolbar_principal(id.key(2), at, group, &mut content);
        }

        let was = ui.set_slot(Some(Building(p)));
        ui.key(i).row(group, &mut content);
        ui.set_slot(was);
      }
    })
  }

  /// The room `slot` between a toolbar's leading and trailing items, `band` wide as of the last
  /// frame, with the principal items in it: in the band's centre where there is room, else as
  /// near it as the other items let them, cut at the room's ends when it is narrower than they
  /// are, never over another item.
  fn toolbar_principal(
    &mut self,
    slot: Id,
    band: Rect,
    group: Style,
    content: impl FnMut(&mut Self),
  ) {
    let w = self.rect(slot.key(0)).map_or(0.0, |r| r.w);
    let off = self.rect(slot).map_or(GAP, |s| {
      let centre = band.x + band.w / 2.0 - s.x - w / 2.0;
      centre.min(s.w - w).max(GAP)
    });
    // Laid out elsewhere (the window resized), the view runs again on where it is now.
    self.settle(slot);
    self.settle(slot.key(0));
    let room = Style::new().grow().min_w(0.0).clip().pads([0.0, 0.0, 0.0, off]);
    self.next_id = Some((slot, false));
    self.row(room.align(Align::Start, Align::Center), |ui| {
      let was = ui.set_slot(Some(Building(ToolbarItemPlacement::Principal)));
      ui.key(0).row(group, content);
      ui.set_slot(was);
    });
    // With principal items, the room ends a gap before the items after it.
    if w > 0.0 {
      self.el(Style::new().w(px(GAP)));
    }
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
  use crate::ui::{Axis, Cursor, Float, Id, List, Split, Style, grow, px};
  use crate::{Event, Icon, Modifiers, Phase, TitleBar};

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

  /// A split of a sidebar `side` of the window wide (none: no sidebar) with its own toolbar of a
  /// toggle and a list of 200 rows under it, scrolled `scroll`; beside it content that draws a
  /// button and a filled capsule in the title bar's row and a button across the bar's bottom
  /// edge, and its toolbar with a title in the centre, a search field and a menu; counts of the
  /// clicks.
  struct Columns {
    side: Option<f32>,
    scroll: Option<f32>,
    toggled: u32,
    pressed: u32,
    titled: u32,
    query: String,
  }

  impl Columns {
    fn content(&mut self, ui: &mut Ui) {
      ui.col(Style::new().grow(), |ui| {
        ui.row(Style::new().h(px(52.0)).pad(12.0).gap(8.0), |ui| {
          ui.id("capsule").el(Style::new().w(px(120.0)).h(px(28.0)).radius(14.0).bg([0.9; 4]));
          if ui.id("info").button("Info").clicked {
            self.pressed += 1;
          }
        });
        let across = Float::parent().offset(300.0, 40.0);
        _ = ui.id("across").float(across).el(Style::new().w(px(60.0)).h(px(24.0)).hit());
        ui.toolbar(|ui| {
          ui.toolbar_item(ToolbarItemPlacement::Principal, |ui| {
            if ui.id("title").button("Title").clicked {
              self.titled += 1;
            }
          });
          ui.toolbar_item(ToolbarItemPlacement::PrimaryAction, |ui| {
            ui.col(Style::new().w(px(120.0)), |ui| {
              _ = ui.id("find").search(&mut self.query, "Search")
            });
          });
          ui.toolbar_item(ToolbarItemPlacement::PrimaryAction, |ui| {
            _ = ui.id("more").menu("More", |ui| _ = ui.menu_item("Copy"))
          });
        });
      });
    }
  }

  impl App for Columns {
    fn view(&mut self, ui: &mut Ui) {
      let Some(side) = self.side else { return self.content(ui) };
      if let Some(y) = self.scroll.take() {
        ui.scroll_to(Id::of("list"), Point::new(0.0, y));
      }

      Split::navigation().initial(side).show(ui.id("split"), |ui, pane| {
        if pane == 1 {
          return self.content(ui);
        }

        ui.col(Style::new().grow().bg([0.5, 0.5, 0.5, 1.0]), |ui| {
          ui.toolbar(|ui| {
            ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
              if ui.id("toggle").icon_button(Icon::SIDEBAR).clicked {
                self.toggled += 1;
              }
            });
          });
          ui.el(Style::new().h(px(52.0)));
          List::new(200, 24.0).style(Style::new().grow()).show(ui.id("list"), |ui, row| {
            ui.text(format_args!("Row {}", row.index), ui.theme().label())
          });
        });
      });
    }
  }

  fn columns(side: Option<f32>) -> Harness<Columns> {
    let (query, titled) = (String::new(), 0);
    let app = Columns { side, scroll: None, toggled: 0, pressed: 0, titled, query };
    let mut h = Harness::new(app, Size::new(640.0, 400.0), 2.0);
    h.set_titlebar(Some(mac_titlebar("Notes", false)));
    h.frame();
    h.frame();
    h
  }

  #[test]
  fn a_sidebars_items_trail_by_its_divider_and_stay_beside_the_lights() {
    let lights = mac_titlebar("", true).buttons.unwrap();
    let after = lights.x + lights.w + AFTER_BUTTONS;
    // A sidebar of a third: the toggle at its trailing edge, on the lights' middle.
    let h = columns(Some(0.35));
    let (t, end) = (h.rect(Id::of("toggle")).unwrap(), h.rect(Id::of("list")).unwrap());
    assert_eq!((t.x + t.w, mid(t).y), (end.x + end.w - TRAIL, mid(lights).y));
    // Narrower than the toggle needs beside the lights: the toggle right of them.
    let h = columns(Some(0.1));
    assert_eq!(h.rect(Id::of("toggle")).unwrap().x, after);
    // The content's toolbar beside the column: its item at the window's end.
    let more = h.rect(Id::of("more")).unwrap();
    assert_eq!(more.x + more.w, 640.0 - TRAIL);
  }

  /// A toolbar in a page's padding, no split's column: its leading items lead.
  struct Padded;

  impl App for Padded {
    fn view(&mut self, ui: &mut Ui) {
      ui.col(Style::new().grow().pads([0.0, 40.0, 0.0, 0.0]), |ui| {
        ui.toolbar(|ui| {
          ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
            _ = ui.id("back").icon_button(Icon::RESET)
          })
        });
      });
    }
  }

  #[test]
  fn only_a_sidebar_column_puts_its_items_at_its_end() {
    let mut h = Harness::new(Padded, Size::new(640.0, 400.0), 2.0);
    h.frame();
    h.frame();
    assert_eq!(h.rect(Id::of("back")).unwrap().x, LEAD);
  }

  /// A header strip, filled, as high as the bar, the toolbar in it.
  struct Strip;

  impl App for Strip {
    fn view(&mut self, ui: &mut Ui) {
      ui.col(Style::new().w(grow()).h(px(52.0)).bg([0.9; 4]), |ui| {
        ui.toolbar(|ui| {
          ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
            _ = ui.id("back").icon_button(Icon::RESET)
          })
        });
      });
    }
  }

  #[test]
  fn the_bars_own_filled_strip_lets_it_move_the_window() {
    let mut h = Harness::new(Strip, Size::new(640.0, 400.0), 2.0);
    h.set_titlebar(Some(mac_titlebar("", false)));
    h.frame();
    assert!(h.drags_window(400.0, 26.0));
  }

  #[test]
  fn rows_scrolled_out_of_view_under_the_bar_let_it_move_the_window() {
    let mut h = columns(Some(0.4));
    h.app_mut().scroll = Some(214.0);
    h.frame();
    h.frame();
    assert!(h.scroll(Id::of("list")).unwrap().offset.y > 200.0);
    assert!(h.drags_window(40.0, 26.0) && h.drags_window(40.0, 10.0));
  }

  #[test]
  fn an_open_menu_takes_the_press_on_the_bar() {
    let mut h = columns(None);
    let more = mid(h.rect(Id::of("more")).unwrap());
    h.click(more.x, more.y);
    h.frame();
    assert!(h.menu_open(Id::of("more")));
    assert!(!h.drags_window(200.0, 26.0), "the menu's backdrop over the bar takes it");
  }

  #[test]
  fn the_bar_is_a_layer_over_the_content() {
    let mut h = columns(Some(0.35));
    // What the content draws in the bar's row, under the bar: no hover, no cursor, no press; a
    // press there moves the window, as on the empty bar over the content and over the sidebar.
    let (capsule, info) = (h.rect(Id::of("capsule")).unwrap(), h.rect(Id::of("info")).unwrap());
    let grip = mid(h.rect(Id::of("split").key("handle")).unwrap());
    let title = h.rect(Id::of("title")).unwrap();
    assert!(capsule.x + 20.0 < title.x, "{capsule:?} {title:?}");
    let left = Point::new(capsule.x + 10.0, mid(capsule).y);
    for p in [left, Point::new(grip.x, 26.0), Point::new(300.0, 6.0)] {
      assert!(h.drags_window(p.x, p.y), "{p:?}");
      h.move_to(p.x, p.y);
      h.frame();
      assert!(h.hover().is_empty() && h.cursor() == Cursor::Arrow, "{p:?}: {:?}", h.hover());
    }

    assert!(h.drags_window(100.0, 26.0));
    let _ = info;
    h.click(left.x, left.y);
    h.frame();
    assert_eq!(h.app().pressed, 0, "nothing of the content's under the bar is clicked");
    // Under the bar's bottom edge the content is the content's again: the divider's cursor.
    h.move_to(grip.x, 120.0);
    h.frame();
    assert_eq!(h.cursor(), Cursor::ResizeX);
    let across = h.rect(Id::of("across")).unwrap();
    assert!(h.drags_window(across.x + 5.0, 45.0) && !h.drags_window(across.x + 5.0, 60.0));
  }

  #[test]
  fn the_principal_items_give_way_to_the_others_in_a_narrow_bar() {
    for w in [1200.0, 640.0, 520.0, 400.0] {
      let (query, titled) = (String::new(), 0);
      let app = Columns { side: Some(0.35), scroll: None, toggled: 0, pressed: 0, titled, query };
      let mut h = Harness::new(app, Size::new(w, 400.0), 2.0);
      h.set_titlebar(Some(mac_titlebar("", false)));
      h.frame();
      h.frame();
      // The title as it shows: cut by the room it is in, its group's parent.
      let t = &h.rt.ui.last;
      let title = t.find(Id::of("title")).unwrap();
      let room = t.nodes[t.nodes[title as usize].parent as usize].parent;
      let shown = t.nodes[title as usize].rect.intersect(t.nodes[room as usize].rect);
      let find = h.rect(Id::of("find")).unwrap();
      assert!(
        shown.x + shown.w <= find.x - GAP + 0.5,
        "{w}: the title to {shown:?}, the field at {find:?}"
      );
      let grip = h.rect(Id::of("split").key("handle")).unwrap();
      assert!(shown.w == 0.0 || shown.x >= mid(grip).x, "{w}: {shown:?}");
      // With room, in the centre of the detail's bar.
      if w > 1000.0 {
        let title = t.nodes[title as usize].rect;
        assert!((mid(title).x - (mid(grip).x + w) / 2.0).abs() < 1.0, "{title:?}");
      }
      // The field's whole width takes its own presses.
      let end = Point::new(find.x + 3.0, mid(find).y);
      assert!(!h.drags_window(end.x, end.y));
      h.click(end.x, end.y);
      h.frame();
      assert_eq!(h.focused(), Some(Id::of("find")), "{w}");
    }
  }

  #[test]
  fn the_bars_items_take_the_pointer_fields_and_menus_too() {
    let mut h = columns(Some(0.35));
    let t = mid(h.rect(Id::of("toggle")).unwrap());
    h.click(t.x, t.y);
    h.frame();
    assert_eq!(h.app().toggled, 1);
    // A search field in the toolbar: no drag, a click focuses it, the text cursor over it.
    let f = mid(h.rect(Id::of("find")).unwrap());
    assert!(!h.drags_window(f.x, f.y));
    h.click(f.x, f.y);
    h.frame();
    assert_eq!(h.focused(), Some(Id::of("find")));
    h.move_to(f.x, f.y);
    h.frame();
    assert_eq!(h.cursor(), Cursor::Text);
    // The principal item, between the detail's other items, on the lights' middle.
    let title = h.rect(Id::of("title")).unwrap();
    assert_eq!(mid(title).y, 26.0);
    h.click(mid(title).x, mid(title).y);
    h.frame();
    assert_eq!(h.app().titled, 1);
  }

  /// What runs on under the bar: a list filling the window below a 52-point inset, its rows
  /// scrolled under the bar, or a filled strip as high as the bar as the content's first row; the
  /// toolbar at the top level.
  struct Under(bool);

  impl App for Under {
    fn view(&mut self, ui: &mut Ui) {
      if self.0 {
        // Its rows scrolled under the bar.
        ui.scroll_to(Id::of("list"), Point::new(0.0, 100.0));
        let list = List::new(100, 24.0).style(Style::new().grow().pads([52.0, 0.0, 0.0, 0.0]));
        list.show(ui.id("list"), |ui, row| {
          ui.text(format_args!("Row {}", row.index), ui.theme().label())
        });
      } else {
        ui.col(Style::new().grow(), |ui| ui.el(Style::new().w(grow()).h(px(52.0)).bg([0.9; 4])));
      }

      ui.toolbar(|ui| {
        ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
          _ = ui.id("back").icon_button(Icon::RESET)
        })
      });
    }
  }

  #[test]
  fn content_running_on_under_the_whole_bar_lets_it_move_the_window() {
    for list in [true, false] {
      let mut h = Harness::new(Under(list), Size::new(640.0, 400.0), 2.0);
      h.set_titlebar(Some(mac_titlebar("", false)));
      h.frame();
      h.frame();
      assert!(h.drags_window(300.0, 26.0) && h.drags_window(300.0, 6.0), "list {list}");
      // The list's rows are there, under the bar, and reach no pointer there; below it they do.
      h.move_to(300.0, 26.0);
      h.frame();
      assert!(h.rt.ui.hover().is_empty(), "{:?}", h.rt.ui.hover());
      h.move_to(300.0, 80.0);
      h.frame();
      assert_eq!(h.rt.ui.hover().len() > 1, list, "{:?}", h.rt.ui.hover());
      let back = mid(h.rect(Id::of("back")).unwrap());
      assert!(!h.drags_window(back.x, back.y));
    }
  }

  /// A list filling the window below a 52-point inset, no scroll set, its rows under the bar.
  struct Wheel;

  impl App for Wheel {
    fn view(&mut self, ui: &mut Ui) {
      let list = List::new(100, 24.0).style(Style::new().grow().pads([52.0, 0.0, 0.0, 0.0]));
      list.show(ui.id("list"), |ui, row| {
        ui.text(format_args!("Row {}", row.index), ui.theme().label())
      });
      ui.toolbar(|ui| {
        ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
          _ = ui.id("back").icon_button(Icon::RESET)
        })
      });
    }
  }

  #[test]
  fn the_wheel_over_the_bars_items_scrolls_the_content_under_them() {
    let mut h = Harness::new(Wheel, Size::new(640.0, 400.0), 2.0);
    h.set_titlebar(Some(mac_titlebar("", false)));
    h.frame();
    h.frame();
    let back = mid(h.rect(Id::of("back")).unwrap());
    let (delta, mods) = (Point::new(0.0, -40.0), Modifiers::empty());
    let (phase, momentum) = (Phase::Changed, Phase::None);
    h.event(Event::Scroll { pos: back, delta, precise: true, phase, momentum, mods });
    h.frame();
    assert!(h.scroll(Id::of("list")).unwrap().offset.y > 0.0);
  }

  /// A 100-point header, then a column with a toolbar in it, lower in the window.
  struct Lower;

  impl App for Lower {
    fn view(&mut self, ui: &mut Ui) {
      ui.el(Style::new().w(grow()).h(px(100.0)));
      ui.col(Style::new().grow(), |ui| {
        ui.toolbar(|ui| {
          ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
            _ = ui.id("back").icon_button(Icon::RESET)
          })
        });
      });
    }
  }

  #[test]
  fn a_toolbar_lower_in_the_window_is_a_header_row() {
    let mut h = Harness::new(Lower, Size::new(640.0, 400.0), 2.0);
    h.set_titlebar(Some(mac_titlebar("", false)));
    h.frame();
    // The title bar keeps its own height: the platform is asked for no toolbar.
    assert!(!h.rt.toolbar());
    assert_eq!(h.rt.titlebar().unwrap().height, 32.0);
    h.frame();
    let back = h.rect(Id::of("back")).unwrap();
    assert_eq!((back.x, mid(back).y), (LEAD, 100.0 + TOOLBAR_HEIGHT / 2.0));
    assert!(!h.drags_window(320.0, 126.0) && !h.drags_window(320.0, 110.0));
  }

  /// A split of two panes, `nav` a navigation split, a vertical split in its leading pane with a
  /// toolbar in its top pane.
  struct Panes(bool);

  impl App for Panes {
    fn view(&mut self, ui: &mut Ui) {
      let split = if self.0 { Split::navigation() } else { Split::new(Axis::Horizontal) };
      split.initial(0.4).show(ui, |ui, pane| {
        if pane == 1 {
          return;
        }

        Split::new(Axis::Vertical).show(ui, |ui, pane| {
          if pane == 0 {
            ui.col(Style::new().grow(), |ui| {
              ui.toolbar(|ui| {
                ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
                  _ = ui.id("back").icon_button(Icon::RESET)
                })
              });
            });
          }
        });
      });
    }
  }

  #[test]
  fn only_a_navigation_splits_sidebar_puts_its_items_at_its_end() {
    // A navigation split's sidebar, through a vertical split in it: at its end.
    let mut h = Harness::new(Panes(true), Size::new(640.0, 400.0), 2.0);
    h.frame();
    h.frame();
    let back = h.rect(Id::of("back")).unwrap();
    assert!(back.x > 200.0, "{back:?}");
    // An HSplitView's leading pane is no sidebar: at its start.
    let mut h = Harness::new(Panes(false), Size::new(640.0, 400.0), 2.0);
    h.frame();
    h.frame();
    assert_eq!(h.rect(Id::of("back")).unwrap().x, LEAD);
  }
}
