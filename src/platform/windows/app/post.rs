// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Shortcuts posted as key events on Windows through `SendInput`: the modifier keys set as the
//! shortcut wants them, the key down and up, the modifiers back as the user holds them
//! ([crate::keys::strokes]), in one call so no other input comes
//! between them. A letter or digit is its own virtual key on every layout (Ctrl-V is VK `V` on a
//! Russian one too); another character the key that types it on the layout of the window in
//! front (`VkKeyScanExW`). Windows drops input to an app running elevated above this one (UIPI).
//! The events carry [super::POSTED], so the app's own hotkey hook lets them pass unseen. Win+.
//! opens the emoji panel for the app's own field ([emoji_panel]).

use std::ptr::null_mut;
use std::sync::OnceLock;

use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
  GetAsyncKeyState, GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
  KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput,
  VK_OEM_PERIOD, VkKeyScanExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::super::{keyboard, wide};
use super::{POSTED, lone_vk};
use crate::Modifiers;
use crate::keys::{Key, Shortcut, strokes};

/// The key tapped around a held Alt or Win lifted and put back: unassigned, so it does nothing,
/// but neither key then reads as tapped alone (AutoHotkey's menu mask key).
const MASK: u16 = 0xE8;

pub fn ready() -> bool {
  true
}

pub fn post(s: Shortcut) -> Result<(), String> {
  let (vk, shift) = match s.key {
    Key::Char(c) => char_key(c).ok_or(format!("no key types {c:?} without AltGr"))?,
    Key::Named(k) => (keyboard::vk_of(k).ok_or("no such key")?, false),
  };

  let mods = if shift { s.mods | Modifiers::SHIFT } else { s.mods };
  press(mods, vk)
}

/// Opens the emoji panel for the focused field with Win+., as Chromium's `ShowEmojiPanel` does
/// (`ui/base/emoji/emoji_panel_helper_win.cc`): Windows has no Win32 API for it, only WinRT's
/// `CoreInputView`. The panel types the emoji into the window as text (`WM_CHAR`, or the input
/// method's result string).
pub(crate) fn emoji_panel() -> Result<(), String> {
  press(Modifiers::META, VK_OEM_PERIOD)
}

/// Whether the emoji panel opens here ([crate::character_palette]): from Windows 10 1709 (build
/// 16299).
pub fn character_palette() -> Result<(), String> {
  static BUILD: OnceLock<Option<u32>> = OnceLock::new();
  match *BUILD.get_or_init(build) {
    Some(b) if b < 16299 => Err(format!("Windows build {b} has no emoji panel (1709+)")),
    _ => Ok(()),
  }
}

/// The Windows build, the registry's `CurrentBuildNumber`: the version functions report the
/// version the app's manifest declares. `None` when it can't be read.
fn build() -> Option<u32> {
  let key = wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
  let name = wide("CurrentBuildNumber");
  let (mut buf, mut len) = ([0u16; 16], 32u32);
  let p = buf.as_mut_ptr().cast();
  let flags = RRF_RT_REG_SZ;
  let err = unsafe {
    RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), name.as_ptr(), flags, null_mut(), p, &mut len)
  };
  let s = String::from_utf16_lossy(&buf[..(len as usize / 2).min(buf.len())]);
  (err == 0).then(|| s.trim_end_matches('\0').parse().ok()).flatten()
}

/// Sends virtual key `vk` with modifiers `mods` in one `SendInput` ([strokes]).
fn press(mods: Modifiers, vk: u16) -> Result<(), String> {
  let of = |code| lone_vk(code).map(|vk| vk as u16);
  let down = |vk: u16| unsafe { GetAsyncKeyState(vk.into()) as u16 & 0x8000 != 0 };
  let presses = strokes(mods, vk, of, down, Some(MASK)).ok_or("no modifier key")?;
  let inputs: Vec<INPUT> = presses.into_iter().map(|(vk, d)| input(vk, d)).collect();
  let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
  match sent as usize == inputs.len() {
    true => Ok(()),
    false => Err("SendInput was blocked (an elevated app in front?)".into()),
  }
}

/// The virtual key typing `c` and whether it takes Shift.
fn char_key(c: char) -> Option<(u16, bool)> {
  if c.is_ascii_alphanumeric() {
    return Some((c.to_ascii_uppercase() as u16, false));
  }

  let mut unit = [0u16; 2];
  let [unit] = c.encode_utf16(&mut unit) else { return None };
  let thread = unsafe { GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut()) };
  let scan = unsafe { VkKeyScanExW(*unit, GetKeyboardLayout(thread)) };
  // The low byte the key, the high one the shift state: 1 Shift, 2 Ctrl, 4 Alt.
  let (vk, state) = ((scan as u16) & 0xFF, (scan as u16) >> 8);
  (scan != -1 && state & !1 == 0).then_some((vk, state & 1 != 0))
}

fn input(vk: u16, down: bool) -> INPUT {
  let mut flags = if down { 0 } else { KEYEVENTF_KEYUP };
  if keyboard::extended(vk) {
    flags |= KEYEVENTF_EXTENDEDKEY;
  }

  let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
  let ki = KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: POSTED };
  INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki } }
}
