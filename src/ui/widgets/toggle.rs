// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The switch: a pill knob sliding over a track, grey off and the accent on, in the rows' size
//! or the regular one ([crate::Sizes::switch], [crate::Sizes::switch_large]).

use crate::scene::mix;
use crate::ui::theme::ControlSize;
use crate::ui::{Align, Behavior, Id, Response, Shadow, Style, Transition, Ui, px, rigid};

/// How fast the knob slides: across in 140 ms, eased.
const SLIDE: Transition = Transition::smooth(1000.0 / 140.0);

impl Ui<'_> {
  /// A check box or switch over `on`, flipped by a click or Space while focused: `changed` when
  /// it was, `on` its value after.
  pub fn toggle_behavior(&mut self, id: Id, on: &mut bool) -> Behavior {
    let mut b = self.button_behavior(id);
    if b.clicked {
      *on = !*on;
    }

    (b.changed, b.on) = (b.clicked, *on);
    b
  }

  /// A switch with a label, toggling `on`, in the rows' size ([crate::Sizes::switch]).
  pub fn toggle(&mut self, on: &mut bool, label: &str) -> Response {
    self.toggle_sized(on, label, ControlSize::Medium)
  }

  /// [Ui::toggle] in `size`: [ControlSize::Large] is the regular switch
  /// ([crate::Sizes::switch_large]) of a header row by a big icon. A pill knob, as far in from
  /// the track's end as from its top, slides over the track, grey off and the accent on; a soft
  /// ring and a small drop shadow under it.
  pub fn toggle_sized(&mut self, on: &mut bool, label: &str, size: ControlSize) -> Response {
    let t = self.theme();
    let id = self.take_id();
    let b = self.toggle_behavior(id, on);
    let k = self.animate(id.key("knob"), *on as u8 as f32, SLIDE);
    let hot = self.ctl_hot(id, b.hovered);
    let rest = mix(t.track, t.track_hover, hot);
    let sw = if size == ControlSize::Large { t.sizes.switch_large } else { t.sizes.switch };
    let ([w, h], [kw, kh]) = (sw.track, sw.knob);
    let inset = (h - kh) / 2.0;
    let row = Style::new().w(rigid()).gap((h / 3.0).round()).align(Align::Start, Align::Center);
    let row = self.control(row.radius(h / 2.0), &b);
    let mut r = self.with_id(id).row(row, |ui| {
      let track = Style::new().w(px(w)).h(px(h)).pad(inset).radius(h / 2.0);
      ui.row(track.bg(mix(rest, mix(t.accent, t.accent_hover(), hot), k)), |ui| {
        ui.el(Style::new().w(px((w - 2.0 * inset - kw) * k)));
        let knob = Style::new().w(px(kw)).h(px(kh)).radius(kh / 2.0).bg(t.knob);
        let (ring, drop) = ([0.0, 0.0, 0.0, 0.04], [0.0, 0.0, 0.0, 0.14]);
        ui.el(knob.border(0.5, ring).shadow(Shadow::new(0.5, 2.5, drop)));
      });
      if !label.is_empty() {
        ui.text(label, t.label());
      }
    });
    (r.clicked, r.changed) = (b.clicked, b.changed);
    r
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use keyboard_types::{Modifiers, NamedKey};

  use crate::app::App;
  use crate::harness::Harness;
  use crate::keys::Key;
  use crate::scene::Size;
  use crate::ui::{Id, Ui};

  #[derive(Default)]
  struct Switch {
    on: bool,
    flips: usize,
  }

  impl App for Switch {
    fn view(&mut self, ui: &mut Ui) {
      if ui.id("s").toggle(&mut self.on, "Wi-Fi").changed {
        self.flips += 1;
      }
    }
  }

  #[test]
  fn a_switch_flips_by_click_and_space_and_slides() {
    let mut h = Harness::new(Switch::default(), Size::new(200.0, 100.0), 1.0);
    h.frame();
    assert!(h.click_id(Id::of("s")));
    assert_eq!((h.app().on, h.app().flips), (true, 1));
    // The knob slides there in under 170 ms.
    let r = h.rect(Id::of("s")).unwrap();
    let at = |h: &mut Harness<Switch>| h.render().pixel(r.x + 8.0, r.y + r.h / 2.0);
    let before = at(&mut h);
    h.wait(Duration::from_millis(170));
    assert_ne!(at(&mut h), before);
    // Space while focused flips it back.
    h.key(NamedKey::Tab, Modifiers::empty());
    h.key(Key::Char(' '), Modifiers::empty());
    assert_eq!((h.app().on, h.app().flips), (false, 2));
  }
}
