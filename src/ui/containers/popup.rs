// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The pop-up button: one option of a few, its list a menu over the button, as macOS draws one.
//!
//! ```ignore
//! ui.id("lang").pop_up(&mut self.lang, &["Auto", "English", "German"]);
//! ```

use crate::keys::{Command, Move};
use crate::scene::{Point, Rect, alpha, mix};
use crate::ui::containers::menu::TypeSelect;
use crate::ui::{Align, Behavior, Id, Response, Style, Ui, grow, px, rigid};

/// A pop-up button over `cur`, an index into `options`, as macOS 26 System Settings draws one:
/// the current option's label 13 in from the frame and a chevron capsule after it, no box.
/// Under the pointer the frame fills with the capsule's colour, which merges into it, and
/// stays filled, a shade darker, while its list is open. A click, Space, Enter or the arrows
/// ([Ui::pop_up_behavior]) open its list over it, the current option checked, highlighted and
/// over the button ([Ui::menu_popup_over]); letters typed while it has focus, open or closed,
/// go to the first option they start ([TypeSelect]). As wide as the current label, or
/// [PopUp::width]; the list is at least as wide as the button and [crate::Sizes::menu_w]. Its
/// items are `id.key(("item", i))`; `changed` when one was picked that wasn't the current.
/// Disabled, its label and chevrons turn [crate::Theme::tertiary] and the capsule fades to half,
/// as System Settings dims one.
pub struct PopUp<'s> {
  cur: &'s mut usize,
  options: &'s [&'s str],
  width: Option<f32>,
  separators: &'s [usize],
}

impl<'s> PopUp<'s> {
  pub fn new(cur: &'s mut usize, options: &'s [&'s str]) -> Self {
    Self { cur, options, width: None, separators: &[] }
  }

  /// A fixed width in points; a label longer than it ends in "…".
  pub fn width(self, w: f32) -> Self {
    Self { width: Some(w), ..self }
  }

  /// A line in the list after each of these options: "System Default", a line, the devices.
  pub fn separators(self, after: &'s [usize]) -> Self {
    Self { separators: after, ..self }
  }

  pub fn show(self, ui: &mut Ui) -> Response {
    let t = ui.theme();
    let id = ui.take_id();
    let options = self.options;
    let mut at = (*self.cur < options.len()).then_some(*self.cur);
    let b = ui.pop_up_behavior(id, options.len(), |i| options[i], &mut at);
    let typed = b.changed && at.is_some();
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
    // Its label's width, or the one given: text beside it wraps instead.
    let frame = frame.w(self.width.map_or(rigid(), px));

    let frame = b.style(frame);
    let label = at.map_or("", |i| options[i]);
    let (text, ink) = match b.disabled {
      true => (t.label().color(t.tertiary), t.tertiary),
      false => (t.label(), t.text),
    };

    let cap = alpha(t.raised, t.raised[3] * if b.disabled { 0.5 } else { 1.0 - hot });
    let mut r = ui.with_id(id).row(frame, |ui| {
      match self.width {
        Some(_) => _ = ui.col(Style::new().w(grow()), |ui| ui.text(label, text)),
        None => ui.text(label, text),
      }

      let d = h - 4.0;
      let cap = Style::new().w(px(d)).h(px(d)).radius(d / 2.0).bg(cap);
      ui.canvas(cap, move |f, r| chevrons(f, r, ink));
    });
    r.clicked = b.clicked;

    // At least as wide as the button, or wider.
    let pop_w = ui.rect(id).map_or(0.0, |r| r.w).max(t.sizes.menu_w);
    let mut pick = None;
    // Disabled while open, it closes.
    let toggle = b.clicked || b.disabled && ui.menu_open(id);
    ui.menu_popup_over(id, toggle, t.popup().min_w(pop_w), at, |ui| {
      for (i, &o) in options.iter().enumerate() {
        let item = id.key(("item", i));
        let ib = ui.menu_item_behavior(item, o);
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
        let mark = Some(i) == at;
        ui.with_id(item).row(ib.pointer(row), |ui| {
          let c = text.color;
          ui.canvas(Style::new().w(px(16.0)).h(px(16.0)), move |f, r| {
            if mark {
              check(f, r, c);
            }
          });
          ui.text(o, text);
        });
        if ib.clicked {
          pick = Some(i);
        }

        // 11 high, from the text column to 16 from the menu's right.
        if self.separators.contains(&i) && i + 1 < options.len() {
          ui.row(Style::new().w(grow()).pads([5.0, 11.0, 5.0, 25.0]), |ui| {
            ui.el(Style::new().w(grow()).h(px(1.0)).bg(t.line));
          });
        }
      }
    });

    if let Some(i) = pick
      && Some(i) != at
    {
      *self.cur = i;
      r.changed = true;
    }

    if let Some(i) = at.filter(|_| typed) {
      *self.cur = i;
      r.changed = true;
    }

    r
  }
}

/// Up and down chevrons centred in `r`, their ink 7.5 by 10.5 points, stroked 1.5.
fn chevrons(f: &mut crate::scene::Frame, r: Rect, c: crate::scene::Color) {
  let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
  let p = |x: f32, y: f32| Point::new(cx + x, cy + y);
  f.polyline(&[p(-3.0, -1.75), p(0.0, -4.5), p(3.0, -1.75)], 1.5, c);
  f.polyline(&[p(-3.0, 1.75), p(0.0, 4.5), p(3.0, 1.75)], 1.5, c);
}

/// A check mark in `r`, a 16-point box.
fn check(f: &mut crate::scene::Frame, r: Rect, c: crate::scene::Color) {
  let p = |x: f32, y: f32| Point::new(r.x + x * r.w / 14.0, r.y + y * r.h / 14.0);
  f.polyline(&[p(3.5, 7.3), p(6.0, 9.8), p(10.6, 4.4)], 1.6, c);
}

impl Ui<'_> {
  /// A [PopUp] as wide as its current option.
  pub fn pop_up(&mut self, cur: &mut usize, options: &[&str]) -> Response {
    PopUp::new(cur, options).show(self)
  }

  /// A pop-up button `id` over one of `n` options labelled `label(i)`, `at` the chosen one: a
  /// button ([Ui::button_behavior]) whose `clicked` (a click, Space, Enter, Up or Down) opens or
  /// closes its list. Letters typed while it has focus and its list is closed choose the first
  /// option starting with them, as NSPopUpButton does ([TypeSelect]): `changed` then.
  pub fn pop_up_behavior<'l>(
    &mut self,
    id: Id,
    n: usize,
    label: impl Fn(usize) -> &'l str,
    at: &mut Option<usize>,
  ) -> Behavior {
    let mut b = self.button_behavior(id);
    if b.disabled {
      return b;
    }

    match self.command(id) {
      Some(Command::InsertNewline | Command::Move(Move::Up | Move::Down, _)) => {
        self.flash(id);
        b.clicked = true;
      }
      Some(_) => self.pass(id),
      None => {}
    }

    let now = self.now();
    let mut ts = std::mem::take(self.state::<TypeSelect>(id));
    if let Some(s) = self.typed(id) {
      match ts.push_str(s, now) {
        true => {
          if let Some(i) = ts.find(n, |i| Some(label(i))) {
            b.changed = *at != Some(i);
            *at = Some(i);
          }
        }
        false => self.pass(id),
      }
    }

    *self.state::<TypeSelect>(id) = ts;
    b
  }
}

#[cfg(test)]
mod tests {
  use keyboard_types::{Modifiers, NamedKey};

  use super::*;
  use crate::app::App;
  use crate::harness::Harness;
  use crate::keys::Key;
  use crate::scene::Size;

  /// A pop-up the app can disable.
  #[derive(Default)]
  struct Pick {
    cur: usize,
    off: bool,
  }

  impl App for Pick {
    fn view(&mut self, ui: &mut Ui) {
      ui.disabled(self.off).id("pick").pop_up(&mut self.cur, &["Red", "Green", "Blue"]);
    }
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
}
