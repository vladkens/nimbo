// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The menu model the tray ([crate::tray]) and context menus ([crate::Ui::context_menu]) share,
//! as SwiftUI's menu content: a [Menu] of [Item]s, buttons and toggles each chosen by an action
//! of the app's own type, submenus, section titles and dividers; shortcut labels, disabled and
//! destructive items. Labels longer than [LABEL] are shortened to fit ([Item::button]).

use crate::keys::Shortcut;
use crate::ui::ButtonRole;

/// A menu's items, in the order they show, each chosen by its action: a value of the app's own
/// type `A` (an enum of what its menus do), as SwiftUI's menus run a button's action.
#[derive(Clone, Debug, PartialEq)]
pub struct Menu<A> {
  pub items: Vec<Item<A>>,
}

impl<A> Default for Menu<A> {
  fn default() -> Self {
    Self { items: Vec::new() }
  }
}

impl<A> Menu<A> {
  pub fn new(items: Vec<Item<A>>) -> Self {
    Self { items }
  }

  /// The menu with each action replaced by its number, depth first in the order they show,
  /// and the actions by number: the tags the OS menus use (a tag, a command id or a D-Bus menu
  /// id), the answer mapped back to the action.
  pub(crate) fn tagged(&self) -> (Menu<usize>, Vec<A>)
  where
    A: Clone,
  {
    fn walk<A: Clone>(items: &[Item<A>], out: &mut Vec<A>) -> Vec<Item<usize>> {
      let mut tagged = Vec::with_capacity(items.len());
      for it in items {
        let n = out.len();
        tagged.push(match it {
          Item::Button { action, label, role, enabled, key } => {
            out.push(action.clone());
            let (label, role, enabled, key) = (label.clone(), *role, *enabled, *key);
            Item::Button { action: n, label, role, enabled, key }
          }
          Item::Toggle { action, label, is_on, enabled, key } => {
            out.push(action.clone());
            let (label, is_on, enabled, key) = (label.clone(), *is_on, *enabled, *key);
            Item::Toggle { action: n, label, is_on, enabled, key }
          }
          Item::Submenu { label, enabled, menu } => {
            let menu = Menu::new(walk(&menu.items, out));
            Item::Submenu { label: label.clone(), enabled: *enabled, menu }
          }
          Item::Section(s) => Item::Section(s.clone()),
          Item::Divider => Item::Divider,
        });
      }

      tagged
    }

    let mut actions = Vec::new();
    (Menu::new(walk(&self.items, &mut actions)), actions)
  }
}

/// A menu item, as SwiftUI's menu content: a button or a toggle chosen by its action
/// ([crate::tray::Tray::on_item], [crate::Ui::context_menu]), a submenu, a section's title, or a
/// divider. A label longer than [LABEL] is shortened to fit ([Item::button]).
#[derive(Clone, Debug, PartialEq)]
pub enum Item<A> {
  Button {
    action: A,
    label: String,
    /// [ButtonRole::Destructive] shows red where the menu has the style (the drawn one), plain
    /// elsewhere.
    role: Option<ButtonRole>,
    enabled: bool,
    /// Shown beside it and, on macOS, working while the menu is open.
    key: Option<Shortcut>,
  },
  /// A check mark when on; Linux panels show an empty box while it is off.
  Toggle {
    action: A,
    label: String,
    is_on: bool,
    enabled: bool,
    key: Option<Shortcut>,
  },
  Submenu {
    label: String,
    enabled: bool,
    menu: Menu<A>,
  },
  /// A section's title over the items after it, never chosen: a section header on macOS 14+, a
  /// disabled item elsewhere.
  Section(String),
  Divider,
}

impl<A> Item<A> {
  /// A button running `action`, `label` shortened past [LABEL] ([Item::full] keeps it whole).
  pub fn button(action: A, label: &str) -> Self {
    Self::full(action, &fit(label, LABEL))
  }

  /// A button showing all of `label`, however long.
  pub fn full(action: A, label: &str) -> Self {
    let (label, role, enabled, key) = (label.into(), None, true, None);
    Item::Button { action, label, role, enabled, key }
  }

  /// A toggle running `action`, a check mark when `is_on`, as SwiftUI's `Toggle` in a menu.
  pub fn toggle(action: A, label: &str, is_on: bool) -> Self {
    let (label, enabled, key) = (fit(label, LABEL), true, None);
    Item::Toggle { action, label, is_on, enabled, key }
  }

  pub fn submenu(label: &str, menu: Menu<A>) -> Self {
    Item::Submenu { label: fit(label, LABEL), enabled: true, menu }
  }

  pub fn section(label: &str) -> Self {
    Item::Section(label.into())
  }

  pub fn enabled(mut self, on: bool) -> Self {
    match &mut self {
      Item::Button { enabled, .. } | Item::Toggle { enabled, .. } => *enabled = on,
      Item::Submenu { enabled, .. } => *enabled = on,
      Item::Section(_) | Item::Divider => {}
    }

    self
  }

  /// A button's role ([ButtonRole::Destructive]: it deletes or discards).
  pub fn role(mut self, r: ButtonRole) -> Self {
    if let Item::Button { role, .. } = &mut self {
      *role = Some(r);
    }

    self
  }

  /// A button's or a toggle's shortcut, as SwiftUI's `.keyboardShortcut`.
  pub fn key(mut self, k: Shortcut) -> Self {
    if let Item::Button { key, .. } | Item::Toggle { key, .. } = &mut self {
      *key = Some(k);
    }

    self
  }

  /// Its label; a divider's is empty.
  pub fn label(&self) -> &str {
    match self {
      Item::Button { label, .. } | Item::Toggle { label, .. } | Item::Submenu { label, .. } => {
        label
      }
      Item::Section(label) => label,
      Item::Divider => "",
    }
  }

  /// Whether it can be chosen or opened; a section's title or a divider never.
  pub fn is_enabled(&self) -> bool {
    match self {
      Item::Button { enabled, .. } | Item::Toggle { enabled, .. } => *enabled,
      Item::Submenu { enabled, .. } => *enabled,
      Item::Section(_) | Item::Divider => false,
    }
  }

  /// Whether it is a toggle that is on, as SwiftUI's `isOn`.
  pub fn is_on(&self) -> bool {
    matches!(self, Item::Toggle { is_on: true, .. })
  }

  pub fn is_destructive(&self) -> bool {
    matches!(self, Item::Button { role: Some(ButtonRole::Destructive), .. })
  }

  pub fn shortcut(&self) -> Option<Shortcut> {
    match self {
      Item::Button { key, .. } | Item::Toggle { key, .. } => *key,
      _ => None,
    }
  }

  /// The action a choice of it runs: a button's or a toggle's.
  pub fn action(&self) -> Option<&A> {
    match self {
      Item::Button { action, .. } | Item::Toggle { action, .. } => Some(action),
      _ => None,
    }
  }

  /// The label of its shortcut as the OS shows it beside the item: `⌘,` on macOS, `Ctrl+,`
  /// elsewhere; empty without one.
  pub fn key_label(&self) -> String {
    self.shortcut().map(|k| k.label()).unwrap_or_default()
  }
}

/// The most characters an item's label shows ([Item::button]): a menu keeps its width whatever the
/// app lists (a device, a file, a warning).
pub const LABEL: usize = 48;

/// `text` in at most `max` characters: the part in brackets shortened first (`System Default
/// (MacBook Pro…)`), or left out when too little of it would show (`No sound from Line In: check
/// the microphone`); else cut at a word, with `…`. From wavo-app's `src/menu.rs` (`fit`) and
/// `src/history.rs` (`shorten`).
fn fit(text: &str, max: usize) -> String {
  let len = |s: &str| s.chars().count();
  if len(text) <= max {
    return text.to_owned();
  }

  // The bracket that closes " (", past any inside it: `Microphone Array (Realtek(R) Audio)`.
  let depth = |d: &mut i32, (i, c): (usize, char)| {
    *d += match c {
      '(' => 1,
      ')' => -1,
      _ => 0,
    };
    Some((i, *d))
  };
  if let Some((head, rest)) = text.split_once(" (")
    && let Some((end, _)) = rest.char_indices().scan(1, depth).find(|&(_, d)| d == 0)
  {
    let (inner, tail) = (&rest[..end], &rest[end + 1..]);
    // " (", "…)" around what shows of it.
    let room = max.saturating_sub(len(head) + len(tail) + 4);
    if room >= 8 {
      return format!("{head} ({}){tail}", shorten(inner, room));
    }

    return fit(&format!("{head}{tail}"), max);
  }

  shorten(text, max.saturating_sub(1))
}

/// `text` on one line, its first words up to `max` characters and `…` where it is cut (a word
/// longer than that cut inside), with no punctuation before it.
fn shorten(text: &str, max: usize) -> String {
  let mut start = String::new();
  for (i, w) in text.split_whitespace().enumerate() {
    if start.chars().count() + usize::from(i > 0) + w.chars().count() > max {
      if start.is_empty() {
        start = w.chars().take(max).collect();
      }

      return format!("{}…", start.trim_end_matches([',', ';', ':', '.', '—', '-']));
    }

    if i > 0 {
      start.push(' ');
    }

    start.push_str(w);
  }

  start
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Labels cut at a word to fit, brackets shortened or left out, and items fitting by themselves.
  #[test]
  fn long_labels_fit() {
    assert_eq!(fit("System Default (MacBook Pro Microphone)", 32), "System Default (MacBook Pro…)");
    assert_eq!(
      fit("a-very-long-model-name-for-the-menu-q8", 32),
      "a-very-long-model-name-for-the…"
    );
    assert_eq!(fit("Elgato Wave:3 Microphone Array", 32), "Elgato Wave:3 Microphone Array");
    assert_eq!(fit("Large v3, Turbo, quantized to eight bits", 24), "Large v3, Turbo…");
    // No room inside the brackets: they go.
    assert_eq!(fit("A Rather Long Device Name (USB)", 24), "A Rather Long Device…");
    assert_eq!(fit("Ünïcödé wörds ärë cöüntëd äs chärs", 20), "Ünïcödé wörds ärë…");
    assert_eq!(fit("anything", 0), "…");
    // Brackets in the middle: shortened, or left out when too little of them would show.
    let warning = "No sound from Line In (High Definition Audio Device): check the microphone";
    assert_eq!(fit(warning, LABEL), "No sound from Line In: check the microphone");
    let warning = "No sound from USB Mic (Elgato Wave:3 Microphone Array): check the microphone";
    assert_eq!(fit(warning, 60), "No sound from USB Mic (Elgato Wave:3…): check the microphone");
    let error = "The last dictation failed: the engine did not answer in time and was restarted";
    assert!(fit(error, LABEL).ends_with('…') && fit(error, LABEL).chars().count() <= LABEL);
    // Brackets inside the brackets: the outer ones close at their own.
    assert_eq!(fit("Microphone Array (Realtek(R) Audio)", 32), "Microphone Array (Realtek(R)…)");
    let warning = "No sound from Microphone Array (Realtek(R) Audio): check the microphone";
    assert_eq!(fit(warning, LABEL), "No sound from Microphone Array: check the…");
    // Items fit by themselves, unless asked to show it all.
    assert_eq!(Item::button(0, error).label(), fit(error, LABEL));
    assert_eq!(Item::full(0, error).label(), error);
  }

  #[derive(Clone, Debug, PartialEq)]
  enum Act {
    Top,
    A,
    B,
    Quit,
  }

  /// Sections and dividers are never chosen; a submenu's items are, in their place: numbered
  /// depth first, each number its action.
  #[test]
  fn actions_are_numbered_depth_first() {
    let sub = Menu::new(vec![Item::button(Act::A, "A"), Item::Divider, Item::button(Act::B, "B")]);
    let quit = Item::button(Act::Quit, "Quit").key(Shortcut::char('q').cmd());
    let menu = Menu::new(vec![
      Item::section("Head"),
      Item::button(Act::Top, "Top"),
      Item::Divider,
      Item::submenu("Sub", sub),
      quit.role(ButtonRole::Destructive),
    ]);
    let (tags, actions) = menu.tagged();
    assert_eq!(actions, [Act::Top, Act::A, Act::B, Act::Quit]);
    assert_eq!(tags.items[4].action(), Some(&3));
    let Item::Submenu { menu: sub, .. } = &tags.items[3] else { panic!() };
    assert_eq!((sub.items[0].action(), sub.items[2].action()), (Some(&1), Some(&2)));
    assert!(!menu.items[0].is_enabled() && menu.items[4].is_destructive());
    let label = if cfg!(target_os = "macos") { "⌘Q" } else { "Ctrl+Q" };
    assert_eq!(menu.items[4].key_label(), label);
  }
}
