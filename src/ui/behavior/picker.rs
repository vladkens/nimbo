// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The picker over its behaviour, drawn by the environment's [PickerStyle], as SwiftUI's `Picker`
//! and `.pickerStyle`: the style chooses its input (a pop-up button and its menu, or segments)
//! and builds the control from a [PickerConfiguration]; the picker writes the option picked back.
//! [InlinePickerStyle], the options side by side, unless the shell put its own.
//!
//! ```ignore
//! struct Chips;
//!
//! impl PickerStyle for Chips {
//!   fn make_body(&self, ui: &mut Ui, mut c: PickerConfiguration) {
//!     let b = c.segments(ui);
//!     ui.row(Style::new().gap(4.0), |ui| {
//!       for i in 0..c.len() {
//!         let fill = if c.selection() == Some(i) { ON } else { OFF };
//!         let chip = b.pointer(Style::new().pad_xy(10.0, 3.0).radius(10.0).bg(fill));
//!         ui.with_id(c.item_id(i)).row(chip, |ui| ui.text(c.label(i), TEXT));
//!       }
//!     });
//!   }
//! }
//!
//! ui.picker_style(&Chips).picker(&mut self.view, &VIEWS);
//! ```

use super::button::plain_text;
use super::{Behavior, MenuPlacement};
use crate::ui::{
  Enabled, EnvironmentValue, Icon, Id, LinePattern, Response, Role, State, Style, Ui, Value, px,
};

/// A value and its label, as SwiftUI's `.tag(value)` on a view: an option of a [Picker]
/// (`Text(label).tag(value)`) or a tab of a toolbar (`.tabItem { Label(label, systemImage:) }`);
/// `const`, so a list of them is a constant:
/// `const LAUNCH: [Tag<Launch>; 2] = [Tag::new(Launch::Window, "Open a window"), ..]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tag<'s, T> {
  pub(crate) value: T,
  pub(crate) label: &'s str,
  /// An icon by the label: over it in a toolbar's tab; a picker's style shows it if it draws one
  /// ([PickerConfiguration::icon]).
  pub(crate) icon: Option<Icon>,
  /// A count drawn after the label, dimmed, by the styles that show one (the segments); the
  /// menu shows none.
  pub(crate) count: Option<usize>,
}

impl<'s, T> Tag<'s, T> {
  pub const fn new(value: T, label: &'s str) -> Self {
    Self { value, label, icon: None, count: None }
  }
}

impl<'s, T: Copy> Tag<'s, T> {
  /// With `icon` by the label, as SwiftUI's `Label(label, systemImage:)`.
  pub const fn icon(self, icon: Icon) -> Self {
    Self { icon: Some(icon), ..self }
  }

  /// With `n` after the label, dimmed: a filter's matches.
  pub const fn count(self, n: usize) -> Self {
    Self { count: Some(n), ..self }
  }

  pub const fn value(&self) -> T {
    self.value
  }

  pub const fn label(&self) -> &'s str {
    self.label
  }
}

/// How a picker looks and takes input, as SwiftUI's `PickerStyle`: [PickerStyle::make_body] runs
/// one of the configuration's inputs, then builds the picker's root (its first element, given its
/// id, focus and the hand) and the options in it. Chosen with [Ui::picker_style] as `&'static`.
pub trait PickerStyle: 'static {
  /// Builds the picker `config` describes.
  fn make_body(&self, ui: &mut Ui<'_>, config: PickerConfiguration<'_>);
}

/// What the input chose this frame, kept by the [Picker] that writes it back.
#[derive(Debug, Default)]
struct Picked {
  /// The input's behaviour, once run.
  b: Option<Behavior>,
  /// The selected option, moved by letters typed or the arrows.
  at: Option<usize>,
  /// An item of the menu clicked or run.
  item: Option<usize>,
  clicked: bool,
}

/// What a [PickerStyle] draws: the options (labels, counts, dividers), the one selected, their
/// ids, and the inputs that pick one. The style sees the options by their place; the [Picker]
/// turns the place picked into its value.
#[non_exhaustive]
#[derive(Debug)]
pub struct PickerConfiguration<'s> {
  id: Id,
  /// Each option's label, icon and count.
  options: Vec<(&'s str, Option<Icon>, Option<usize>)>,
  /// The options a divider follows.
  lines: Vec<usize>,
  width: Option<f32>,
  max_width: Option<f32>,
  enabled: bool,
  picked: &'s mut Picked,
}

impl<'s> PickerConfiguration<'s> {
  /// The picker's id, its root's: for the style's hover fade and its menu's place.
  pub fn id(&self) -> Id {
    self.id
  }

  /// How many options it has, the dividers not counted.
  pub fn len(&self) -> usize {
    self.options.len()
  }

  pub fn is_empty(&self) -> bool {
    self.options.is_empty()
  }

  /// Option `i`'s label.
  pub fn label(&self, i: usize) -> &'s str {
    self.options[i].0
  }

  /// Option `i`'s icon ([Tag::icon]).
  pub fn icon(&self, i: usize) -> Option<Icon> {
    self.options[i].1
  }

  /// Option `i`'s count ([Tag::count]).
  pub fn count(&self, i: usize) -> Option<usize> {
    self.options[i].2
  }

  /// Whether a divider stands between option `i` and the next ([Picker::divider]).
  pub fn divider_after(&self, i: usize) -> bool {
    self.lines.contains(&i)
  }

  /// The selected option, the input's choice this frame included; none for a value none of the
  /// options has.
  pub fn selection(&self) -> Option<usize> {
    self.picked.at
  }

  /// Option `i`'s id, `id.key(("item", i))`: a segment's or a menu item's.
  pub fn item_id(&self, i: usize) -> Id {
    self.id.key(("item", i))
  }

  /// Not disabled ([Ui::disabled]): it reads input, and is drawn so.
  pub fn is_enabled(&self) -> bool {
    self.enabled
  }

  /// The width set for it ([Picker::width]).
  pub fn width(&self) -> Option<f32> {
    self.width
  }

  /// The most width set for it ([Picker::max_width]).
  pub fn max_width(&self) -> Option<f32> {
    self.max_width
  }

  /// Runs the pop-up button's input ([Ui::pop_up_behavior]): a click, Space, Enter or the arrows
  /// open the menu ([PickerConfiguration::menu]), letters typed select an option. The next
  /// element is the picker's root ([Ui::control]).
  pub fn pop_up(&mut self, ui: &mut Ui) -> Behavior {
    debug_assert!(self.picked.b.is_none(), "a picker runs one input");
    let options = &self.options;
    let b = ui.pop_up_behavior(self.id, options.len(), |i| options[i].0, &mut self.picked.at);
    (self.picked.b, self.picked.clicked) = (Some(b), b.clicked);
    ui.control(&b);
    b
  }

  /// Runs the segments' input ([Ui::pick_behavior]): a click on an item ([Self::item_id])
  /// selects it, the arrows step while the picker has focus; radio buttons in a group to
  /// assistive technology. The next element is the picker's root ([Ui::control]).
  pub fn segments(&mut self, ui: &mut Ui) -> Behavior {
    debug_assert!(self.picked.b.is_none(), "a picker runs one input");
    let id = self.id;
    // A menu left open by another style before closes with it.
    ui.dismiss_menu(id);
    let b = ui.pick_behavior(id, self.len(), |i| id.key(("item", i)), &mut self.picked.at);
    ui.a11y(id, Role::RadioGroup);
    for i in 0..self.len() {
      let on = if self.picked.at == Some(i) { Value::On } else { Value::Off };
      ui.a11y(self.item_id(i), Role::RadioButton).value(on);
    }

    (self.picked.b, self.picked.clicked) = (Some(b), b.changed);
    ui.control(&b);
    b
  }

  /// The pop-up button's menu ([Ui::menu_popup]) in a `style` box, toggled by its click, the
  /// selected option over it ([MenuPlacement::Over]), closed when disabled: `items` builds its
  /// items ([Self::menu_item]). Whether it is open.
  pub fn menu(
    &mut self,
    ui: &mut Ui,
    style: Style,
    items: impl FnOnce(&mut Ui, &mut Self),
  ) -> bool {
    let b = self.picked.b.unwrap_or_default();
    debug_assert!(self.picked.b.is_some(), "a menu after the pop-up button's input");
    let toggle = b.clicked || b.disabled && ui.menu_open(self.id);
    let place = MenuPlacement::Over { start: self.picked.at };
    ui.menu_popup(self.id, toggle, place, style, |ui| items(ui, self))
  }

  /// Option `i` as an item of the menu ([Ui::menu_item_behavior]), selected to assistive
  /// technology when it is the selected option: picked when clicked or run. Its element is
  /// [Self::item_id].
  pub fn menu_item(&mut self, ui: &mut Ui, i: usize) -> Behavior {
    let item = self.item_id(i);
    let b = ui.menu_item_behavior(item, self.label(i));
    ui.a11y(item, Role::MenuItem).state(State::Selected, self.picked.at == Some(i));
    if b.clicked {
      self.picked.item = Some(i);
    }

    b
  }
}

/// One value among its options, as SwiftUI's `Picker` with `Divider`s among them: drawn by the
/// environment's [PickerStyle] ([Ui::picker_style]), which picks by place; the value picked is
/// written back to `cur`, `changed` then.
pub struct Picker<'s, T> {
  cur: &'s mut T,
  /// Its options, `None` a divider.
  entries: Vec<Option<Tag<'s, T>>>,
  width: Option<f32>,
  max_width: Option<f32>,
}

impl<'s, T: PartialEq + Copy> Picker<'s, T> {
  pub fn new(cur: &'s mut T) -> Self {
    Self { cur, entries: Vec::new(), width: None, max_width: None }
  }

  /// An option: `value`, shown as `label`.
  pub fn option(mut self, value: T, label: &'s str) -> Self {
    self.entries.push(Some(Tag::new(value, label)));
    self
  }

  /// An option for each of `options`, as SwiftUI's `ForEach` of tagged texts.
  pub fn options(mut self, options: &[Tag<'s, T>]) -> Self {
    self.entries.extend(options.iter().copied().map(Some));
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

  /// Builds it under the id set for it: the style's body, then the option picked written back.
  pub fn show(self, ui: &mut Ui) -> Response {
    // The values put for it reach its menu too.
    ui.group(|ui| self.build(ui))
  }

  fn build(self, ui: &mut Ui) -> Response {
    let style = ui.environment_value::<&dyn PickerStyle>();
    let id = ui.take_id();
    let tags: Vec<Tag<T>> = self.entries.iter().flatten().copied().collect();
    // A line after each option a divider follows, but the last.
    let (mut lines, mut before) = (Vec::new(), 0);
    for e in &self.entries {
      match e {
        Some(_) => before += 1,
        None if before > 0 && before < tags.len() => lines.push(before - 1),
        None => {}
      }
    }

    let was = tags.iter().position(|o| o.value == *self.cur);
    let mut picked = Picked { at: was, ..Picked::default() };
    let config = PickerConfiguration {
      id,
      options: tags.iter().map(|o| (o.label, o.icon, o.count)).collect(),
      lines,
      width: self.width,
      max_width: self.max_width,
      enabled: ui.environment_value::<Enabled>().0,
      picked: &mut picked,
    };

    let root = ui.st.tree.nodes.len();
    style.make_body(ui, config);
    debug_assert!(
      picked.b.is_some() && ui.st.tree.nodes.get(root).is_some_and(|n| n.id == id),
      "a picker style runs an input, then builds the picker's root first"
    );

    let mut r = ui.st.response(id);
    r.clicked = picked.clicked;
    if let Some(i) = picked.item.or(picked.at).filter(|&i| Some(i) != was) {
      *self.cur = tags[i].value;
      r.changed = true;
    }

    // Disabled, it reads no input: the tree's hit is under it all the same.
    if picked.b.is_none_or(|b| b.disabled) {
      (r.hovered, r.pressed, r.focused) = (false, false, false);
    }

    r
  }
}

/// The options side by side as plain labels, as SwiftUI's `.inline`: the selected one underlined
/// in the label colour, the others secondary, a thin line at a divider; over the segments' input.
/// The default under the shell's.
#[derive(Clone, Copy, Debug, Default)]
pub struct InlinePickerStyle;

impl PickerStyle for InlinePickerStyle {
  fn make_body(&self, ui: &mut Ui<'_>, mut c: PickerConfiguration<'_>) {
    let b = c.segments(ui);
    // macOS's label colours: 85% of black or white, its secondary's 50%, its tertiary's 25%.
    let (on, off) = if b.disabled { (0.25, 0.25) } else { (0.85, 0.5) };
    let line = plain_text(ui, 0.25).color;
    let h = plain_text(ui, 1.0).font.size;
    ui.row(Style::new().gap(12.0).center(), |ui| {
      for i in 0..c.len() {
        let selected = c.selection() == Some(i);
        let s = plain_text(ui, if selected { on } else { off }).nowrap();
        let item = b.pointer(Style::new().gap(4.0).center());
        ui.with_id(c.item_id(i)).row(item, |ui| {
          match selected {
            true => ui.text(c.label(i), s.underline(LinePattern::Solid)),
            false => ui.text(c.label(i), s),
          }

          if let Some(n) = c.count(i) {
            ui.text(n, plain_text(ui, off).nowrap());
          }
        });

        if c.divider_after(i) {
          ui.el(Style::new().w(px(1.0)).h(px(h)).bg(line));
        }
      }
    });
  }
}

/// [InlinePickerStyle] where neither a container nor the window nor the shell put one.
impl EnvironmentValue for &'static dyn PickerStyle {
  fn default_value(_: &Ui) -> Self {
    &InlinePickerStyle
  }
}

impl Ui<'_> {
  /// A [Picker] of `options`: SwiftUI's `Picker(selection:)` of `Text`s tagged with their
  /// values.
  pub fn picker<T: PartialEq + Copy>(&mut self, cur: &mut T, options: &[Tag<T>]) -> Response {
    Picker::new(cur).options(options).show(self)
  }

  /// Draws the next picker, or every picker built in the next container, in `style`, as
  /// SwiftUI's `.pickerStyle`: `ui.picker_style(&SegmentedPickerStyle).picker(&mut v, &VIEWS)`.
  pub fn picker_style(&mut self, style: &'static dyn PickerStyle) -> &mut Self {
    self.environment(style)
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::{Quad, Size};
  use crate::ui::{Cursor, UiState};
  use crate::{Modifiers, NamedKey};

  const UP: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
  const OVER: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
  const ON: [f32; 4] = [1.0; 4];
  const OFF: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

  /// A 20-point square per option of the app's: white the selected one, green hovered, blue
  /// else, red when disabled; it never sets focus or a pointer.
  struct Squares;

  impl PickerStyle for Squares {
    fn make_body(&self, ui: &mut Ui<'_>, mut c: PickerConfiguration<'_>) {
      let b = c.segments(ui);
      ui.row(Style::new(), |ui| {
        for i in 0..c.len() {
          let item = c.item_id(i);
          let fill = match (b.disabled, c.selection() == Some(i), ui.hovered(item)) {
            (true, ..) => OFF,
            (_, true, _) => ON,
            (_, _, true) => OVER,
            _ => UP,
          };

          let s = Style::new().w(px(20.0)).h(px(20.0));
          ui.with_id(item).canvas(s, move |f, r| f.quad(Quad::new(r, fill)));
        }
      });
    }
  }

  const ABC: [Tag<char>; 3] = [Tag::new('a', "A"), Tag::new('b', "B"), Tag::new('c', "C")];

  struct Pick {
    cur: char,
    off: bool,
    changes: usize,
  }

  impl App for Pick {
    fn view(&mut self, ui: &mut Ui) {
      let ui = ui.picker_style(&Squares).disabled(self.off);
      self.changes += ui.id("p").picker(&mut self.cur, &ABC).changed as usize;
    }
  }

  #[test]
  fn a_style_draws_over_the_input_and_the_picker_writes_back() {
    let mut h =
      Harness::new(Pick { cur: 'a', off: false, changes: 0 }, Size::new(100.0, 40.0), 1.0);
    let px = |h: &mut Harness<Pick>, x: f32| h.render().pixel(x, 10.0);
    let rgb = |c: [f32; 4]| c.map(|v| (v * 255.0) as u8);
    assert_eq!((px(&mut h, 10.0), px(&mut h, 30.0)), (rgb(ON), rgb(UP)));
    assert_eq!(h.cursor(), Cursor::Arrow);

    // Hovered under the hand the root gives it, clicked: the value written back.
    h.move_to(30.0, 10.0);
    assert_eq!((px(&mut h, 30.0), h.cursor()), (rgb(OVER), Cursor::Pointer));
    h.click(30.0, 10.0);
    assert_eq!((h.app().cur, h.app().changes), ('b', 1));
    assert_eq!(px(&mut h, 30.0), rgb(ON));

    // Focused by Tab, stepped by the arrows; radio buttons in a group.
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!(h.focus_id(), Some(Id::of("p")));
    h.key(NamedKey::ArrowRight, Modifiers::empty());
    assert_eq!((h.app().cur, h.app().changes), ('c', 2));
    h.frame();
    let a = h.a11y();
    let lines: Vec<&str> = a.lines().map(str::trim).collect();
    assert_eq!(lines[1..], ["radio button off", "radio button off", "radio button on selected"]);
    assert!(lines[0].starts_with("radio group"), "{a}");

    // Disabled: its own red, no hand, no focus, no picks.
    h.app_mut().off = true;
    h.redraw();
    assert_eq!(px(&mut h, 10.0), rgb(OFF));
    assert_eq!(h.focus_id(), None);
    h.move_to(11.0, 10.0);
    assert_eq!(h.cursor(), Cursor::Arrow);
    h.click(10.0, 10.0);
    h.key(NamedKey::Tab, Modifiers::empty());
    assert_eq!((h.focus_id(), h.app().cur, h.app().changes), (None, 'c', 2));
    assert!(h.a11y().contains("radio button off disabled"), "{}", h.a11y());
  }

  /// Pickers in a container given the squares, and one after it.
  struct Cascade;

  impl App for Cascade {
    fn view(&mut self, ui: &mut Ui) {
      ui.picker_style(&Squares).col(Style::new(), |ui| {
        ui.id("a").picker(&mut 'a', &ABC);
        ui.row(Style::new(), |ui| _ = ui.id("b").picker(&mut 'a', &ABC));
      });
      ui.id("c").picker(&mut 'a', &ABC);
    }
  }

  /// The container's style reaches its pickers only; the one after it is the shell's pop-up.
  #[test]
  fn a_containers_style_reaches_its_pickers_only() {
    let mut h = Harness::new(Cascade, Size::new(300.0, 200.0), 1.0);
    h.frame();
    let side = |n: &str| h.rect(Id::of(n)).map(|r| (r.w, r.h)).unwrap();
    assert_eq!((side("a"), side("b")), ((60.0, 20.0), (60.0, 20.0)));
    assert_ne!(side("c"), (60.0, 20.0));
    let a = h.a11y();
    assert!(a.lines().any(|l| l.trim() == "pop up button value \"A\""), "{a}");
  }

  /// The shell's pop-up, or the squares, and the option picked.
  struct Switched(bool, char);

  impl App for Switched {
    fn view(&mut self, ui: &mut Ui) {
      if self.0 {
        ui.picker_style(&Squares);
      }

      ui.id("p").picker(&mut self.1, &ABC);
    }
  }

  /// A menu open when the style turns to segments closes, the focus back on the picker, whose
  /// arrows step; back to the pop-up, it stays closed.
  #[test]
  fn segments_close_a_menu_left_open() {
    let mut h = Harness::new(Switched(false, 'a'), Size::new(300.0, 200.0), 1.0);
    h.frame();
    h.key(NamedKey::Tab, Modifiers::empty());
    assert!(h.click_id(Id::of("p")));
    assert!(h.menu_open(Id::of("p")));
    h.app_mut().0 = true;
    h.redraw();
    h.frame();
    assert_eq!(h.focus_id(), Some(Id::of("p")));
    h.key(NamedKey::ArrowRight, Modifiers::empty());
    assert_eq!(h.app().1, 'b');

    h.app_mut().0 = false;
    h.redraw();
    h.frame();
    assert!(!h.menu_open(Id::of("p")));
  }

  /// With no shell, inline: each option an element, no fill.
  #[test]
  fn inline_is_the_default_without_the_shell() {
    let mut ui = Ui::new(UiState::default(), Size::new(200.0, 100.0), None, Duration::ZERO);
    ui.id("p").picker(&mut 'b', &ABC);
    let node = |id: Id| ui.st.tree.nodes.iter().find(|n| n.id == id);
    let root = node(Id::of("p")).unwrap();
    assert_eq!(root.style.bg, [0.0; 4]);
    assert!((0..3usize).all(|i| node(Id::of("p").key(("item", i))).is_some()));
  }
}
