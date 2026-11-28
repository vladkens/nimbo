// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The toggle's looks from the theme, as SwiftUI's toggle styles ([ToggleStyle]): the switch
//! ([SwitchToggleStyle]), a pill knob sliding over a track in the rows' size or the large one, and
//! the bordered button drawn selected while on ([ButtonToggleStyle]); the check box's is beside
//! ([crate::CheckboxToggleStyle]).

use super::button::{Face, Variant, themed};
use crate::scene::mix;
use crate::ui::{
  Align, ControlSize, Role, Shadow, Style, ToggleConfiguration, ToggleStyle, Transition, Ui, px,
  rigid,
};

/// How fast the knob slides: across in 140 ms, eased.
const SLIDE: Transition = Transition::smooth(1000.0 / 140.0);

/// The switch, as SwiftUI's `.switch`, the shell's default: in the rows' size
/// ([crate::Sizes::switch]), or the large one at [ControlSize::Large]. A pill knob, as far in
/// from the track's end as from its top, with a soft ring and a drop shadow, slides over the
/// track, grey off and the accent on. A switch to assistive technology.
#[derive(Clone, Copy, Debug, Default)]
pub struct SwitchToggleStyle;

/// A toggle as the bordered push button ([crate::BorderedButtonStyle]), as SwiftUI's `.button`:
/// tinted with the accent while on, the label in it; square for an icon alone.
#[derive(Clone, Copy, Debug, Default)]
pub struct ButtonToggleStyle;

impl ToggleStyle for SwitchToggleStyle {
  fn make_body(&self, ui: &mut Ui<'_>, mut c: ToggleConfiguration<'_>) {
    let (t, size) = (ui.controls_theme(), ui.environment_value::<ControlSize>());
    let id = c.id();
    // The check box with the switch subrole.
    ui.a11y(id, Role::Switch);
    let k = ui.animate(id.key("knob"), c.is_on() as u8 as f32, SLIDE);
    let hot = ui.ctl_hot(id, c.is_hovered());
    let rest = mix(t.track, t.track_hover, hot);

    let sw = if size == ControlSize::Large { t.sizes.switch_large } else { t.sizes.switch };
    let ([w, h], [kw, kh]) = (sw.track, sw.knob);
    let inset = (h - kh) / 2.0;
    let row = Style::new().w(rigid()).gap((h / 3.0).round()).align(Align::Start, Align::Center);
    let row = ui.dim_style(row.radius(h / 2.0), !c.is_enabled());

    ui.row(row, |ui| {
      let track = Style::new().w(px(w)).h(px(h)).pad(inset).radius(h / 2.0);
      ui.row(track.bg(mix(rest, mix(t.accent, t.accent_hover(), hot), k)), |ui| {
        ui.el(Style::new().w(px((w - 2.0 * inset - kw) * k)));
        let knob = Style::new().w(px(kw)).h(px(kh)).radius(kh / 2.0).bg(t.knob);
        let (ring, drop) = ([0.0, 0.0, 0.0, 0.04], [0.0, 0.0, 0.0, 0.14]);
        ui.el(knob.border(0.5, ring).shadow(Shadow::new(0.5, 2.5, drop)));
      });
      // An icon draws past the row's opacity: dimmed on its own.
      let s = t.label();
      c.label(ui, s.color(ui.dim(s.color, c.is_icon_only() && !c.is_enabled())));
    });
  }
}

impl ToggleStyle for ButtonToggleStyle {
  fn make_body(&self, ui: &mut Ui<'_>, mut c: ToggleConfiguration<'_>) {
    themed(ui, Face::of_toggle(&c), Variant::Bordered, None, |ui, s| c.label(ui, s));
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use crate::app::App;
  use crate::keys::Key;
  use crate::scene::Size;
  use crate::ui::{
    ButtonToggleStyle, CheckboxToggleStyle, Icon, Id, PlainToggleStyle, Style, Theme, ToggleStyle,
    Ui,
  };
  use crate::{Harness, Modifiers, NamedKey};

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

  /// A toggle in the window's style the view put, else in the shell's.
  struct Window(Option<&'static dyn ToggleStyle>);

  impl App for Window {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_environment(self.0);
      ui.id("t").toggle(&mut true, "Wi-Fi");
    }
  }

  /// The shell puts the switch under the window's style: a switch to assistive technology too.
  #[test]
  fn the_shell_puts_the_switch() {
    let mut h = Harness::new(Window(None), Size::new(200.0, 100.0), 1.0);
    let mut a11y = |s| {
      h.app_mut().0 = s;
      h.redraw();
      h.frame();
      h.a11y()
    };
    assert!(a11y(None).contains("switch \"Wi-Fi\" on"));
    assert!(a11y(Some(&PlainToggleStyle)).contains("check box \"Wi-Fi\" on"));
    assert!(a11y(None).contains("switch \"Wi-Fi\" on"), "back to the shell's");
  }

  /// A button toggle on beside one off, in the light theme.
  struct Picked;

  impl App for Picked {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_theme(Theme::PLATES_LIGHT);
      let ground = Style::new().grow().bg(ui.theme().bg);
      ui.toggle_style(&ButtonToggleStyle).row(ground, |ui| {
        ui.id("on").toggle(&mut true, "On");
        ui.id("off").toggle(&mut false, "Off");
      });
    }
  }

  /// On, a button toggle is tinted with the accent; off, the bordered button's grey.
  #[test]
  fn a_button_toggle_on_takes_the_accent() {
    let mut h = Harness::new(Picked, Size::new(200.0, 60.0), 1.0);
    h.frame();
    let img = h.render();
    let fill = |h: &Harness<Picked>, n: &str| {
      let r = h.rect(Id::of(n)).unwrap();
      img.pixel(r.x + 4.0, r.y + r.h / 2.0)
    };
    let (on, off) = (fill(&h, "on"), fill(&h, "off"));
    assert!(on[2] > on[0] + 20 && off[2] == off[0], "{on:?} on, {off:?} off");
    assert!(h.a11y().contains("check box \"On\" on"), "{}", h.a11y());
  }

  /// A switch and a check box with an icon label, enabled or not.
  struct Icons(bool);

  impl App for Icons {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_theme(Theme::PLATES_LIGHT);
      let ground = Style::new().grow().gap(20.0).bg(ui.theme().bg);
      ui.disabled(self.0).col(ground, |ui| {
        ui.id("s").toggle(&mut true, Icon::GEAR);
        ui.toggle_style(&CheckboxToggleStyle).id("c").toggle(&mut true, Icon::GEAR);
      });
    }
  }

  /// Disabled, an icon label dims as the control does: it draws past the row's opacity.
  #[test]
  fn a_disabled_toggle_dims_its_icon() {
    let mut h = Harness::new(Icons(false), Size::new(200.0, 100.0), 1.0);
    h.frame();
    // The icon's box, the row's end.
    let ink = |h: &mut Harness<Icons>, n: &str| {
      let (r, img) = (h.rect(Id::of(n)).unwrap(), h.render());
      let xs = (r.x + r.w - 15.0) as u32..(r.x + r.w) as u32;
      let ys = (r.y + r.h / 2.0 - 7.0) as u32..(r.y + r.h / 2.0 + 7.0) as u32;
      xs.flat_map(|x| ys.clone().map(move |y| (x, y)))
        .map(|(x, y)| 255 - img.pixel(x as f32, y as f32)[0] as u32)
        .sum::<u32>()
    };
    let on = [ink(&mut h, "s"), ink(&mut h, "c")];
    h.app_mut().0 = true;
    h.redraw();
    let off = [ink(&mut h, "s"), ink(&mut h, "c")];
    assert!(off[0] * 2 < on[0] && off[1] * 2 < on[1], "{on:?} enabled, {off:?} disabled");
  }
}
