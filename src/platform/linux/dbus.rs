// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! D-Bus over GIO's GDBus, loaded at run time ([super::dl]): the session bus, calls (with a file
//! descriptor in the reply), signals, exported objects, the XDG desktop portal's requests, and
//! GVariants built from an owned tree ([V]), so what the tray and the portals get is plain Rust
//! data until it goes out (and testable without a bus).

use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::os::fd::{FromRawFd, OwnedFd};
use std::ptr::{null, null_mut};
use std::rc::{Rc, Weak};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use super::dl::fns;
use super::{Fp, P, S, lib};

/// `G_BUS_TYPE_SYSTEM` and `G_BUS_TYPE_SESSION`.
const SYSTEM: c_int = 1;
const SESSION: c_int = 2;
/// `G_DBUS_CALL_FLAGS_NO_AUTO_START`.
const NO_AUTO_START: c_int = 1;

fns!(Gio {
  fn g_bus_get_sync(bus: c_int, cancel: P, err: *mut P) -> P;
  fn g_dbus_address_get_for_bus_sync(bus: c_int, cancel: P, err: *mut P) -> *mut c_char;
  fn g_dbus_connection_new_for_address_sync(addr: S, flags: c_int, observer: P, cancel: P,
    err: *mut P) -> P;
  fn g_free(p: P);
  fn g_bus_own_name_on_connection(c: P, name: S, flags: c_int, acquired: Fp, lost: Fp, data: P,
    free: Fp) -> c_uint;
  fn g_bus_unown_name(id: c_uint);
  fn g_bus_watch_name_on_connection(c: P, name: S, flags: c_int, appeared: Fp, vanished: Fp,
    data: P, free: Fp) -> c_uint;
  fn g_bus_unwatch_name(id: c_uint);
  fn g_dbus_connection_get_unique_name(c: P) -> S;
  fn g_dbus_connection_close_sync(c: P, cancel: P, err: *mut P) -> c_int;
  fn g_object_ref(obj: P) -> P;
  fn g_object_unref(obj: P);
  fn g_dbus_connection_call_with_unix_fd_list_sync(c: P, name: S, path: S, iface: S,
    method: S, args: P, reply: P, flags: c_int, ms: c_int, fds: P, out_fds: *mut P, cancel: P,
    err: *mut P) -> P;
  fn g_unix_fd_list_get(list: P, i: c_int, err: *mut P) -> c_int;
  fn g_dbus_connection_signal_subscribe(c: P, sender: S, iface: S, member: S, path: S, arg0: S,
    flags: c_int, cb: Fp, data: P, free: Fp) -> c_uint;
  fn g_dbus_connection_signal_unsubscribe(c: P, id: c_uint);
  fn g_dbus_connection_emit_signal(c: P, dest: S, path: S, iface: S, name: S, args: P,
    err: *mut P) -> c_int;
  fn g_dbus_connection_register_object(c: P, path: S, iface: P, vtable: *const VTable, data: P,
    free: Fp, err: *mut P) -> c_uint;
  fn g_dbus_connection_unregister_object(c: P, id: c_uint) -> c_int;
  fn g_dbus_node_info_new_for_xml(xml: S, err: *mut P) -> P;
  fn g_dbus_node_info_lookup_interface(info: P, name: S) -> P;
  fn g_dbus_method_invocation_return_value(inv: P, v: P);
  fn g_dbus_method_invocation_return_dbus_error(inv: P, name: S, msg: S);
  fn g_variant_new_int32(v: i32) -> P;
  fn g_variant_new_uint32(v: u32) -> P;
  fn g_variant_new_boolean(v: c_int) -> P;
  fn g_variant_new_string(v: S) -> P;
  fn g_variant_new_object_path(v: S) -> P;
  fn g_variant_new_variant(v: P) -> P;
  fn g_variant_new_tuple(items: *const P, n: usize) -> P;
  fn g_variant_new_array(ty: S, items: *const P, n: usize) -> P;
  fn g_variant_new_dict_entry(k: P, v: P) -> P;
  fn g_variant_new_fixed_array(ty: S, data: *const c_void, n: usize, size: usize) -> P;
  fn g_variant_get_child_value(v: P, i: usize) -> P;
  fn g_variant_n_children(v: P) -> usize;
  fn g_variant_get_variant(v: P) -> P;
  fn g_variant_get_type_string(v: P) -> S;
  fn g_variant_get_int32(v: P) -> i32;
  fn g_variant_get_uint32(v: P) -> u32;
  fn g_variant_get_double(v: P) -> f64;
  fn g_variant_get_boolean(v: P) -> c_int;
  fn g_variant_get_string(v: P, len: *mut usize) -> S;
  fn g_variant_get_handle(v: P) -> i32;
  fn g_variant_lookup_value(dict: P, key: S, ty: S) -> P;
  fn g_variant_ref_sink(v: P) -> P;
  fn g_variant_unref(v: P);
  fn g_error_free(e: P);
});

/// `GDBusInterfaceVTable`: the exported object's handlers, then GLib's padding.
#[repr(C)]
pub struct VTable {
  pub method_call: Fp,
  pub get_property: Fp,
  pub set_property: Fp,
  pub padding: [P; 8],
}

// SAFETY: function pointers and nulls, never written.
unsafe impl Sync for VTable {}
unsafe impl Send for VTable {}

/// GIO's functions; `None` without GTK 3.
fn gio() -> Option<&'static Gio> {
  static GIO: OnceLock<Option<Gio>> = OnceLock::new();
  GIO.get_or_init(|| Gio::load(&lib()?)).as_ref()
}

fn f() -> &'static Gio {
  gio().expect("GIO is loaded before D-Bus is used")
}

/// A C string of `s`, NULs dropped (D-Bus strings have none).
pub fn cstr(s: &str) -> CString {
  CString::new(s.replace('\0', "")).unwrap_or_default()
}

/// The text of a C string GIO owns.
pub fn text(s: S) -> String {
  match s.is_null() {
    true => String::new(),
    false => unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned(),
  }
}

/// A GError's message, the error freed; empty for none.
pub fn error(err: P) -> String {
  if err.is_null() {
    return String::new();
  }

  // GError: a quark, a code, then the message.
  #[repr(C)]
  struct GError {
    domain: u32,
    code: c_int,
    message: *const c_char,
  }

  let msg = text(unsafe { (*(err as *const GError)).message });
  unsafe { (f().g_error_free)(err) };
  msg
}

/// The session bus (GIO's shared connection, referenced once and kept for the process); `Err`
/// with GIO's reason.
pub fn session() -> Result<P, String> {
  static BUS: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
  shared(SESSION, &BUS)
}

/// The system bus, as [session].
pub fn system() -> Result<P, String> {
  static BUS: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
  shared(SYSTEM, &BUS)
}

fn shared(kind: c_int, slot: &AtomicPtr<c_void>) -> Result<P, String> {
  let bus = slot.load(Ordering::Acquire);
  if !bus.is_null() {
    return Ok(bus);
  }

  let gio = gio().ok_or("GTK 3's GIO did not load")?;
  let mut err = null_mut();
  let bus = unsafe { (gio.g_bus_get_sync)(kind, null_mut(), &mut err) };
  if bus.is_null() {
    return Err(format!("no D-Bus bus {kind}: {}", error(err)));
  }

  // Another thread's reference, if it got there first, is GIO's shared connection too.
  slot.store(bus, Ordering::Release);
  Ok(bus)
}

/// A connection to the session bus of its own (`G_DBUS_CONNECTION_FLAGS_AUTHENTICATION_CLIENT |
/// MESSAGE_BUS_CONNECTION`), kept for the process: a portal call made first on it registers
/// the app's id for it, which the shared one, used for the settings first, can no longer do.
pub fn private_session() -> Result<P, String> {
  let g = gio().ok_or("GTK 3's GIO did not load")?;
  let mut err = null_mut();
  let addr = unsafe { (g.g_dbus_address_get_for_bus_sync)(SESSION, null_mut(), &mut err) };
  if addr.is_null() {
    return Err(format!("no session bus: {}", error(err)));
  }

  let bus = unsafe {
    (g.g_dbus_connection_new_for_address_sync)(addr, 1 | 8, null_mut(), null_mut(), &mut err)
  };
  unsafe { (g.g_free)(addr.cast()) };
  match bus.is_null() {
    true => Err(format!("no session bus: {}", error(err))),
    false => Ok(bus),
  }
}

/// Closes a connection of [private_session]'s and lets it go.
pub fn close(bus: P) {
  if bus.is_null() {
    return;
  }

  let mut err = null_mut();
  unsafe { (f().g_dbus_connection_close_sync)(bus, null_mut(), &mut err) };
  let _ = error(err);
  unsafe { (f().g_object_unref)(bus) };
}

/// Another reference to connection `bus` (as a number, to move to another thread, which GDBus
/// allows), for work that may outlive its owner's: closed meanwhile, its calls fail. [release]
/// lets it go.
pub fn retain(bus: P) -> usize {
  unsafe { (f().g_object_ref)(bus) as usize }
}

pub fn release(bus: usize) {
  unsafe { (f().g_object_unref)(bus as P) };
}

/// This connection's unique name (`:1.42`).
pub fn unique_name(bus: P) -> String {
  text(unsafe { (f().g_dbus_connection_get_unique_name)(bus) })
}

/// Calls `method`, waiting at most `ms`; the reply's tuple, owned.
pub fn call(
  bus: P,
  dest: &str,
  path: &str,
  iface: &str,
  method: &str,
  args: V,
  ms: c_int,
) -> Result<Gv, String> {
  call_with(bus, [dest, path, iface, method], args, ms, 0, null_mut())
}

/// The properties of `iface` at `dest`'s `path` (`GetAll`'s `a{sv}`), waiting at most `ms`,
/// without starting `dest` when it isn't running.
pub fn properties(bus: P, dest: &str, path: &str, iface: &str, ms: c_int) -> Result<Gv, String> {
  let names = [dest, path, "org.freedesktop.DBus.Properties", "GetAll"];
  let args = V::Tuple(vec![V::s(iface)]);
  Ok(call_with(bus, names, args, ms, NO_AUTO_START, null_mut())?.child(0))
}

/// Calls `method`, whose reply is a file descriptor (`h`, as the clipboard portal's
/// `SelectionRead`): the descriptor, owned.
pub fn call_fd(
  bus: P,
  dest: &str,
  path: &str,
  iface: &str,
  method: &str,
  args: V,
  ms: c_int,
) -> Result<OwnedFd, String> {
  let mut fds = null_mut();
  let reply = call_with(bus, [dest, path, iface, method], args, ms, 0, &mut fds)?;
  if fds.is_null() {
    return Err(format!("{method} gave no file descriptor"));
  }

  let mut err = null_mut();
  let i = unsafe { (f().g_variant_get_handle)(reply.child(0).0) };
  // A duplicate of the list's, which goes with its last reference.
  let fd = unsafe { (f().g_unix_fd_list_get)(fds, i, &mut err) };
  unsafe { (f().g_object_unref)(fds) };
  match fd {
    -1 => Err(format!("{method}: {}", error(err))),
    // SAFETY: GIO's duplicate, ours alone.
    fd => Ok(unsafe { OwnedFd::from_raw_fd(fd) }),
  }
}

/// [call] with `flags` (`GDBusCallFlags`), the reply's file descriptors in `fds` unless it is null.
fn call_with(
  bus: P,
  names: [&str; 4],
  args: V,
  ms: c_int,
  flags: c_int,
  fds: *mut P,
) -> Result<Gv, String> {
  let [d, p, i, m] = names.map(cstr);
  let mut err = null_mut();
  let args = args.build();
  let reply = unsafe {
    (f().g_dbus_connection_call_with_unix_fd_list_sync)(
      bus,
      d.as_ptr(),
      p.as_ptr(),
      i.as_ptr(),
      m.as_ptr(),
      args,
      null_mut(),
      flags,
      ms,
      null_mut(),
      fds,
      null_mut(),
      &mut err,
    )
  };
  match reply.is_null() {
    true => Err(error(err)),
    false => Ok(Gv(reply)),
  }
}

/// Subscribes `cb` (`GDBusSignalCallback`) to `member` of `iface` from `sender` only (GDBus
/// follows a well-known name's owner), on `path` (any when `None`); `data` ([weak]'s) is freed
/// when unsubscribed.
pub fn subscribe(
  bus: P,
  sender: &str,
  iface: &str,
  member: &str,
  path: Option<&str>,
  cb: Fp,
  (data, free): (P, Fp),
) -> c_uint {
  let (from, i, m, p) = (cstr(sender), cstr(iface), cstr(member), path.map(cstr));
  let path = p.as_ref().map_or(null(), |p| p.as_ptr());
  let sub = f().g_dbus_connection_signal_subscribe;
  unsafe { sub(bus, from.as_ptr(), i.as_ptr(), m.as_ptr(), path, null(), 0, cb, data, free) }
}

pub fn unsubscribe(bus: P, id: c_uint) {
  unsafe { (f().g_dbus_connection_signal_unsubscribe)(bus, id) };
}

/// Emits signal `name` of `iface` from `path` to everyone.
pub fn emit(bus: P, path: &str, iface: &str, name: &str, args: V) {
  let (p, i, n) = (cstr(path), cstr(iface), cstr(name));
  let mut err = null_mut();
  let args = args.build();
  unsafe {
    (f().g_dbus_connection_emit_signal)(
      bus,
      null(),
      p.as_ptr(),
      i.as_ptr(),
      n.as_ptr(),
      args,
      &mut err,
    )
  };
  let _ = error(err);
}

/// Exports interface `iface` of introspection `xml` at `path`, its calls to `vtable` with
/// `data` ([weak]'s, freed when unregistered); the registration id, 0 when it failed.
pub fn export(
  bus: P,
  path: &str,
  xml: &CStr,
  iface: &str,
  vtable: &'static VTable,
  (data, free): (P, Fp),
) -> c_uint {
  let g = f();
  let mut err = null_mut();
  let info = unsafe { (g.g_dbus_node_info_new_for_xml)(xml.as_ptr(), &mut err) };
  if info.is_null() {
    eprintln!("nimbo: D-Bus interface {iface}: {}", error(err));
    return 0;
  }

  // The node info is kept: the registration refers to it for as long as it lasts.
  let (i, p) = (cstr(iface), cstr(path));
  let iface_info = unsafe { (g.g_dbus_node_info_lookup_interface)(info, i.as_ptr()) };
  let id = unsafe {
    (g.g_dbus_connection_register_object)(bus, p.as_ptr(), iface_info, vtable, data, free, &mut err)
  };
  if id == 0 {
    eprintln!("nimbo: D-Bus object {path}: {}", error(err));
  }

  id
}

pub fn unexport(bus: P, id: c_uint) {
  unsafe { (f().g_dbus_connection_unregister_object)(bus, id) };
}

/// Owns well-known `name` on `bus`; `acquired` and `lost` are `GBusNameAcquiredCallback`s.
pub fn own(bus: P, name: &str, acquired: Fp, lost: Fp, (data, free): (P, Fp)) -> c_uint {
  let n = cstr(name);
  unsafe { (f().g_bus_own_name_on_connection)(bus, n.as_ptr(), 0, acquired, lost, data, free) }
}

pub fn unown(id: c_uint) {
  unsafe { (f().g_bus_unown_name)(id) };
}

/// Watches `name`'s owner: `appeared` (`GBusNameAppearedCallback`) as one has it, `vanished`
/// as it goes.
pub fn watch(bus: P, name: &str, appeared: Fp, vanished: Fp, (data, free): (P, Fp)) -> c_uint {
  let n = cstr(name);
  unsafe {
    (f().g_bus_watch_name_on_connection)(bus, n.as_ptr(), 0, appeared, vanished, data, free)
  }
}

pub fn unwatch(id: c_uint) {
  unsafe { (f().g_bus_unwatch_name)(id) };
}

/// A callback's data: a weak handle to `rc`, boxed, and the `GDestroyNotify` that frees it.
pub fn weak<T>(rc: &Rc<T>) -> (P, Fp) {
  (Box::into_raw(Box::new(Rc::downgrade(rc))) as P, free_weak::<T> as Fp)
}

unsafe extern "C" fn free_weak<T>(data: P) {
  drop(unsafe { Box::from_raw(data as *mut Weak<T>) });
}

/// What a callback's data from [weak] points to, if it still lives.
///
/// # Safety
/// `data` is [weak]'s for a `T`, not freed yet.
pub unsafe fn upgrade<T>(data: P) -> Option<Rc<T>> {
  unsafe { &*(data as *const Weak<T>) }.upgrade()
}

/// Answers a method call with `out`, a tuple.
pub fn reply(inv: P, out: V) {
  unsafe { (f().g_dbus_method_invocation_return_value)(inv, out.build()) };
}

/// Answers a method call with a D-Bus error.
pub fn fail(inv: P, name: &str, msg: &str) {
  let (n, m) = (cstr(name), cstr(msg));
  unsafe { (f().g_dbus_method_invocation_return_dbus_error)(inv, n.as_ptr(), m.as_ptr()) };
}

// MARK: Portals

/// The XDG desktop portal's name and object.
pub const PORTAL: &str = "org.freedesktop.portal.Desktop";
pub const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const MS: c_int = 2000;

/// A connection of the app's own to the portals, its `id` registered with
/// `org.freedesktop.host.portal.Registry` (xdg-desktop-portal 1.19+), which the backend keeps
/// what the user allowed under; older portals have no registry and take the id from the
/// process's systemd scope.
pub fn portal(id: &str) -> Result<P, String> {
  let bus = private_session()?;
  let args = V::Tuple(vec![V::s(id), V::dict(Vec::new())]);
  let registry = "org.freedesktop.host.portal.Registry";
  let _ = call(bus, PORTAL, PORTAL_PATH, registry, "Register", args, MS);
  Ok(bus)
}

/// Calls portal `iface`'s `method` on `bus` with `args` and options last: `opts`, a fresh
/// `handle_token` (and for a session its `session_handle_token`); `done` gets the request's
/// `Response` (its code, 0 for success, and results) with `data`. The `Response`'s subscription,
/// to end once it came.
pub fn request(
  bus: P,
  iface: &str,
  method: &str,
  mut args: Vec<V>,
  mut opts: Vec<(&str, V)>,
  done: Fp,
  data: (P, Fp),
) -> Result<c_uint, String> {
  static TOKENS: AtomicU32 = AtomicU32::new(0);
  let token = format!("nimbo{}_{}", std::process::id(), TOKENS.fetch_add(1, Ordering::Relaxed));
  // The request's object path, as the spec makes it: the sender's unique name without the
  // colon, dots as underscores.
  let sender = unique_name(bus).trim_start_matches(':').replace('.', "_");
  let path = format!("{PORTAL_PATH}/request/{sender}/{token}");
  opts.push(("handle_token", V::S(token.clone())));
  if method == "CreateSession" {
    opts.push(("session_handle_token", V::S(token)));
  }

  args.push(V::dict(opts));
  // Subscribed first: the response may come before the call returns.
  let request = "org.freedesktop.portal.Request";
  let sub = subscribe(bus, PORTAL, request, "Response", Some(&path), done, data);
  match call(bus, PORTAL, PORTAL_PATH, iface, method, V::Tuple(args), MS) {
    Ok(_) => Ok(sub),
    Err(e) => {
      unsubscribe(bus, sub);
      Err(format!("{method}: {e}"))
    }
  }
}

// MARK: Values

/// A GVariant to send, as owned data: built into one ([V::build]) only as it goes out.
#[derive(Clone, Debug, PartialEq)]
pub enum V {
  I(i32),
  U(u32),
  B(bool),
  S(String),
  /// An object path.
  O(String),
  /// Boxed in a variant (`v`).
  Var(Box<V>),
  Tuple(Vec<V>),
  /// An array of the element type (its type string), for when it is empty.
  Array(&'static str, Vec<V>),
  /// A dictionary entry, inside an array of `{kv}`.
  Entry(Box<V>, Box<V>),
  /// `ay`.
  Bytes(Vec<u8>),
}

impl V {
  pub fn s(s: &str) -> Self {
    V::S(s.into())
  }

  /// `a{sv}`: each value boxed in a variant.
  pub fn dict(items: Vec<(&str, V)>) -> Self {
    let entries =
      items.into_iter().map(|(k, v)| V::Entry(Box::new(V::s(k)), Box::new(V::Var(Box::new(v)))));
    V::Array("{sv}", entries.collect())
  }

  /// Its GVariant type string.
  #[cfg(test)]
  pub fn ty(&self) -> String {
    match self {
      V::I(_) => "i".into(),
      V::U(_) => "u".into(),
      V::B(_) => "b".into(),
      V::S(_) => "s".into(),
      V::O(_) => "o".into(),
      V::Var(_) => "v".into(),
      V::Tuple(items) => format!("({})", items.iter().map(V::ty).collect::<String>()),
      V::Array(elem, _) => format!("a{elem}"),
      V::Entry(k, v) => format!("{{{}{}}}", k.ty(), v.ty()),
      V::Bytes(_) => "ay".into(),
    }
  }

  /// The GVariant, a floating reference: the call or container it goes into takes it.
  pub fn build(&self) -> P {
    let g = f();
    let all = |items: &[V]| items.iter().map(V::build).collect::<Vec<_>>();
    unsafe {
      match self {
        V::I(v) => (g.g_variant_new_int32)(*v),
        V::U(v) => (g.g_variant_new_uint32)(*v),
        V::B(v) => (g.g_variant_new_boolean)(*v as c_int),
        V::S(s) => (g.g_variant_new_string)(cstr(s).as_ptr()),
        V::O(s) => (g.g_variant_new_object_path)(cstr(s).as_ptr()),
        V::Var(v) => (g.g_variant_new_variant)(v.build()),
        V::Tuple(items) => {
          let kids = all(items);
          (g.g_variant_new_tuple)(kids.as_ptr(), kids.len())
        }
        V::Array(elem, items) => {
          // A type string is a GVariantType (G_VARIANT_TYPE casts one).
          let ty = cstr(elem);
          let kids = all(items);
          let ty = if kids.is_empty() { ty.as_ptr() } else { null() };
          (g.g_variant_new_array)(ty, kids.as_ptr(), kids.len())
        }
        V::Entry(k, v) => (g.g_variant_new_dict_entry)(k.build(), v.build()),
        V::Bytes(b) => (g.g_variant_new_fixed_array)(c"y".as_ptr(), b.as_ptr().cast(), b.len(), 1),
      }
    }
  }
}

/// A GVariant we hold a reference to, let go when dropped.
pub struct Gv(pub P);

impl Drop for Gv {
  fn drop(&mut self) {
    unsafe { (f().g_variant_unref)(self.0) };
  }
}

impl Gv {
  /// A borrowed GVariant (a callback's parameters), referenced while this lives.
  pub fn borrowed(v: P) -> Self {
    Self(unsafe { (f().g_variant_ref_sink)(v) })
  }

  pub fn child(&self, i: usize) -> Gv {
    Gv(unsafe { (f().g_variant_get_child_value)(self.0, i) })
  }

  pub fn len(&self) -> usize {
    unsafe { (f().g_variant_n_children)(self.0) }
  }

  pub fn ty(&self) -> String {
    text(unsafe { (f().g_variant_get_type_string)(self.0) })
  }

  /// Unboxed from its variants.
  pub fn inner(self) -> Gv {
    let mut v = self;
    while v.ty() == "v" {
      v = Gv(unsafe { (f().g_variant_get_variant)(v.0) });
    }

    v
  }

  pub fn i32(&self) -> i32 {
    if self.ty() == "i" { unsafe { (f().g_variant_get_int32)(self.0) } } else { 0 }
  }

  pub fn u32(&self) -> u32 {
    if self.ty() == "u" { unsafe { (f().g_variant_get_uint32)(self.0) } } else { 0 }
  }

  pub fn f64(&self) -> f64 {
    if self.ty() == "d" { unsafe { (f().g_variant_get_double)(self.0) } } else { 0.0 }
  }

  pub fn bool(&self) -> bool {
    self.ty() == "b" && unsafe { (f().g_variant_get_boolean)(self.0) } != 0
  }

  /// A string or an object path; empty for anything else.
  pub fn str(&self) -> String {
    match matches!(self.ty().as_str(), "s" | "o" | "g") {
      true => text(unsafe { (f().g_variant_get_string)(self.0, null_mut()) }),
      false => String::new(),
    }
  }

  /// The value of `key` in an `a{sv}`, unboxed.
  pub fn get(&self, key: &str) -> Option<Gv> {
    let k = cstr(key);
    let v = unsafe { (f().g_variant_lookup_value)(self.0, k.as_ptr(), null()) };
    (!v.is_null()).then(|| Gv(v).inner())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn type_strings() {
    let layout =
      V::Tuple(vec![V::I(0), V::dict(vec![("label", V::s("A"))]), V::Array("v", vec![])]);
    assert_eq!(layout.ty(), "(ia{sv}av)");
    let pixmap = V::Array("(iiay)", vec![V::Tuple(vec![V::I(1), V::I(1), V::Bytes(vec![0; 4])])]);
    assert_eq!(pixmap.ty(), "a(iiay)");
    assert_eq!(V::Tuple(vec![V::U(3), V::O("/x".into()), V::B(true)]).ty(), "(uob)");
  }
}
