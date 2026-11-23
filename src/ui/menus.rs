// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A menu's open state between frames, kept by its control's id, and the platform's own context
//! menu: asked for at a point, open until its answer comes ([Ui::native_menu]). How a menu is
//! drawn is the styled layer's, and where the platform shows none (the harness, unless
//! [crate::Harness::set_native_menus] plays one that does) the styled layer draws it.

use super::{Id, Ui, UiState};
use crate::app::Event;
use crate::menu::Menu;
use crate::scene::Point;

/// Whether `id`'s menu is open.
#[derive(Default)]
struct MenuOpen(bool);

/// The actions of a native context menu waiting for its answer, by number ([Menu::tagged]).
struct Actions<A>(Vec<A>);

impl<A> Default for Actions<A> {
  fn default() -> Self {
    Self(Vec::new())
  }
}

/// What the platform's context menu of an element did in this pass ([Ui::native_menu]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeMenu<A> {
  /// The platform shows none here (the harness, where a menu is drawn): the caller draws it.
  Unavailable,
  /// None asked for, or one open with its answer still to come.
  Unanswered,
  /// Its answer: the action chosen, `None` when it was dismissed.
  Answered(Option<A>),
}

impl Ui<'_> {
  /// Whether the menu of the control `id` is open: as the input since the last frame left it, so
  /// a trigger drawn before its menu shows it closed in the frame it closes; a native context
  /// menu ([Ui::native_menu]) until its answer comes.
  pub fn menu_open(&self, id: Id) -> bool {
    self.st.menu_open(id)
  }

  /// Opens or closes the menu of the control `id`, for the menu's behaviour, which puts the focus
  /// back when it closes. Opening it stops what was scrolling under it: a trackpad's gesture, a
  /// wheel's momentum.
  pub(crate) fn set_menu_open(&mut self, id: Id, open: bool) {
    if open && !self.menu_open(id) {
      self.st.halt_scrolling();
    }

    self.state::<MenuOpen>(id).0 = open;
  }

  /// The platform's context menu of `id`: asked for at `at` with `menu()` (run only then), free
  /// to go past the window's edges, open until its answer comes in a later pass ([Event::Menu]),
  /// which this returns then. [NativeMenu::Unavailable] where the platform shows none (the
  /// harness, unless it plays one that does): `menu` is not run, and the caller draws the menu.
  pub fn native_menu<A: Clone + 'static>(
    &mut self,
    id: Id,
    at: Option<Point>,
    menu: impl FnOnce() -> Menu<A>,
  ) -> NativeMenu<A> {
    if !self.st.native_menus {
      return NativeMenu::Unavailable;
    }

    let answer = self.st.take(id, |e| match e {
      Event::Menu { item, .. } => Some(*item),
      _ => None,
    });
    if let Some(item) = answer {
      self.set_menu_open(id, false);
      let actions = std::mem::take(&mut self.state::<Actions<A>>(id).0);
      return NativeMenu::Answered(item.and_then(|i| actions.get(i).cloned()));
    }

    // Asked for, it stays open until the answer comes, its actions kept till then.
    if at.is_some() || self.menu_open(id) {
      self.state::<MenuOpen>(id).0 = true;
      self.retain(id);
    }

    if let Some(at) = at {
      let (menu, actions) = menu().tagged();
      self.state::<Actions<A>>(id).0 = actions;
      self.st.context_menu = Some((id, at, menu));
    }

    NativeMenu::Unanswered
  }
}

impl UiState {
  /// [Ui::menu_open] as the last frame and the input since left it.
  pub fn menu_open(&self, id: Id) -> bool {
    self.store.peek::<MenuOpen>(id).is_some_and(|o| o.0)
  }

  /// Closes `id`'s menu: the platform's answered it.
  pub(crate) fn close_menu(&mut self, id: Id) {
    if let Some(o) = self.store.peek_mut::<MenuOpen>(id) {
      o.0 = false;
    }
  }
}
