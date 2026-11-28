// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tabs: a tab view, as macOS's: segmented tabs over a box with the selected one's page.
//!
//! ```ignore
//! Tabs::new(&mut self.tab, &["Files", "History"]).show(ui.id("tabs"), |ui, i| match i {
//!   0 => self.files(ui),
//!   _ => self.history(ui),
//! });
//! ```
//!
//! Only the selected page is built; the others keep their state (scroll offsets, selections)
//! by being retained ([Ui::retain]). The behaviour is [Ui::pick_behavior].

use crate::scene::{Frame, Quad, Rect, mix};
use crate::ui::widgets::{segmented_frame, segments};
use crate::ui::{Axes, Response, Style, Ui, grow, px};

/// The box's corners, as a form's plates'.
const RADIUS: f32 = 12.0;

/// A tab view, as SwiftUI's `TabView` on macOS (AppKit's `NSTabView`): the tabs as a segmented
/// control centred on the top edge of a box that holds the selected page (`sel`), padded 12.
/// Tabs are keyed by title (`id.key(("tab", title))`) and so are pages (`id.key(("page",
/// title))`); a title seen before also takes its count (`("tab", title, 1)` for the second), so
/// repeated titles stay apart. The tabs take focus by Tab, the arrows then move the selection;
/// too many for the width scroll sideways. [Ui::disabled] dims the tabs as a disabled control.
pub struct Tabs<'s> {
  sel: &'s mut usize,
  titles: &'s [&'s str],
}

impl<'s> Tabs<'s> {
  pub fn new(sel: &'s mut usize, titles: &'s [&'s str]) -> Self {
    Self { sel, titles }
  }

  /// Builds the tabs and the selected page with `page(ui, index)`, keeping the other pages'
  /// state; the tabs' response, `changed` when the selection moved.
  pub fn show<'a>(self, ui: &mut Ui<'a>, page: impl FnOnce(&mut Ui<'a>, usize)) -> Response {
    let (t, c) = (ui.theme(), ui.controls_theme());
    let id = ui.take_id();
    let (n, titles) = (self.titles.len(), self.titles);
    let key = move |i: usize, what: &str| {
      let title = titles[i];
      match titles[..i].iter().filter(|t| **t == title).count() {
        0 => id.key((what, title)),
        k => id.key((what, title, k)),
      }
    };
    let mut at = Some(*self.sel);
    let b = ui.pick_behavior(id, n, |i| key(i, "tab"), &mut at);
    *self.sel = at.unwrap_or(*self.sel).min(n.saturating_sub(1));
    let sel = *self.sel;
    // The tab picked into view in a strip too narrow for them all.
    if b.changed
      && let (Some(r), Some(sc)) = (ui.rect(key(sel, "tab")), ui.scroll(id))
    {
      ui.reveal(id, Rect::new(r.x - sc.viewport.x + sc.offset.x, 0.0, r.w, r.h));
    }

    // The control opaque over the box's edge, dimmed or not: on the ground, its fill over it.
    let frame = segmented_frame(&c);
    let [r, g, bl, a] = frame.bg;
    let back = Style::new().radius(frame.radii[0]).bg(t.bg);
    let frame = frame.bg(mix(t.bg, [r, g, bl, 1.0], a)).scroll(Axes::HORIZONTAL);
    let frame = ui.dim_style(frame, b.disabled);

    // The box from the tabs' middle down.
    let h = c.sizes.control_h;
    let (fill, edge) = (t.plate, t.line);
    let paint = move |f: &mut Frame, r: Rect| {
      let r = Rect::new(r.x, r.y + h / 2.0, r.w, r.h - h / 2.0);
      f.quad(Quad::new(r, fill).radius(RADIUS).border(1.0, edge));
    };

    let mut resp = Response::default();
    // The box of tabs and page keyed apart from the tabs, which have the id.
    let boxed = ui.with_id(id.key("box")).allows_hit_testing(false);
    boxed.canvas_col(Style::new().grow(), paint, |ui| {
      ui.row(Style::new().w(grow()).h(px(h)).center(), |ui| {
        ui.row(back, |ui| {
          resp = ui.control(&b).row(frame, |ui| {
            segments(
              ui,
              &c,
              &b,
              n,
              Some(sel),
              |i| key(i, "tab"),
              |ui, i, text| {
                ui.text(titles[i], text.nowrap());
              },
            );
          });
        });
      });
      for i in (0..n).filter(|&i| i != sel) {
        ui.retain(key(i, "page"));
      }

      if sel < n {
        let page_box = Style::new().grow().pads([8.0, 12.0, 12.0, 12.0]).clip();
        ui.with_id(key(sel, "page")).allows_hit_testing(false).col(page_box, |ui| page(ui, sel));
      }
    });
    resp.changed = b.changed;
    resp
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::Harness;
  use crate::app::App;
  use crate::scene::Size;
  use crate::ui::input::ScrollState;
  use crate::ui::{Id, Theme};

  /// Two tabs of the same title, each page a scroll area.
  struct Same {
    tab: usize,
  }

  impl App for Same {
    fn view(&mut self, ui: &mut Ui) {
      Tabs::new(&mut self.tab, &["Same", "Same"]).show(ui.id("tabs"), |ui, i| {
        ui.id(("page", i)).scroll_area(Style::new().grow(), |ui| {
          ui.el(Style::new().w(px(10.0)).h(px(1000.0)));
        });
      });
    }
  }

  /// Tabs with no name: the strip takes the id by place, its box another.
  struct Plain(usize);

  impl App for Plain {
    fn view(&mut self, ui: &mut Ui) {
      Tabs::new(&mut self.0, &["A", "B"]).show(ui, |ui, _| _ = ui.el(Style::new().grow()));
    }
  }

  /// Two tabs filling the window, disabled when `.0`.
  struct Two(bool);

  impl App for Two {
    fn view(&mut self, ui: &mut Ui) {
      let mut sel = 0;
      let tabs = Tabs::new(&mut sel, &["A", "B"]);
      tabs.show(ui.disabled(self.0).id("tabs"), |ui, _| _ = ui.el(Style::new().grow()));
    }
  }

  /// The tabs sit centred at the top, the page under them; disabled, they dim as a disabled
  /// control.
  #[test]
  fn the_tabs_sit_on_the_box_and_dim_when_disabled() {
    let mut h = Harness::new(Two(false), Size::new(300.0, 200.0), 1.0);
    h.frame();
    let tabs = Id::of("tabs");
    let (strip, page) = (h.rect(tabs).unwrap(), h.rect(tabs.key(("page", "A"))).unwrap());
    assert!((strip.x + strip.w / 2.0 - 150.0).abs() <= 1.0, "centred: {strip:?}");
    assert_eq!(strip.y, 0.0);
    assert!(page.y >= strip.y + strip.h && page.y + page.h <= 200.0, "{page:?}");
    let opacity = |h: &Harness<Two>| h.rt.ui.last.get(tabs).unwrap().style.opacity;
    assert_eq!(opacity(&h), 1.0);

    h.app_mut().0 = true;
    h.redraw();
    h.frame();
    assert_eq!(opacity(&h), Theme::system(h.rt.ui.appearance()).disabled);
  }

  #[test]
  fn unnamed_tabs_build_without_duplicate_ids() {
    // Duplicate ids panic in a debug build's layout.
    let mut h = Harness::new(Plain(0), Size::new(300.0, 200.0), 1.0);
    h.frame();
    let strip = Id::ROOT.nth(0);
    h.click_id(strip.key(("tab", "B")));
    h.frame();
    assert_eq!(h.app().0, 1);
  }

  /// Ten tabs in a strip too narrow for them.
  struct Many(usize);

  impl App for Many {
    fn view(&mut self, ui: &mut Ui) {
      const T: [&str; 10] =
        ["Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine"];
      Tabs::new(&mut self.0, &T).show(ui.id("tabs"), |ui, _| _ = ui.el(Style::new().grow()));
    }
  }

  #[test]
  fn the_arrows_bring_the_picked_tab_into_view() {
    let mut h = Harness::new(Many(0), Size::new(200.0, 100.0), 1.0);
    h.frame();
    let tabs = Id::of("tabs");
    h.rt.ui.set_focus(Some(tabs));
    for _ in 0..9 {
      h.key(crate::NamedKey::ArrowRight, crate::Modifiers::empty());
      h.frame();
    }

    assert_eq!(h.app().0, 9);
    h.frame();
    let r = h.rect(tabs.key(("tab", "Nine"))).unwrap();
    assert!(r.x >= 0.0 && r.x + r.w <= 200.5, "{r:?}");
  }

  #[test]
  fn repeated_titles_stay_apart_and_hidden_pages_keep_their_state() {
    let mut h = Harness::new(Same { tab: 0 }, Size::new(300.0, 200.0), 1.0);
    h.frame();
    let page = Id::of(("page", 0usize));
    h.rt.ui.store.raw::<ScrollState>(page).offset[1] = 50.0;
    h.redraw();
    h.frame();

    // The second "Same" is its own tab; the first page's offset outlives it hidden.
    h.click_id(Id::of("tabs").key(("tab", "Same", 1usize)));
    h.frame();
    assert_eq!(h.app().tab, 1);
    assert!(h.rect(page).is_none());
    h.click_id(Id::of("tabs").key(("tab", "Same")));
    h.frame();
    assert_eq!(h.scroll(page).unwrap().offset.y, 50.0);
  }
}
