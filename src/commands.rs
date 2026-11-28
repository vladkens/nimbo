// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The menu bar's commands, as SwiftUI's `.commands`: the standard menus of a Mac app (the app
//! menu, File, Edit, View, Window, Help) and the app's own ([Commands]), its menus after View, as
//! `CommandMenu`, and its items before, after or in place of a standard group
//! ([CommandGroupPlacement]), as `CommandGroup`. macOS shows them as the app's menu bar
//! ([crate::Main::set_commands]); Windows and Linux have none.

use std::iter;

use crate::keys::{Command, EDIT_KEYS, Shortcut, shortcut};
use crate::menu::{Item, Menu, map};
use crate::t;

/// A group of the standard menus, where the app's items go ([Commands::before],
/// [Commands::after], [Commands::replacing]), as SwiftUI's `CommandGroupPlacement`. A group with
/// no standard items stays empty until the app adds some.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandGroupPlacement {
  /// The app menu's About {name}.
  AppInfo,
  /// Settings… ([Commands::settings]).
  AppSettings,
  /// Services, which the system fills.
  SystemServices,
  /// Hide {name}, Hide Others, Show All.
  AppVisibility,
  /// Quit {name}.
  AppTermination,
  /// File's first group.
  NewItem,
  /// Close Window.
  SaveItem,
  ImportExport,
  PrintItem,
  /// Edit's Undo and Redo.
  UndoRedo,
  /// Cut, Copy, Paste, Select All.
  Pasteboard,
  TextEditing,
  TextFormatting,
  /// View's first group.
  Toolbar,
  /// Enter Full Screen.
  Sidebar,
  /// Window's Minimize and Zoom.
  WindowSize,
  /// Bring All to Front; the system lists the windows after it.
  WindowArrangement,
  /// Help, where the system adds its search field.
  Help,
}

use CommandGroupPlacement as P;

/// The standard menus' groups, a divider between sections: the app menu, File, Edit, View,
/// Window, Help.
const MENUS: [&[&[P]]; 6] = [
  &[
    &[P::AppInfo],
    &[P::AppSettings],
    &[P::SystemServices],
    &[P::AppVisibility],
    &[P::AppTermination],
  ],
  &[&[P::NewItem], &[P::SaveItem, P::ImportExport], &[P::PrintItem]],
  &[&[P::UndoRedo], &[P::Pasteboard], &[P::TextEditing, P::TextFormatting]],
  &[&[P::Toolbar], &[P::Sidebar]],
  &[&[P::WindowSize], &[P::WindowArrangement]],
  &[&[P::Help]],
];

/// Where the app's items go beside a group's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum At {
  Before,
  After,
  Replacing,
}

/// The app's commands in the menu bar, as SwiftUI's `.commands { … }`: menus of its own and
/// items at the standard groups, each chosen by an action of the app's type `A`, as a [Menu]'s
/// are. Empty, the standard bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Commands<A> {
  groups: Vec<(CommandGroupPlacement, At, Vec<Item<A>>)>,
  menus: Vec<(String, Menu<A>)>,
}

impl<A> Default for Commands<A> {
  fn default() -> Self {
    Self { groups: Vec::new(), menus: Vec::new() }
  }
}

impl<A> Commands<A> {
  pub fn new() -> Self {
    Self::default()
  }

  /// A menu of the app's own, after View and the menus added before it, as SwiftUI's
  /// `CommandMenu("Track") { … }`.
  pub fn menu(mut self, name: &str, menu: Menu<A>) -> Self {
    self.menus.push((name.into(), menu));
    self
  }

  /// `items` before group `p`'s, as SwiftUI's `CommandGroup(before:)`.
  pub fn before(self, p: CommandGroupPlacement, items: Vec<Item<A>>) -> Self {
    self.group(p, At::Before, items)
  }

  /// `items` after group `p`'s, as SwiftUI's `CommandGroup(after:)`.
  pub fn after(self, p: CommandGroupPlacement, items: Vec<Item<A>>) -> Self {
    self.group(p, At::After, items)
  }

  /// `items` in place of group `p`'s, as SwiftUI's `CommandGroup(replacing:)`: none removes
  /// the group (`replacing(CommandGroupPlacement::SaveItem, vec![])`, no Close Window).
  pub fn replacing(self, p: CommandGroupPlacement, items: Vec<Item<A>>) -> Self {
    self.group(p, At::Replacing, items)
  }

  /// Settings… ⌘, in the app menu, running `action`, as SwiftUI's `Settings` scene adds it.
  pub fn settings(self, action: A) -> Self {
    let item = Item::full(action, &t!("Settings…")).key(Shortcut::char(',').cmd());
    self.replacing(P::AppSettings, vec![item])
  }

  fn group(mut self, p: CommandGroupPlacement, at: At, items: Vec<Item<A>>) -> Self {
    self.groups.push((p, at, items));
    self
  }
}

/// A standard item, on the system's own action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Standard {
  About,
  Services,
  Hide,
  HideOthers,
  ShowAll,
  Quit,
  Close,
  /// Edit's items, the command their key runs.
  Edit(Command),
  FullScreen,
  Minimize,
  Zoom,
  BringAllToFront,
}

/// An item's action in the bar: the system's or the app's.
#[derive(Clone, Debug, PartialEq)]
enum Action<A> {
  Standard(Standard),
  App(A),
}

/// The bar as the platform builds it: a submenu per menu, Window and Help the last two, every
/// item's action numbered depth first ([Menu::tagged]); and the standard actions by number.
pub(crate) type Bar = (Menu<usize>, Vec<Option<Standard>>);

impl<A: Clone> Commands<A> {
  /// The bar of the app `name`, and the app's actions by number. File, Edit and View go when
  /// nothing is left in them; the system fills the others.
  pub(crate) fn bar(&self, name: &str) -> (Bar, Vec<Option<A>>) {
    // Translators: the menu bar's menus, as macOS names them.
    let titles = [t!("File"), t!("Edit"), t!("View"), t!("Window"), t!("Help")].map(|t| t.as_str());
    let titles = iter::once(name).chain(titles);
    let mut menus = Vec::new();
    for (i, (title, sections)) in titles.zip(MENUS).enumerate() {
      let mut items = Vec::new();
      for section in sections {
        let part: Vec<_> = section.iter().flat_map(|&p| self.items(p, name)).collect();
        if !part.is_empty() && !items.is_empty() {
          items.push(Item::Divider);
        }

        items.extend(part);
      }

      if !items.is_empty() || !(1..=3).contains(&i) {
        menus.push(Item::submenu(title, Menu::new(items)));
      }

      if i == 3 {
        let app = |m: &Menu<A>| Menu::new(map(&m.items, &mut |a: &A| Action::App(a.clone())));
        menus.extend(self.menus.iter().map(|(name, m)| Item::submenu(name, app(m))));
      }
    }

    let (menu, actions) = Menu::new(menus).tagged();
    let standard =
      actions.iter().map(|a| if let Action::Standard(s) = a { Some(*s) } else { None });
    let standard = standard.collect();
    let app = actions.into_iter().map(|a| if let Action::App(a) = a { Some(a) } else { None });
    ((menu, standard), app.collect())
  }

  /// Group `p`'s items: the standard ones or the app's last replacement, with the app's before and
  /// after them in the order given.
  fn items(&self, p: CommandGroupPlacement, name: &str) -> Vec<Item<Action<A>>> {
    let (mut before, mut base, mut after) = (Vec::new(), standard(p, name), Vec::new());
    for (_, at, theirs) in self.groups.iter().filter(|g| g.0 == p) {
      let theirs = map(theirs, &mut |a: &A| Action::App(a.clone()));
      match at {
        At::Before => before.extend(theirs),
        At::After => after.extend(theirs),
        At::Replacing => base = theirs,
      }
    }

    [before, base, after].concat()
  }
}

/// Group `p`'s standard items, in the app `name`.
fn standard<A>(p: CommandGroupPlacement, name: &str) -> Vec<Item<Action<A>>> {
  use Command as C;
  use Standard as S;
  let item = |s, label: &str, key: Option<Shortcut>| {
    let it = Item::full(Action::Standard(s), label);
    if let Some(k) = key { it.key(k) } else { it }
  };
  let cmd = |c| Some(Shortcut::char(c).cmd());
  let edit = |c, label: &str| item(S::Edit(c), label, shortcut(EDIT_KEYS, c));
  match p {
    P::AppInfo => vec![item(S::About, &t!("About {name}", name), None)],
    P::SystemServices => vec![item(S::Services, &t!("Services"), None)],
    P::AppVisibility => vec![
      item(S::Hide, &t!("Hide {name}", name), cmd('h')),
      item(S::HideOthers, &t!("Hide Others"), Some(Shortcut::char('h').cmd().option())),
      item(S::ShowAll, &t!("Show All"), None),
    ],
    P::AppTermination => vec![item(S::Quit, &t!("Quit {name}", name), cmd('q'))],
    P::SaveItem => vec![item(S::Close, &t!("Close Window"), cmd('w'))],
    P::UndoRedo => vec![edit(C::Undo, &t!("Undo")), edit(C::Redo, &t!("Redo"))],
    P::Pasteboard => vec![
      edit(C::Cut, &t!("Cut")),
      edit(C::Copy, &t!("Copy")),
      edit(C::Paste, &t!("Paste")),
      edit(C::SelectAll, &t!("Select All")),
    ],
    P::Sidebar => {
      let key = Shortcut::char('f').cmd().ctrl();
      vec![item(S::FullScreen, &t!("Enter Full Screen"), Some(key))]
    }
    P::WindowSize => {
      vec![item(S::Minimize, &t!("Minimize"), cmd('m')), item(S::Zoom, &t!("Zoom"), None)]
    }
    P::WindowArrangement => vec![item(S::BringAllToFront, &t!("Bring All to Front"), None)],
    _ => Vec::new(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::locale::{self, Locale};

  /// Each menu as `Title: item, item, -, item`, a key after its item as `+⌘Q`.
  fn describe((bar, _): &Bar) -> Vec<String> {
    let item = |it: &Item<usize>| match (it, it.shortcut()) {
      (Item::Divider, _) => "-".to_owned(),
      (_, Some(k)) => format!("{}+{}", it.label(), k.label()),
      _ => it.label().to_owned(),
    };
    let menu = |it: &Item<usize>| {
      let Item::Submenu { menu, .. } = it else { panic!("{it:?}") };
      let items: Vec<_> = menu.items.iter().map(item).collect();
      format!("{}: {}", it.label(), items.join(", "))
    };
    bar.items.iter().map(menu).collect()
  }

  #[derive(Clone, Debug, PartialEq)]
  enum Act {
    New,
    Settings,
    Play,
    Updates,
  }

  /// The standard bar: the app menu, File, Edit, View, Window and Help, on the system's actions.
  #[test]
  fn the_standard_bar() {
    locale::set_thread(Locale::Tag("en"), &[]);
    let (bar, app) = Commands::<Act>::new().bar("Wavo");
    let cmd = |k: &str| Shortcut::parse(k).unwrap().label();
    let want = [
      format!(
        "Wavo: About Wavo, -, Services, -, Hide Wavo+{}, Hide Others+{}, Show All, -, Quit Wavo+{}",
        cmd("cmd+h"),
        cmd("option+cmd+h"),
        cmd("cmd+q")
      ),
      format!("File: Close Window+{}", cmd("cmd+w")),
      format!(
        "Edit: Undo+{}, Redo+{}, -, Cut+{}, Copy+{}, Paste+{}, Select All+{}",
        cmd("cmd+z"),
        cmd("shift+cmd+z"),
        cmd("cmd+x"),
        cmd("cmd+c"),
        cmd("cmd+v"),
        cmd("cmd+a")
      ),
      format!("View: Enter Full Screen+{}", cmd("ctrl+cmd+f")),
      format!("Window: Minimize+{}, Zoom, -, Bring All to Front", cmd("cmd+m")),
      "Help: ".to_owned(),
    ];
    assert_eq!(describe(&bar), want);
    assert!(app.iter().all(Option::is_none));
    let copy = bar.1.iter().position(|s| *s == Some(Standard::Edit(Command::Copy)));
    assert_eq!(copy, Some(10), "numbered depth first");
  }

  /// The app's items go before, after or in place of a group's, its menus after View; a menu
  /// left empty goes, and the window and help menus move with it.
  #[test]
  fn the_apps_commands_in_place() {
    locale::set_thread(Locale::Tag("en"), &[]);
    let play = Menu::new(vec![Item::button(Act::Play, "Play")]);
    let commands = Commands::new()
      .after(P::AppSettings, vec![Item::button(Act::Updates, "Check for Updates…")])
      .settings(Act::Settings)
      .replacing(P::NewItem, vec![Item::button(Act::New, "New Recording")])
      .before(P::NewItem, vec![Item::Divider])
      .replacing(P::SaveItem, vec![])
      .replacing(P::Sidebar, vec![])
      .menu("Track", play);

    let (bar, app) = commands.bar("Wavo");
    let menus = describe(&bar);
    let settings = Shortcut::parse("cmd+comma").unwrap().label();
    assert!(menus[0].starts_with(&format!(
      "Wavo: About Wavo, -, Settings…+{settings}, Check for Updates…, -, Services"
    )));

    assert_eq!(menus[1], "File: -, New Recording");
    assert_eq!(menus[3], "Track: Play", "no View: empty");
    assert_eq!(menus[4..].iter().map(|m| &m[..4]).collect::<Vec<_>>(), ["Wind", "Help"]);
    let acts: Vec<_> = app.into_iter().flatten().collect();
    assert_eq!(acts, [Act::Settings, Act::Updates, Act::New, Act::Play]);
    assert_eq!(bar.1.len(), bar.1.iter().filter(|s| s.is_some()).count() + 4);
  }
}
