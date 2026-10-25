// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Files on Linux: the open and save panels a window asks for ([crate::FileDialog]) through
//! the XDG desktop portal's `org.freedesktop.portal.FileChooser`
//! (<https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.FileChooser.html>,
//! as of 2026-10), and files dragged onto a window, GTK 3's drag destination on its area.
//!
//! A panel is a portal request: its `Response` signal, subscribed before the call, brings the
//! picked file's URI to the window as [Event::FileDialog]; without a portal the answer is `None`
//! at once. A drag's files come from GTK only on request (`text/uri-list`): asked on its first
//! motion, they come with each [Event::DragOver] after it and with the [Event::Drop].

use std::cell::{Cell, RefCell};
use std::ffi::{OsString, c_int, c_uint, c_ulong};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::ptr::{null, null_mut};

use super::dbus::{self, Gv, V};
use super::{FALSE, Fp, P, S, TRUE, connect_id, gtk, lib, overlay, with_data};
use crate::scene::Point;
use crate::{Event, FileDialog};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const REQUEST: &str = "org.freedesktop.portal.Request";
/// The portal may start on the first call (D-Bus activation): it answers within this, in ms.
const MS: c_int = 5000;
/// `GDK_ACTION_COPY`; `GDK_DROP_START`, a `GdkEventType`.
const COPY: c_int = 2;
const DROP_START: c_int = 26;

/// `GtkTargetEntry`: the target, its flags and info.
#[repr(C)]
struct Target(S, c_uint, c_uint);

/// The drag over a window: its context, the pointer, and its files once GTK gave them.
struct Drag {
  ctx: P,
  pos: Point,
  paths: Vec<PathBuf>,
}

const NONE: Drag = Drag { ctx: null_mut(), pos: Point::new(0.0, 0.0), paths: Vec::new() };

thread_local! {
  static DRAG: RefCell<Drag> = const { RefCell::new(NONE) };
  /// The windows with a panel up, each with its request's object path.
  static OPEN: RefCell<Vec<(u64, String)>> = const { RefCell::new(Vec::new()) };
  static SUBSCRIBED: Cell<bool> = const { Cell::new(false) };
}

// MARK: Panels

/// Asks the portal for panel `d` over window `id` (its GtkWindow `window`), closing the one
/// still open for it: a portal that went away with it never answers. `Err` without a portal.
pub fn show(id: u64, window: P, d: &FileDialog) -> Result<(), String> {
  let bus = dbus::session()?;
  if let Some(old) = OPEN.with_borrow_mut(|o| Some(o.remove(o.iter().position(|r| r.0 == id)?).1)) {
    _ = dbus::call(bus, PORTAL, &old, REQUEST, "Close", V::Tuple(vec![]), MS);
  }

  // Every request's `Response`, subscribed before the first call: one may come before its call
  // returns.
  if !SUBSCRIBED.replace(true) {
    let cb = on_response as Fp;
    dbus::subscribe(bus, PORTAL, REQUEST, "Response", None, cb, (null_mut(), null()));
  }

  // The request's object path, by the portal's spec: the sender's unique name without the
  // colon, dots as underscores, then the token (one request per window at a time).
  let token = format!("nimbo{}_{id}", std::process::id());
  let sender = dbus::unique_name(bus).trim_start_matches(':').replace('.', "_");
  OPEN.with_borrow_mut(|o| o.push((id, format!("{PATH}/request/{sender}/{token}"))));
  let args = V::Tuple(vec![V::S(parent(window)), V::S(d.title.clone()), options(d, &token)]);
  let (chooser, method) = ("org.freedesktop.portal.FileChooser", ["OpenFile", "SaveFile"]);
  dbus::call(bus, PORTAL, PATH, chooser, method[d.save as usize], args, MS).map(drop).map_err(|e| {
    OPEN.with_borrow_mut(|o| o.retain(|r| r.0 != id));
    format!("no FileChooser portal: {e}")
  })
}

/// The portal's options for `d`: the request's token, modal, its types as one filter of
/// `*.ext` globs (either case: GTK's are case-sensitive), the folder it starts in and a save
/// panel's name.
fn options(d: &FileDialog, token: &str) -> V {
  let mut o = vec![("handle_token", V::s(token)), ("modal", V::B(true))];
  if !d.types.is_empty() {
    let name = d.types.iter().map(|t| format!("*.{t}")).collect::<Vec<_>>().join(", ");
    let globs = d.types.iter().flat_map(|t| [t.to_lowercase(), t.to_uppercase()]);
    let globs = globs.map(|t| V::Tuple(vec![V::U(0), V::S(format!("*.{t}"))])).collect();
    let filter = V::Tuple(vec![V::S(name), V::Array("(us)", globs)]);
    o.push(("filters", V::Array("(sa(us))", vec![filter])));
  }

  if let Some(dir) = &d.dir {
    o.push(("current_folder", V::Bytes([dir.as_os_str().as_bytes(), &[0]].concat())));
  }

  o.extend(d.name.as_ref().filter(|_| d.save).map(|n| ("current_name", V::s(n))));
  V::dict(o)
}

/// The portal's handle of `window`: `x11:<XID>` on X11; none on Wayland, where exporting one
/// takes a protocol of its own, so the panel comes as a window of its own.
fn parent(window: P) -> String {
  type Xid = unsafe extern "C" fn(P) -> c_ulong;
  let xid = lib().and_then(|l| unsafe { l.sym::<Xid>("gdk_x11_window_get_xid\0") });
  let gdk = unsafe { (gtk().gtk_widget_get_window)(window) };
  match xid {
    Some(f) if !gdk.is_null() && !overlay::wayland() => format!("x11:{:x}", unsafe { f(gdk) }),
    _ => String::new(),
  }
}

/// A request's `Response(u code, a{sv} results)`: for a panel of ours, the first of `uris` on
/// success (code 0) to its window.
unsafe extern "C" fn on_response(_: P, _: S, path: S, _: S, _: S, args: P, _: P) {
  let path = dbus::text(path);
  let id = OPEN.with_borrow_mut(|o| Some(o.remove(o.iter().position(|r| r.1 == path)?).0));
  let Some(id) = id else { return };
  let args = Gv::borrowed(args);
  let uris = (args.child(0).u32() == 0).then(|| args.child(1).get("uris")).flatten();
  let path = uris.filter(|u| u.len() > 0).and_then(|u| file(&u.child(0).str()));
  with_data(id as usize as P, false, |w| w.send(Event::FileDialog(path)));
}

// MARK: Drops

/// Makes `area`, window `id`'s, take files dragged onto it, its drop and data handled here (no
/// `GtkDestDefaults`: GTK neither highlights the area nor answers for us).
pub fn dest(area: P, id: u64) {
  let uris = Target(c"text/uri-list".as_ptr(), 0, 0);
  // SAFETY: GTK copies the target; each callback has its signal's signature.
  unsafe {
    (gtk().gtk_drag_dest_set)(area, 0, (&raw const uris).cast_mut().cast(), 1, COPY);
    connect_id(area, c"drag-motion", on_motion as Fp, 0, id);
    connect_id(area, c"drag-leave", on_leave as Fp, 0, id);
    connect_id(area, c"drag-drop", on_drop as Fp, 0, id);
    connect_id(area, c"drag-data-received", on_data as Fp, 0, id);
  }
}

/// The drag of `ctx`, which ends here, if it is the one known.
fn end_drag(ctx: P) -> Option<Drag> {
  DRAG.with_borrow_mut(|d| (d.ctx == ctx).then(|| std::mem::replace(d, NONE)))
}

/// Tells the source whether a drop would copy (it would with files), and the window where they
/// are.
fn status(ctx: P, time: u32, data: P) {
  let (pos, paths) = DRAG.with_borrow(|d| (d.pos, d.paths.clone()));
  unsafe { (gtk().gdk_drag_status)(ctx, if paths.is_empty() { 0 } else { COPY }, time) };
  if !paths.is_empty() {
    with_data(data, false, |w| w.send(Event::DragOver { pos, paths }));
  }
}

/// `drag-motion`: a new drag asks for its files, the answer waiting for them ([on_data]); a
/// known one answers with the files it has.
unsafe extern "C" fn on_motion(area: P, ctx: P, x: i32, y: i32, time: u32, data: P) -> c_int {
  let pos = Point::new(x as f32, y as f32);
  let new = DRAG.with_borrow_mut(|d| {
    let new = d.ctx != ctx;
    *d = Drag { ctx, pos, paths: if new { Vec::new() } else { std::mem::take(&mut d.paths) } };
    new
  });
  let target = unsafe { (gtk().gtk_drag_dest_find_target)(area, ctx, null_mut()) };
  match new && !target.is_null() {
    true => unsafe { (gtk().gtk_drag_get_data)(area, ctx, target, time) },
    false => status(ctx, time, data),
  }

  TRUE
}

/// `drag-leave`: the files left the window, unless GTK sends it before a drop.
unsafe extern "C" fn on_leave(_: P, ctx: P, _: u32, data: P) {
  let ev = unsafe { (gtk().gtk_get_current_event)() };
  let dropping = !ev.is_null() && unsafe { (gtk().gdk_event_get_event_type)(ev) } == DROP_START;
  if !ev.is_null() {
    unsafe { (gtk().gdk_event_free)(ev) };
  }

  if !dropping && end_drag(ctx).is_some_and(|d| !d.paths.is_empty()) {
    with_data(data, false, |w| w.send(Event::DragExit));
  }
}

/// `drag-drop`: the files known drop. Only a drag answered with files drops ([status]): they
/// came on its first motion.
unsafe extern "C" fn on_drop(_: P, ctx: P, x: i32, y: i32, time: u32, data: P) -> c_int {
  let paths = end_drag(ctx).map(|d| d.paths).unwrap_or_default();
  unsafe { (gtk().gtk_drag_finish)(ctx, !paths.is_empty() as c_int, FALSE, time) };
  if !paths.is_empty() {
    with_data(data, false, |w| w.send(Event::Drop { pos: Point::new(x as f32, y as f32), paths }));
  }

  TRUE
}

/// `drag-data-received`: the files of the drag, which it waited for.
unsafe extern "C" fn on_data(_: P, ctx: P, _: i32, _: i32, sel: P, _: u32, t: u32, data: P) {
  let mut len = 0;
  let ptr = unsafe { (gtk().gtk_selection_data_get_data_with_length)(sel, &mut len) };
  let paths = match ptr.is_null() || len <= 0 {
    true => Vec::new(),
    // SAFETY: GTK's data of `len` bytes, alive during this callback.
    false => uri_list(unsafe { std::slice::from_raw_parts(ptr, len as usize) }),
  };

  if DRAG.with_borrow_mut(|d| (d.ctx == ctx).then(|| d.paths = paths).is_some()) {
    status(ctx, t, data);
  }
}

// MARK: URIs

/// The files of a `text/uri-list` (RFC 2483: a URI a line, `#` comments): its `file://` URIs.
fn uri_list(bytes: &[u8]) -> Vec<PathBuf> {
  let text = String::from_utf8_lossy(bytes);
  text.lines().map(str::trim).filter(|l| !l.starts_with('#')).filter_map(file).collect()
}

/// The path of a local `file://` URI (no host, or `localhost`), percent-decoded; `None` for
/// another scheme or host: a file on another machine is not the local one at its path.
fn file(uri: &str) -> Option<PathBuf> {
  let rest = uri.strip_prefix("file://localhost").or(uri.strip_prefix("file://"))?;
  let b = rest.starts_with('/').then_some(rest.as_bytes())?;
  let (mut out, mut i) = (Vec::with_capacity(b.len()), 0);
  while let Some(&c) = b.get(i) {
    let hex = |j: usize| b.get(i + j).and_then(|&h| (h as char).to_digit(16));
    let (byte, n) = match (c, hex(1), hex(2)) {
      (b'%', Some(h), Some(l)) => ((h * 16 + l) as u8, 3),
      _ => (c, 1),
    };

    out.push(byte);
    i += n;
  }

  Some(PathBuf::from(OsString::from_vec(out)))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn file_uris_decode_to_paths() {
    let p = |s: &str| Some(PathBuf::from(s));
    assert_eq!(file("file:///home/me/My%20Notes.txt"), p("/home/me/My Notes.txt"));
    assert_eq!(file("file://localhost/tmp/%D1%84%25x%2"), p("/tmp/ф%x%2"));
    assert_eq!(file("https://example.com/a"), None);
    assert_eq!(file("file://host"), None);
    assert_eq!(file("file://other/home/me/a.txt"), None);
    let list = b"# from Files\r\nfile:///a/b.tar\r\nhttps://x.org/\r\n\r\nfile:///c%23d\r\n";
    assert_eq!(uri_list(list), vec![PathBuf::from("/a/b.tar"), PathBuf::from("/c#d")]);
  }

  #[test]
  fn portal_options() {
    let d = FileDialog { dir: Some("/tmp".into()), ..FileDialog::save("Export") };
    let d = d.types(&["tar", "7z"]).name("h.tar");
    let o = options(&d, "t1");
    assert_eq!(o.ty(), "a{sv}");
    let glob = |g: &str| V::Tuple(vec![V::U(0), V::s(g)]);
    let globs = vec![glob("*.tar"), glob("*.TAR"), glob("*.7z"), glob("*.7Z")];
    let filter = V::Tuple(vec![V::s("*.tar, *.7z"), V::Array("(us)", globs)]);
    let want = V::dict(vec![
      ("handle_token", V::s("t1")),
      ("modal", V::B(true)),
      ("filters", V::Array("(sa(us))", vec![filter])),
      ("current_folder", V::Bytes(b"/tmp\0".to_vec())),
      ("current_name", V::s("h.tar")),
    ]);
    assert_eq!(o, want);
    let open = options(&FileDialog::open("Import").name("x"), "t2");
    assert_eq!(open, V::dict(vec![("handle_token", V::s("t2")), ("modal", V::B(true))]));
  }
}
