// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The clipboard for a paste on Wayland through data control (`ext_data_control_manager_v1`, else
//! wlroots' `zwlr_data_control_manager_v1`, <https://wayland.app/protocols/ext-data-control-v1>),
//! the clipboard managers' protocol, which wlroots' compositors (Sway) and Hyprland have: a client
//! sets and reads the selection there without the keyboard focus, which GTK's own clipboard can't
//! and XWayland's doesn't reach on them. On the app's own Wayland client ([super::wl]), for
//! [super::paste] where the portal has no clipboard: an offer is a source of the types, each
//! `send` answered with the bytes [super::paste::read] gives; a snapshot reads every type of the
//! selection's offer through a pipe, all within [READ] (the owner is another app: one of the
//! app's own is what it serves); the source's `cancelled` is another owner's taking it.

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char, c_int, c_short};
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use super::super::{Fp, P};
use super::paste::{self, META};
use super::wl::{self, Arg, Client, Interface};
use crate::clipboard::Snapshot;

// glibc's, <poll.h>.
unsafe extern "C" {
  fn poll(fds: *mut PollFd, n: u64, timeout: c_int) -> c_int;
}

/// `struct pollfd`.
#[repr(C)]
struct PollFd {
  fd: c_int,
  events: c_short,
  revents: c_short,
}

const POLLIN: c_short = 1;
/// How long the selection's owner may take to give its types.
const READ: Duration = Duration::from_millis(500);
/// The manager's, the device's, the source's and the offer's requests.
const CREATE_SOURCE: u32 = 0;
const GET_DEVICE: u32 = 1;
const SET_SELECTION: u32 = 0;
const OFFER: u32 = 0;
const DESTROY_SOURCE: u32 = 1;
const RECEIVE: u32 = 0;
const DESTROY_OFFER: u32 = 1;

/// One protocol's interfaces: ext's and wlroots' have the same messages under their own names.
struct Protocol {
  manager_name: &'static CStr,
  manager: *const Interface,
  device: *const Interface,
  source: *const Interface,
}

// SAFETY: leaked, and only read.
unsafe impl Send for Protocol {}
unsafe impl Sync for Protocol {}

/// The interface names of ext's protocol, then of wlroots'.
const NAMES: [[&CStr; 4]; 2] = [
  [
    c"ext_data_control_manager_v1",
    c"ext_data_control_device_v1",
    c"ext_data_control_source_v1",
    c"ext_data_control_offer_v1",
  ],
  [
    c"zwlr_data_control_manager_v1",
    c"zwlr_data_control_device_v1",
    c"zwlr_data_control_source_v1",
    c"zwlr_data_control_offer_v1",
  ],
];

fn protocols() -> Option<&'static [Protocol; 2]> {
  static ALL: OnceLock<Option<[Protocol; 2]>> = OnceLock::new();
  (ALL.get_or_init(|| {
    let seat = wl::lw()?.seat;
    Some(NAMES.map(|[manager_name, device, source, offer]| {
      let none: &[*const Interface] = &[null(); 2];
      let offer = Interface::new(
        offer,
        &[(c"receive", c"sh", none), (c"destroy", c"", none)],
        &[(c"offer", c"s", none)],
      );
      let source = Interface::new(
        source,
        &[(c"offer", c"s", none), (c"destroy", c"", none)],
        &[(c"send", c"sh", none), (c"cancelled", c"", none)],
      );
      let device = Interface::new(
        device,
        &[
          (c"set_selection", c"?o", &[source]),
          (c"destroy", c"", none),
          (c"set_primary_selection", c"?o", &[source]),
        ],
        &[
          (c"data_offer", c"n", &[offer]),
          (c"selection", c"?o", &[offer]),
          (c"finished", c"", none),
          (c"primary_selection", c"?o", &[offer]),
        ],
      );
      let manager = Interface::new(
        manager_name,
        &[
          (c"create_data_source", c"n", &[source]),
          (c"get_data_device", c"no", &[device, seat]),
          (c"destroy", c"", none),
        ],
        &[],
      );
      Protocol { manager_name, manager, device, source }
    }))
  }))
  .as_ref()
}

/// The app's data device: its client, protocol, manager and device.
struct Device {
  client: &'static Client,
  protocol: &'static Protocol,
  manager: P,
  device: P,
}

#[derive(Default)]
struct State {
  /// The offers the device announced, with their types; the selection's, when it has one.
  offers: Vec<(usize, Vec<String>)>,
  selection: Option<usize>,
  /// The source the app set, while no other owner took the selection.
  source: Option<usize>,
}

thread_local! {
  static DEVICE: RefCell<Option<Option<Device>>> = const { RefCell::new(None) };
  static STATE: RefCell<State> = RefCell::default();
}

/// The compositor has data control: [super::paste] serves and reads the clipboard through it.
pub(super) fn available() -> bool {
  with(|_| ()).is_some()
}

fn with<R>(f: impl FnOnce(&Device) -> R) -> Option<R> {
  let client = wl::client()?;
  DEVICE.with_borrow_mut(|d| d.get_or_insert_with(|| make(client)).as_ref().map(f))
}

fn make(client: &'static Client) -> Option<Device> {
  let (protocol, manager) =
    protocols()?.iter().find_map(|p| Some((p, client.bind(p.manager_name, p.manager)?)))?;
  let seat = client.bind(c"wl_seat", wl::lw()?.seat)?;
  let mut args = [Arg { o: null_mut() }, Arg { o: seat }];
  let device = wl::marshal(manager, GET_DEVICE, protocol.device, &mut args);
  wl::listen(device, &[data_offer as Fp, selection as Fp, finished as Fp, primary as Fp]);
  // The selection as it is comes now.
  client.roundtrip().then_some(Device { client, protocol, manager, device })
}

unsafe extern "C" fn data_offer(_: P, _: P, offer: P) {
  wl::listen(offer, &[offered as Fp]);
  STATE.with_borrow_mut(|s| s.offers.push((offer as usize, Vec::new())));
}

unsafe extern "C" fn offered(_: P, offer: P, kind: *const c_char) {
  let kind = unsafe { CStr::from_ptr(kind) }.to_string_lossy().into_owned();
  STATE.with_borrow_mut(|s| {
    if let Some((_, types)) = s.offers.iter_mut().find(|(o, _)| *o == offer as usize) {
      types.push(kind);
    }
  });
}

/// The selection's offer now (none: empty): the ones before it go.
unsafe extern "C" fn selection(_: P, _: P, offer: P) {
  let current = (!offer.is_null()).then_some(offer as usize);
  let gone = STATE.with_borrow_mut(|s| {
    s.selection = current;
    let (keep, gone): (Vec<_>, Vec<_>) = s.offers.drain(..).partition(|(o, _)| Some(*o) == current);
    s.offers = keep;
    gone
  });
  for (o, _) in gone {
    wl::destroy(o as P, DESTROY_OFFER);
  }
}

/// The primary selection's offer: not the clipboard's.
unsafe extern "C" fn primary(_: P, _: P, offer: P) {
  let ours = STATE.with_borrow(|s| s.selection == Some(offer as usize));
  if !offer.is_null() && !ours {
    STATE.with_borrow_mut(|s| s.offers.retain(|(o, _)| *o != offer as usize));
    wl::destroy(offer, DESTROY_OFFER);
  }
}

/// The device is no longer valid (the seat went): the next use makes none.
unsafe extern "C" fn finished(_: P, _: P) {
  DEVICE.set(Some(None));
}

/// A reader wants type `kind` of the app's source, written to `fd` on a thread: the reader may be
/// the app itself, reading on this thread once the pipe has more than it holds.
unsafe extern "C" fn send(_: P, _: P, kind: *const c_char, fd: c_int) {
  // SAFETY: the compositor's descriptor, closed once written.
  let mut file = unsafe { File::from_raw_fd(fd) };
  let kind = unsafe { CStr::from_ptr(kind) }.to_string_lossy();
  if let Some(bytes) = paste::read(None, |_, t| t == kind) {
    std::thread::spawn(move || _ = file.write_all(&bytes));
  }
}

/// Another owner took the selection: what the app served goes.
unsafe extern "C" fn cancelled(_: P, source: P) {
  let ours = STATE.with_borrow_mut(|s| s.source.take_if(|&mut o| o == source as usize).is_some());
  wl::destroy(source, DESTROY_SOURCE);
  if ours {
    paste::lost();
  }
}

/// Takes the selection with `types`, their bytes from [paste::read] at each read.
pub(super) fn offer(types: &[String]) -> bool {
  with(|d| {
    let mut args = [Arg { o: null_mut() }];
    let source = wl::marshal(d.manager, CREATE_SOURCE, d.protocol.source, &mut args);
    wl::listen(source, &[send as Fp, cancelled as Fp]);
    for t in types {
      let t = CString::new(t.as_str()).unwrap_or_default();
      wl::marshal(source, OFFER, null(), &mut [Arg { s: t.as_ptr() }]);
    }

    set(d, source);
    d.client.roundtrip()
  })
  .unwrap_or(false)
}

/// The selection set to `source` (null: emptied), the app's source before it gone.
fn set(d: &Device, source: P) {
  let new = (!source.is_null()).then_some(source as usize);
  let old = STATE.with_borrow_mut(|s| std::mem::replace(&mut s.source, new));
  wl::marshal(d.device, SET_SELECTION, null(), &mut [Arg { o: source }]);
  if let Some(old) = old.filter(|&o| Some(o) != new) {
    wl::destroy(old as P, DESTROY_SOURCE);
  }
}

/// Empties the clipboard: what the app owns goes.
pub(super) fn clear() -> bool {
  with(|d| {
    set(d, null_mut());
    d.client.roundtrip()
  })
  .unwrap_or(false)
}

/// The clipboard's contents now: the app's own as it serves them, else each type of the
/// selection's offer as its owner gives it within [READ] (one it didn't give counts as missing).
pub(super) fn snapshot() -> Snapshot {
  let mut snapshot = Snapshot::default();
  let Some(client) = with(|d| d.client) else { return snapshot };
  client.roundtrip();
  if STATE.with_borrow(|s| s.source.is_some()) {
    snapshot.items.extend(paste::served());
    return snapshot;
  }

  let offer = STATE.with_borrow(|s| {
    let o = s.selection?;
    s.offers.iter().find(|(p, _)| *p == o).map(|(p, t)| (*p, t.clone()))
  });
  let Some((offer, kinds)) = offer else { return snapshot };
  // One wait for them all: an owner that doesn't answer one type answers none.
  let (mut types, deadline) = (Vec::new(), Instant::now() + READ);
  for kind in kinds.into_iter().filter(|k| !META.contains(&k.as_str())) {
    match receive(client, offer as P, &kind, deadline) {
      Some(bytes) => types.push((kind, bytes)),
      None => snapshot.missing += 1,
    }
  }

  if !types.is_empty() {
    snapshot.items.push(types);
  }

  snapshot
}

/// Type `kind` of `offer`, read until its owner closes the pipe, or none at `deadline`.
fn receive(client: &Client, offer: P, kind: &str, deadline: Instant) -> Option<Vec<u8>> {
  let (mut reader, writer) = std::io::pipe().ok()?;
  let name = CString::new(kind).ok()?;
  wl::marshal(
    offer,
    RECEIVE,
    null(),
    &mut [Arg { s: name.as_ptr() }, Arg { i: writer.as_raw_fd() }],
  );
  client.flush();
  // The owner has its copy: the pipe ends when it closes that.
  drop(writer);
  let (mut bytes, mut chunk) = (Vec::new(), [0u8; 64 * 1024]);
  loop {
    let left = deadline.saturating_duration_since(Instant::now()).as_millis() as c_int;
    let mut fd = PollFd { fd: reader.as_raw_fd(), events: POLLIN, revents: 0 };
    if left == 0 || unsafe { poll(&mut fd, 1, left) } <= 0 {
      return None;
    }

    match reader.read(&mut chunk) {
      Ok(0) => return Some(bytes),
      Ok(n) => bytes.extend_from_slice(&chunk[..n]),
      Err(e) if e.kind() == ErrorKind::Interrupted => {}
      Err(_) => return None,
    }
  }
}
