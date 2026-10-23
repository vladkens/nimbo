// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The hotkey on Windows, on a "hotkey" thread with a message queue of its own; the app's
//! callback runs on another ("hotkey-edges"), so a slow one never holds up the hook or a
//! registration. A chord is `RegisterHotKey`'s (for the thread, no window): its presses and
//! autorepeats come as `WM_HOTKEY`, its release, which Windows doesn't report, is found by
//! polling the key's state every [POLL] after the press. A modifier key alone is a low-level
//! keyboard hook's (`WH_KEYBOARD_LL`, installed by `start` on the same thread): [Lone] sees the
//! key go down with no other modifier held and up again, and another key pressed within
//! [CHORD_WINDOW] makes it the user's chord. While [Hotkeys::escape] is on the hook also takes
//! Escape ([Lone::escape]): it never reaches the app in front. The hook only queues and posts:
//! low-level hooks that take too long are removed by Windows without a word. Every other key
//! passes through.
//!
//! Keys by place: a character key's virtual key is the US layout's scan code of its place mapped
//! by the current layout (`MapVirtualKeyW`), a named key's is fixed. The Fn key never reaches
//! Windows.

use std::cell::RefCell;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keyboard_types::{Code, Modifiers};
use windows_sys::Win32::Foundation::{GetLastError, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::hotkey::{Edge, Listener, OnEdge, Status};
use crate::keys::Hotkey;
use crate::platform::scan_code;

/// How often a chord's key is looked at while held, for its release.
const POLL: Duration = Duration::from_millis(10);
/// A key pressed this soon after a modifier hotkey went down makes it a chord of the user's.
const CHORD_WINDOW: Duration = Duration::from_millis(300);
/// The hotkey thread's messages: commands waiting, edges the hook queued.
const WM_CMD: u32 = WM_APP + 1;
const WM_EDGE: u32 = WM_APP + 2;
const CHORD_ID: i32 = 1;
/// The fake Left Ctrl an AltGr layout sends before Right Alt: its scan code.
const ALTGR_CTRL: u32 = 0x21D;

const STARTING: u8 = 1;
const ACTIVE: u8 = 2;
const FAILED: u8 = 3;

/// What the hotkey is, as the thread takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
  Chord { vk: u32, mods: u32 },
  Lone(u32),
}

enum Cmd {
  Set(Option<Target>, SyncSender<Result<(), String>>),
  Start,
  Stop,
}

/// `listener`: 0, its default, is off (not started), then these.
#[derive(Default)]
struct Shared {
  listener: AtomicU8,
  listener_error: Mutex<Option<String>>,
  fallback: AtomicBool,
  fallback_error: Mutex<Option<String>>,
  listener_events: AtomicU64,
  fallback_events: AtomicU64,
  fallback_repeats: AtomicU64,
  /// The hook takes Escape while this is set.
  escape: AtomicBool,
}

/// The hotkey thread, driven from the main thread.
pub struct Hotkeys {
  thread: u32,
  cmds: Sender<Cmd>,
  shared: Arc<Shared>,
}

impl Hotkeys {
  pub fn new(on: OnEdge) -> Self {
    let shared = Arc::new(Shared::default());
    let (cmds, rx) = channel();
    let (ready, id) = sync_channel(1);
    let s = shared.clone();
    let (edges, heard) = channel::<Edge>();
    std::thread::Builder::new()
      .name("hotkey-edges".into())
      .spawn(move || heard.into_iter().for_each(on))
      .expect("spawn the hotkey edges thread");
    std::thread::Builder::new()
      .name("hotkey".into())
      .spawn(move || run(&edges, s, &rx, &ready))
      .expect("spawn the hotkey thread");
    let thread = id.recv().expect("the hotkey thread starts");
    Self { thread, cmds, shared }
  }

  fn send(&self, cmd: Cmd) {
    if self.cmds.send(cmd).is_ok() {
      unsafe { PostThreadMessageW(self.thread, WM_CMD, 0, 0) };
    }
  }

  /// Takes `hotkey` on the thread; the answer waits for `RegisterHotKey`'s.
  pub fn set(&mut self, hotkey: Option<Hotkey>) -> Result<(), String> {
    let (target, err) = match hotkey.map(target).transpose() {
      Ok(t) => (t, None),
      Err(e) => (None, Some(e)),
    };

    let (tx, rx) = sync_channel(1);
    self.send(Cmd::Set(target, tx));
    let answer = rx.recv_timeout(Duration::from_secs(2));
    let taken = answer.unwrap_or_else(|_| Err("the hotkey thread did not answer".into()));
    err.map_or(taken, Err)
  }

  /// Installs the hook, unless it is in or going in.
  pub fn start(&mut self) -> Result<(), String> {
    let was = self.shared.listener.load(Ordering::Acquire);
    if matches!(was, STARTING | ACTIVE) {
      return Ok(());
    }

    self.shared.listener.store(STARTING, Ordering::Release);
    self.send(Cmd::Start);
    Ok(())
  }

  /// While on, the hook takes Escape.
  pub fn escape(&mut self, on: bool) {
    self.shared.escape.store(on, Ordering::Release);
  }

  pub fn status(&self, s: &mut Status) {
    let sh = &self.shared;
    let n = |a: &AtomicU64| a.load(Ordering::Relaxed);
    s.listener = match sh.listener.load(Ordering::Acquire) {
      STARTING => Listener::Starting,
      ACTIVE => Listener::Active,
      FAILED => Listener::Failed,
      _ => Listener::Off,
    };
    s.listener_error = sh.listener_error.lock().ok().and_then(|e| e.clone());
    s.fallback = sh.fallback.load(Ordering::Acquire);
    s.fallback_error = sh.fallback_error.lock().ok().and_then(|e| e.clone());
    s.counts.listener = n(&sh.listener_events);
    s.counts.fallback = n(&sh.fallback_events);
    s.counts.fallback_repeats = n(&sh.fallback_repeats);
    if sh.escape.load(Ordering::Relaxed) && s.listener != Listener::Active {
      let why = s.listener_error.clone();
      s.escape_error = Some(why.unwrap_or_else(|| "the keyboard hook is not installed".into()));
    }
  }
}

impl Drop for Hotkeys {
  fn drop(&mut self) {
    self.send(Cmd::Stop);
  }
}

// MARK: The thread

/// What the hook works with, on the hotkey thread.
#[derive(Default)]
struct Hook {
  lone: Lone,
  queue: Vec<Edge>,
  thread: u32,
  shared: Option<Arc<Shared>>,
}

thread_local! {
  static HOOK: RefCell<Hook> = RefCell::default();
}

/// The thread's state besides the hook's.
struct Chord {
  registered: bool,
  /// The chord's key, held since its press, polled by the timer.
  held: Option<u32>,
  timer: usize,
  hook: HHOOK,
}

fn run(edges: &Sender<Edge>, shared: Arc<Shared>, cmds: &Receiver<Cmd>, ready: &SyncSender<u32>) {
  let on = |e| _ = edges.send(e);
  let mut msg: MSG = unsafe { std::mem::zeroed() };
  // The thread's message queue exists once it asked for a message.
  unsafe { PeekMessageW(&mut msg, null_mut(), WM_USER, WM_USER, PM_NOREMOVE) };
  let thread = unsafe { GetCurrentThreadId() };
  HOOK.with_borrow_mut(|h| (h.thread, h.shared) = (thread, Some(shared.clone())));
  _ = ready.send(thread);
  let mut chord = Chord { registered: false, held: None, timer: 0, hook: null_mut() };
  while unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) } > 0 {
    let now = Instant::now();
    match msg.message {
      WM_HOTKEY => {
        shared.fallback_events.fetch_add(1, Ordering::Relaxed);
        let repeat = chord.held.is_some();
        if repeat {
          shared.fallback_repeats.fetch_add(1, Ordering::Relaxed);
        } else {
          // The key is the high word of lParam.
          chord.held = Some((msg.lParam >> 16 & 0xFFFF) as u32);
          chord.timer = unsafe { SetTimer(null_mut(), 0, POLL.as_millis() as u32, None) };
        }

        on(Edge::Down { at: now, repeat });
      }
      WM_TIMER if msg.wParam == chord.timer && chord.timer != 0 => {
        let up =
          chord.held.is_none_or(|vk| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 == 0);
        if up {
          unsafe { KillTimer(null_mut(), chord.timer) };
          (chord.held, chord.timer) = (None, 0);
          on(Edge::Up { at: now });
        }
      }
      WM_EDGE => {
        for edge in HOOK.with_borrow_mut(|h| std::mem::take(&mut h.queue)) {
          shared.listener_events.fetch_add(1, Ordering::Relaxed);
          on(edge);
        }
      }
      WM_CMD => {
        while let Ok(cmd) = cmds.try_recv() {
          match cmd {
            Cmd::Set(target, reply) => _ = reply.send(set(&shared, &mut chord, target)),
            Cmd::Start => start(&shared, &mut chord),
            Cmd::Stop => {
              _ = set(&shared, &mut chord, None);
              if !chord.hook.is_null() {
                unsafe { UnhookWindowsHookEx(chord.hook) };
              }

              return;
            }
          }
        }
      }
      _ => unsafe {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
      },
    }
  }
}

/// Takes `target`: the chord registered (an error when another app has it), or the key the hook
/// watches.
fn set(shared: &Shared, chord: &mut Chord, target: Option<Target>) -> Result<(), String> {
  if chord.registered {
    unsafe { UnregisterHotKey(null_mut(), CHORD_ID) };
    chord.registered = false;
  }

  if chord.timer != 0 {
    unsafe { KillTimer(null_mut(), chord.timer) };
    (chord.held, chord.timer) = (None, 0);
  }

  let lone = match target {
    Some(Target::Lone(vk)) => vk,
    _ => 0,
  };

  HOOK.with_borrow_mut(|h| h.lone.watch(lone));
  let result = match target {
    Some(Target::Chord { vk, mods }) => {
      match unsafe { RegisterHotKey(null_mut(), CHORD_ID, mods, vk) } {
        0 => Err(match unsafe { GetLastError() } {
          1409 => "another app has registered this hotkey".to_string(),
          1459 => "no interactive desktop here (a service or a remote shell)".to_string(),
          e => format!("RegisterHotKey failed (error {e})"),
        }),
        _ => Ok(()),
      }
    }
    _ => Ok(()),
  };

  chord.registered = matches!(target, Some(Target::Chord { .. })) && result.is_ok();
  shared.fallback.store(chord.registered, Ordering::Release);
  if let Ok(mut e) = shared.fallback_error.lock() {
    *e = result.clone().err();
  }

  result
}

/// Installs the low-level keyboard hook on this thread.
fn start(shared: &Shared, chord: &mut Chord) {
  if chord.hook.is_null() {
    let module = unsafe { GetModuleHandleW(std::ptr::null()) };
    chord.hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), module, 0) };
  }

  let failed = chord.hook.is_null();
  let error =
    failed.then(|| format!("SetWindowsHookExW failed (error {})", unsafe { GetLastError() }));
  if let Ok(mut e) = shared.listener_error.lock() {
    *e = error;
  }

  shared.listener.store(if failed { FAILED } else { ACTIVE }, Ordering::Release);
}

/// Every key in the session: [Lone] judges it, an edge is queued for the thread's loop.
unsafe extern "system" fn hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
  // SAFETY: for HC_ACTION, lParam points to the event's KBDLLHOOKSTRUCT.
  let k = (code == HC_ACTION as i32).then(|| unsafe { &*(lp as *const KBDLLHOOKSTRUCT) });
  let fake = k.is_some_and(|k| k.vkCode == VK_LCONTROL as u32 && k.scanCode == ALTGR_CTRL);
  if let Some(k) = k.filter(|_| !fake) {
    // AltGr's Left Ctrl, filtered, is no key of the user's: Right Alt stays alone.
    let down = matches!(wp as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
    let now = Instant::now();
    let taken = HOOK.with_borrow_mut(|h| {
      let on = h.shared.as_ref().is_some_and(|s| s.escape.load(Ordering::Acquire));
      let (hit, taken) = h.lone.judge(on, k.vkCode, down, now);
      if let Some(hit) = hit {
        h.queue.push(match hit {
          Hit::Down => Edge::Down { at: now, repeat: false },
          Hit::Up => Edge::Up { at: now },
          Hit::Escape => Edge::Escape,
          Hit::Aborted => Edge::Aborted,
        });
        unsafe { PostThreadMessageW(h.thread, WM_EDGE, 0, 0) };
      }

      taken
    });
    if taken {
      return 1;
    }
  }

  unsafe { CallNextHookEx(null_mut(), code, wp, lp) }
}

// MARK: A modifier alone

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
  Down,
  Up,
  Escape,
  Aborted,
}

/// A modifier key alone as the hotkey: its press with no other modifier held, its release; a
/// key pressed within [CHORD_WINDOW] while it is held makes it the user's chord.
#[derive(Debug, Default)]
struct Lone {
  /// The key's virtual key (left or right), 0 for none.
  key: u32,
  held: Option<Instant>,
  /// The modifier keys down now.
  down: Vec<u32>,
  /// Escape is down: whether it was taken.
  escape: Option<bool>,
}

impl Lone {
  /// Watches modifier key `key` (0: none) from now on; the keys down, Escape's too, stay known.
  fn watch(&mut self, key: u32) {
    (self.key, self.held) = (key, None);
  }

  /// What the hook does with a key: the edge to tell, and whether the key is taken ([Lone::key],
  /// then [Lone::escape] while `escape`). Escape that aborted the hotkey is the user's chord: the
  /// app's.
  fn judge(&mut self, escape: bool, vk: u32, down: bool, now: Instant) -> (Option<Hit>, bool) {
    let hit = self.key(vk, down, now);
    let taken = self.escape(escape && hit.is_none(), vk, down);
    (hit.or(taken.flatten()), taken.is_some())
  }

  fn key(&mut self, vk: u32, down: bool, now: Instant) -> Option<Hit> {
    let others = self.down.iter().any(|&m| m != self.key);
    let repeat = down && self.down.contains(&vk);
    if is_modifier(vk) {
      self.down.retain(|&m| m != vk);
      if down {
        self.down.push(vk);
      }
    }

    if self.key == 0 {
      return None;
    }

    if vk == self.key {
      return match (down, self.held) {
        (true, None) if !others => {
          self.held = Some(now);
          Some(Hit::Down)
        }
        (false, Some(_)) => {
          self.held = None;
          Some(Hit::Up)
        }
        _ => None,
      };
    }

    let since = self.held.filter(|_| down && !repeat)?;
    if now.saturating_duration_since(since) < CHORD_WINDOW {
      self.held = None;
      return Some(Hit::Aborted);
    }

    None
  }

  /// Escape while taking it is `on`: a press with no Ctrl, Alt or Win held (Shift is fine, and so
  /// is the hotkey's own key, held past [CHORD_WINDOW]) is taken and told; its autorepeats and
  /// release are taken too, also once it went off. A press the app saw stays the app's. `Some`:
  /// swallowed, with the edge if any.
  fn escape(&mut self, on: bool, vk: u32, down: bool) -> Option<Option<Hit>> {
    if vk != VK_ESCAPE as u32 {
      return None;
    }

    if !down {
      return self.escape.take().filter(|&taken| taken).map(|_| None);
    }

    if let Some(taken) = self.escape {
      return taken.then_some(None);
    }

    let own = self.held.map(|_| self.key);
    let shift = [VK_LSHIFT as u32, VK_RSHIFT as u32];
    let chord = self.down.iter().any(|&m| Some(m) != own && !shift.contains(&m));
    let taken = on && !chord;
    self.escape = Some(taken);
    taken.then_some(Some(Hit::Escape))
  }
}

fn is_modifier(vk: u32) -> bool {
  (VK_LSHIFT as u32..=VK_RMENU as u32).contains(&vk) || vk == VK_LWIN as u32 || vk == VK_RWIN as u32
}

// MARK: Keys

/// The thread's target for `hotkey`.
fn target(hotkey: Hotkey) -> Result<Target, String> {
  match hotkey {
    Hotkey::Modifier(code) => lone_vk(code).map(Target::Lone).ok_or_else(|| match code {
      Code::Fn => "the fn key is not visible to Windows".to_string(),
      code => format!("{code} is no modifier key"),
    }),
    Hotkey::Shortcut(s) => {
      let vk =
        s.code().and_then(vk_of).ok_or_else(|| format!("{s} has no key on this keyboard"))?;
      Ok(Target::Chord { vk, mods: mods_of(s.mods) })
    }
  }
}

/// `RegisterHotKey`'s modifier bits; ⌘ (`cmd`) is the Windows key.
fn mods_of(m: Modifiers) -> u32 {
  let bits = [
    (Modifiers::ALT, MOD_ALT),
    (Modifiers::CONTROL, MOD_CONTROL),
    (Modifiers::SHIFT, MOD_SHIFT),
    (Modifiers::META, MOD_WIN),
  ];
  bits.iter().filter(|(k, _)| m.contains(*k)).fold(0, |all, (_, b)| all | b)
}

/// The left or right modifier key at `code`.
fn lone_vk(code: Code) -> Option<u32> {
  let vk = match code {
    Code::ControlLeft => VK_LCONTROL,
    Code::ControlRight => VK_RCONTROL,
    Code::ShiftLeft => VK_LSHIFT,
    Code::ShiftRight => VK_RSHIFT,
    Code::AltLeft => VK_LMENU,
    Code::AltRight => VK_RMENU,
    Code::MetaLeft => VK_LWIN,
    Code::MetaRight => VK_RWIN,
    _ => return None,
  };

  Some(vk as u32)
}

/// A named key's virtual key, the same on every layout.
fn named_vk(code: Code) -> Option<u32> {
  use Code::*;
  let f =
    [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16, F17, F18, F19, F20];
  if let Some(i) = f.iter().position(|&k| k == code) {
    return Some(VK_F1 as u32 + i as u32);
  }

  let vk = match code {
    ArrowLeft => VK_LEFT,
    ArrowUp => VK_UP,
    ArrowRight => VK_RIGHT,
    ArrowDown => VK_DOWN,
    Home => VK_HOME,
    End => VK_END,
    PageUp => VK_PRIOR,
    PageDown => VK_NEXT,
    Insert => VK_INSERT,
    Delete => VK_DELETE,
    Enter => VK_RETURN,
    Tab => VK_TAB,
    Backspace => VK_BACK,
    Escape => VK_ESCAPE,
    Space => VK_SPACE,
    _ => return None,
  };

  Some(vk as u32)
}

/// The scan code (set 1) of the key at `code` on a PC keyboard: a character key's place.
fn scan_of(code: Code) -> Option<u32> {
  (1u16..89).find(|&s| scan_code(s) == code).map(u32::from)
}

/// The virtual key at `code` with the current layout.
fn vk_of(code: Code) -> Option<u32> {
  named_vk(code).or_else(|| {
    let vk = unsafe { MapVirtualKeyW(scan_of(code)?, MAPVK_VSC_TO_VK) };
    (vk != 0).then_some(vk)
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn hotkeys_map_to_virtual_keys() {
    let chord = |s: &str| target(Hotkey::parse(s).unwrap());
    assert_eq!(
      chord("ctrl+option+F12"),
      Ok(Target::Chord { vk: VK_F12 as u32, mods: MOD_CONTROL | MOD_ALT })
    );
    assert_eq!(
      chord("cmd+shift+space"),
      Ok(Target::Chord { vk: VK_SPACE as u32, mods: MOD_WIN | MOD_SHIFT })
    );
    assert_eq!(chord("right_option"), Ok(Target::Lone(VK_RMENU as u32)));
    assert_eq!(chord("left_cmd"), Ok(Target::Lone(VK_LWIN as u32)));
    assert!(chord("fn").is_err_and(|e| e.contains("fn")));
    assert_eq!(
      (scan_of(Code::KeyA), scan_of(Code::Digit1), scan_of(Code::Slash)),
      (Some(0x1E), Some(0x02), Some(0x35))
    );
    assert_eq!(named_vk(Code::PageDown), Some(VK_NEXT as u32));
    assert_eq!(named_vk(Code::KeyA), None, "by the layout");
  }

  #[test]
  fn a_modifier_alone_fires_on_its_press_and_release() {
    let t0 = Instant::now();
    let ms = |n| t0 + Duration::from_millis(n);
    let (ralt, lalt, shift, e) = (VK_RMENU as u32, VK_LMENU as u32, VK_LSHIFT as u32, 0x45);
    let mut l = Lone { key: ralt, ..Lone::default() };
    assert_eq!(l.key(ralt, true, ms(0)), Some(Hit::Down));
    assert_eq!(l.key(ralt, true, ms(30)), None, "its autorepeat");
    assert_eq!(l.key(ralt, false, ms(100)), Some(Hit::Up));
    // Another modifier held first: the user's chord.
    assert_eq!(l.key(lalt, true, ms(200)), None);
    assert_eq!(l.key(ralt, true, ms(210)), None);
    assert_eq!(l.key(ralt, false, ms(220)), None, "no release owed");
    assert_eq!(l.key(lalt, false, ms(230)), None);
    // A key soon after the press aborts it; its release is no edge.
    assert_eq!(l.key(ralt, true, ms(300)), Some(Hit::Down));
    assert_eq!(l.key(e, true, ms(350)), Some(Hit::Aborted));
    assert_eq!(l.key(ralt, false, ms(400)), None);
    // Later keys are typing while it is held: they never cancel.
    assert_eq!(l.key(ralt, true, ms(1000)), Some(Hit::Down));
    assert_eq!(l.key(shift, true, ms(1500)), None);
    assert_eq!(l.key(shift, false, ms(1550)), None);
    assert_eq!(l.key(ralt, false, ms(2000)), Some(Hit::Up));
  }

  #[test]
  fn escape_is_taken_only_while_on() {
    let t0 = Instant::now();
    let (esc, ctrl, shift, win, a) =
      (VK_ESCAPE as u32, VK_LCONTROL as u32, VK_LSHIFT as u32, VK_LWIN as u32, 0x41);
    let mut l = Lone::default();
    let taken = Some(Some(Hit::Escape));
    assert_eq!(l.escape(false, esc, true), None);
    assert_eq!(l.escape(false, esc, false), None);
    assert_eq!(l.escape(true, esc, true), taken);
    assert_eq!(l.escape(true, esc, true), Some(None), "its autorepeat");
    assert_eq!(l.escape(false, esc, false), Some(None), "its release, once off");
    // Held since before it was on: the app's, repeats and release too.
    assert_eq!(l.escape(false, esc, true), None);
    assert_eq!(l.escape(true, esc, true), None);
    assert_eq!(l.escape(true, esc, false), None);
    assert_eq!(l.escape(true, a, true), None, "other keys pass");
    // Ctrl+Esc (Start), Win+Esc are not Escape; Shift+Esc is.
    for m in [ctrl, win] {
      l.key(m, true, t0);
      assert_eq!(l.escape(true, esc, true), None, "{m:#x}");
      assert_eq!(l.escape(true, esc, false), None);
      l.key(m, false, t0);
    }

    l.key(shift, true, t0);
    assert_eq!(l.escape(true, esc, true), taken);
    assert_eq!(l.escape(true, esc, false), Some(None));
    // Another hotkey meanwhile: a taken press stays taken, repeats and release.
    assert_eq!(l.escape(true, esc, true), taken);
    l.watch(VK_RMENU as u32);
    assert_eq!(l.escape(true, esc, true), Some(None));
    assert_eq!(l.escape(false, esc, false), Some(None));
  }

  #[test]
  fn escape_with_a_modifier_hotkey_held() {
    let t0 = Instant::now();
    let ms = |n| t0 + Duration::from_millis(n);
    let (ralt, lalt, esc) = (VK_RMENU as u32, VK_LMENU as u32, VK_ESCAPE as u32);
    let mut l = Lone { key: ralt, ..Lone::default() };
    let mut judge = |vk, down, at| l.judge(true, vk, down, ms(at));
    // Right after the press: the user's Alt+Esc, which aborts the press and passes.
    assert_eq!(judge(ralt, true, 0), (Some(Hit::Down), false));
    assert_eq!(judge(esc, true, 100), (Some(Hit::Aborted), false));
    assert_eq!(judge(esc, true, 130), (None, false), "its autorepeat is the app's too");
    assert_eq!(judge(esc, false, 150), (None, false));
    assert_eq!(judge(ralt, false, 200), (None, false), "no release owed");
    // Held past the chord window: Escape is taken, the hotkey's own Alt notwithstanding.
    assert_eq!(judge(ralt, true, 1000), (Some(Hit::Down), false));
    assert_eq!(judge(esc, true, 2000), (Some(Hit::Escape), true));
    assert_eq!(judge(esc, true, 2030), (None, true));
    assert_eq!(judge(esc, false, 2050), (None, true));
    assert_eq!(judge(ralt, false, 2100), (Some(Hit::Up), false));
    // Left Alt is another key: Alt+Esc stays the app's.
    assert_eq!(judge(lalt, true, 3000), (None, false));
    assert_eq!(judge(esc, true, 4000), (None, false));
  }
}
