// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Where the user types on Wayland compositors that tell their windows to a client
//! (`zwlr_foreign_toplevel_manager_v1`, <https://wayland.app/protocols/wlr-foreign-toplevel-management-unstable-v1>):
//! wlroots' (Sway) and Hyprland. The activated window is the focus: its `app_id` as the app's name
//! (a terminal's mapped to its process name, which [crate::paste] knows terminals by), and a number
//! of the app's own per window as its identity, so a moved focus is caught; no pid, which the
//! protocol doesn't tell. Started on the main thread at the first look there, on the app's own
//! Wayland client ([super::wl]); its events change a state any thread reads, and a look from
//! another thread before the start asks the main thread to start it and waits for it a while.

use std::ffi::{CStr, c_char, c_int};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use super::super::{Fp, P, gtk};
use super::wl::{self, Interface};
use crate::focus::{self, Focused};

/// How long a look from another thread waits for the main thread to start the tracking.
const START: Duration = Duration::from_millis(500);
/// `zwlr_foreign_toplevel_handle_v1.state`'s activated, and the handle's `destroy`.
const ACTIVATED: u32 = 2;
const DESTROY: u32 = 7;
/// Terminals whose `app_id` isn't their process's name.
const NAMES: [(&str, &str); 4] = [
  ("org.gnome.Terminal", "gnome-terminal-"),
  ("org.gnome.Console", "kgx"),
  ("org.wezfurlong.wezterm", "wezterm-gui"),
  ("com.mitchellh.ghostty", "ghostty"),
];

/// A window, as its events told it: those since its last `done` are pending.
#[derive(Clone, Default)]
struct Toplevel {
  handle: usize,
  id: u64,
  app_id: String,
  activated: bool,
  pending: (Option<String>, Option<bool>),
}

#[derive(Default)]
struct State {
  windows: Vec<Toplevel>,
  next: u64,
}

/// Not asked yet, asked of the main thread, running, or none (no compositor's manager).
const NEW: u8 = 0;
const ASKED: u8 = 1;
const ON: u8 = 2;
const OFF: u8 = 3;
static PHASE: AtomicU8 = AtomicU8::new(NEW);
static STATE: Mutex<State> = Mutex::new(State { windows: Vec::new(), next: 1 });
static STARTED: Condvar = Condvar::new();

struct Protocol {
  manager: *const Interface,
}

// SAFETY: leaked, and only read.
unsafe impl Send for Protocol {}
unsafe impl Sync for Protocol {}

fn protocol() -> &'static Protocol {
  static PROTOCOL: OnceLock<Protocol> = OnceLock::new();
  PROTOCOL.get_or_init(|| {
    let none: &[*const Interface] = &[null(); 5];
    let handle = Interface::new(
      c"zwlr_foreign_toplevel_handle_v1",
      &[
        (c"set_maximized", c"", none),
        (c"unset_maximized", c"", none),
        (c"set_minimized", c"", none),
        (c"unset_minimized", c"", none),
        (c"activate", c"o", none),
        (c"close", c"", none),
        (c"set_rectangle", c"oiiii", none),
        (c"destroy", c"", none),
      ],
      &[
        (c"title", c"s", none),
        (c"app_id", c"s", none),
        (c"output_enter", c"o", none),
        (c"output_leave", c"o", none),
        (c"state", c"a", none),
        (c"done", c"", none),
        (c"closed", c"", none),
      ],
    );
    let manager = Interface::new(
      c"zwlr_foreign_toplevel_manager_v1",
      &[(c"stop", c"", none)],
      &[(c"toplevel", c"n", &[handle]), (c"finished", c"", none)],
    );
    Protocol { manager }
  })
}

/// The compositor tells its windows, so the focus is known ([current]); on the main thread it
/// starts the tracking, from another it says whether it runs.
pub fn known() -> bool {
  if wl::main() {
    start();
  }

  PHASE.load(Ordering::Acquire) == ON
}

/// The activated window now; `None` when no window has the focus, or the compositor doesn't
/// tell ([known]).
pub fn current() -> Option<Focused> {
  if !wl::main() && PHASE.load(Ordering::Acquire) != ON {
    wait();
  }

  if !known() {
    return None;
  }

  let state = STATE.lock().ok()?;
  let w = state.windows.iter().find(|w| w.activated)?;
  Some(Focused {
    pid: 0,
    name: name(&w.app_id),
    window: Ok(Some(focus::Window(super::focus::Window(w.id as _)))),
    secure: Ok(false),
  })
}

/// The process name [crate::paste] knows a terminal by, else the `app_id` as it is.
fn name(app_id: &str) -> String {
  let mapped = NAMES.iter().find(|(id, _)| *id == app_id).map(|(_, name)| name.to_string());
  mapped.unwrap_or_else(|| app_id.to_lowercase())
}

/// From another thread: asks the main thread to start, and waits for it at most [START].
fn wait() {
  if PHASE.compare_exchange(NEW, ASKED, Ordering::AcqRel, Ordering::Acquire).is_ok() {
    unsafe { (gtk().g_idle_add)(on_start as Fp, null_mut()) };
  }

  let Ok(state) = STATE.lock() else { return };
  _ = STARTED.wait_timeout_while(state, START, |_| PHASE.load(Ordering::Acquire) < ON);
}

/// `GSourceFunc` of the idle [wait] adds: [start], once.
unsafe extern "C" fn on_start(_: P) -> c_int {
  start();
  0
}

/// Binds the manager once, on the main thread: the windows come at the roundtrip.
fn start() {
  if PHASE.load(Ordering::Acquire) >= ON {
    return;
  }

  let manager = wl::client()
    .and_then(|c| Some((c, c.bind(c"zwlr_foreign_toplevel_manager_v1", protocol().manager)?)));
  let phase = match manager {
    Some((client, manager)) => {
      wl::listen(manager, &[toplevel as Fp, finished as Fp]);
      client.roundtrip();
      ON
    }
    None => OFF,
  };

  PHASE.store(phase, Ordering::Release);
  STARTED.notify_all();
}

fn with(handle: P, f: impl FnOnce(&mut Toplevel)) {
  if let Ok(mut s) = STATE.lock()
    && let Some(w) = s.windows.iter_mut().find(|w| w.handle == handle as usize)
  {
    f(w);
  }
}

unsafe extern "C" fn toplevel(_: P, _: P, handle: P) {
  wl::listen(
    handle,
    &[title as Fp, app_id as Fp, output as Fp, output as Fp, state as Fp, done as Fp, closed as Fp],
  );
  if let Ok(mut s) = STATE.lock() {
    let id = s.next;
    s.next += 1;
    s.windows.push(Toplevel { handle: handle as usize, id, ..Toplevel::default() });
  }
}

/// The compositor stopped telling: the focus is unknown again.
unsafe extern "C" fn finished(_: P, _: P) {
  PHASE.store(OFF, Ordering::Release);
}

unsafe extern "C" fn title(_: P, _: P, _: *const c_char) {}

unsafe extern "C" fn app_id(_: P, handle: P, id: *const c_char) {
  let id = unsafe { CStr::from_ptr(id) }.to_string_lossy().into_owned();
  with(handle, |w| w.pending.0 = Some(id));
}

unsafe extern "C" fn output(_: P, _: P, _: P) {}

/// `wl_array`.
#[repr(C)]
struct Array {
  size: usize,
  alloc: usize,
  data: *const u32,
}

unsafe extern "C" fn state(_: P, handle: P, states: *const Array) {
  // SAFETY: libwayland's array of the event, `size` bytes of u32 states.
  let all = unsafe { std::slice::from_raw_parts((*states).data, (*states).size / 4) };
  let activated = all.contains(&ACTIVATED);
  with(handle, |w| w.pending.1 = Some(activated));
}

unsafe extern "C" fn done(_: P, handle: P) {
  with(handle, |w| {
    let (app_id, activated) = std::mem::take(&mut w.pending);
    w.app_id = app_id.unwrap_or(std::mem::take(&mut w.app_id));
    w.activated = activated.unwrap_or(w.activated);
  });
}

unsafe extern "C" fn closed(_: P, handle: P) {
  if let Ok(mut s) = STATE.lock() {
    s.windows.retain(|w| w.handle != handle as usize);
  }

  wl::destroy(handle, DESTROY);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn names_of_terminals() {
    assert_eq!(name("com.mitchellh.ghostty"), "ghostty");
    assert_eq!(name("Alacritty"), "alacritty");
    assert_eq!(name("org.wezfurlong.wezterm"), "wezterm-gui");
    assert_eq!(name("kitty"), "kitty");
  }
}
