// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The environment: values the view reads by type ([EnvironmentValue]), scoped to an element and
//! what is built inside it ([Ui::environment]), put for the whole window ([Ui::set_environment]),
//! or the type's default; [ControlSize] and [Enabled] among them.
//!
//! ```ignore
//! ui.environment(ControlSize::Small).row(Style::new(), |ui| {
//!   ui.button("Small");
//!   let enabled = ui.environment_value::<Enabled>().0;
//! });
//! ```

use std::any::{Any, TypeId};

use super::{ControlSize, Ui};
use crate::app::ColorScheme;

/// A value of the environment, its type its key: what [Ui::environment_value] reads when neither
/// an element around nor the window put one, worked out from the window (its appearance).
pub trait EnvironmentValue: Clone + 'static {
  fn default_value(ui: &Ui) -> Self;
}

/// Whether the widgets built here take input, `true` by default: [Ui::disabled] puts `false`;
/// `Enabled(true)` on an element enables it and what is built inside it again, a part that
/// works in a disabled container (a modal dialog, an ⓘ in a disabled row).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Enabled(pub bool);

impl EnvironmentValue for Enabled {
  fn default_value(_: &Ui) -> Self {
    Self(true)
  }
}

/// [ControlSize::Regular] outside a [Ui::control_size].
impl EnvironmentValue for ControlSize {
  fn default_value(_: &Ui) -> Self {
    Self::Regular
  }
}

/// The OS's light or dark ([Ui::appearance]), unless a theme the app set puts its own.
impl EnvironmentValue for ColorScheme {
  fn default_value(ui: &Ui) -> Self {
    ui.appearance().scheme
  }
}

/// One type's scoped values, innermost last, without its type ([Scoped]).
trait Values: Any {
  fn drop_last(&mut self);
  fn drop_all(&mut self);
}

impl<T: 'static> Values for Vec<T> {
  fn drop_last(&mut self) {
    self.pop();
  }

  fn drop_all(&mut self) {
    self.clear();
  }
}

/// A scope's owner while it waits for the next element.
const PENDING: u32 = u32::MAX;
/// The owners of groups' scopes ([Ui::group]), past every node's index: this and the depth.
const GROUP: u32 = 1 << 31;

/// The values scoped to elements being built ([Ui::environment]): a stack per type, and every
/// value put with its stack and the container it is scoped to, in the order put. Kept across
/// frames, cleared with their capacity kept, so a steady frame allocates nothing for them.
#[derive(Default)]
pub(crate) struct Scoped {
  stacks: Vec<(TypeId, Box<dyn Values>)>,
  put: Vec<(u32, usize)>,
  /// Groups open ([Ui::group]).
  groups: u32,
}

impl Scoped {
  pub(crate) fn clear(&mut self) {
    self.stacks.iter_mut().for_each(|s| s.1.drop_all());
    self.put.clear();
    self.groups = 0;
  }

  fn get<T: 'static>(&self) -> Option<&T> {
    let (_, s) = self.stacks.iter().find(|s| s.0 == TypeId::of::<T>())?;
    (&**s as &dyn Any).downcast_ref::<Vec<T>>()?.last()
  }

  /// Puts `v` for the next element.
  fn push<T: 'static>(&mut self, v: T) {
    let at = match self.stacks.iter().position(|s| s.0 == TypeId::of::<T>()) {
      Some(at) => at,
      None => {
        self.stacks.push((TypeId::of::<T>(), Box::new(Vec::<T>::new())));
        self.stacks.len() - 1
      }
    };

    let s = &mut *self.stacks[at].1 as &mut dyn Any;
    s.downcast_mut::<Vec<T>>().expect("its type's stack").push(v);
    self.put.push((PENDING, at));
  }

  /// Scopes the values waiting for the next element to container `i`, which opens.
  pub(crate) fn open(&mut self, i: u32) {
    for p in self.put.iter_mut().rev().take_while(|p| p.0 == PENDING) {
      p.0 = i;
    }
  }

  /// Drops the values of the element built (a leaf) or of container `i`, which closes, and the
  /// ones waiting for an element that never came.
  #[inline(always)]
  pub(crate) fn close(&mut self, i: u32) {
    while let Some(&(owner, at)) = self.put.last()
      && (owner == PENDING || owner == i)
    {
      self.stacks[at].1.drop_last();
      self.put.pop();
    }
  }
}

impl Ui<'_> {
  /// Builds `f` as one element for the values put for the next element ([Ui::environment],
  /// [Ui::disabled], [Ui::control_size]): they reach all it builds and end with it, as SwiftUI's
  /// `Group`. It adds no element: a widget of several elements side by side builds in one.
  pub fn group<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
    let sc = &mut self.st.scoped;
    sc.groups += 1;
    let owner = GROUP + sc.groups;
    sc.open(owner);
    let r = f(self);
    self.st.scoped.close(owner);
    self.st.scoped.groups -= 1;
    r
  }

  /// Puts `v` for the next element: what a container builds reads it until it closes, an element
  /// with no children drops it, and the widget being built reads it at once
  /// (`ui.environment(ControlSize::Small).button("Save")`).
  pub fn environment<T: EnvironmentValue>(&mut self, v: T) -> &mut Self {
    self.st.scoped.push(v);
    self
  }

  /// The value of type `T` here: the innermost an element around put ([Ui::environment]), one
  /// put for the next element too, else the window's ([Ui::set_environment]), else the type's
  /// default ([EnvironmentValue::default_value]).
  pub fn environment_value<T: EnvironmentValue>(&self) -> T {
    let window = || self.st.env.iter().find_map(|v| v.downcast_ref::<T>());
    match self.st.scoped.get::<T>().or_else(window) {
      Some(v) => v.clone(),
      None => T::default_value(self),
    }
  }

  /// Puts `v` as the window's value of type `T`, under the ones elements put, or clears it with
  /// `None` back to the default; returns the one it replaces. Kept across frames, so a view may
  /// put it every frame: one already there is replaced in place.
  pub fn set_environment<T: EnvironmentValue>(&mut self, v: Option<T>) -> Option<T> {
    let env = &mut self.st.env;
    let i = env.iter().position(|e| e.is::<T>());
    match (i, v) {
      (Some(i), Some(v)) => env[i].downcast_mut().map(|old| std::mem::replace(old, v)),
      (Some(i), None) => env.swap_remove(i).downcast().ok().map(|old| *old),
      (None, Some(v)) => {
        env.push(Box::new(v));
        None
      }
      (None, None) => None,
    }
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::app::Appearance;
  use crate::scene::Size;
  use crate::ui::{Style, UiState};

  #[derive(Clone, Copy, Debug, PartialEq)]
  struct Tint(u32);

  impl EnvironmentValue for Tint {
    fn default_value(_: &Ui) -> Self {
      Self(0)
    }
  }

  fn ui(st: UiState) -> Ui<'static> {
    Ui::new(st, Size::new(10.0, 10.0), None, Duration::ZERO)
  }

  /// A container's value inside it, nested ones over it, and gone after it for its sibling.
  #[test]
  fn a_value_is_its_containers() {
    let mut ui = ui(UiState::default());
    ui.environment(Tint(1)).col(Style::new(), |ui| {
      assert_eq!(ui.environment_value::<Tint>(), Tint(1));
      ui.environment(Tint(2)).row(Style::new(), |ui| {
        ui.el(Style::new());
        assert_eq!(ui.environment_value::<Tint>(), Tint(2));
      });
      assert_eq!(ui.environment_value::<Tint>(), Tint(1));
    });
    ui.row(Style::new(), |ui| assert_eq!(ui.environment_value::<Tint>(), Tint(0)));
  }

  /// Put for the next element, read before it is built; an element with no children, or a
  /// container's end, drops it.
  #[test]
  fn a_leaf_drops_the_value() {
    let mut ui = ui(UiState::default());
    ui.environment(Tint(1));
    assert_eq!(ui.environment_value::<Tint>(), Tint(1));
    ui.el(Style::new());
    assert_eq!(ui.environment_value::<Tint>(), Tint(0));
    ui.col(Style::new(), |ui| _ = ui.environment(Tint(2)));
    assert_eq!(ui.environment_value::<Tint>(), Tint(0));
  }

  /// The window's value under a scoped one, kept across frames, replaced in place.
  #[test]
  fn the_windows_value_is_under_the_scoped_ones() {
    let mut ui = ui(UiState::default());
    assert!(ui.set_environment(Some(Tint(1))).is_none());
    let mut ui = super::tests::ui(ui.finish());
    assert_eq!(ui.environment_value::<Tint>(), Tint(1));
    ui.environment(Tint(2))
      .col(Style::new(), |ui| assert_eq!(ui.environment_value::<Tint>(), Tint(2)));
    assert_eq!(ui.set_environment(Some(Tint(3))), Some(Tint(1)));
    assert_eq!(ui.environment_value::<Tint>(), Tint(3));
    assert_eq!(ui.set_environment::<Tint>(None), Some(Tint(3)));
    assert_eq!(ui.environment_value::<Tint>(), Tint(0));
  }

  /// The defaults come from the window's appearance.
  #[test]
  fn defaults_follow_the_appearance() {
    for a in [Appearance::LIGHT, Appearance::DARK] {
      let mut st = UiState::default();
      st.set_appearance(a);
      let ui = ui(st);
      assert_eq!(ui.environment_value::<ColorScheme>(), a.scheme);
      assert_eq!(ui.environment_value::<ControlSize>(), ControlSize::Regular);
      assert_eq!(ui.environment_value::<Enabled>(), Enabled(true));
    }
  }

  /// A group's values reach all it builds, nested groups their own, and end with it.
  #[test]
  fn a_group_spans_its_elements() {
    let mut ui = ui(UiState::default());
    let tint = |ui: &Ui| ui.environment_value::<Tint>();
    ui.environment(Tint(1)).group(|ui| {
      ui.el(Style::new());
      assert_eq!(tint(ui), Tint(1));
      ui.environment(Tint(2)).group(|ui| {
        ui.el(Style::new());
        ui.row(Style::new(), |ui| assert_eq!(tint(ui), Tint(2)));
      });
      assert_eq!(tint(ui), Tint(1));
      ui.environment(Tint(3));
    });
    assert_eq!(tint(&ui), Tint(0));
  }

  /// `disabled(false)` leaves a disabled container disabled; `Enabled(true)` enables a part of it.
  #[test]
  fn disabled_only_disables() {
    let mut ui = ui(UiState::default());
    let on = |ui: &Ui| ui.environment_value::<Enabled>().0;
    ui.disabled(true).col(Style::new(), |ui| {
      ui.disabled(false).row(Style::new(), |ui| assert!(!on(ui)));
      ui.environment(Enabled(true)).row(Style::new(), |ui| assert!(on(ui)));
      assert!(!on(ui));
    });
    assert!(on(&ui));
  }
}
