// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Shortcuts posted as key events on Windows through `SendInput`: the modifier keys down, the key
//! down and up, the modifiers up ([crate::keys::strokes]), in one call so no other input comes
//! between them. A letter or digit is its own virtual key on every layout (Ctrl-V is VK `V` on a
//! Russian one too); another character the key that types it on the layout of the window in
//! front (`VkKeyScanExW`). Windows drops input to an app running elevated above this one (UIPI).

use keyboard_types::Modifiers;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
  GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
  KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput, VK_CONTROL, VK_LWIN, VK_MENU,
  VK_SHIFT, VkKeyScanExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::super::keyboard;
use crate::keys::{Key, Shortcut, lower, strokes};

pub fn post(s: Shortcut) -> Result<(), String> {
  let (vk, shift) = match s.key {
    Key::Char(c) => char_key(lower(c)).ok_or(format!("no key types {c:?} without AltGr"))?,
    Key::Named(k) => (keyboard::vk_of(k).ok_or("no such key")?, false),
  };

  let mods = if shift { s.mods | Modifiers::SHIFT } else { s.mods };
  let modkey = |m| match m {
    Modifiers::CONTROL => VK_CONTROL,
    Modifiers::ALT => VK_MENU,
    Modifiers::SHIFT => VK_SHIFT,
    _ => VK_LWIN,
  };
  let inputs: Vec<INPUT> =
    strokes(mods, vk, modkey).into_iter().map(|(vk, d)| input(vk, d)).collect();
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
  let ki = KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 };
  INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki } }
}
