// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The picker's menu look, [MenuPickerStyle]: macOS's pop-up button, its options a menu over it;
//! its input is the pop-up's ([PickerConfiguration::pop_up]).
//!
//! ```ignore
//! ui.id("launch").picker(&mut self.launch, &[Tag::new(Launch::Window, "Show the window"), ..]);
//! Picker::new(&mut self.lang).option(0, "System Default").divider().options(&LANGS).show(ui);
//! ```

use crate::scene::{Color, Frame, Point, Rect, alpha, mix};
use crate::ui::{Align, PickerConfiguration, PickerStyle, Style, Ui, grow, px, rigid};

/// macOS 26 System Settings' pop-up button, as SwiftUI's `.menu`, the shell's default: the label
/// 13 in and a chevron capsule after it, no box; hovered or open, the frame fills with the
/// capsule's colour. As wide as the label, up to [crate::Picker::max_width], or
/// [crate::Picker::width]; its menu at least [crate::Sizes::menu_w], the selected option checked.
/// Disabled, [crate::Theme::tertiary] ink and the capsule at half, as System Settings dims one.
#[derive(Clone, Copy, Debug, Default)]
pub struct MenuPickerStyle;

impl PickerStyle for MenuPickerStyle {
  fn make_body(&self, ui: &mut Ui<'_>, mut c: PickerConfiguration<'_>) {
    let t = ui.theme();
    let id = c.id();
    let b = c.pop_up(ui);
    let open = ui.menu_open(id);
    // macOS's own hover, whatever hover_all says: the frame fills, the capsule merges into it.
    let hot = ui.hot(id, b.hovered || open);
    let down = ui.hot(id.key("open"), open);
    let h = t.sizes.control_h;
    let fill = mix(t.raised, t.pressed, down);
    let frame = Style::new()
      .h(px(h))
      .pads([0.0, 3.5, 0.0, 13.0])
      .gap(13.0)
      .align(Align::Start, Align::Center)
      .radius(t.radius)
      .bg(alpha(fill, fill[3] * hot));
    // Its label's width (up to the most given), or the one given: text beside it wraps instead.
    let frame = match (c.width(), c.max_width()) {
      (Some(w), _) => frame.w(px(w)),
      (None, Some(max)) => frame.w(rigid()).max_w(max),
      (None, None) => frame.w(rigid()),
    };

    let at = c.selection();
    let label = at.map_or("", |i| c.label(i));
    let (text, ink) = match b.disabled {
      true => (t.label().color(t.tertiary), t.tertiary),
      false => (t.label(), t.text),
    };

    let cap = alpha(t.raised, t.raised[3] * if b.disabled { 0.5 } else { 1.0 - hot });
    ui.row(frame, |ui| {
      match c.width() {
        Some(_) => _ = ui.col(Style::new().w(grow()), |ui| ui.text(label, text)),
        None => ui.text(label, text),
      }

      let d = h - 4.0;
      let cap = Style::new().w(px(d)).h(px(d)).radius(d / 2.0).bg(cap);
      ui.canvas(cap, move |f, r| chevrons(f, r, ink));
    });

    // At least as wide as the button, or wider.
    let pop_w = ui.rect(id).map_or(0.0, |r| r.w).max(t.sizes.menu_w);
    c.menu(ui, t.popup().min_w(pop_w), |ui, c| {
      for i in 0..c.len() {
        let item = c.item_id(i);
        let ib = c.menu_item(ui, i);
        let (bg, text) = match ib.on {
          true => (t.accent, t.label().color(t.on_accent)),
          false => (alpha(t.accent, 0.0), t.label()),
        };

        // The highlight 5 in from the menu's sides (its padding), its corners concentric.
        let row = Style::new()
          .w(grow())
          .h(px(t.sizes.item_h))
          .pads([0.0, 12.0, 0.0, 6.0])
          .gap(4.0)
          .align(Align::Start, Align::Center)
          .radius((t.sizes.menu_radius - 5.0).max(t.radius))
          .bg(bg);
        let (mark, label) = (Some(i) == at, c.label(i));
        ui.with_id(item).row(ib.pointer(row), |ui| {
          let ink = text.color;
          ui.canvas(Style::new().w(px(16.0)).h(px(16.0)), move |f, r| {
            if mark {
              check(f, r, ink);
            }
          });
          ui.text(label, text);
        });

        // 11 high, from the text column to 16 from the menu's right.
        if c.divider_after(i) {
          ui.row(Style::new().w(grow()).pads([5.0, 11.0, 5.0, 25.0]), |ui| {
            ui.el(Style::new().w(grow()).h(px(1.0)).bg(t.line));
          });
        }
      }
    });
  }
}

/// Up and down chevrons centred in `r`, their ink 7.5 by 10.5 points, stroked 1.5.
fn chevrons(f: &mut Frame, r: Rect, c: Color) {
  let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
  let p = |x: f32, y: f32| Point::new(cx + x, cy + y);
  f.polyline(&[p(-3.0, -1.75), p(0.0, -4.5), p(3.0, -1.75)], 1.5, c);
  f.polyline(&[p(-3.0, 1.75), p(0.0, 4.5), p(3.0, 1.75)], 1.5, c);
}

/// A check mark in `r`, a 16-point box.
pub(super) fn check(f: &mut Frame, r: Rect, c: Color) {
  let p = |x: f32, y: f32| Point::new(r.x + x * r.w / 14.0, r.y + y * r.h / 14.0);
  f.polyline(&[p(3.5, 7.3), p(6.0, 9.8), p(10.6, 4.4)], 1.6, c);
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::{App, Event};
  use crate::keys::Key;
  use crate::scene::Size;
  use crate::ui::{FormRow, FormStyle, Id, Picker, Tag, Theme};
  use crate::{Harness, Modifiers, NamedKey};

  /// A pop-up the app can disable.
  #[derive(Default)]
  struct Pick {
    cur: usize,
    off: bool,
  }

  impl App for Pick {
    fn view(&mut self, ui: &mut Ui) {
      let options = [Tag::new(0, "Red"), Tag::new(1, "Green"), Tag::new(2, "Blue")];
      ui.disabled(self.off).id("pick").picker(&mut self.cur, &options);
    }
  }

  /// A form row with a pop-up showing `value`, at most `max` wide if given.
  struct Long {
    value: &'static str,
    max: Option<f32>,
  }

  impl App for Long {
    fn view(&mut self, ui: &mut Ui) {
      let fs = FormStyle::plates(&ui.theme());
      let mut cur = 0;
      ui.id("row").form_row(&fs, FormRow::new("Microphone"), |ui| {
        let pop = Picker::new(&mut cur).option(0, self.value);
        let pop = match self.max {
          Some(w) => pop.max_width(w),
          None => pop,
        };

        pop.show(ui.id("pick"));
      });
    }
  }

  #[derive(Clone, Copy, Debug, PartialEq)]
  enum Lang {
    Auto,
    En,
    De,
  }

  /// A picker of values with a divider after the first.
  struct Langs(Lang);

  impl App for Langs {
    fn view(&mut self, ui: &mut Ui) {
      let p = Picker::new(&mut self.0).option(Lang::Auto, "System Default").divider();
      p.option(Lang::En, "English").option(Lang::De, "Deutsch").show(ui.id("lang"));
    }
  }

  /// The value picked is written back; a divider draws a line between the options it stands
  /// between, the items counting options only.
  #[test]
  fn a_picker_writes_back_values_with_a_line_at_its_divider() {
    let mut h = Harness::new(Langs(Lang::Auto), Size::new(300.0, 240.0), 1.0);
    let lang = Id::of("lang");
    h.frame();
    h.click_id(lang);
    h.frame();
    let (a, b) = (h.rect(lang.key(("item", 0usize))).unwrap(), h.rect(lang.key(("item", 1usize))));
    let b = b.unwrap();
    assert!(b.y - (a.y + a.h) >= 10.0, "a line between: {a:?} {b:?}");
    let c = h.rect(lang.key(("item", 2usize))).unwrap();
    assert_eq!(c.y, b.y + b.h, "none after");
    h.click_id(lang.key(("item", 2usize)));
    assert_eq!(h.app().0, Lang::De);
  }

  #[test]
  fn a_pop_up_picks_by_letters_and_closes_when_disabled() {
    let mut h = Harness::new(Pick::default(), Size::new(300.0, 200.0), 1.0);
    let pick = Id::of("pick");
    h.frame();
    h.rt.ui.set_focus(Some(pick));
    h.key(Key::Char('b'), Modifiers::empty());
    assert_eq!(h.app().cur, 2);

    // Down opens it on the current option, checked.
    h.key(NamedKey::ArrowDown, Modifiers::empty());
    h.frame();
    h.frame();
    assert!(h.rect(pick.key("menu")).is_some() && h.rect(pick.key(("item", 2usize))).is_some());

    // Disabled while open: it closes and its options can't be picked.
    h.app_mut().off = true;
    h.frame();
    h.frame();
    assert!(h.rect(pick.key("menu")).is_none());
    h.key(NamedKey::Enter, Modifiers::empty());
    assert_eq!(h.app().cur, 2);
  }

  /// The window losing the focus closes an open list, nothing picked, as macOS closes its menus.
  #[test]
  fn a_pop_up_closes_when_the_window_loses_the_focus() {
    let mut h = Harness::new(Pick::default(), Size::new(300.0, 200.0), 1.0);
    let pick = Id::of("pick");
    h.frame();
    h.rt.ui.set_focus(Some(pick));
    h.key(NamedKey::ArrowDown, Modifiers::empty());
    h.frame();
    h.frame();
    assert!(h.rect(pick.key("menu")).is_some());
    h.event(Event::Focus(false));
    h.frame();
    assert!(h.rect(pick.key("menu")).is_none());
    assert_eq!(h.app().cur, 0);
  }

  /// At most 220 wide, a long value ends in "…" there: the row's label keeps its line.
  #[test]
  fn a_long_value_stops_at_its_most() {
    let value = "A microphone with a very long name, as USB devices report theirs";
    let mut h = Harness::new(Long { value, max: Some(220.0) }, Size::new(500.0, 200.0), 1.0);
    h.frame();
    assert_eq!(h.rect(Id::of("pick")).map(|r| r.w), Some(220.0));
    assert!(h.rect(Id::of("row")).unwrap().h < 60.0, "the label on one line");
  }

  /// A value that fits its row is shown whole, wider than the list's least width.
  #[test]
  fn a_value_that_fits_stays_whole() {
    let value = "System Default (MacBook Pro Microphone)";
    let mut h = Harness::new(Long { value, max: None }, Size::new(560.0, 200.0), 1.0);
    h.frame();
    let w = h.rect(Id::of("pick")).unwrap().w;
    assert!(w > Theme::system(h.rt.ui.appearance()).sizes.menu_w, "{w}");
    assert!(h.rect(Id::of("row")).unwrap().h < 60.0, "the label on one line");
  }
}
