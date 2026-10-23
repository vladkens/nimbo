// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The tray on Linux: a StatusNotifierItem (`org.kde.StatusNotifierItem` at
//! `/StatusNotifierItem`, KDE's spec that freedesktop.org hosts) under a name of its own, with
//! its menu as `com.canonical.dbusmenu` at `/MenuBar` ([super::menu]), both exported through
//! GDBus on the main loop. The item registers with `org.kde.StatusNotifierWatcher` whenever one
//! appears, so a panel that starts later or restarts shows it.
//!
//! The icon is a template filled white on a dark appearance, black on a light one, at 22 and
//! 44 px (`IconPixmap`, ARGB in network order). `ItemIsMenu`: a click opens the menu. Panels ask
//! `AboutToShow` as it opens ([crate::tray::Tray::on_open] runs then) and fetch the layout; a
//! menu set while it is open waits until it closes (`Event` "closed" on the root, or a click),
//! and at the latest until it opens next, as hosts don't all send "closed".

use std::cell::{Cell, RefCell};
use std::ffi::c_uint;
use std::ptr::null;
use std::rc::Rc;
use std::sync::OnceLock;

use super::super::{Fp, P, S};
use super::dbus::{self, Gv, V, VTable};
use super::menu::{self, nodes};
use crate::Main;
use crate::tray::{Menu, OnItem, OnOpen, TrayIcon};

const SNI: &str = "org.kde.StatusNotifierItem";
const SNI_PATH: &str = "/StatusNotifierItem";
const MENU: &str = "com.canonical.dbusmenu";
const MENU_PATH: &str = "/MenuBar";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
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
  icon: RefCell<(u32, u32)>,
  tooltip: RefCell<String>,
  menu: RefCell<Menu>,
  revision: Cell<u32>,
  on_open: RefCell<Option<OnOpen>>,
  on_item: RefCell<Option<OnItem>>,
  /// Between `AboutToShow` of the root and its close (or a click).
  open: Cell<bool>,
  /// The last menu set while it was open.
  pending: RefCell<Option<Menu>>,
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
    let tray = Self::detached(icon, tooltip);
    // A connection of its own: two trays' objects would share the paths on one.
    match dbus::private_session() {
      Ok(conn) => *tray.0.bus.borrow_mut() = Some(tray.connect(conn)),
      Err(e) => eprintln!("nimbo: the tray shows nothing: {e}"),
    }

    tray
  }

  pub fn detached(icon: TrayIcon, tooltip: &str) -> Self {
    let tray = Self(Rc::new(Inner {
      pixmaps: RefCell::new(V::Array("(iiay)", Vec::new())),
      icon: RefCell::new((0, 0)),
      tooltip: RefCell::new(tooltip.into()),
      menu: RefCell::default(),
      revision: Cell::new(1),
      on_open: RefCell::default(),
      on_item: RefCell::default(),
      open: Cell::new(false),
      pending: RefCell::default(),
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

  /// The icon's pixmaps, tinted for the appearance now.
  fn paint(&self, icon: &TrayIcon) {
    let tint = if crate::platform::appearance().dark { [255; 3] } else { [0; 3] };
    let pixmap = |side: u32| {
      let rgba = icon.pixels(side, tint);
      let argb = rgba.chunks(4).flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
      V::Tuple(vec![V::I(side as i32), V::I(side as i32), V::Bytes(argb)])
    };
    *self.0.pixmaps.borrow_mut() = V::Array("(iiay)", SIDES.map(pixmap).to_vec());
    *self.0.icon.borrow_mut() = (SIDES[0], SIDES[0]);
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
    if self.0.open.get() {
      *self.0.pending.borrow_mut() = Some(menu.clone());
      return;
    }

    self.0.pending.take();
    self.apply(menu.clone());
  }

  /// Serves `menu` from now on, a new revision the host is told of.
  fn apply(&self, menu: Menu) {
    *self.0.menu.borrow_mut() = menu;
    let rev = self.0.revision.get() + 1;
    self.0.revision.set(rev);
    self.emit(MENU_PATH, MENU, "LayoutUpdated", V::Tuple(vec![V::U(rev), V::I(0)]));
  }

  /// The menu closed: the one set meanwhile, if any, is served.
  fn closed(&self) {
    self.0.open.set(false);
    if let Some(menu) = self.0.pending.take() {
      self.apply(menu);
    }
  }

  pub fn on_open(&self, f: OnOpen) {
    *self.0.on_open.borrow_mut() = Some(f);
  }

  pub fn on_item(&self, f: OnItem) {
    *self.0.on_item.borrow_mut() = Some(f);
  }

  /// `AboutToShow` of the root: the app refreshes the menu; whether the layout changed.
  fn about_to_show(&self) -> bool {
    self.closed();
    let rev = self.0.revision.get();
    if let Some(mut f) = self.0.on_open.take() {
      f(&mut Main::here(), &crate::tray::Tray(self.clone()));
      self.0.on_open.borrow_mut().get_or_insert(f);
    }

    self.0.open.set(true);
    self.0.revision.get() != rev
  }

  /// An `Event` on item `id`: a click goes to the app by the item's id, after the menu closed.
  fn event(&self, id: i32, kind: &str) {
    match kind {
      "clicked" => {
        let choice: Option<String> = {
          let menu = self.0.menu.borrow();
          let n = nodes(&menu);
          let it = usize::try_from(id).ok().and_then(|i| n.get(i)?.item);
          it.filter(|it| !it.separator && it.submenu.is_none()).map(|it| it.id.clone())
        };
        self.closed();
        let (Some(choice), Some(mut f)) = (choice, self.0.on_item.take()) else { return };
        f(&mut Main::here(), &crate::tray::Tray(self.clone()), &choice);
        self.0.on_item.borrow_mut().get_or_insert(f);
      }
      "closed" if id == 0 => self.closed(),
      _ => {}
    }
  }

  pub fn describe(&self) -> Vec<String> {
    let (w, h) = *self.0.icon.borrow();
    let mut out = vec![format!("icon {w}x{h}")];
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
      self.event(id as i32, "clicked");
    }
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
      (SNI, "IconName" | "OverlayIconName" | "AttentionIconName" | "AttentionMovieName") => s(""),
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
        let menu = self.0.menu.borrow();
        let n = nodes(&menu);
        let Some(id) = usize::try_from(parent).ok().filter(|&i| i < n.len()) else {
          return dbus::fail(inv, "org.freedesktop.DBus.Error.InvalidArgs", "no such item");
        };
        V::Tuple(vec![V::U(self.0.revision.get()), menu::layout(&n, id, depth)])
      }
      (MENU, "GetGroupProperties") => {
        let menu = self.0.menu.borrow();
        let n = nodes(&menu);
        let found = ints(args.child(0)).into_iter().filter_map(|id| {
          let node = n.get(usize::try_from(id).ok()?)?;
          Some(V::Tuple(vec![V::I(id), menu::props(node)]))
        });
        V::Tuple(vec![V::Array("(ia{sv})", found.collect())])
      }
      (MENU, "GetProperty") => {
        let (id, name) = (args.child(0).i32(), args.child(1).str());
        let menu = self.0.menu.borrow();
        let n = nodes(&menu);
        let props = usize::try_from(id).ok().and_then(|i| n.get(i)).map(menu::props);
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
