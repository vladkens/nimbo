// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! XCB, loaded at run time ([super::super::dl]): `libxcb.so.1` for the core protocol and
//! `libxcb-shm.so.0` for MIT-SHM presents when it is there. The functions are declared by hand
//! from `<xcb/xproto.h>` and `<xcb/shm.h>`; [X] wraps the few requests the window needs. Cookies
//! are passed as `u32` (a struct of one `unsigned int`, the same in the C ABI).

use std::ffi::{c_char, c_int, c_void};
use std::os::fd::{BorrowedFd, IntoRawFd, OwnedFd};
use std::sync::OnceLock;

use crate::platform::linux::dl::{Lib, fns};

type C = *mut c_void;
type E = *mut *mut c_void;

// glibc: XCB's events and replies are malloc'd for the caller to free.
unsafe extern "C" {
  fn free(p: *mut c_void);
}

fns!(Xcb {
  fn xcb_connect(name: *const c_char, screen: *mut c_int) -> C;
  fn xcb_connection_has_error(c: C) -> c_int;
  fn xcb_disconnect(c: C);
  fn xcb_get_setup(c: C) -> *const c_void;
  fn xcb_setup_roots_iterator(setup: *const c_void) -> ScreenIter;
  fn xcb_screen_next(i: *mut ScreenIter);
  fn xcb_generate_id(c: C) -> u32;
  fn xcb_get_file_descriptor(c: C) -> c_int;
  fn xcb_flush(c: C) -> c_int;
  fn xcb_poll_for_event(c: C) -> *mut Event;
  fn xcb_poll_for_queued_event(c: C) -> *mut Event;
  fn xcb_request_check(c: C, cookie: u32) -> *mut c_void;
  fn xcb_get_maximum_request_length(c: C) -> u32;
  fn xcb_create_window(c: C, depth: u8, wid: u32, parent: u32, x: i16, y: i16, w: u16, h: u16,
    border: u16, class: u16, visual: u32, mask: u32, values: *const u32) -> u32;
  fn xcb_map_window(c: C, w: u32) -> u32;
  fn xcb_destroy_window(c: C, w: u32) -> u32;
  fn xcb_change_window_attributes(c: C, w: u32, mask: u32, values: *const u32) -> u32;
  fn xcb_change_property(c: C, mode: u8, w: u32, prop: u32, ty: u32, format: u8, len: u32,
    data: *const c_void) -> u32;
  fn xcb_get_property(c: C, delete: u8, w: u32, prop: u32, ty: u32, offset: u32, len: u32) -> u32;
  fn xcb_get_property_reply(c: C, cookie: u32, e: E) -> *mut PropertyReply;
  fn xcb_get_property_value(r: *const PropertyReply) -> *const u8;
  fn xcb_get_property_value_length(r: *const PropertyReply) -> c_int;
  fn xcb_intern_atom(c: C, only_if_exists: u8, len: u16, name: *const c_char) -> u32;
  fn xcb_intern_atom_reply(c: C, cookie: u32, e: E) -> *mut AtomReply;
  fn xcb_query_extension(c: C, len: u16, name: *const c_char) -> u32;
  fn xcb_query_extension_reply(c: C, cookie: u32, e: E) -> *mut ExtensionReply;
  fn xcb_send_event(c: C, propagate: u8, dest: u32, mask: u32, event: *const c_char) -> u32;
  fn xcb_create_gc(c: C, gc: u32, drawable: u32, mask: u32, values: *const u32) -> u32;
  fn xcb_put_image(c: C, format: u8, drawable: u32, gc: u32, w: u16, h: u16, x: i16, y: i16,
    left_pad: u8, depth: u8, len: u32, data: *const u8) -> u32;
  fn xcb_get_selection_owner(c: C, selection: u32) -> u32;
  fn xcb_get_selection_owner_reply(c: C, cookie: u32, e: E) -> *mut OwnerReply;
});

fns!(XcbShm {
  fn xcb_shm_query_version(c: C) -> u32;
  fn xcb_shm_query_version_reply(c: C, cookie: u32, e: E) -> *mut ShmVersionReply;
  fn xcb_shm_attach_fd_checked(c: C, seg: u32, fd: c_int, read_only: u8) -> u32;
  fn xcb_shm_detach(c: C, seg: u32) -> u32;
  fn xcb_shm_put_image(c: C, drawable: u32, gc: u32, total_w: u16, total_h: u16, src_x: u16,
    src_y: u16, src_w: u16, src_h: u16, dst_x: i16, dst_y: i16, depth: u8, format: u8,
    send_event: u8, seg: u32, offset: u32) -> u32;
});

struct Libs {
  xcb: Xcb,
  shm: Option<XcbShm>,
}

fn libs() -> Option<&'static Libs> {
  static LIBS: OnceLock<Option<Libs>> = OnceLock::new();
  LIBS
    .get_or_init(|| {
      let xcb = Xcb::load(&Lib::open(&[c"libxcb.so.1"])?)?;
      let shm = Lib::open(&[c"libxcb-shm.so.0"]).and_then(|l| XcbShm::load(&l));
      Some(Libs { xcb, shm })
    })
    .as_ref()
}

// MARK: Protocol

#[repr(C)]
struct ScreenIter {
  data: *mut Screen,
  rem: c_int,
  index: c_int,
}

/// `xcb_screen_t`.
#[repr(C)]
pub struct Screen {
  pub root: u32,
  pub default_colormap: u32,
  pub white_pixel: u32,
  pub black_pixel: u32,
  pub current_input_masks: u32,
  pub width_in_pixels: u16,
  pub height_in_pixels: u16,
  pub width_in_millimeters: u16,
  pub height_in_millimeters: u16,
  pub min_installed_maps: u16,
  pub max_installed_maps: u16,
  pub root_visual: u32,
  pub backing_stores: u8,
  pub save_unders: u8,
  pub root_depth: u8,
  pub allowed_depths_len: u8,
}

/// The head every event shares; XCB allocates 36 bytes (32 and the full sequence).
#[repr(C)]
pub struct Event {
  pub response_type: u8,
  pub detail: u8,
  pub sequence: u16,
  rest: [u32; 8],
}

#[repr(C)]
pub struct PropertyReply {
  response_type: u8,
  format: u8,
  sequence: u16,
  length: u32,
  ty: u32,
  bytes_after: u32,
  value_len: u32,
  pad: [u8; 12],
}

#[repr(C)]
pub struct AtomReply {
  response_type: u8,
  pad: u8,
  sequence: u16,
  length: u32,
  atom: u32,
}

#[repr(C)]
pub struct OwnerReply {
  response_type: u8,
  pad: u8,
  sequence: u16,
  length: u32,
  owner: u32,
}

#[repr(C)]
pub struct ExtensionReply {
  response_type: u8,
  pad: u8,
  sequence: u16,
  length: u32,
  present: u8,
  major_opcode: u8,
  first_event: u8,
  first_error: u8,
}

#[repr(C)]
pub struct ShmVersionReply {
  response_type: u8,
  shared_pixmaps: u8,
  sequence: u16,
  length: u32,
  major: u16,
  minor: u16,
  uid: u16,
  gid: u16,
  pixmap_format: u8,
  pad: [u8; 15],
}

// Events, `<xcb/xproto.h>`: the response types and their layouts.
pub const FOCUS_IN: u8 = 9;
pub const FOCUS_OUT: u8 = 10;
pub const EXPOSE: u8 = 12;
pub const DESTROY_NOTIFY: u8 = 17;
pub const UNMAP_NOTIFY: u8 = 18;
pub const MAP_NOTIFY: u8 = 19;
pub const CONFIGURE_NOTIFY: u8 = 22;
pub const PROPERTY_NOTIFY: u8 = 28;
pub const CLIENT_MESSAGE: u8 = 33;

#[repr(C)]
pub struct FocusEv {
  pub response_type: u8,
  pub detail: u8,
  pub sequence: u16,
  pub event: u32,
  pub mode: u8,
  pad: [u8; 3],
}

#[repr(C)]
pub struct ExposeEv {
  pub response_type: u8,
  pad: u8,
  pub sequence: u16,
  pub window: u32,
  pub x: u16,
  pub y: u16,
  pub width: u16,
  pub height: u16,
  pub count: u16,
  pad1: [u8; 2],
}

#[repr(C)]
pub struct ConfigureEv {
  pub response_type: u8,
  pad: u8,
  pub sequence: u16,
  pub event: u32,
  pub window: u32,
  pub above_sibling: u32,
  pub x: i16,
  pub y: i16,
  pub width: u16,
  pub height: u16,
  pub border_width: u16,
  pub override_redirect: u8,
  pad1: u8,
}

#[repr(C)]
pub struct ClientMessageEv {
  pub response_type: u8,
  pub format: u8,
  pub sequence: u16,
  pub window: u32,
  pub ty: u32,
  pub data: [u32; 5],
}

#[repr(C)]
pub struct PropertyEv {
  pub response_type: u8,
  pad: u8,
  pub sequence: u16,
  pub window: u32,
  pub atom: u32,
  pub time: u32,
  pub state: u8,
  pad1: [u8; 3],
}

/// MIT-SHM's `ShmCompletion`, at the extension's first event.
#[repr(C)]
pub struct ShmCompletionEv {
  pub response_type: u8,
  pad: u8,
  pub sequence: u16,
  pub drawable: u32,
  pub minor_event: u16,
  pub major_event: u8,
  pad1: u8,
  pub shmseg: u32,
  pub offset: u32,
}

// Event masks, window attributes and the rest of the constants used.
pub const EM_EXPOSURE: u32 = 32768;
pub const EM_STRUCTURE_NOTIFY: u32 = 131072;
pub const EM_SUBSTRUCTURE_NOTIFY: u32 = 524288;
pub const EM_SUBSTRUCTURE_REDIRECT: u32 = 1048576;
pub const EM_FOCUS_CHANGE: u32 = 2097152;
pub const EM_PROPERTY_CHANGE: u32 = 4194304;

const CW_BACK_PIXMAP: u32 = 1;
const CW_BIT_GRAVITY: u32 = 16;
const CW_EVENT_MASK: u32 = 2048;
const GRAVITY_NORTH_WEST: u32 = 1;
const WINDOW_CLASS_INPUT_OUTPUT: u16 = 1;
const PROP_MODE_REPLACE: u8 = 0;
const IMAGE_FORMAT_Z_PIXMAP: u8 = 2;
const GC_GRAPHICS_EXPOSURES: u32 = 65536;

/// Predefined atoms.
pub const ATOM_ATOM: u32 = 4;
pub const ATOM_CARDINAL: u32 = 6;
pub const ATOM_RESOURCE_MANAGER: u32 = 23;
pub const ATOM_STRING: u32 = 31;
pub const ATOM_WM_NAME: u32 = 39;
pub const ATOM_WM_NORMAL_HINTS: u32 = 40;
pub const ATOM_WM_SIZE_HINTS: u32 = 41;
pub const ATOM_WM_CLASS: u32 = 67;

// MARK: Connection

/// An event or error (`response_type` 0) from the server; freed on drop.
pub struct Ev(*mut Event);

impl Ev {
  /// The response type without the bit that marks a sent event.
  pub fn kind(&self) -> u8 {
    unsafe { (*self.0).response_type & 0x7f }
  }

  /// The event's 32 bytes.
  pub fn bytes(&self) -> [u8; 32] {
    // SAFETY: XCB allocates at least 32 bytes per event.
    unsafe { *self.0.cast::<[u8; 32]>() }
  }

  /// The event as its layout `T` (at most the 32 bytes every event has).
  pub fn get<T>(&self) -> &T {
    const { assert!(size_of::<T>() <= 32) };
    // SAFETY: XCB allocates at least 32 bytes per event; `T` is a `repr(C)` event layout.
    unsafe { &*self.0.cast::<T>() }
  }
}

impl Drop for Ev {
  fn drop(&mut self) {
    unsafe { free(self.0.cast()) };
  }
}

/// A property's type and bytes.
pub struct Prop {
  pub ty: u32,
  pub data: Vec<u8>,
}

/// A connection: its handle and the libraries. Copy: closed once with [X::disconnect].
#[derive(Clone, Copy)]
pub struct X {
  c: C,
  l: &'static Libs,
}

/// Takes a reply and frees it.
fn reply<R, T>(p: *mut R, f: impl FnOnce(&R) -> T) -> Option<T> {
  if p.is_null() {
    return None;
  }

  let out = f(unsafe { &*p });
  unsafe { free(p.cast()) };
  Some(out)
}

impl X {
  /// Connects to `$DISPLAY`; returns its default screen and that screen's number.
  pub fn connect() -> Result<(Self, *mut Screen, i32), String> {
    let l = libs().ok_or("libxcb.so.1 not found")?;
    let mut n: c_int = 0;
    let c = unsafe { (l.xcb.xcb_connect)(std::ptr::null(), &mut n) };
    if c.is_null() || unsafe { (l.xcb.xcb_connection_has_error)(c) } != 0 {
      let d = std::env::var("DISPLAY").unwrap_or_default();
      return Err(format!("can't connect to the X server (DISPLAY={d:?})"));
    }

    let x = Self { c, l };
    let mut it = unsafe { (l.xcb.xcb_setup_roots_iterator)((l.xcb.xcb_get_setup)(c)) };
    for _ in 0..n {
      unsafe { (l.xcb.xcb_screen_next)(&mut it) };
    }

    if it.data.is_null() {
      x.disconnect();
      return Err(format!("no X screen {n}"));
    }

    Ok((x, it.data, n))
  }

  pub fn disconnect(self) {
    unsafe { (self.l.xcb.xcb_disconnect)(self.c) };
  }

  pub fn has_error(&self) -> bool {
    unsafe { (self.l.xcb.xcb_connection_has_error)(self.c) != 0 }
  }

  pub fn flush(&self) {
    unsafe { (self.l.xcb.xcb_flush)(self.c) };
  }

  pub fn fd(&self) -> BorrowedFd<'_> {
    // SAFETY: the connection's socket, open until `disconnect`.
    unsafe { BorrowedFd::borrow_raw((self.l.xcb.xcb_get_file_descriptor)(self.c)) }
  }

  pub fn id(&self) -> u32 {
    unsafe { (self.l.xcb.xcb_generate_id)(self.c) }
  }

  /// The next event, read from the socket if none is queued.
  pub fn poll_event(&self) -> Option<Ev> {
    let p = unsafe { (self.l.xcb.xcb_poll_for_event)(self.c) };
    (!p.is_null()).then(|| Ev(p))
  }

  /// The next event XCB already read (while waiting for a reply), without reading.
  pub fn queued_event(&self) -> Option<Ev> {
    let p = unsafe { (self.l.xcb.xcb_poll_for_queued_event)(self.c) };
    (!p.is_null()).then(|| Ev(p))
  }

  /// The atoms of `names`, interned in one round trip.
  pub fn atoms<const N: usize>(&self, names: [&str; N]) -> [u32; N] {
    let f = &self.l.xcb;
    let cookies =
      names.map(|n| unsafe { (f.xcb_intern_atom)(self.c, 0, n.len() as u16, n.as_ptr().cast()) });
    cookies.map(|k| {
      let r = unsafe { (f.xcb_intern_atom_reply)(self.c, k, std::ptr::null_mut()) };
      reply(r, |r| r.atom).unwrap_or(0)
    })
  }

  /// A window's property (any type when `ty` is 0), whole; deleted after it is read when
  /// `delete`.
  pub fn property(&self, win: u32, prop: u32, ty: u32, delete: bool) -> Option<Prop> {
    let f = &self.l.xcb;
    let k = unsafe { (f.xcb_get_property)(self.c, delete as u8, win, prop, ty, 0, u32::MAX / 4) };
    let r = unsafe { (f.xcb_get_property_reply)(self.c, k, std::ptr::null_mut()) };
    reply(r, |r| {
      let n = unsafe { (f.xcb_get_property_value_length)(r) }.max(0) as usize;
      let p = unsafe { (f.xcb_get_property_value)(r) };
      let data = match n {
        0 => Vec::new(),
        n => unsafe { std::slice::from_raw_parts(p, n) }.to_vec(),
      };

      Prop { ty: r.ty, data }
    })
    .filter(|p| p.ty != 0)
  }

  /// Replaces a property with `data`, `format` bits per item.
  pub fn set_property(&self, win: u32, prop: u32, ty: u32, format: u8, data: &[u8]) {
    let n = data.len() / (format as usize / 8);
    unsafe {
      (self.l.xcb.xcb_change_property)(
        self.c,
        PROP_MODE_REPLACE,
        win,
        prop,
        ty,
        format,
        n as u32,
        data.as_ptr().cast(),
      )
    };
  }

  pub fn set_property32(&self, win: u32, prop: u32, ty: u32, data: &[u32]) {
    let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_ne_bytes()).collect();
    self.set_property(win, prop, ty, 32, &bytes);
  }

  /// The largest request in bytes (BIG-REQUESTS when the server has it).
  pub fn max_request(&self) -> usize {
    unsafe { (self.l.xcb.xcb_get_maximum_request_length)(self.c) as usize * 4 }
  }

  /// An extension's first event code, if the server has it.
  pub fn extension_event(&self, name: &str) -> Option<u8> {
    let f = &self.l.xcb;
    let k = unsafe { (f.xcb_query_extension)(self.c, name.len() as u16, name.as_ptr().cast()) };
    let r = unsafe { (f.xcb_query_extension_reply)(self.c, k, std::ptr::null_mut()) };
    reply(r, |r| (r.present != 0).then_some(r.first_event)).flatten()
  }

  /// A 32-byte event sent to `dest`.
  pub fn send_event(&self, dest: u32, mask: u32, ev: &[u8; 32]) {
    unsafe { (self.l.xcb.xcb_send_event)(self.c, 0, dest, mask, ev.as_ptr().cast()) };
  }

  // MARK: Windows

  /// A top-level window of the screen's visual, with no background (nothing is cleared before
  /// our frame) and its content kept at the top left as it resizes.
  pub fn create_window(&self, screen: &Screen, w: u16, h: u16, events: u32) -> u32 {
    let id = self.id();
    let values = [0, GRAVITY_NORTH_WEST, events];
    let mask = CW_BACK_PIXMAP | CW_BIT_GRAVITY | CW_EVENT_MASK;
    unsafe {
      (self.l.xcb.xcb_create_window)(
        self.c,
        0,
        id,
        screen.root,
        0,
        0,
        w,
        h,
        0,
        WINDOW_CLASS_INPUT_OUTPUT,
        screen.root_visual,
        mask,
        values.as_ptr(),
      )
    };
    id
  }

  pub fn map_window(&self, w: u32) {
    unsafe { (self.l.xcb.xcb_map_window)(self.c, w) };
  }

  pub fn destroy_window(&self, w: u32) {
    unsafe { (self.l.xcb.xcb_destroy_window)(self.c, w) };
  }

  /// Selects `events` on a window that isn't ours (the root, the XSETTINGS owner).
  pub fn select_events(&self, w: u32, events: u32) {
    unsafe { (self.l.xcb.xcb_change_window_attributes)(self.c, w, CW_EVENT_MASK, &events) };
  }

  /// A graphics context for images, no GraphicsExpose events.
  pub fn create_gc(&self, drawable: u32) -> u32 {
    let id = self.id();
    let values = [0u32];
    let f = &self.l.xcb;
    unsafe { (f.xcb_create_gc)(self.c, id, drawable, GC_GRAPHICS_EXPOSURES, values.as_ptr()) };
    id
  }

  /// `w` × `h` pixels at (`x`, `y`), rows packed, through the socket.
  #[allow(clippy::too_many_arguments)]
  pub fn put_image(&self, win: u32, gc: u32, x: i16, y: i16, w: u16, h: u16, depth: u8, px: &[u8]) {
    unsafe {
      (self.l.xcb.xcb_put_image)(
        self.c,
        IMAGE_FORMAT_Z_PIXMAP,
        win,
        gc,
        w,
        h,
        x,
        y,
        0,
        depth,
        px.len() as u32,
        px.as_ptr(),
      )
    };
  }

  // MARK: Selections

  pub fn selection_owner(&self, selection: u32) -> u32 {
    let f = &self.l.xcb;
    let k = unsafe { (f.xcb_get_selection_owner)(self.c, selection) };
    let r = unsafe { (f.xcb_get_selection_owner_reply)(self.c, k, std::ptr::null_mut()) };
    reply(r, |r| r.owner).unwrap_or(0)
  }

  // MARK: MIT-SHM

  /// MIT-SHM 1.2 or later (segments passed as file descriptors) and its completion event code.
  pub fn shm(&self) -> Option<u8> {
    let f = self.l.shm.as_ref()?;
    let first = self.extension_event("MIT-SHM")?;
    let k = unsafe { (f.xcb_shm_query_version)(self.c) };
    let r = unsafe { (f.xcb_shm_query_version_reply)(self.c, k, std::ptr::null_mut()) };
    let v = reply(r, |r| (r.major, r.minor))?;
    (v >= (1, 2)).then_some(first)
  }

  /// Attaches a segment (the server maps `fd`); false when the server refuses it (a remote
  /// display).
  pub fn shm_attach(&self, seg: u32, fd: OwnedFd) -> bool {
    let Some(f) = &self.l.shm else { return false };
    let k = unsafe { (f.xcb_shm_attach_fd_checked)(self.c, seg, fd.into_raw_fd(), 0) };
    let e = unsafe { (self.l.xcb.xcb_request_check)(self.c, k) };
    if e.is_null() {
      return true;
    }

    unsafe { free(e) };
    false
  }

  pub fn shm_detach(&self, seg: u32) {
    if let Some(f) = &self.l.shm {
      unsafe { (f.xcb_shm_detach)(self.c, seg) };
    }
  }

  /// A rect of a `total` image in a segment to the same place in the window; `notify` asks for
  /// a completion event once the server has read it.
  #[allow(clippy::too_many_arguments)]
  pub fn shm_put_image(
    &self,
    win: u32,
    gc: u32,
    total: (u16, u16),
    rect: [u16; 4],
    depth: u8,
    notify: bool,
    seg: u32,
  ) {
    let Some(f) = &self.l.shm else { return };
    let [x, y, w, h] = rect;
    unsafe {
      (f.xcb_shm_put_image)(
        self.c,
        win,
        gc,
        total.0,
        total.1,
        x,
        y,
        w,
        h,
        x as i16,
        y as i16,
        depth,
        IMAGE_FORMAT_Z_PIXMAP,
        notify as u8,
        seg,
        0,
      )
    };
  }
}
