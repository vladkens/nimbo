// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Menus: a pop-up menu of items over the window, opened by a control (its trigger) or at a
//! point as a context menu ([MenuPlacement::At]), a button that opens one ([Ui::menu]), and
//! the context menu of any element from a [Menu] ([Ui::context_menu]): the platform's own in a
//! window, drawn from the model in the harness.
//!
//! ```ignore
//! ui.id("view").menu("View", |ui| {
//!   if ui.menu_item("Zoom In").clicked { self.zoom += 1 }
//!   ui.disabled(self.zoom == 0).menu_item("Actual Size");
//! });
//! ```
//!
//! The behaviour is [Ui::menu_popup] (open state, placement, keys, type-select) and
//! [Ui::menu_item_behavior] (an item's place, highlight and run), in [crate::ui::behavior];
//! [Ui::menu_item] draws an item from the theme.

use crate::menu::{Item, Menu};
use crate::scene::{Point, alpha};
pub use crate::ui::behavior::{MenuPlacement, TYPE_SELECT, TypeSelect};
use crate::ui::containers::popup::check as check_mark;
use crate::ui::{Align, Button, Id, Response, Role, State, Style, Ui, grow, px};

impl Ui<'_> {
  /// A button that opens a menu of `items` ([Ui::menu_item]) under it: [Ui::menu_popup] in
  /// the theme's popup.
  pub fn menu<R>(&mut self, label: &str, items: impl FnOnce(&mut Self) -> R) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let off = self.take_disabled();
    let r = Button::new(label).show(self.disabled(off).with_id(id));
    // Disabled while open, it closes.
    let toggle = r.clicked || off && self.menu_open(id);
    self.menu_popup(id, toggle, MenuPlacement::Below, t.popup().min_w(160.0), items);
    r
  }

  /// An item of a menu: highlighted in the accent under the pointer or by the menu's keys,
  /// dimmed when disabled.
  pub fn menu_item(&mut self, label: &str) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let b = self.menu_item_behavior(id, label);
    let (bg, text) = match b.on {
      true => (t.accent, t.label().color(t.on_accent)),
      false => (alpha(t.accent, 0.0), t.label()),
    };

    let s = Style::new()
      .w(grow())
      .h(px(t.sizes.item_h))
      .pad_xy(10.0, 0.0)
      .align(Align::Start, Align::Center)
      .radius((t.sizes.menu_radius - 5.0).max(t.radius))
      .bg(bg)
      .opacity(if b.disabled { t.disabled } else { 1.0 });
    let mut r = self.with_id(id).row(b.pointer(s), |ui| ui.text(label, text));
    r.clicked = b.clicked;
    r
  }
}

impl Ui<'_> {
  /// The context menu of the element `id`, built before or after it: a secondary press on it or
  /// inside it ([Ui::context_press]) opens `menu()` at the pointer (the closure runs only then,
  /// and while a drawn one is open); returns the id of the item chosen. In a window the
  /// platform shows its own menu (`Runtime::set_native_menus`), free to go past the
  /// window's edges, and the answer comes in a later pass as [Event::Menu]; in the harness it is
  /// drawn ([Ui::menu_popup] of `id` at the point): a check column, headers in secondary text,
  /// shortcuts at the right, destructive items red, a submenu's items under its label; each item
  /// `id.key(item.id)`.
  pub fn context_menu<A: Clone + 'static>(
    &mut self,
    id: Id,
    menu: impl FnOnce() -> Menu<A>,
  ) -> Option<A> {
    let at = self.context_press(id);
    self.context_menu_at(id, at, menu)
  }

  /// [Ui::context_menu] opened at `at` by a behaviour that saw the press itself (a field's edit
  /// menu, [crate::EditMenu]).
  pub fn context_menu_at<A: Clone + 'static>(
    &mut self,
    id: Id,
    at: Option<Point>,
    menu: impl FnOnce() -> Menu<A>,
  ) -> Option<A> {
    if self.native_menus() {
      return self.native_menu(id, at, menu);
    }

    let t = self.theme();
    let menu = (at.is_some() || self.menu_open(id)).then(|| menu().tagged());
    let (check, mut chosen) = (menu.as_ref().is_some_and(|m| checks(&m.0.items)), None);
    let place = MenuPlacement::At(at.unwrap_or_default());
    self.menu_popup(id, at.is_some(), place, t.popup().min_w(160.0), |ui| {
      for it in menu.iter().flat_map(|m| &m.0.items) {
        ui.model_item(id, it, check, true, &mut chosen);
      }
    });
    chosen.and_then(|i| menu.and_then(|m| m.1.get(i).cloned()))
  }

  /// An item of a drawn context menu from its model: a divider, a section's title, a submenu's
  /// label with its items under it, or a button or toggle `id.key(("item", n))` of action
  /// number `n` with its check column, its label (red when destructive) and its shortcut;
  /// `chosen` takes its number when clicked. Under a disabled submenu (`on` false) none is.
  fn model_item(
    &mut self,
    id: Id,
    it: &Item<usize>,
    check: bool,
    on: bool,
    chosen: &mut Option<usize>,
  ) {
    let t = self.theme();
    let row = |left: f32| {
      let s = Style::new().w(grow()).h(px(t.sizes.item_h)).pads([0.0, 10.0, 0.0, left]);
      s.gap(4.0).align(Align::Start, Align::Center)
    };
    if let Item::Divider = it {
      let line = Style::new().w(grow()).h(px(1.0)).bg(t.line);
      self.row(Style::new().w(grow()).pads([5.0, 10.0, 5.0, 10.0]), |ui| _ = ui.el(line));
      return;
    }

    // Under the check column, the item's text.
    let Some(&n) = it.action() else {
      self.row(row(if check { 26.0 } else { 10.0 }), |ui| ui.text(it.label(), t.section()));
      if let Item::Submenu { menu, enabled, .. } = it {
        for sub in &menu.items {
          self.model_item(id, sub, check, on && *enabled, chosen);
        }
      }

      return;
    };

    // Named by its label alone, its shortcut apart as in the OS's menus.
    let (item, label) = (id.key(("item", n)), it.label());
    let off = !(on && it.is_enabled());
    let b = self.a11y_label(label).disabled(off).menu_item_behavior(item, label);
    let (bg, ink) = match b.on {
      true => (t.accent, t.on_accent),
      false => (alpha(t.accent, 0.0), if it.is_destructive() { t.danger } else { t.text }),
    };

    let s = row(if check { 6.0 } else { 10.0 })
      .radius((t.sizes.menu_radius - 5.0).max(t.radius))
      .bg(bg)
      .opacity(if b.disabled { t.disabled } else { 1.0 });
    let (on, key) = (it.is_on(), it.key_label());
    self.a11y(item, Role::MenuItem).state(State::Selected, on);
    self.with_id(item).row(b.pointer(s), |ui| {
      if check {
        ui.canvas(Style::new().w(px(16.0)).h(px(16.0)), move |f, r| {
          if on {
            check_mark(f, r, ink);
          }
        });
      }

      ui.text(label, t.label().color(ink));
      if !key.is_empty() {
        ui.el(Style::new().w(grow()).min_w(20.0));
        ui.text(&key, t.label().color(if b.on { ink } else { t.muted }).nowrap());
      }
    });
    if b.clicked {
      *chosen = Some(n);
    }
  }
}

/// Whether any of `items` has a check mark to show, in a submenu too: the menu has a column
/// for them then.
fn checks(items: &[Item<usize>]) -> bool {
  items.iter().any(|it| match it {
    Item::Toggle { .. } => true,
    Item::Submenu { menu, .. } => checks(&menu.items),
    _ => false,
  })
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use keyboard_types::{Modifiers, NamedKey};

  use super::*;
  use crate::app::{App, Event, MouseButton, Phase};
  use crate::harness::Harness;
  use crate::scene::Size;

  /// A menu inside a scroll area over tall content; its items count their runs.
  #[derive(Default)]
  struct Menus {
    runs: [usize; 3],
    off: bool,
  }

  impl App for Menus {
    fn view(&mut self, ui: &mut Ui) {
      ui.id("area").scroll_area(Style::new().grow(), |ui| {
        ui.el(Style::new().w(px(10.0)).h(px(150.0)));
        ui.disabled(self.off).id("menu").menu("Menu", |ui| {
          for (i, name) in ["one", "two", "three"].into_iter().enumerate() {
            if ui.disabled(i == 1).id(name).menu_item(name).clicked {
              self.runs[i] += 1;
            }
          }
        });
        ui.el(Style::new().w(px(10.0)).h(px(2000.0)));
      });
    }
  }

  #[test]
  fn the_wheel_scrolls_nothing_under_an_open_menu() {
    let mut h = Harness::new(Menus::default(), Size::new(400.0, 300.0), 1.0);
    h.frame();
    let (menu, area) = (Id::of("menu"), Id::of("area"));
    let wheel = |h: &mut Harness<Menus>, x: f32, y: f32| {
      let (pos, delta, mods) = (Point::new(x, y), Point::new(0.0, -100.0), Modifiers::empty());
      h.event(Event::Scroll {
        pos,
        delta,
        precise: false,
        phase: Phase::None,
        momentum: Phase::None,
        mods,
      });
      h.wait(Duration::from_millis(500));
    };

    // A wheel's momentum still running as it opens stops.
    wheel(&mut h, 350.0, 250.0);
    let at = h.rt.ui.scroll(area).unwrap().offset.y;
    assert!(at > 0.0);
    h.click_id(menu);
    h.frame();
    h.frame();
    assert_eq!(h.focused(), Some(menu.key("menu")));

    // Over the open menu, or the cover around it: nothing under them scrolls.
    let r = h.rect(Id::of("one")).unwrap();
    wheel(&mut h, r.x + 5.0, r.y + 5.0);
    wheel(&mut h, 350.0, 250.0);
    assert_eq!(h.rt.ui.scroll(area).unwrap().offset.y, at);

    // The pointer highlights an item; Down skips the disabled one; Enter runs it.
    h.move_to(r.x + 5.0, r.y + 5.0);
    h.frame();
    h.key(NamedKey::ArrowDown, Modifiers::empty());
    h.key(NamedKey::Enter, Modifiers::empty());
    h.frame();
    assert_eq!(h.app().runs, [0, 0, 1]);
    assert!(h.rect(menu.key("menu")).is_none() && h.focused().is_none());

    // Its button disabled while it is open, it closes.
    h.click_id(menu);
    h.frame();
    h.frame();
    h.app_mut().off = true;
    h.frame();
    h.frame();
    assert!(h.rect(menu.key("menu")).is_none());
  }

  /// A menu with a label longer than its window is wide.
  struct Wide;

  impl App for Wide {
    fn view(&mut self, ui: &mut Ui) {
      ui.id("m").menu("Menu", |ui| {
        ui.menu_item(
          "A label far longer than this narrow window is wide, which ends in an ellipsis",
        );
      });
    }
  }

  #[test]
  fn a_menu_is_no_wider_than_its_window() {
    let mut h = Harness::new(Wide, Size::new(200.0, 200.0), 1.0);
    h.frame();
    h.click_id(Id::of("m"));
    h.frame();
    h.frame();
    let m = h.rect(Id::of("m").key("menu")).unwrap();
    assert!(m.x >= 0.0 && m.x + m.w <= 200.0, "{m:?}");
  }

  /// An element whose context menu has a disabled submenu, its items "a" and "b".
  #[derive(Default)]
  struct Sub {
    chosen: Option<String>,
  }

  impl App for Sub {
    fn view(&mut self, ui: &mut Ui) {
      let el = ui.id("el").el(Style::new().w(px(100.0)).h(px(40.0)));
      let items = vec![Item::button("a", "A"), Item::button("b", "B")];
      let sub = Item::submenu("Sub", Menu::new(items)).enabled(false);
      if let Some(item) = ui.context_menu(el.id, || Menu::new(vec![Item::button("x", "X"), sub])) {
        self.chosen = Some(item.to_owned());
      }
    }
  }

  #[test]
  fn a_disabled_submenus_items_are_disabled_when_drawn() {
    let mut h = Harness::new(Sub::default(), Size::new(300.0, 300.0), 1.0);
    h.frame();
    h.right_click(50.0, 20.0);
    h.frame();
    h.frame();
    let dump = h.a11y();
    assert!(dump.contains("menu item \"A\" disabled"), "{dump}");
    assert!(dump.contains("menu item \"X\"") && !dump.contains("\"X\" disabled"), "{dump}");
    h.click_menu_item("A");
    h.frame();
    assert_eq!(h.app().chosen, None);
  }

  /// A button inside an element with a menu; its clicks counted.
  #[derive(Default)]
  struct Boxed {
    clicks: usize,
  }

  impl App for Boxed {
    fn view(&mut self, ui: &mut Ui) {
      let r = ui.id("box").row(Style::new().w(px(200.0)).h(px(60.0)).pad(10.0), |ui| {
        if ui.id("b").button("B").clicked {
          self.clicks += 1;
        }
      });
      ui.context_menu(r.id, || Menu::new(vec![Item::button("x", "X")]));
    }
  }

  #[test]
  fn a_control_click_on_a_button_is_no_click_on_the_mac() {
    use crate::keys::Os;
    let mut h = Harness::new(Boxed::default(), Size::new(300.0, 300.0), 1.0);
    h.set_os(Os::Mac);
    h.native_menus();
    h.frame();

    let r = h.rect(Id::of("b")).unwrap();
    let pos = Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
    let button = MouseButton::Left;
    h.move_to(pos.x, pos.y);
    for mods in [Modifiers::CONTROL, Modifiers::CONTROL | Modifiers::SHIFT] {
      h.event(Event::MouseDown { pos, button, clicks: 1, mods });
      h.frame();
      h.event(Event::MouseUp { pos, button, mods });
      h.frame();
      assert!(h.context_menu(None).is_some(), "{mods:?}: the box's menu");
      h.frame();
      assert_eq!(h.app().clicks, 0, "{mods:?}");
    }

    h.click(pos.x, pos.y);
    h.frame();
    assert_eq!(h.app().clicks, 1, "a plain click still clicks");
  }

  /// An element with a menu, built only while `shown`, its state kept meanwhile.
  struct Gone {
    shown: bool,
  }

  impl App for Gone {
    fn view(&mut self, ui: &mut Ui) {
      if !self.shown {
        // Kept as a hidden tab keeps its page.
        ui.retain(Id::of("el"));
        return;
      }

      let el = ui.id("el").el(Style::new().w(px(100.0)).h(px(40.0)));
      ui.context_menu(el.id, || Menu::new(vec![Item::button("x", "X")]));
    }
  }

  #[test]
  fn a_native_menu_answered_while_its_element_is_gone_is_closed() {
    let mut h = Harness::new(Gone { shown: true }, Size::new(300.0, 300.0), 1.0);
    h.native_menus();
    h.frame();
    h.right_click(50.0, 20.0);
    h.frame();
    assert!(h.rt.ui.menu_open(Id::of("el")));

    let (on, ..) = h.rt.take_context_menu().unwrap();
    h.app_mut().shown = false;
    h.frame();
    h.event(Event::Menu { on, item: None });
    h.frame();
    h.app_mut().shown = true;
    h.frame();
    assert!(!h.rt.ui.menu_open(Id::of("el")));
  }
}
