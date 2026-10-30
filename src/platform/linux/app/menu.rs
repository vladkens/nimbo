// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A [Menu] as `com.canonical.dbusmenu` serves it: its items numbered depth first (the root 0,
//! then each item, separators and submenus too, in the order they show), each item's
//! properties, and the layout tree `GetLayout` returns, as plain data ([V]).

use super::super::dbus::V;
use super::hotkey::keysym;
use crate::Modifiers;
use crate::keys::Shortcut;
use crate::tray::{Item, Menu};

/// An item of the menu by its number: the root (`item` none) or an item, and its children's
/// numbers.
pub struct Node<'a> {
  pub item: Option<&'a Item>,
  pub children: Vec<usize>,
}

/// The menu's items by number, the root first.
pub fn nodes(menu: &Menu) -> Vec<Node<'_>> {
  fn add<'a>(items: &'a [Item], out: &mut Vec<Node<'a>>) -> Vec<usize> {
    let mut ids = Vec::new();
    for it in items {
      let id = out.len();
      out.push(Node { item: Some(it), children: Vec::new() });
      ids.push(id);
      if let Some(sub) = it.submenu.as_ref().filter(|_| !it.separator && !it.header) {
        out[id].children = add(&sub.items, out);
      }
    }

    ids
  }

  let mut out = vec![Node { item: None, children: Vec::new() }];
  out[0].children = add(&menu.items, &mut out);
  out
}

/// The number of the item at `path` (indices into the menu, then its submenus).
pub fn at(nodes: &[Node], path: &[usize]) -> Option<usize> {
  path.iter().try_fold(0, |id, &i| nodes.get(id)?.children.get(i).copied())
}

/// Item `id`'s dbusmenu properties: `label` (its `_` doubled, a single one being a mnemonic),
/// `enabled`, `type` for a separator, `toggle-type` and `toggle-state` when checkable,
/// `children-display` for a submenu, `shortcut`.
pub fn props(node: &Node) -> V {
  let Some(it) = node.item else { return V::dict(vec![("children-display", V::s("submenu"))]) };
  if it.separator {
    return V::dict(vec![("type", V::s("separator"))]);
  }

  let mut p =
    vec![("label", V::S(it.label.replace('_', "__"))), ("enabled", V::B(it.enabled && !it.header))];
  if !it.header && (it.checkable || it.checked) {
    p.extend([("toggle-type", V::s("checkmark")), ("toggle-state", V::I(it.checked as i32))]);
  }

  if !node.children.is_empty() {
    p.push(("children-display", V::s("submenu")));
  }

  if let Some(keys) = it.key.as_deref().and_then(shortcut) {
    p.push(("shortcut", V::Array("as", vec![V::Array("s", keys.into_iter().map(V::S).collect())])));
  }

  V::dict(p)
}

/// An item's shortcut as dbusmenu writes one: the modifiers (`Control`, `Alt`, `Shift`,
/// `Super`), then the key's keysym name.
fn shortcut(text: &str) -> Option<Vec<String>> {
  let s = Shortcut::parse(text).ok()?;
  let mods = [
    (Modifiers::CONTROL, "Control"),
    (Modifiers::ALT, "Alt"),
    (Modifiers::SHIFT, "Shift"),
    (Modifiers::META, "Super"),
  ];
  let mut out: Vec<String> =
    mods.iter().filter(|m| s.mods.contains(m.0)).map(|m| m.1.into()).collect();
  out.push(keysym(s.key)?);
  Some(out)
}

/// The layout of item `id` and its children `depth` levels down (all for -1): `(ia{sv}av)`, the
/// items' numbers past the root's `base` higher.
pub fn layout(nodes: &[Node], id: usize, depth: i32, base: i32) -> V {
  let node = &nodes[id];
  let kids = match depth {
    0 => Vec::new(),
    _ => {
      (node.children.iter()).map(|&c| V::Var(Box::new(layout(nodes, c, depth - 1, base)))).collect()
    }
  };

  let id = if id == 0 { 0 } else { base + id as i32 };
  V::Tuple(vec![V::I(id), props(node), V::Array("v", kids)])
}

/// The menu as the tray's `describe` shows it: `label|key|on or off|enabled or disabled`, then
/// `|header` for a header (served as a disabled item), `-` a separator, a submenu's items
/// indented.
pub fn describe(nodes: &[Node], id: usize, depth: usize, out: &mut Vec<String>) {
  for &c in &nodes[id].children {
    let Some(it) = nodes[c].item else { continue };
    let pad = "  ".repeat(depth);
    if it.separator {
      out.push(format!("{pad}-"));
      continue;
    }

    let on = if it.checked { "on" } else { "off" };
    let enabled = if it.enabled { "enabled" } else { "disabled" };
    let head = if it.header { "|header" } else { "" };
    out.push(format!("{pad}{}|{}|{on}|{enabled}{head}", it.label, it.key_label()));
    describe(nodes, c, depth + 1, out);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn menu() -> Menu {
    let sub =
      Menu::new(vec![Item::new("a", "A_1").checked(true), Item::new("b", "B").checked(false)]);
    Menu::new(vec![
      Item::header("Head"),
      Item::separator(),
      Item::new("sub", "Sub").submenu(sub),
      Item::new("quit", "Quit").key("cmd+q"),
    ])
  }

  #[test]
  fn items_are_numbered_depth_first() {
    let m = menu();
    let n = nodes(&m);
    assert_eq!(n.len(), 7);
    assert_eq!(n[0].children, [1, 2, 3, 6]);
    assert_eq!(n[3].children, [4, 5]);
    assert_eq!((at(&n, &[2, 1]), at(&n, &[3]), at(&n, &[9])), (Some(5), Some(6), None));
  }

  #[test]
  fn a_header_is_disabled_and_has_no_items() {
    let sub = Menu::new(vec![Item::new("a", "A")]);
    let m = Menu::new(vec![Item::header("Head").enabled(true).submenu(sub)]);
    let n = nodes(&m);
    assert_eq!((n.len(), n[1].children.len()), (2, 0));
    let entry = |k: &str, v: V| V::Entry(Box::new(V::s(k)), Box::new(V::Var(Box::new(v))));
    let V::Array(_, head) = props(&n[1]) else { panic!() };
    assert!(head.contains(&entry("enabled", V::B(false))));
  }

  #[test]
  fn properties_and_layout() {
    let m = menu();
    let n = nodes(&m);
    let entry = |k: &str, v: V| V::Entry(Box::new(V::s(k)), Box::new(V::Var(Box::new(v))));
    let V::Array(_, sep) = props(&n[2]) else { panic!() };
    assert_eq!(sep, [entry("type", V::s("separator"))]);
    let V::Array(_, a) = props(&n[4]) else { panic!() };
    assert!(a.contains(&entry("label", V::s("A__1"))));
    assert!(a.contains(&entry("toggle-state", V::I(1))));
    // Checkable but off: the box shows empty, and turns on in place when it changes.
    let V::Array(_, b) = props(&n[5]) else { panic!() };
    assert!(b.contains(&entry("toggle-type", V::s("checkmark"))));
    assert!(b.contains(&entry("toggle-state", V::I(0))));
    let V::Array(_, head) = props(&n[1]) else { panic!() };
    assert!(!head.iter().any(|e| matches!(e, V::Entry(k, _) if **k == V::s("toggle-type"))));
    let V::Array(_, quit) = props(&n[6]) else { panic!() };
    let keys = V::Array("as", vec![V::Array("s", vec![V::s("Control"), V::s("q")])]);
    assert!(quit.contains(&entry("shortcut", keys)));
    assert_eq!(layout(&n, 0, -1, 0).ty(), "(ia{sv}av)");
    let V::Tuple(root) = layout(&n, 0, 1, 100) else { panic!() };
    assert_eq!(root[0], V::I(0), "the root keeps 0");
    let V::Array(_, kids) = &root[2] else { panic!() };
    let V::Var(sub) = &kids[2] else { panic!() };
    assert!(matches!(&**sub, V::Tuple(t) if t[0] == V::I(103)), "numbered past the base");
    assert!(matches!(&**sub, V::Tuple(t) if t[2] == V::Array("v", vec![])), "one level only");
  }

  #[test]
  fn describes_like_the_other_platforms() {
    let m = menu();
    let mut out = Vec::new();
    describe(&nodes(&m), 0, 0, &mut out);
    let want = [
      "Head||off|disabled|header",
      "-",
      "Sub||off|enabled",
      "  A_1||on|enabled",
      "  B||off|enabled",
      "Quit|Ctrl+Q|off|enabled",
    ];
    assert_eq!(out, want);
  }
}
