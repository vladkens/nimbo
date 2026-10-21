// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The keyboard through xkbcommon, loaded at run time ([super::dl]): the compositor's keymap,
//! modifier state, keysyms and the text a key types, dead keys and compose sequences from the
//! locale's compose table. Keys become [KeyEvent]s with macOS virtual key codes ([kvk]), so
//! bindings and [Key::named] work unchanged; Ctrl is the command modifier on Linux (`cmd`),
//! Super is `ctrl`.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::OnceLock;

use super::dl::{Lib, fns};
use crate::app::Mods;
use crate::keys::{self, Key, KeyEvent};

type P = *mut c_void;

fns!(Xkb {
  fn xkb_context_new(flags: c_int) -> P;
  fn xkb_context_unref(ctx: P);
  fn xkb_keymap_new_from_string(ctx: P, s: *const c_char, format: c_int, flags: c_int) -> P;
  fn xkb_keymap_new_from_names(ctx: P, names: *const RuleNames, flags: c_int) -> P;
  fn xkb_keymap_unref(keymap: P);
  fn xkb_keymap_num_layouts(keymap: P) -> u32;
  fn xkb_keymap_key_repeats(keymap: P, key: u32) -> c_int;
  fn xkb_keymap_key_get_syms_by_level(keymap: P, key: u32, layout: u32, level: u32,
    syms: *mut *const u32) -> c_int;
  fn xkb_state_new(keymap: P) -> P;
  fn xkb_state_unref(state: P);
  fn xkb_state_update_mask(state: P, dep: u32, lat: u32, lock: u32, dl: u32, ll: u32, kl: u32)
    -> c_int;
  fn xkb_state_update_key(state: P, key: u32, direction: c_int) -> c_int;
  fn xkb_state_key_get_one_sym(state: P, key: u32) -> u32;
  fn xkb_state_key_get_utf8(state: P, key: u32, buf: *mut c_char, size: usize) -> c_int;
  fn xkb_state_mod_name_is_active(state: P, name: *const c_char, kind: c_int) -> c_int;
  fn xkb_keysym_to_utf32(sym: u32) -> u32;
  fn xkb_compose_table_new_from_locale(ctx: P, locale: *const c_char, flags: c_int) -> P;
  fn xkb_compose_table_unref(table: P);
  fn xkb_compose_state_new(table: P, flags: c_int) -> P;
  fn xkb_compose_state_unref(state: P);
  fn xkb_compose_state_feed(state: P, sym: u32) -> c_int;
  fn xkb_compose_state_get_status(state: P) -> c_int;
  fn xkb_compose_state_get_utf8(state: P, buf: *mut c_char, size: usize) -> c_int;
  fn xkb_compose_state_reset(state: P);
});

/// `struct xkb_rule_names`.
#[repr(C)]
struct RuleNames {
  rules: *const c_char,
  model: *const c_char,
  layout: *const c_char,
  variant: *const c_char,
  options: *const c_char,
}

const KEYMAP_FORMAT_TEXT_V1: c_int = 1;
const STATE_MODS_EFFECTIVE: c_int = 1 << 3;
const COMPOSE_COMPOSING: c_int = 1;
const COMPOSE_COMPOSED: c_int = 2;
const COMPOSE_CANCELLED: c_int = 3;

struct Sys {
  f: Xkb,
}

/// The library's functions; each [Keyboard] has its own context (xkbcommon's objects are not
/// thread-safe).
fn sys() -> Option<&'static Sys> {
  static SYS: OnceLock<Option<Sys>> = OnceLock::new();
  SYS.get_or_init(|| Some(Sys { f: Xkb::load(&Lib::open(&[c"libxkbcommon.so.0"])?)? })).as_ref()
}

/// What a key press turns into after the app saw its [KeyEvent] and didn't use it.
#[derive(Debug, PartialEq)]
pub enum Typed {
  Text(String),
  Command(keys::Command),
  /// A dead key or a compose sequence in progress, or nothing.
  None,
}

/// A keymap and its state.
pub struct Keyboard {
  ctx: P,
  keymap: P,
  state: P,
  compose: Option<(P, P)>,
}

impl Drop for Keyboard {
  fn drop(&mut self) {
    let Some(s) = sys() else { return };
    unsafe {
      (s.f.xkb_state_unref)(self.state);
      (s.f.xkb_keymap_unref)(self.keymap);
      if let Some((table, state)) = self.compose {
        (s.f.xkb_compose_state_unref)(state);
        (s.f.xkb_compose_table_unref)(table);
      }

      (s.f.xkb_context_unref)(self.ctx);
    }
  }
}

impl Keyboard {
  /// From the compositor's keymap text (`wl_keyboard.keymap`, NUL-terminated).
  pub fn from_string(text: &CStr) -> Option<Self> {
    Self::new(|s, ctx| unsafe {
      (s.f.xkb_keymap_new_from_string)(ctx, text.as_ptr(), KEYMAP_FORMAT_TEXT_V1, 0)
    })
  }

  /// From RMLVO names, as `setxkbmap` takes them: `"us"`, `"us,ru"`.
  #[cfg(test)]
  pub fn from_layout(layout: &CStr) -> Option<Self> {
    Self::from_names([None, None, Some(layout), None, None])
  }

  /// From rules, model, layout, variant and options; xkbcommon's defaults for the missing.
  #[cfg(test)]
  pub fn from_names(rmlvo: [Option<&CStr>; 5]) -> Option<Self> {
    let p = |i: usize| rmlvo[i].map_or(std::ptr::null(), |s| s.as_ptr());
    let names = RuleNames { rules: p(0), model: p(1), layout: p(2), variant: p(3), options: p(4) };
    Self::new(|s, ctx| unsafe { (s.f.xkb_keymap_new_from_names)(ctx, &names, 0) })
  }

  fn new(keymap: impl FnOnce(&Sys, P) -> P) -> Option<Self> {
    let s = sys()?;
    let ctx = unsafe { (s.f.xkb_context_new)(0) };
    if ctx.is_null() {
      return None;
    }

    let keymap = keymap(s, ctx);
    if keymap.is_null() {
      unsafe { (s.f.xkb_context_unref)(ctx) };
      return None;
    }

    let state = unsafe { (s.f.xkb_state_new)(keymap) };
    let compose = compose_table(s, ctx).and_then(|table| {
      let st = unsafe { (s.f.xkb_compose_state_new)(table, 0) };
      (!st.is_null()).then_some((table, st))
    });
    Some(Self { ctx, keymap, state, compose })
  }

  /// `wl_keyboard.modifiers`.
  pub fn modifiers(&mut self, dep: u32, lat: u32, lock: u32, group: u32) {
    let Some(s) = sys() else { return };
    unsafe { (s.f.xkb_state_update_mask)(self.state, dep, lat, lock, 0, 0, group) };
  }

  pub fn mods(&self) -> Mods {
    let Some(s) = sys() else { return Mods::NONE };
    let on = |name: &CStr| unsafe {
      (s.f.xkb_state_mod_name_is_active)(self.state, name.as_ptr(), STATE_MODS_EFFECTIVE) > 0
    };
    Mods { shift: on(c"Shift"), ctrl: on(c"Mod4"), alt: on(c"Mod1"), cmd: on(c"Control") }
  }

  /// Whether a held `key` (evdev code) repeats.
  pub fn repeats(&self, key: u32) -> bool {
    sys().is_some_and(|s| unsafe { (s.f.xkb_keymap_key_repeats)(self.keymap, key + 8) } != 0)
  }

  /// The key event for evdev code `key`, and the text it types with every modifier applied.
  pub fn key(&self, key: u32, repeat: bool) -> (KeyEvent, String) {
    let mods = self.mods();
    let code = kvk(key).unwrap_or(0xffff);
    let utf8 = self.utf8(key);
    let Some(s) = sys() else {
      return (KeyEvent { key: Key::Other(code), text: None, code, mods, repeat }, utf8);
    };

    let sym = unsafe { (s.f.xkb_state_key_get_one_sym)(self.state, key + 8) };
    let ch = char::from_u32(unsafe { (s.f.xkb_keysym_to_utf32)(sym) }).filter(|&c| c != '\0');
    let shortcut = mods.cmd || mods.ctrl;
    let named = kvk(key).and_then(Key::named);
    let k = named.unwrap_or_else(|| match ch {
      Some(c) if shortcut && !c.is_ascii() => self
        .ascii(key, mods.shift)
        .or_else(|| keys::us_char(code, mods.shift))
        .map_or(Key::Char(c), Key::Char),
      Some(c) if !c.is_control() => Key::Char(c),
      _ => Key::Other(code),
    });
    let text = match k {
      Key::Char(_) if !shortcut && keys::printable(&utf8) => {
        let mut cs = utf8.chars();
        cs.next().filter(|_| cs.next().is_none())
      }
      _ => None,
    };

    (KeyEvent { key: k, text, code, mods, repeat }, utf8)
  }

  /// What a key nobody used does: a dead key or compose sequence goes to the compose state,
  /// editing keys become commands ([keys::pc]), the rest types its text (not with Ctrl, Super
  /// or Alt, as GTK).
  pub fn interpret(&mut self, key: u32, k: &KeyEvent, utf8: &str) -> Typed {
    if let Some(s) = sys()
      && let Some((_, cs)) = self.compose
      && !k.repeat
    {
      let sym = unsafe { (s.f.xkb_state_key_get_one_sym)(self.state, key + 8) };
      unsafe { (s.f.xkb_compose_state_feed)(cs, sym) };
      match unsafe { (s.f.xkb_compose_state_get_status)(cs) } {
        COMPOSE_COMPOSING => return Typed::None,
        COMPOSE_COMPOSED => {
          let text = read_utf8(|buf, n| unsafe { (s.f.xkb_compose_state_get_utf8)(cs, buf, n) });
          unsafe { (s.f.xkb_compose_state_reset)(cs) };
          return match keys::printable(&text) {
            true => Typed::Text(text),
            false => Typed::None,
          };
        }
        COMPOSE_CANCELLED => {
          unsafe { (s.f.xkb_compose_state_reset)(cs) };
          return Typed::None;
        }
        _ => {}
      }
    }

    if let Some(cmd) = keys::pc(k) {
      return Typed::Command(cmd);
    }

    let m = k.mods;
    match !(m.cmd || m.ctrl || m.alt) && keys::printable(utf8) {
      true => Typed::Text(utf8.to_owned()),
      false => Typed::None,
    }
  }

  fn utf8(&self, key: u32) -> String {
    let Some(s) = sys() else { return String::new() };
    read_utf8(|buf, n| unsafe { (s.f.xkb_state_key_get_utf8)(self.state, key + 8, buf, n) })
  }

  /// The ASCII character `key` types on the first layout that has one, Shift applied or not:
  /// shortcuts on a Russian layout match their Latin keys.
  fn ascii(&self, key: u32, shift: bool) -> Option<char> {
    let s = sys()?;
    let layouts = unsafe { (s.f.xkb_keymap_num_layouts)(self.keymap) };
    (0..layouts).find_map(|layout| {
      let mut syms = std::ptr::null();
      let n = unsafe {
        (s.f.xkb_keymap_key_get_syms_by_level)(
          self.keymap,
          key + 8,
          layout,
          shift as u32,
          &mut syms,
        )
      };
      if n < 1 || syms.is_null() {
        return None;
      }

      let c = char::from_u32(unsafe { (s.f.xkb_keysym_to_utf32)(*syms) })?;
      (c.is_ascii() && !c.is_ascii_control()).then_some(c)
    })
  }
}

fn compose_table(s: &Sys, ctx: P) -> Option<P> {
  let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
    .iter()
    .find_map(|v| std::env::var(v).ok().filter(|l| !l.is_empty()))
    .unwrap_or_else(|| "C".into());
  let locale = std::ffi::CString::new(locale).ok()?;
  let table = unsafe { (s.f.xkb_compose_table_new_from_locale)(ctx, locale.as_ptr(), 0) };
  (!table.is_null()).then_some(table)
}

/// A string from an xkbcommon call that fills a buffer of a size and returns the length.
fn read_utf8(f: impl Fn(*mut c_char, usize) -> c_int) -> String {
  let mut buf = [0u8; 64];
  let n = f(buf.as_mut_ptr().cast(), buf.len());
  let n = (n.max(0) as usize).min(buf.len() - 1);
  String::from_utf8_lossy(&buf[..n]).into_owned()
}

// MARK: Key codes

/// Evdev key codes (<linux/input-event-codes.h>) to macOS virtual key codes (`kVK_*`): US ANSI
/// positions, as [crate::keys] names them.
const KVK: &[(u16, u16)] = &[
  (1, 53),    // Esc
  (2, 18),    // 1
  (3, 19),    // 2
  (4, 20),    // 3
  (5, 21),    // 4
  (6, 23),    // 5
  (7, 22),    // 6
  (8, 26),    // 7
  (9, 28),    // 8
  (10, 25),   // 9
  (11, 29),   // 0
  (12, 27),   // -
  (13, 24),   // =
  (14, 51),   // Backspace
  (15, 48),   // Tab
  (16, 12),   // Q
  (17, 13),   // W
  (18, 14),   // E
  (19, 15),   // R
  (20, 17),   // T
  (21, 16),   // Y
  (22, 32),   // U
  (23, 34),   // I
  (24, 31),   // O
  (25, 35),   // P
  (26, 33),   // [
  (27, 30),   // ]
  (28, 36),   // Enter
  (29, 59),   // Left Ctrl
  (30, 0),    // A
  (31, 1),    // S
  (32, 2),    // D
  (33, 3),    // F
  (34, 5),    // G
  (35, 4),    // H
  (36, 38),   // J
  (37, 40),   // K
  (38, 37),   // L
  (39, 41),   // ;
  (40, 39),   // '
  (41, 50),   // `
  (42, 56),   // Left Shift
  (43, 42),   // backslash
  (44, 6),    // Z
  (45, 7),    // X
  (46, 8),    // C
  (47, 9),    // V
  (48, 11),   // B
  (49, 45),   // N
  (50, 46),   // M
  (51, 43),   // ,
  (52, 47),   // .
  (53, 44),   // /
  (54, 60),   // Right Shift
  (55, 67),   // Keypad *
  (56, 58),   // Left Alt
  (57, 49),   // Space
  (58, 57),   // Caps Lock
  (59, 122),  // F1
  (60, 120),  // F2
  (61, 99),   // F3
  (62, 118),  // F4
  (63, 96),   // F5
  (64, 97),   // F6
  (65, 98),   // F7
  (66, 100),  // F8
  (67, 101),  // F9
  (68, 109),  // F10
  (69, 71),   // Num Lock: Keypad Clear
  (71, 89),   // Keypad 7
  (72, 91),   // Keypad 8
  (73, 92),   // Keypad 9
  (74, 78),   // Keypad -
  (75, 86),   // Keypad 4
  (76, 87),   // Keypad 5
  (77, 88),   // Keypad 6
  (78, 69),   // Keypad +
  (79, 83),   // Keypad 1
  (80, 84),   // Keypad 2
  (81, 85),   // Keypad 3
  (82, 82),   // Keypad 0
  (83, 65),   // Keypad .
  (86, 10),   // 102nd key: ISO section
  (87, 103),  // F11
  (88, 111),  // F12
  (96, 76),   // Keypad Enter
  (97, 62),   // Right Ctrl
  (98, 75),   // Keypad /
  (100, 61),  // Right Alt
  (102, 115), // Home
  (103, 126), // Up
  (104, 116), // Page Up
  (105, 123), // Left
  (106, 124), // Right
  (107, 119), // End
  (108, 125), // Down
  (109, 121), // Page Down
  (110, 114), // Insert: Help
  (111, 117), // Delete
  (117, 81),  // Keypad =
  (125, 55),  // Left Super: Command
  (126, 54),  // Right Super
];

/// The macOS virtual key code of evdev key `key`.
pub fn kvk(key: u32) -> Option<u16> {
  KVK.iter().find(|(e, _)| *e as u32 == key).map(|(_, k)| *k)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::keys::{Command, Delete, Move};

  // Evdev codes.
  const A: u32 = 30;
  const C: u32 = 46;
  const LEFT: u32 = 105;
  const BACKSPACE: u32 = 14;
  const APOSTROPHE: u32 = 40;
  const E: u32 = 18;

  /// Modifier masks of the default keymaps: Shift, Control, Mod1 (Alt).
  const SHIFT: u32 = 1;
  const CTRL: u32 = 4;

  #[test]
  fn keys_type_and_name_like_appkit() {
    let Some(mut kb) = Keyboard::from_layout(c"us") else {
      eprintln!("no xkbcommon: skipped");
      return;
    };
    let (k, text) = kb.key(A, false);
    assert_eq!((k.key, k.text, k.code, text.as_str()), (Key::Char('a'), Some('a'), 0, "a"));
    assert_eq!(kb.interpret(A, &k, &text), Typed::Text("a".into()));

    kb.modifiers(SHIFT, 0, 0, 0);
    let (k, text) = kb.key(A, false);
    assert_eq!((k.key, k.text, k.mods), (Key::Char('A'), Some('A'), Mods::SHIFT));
    let (k, _) = kb.key(LEFT, false);
    assert_eq!(kb.interpret(LEFT, &k, ""), Typed::Command(Command::Move(Move::Left, true)));
    assert_eq!(text, "A");

    // Ctrl is the command modifier: Ctrl-C is "cmd-c", and types nothing.
    kb.modifiers(CTRL, 0, 0, 0);
    let (k, text) = kb.key(C, false);
    assert!(k.is("cmd-c") && k.text.is_none(), "{k:?}");
    assert_eq!(kb.interpret(C, &k, &text), Typed::None);
    let (k, _) = kb.key(BACKSPACE, false);
    assert_eq!(
      kb.interpret(BACKSPACE, &k, ""),
      Typed::Command(Command::Delete(Delete::WordBackward))
    );
  }

  #[test]
  fn shortcuts_on_a_cyrillic_layout_use_the_latin_key() {
    let Some(mut kb) = Keyboard::from_layout(c"ru,us") else { panic!("no ru,us keymap") };
    let (k, text) = kb.key(C, false);
    assert_eq!((k.key, text.as_str()), (Key::Char('с'), "с"));
    kb.modifiers(CTRL, 0, 0, 0);
    let (k, _) = kb.key(C, false);
    assert!(k.is("cmd-c"), "{k:?}");
  }

  #[test]
  fn dead_keys_compose() {
    let Some(mut kb) = Keyboard::from_layout(c"us(intl)") else { panic!("no us(intl) keymap") };
    if kb.compose.is_none() {
      eprintln!("no compose table: skipped");
      return;
    }
    // US international: ' is dead, then e gives é.
    let (k, text) = kb.key(APOSTROPHE, false);
    assert_eq!(kb.interpret(APOSTROPHE, &k, &text), Typed::None);
    let (k, text) = kb.key(E, false);
    assert_eq!(kb.interpret(E, &k, &text), Typed::Text("é".into()));
  }

  #[test]
  fn key_codes_map_to_named_keys() {
    assert_eq!(kvk(LEFT).and_then(Key::named), Some(Key::Left));
    assert_eq!(kvk(28).and_then(Key::named), Some(Key::Enter));
    assert_eq!(kvk(88).and_then(Key::named), Some(Key::F(12)));
    assert_eq!(kvk(A), Some(0));
  }
}
