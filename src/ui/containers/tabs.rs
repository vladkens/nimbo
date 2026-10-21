// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tabs: a strip of titles over pages, one page shown.
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

use crate::keys::{Command, Move};
use crate::scene::{alpha, mix};
use crate::ui::{Align, Behavior, Id, Response, Style, Ui, grow, px};

/// A tab strip over pages, one shown (`sel`). Tabs are keyed by title (`id.key(("tab",
/// title))`) and so are pages (`id.key(("page", title))`); a title seen before also takes its
/// count (`("tab", title, 1)` for the second), so repeated titles stay apart. The strip takes
/// focus by Tab, the arrows then move the selection.
pub struct Tabs<'s> {
  sel: &'s mut usize,
  titles: &'s [&'s str],
}

impl<'s> Tabs<'s> {
  pub fn new(sel: &'s mut usize, titles: &'s [&'s str]) -> Self {
    Self { sel, titles }
  }

  /// Builds the strip and the selected page with `page(ui, index)`, keeping the other pages'
  /// state; the strip's response, `changed` when the selection moved.
  pub fn show<'a>(self, ui: &mut Ui<'a>, page: impl FnOnce(&mut Ui<'a>, usize)) -> Response {
    let t = ui.theme();
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
    let strip = Style::new().w(grow()).h(px(t.sizes.tab_h)).bg(t.panel);
    let strip = strip.borders([0.0, 0.0, 1.0, 0.0], t.line).scroll_x();
    let strip = if b.disabled { strip } else { strip.tab_focusable_quiet() };
    let mut r = Response::default();
    // The box of strip and page keyed apart from the strip, which has the id.
    ui.next_id = Some((id.key("box"), false));
    ui.col(Style::new().grow(), |ui| {
      r = ui.with_id(id).row(strip, |ui| {
        for (i, title) in titles.iter().enumerate() {
          let tab = key(i, "tab");
          let on = i == sel;
          let hot = ui.hot(tab, ui.hovered(tab) && !on);
          let (bg, edge) = match on {
            true => (t.bg, [0.0, 1.0, 0.0, 0.0]),
            false => (mix(alpha(t.hover, 0.0), t.hover, hot), [0.0, 1.0, 1.0, 0.0]),
          };

          let s = Style::new()
            .h(grow())
            .min_w(64.0)
            .max_w(220.0)
            .pad_xy(14.0, 0.0)
            .align(Align::Center, Align::Center)
            .bg(bg)
            .borders(edge, t.line);
          // Focused, the selected tab rings itself in place of the strip.
          let s = if on && b.focused { s.border(2.0, t.focus) } else { s };
          ui.with_id(tab).row(b.pointer(s), |ui| {
            ui.text(*title, if on { t.label() } else { t.muted() });
          });
        }
      });
      for i in (0..n).filter(|&i| i != sel) {
        ui.retain(key(i, "page"));
      }

      if sel < n {
        ui.next_id = Some((key(sel, "page"), false));
        ui.col(Style::new().grow().clip(), |ui| page(ui, sel));
      }
    });
    r.changed = b.changed;
    r
  }
}

impl Ui<'_> {
  /// A step from the arrows while `id` has focus, as the platform's commands: -1 for Left or
  /// Up, 1 for Right or Down; other commands go on unused.
  pub(crate) fn arrows(&mut self, id: Id) -> i32 {
    match self.command(id) {
      Some(Command::Move(Move::Left | Move::Up, _)) => -1,
      Some(Command::Move(Move::Right | Move::Down, _)) => 1,
      Some(_) => {
        self.pass(id);
        0
      }
      None => 0,
    }
  }

  /// One of `n` items picked by a click on `item(i)`, or by the arrows while `id` is focused
  /// (from none, the first): tabs, a segmented control. `at` is the picked one; `changed` when
  /// it moved.
  pub fn pick_behavior(
    &mut self,
    id: Id,
    n: usize,
    item: impl Fn(usize) -> Id,
    at: &mut Option<usize>,
  ) -> Behavior {
    if let Some(b) = self.begin(id) {
      return b;
    }

    let was = *at;
    let step = self.arrows(id);
    if step != 0 && n > 0 {
      let i = at.map_or(0, |i| (i as i32 + step).clamp(0, n as i32 - 1));
      *at = Some(i as usize);
    }

    for i in 0..n {
      if self.clicked(item(i)) {
        *at = Some(i);
      }
    }

    Behavior { changed: *at != was, ..self.pointer_state(id) }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::Size;

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

  #[test]
  fn repeated_titles_stay_apart_and_hidden_pages_keep_their_state() {
    let mut h = Harness::new(Same { tab: 0 }, Size::new(300.0, 200.0), 1.0);
    h.frame();
    let page = Id::of(("page", 0usize));
    h.rt.ui.store.raw::<crate::ui::input::ScrollState>(page).offset.y = 50.0;
    h.rt.redraw();
    h.frame();
    // The second "Same" is its own tab; the first page's offset outlives it hidden.
    h.click_id(Id::of("tabs").key(("tab", "Same", 1usize)));
    h.frame();
    assert_eq!(h.app().tab, 1);
    assert!(h.rect(page).is_none());
    h.click_id(Id::of("tabs").key(("tab", "Same")));
    h.frame();
    assert_eq!(h.rt.ui.scroll(page).unwrap().offset.y, 50.0);
  }
}
