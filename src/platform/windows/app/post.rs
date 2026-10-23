// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Shortcuts posted as key events on Windows through `SendInput`: the modifier keys set as the
//! shortcut wants them, the key down and up, the modifiers back as the user holds them
//! ([crate::keys::strokes]), in one call so no other input comes
//! between them. A letter or digit is its own virtual key on every layout (Ctrl-V is VK `V` on a
//! Russian one too); another character the key that types it on the layout of the window in
//! front (`VkKeyScanExW`). Windows drops input to an app running elevated above this one (UIPI).
//! The events carry [POSTED], so the app's own hotkey hook lets them pass unseen.

use keyboard_types::Modifiers;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
  GetAsyncKeyState, GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
  KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput, VkKeyScanExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::super::keyboard;
use super::hotkey::lone_vk;
use crate::keys::{Key, Shortcut, strokes};

/// The posted events' `dwExtraInfo`: the hotkey hook leaves them to the app in front.
pub(super) const POSTED: usize = u32::from_be_bytes(*b"nmbo") as usize;
/// The key tapped around a held Alt or Win lifted and put back: unassigned, so it does nothing,
/// but neither key then reads as tapped alone (AutoHotkey's menu mask key).
const MASK: u16 = 0xE8;

pub fn post(s: Shortcut) -> Result<(), String> {
  let (vk, shift) = match s.key {
    Key::Char(c) => char_key(c).ok_or(format!("no key types {c:?} without AltGr"))?,
    Key::Named(k) => (keyboard::vk_of(k).ok_or("no such key")?, false),
  };

  let mods = if shift { s.mods | Modifiers::SHIFT } else { s.mods };
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
