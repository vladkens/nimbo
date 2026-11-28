// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Shortcuts pressed through the compositor's virtual keyboard on Wayland
//! (`zwp_virtual_keyboard_manager_v1`, <https://wayland.app/protocols/virtual-keyboard-unstable-v1>),
//! which wlroots' compositors (Sway) and Hyprland have while their portals have no RemoteDesktop:
//! there [super::post] types through it, with no question to the user. One keyboard for the app's
//! life, on the app's own Wayland client ([super::wl]). Its keymap is its own, as `wtype`'s: one
//! key with the shortcut's keysym, so the key types that symbol whatever layout the user has on
//! (Ctrl+V on a Russian layout), uploaded again only for another symbol; the app in focus takes it
//! while the keys come, and the user's own keymap back at their next key. A shortcut is its
//! modifiers as a mask, the key pressed and released, the modifiers let go.

use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_int};
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd};
use std::ptr::null;
use std::sync::OnceLock;
use std::time::Instant;

use keyboard_types::Modifiers;

use super::super::P;
use super::post;
use super::wl::{self, Arg, Client, Interface};
use crate::keys::Shortcut;

// glibc's (2.27+), <sys/mman.h>.
unsafe extern "C" {
  fn memfd_create(name: *const c_char, flags: u32) -> c_int;
}

const MFD_CLOEXEC: u32 = 1;
/// The keyboard's requests, and `wl_keyboard.keymap_format.xkb_v1`.
const KEYMAP: u32 = 0;
const KEY: u32 = 1;
const MODIFIERS: u32 = 2;
const XKB_V1: u32 = 1;
/// The evdev code of the keymap's one key, xkb's keycode 9.
const CODE: u32 = 1;
/// The real modifiers [keymap] maps the modifier keys to (Shift, Control, Mod1, Mod4), their bits
/// of xkbcommon's mask.
const MASKS: [(Modifiers, u32); 4] = [
  (Modifiers::SHIFT, 1),
  (Modifiers::CONTROL, 1 << 2),
  (Modifiers::ALT, 1 << 3),
  (Modifiers::META, 1 << 6),
];

/// The manager's and the keyboard's interfaces.
struct Protocol {
  manager: *const Interface,
  keyboard: *const Interface,
}

// SAFETY: leaked, and only read.
unsafe impl Send for Protocol {}
unsafe impl Sync for Protocol {}

fn protocol() -> Option<&'static Protocol> {
  static PROTOCOL: OnceLock<Option<Protocol>> = OnceLock::new();
  (PROTOCOL.get_or_init(|| {
    let none: &[*const Interface] = &[null(); 4];
    let keyboard = Interface::new(
      c"zwp_virtual_keyboard_v1",
      &[
        (c"keymap", c"uhu", none),
        (c"key", c"uuu", none),
        (c"modifiers", c"uuuu", none),
        (c"destroy", c"", none),
      ],
      &[],
    );
    let types = [wl::lw()?.seat, keyboard];
    let manager = Interface::new(
      c"zwp_virtual_keyboard_manager_v1",
      &[(c"create_virtual_keyboard", c"on", &types)],
      &[],
    );
    Some(Protocol { manager, keyboard })
  }))
  .as_ref()
}

/// The app's virtual keyboard, and the keysym its keymap has.
struct Keyboard {
  client: &'static Client,
  keyboard: P,
  uploaded: Cell<Option<u32>>,
}

thread_local! {
  /// Made at the first shortcut; `None` where the compositor has no manager.
  static KEYBOARD: RefCell<Option<Option<Keyboard>>> = const { RefCell::new(None) };
}

/// The compositor has a virtual keyboard: shortcuts go through it ([post]).
pub(super) fn available() -> bool {
  with(|_| ()).is_some()
}

fn with<R>(f: impl FnOnce(&Keyboard) -> R) -> Option<R> {
  let client = wl::client()?;
  KEYBOARD.with_borrow_mut(|k| k.get_or_insert_with(|| make(client)).as_ref().map(f))
}

fn make(client: &'static Client) -> Option<Keyboard> {
  let p = protocol()?;
  let manager = client.bind(c"zwp_virtual_keyboard_manager_v1", p.manager)?;
  let seat = client.bind(c"wl_seat", wl::lw()?.seat)?;
  let mut args = [Arg { o: seat }, Arg { o: std::ptr::null_mut() }];
  let keyboard = wl::marshal(manager, 0, p.keyboard, &mut args);
  client.roundtrip().then_some(Keyboard { client, keyboard, uploaded: Cell::new(None) })
}

/// Presses `s` into the app in focus; `None` where the compositor has no virtual keyboard, so
/// the portal's keys are tried ([super::post]).
pub(super) fn post(s: Shortcut) -> Option<Result<(), String>> {
  with(|k| press(k, s))
}

fn press(k: &Keyboard, s: Shortcut) -> Result<(), String> {
  let sym = post::keysym(s.key).ok_or("no keysym for the key")? as u32;
  if k.uploaded.get() != Some(sym) {
    upload(k, sym)?;
    k.uploaded.set(Some(sym));
  }

  let time = time();
  let send = |opcode, mut args: Vec<Arg>| _ = wl::marshal(k.keyboard, opcode, null(), &mut args);
  send(MODIFIERS, vec![Arg { u: mask(s.mods) }, Arg { u: 0 }, Arg { u: 0 }, Arg { u: 0 }]);
  send(KEY, vec![Arg { u: time }, Arg { u: CODE }, Arg { u: 1 }]);
  send(KEY, vec![Arg { u: time }, Arg { u: CODE }, Arg { u: 0 }]);
  send(MODIFIERS, vec![Arg { u: 0 }, Arg { u: 0 }, Arg { u: 0 }, Arg { u: 0 }]);
  k.client.roundtrip().then_some(()).ok_or_else(|| "the compositor's connection failed".into())
}

/// A keymap of one key, at xkb's keycode 9 (evdev's [CODE]), that types `sym` (an X keysym), and
/// the modifier keys of [MASKS]: GTK takes a shortcut only where the keymap maps its modifiers.
fn keymap(sym: u32) -> String {
  format!(
    "xkb_keymap {{\n\
     xkb_keycodes \"nimbo\" {{ minimum = 8; maximum = 255; <K1> = 9; <LFSH> = 50; <LCTL> = 37; <LALT> = 64; <LWIN> = 133; }};\n\
     xkb_types \"nimbo\" {{ include \"complete\" }};\n\
     xkb_compatibility \"nimbo\" {{ include \"complete\" }};\n\
     xkb_symbols \"nimbo\" {{\n\
     key <K1> {{ [ 0x{sym:x} ] }};\n\
     key <LFSH> {{ [ Shift_L ] }}; key <LCTL> {{ [ Control_L ] }}; key <LALT> {{ [ Alt_L ] }}; key <LWIN> {{ [ Super_L ] }};\n\
     modifier_map Shift {{ <LFSH> }}; modifier_map Control {{ <LCTL> }}; modifier_map Mod1 {{ <LALT> }}; modifier_map Mod4 {{ <LWIN> }};\n\
     }};\n\
     }};\n"
  )
}

/// The keymap for `sym` to the keyboard, in a file of memory the compositor maps, its text
/// ended by a NUL as `wl_keyboard`'s are; the compositor has it once the roundtrip is back.
fn upload(k: &Keyboard, sym: u32) -> Result<(), String> {
  let mut text = keymap(sym).into_bytes();
  text.push(0);
  let fd = unsafe { memfd_create(c"nimbo-keymap".as_ptr(), MFD_CLOEXEC) };
  if fd < 0 {
    return Err(format!("no file for the keymap: {}", std::io::Error::last_os_error()));
  }

  // SAFETY: the new file's descriptor, owned here; libwayland sends a copy of it.
  let mut file = unsafe { File::from_raw_fd(fd) };
  file.write_all(&text).map_err(|e| format!("the keymap's file: {e}"))?;
  let mut args = [Arg { u: XKB_V1 }, Arg { i: file.as_raw_fd() }, Arg { u: text.len() as u32 }];
  wl::marshal(k.keyboard, KEYMAP, null(), &mut args);
  k.client.roundtrip().then_some(()).ok_or_else(|| "the compositor refused the keymap".into())
}

/// The modifiers as xkbcommon's mask of real modifiers.
fn mask(mods: Modifiers) -> u32 {
  MASKS.iter().filter(|(m, _)| mods.contains(*m)).fold(0, |all, (_, bit)| all | bit)
}

/// Milliseconds since the first key, as a key's time.
fn time() -> u32 {
  static START: OnceLock<Instant> = OnceLock::new();
  START.get_or_init(Instant::now).elapsed().as_millis() as u32
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn masks_of_the_modifiers() {
    assert_eq!(mask(Modifiers::CONTROL), 4);
    assert_eq!(mask(Modifiers::CONTROL | Modifiers::SHIFT), 5);
    assert_eq!(mask(Modifiers::empty()), 0);
    assert_eq!(mask(Modifiers::META | Modifiers::ALT), 72);
  }

  #[test]
  fn a_keymap_of_one_key() {
    let map = keymap(0x76);
    assert!(map.contains("<K1> = 9;") && map.contains("key <K1> { [ 0x76 ] };"), "{map}");
    assert!(map.contains("modifier_map Control { <LCTL> };"), "{map}");
  }
}
