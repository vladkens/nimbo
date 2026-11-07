// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The picker: one value of a few, as SwiftUI's `Picker` in the menu style, drawn as macOS draws
//! a pop-up button, its list a menu over the button.
//!
//! ```ignore
//! ui.id("launch").picker(&mut self.launch, &[Tag::new(Launch::Window, "Show the window"), ..]);
//! Picker::new(&mut self.lang).option(0, "System Default").divider().options(&LANGS).show(ui);
//! ```

use crate::keys::{Command, Move};
use crate::scene::{Point, Rect, alpha, mix};
use crate::ui::containers::menu::{MenuPlacement, TypeSelect};
use crate::ui::{Align, Behavior, Id, Response, Role, State, Style, Ui, grow, px, rigid};

/// A picker's option, as SwiftUI's `Text(label).tag(value)`: `const`, so a list of them is a
/// constant: `const LAUNCH: [Tag<Launch>; 2] = [Tag::new(Launch::Window, "Open a window"), ..]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tag<'s, T> {
  pub value: T,
  pub label: &'s str,
}

impl<'s, T> Tag<'s, T> {
  pub const fn new(value: T, label: &'s str) -> Self {
    Self { value, label }
  }
}

/// A picker of `cur` among its options, in their order, a line where a divider is between them,
/// as SwiftUI's `Picker` with `Divider` among its children; as macOS 26 System Settings draws a
/// pop-up button: the current option's label 13 in from the frame and a chevron capsule after
/// it, no box. Under the pointer the frame fills with the capsule's colour, which merges into
/// it, and stays filled, a shade darker, while its list is open. A click, Space, Enter or the
/// arrows ([Ui::pop_up_behavior]) open its list over it, the current option checked,
/// highlighted and over the button ([MenuPlacement::Over]); letters typed while it has focus,
/// open or closed, go to the first option they start ([TypeSelect]). As wide as the current
/// label, up to [Picker::max_width], or [Picker::width]; the list is at least as wide as the
/// button and [crate::Sizes::menu_w]. A `cur` that is none of the options shows no label. Its
/// items are `id.key(("item", i))`, `i` counting options only; `changed` when one was picked that
/// wasn't the current, written back to `cur`. Disabled, its label and chevrons turn
/// [crate::Theme::tertiary] and the capsule fades to half, as System Settings dims one.
pub struct Picker<'s, T> {
  cur: &'s mut T,
  /// Its options, `None` a divider.
  entries: Vec<Option<(T, &'s str)>>,
  width: Option<f32>,
  max_width: Option<f32>,
}

impl<'s, T: PartialEq + Copy> Picker<'s, T> {
  pub fn new(cur: &'s mut T) -> Self {
    Self { cur, entries: Vec::new(), width: None, max_width: None }
  }

  /// An option: `value`, shown as `label`.
  pub fn option(mut self, value: T, label: &'s str) -> Self {
    self.entries.push(Some((value, label)));
    self
  }

  /// An option for each of `options`, as SwiftUI's `ForEach` of tagged texts.
  pub fn options(mut self, options: &[Tag<'s, T>]) -> Self {
    self.entries.extend(options.iter().map(|o| Some((o.value, o.label))));
    self
  }

  /// A line between the options before and after it: "System Default", a line, the devices.
  pub fn divider(mut self) -> Self {
    self.entries.push(None);
    self
  }

  /// A fixed width in points; a label longer than it ends in "…".
  pub fn width(self, w: f32) -> Self {
    Self { width: Some(w), ..self }
  }

  /// At most `w` points wide, as wide as the label up to it: a longer one ends in "…", so a
  /// long value leaves the label beside it its line.
  pub fn max_width(self, w: f32) -> Self {
    Self { max_width: Some(w), ..self }
  }

  pub fn show(self, ui: &mut Ui) -> Response {
    let t = ui.theme();
    let id = ui.take_id();
    let options: Vec<(T, &str)> = self.entries.iter().flatten().copied().collect();
    let labels: Vec<&str> = options.iter().map(|o| o.1).collect();
    // A line after each option a divider follows, but the last.
    let mut lines = Vec::new();
    for (k, e) in self.entries.iter().enumerate() {
      let before = self.entries[..k].iter().flatten().count();
      if e.is_none() && before > 0 && before < options.len() {
        lines.push(before - 1);
      }
    }

    let mut at = options.iter().position(|o| o.0 == *self.cur);
    let b = ui.pop_up_behavior(id, labels.len(), |i| labels[i], &mut at);
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
    // Its label's width (up to the most given), or the one given: text beside it wraps instead.
    let frame = match (self.width, self.max_width) {
      (Some(w), _) => frame.w(px(w)),
      (None, Some(max)) => frame.w(rigid()).max_w(max),
      (None, None) => frame.w(rigid()),
    };

    let frame = b.style(frame);
    let label = at.map_or("", |i| labels[i]);
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
    let place = MenuPlacement::Over { start: at };
    ui.menu_popup(id, toggle, place, t.popup().min_w(pop_w), |ui| {
      for (i, &o) in labels.iter().enumerate() {
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
        ui.a11y(item, Role::MenuItem).state(State::Selected, mark);
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
        if lines.contains(&i) {
          ui.row(Style::new().w(grow()).pads([5.0, 11.0, 5.0, 25.0]), |ui| {
            ui.el(Style::new().w(grow()).h(px(1.0)).bg(t.line));
          });
        }
      }
    });

    if let Some(i) = pick.filter(|&i| Some(i) != at).or(at.filter(|_| typed)) {
      *self.cur = options[i].0;
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
pub(super) fn check(f: &mut crate::scene::Frame, r: Rect, c: crate::scene::Color) {
  let p = |x: f32, y: f32| Point::new(r.x + x * r.w / 14.0, r.y + y * r.h / 14.0);
  f.polyline(&[p(3.5, 7.3), p(6.0, 9.8), p(10.6, 4.4)], 1.6, c);
}

impl Ui<'_> {
  /// A [Picker] of `options` as wide as its current option: SwiftUI's `Picker(selection:)` of
  /// `Text`s tagged with their values.
  pub fn picker<T: PartialEq + Copy>(&mut self, cur: &mut T, options: &[Tag<T>]) -> Response {
    Picker::new(cur).options(options).show(self)
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
    // Its value the current option's label; expanded while open ([Ui::menu_popup]).
    if b.disabled {
      self.a11y(id, Role::PopUpButton).text(at.map_or("", label));
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
    self.a11y(id, Role::PopUpButton).text(at.map_or("", label));
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
      let fs = crate::ui::FormStyle::plates(&ui.theme());
      let mut cur = 0;
      ui.id("row").form_row(&fs, crate::ui::FormRow::new("Microphone"), |ui| {
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
    assert!(w > h.rt.ui.theme.sizes.menu_w, "{w}");
    assert!(h.rect(Id::of("row")).unwrap().h < 60.0, "the label on one line");
  }
}
