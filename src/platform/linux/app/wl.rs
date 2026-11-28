// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The protocols GTK doesn't speak, on GTK's own `wl_display` (`libwayland-client`, loaded at run
//! time) and its default queue, so GDK's loop dispatches their events (a reader's request for the
//! clipboard the app serves comes any time): a registry of the app's and the globals it tells, and
//! requests marshalled from interfaces built at run time ([Interface::new]), their argument types
//! pointing at libwayland's own (`wl_seat`). The virtual keyboard ([super::vkbd]) and the
//! data-control clipboard ([super::datactl]) run on it. On the main thread while the app runs on
//! Wayland.

use std::cell::RefCell;
use std::ffi::{CStr, c_char, c_int};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use super::super::dl::{Lib, fns};
use super::super::{Fp, P, display, gtk, lib};

fns!(Wl {
  fn wl_display_roundtrip(d: P) -> c_int;
  fn wl_display_flush(d: P) -> c_int;
  fn wl_proxy_get_version(p: P) -> u32;
  fn wl_proxy_add_listener(p: P, listener: *const Fp, data: P) -> c_int;
  fn wl_proxy_marshal_array_flags(p: P, opcode: u32, interface: *const Interface, version: u32,
    flags: u32, args: *mut Arg) -> P;
});

fns!(GdkWayland {
  fn gdk_wayland_display_get_wl_display(display: P) -> P;
  fn g_main_context_is_owner(context: P) -> c_int;
});

/// `wl_interface`.
#[repr(C)]
pub(super) struct Interface {
  name: *const c_char,
  version: c_int,
  method_count: c_int,
  methods: *const Message,
  event_count: c_int,
  events: *const Message,
}

/// `wl_message`.
#[repr(C)]
struct Message {
  name: *const c_char,
  signature: *const c_char,
  types: *const *const Interface,
}

/// `wl_argument`: one request argument.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) union Arg {
  pub u: u32,
  pub i: i32,
  pub s: *const c_char,
  pub o: P,
}

/// An interface of messages, each its name, signature and the interfaces of its object
/// arguments (null for the others), leaked: libwayland reads it as long as a proxy lives.
pub(super) type Messages<'a> = &'a [(&'static CStr, &'static CStr, &'a [*const Interface])];

impl Interface {
  pub(super) fn new(name: &'static CStr, requests: Messages, events: Messages) -> *const Self {
    let list = |m: Messages| -> *const Message {
      let all: Vec<Message> = (m.iter())
        .map(|(name, signature, types)| Message {
          name: name.as_ptr(),
          signature: signature.as_ptr(),
          types: Box::leak(types.to_vec().into_boxed_slice()).as_ptr(),
        })
        .collect();
      Box::leak(all.into_boxed_slice()).as_ptr()
    };
    Box::into_raw(Box::new(Self {
      name: name.as_ptr(),
      version: 1,
      method_count: requests.len() as c_int,
      methods: list(requests),
      event_count: events.len() as c_int,
      events: list(events),
    }))
  }
}

/// libwayland's functions and the interfaces of its own that the protocols point at.
pub(super) struct Lw {
  wl: Wl,
  pub registry: *const Interface,
  pub seat: *const Interface,
}

// SAFETY: libwayland's interfaces are read only.
unsafe impl Send for Lw {}
unsafe impl Sync for Lw {}

pub(super) fn lw() -> Option<&'static Lw> {
  static LW: OnceLock<Option<Lw>> = OnceLock::new();
  (LW.get_or_init(|| {
    let l = Lib::open(&[c"libwayland-client.so.0"])?;
    // SAFETY: data symbols, their addresses wanted.
    let data = |name: &str| unsafe { l.sym::<*const Interface>(name) };
    let (registry, seat) = (data("wl_registry_interface\0")?, data("wl_seat_interface\0")?);
    Some(Lw { wl: Wl::load(&l)?, registry, seat })
  }))
  .as_ref()
}

/// GTK's connection, the app's registry on it, and the globals that told (interface, name,
/// version).
pub(super) struct Client {
  display: P,
  registry: P,
  globals: Vec<(String, u32, u32)>,
}

thread_local! {
  /// Made at the first use; `None` off Wayland.
  static CLIENT: RefCell<Option<Option<&'static Client>>> = const { RefCell::new(None) };
  static GLOBALS: RefCell<Vec<(String, u32, u32)>> = const { RefCell::new(Vec::new()) };
}

/// The client, made once on GTK's display: `None` off Wayland, before GTK runs, or off its main
/// thread, where GTK's display is not to be used.
pub(super) fn client() -> Option<&'static Client> {
  if !super::running() || !main() {
    return None;
  }

  CLIENT.with_borrow_mut(|c| *c.get_or_insert_with(|| connect().map(|c| &*Box::leak(Box::new(c)))))
}

/// The thread runs GTK's loop (it owns GLib's default context), the only one that may use GTK's
/// display.
pub(super) fn main() -> bool {
  static GDK: OnceLock<Option<GdkWayland>> = OnceLock::new();
  let g = GDK.get_or_init(|| GdkWayland::load(&lib()?)).as_ref();
  g.is_some_and(|g| unsafe { (g.g_main_context_is_owner)(null_mut()) } != 0)
}

fn connect() -> Option<Client> {
  if !display::wayland() {
    return None;
  }

  let (l, gdk) = (lw()?, GdkWayland::load(&lib()?)?);
  let display =
    unsafe { (gdk.gdk_wayland_display_get_wl_display)((gtk().gdk_display_get_default)()) };
  if display.is_null() {
    return None;
  }

  // `wl_display.get_registry`.
  let registry = marshal(display, 1, l.registry, &mut [Arg { o: null_mut() }]);
  listen(registry, &[global as Fp, removed as Fp]);
  unsafe { (l.wl.wl_display_roundtrip)(display) };
  Some(Client { display, registry, globals: GLOBALS.take() })
}

unsafe extern "C" fn global(_: P, _: P, id: u32, name: *const c_char, version: u32) {
  let name = unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned();
  GLOBALS.with_borrow_mut(|g| g.push((name, id, version)));
}

/// A global gone: what the app bound stays as long as the compositor keeps it.
unsafe extern "C" fn removed(_: P, _: P, _: u32) {}

impl Client {
  /// Binds the first global of `interface` at version 1; `None` where the compositor has none.
  pub(super) fn bind(&self, name: &CStr, interface: *const Interface) -> Option<P> {
    let name_str = name.to_str().ok()?;
    let &(_, id, _) = self.globals.iter().find(|(n, ..)| n == name_str)?;
    let mut args = [Arg { u: id }, Arg { s: name.as_ptr() }, Arg { u: 1 }, Arg { o: null_mut() }];
    let wl = &lw()?.wl;
    // `wl_registry.bind`.
    let p = unsafe {
      (wl.wl_proxy_marshal_array_flags)(self.registry, 0, interface, 1, 0, args.as_mut_ptr())
    };
    (!p.is_null()).then_some(p)
  }

  /// Sends what is queued and dispatches the events until the compositor answered (GTK's too, as
  /// a nested loop would): false when the connection broke.
  pub(super) fn roundtrip(&self) -> bool {
    lw().is_some_and(|l| unsafe { (l.wl.wl_display_roundtrip)(self.display) } >= 0)
  }

  /// Sends what is queued.
  pub(super) fn flush(&self) {
    if let Some(l) = lw() {
      unsafe { (l.wl.wl_display_flush)(self.display) };
    }
  }
}

/// Request `opcode` of `proxy`, its new object's interface `interface` (null when none).
pub(super) fn marshal(proxy: P, opcode: u32, interface: *const Interface, args: &mut [Arg]) -> P {
  let wl = &lw().expect("loaded before a request").wl;
  let version = unsafe { (wl.wl_proxy_get_version)(proxy) };
  unsafe {
    (wl.wl_proxy_marshal_array_flags)(proxy, opcode, interface, version, 0, args.as_mut_ptr())
  }
}

/// The destructor request `opcode` of `proxy`: the proxy goes with it.
pub(super) fn destroy(proxy: P, opcode: u32) {
  let wl = &lw().expect("loaded before a request").wl;
  let version = unsafe { (wl.wl_proxy_get_version)(proxy) };
  // `WL_MARSHAL_FLAG_DESTROY`.
  unsafe { (wl.wl_proxy_marshal_array_flags)(proxy, opcode, null(), version, 1, null_mut()) };
}

/// Listens to `proxy`'s events, `handlers` in their order, leaked as the proxy may outlive the
/// call.
pub(super) fn listen(proxy: P, handlers: &[Fp]) {
  let wl = &lw().expect("loaded before a listener").wl;
  let table = Box::leak(handlers.to_vec().into_boxed_slice());
  unsafe { (wl.wl_proxy_add_listener)(proxy, table.as_ptr(), null_mut()) };
}
