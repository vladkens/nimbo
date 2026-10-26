// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The clipboard for a paste of the app's own on Linux ([crate::clipboard]): GTK's CLIPBOARD
//! selection, read target by target (each a wait in a nested main loop, as GTK's own reads), and
//! served by the app: GTK asks for a target's data at each read (`gtk_clipboard_set_with_data`),
//! and tells when another owner takes the selection (its clear callback), which ends what the app
//! served. On Wayland, where only the app with the keyboard focus can set the clipboard, the
//! RemoteDesktop session's clipboard does all of it ([super::remote]) once the desktop gave it,
//! its targets the MIME types; GTK's otherwise. X11 and Wayland keep no change count. On the main
//! thread while the app runs.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString, c_char, c_int, c_uint};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use super::super::dl::fns;
use super::super::{CLIPBOARD, Fp, Gtk, P, lib, load};
use super::remote;
use super::shell::running;
use crate::clipboard::Snapshot;

// What the plain-text clipboard ([super::super::clipboard]) doesn't need.
fns!(Selection {
  fn gtk_clipboard_wait_for_targets(c: P, targets: *mut *mut P, n: *mut c_int) -> c_int;
  fn gtk_clipboard_wait_for_contents(c: P, target: P) -> P;
  fn gtk_clipboard_set_with_data(c: P, t: *const Target, n: c_uint, get: Fp, clear: Fp, data: P)
    -> c_int;
  fn gtk_clipboard_set_can_store(c: P, targets: *const Target, n: c_int);
  fn gtk_clipboard_clear(c: P);
  fn gtk_selection_data_get_data_with_length(d: P, len: *mut c_int) -> *const u8;
  fn gtk_selection_data_get_target(d: P) -> P;
  fn gtk_selection_data_get_data_type(d: P) -> P;
  fn gtk_selection_data_get_format(d: P) -> c_int;
  fn gtk_selection_data_set(d: P, kind: P, format: c_int, data: *const u8, len: c_int);
  fn gtk_selection_data_free(d: P);
  fn gdk_atom_name(atom: P) -> *mut c_char;
});

/// `GtkTargetEntry`.
#[repr(C)]
struct Target {
  target: *const c_char,
  flags: c_uint,
  info: c_uint,
}

/// The targets an offer's text goes as (UTF-8 each), then KDE's marker that clipboard managers
/// skip.
const TEXT: [&str; 3] = ["UTF8_STRING", "text/plain;charset=utf-8", "text/plain"];
const MARKER: (&str, &[u8]) = ("x-kde-passwordManagerHint", b"secret");
/// Targets about the selection, not data in it.
pub(super) const META: [&str; 5] = ["TARGETS", "TIMESTAMP", "MULTIPLE", "SAVE_TARGETS", "DELETE"];

type OnRead = Box<dyn FnMut()>;

/// What the app serves: its number, the types, and who hears the text read (an offer's).
struct Served {
  id: u64,
  types: Vec<(String, Vec<u8>)>,
  on_read: Option<OnRead>,
}

thread_local! {
  static SERVED: RefCell<Option<Served>> = const { RefCell::new(None) };
  static NEXT: Cell<u64> = const { Cell::new(1) };
  /// A snapshot reads: the app's own reads of its offer are not the paste's.
  static SNAPSHOT: Cell<bool> = const { Cell::new(false) };
}

/// The functions and the CLIPBOARD selection, while the app runs.
fn clip() -> Option<(&'static Selection, &'static Gtk, P)> {
  static SELECTION: OnceLock<Option<Selection>> = OnceLock::new();
  let s = SELECTION.get_or_init(|| Selection::load(&lib()?)).as_ref().filter(|_| running())?;
  let g = load()?;
  Some((s, g, unsafe { (g.gtk_clipboard_get)(CLIPBOARD) }))
}

pub fn change_count() -> Option<u64> {
  None
}

pub fn holds(id: u64) -> bool {
  SERVED.with_borrow(|s| s.as_ref().is_some_and(|s| s.id == id))
}

pub fn readable() -> bool {
  true
}

pub fn snapshot() -> Option<Snapshot> {
  if let Some(r) = remote::clipboard() {
    return Some(remote::snapshot(&r));
  }

  let (s, g, c) = clip()?;
  let (mut atoms, mut n) = (null_mut(), 0);
  let mut snapshot = Snapshot::default();
  // No targets: empty, or an owner that did not answer, which a restore must not empty.
  if unsafe { (s.gtk_clipboard_wait_for_targets)(c, &mut atoms, &mut n) } == 0 {
    snapshot.missing = 1;
    return Some(snapshot);
  }

  let mut types = Vec::new();
  // SAFETY: GTK filled `n` atoms, freed below.
  for &atom in unsafe { std::slice::from_raw_parts(atoms, n.max(0) as usize) } {
    let name = unsafe { (s.gdk_atom_name)(atom) };
    let kind = unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned();
    unsafe { (g.g_free)(name.cast()) };
    if META.contains(&kind.as_str()) {
      continue;
    }

    SNAPSHOT.set(true);
    let data = contents(s, c, atom);
    SNAPSHOT.set(false);
    match data {
      Some(data) => types.push((kind, data)),
      None => snapshot.missing += 1,
    }
  }

  unsafe { (g.g_free)(atoms.cast()) };
  if !types.is_empty() {
    snapshot.items.push(types);
  }

  Some(snapshot)
}

/// The owner's data for `target`, waited for: bytes of the target's own type, which a restore
/// serves as it (`TEXT` answered as `COMPOUND_TEXT`, 16- or 32-bit data count as missing).
fn contents(s: &Selection, c: P, target: P) -> Option<Vec<u8>> {
  let data = unsafe { (s.gtk_clipboard_wait_for_contents)(c, target) };
  if data.is_null() {
    return None;
  }

  let (mut len, mut p) = (0, null());
  let kind = unsafe { (s.gtk_selection_data_get_data_type)(data) };
  if kind == target && unsafe { (s.gtk_selection_data_get_format)(data) } == 8 {
    p = unsafe { (s.gtk_selection_data_get_data_with_length)(data, &mut len) };
  }

  // SAFETY: the selection data holds `len` bytes, copied before it is freed.
  let bytes = (!p.is_null() && len >= 0)
    .then(|| unsafe { std::slice::from_raw_parts(p, len as usize) }.to_vec());
  unsafe { (s.gtk_selection_data_free)(data) };
  bytes
}

/// Served by the app until another owner takes the selection, and kept after it quits where a
/// clipboard manager stores it.
pub fn restore(snapshot: Snapshot) -> bool {
  let (types, remote): (Vec<_>, _) =
    (snapshot.items.into_iter().flatten().collect(), remote::clipboard());
  if types.is_empty() && remote.is_some() {
    // The portal can't empty the clipboard: what it asks for of the app's own is not there.
    SERVED.set(None);
    return true;
  }

  let Some((s, _, c)) = clip() else { return false };
  if types.is_empty() {
    // What the app owns goes (an offer, its plain text); GTK leaves another owner's alone.
    unsafe { (s.gtk_clipboard_clear)(c) };
    return true;
  }

  let served = serve(types, None).is_some();
  if served && remote.is_none() {
    unsafe { (s.gtk_clipboard_set_can_store)(c, null(), 0) };
  }

  served
}

pub fn offer(text: &str, on_read: Option<OnRead>) -> Option<u64> {
  let mut types: Vec<_> = TEXT.iter().map(|t| (t.to_string(), text.as_bytes().to_vec())).collect();
  types.push((MARKER.0.into(), MARKER.1.to_vec()));
  serve(types, on_read)
}

/// Takes the selection, serving `types`.
fn serve(types: Vec<(String, Vec<u8>)>, on_read: Option<OnRead>) -> Option<u64> {
  let id = NEXT.replace(NEXT.get() + 1);
  if let Some(r) = remote::clipboard() {
    remote::offer(&r, types.iter().map(|t| t.0.clone()).collect()).then_some(())?;
    SERVED.set(Some(Served { id, types, on_read }));
    return Some(id);
  }

  let (s, _, c) = clip()?;
  let names: Vec<CString> =
    types.iter().map(|(t, _)| CString::new(t.as_str()).unwrap_or_default()).collect();
  let targets: Vec<Target> = (names.iter().enumerate())
    .map(|(i, n)| Target { target: n.as_ptr(), flags: 0, info: i as c_uint })
    .collect();
  // GTK copies the targets, and clears what the app served before (its `clear`).
  let (get, clear, n) = (get as Fp, clear as Fp, targets.len() as c_uint);
  if unsafe { (s.gtk_clipboard_set_with_data)(c, targets.as_ptr(), n, get, clear, id as P) } == 0 {
    return None;
  }

  SERVED.set(Some(Served { id, types, on_read }));
  Some(id)
}

/// `GtkClipboardGetFunc`: a reader wants target number `info` of what `data` (an id) serves.
unsafe extern "C" fn get(_c: P, selection: P, info: c_uint, data: P) {
  let Some((s, _, _)) = clip() else { return };
  let Some(bytes) = read(Some(data as u64), |i, _| i == info as usize) else { return };
  let len = bytes.len().min(c_int::MAX as usize) as c_int;
  unsafe {
    let target = (s.gtk_selection_data_get_target)(selection);
    (s.gtk_selection_data_set)(selection, target, 8, bytes.as_ptr(), len);
  }
}

/// The bytes of the type `pick` finds (by its number and name) of what the app serves (as `id`,
/// when given); a read of the text that is not a snapshot's tells the offer's reader first.
pub(super) fn read(id: Option<u64>, pick: impl Fn(usize, &str) -> bool) -> Option<Vec<u8>> {
  let (id, bytes, text) = SERVED.with_borrow(|served| {
    let served = served.as_ref().filter(|s| id.is_none_or(|id| s.id == id))?;
    let (_, (kind, bytes)) = served.types.iter().enumerate().find(|(i, t)| pick(*i, &t.0))?;
    Some((served.id, bytes.clone(), kind != MARKER.0))
  })?;
  if !text || SNAPSHOT.get() {
    return Some(bytes);
  }

  let Some(mut on_read) = SERVED.with_borrow_mut(|s| s.as_mut()?.on_read.take()) else {
    return Some(bytes);
  };
  on_read();
  // Kept unless the selection went meanwhile: the next read tells again.
  SERVED.with_borrow_mut(|s| {
    if let Some(s) = s.as_mut().filter(|s| s.id == id) {
      s.on_read.get_or_insert(on_read);
    }
  });
  Some(bytes)
}

/// What the app serves now, its types and their bytes.
pub(super) fn served() -> Option<Vec<(String, Vec<u8>)>> {
  SERVED.with_borrow(|s| s.as_ref().map(|s| s.types.clone()))
}

/// Another owner took the clipboard: what the app served goes.
pub(super) fn lost() {
  SERVED.set(None);
}

/// `GtkClipboardClearFunc`: another owner took the selection (or the app replaced it).
unsafe extern "C" fn clear(_c: P, data: P) {
  SERVED.with_borrow_mut(|s| _ = s.take_if(|s| s.id == data as u64));
}
