// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The menu model the tray ([crate::tray]) and context menus ([crate::Ui::context_menu]) share:
//! a [Menu] of [Item]s, each chosen by its id; section headers, separators, check marks,
//! shortcut labels, disabled and destructive items, submenus. Labels longer than [LABEL] are
//! shortened to fit ([Item::new]).

/// A menu's items, in the order they show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Menu {
  pub items: Vec<Item>,
}

impl Menu {
  pub fn new(items: Vec<Item>) -> Self {
    Self { items }
  }
}

/// A menu item: `id` is what comes back when it is chosen ([crate::tray::Tray::on_item],
/// [crate::Event::Menu]); one with a submenu, a header or a separator is never chosen.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
  pub id: String,
  pub label: String,
  pub enabled: bool,
  /// Shows a check mark when on.
  pub checked: bool,
  /// Can be checked ([Item::checked] sets it): Linux panels show an empty box while it is off,
  /// and a host adds the box to an item only as it builds it.
  pub checkable: bool,
  pub submenu: Option<Menu>,
  /// Its shortcut as [crate::Shortcut::parse] spells it (`cmd+comma`, `cmd+q`), shown beside it
  /// and, on macOS, working while the menu is open.
  pub key: Option<String>,
  /// A line between groups; the other fields are ignored.
  pub separator: bool,
  /// A section's title over the items after it ([Item::header]).
  pub header: bool,
  /// Deletes or discards: red where the menu has the style (the drawn one), plain elsewhere.
  pub destructive: bool,
}

impl Item {
  /// An item labelled `label`, a longer one than [LABEL] shortened ([Item::full] keeps it whole).
  pub fn new(id: &str, label: &str) -> Self {
    Self::full(id, &fit(label, LABEL))
  }

  /// An item showing all of `label`, however long.
  pub fn full(id: &str, label: &str) -> Self {
    let (enabled, checked, checkable, submenu, key) = (true, false, false, None, None);
    let (separator, header, destructive) = (false, false, false);
    let (id, label) = (id.into(), label.into());
    Self { id, label, enabled, checked, checkable, submenu, key, separator, header, destructive }
  }

  pub fn separator() -> Self {
    Self { separator: true, ..Self::new("", "") }
  }

  /// A section title, never chosen: a section header on macOS 14+, a disabled item elsewhere.
  pub fn header(label: &str) -> Self {
    Self { header: true, enabled: false, ..Self::new("", label) }
  }

  pub fn enabled(self, on: bool) -> Self {
    Self { enabled: on, ..self }
  }

  /// A check mark, shown when `on`.
  pub fn checked(self, on: bool) -> Self {
    Self { checked: on, checkable: true, ..self }
  }

  pub fn submenu(self, menu: Menu) -> Self {
    Self { submenu: Some(menu), ..self }
  }

  pub fn key(self, key: &str) -> Self {
    Self { key: Some(key.into()), ..self }
  }

  /// Marks it as one that deletes or discards.
  pub fn destructive(self) -> Self {
    Self { destructive: true, ..self }
  }

  /// The label of [Item::key] as the OS shows it beside the item: `⌘,` on macOS, `Ctrl+,`
  /// elsewhere; empty when it does not parse.
  pub fn key_label(&self) -> String {
    let key = self.key.as_deref().and_then(|k| crate::Shortcut::parse(k).ok());
    key.map(|k| k.label()).unwrap_or_default()
  }
}

/// The items of `menu` that can be chosen, depth first, in the order they show: the item
/// numbers the OS menus use (an index here is a tag, a command id or a D-Bus menu id).
#[cfg(any(test, windows, target_os = "macos", target_os = "linux"))]
pub(crate) fn choices(menu: &Menu) -> Vec<String> {
  fn walk(items: &[Item], out: &mut Vec<String>) {
    for it in items.iter().filter(|it| !it.separator && !it.header) {
      match &it.submenu {
        Some(sub) => walk(&sub.items, out),
        None => out.push(it.id.clone()),
      }
    }
  }

  let mut out = Vec::new();
  walk(&menu.items, &mut out);
  out
}

/// The most characters an item's label shows ([Item::new]): a menu keeps its width whatever the
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
    assert_eq!(Item::new("e", error).label, fit(error, LABEL));
    assert_eq!(Item::full("e", error).label, error);
  }

  /// Headers and separators are never chosen; a submenu's items are, in their place.
  #[test]
  fn choices_are_numbered_depth_first() {
    let sub = Menu::new(vec![Item::new("a", "A"), Item::separator(), Item::new("b", "B")]);
    let menu = Menu::new(vec![
      Item::header("Head"),
      Item::new("top", "Top"),
      Item::separator(),
      Item::new("sub", "Sub").submenu(sub),
      Item::new("quit", "Quit").key("cmd+q").destructive(),
    ]);
    assert_eq!(choices(&menu), ["top", "a", "b", "quit"]);
    assert!(menu.items[0].header && !menu.items[0].enabled && menu.items[4].destructive);
    let label = if cfg!(target_os = "macos") { "⌘Q" } else { "Ctrl+Q" };
    assert_eq!(menu.items[4].key_label(), label);
    assert_eq!(Item::new("x", "X").key("cmd+nope").key_label(), "");
  }
}
