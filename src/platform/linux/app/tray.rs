// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The tray on Linux: a StatusNotifierItem (`org.kde.StatusNotifierItem` at
//! `/StatusNotifierItem`, KDE's spec that freedesktop.org hosts) under a name of its own, with
//! its menu as `com.canonical.dbusmenu` at `/MenuBar` ([super::menu]), both exported through
//! GDBus on the main loop. The item registers with `org.kde.StatusNotifierWatcher` whenever one
//! appears, so a panel that starts later or restarts shows it.
//!
//! The icon is a template, served by name (`IconName`) as an SVG the panel fills with its own
//! text colour ([TrayIcon::svg]), so it follows the panel and not the app's appearance (Kubuntu's
//! panel is dark over light apps): Plasma by its colour scheme stylesheet, GNOME's AppIndicator
//! by the `-symbolic` name. The SVG goes in the user's icons ([crate::desktop::tray_icon]) with
//! no `IconThemePath`: Plasma 5 colours an icon from that path with the apps' colours, one it
//! finds itself with the panel's. Plasma draws an item's icon once, so it is sent again a moment
//! after Plasma's settings change ([follow_plasma]). `IconPixmap` (ARGB in network order, at 22
//! and 44 px) is the fallback for a host that reads no names, white on a dark appearance or
//! GNOME (its top bar is dark in either), black on a light one, painted again as the appearance
//! changes ([repaint]).
//! `ItemIsMenu`: a click opens the menu. Panels ask `AboutToShow` as it opens
//! ([crate::tray::Tray::on_open] runs then) and fetch the layout; a menu set while it is open
//! waits until it closes (`Event` "closed" on the root, or a click), until the host fetches it
//! whole, or at the latest until it opens next, as hosts don't all send "closed". Each menu set
//! numbers its items past the last one's: a host keeps what it fetched by number (Plasma draws
//! an item cached under a number with the properties of the one it has now), so a number never
//! means another item, and a click on one gone does nothing.

use std::cell::{Cell, RefCell};
use std::ffi::{CString, c_int, c_uint};
use std::ptr::{null, null_mut};
use std::rc::{Rc, Weak};
use std::sync::OnceLock;

use super::super::dbus::{self, Gv, V, VTable};
use super::super::dl::fns;
use super::super::{Fp, P, S, connect, lib, load};
use super::menu::{self, nodes};
use crate::tray::{Menu, State, TrayIcon};

const SNI: &str = "org.kde.StatusNotifierItem";
const SNI_PATH: &str = "/StatusNotifierItem";
const MENU: &str = "com.canonical.dbusmenu";
const MENU_PATH: &str = "/MenuBar";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
/// Plasma's settings its panel's colours follow: its style, and the colour scheme.
const PLASMA: [&str; 2] = ["plasmarc", "kdeglobals"];

fns!(Files {
  fn g_file_new_for_path(path: S) -> P;
  fn g_file_monitor_file(file: P, flags: c_int, cancel: P, err: *mut P) -> P;
});

/// The icon's sides in pixels: a panel's 22 px, and twice that at scale 2.
const SIDES: [u32; 2] = [22, 44];

const SNI_XML: &std::ffi::CStr = cr#"<node><interface name="org.kde.StatusNotifierItem">
<property name="Category" type="s" access="read"/><property name="Id" type="s" access="read"/>
<property name="Title" type="s" access="read"/><property name="Status" type="s" access="read"/>
<property name="WindowId" type="i" access="read"/><property name="IconName" type="s" access="read"/>
<property name="IconPixmap" type="a(iiay)" access="read"/>
<property name="OverlayIconName" type="s" access="read"/>
<property name="OverlayIconPixmap" type="a(iiay)" access="read"/>
<property name="AttentionIconName" type="s" access="read"/>
<property name="AttentionIconPixmap" type="a(iiay)" access="read"/>
<property name="AttentionMovieName" type="s" access="read"/>
<property name="ToolTip" type="(sa(iiay)ss)" access="read"/>
<property name="ItemIsMenu" type="b" access="read"/><property name="Menu" type="o" access="read"/>
<property name="IconThemePath" type="s" access="read"/>
<method name="ContextMenu"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="SecondaryActivate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="Scroll"><arg type="i" direction="in"/><arg type="s" direction="in"/></method>
<signal name="NewTitle"/><signal name="NewIcon"/><signal name="NewAttentionIcon"/>
<signal name="NewOverlayIcon"/><signal name="NewToolTip"/>
<signal name="NewStatus"><arg type="s"/></signal>
</interface></node>"#;

const MENU_XML: &std::ffi::CStr = cr#"<node><interface name="com.canonical.dbusmenu">
<property name="Version" type="u" access="read"/><property name="TextDirection" type="s" access="read"/>
<property name="Status" type="s" access="read"/><property name="IconThemePath" type="as" access="read"/>
<method name="GetLayout"><arg type="i" direction="in"/><arg type="i" direction="in"/>
<arg type="as" direction="in"/><arg type="u" direction="out"/><arg type="(ia{sv}av)" direction="out"/></method>
<method name="GetGroupProperties"><arg type="ai" direction="in"/><arg type="as" direction="in"/>
<arg type="a(ia{sv})" direction="out"/></method>
<method name="GetProperty"><arg type="i" direction="in"/><arg type="s" direction="in"/>
<arg type="v" direction="out"/></method>
<method name="Event"><arg type="i" direction="in"/><arg type="s" direction="in"/>
<arg type="v" direction="in"/><arg type="u" direction="in"/></method>
<method name="EventGroup"><arg type="a(isvu)" direction="in"/><arg type="ai" direction="out"/></method>
<method name="AboutToShow"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
<method name="AboutToShowGroup"><arg type="ai" direction="in"/><arg type="ai" direction="out"/>
<arg type="ai" direction="out"/></method>
<signal name="ItemsPropertiesUpdated"><arg type="a(ia{sv})"/><arg type="a(ias)"/></signal>
<signal name="LayoutUpdated"><arg type="u"/><arg type="i"/></signal>
<signal name="ItemActivationRequested"><arg type="i"/><arg type="u"/></signal>
</interface></node>"#;

/// The tray's parts on the session bus.
struct Bus {
  conn: P,
  name: String,
  objects: [c_uint; 2],
  own: c_uint,
  watch: c_uint,
  /// The name is ours and a watcher runs: registered with it.
  owned: Cell<bool>,
  watcher: Cell<bool>,
}

struct Inner {
  /// The icon at [SIDES], tinted, as `IconPixmap` sends it.
  pixmaps: RefCell<V>,
  icon: RefCell<TrayIcon>,
  /// `IconName`: the icon's SVG in the user's icons, empty until it is written there.
  name: RefCell<String>,
  tooltip: RefCell<String>,
  menu: RefCell<Menu>,
  revision: Cell<u32>,
  /// What the menu's numbers ([menu::nodes]) are past the root's on the bus.
  base: Cell<i32>,
  /// Open between `AboutToShow` of the root and its close (or a click).
  state: State,
  bus: RefCell<Option<Bus>>,
}

impl Drop for Inner {
  fn drop(&mut self) {
    let Some(b) = self.bus.take() else { return };
    for id in b.objects.into_iter().filter(|&id| id != 0) {
      dbus::unexport(b.conn, id);
    }

    dbus::unown(b.own);
    dbus::unwatch(b.watch);
    dbus::close(b.conn);
  }
}

#[derive(Clone)]
pub struct Tray(Rc<Inner>);

impl Tray {
  pub fn new(icon: TrayIcon, tooltip: &str) -> Self {
    let tray = Self::detached(icon.clone(), tooltip);
    // A connection of its own: two trays' objects would share the paths on one.
    match dbus::private_session() {
      Ok(conn) => *tray.0.bus.borrow_mut() = Some(tray.connect(conn)),
      Err(e) => eprintln!("nimbo: the tray shows nothing: {e}"),
    }

    TRAYS.with_borrow_mut(|t| t.push(Rc::downgrade(&tray.0)));
    static FOLLOW: std::sync::Once = std::sync::Once::new();
    FOLLOW.call_once(follow_plasma);
    tray.paint(&icon);
    tray
  }

  pub fn detached(icon: TrayIcon, tooltip: &str) -> Self {
    let tray = Self(Rc::new(Inner {
      pixmaps: RefCell::new(V::Array("(iiay)", Vec::new())),
      icon: RefCell::new(icon.clone()),
      name: RefCell::default(),
      tooltip: RefCell::new(tooltip.into()),
      menu: RefCell::default(),
      revision: Cell::new(1),
      base: Cell::new(0),
      state: State::default(),
      bus: RefCell::default(),
    }));
    tray.paint(&icon);
    tray
  }

  /// Exports the item and its menu, owns a name for them and watches for a watcher.
  fn connect(&self, conn: P) -> Bus {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let name = format!("{SNI}-{}-{n}", std::process::id());
    let data = || dbus::weak(&self.0);
    let objects = [
      dbus::export(conn, SNI_PATH, SNI_XML, SNI, vtable(), data()),
      dbus::export(conn, MENU_PATH, MENU_XML, MENU, vtable(), data()),
    ];
    let own = dbus::own(conn, &name, on_owned as Fp, null(), data());
    let watch = dbus::watch(conn, WATCHER, on_watcher as Fp, on_no_watcher as Fp, data());
    let (owned, watcher) = (Cell::new(false), Cell::new(false));
    Bus { conn, name, objects, own, watch, owned, watcher }
  }

  /// Tells the watcher, once the name is ours and one runs.
  fn register(&self) {
    let bus = self.0.bus.borrow();
    let Some(b) = bus.as_ref().filter(|b| b.owned.get() && b.watcher.get()) else { return };
    let args = V::Tuple(vec![V::S(b.name.clone())]);
    let path = "/StatusNotifierWatcher";
    if let Err(e) =
      dbus::call(b.conn, WATCHER, path, WATCHER, "RegisterStatusNotifierItem", args, 2000)
    {
      eprintln!("nimbo: the tray item was not registered: {e}");
    }
  }

  fn emit(&self, path: &str, iface: &str, signal: &str, args: V) {
    if let Some(b) = self.0.bus.borrow().as_ref() {
      dbus::emit(b.conn, path, iface, signal, args);
    }
  }

  /// The icon's pixmaps, tinted for the appearance now, and on KDE's bus its SVG by name.
  /// Elsewhere only the pixmaps: GNOME's AppIndicator shows a name it can't find yet (its shell
  /// looks at the folder again only after 5 s) as a placeholder, and in Flatpak or a Snap the
  /// panel can't reach the app's data folder.
  fn paint(&self, icon: &TrayIcon) {
    let gnome = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.contains("GNOME"));
    let boxed = ["FLATPAK_ID", "SNAP"].iter().any(|v| std::env::var_os(v).is_some());
    let tint = if gnome || crate::platform::appearance().dark { [255; 3] } else { [0; 3] };
    let pixmap = |side: u32| {
      let rgba = icon.pixels(side, tint);
      let argb = rgba.chunks(4).flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
      V::Tuple(vec![V::I(side as i32), V::I(side as i32), V::Bytes(argb)])
    };
    *self.0.pixmaps.borrow_mut() = V::Array("(iiay)", SIDES.map(pixmap).to_vec());
    *self.0.icon.borrow_mut() = icon.clone();
    if self.0.bus.borrow().is_none() || !kde() || boxed {
      return;
    }

    static FIRST: std::sync::Once = std::sync::Once::new();
    let (data, id, mut first) = (crate::paths::data_dir(""), super::shell::info().id, false);
    FIRST.call_once(|| first = true);
    let name = crate::desktop::tray_icon(&data, id, &icon.svg(SIDES[1], tint), first);
    if let (Err(e), true) = (&name, first) {
      eprintln!("nimbo: the tray icon goes as pixels, its file was not written: {e}");
    }

    *self.0.name.borrow_mut() = name.unwrap_or_default();
  }

  pub fn set_icon(&self, icon: TrayIcon) {
    self.paint(&icon);
    self.emit(SNI_PATH, SNI, "NewIcon", V::Tuple(Vec::new()));
  }

  pub fn set_tooltip(&self, tooltip: &str) {
    *self.0.tooltip.borrow_mut() = tooltip.into();
    self.emit(SNI_PATH, SNI, "NewToolTip", V::Tuple(Vec::new()));
    self.emit(SNI_PATH, SNI, "NewTitle", V::Tuple(Vec::new()));
  }

  pub fn set_menu(&self, menu: &Menu) {
    if !self.0.state.hold(menu) {
      self.apply(menu.clone());
    }
  }

  /// Serves `menu` from now on, a new revision the host is told of.
  fn apply(&self, menu: Menu) {
    let gone = nodes(&self.0.menu.replace(menu)).len() as i32;
    self.0.base.set(self.0.base.get() + gone);
    let rev = self.0.revision.get() + 1;
    self.0.revision.set(rev);
    self.emit(MENU_PATH, MENU, "LayoutUpdated", V::Tuple(vec![V::U(rev), V::I(0)]));
  }

  /// The menu closed: the one set meanwhile, if any, is served.
  fn closed(&self) {
    self.0.state.set_open(false);
    if let Some(menu) = self.0.state.released() {
      self.apply(menu);
    }
  }

  pub fn state(&self) -> &State {
    &self.0.state
  }

  /// `AboutToShow` of the root: the app refreshes the menu; whether the layout changed. An empty
  /// menu doesn't count as open: GNOME's AppIndicator asks when the item registers, before the
  /// app's menu and without a close, and nothing in it would be rebuilt under the person.
  fn about_to_show(&self) -> bool {
    self.closed();
    let rev = self.0.revision.get();
    self.0.state.opening(self);
    self.0.state.set_open(!self.0.menu.borrow().items.is_empty());
    self.0.revision.get() != rev
  }

  /// An `Event` on item `id`: a click goes to the app by the item's id, after the menu closed.
  fn event(&self, id: i32, kind: &str) {
    match kind {
      "clicked" => {
        let choice: Option<String> = {
          let menu = self.0.menu.borrow();
          let n = nodes(&menu);
          let it = self.index(id, n.len()).and_then(|i| n[i].item);
          it.filter(|it| !it.separator && it.submenu.is_none()).map(|it| it.id.clone())
        };
        self.closed();
        if let Some(choice) = choice {
          self.0.state.chosen(self, &choice);
        }
      }
      "closed" if id == 0 => self.closed(),
      _ => {}
    }
  }

  pub fn describe(&self) -> Vec<String> {
    let mut out = vec![format!("icon {0}x{0}", SIDES[0])];
    menu::describe(&nodes(&self.0.menu.borrow()), 0, 0, &mut out);
    out
  }

  pub fn opening(&self) {
    self.about_to_show();
  }

  pub fn closing(&self) {
    self.event(0, "closed");
  }

  pub fn choose(&self, path: &[usize]) {
    let id = menu::at(&nodes(&self.0.menu.borrow()), path);
    if let Some(id) = id {
      self.event(self.0.base.get() + id as i32, "clicked");
    }
  }

  /// The number of the item that is `id` on the bus, of a menu of `n` (the root's 0 its own).
  fn index(&self, id: i32, n: usize) -> Option<usize> {
    let i =
      if id == 0 { 0 } else { usize::try_from(id - self.0.base.get()).ok().filter(|&i| i > 0)? };
    (i < n).then_some(i)
  }

  // MARK: D-Bus

  fn property(&self, iface: &str, name: &str) -> Option<V> {
    let info = super::shell::info();
    let s = |s: &str| Some(V::s(s));
    let no_pixmaps = || Some(V::Array("(iiay)", Vec::new()));
    match (iface, name) {
      (SNI, "Category") => s("ApplicationStatus"),
      (SNI, "Id") => s(info.id),
      (SNI, "Title") => s(info.name),
      (SNI, "Status") => s("Active"),
      (SNI, "WindowId") => Some(V::I(0)),
      (SNI, "IconName") => s(&self.0.name.borrow()),
      (SNI, "OverlayIconName" | "AttentionIconName" | "AttentionMovieName") => s(""),
      (SNI, "IconThemePath") => s(""),
      (SNI, "IconPixmap") => Some(self.0.pixmaps.borrow().clone()),
      (SNI, "OverlayIconPixmap" | "AttentionIconPixmap") => no_pixmaps(),
      (SNI, "ToolTip") => {
        let tip = V::S(self.0.tooltip.borrow().clone());
        Some(V::Tuple(vec![V::s(""), V::Array("(iiay)", Vec::new()), tip, V::s("")]))
      }
      (SNI, "ItemIsMenu") => Some(V::B(true)),
      (SNI, "Menu") => Some(V::O(MENU_PATH.into())),
      (MENU, "Version") => Some(V::U(3)),
      (MENU, "TextDirection") => s("ltr"),
      (MENU, "Status") => s("normal"),
      (MENU, "IconThemePath") => Some(V::Array("s", Vec::new())),
      _ => None,
    }
  }

  fn call(&self, iface: &str, method: &str, args: &Gv, inv: P) {
    let ints = |v: Gv| (0..v.len()).map(|i| v.child(i).i32()).collect::<Vec<_>>();
    let out = match (iface, method) {
      // A host shows the menu itself when the item can't do these (Plasma calls `Activate` on
      // a click whatever `ItemIsMenu` says, and opens the menu when it fails).
      (SNI, "Activate" | "ContextMenu") => {
        return dbus::fail(inv, "org.freedesktop.DBus.Error.NotSupported", "the menu opens");
      }
      (SNI, _) => V::Tuple(Vec::new()),
      (MENU, "GetLayout") => {
        let (parent, depth) = (args.child(0).i32(), args.child(1).i32());
        // Fetched whole, the menu is built anew: the one that waited goes in.
        if parent == 0
          && let Some(menu) = self.0.state.take_pending()
        {
          self.apply(menu);
        }

        let menu = self.0.menu.borrow();
        let n = nodes(&menu);
        let Some(id) = self.index(parent, n.len()) else {
          return dbus::fail(inv, "org.freedesktop.DBus.Error.InvalidArgs", "no such item");
        };
        let layout = menu::layout(&n, id, depth, self.0.base.get());
        V::Tuple(vec![V::U(self.0.revision.get()), layout])
      }
      (MENU, "GetGroupProperties") => {
        let menu = self.0.menu.borrow();
        let n = nodes(&menu);
        let found = ints(args.child(0)).into_iter().filter_map(|id| {
          let node = &n[self.index(id, n.len())?];
          Some(V::Tuple(vec![V::I(id), menu::props(node)]))
        });
        V::Tuple(vec![V::Array("(ia{sv})", found.collect())])
      }
      (MENU, "GetProperty") => {
        let (id, name) = (args.child(0).i32(), args.child(1).str());
        let menu = self.0.menu.borrow();
        let n = nodes(&menu);
        let props = self.index(id, n.len()).map(|i| menu::props(&n[i]));
        let value = match props {
          Some(V::Array(_, entries)) => entries.into_iter().find_map(|e| match e {
            V::Entry(k, v) if *k == V::S(name.clone()) => Some(*v),
            _ => None,
          }),
          _ => None,
        };

        let Some(value) = value else {
          return dbus::fail(inv, "org.freedesktop.DBus.Error.InvalidArgs", "no such property");
        };
        V::Tuple(vec![value])
      }
      (MENU, "Event") => {
        dbus::reply(inv, V::Tuple(Vec::new()));
        return self.event(args.child(0).i32(), &args.child(1).str());
      }
      (MENU, "EventGroup") => {
        let all = args.child(0);
        let events: Vec<_> = (0..all.len())
          .map(|i| (all.child(i).child(0).i32(), all.child(i).child(1).str()))
          .collect();
        dbus::reply(inv, V::Tuple(vec![V::Array("i", Vec::new())]));
        for (id, kind) in events {
          self.event(id, &kind);
        }

        return;
      }
      (MENU, "AboutToShow") => {
        V::Tuple(vec![V::B(args.child(0).i32() == 0 && self.about_to_show())])
      }
      (MENU, "AboutToShowGroup") => {
        let changed = ints(args.child(0)).contains(&0) && self.about_to_show();
        let updated = if changed { vec![V::I(0)] } else { Vec::new() };
        V::Tuple(vec![V::Array("i", updated), V::Array("i", Vec::new())])
      }
      _ => return dbus::fail(inv, "org.freedesktop.DBus.Error.UnknownMethod", method),
    };

    dbus::reply(inv, out);
  }
}

thread_local! {
  /// The trays made, for [repaint].
  static TRAYS: RefCell<Vec<Weak<Inner>>> = const { RefCell::new(Vec::new()) };
  /// A [repaint] is due after Plasma's settings changed.
  static DUE: Cell<bool> = const { Cell::new(false) };
}

/// On KDE, watches [PLASMA] in the config folder: as either changes, the trays are painted again
/// a second later, once Plasma has read it, for the panel to draw the icon in its new colours.
/// The panel is Plasma's.
fn kde() -> bool {
  std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.contains("KDE"))
}

fn follow_plasma() {
  let Some(f) = lib().and_then(|l| Files::load(&l)).filter(|_| kde()) else { return };

  for name in PLASMA {
    let Ok(path) =
      CString::new(crate::paths::config_dir(name).into_os_string().into_encoded_bytes())
    else {
      continue;
    };

    // SAFETY: GIO's calls; the monitor lives as long as the process.
    unsafe {
      let file = (f.g_file_new_for_path)(path.as_ptr());
      let monitor = (f.g_file_monitor_file)(file, 0, null_mut(), null_mut());
      if !monitor.is_null() {
        connect(monitor, c"changed", on_plasma as Fp, 0);
      }
    }
  }
}

/// `changed` of a monitor: a [repaint] in a second, unless one is due.
unsafe extern "C" fn on_plasma(_: P, _: P, _: P, _: c_int, _: P) {
  if let Some(g) = load().filter(|_| !DUE.replace(true)) {
    unsafe { (g.g_timeout_add)(1000, on_due as Fp, null_mut()) };
  }
}

unsafe extern "C" fn on_due(_: P) -> c_int {
  DUE.set(false);
  repaint();
  0
}

/// The trays that live.
fn trays() -> Vec<Rc<Inner>> {
  TRAYS.with_borrow_mut(|t| {
    t.retain(|w| w.strong_count() > 0);
    t.iter().filter_map(Weak::upgrade).collect()
  })
}

/// Paints the icon of each tray again: the appearance changed, and so their pixmaps' tint.
pub fn repaint() {
  for t in trays().into_iter().map(Tray) {
    let icon = t.0.icon.borrow().clone();
    t.set_icon(icon);
  }
}

/// Whether a StatusNotifierItem host shows items now: a watcher with a host registered.
pub fn available() -> Result<(), String> {
  let bus = dbus::session()?;
  let args = V::Tuple(vec![V::s(WATCHER), V::s("IsStatusNotifierHostRegistered")]);
  let props = "org.freedesktop.DBus.Properties";
  let host = dbus::call(bus, WATCHER, "/StatusNotifierWatcher", props, "Get", args, 500);
  match host.map(|r| r.child(0).inner().bool()) {
    Ok(true) => Ok(()),
    _ => Err("no StatusNotifierItem host on the session bus: the panel shows no tray icons (GNOME shows them only with the AppIndicator extension)".into()),
  }
}

// MARK: Callbacks

/// The handlers both objects share: `GDBusInterfaceVTable`.
fn vtable() -> &'static VTable {
  static VT: OnceLock<VTable> = OnceLock::new();
  VT.get_or_init(|| VTable {
    method_call: on_call as Fp,
    get_property: on_get as Fp,
    set_property: null(),
    padding: [std::ptr::null_mut(); 8],
  })
}

/// The tray a callback's data points to, if it still lives.
unsafe fn tray(data: P) -> Option<Tray> {
  unsafe { dbus::upgrade(data) }.map(Tray)
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn on_call(_: P, _: S, _: S, iface: S, method: S, args: P, inv: P, data: P) {
  let Some(t) = (unsafe { tray(data) }) else {
    return dbus::fail(inv, "org.freedesktop.DBus.Error.UnknownObject", "the tray is gone");
  };
  t.call(&dbus::text(iface), &dbus::text(method), &Gv::borrowed(args), inv);
}

unsafe extern "C" fn on_get(_: P, _: S, _: S, iface: S, name: S, _: *mut P, data: P) -> P {
  let t = unsafe { tray(data) };
  let v = t.and_then(|t| t.property(&dbus::text(iface), &dbus::text(name)));
  v.map_or(std::ptr::null_mut(), |v| v.build())
}

unsafe extern "C" fn on_owned(_: P, _: S, data: P) {
  let Some(t) = (unsafe { tray(data) }) else { return };
  if let Some(b) = t.0.bus.borrow().as_ref() {
    b.owned.set(true);
  }

  t.register();
}

unsafe extern "C" fn on_watcher(_: P, _: S, _: S, data: P) {
  let Some(t) = (unsafe { tray(data) }) else { return };
  if let Some(b) = t.0.bus.borrow().as_ref() {
    b.watcher.set(true);
  }

  t.register();
}

unsafe extern "C" fn on_no_watcher(_: P, _: S, data: P) {
  if let Some(t) = unsafe { tray(data) }
    && let Some(b) = t.0.bus.borrow().as_ref()
  {
    b.watcher.set(false);
  }
}
