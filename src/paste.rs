// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A paste of the app's own text where the user types in another app, as a dictation app does:
//! the text goes on the clipboard as an offer that tells when it is read
//! ([SystemClipboard::offer]), the paste shortcut goes to the app in focus ([crate::keys::post]),
//! and the clipboard the user had comes back [RESTORE] after the read (some apps read more than
//! once), or [CAP] after the paste when nothing reads it; only while the clipboard still holds the
//! text: what the user copied meanwhile stays. A restore still waiting runs when the app quits.
//! Ported from wavo-app's `src/app.rs` (same author, MIT).
//!
//! The shortcut is ⌘V on macOS and Ctrl+V elsewhere, Ctrl+Shift+V into a terminal on Linux (Ctrl+V
//! is the shell's quoted insert there), known by its `/proc/<pid>/comm` where the focus is known
//! (X11). Posting it needs Accessibility on macOS; on Wayland the first paste starts the
//! RemoteDesktop portal's session (unless [crate::keys::ready]), whose dialog waits for the user's
//! answer, up to 60 s: [text] returns after it, so a caller off the main thread
//! ([crate::Proxy::call]) waits [PORTAL_WAIT] there.
//!
//! ```no_run
//! use nimbo::paste::{self, Paste};
//!
//! // Where the user typed when they asked (a dictation's start)...
//! let target = nimbo::focus::current();
//! // ...and later, on the main thread, the text goes there if the focus is still there.
//! if let Err(e) = paste::text("Hello", Paste { restore: true, target }) {
//!   eprintln!("not pasted: {e}");
//! }
//! ```

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::clipboard::{Offer, Snapshot, SystemClipboard};
use crate::focus::{self, Focused};
use crate::keys::{CMD, Key, Shortcut};
use crate::{Clipboard, Main, Modifiers, permissions};

/// How long after the offer's read the clipboard comes back: some apps read it more than once.
pub const RESTORE: Duration = Duration::from_millis(500);
/// How long after the paste the clipboard comes back when nothing reads the offer.
pub const CAP: Duration = Duration::from_secs(8);
/// How long a caller off the main thread waits for [text] on Wayland ([crate::focus::hidden]),
/// where the first paste waits for the user's answer to the desktop's dialog (nimbo gives it 60 s).
pub const PORTAL_WAIT: Duration = Duration::from_secs(65);

const PASTE: Shortcut = Shortcut { key: Key::Char('v'), mods: CMD };

/// Linux terminals that paste with Ctrl+Shift+V, by `/proc/<pid>/comm`, which keeps 15 bytes:
/// gnome-terminal-server is `gnome-terminal-`.
#[rustfmt::skip]
const TERMINALS: [&str; 20] = [
  "gnome-terminal-", "kgx", "ptyxis", "konsole", "kitty", "alacritty", "wezterm-gui", "ghostty",
  "terminator", "tilix", "xfce4-terminal", "mate-terminal", "lxterminal", "qterminal", "foot",
  "terminology", "guake", "tilda", "blackbox", "st",
];

/// How [text] pastes.
#[derive(Clone, Debug, PartialEq)]
pub struct Paste {
  /// The clipboard the user had comes back after the paste; otherwise the text stays on it, as
  /// if copied.
  pub restore: bool,
  /// Where the text goes, as [focus::current] found it when the user asked: the paste is refused
  /// when the focus moved from it since. `None` (not known) refuses it, unless the OS hides the
  /// focus ([focus::hidden]: Wayland): then it goes wherever the focus is.
  pub target: Option<Focused>,
}

/// Why [text] did not paste.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotPasted {
  /// Called off the main thread.
  NotMain,
  /// The app in focus is not known: when the user asked ([Paste::target] `None`), or now.
  Unknown,
  /// The focus moved from the target to another app, or another window of it.
  FocusMoved { from: String, to: String },
  /// A password field has the focus.
  SecureField,
  /// The app in focus did not tell `what` (its focused window or element), as it did before or
  /// must.
  Unanswered { app: String, what: &'static str, why: String },
  /// No Accessibility access (macOS): the shortcut can't be posted.
  NoAccess,
  /// The clipboard can't be reached.
  NoClipboard,
  /// The shortcut (`key`, its label) was not posted.
  NotSent { key: String, why: String },
}

impl std::fmt::Display for NotPasted {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    match self {
      Self::NotMain => write!(f, "a paste runs on the main thread"),
      Self::Unknown => write!(f, "the app in focus is unknown"),
      Self::FocusMoved { from, to } => write!(f, "the focus moved from {from} to {to}"),
      Self::SecureField => write!(f, "the focused field is a password field"),
      Self::Unanswered { app, what, why } => write!(f, "{app} did not tell its {what} ({why})"),
      Self::NoAccess => write!(f, "no Accessibility access: {} can't be sent", PASTE.label()),
      Self::NoClipboard => write!(f, "the clipboard can't be reached"),
      Self::NotSent { key, why } => write!(f, "{key} was not sent: {why}"),
    }
  }
}

impl std::error::Error for NotPasted {}

/// Pastes `text` where the user types (see the module docs), on the main thread while the app
/// runs ([NotPasted::NotMain] elsewhere). Refused before the clipboard changes when the focus is
/// not where `opts` says, or it is a password field (or Secure Input is on and the app in focus
/// can't tell).
pub fn text(text: &str, opts: Paste) -> Result<(), NotPasted> {
  if !crate::platform::shell::is_main() {
    return Err(NotPasted::NotMain);
  }

  paste(SYSTEM, text, opts)
}

/// The paste's clipboard comes back at quit: an internal hook of [crate::shell]'s end.
pub(crate) fn finish() {
  settle(SYSTEM, None);
}

type OnRead = Box<dyn FnMut()>;

/// What a paste reaches outside the app: the system's, or a test's.
#[derive(Clone, Copy)]
struct Sys {
  /// The clipboard's contents, `None` also where it may not be read without asking.
  snapshot: fn() -> Option<Snapshot>,
  offer: fn(&str, Option<OnRead>) -> Option<Offer>,
  write: fn(&str),
  /// Puts the snapshot back if the clipboard still holds the offer.
  give_back: fn(Offer, Snapshot),
  post: fn(Shortcut) -> Result<(), String>,
  focus: fn() -> Option<Focused>,
  hidden: fn() -> bool,
  accessibility: fn() -> bool,
  secure_input: fn() -> bool,
  /// Runs a closure on the main thread after a delay.
  after: fn(Duration, Box<dyn FnOnce() + Send>),
}

const SYSTEM: Sys = Sys {
  snapshot: || SystemClipboard.readable().then(|| SystemClipboard.snapshot()).flatten(),
  offer: |text, on_read| SystemClipboard.offer(text, on_read),
  write: |text| SystemClipboard.write(text),
  give_back: |offer, old| _ = SystemClipboard.holds(offer) && SystemClipboard.restore(old),
  post: crate::keys::post,
  focus: focus::current,
  hidden: focus::hidden,
  accessibility: || permissions::accessibility(false),
  secure_input: permissions::secure_input,
  after: |delay, f| Main::here().proxy().after(delay, move |_| f()),
};

thread_local! {
  /// The paste whose clipboard has yet to come back: its id, offer and the snapshot to put back.
  static PENDING: RefCell<Option<(u64, Offer, Snapshot)>> = const { RefCell::new(None) };
}

fn paste(sys: Sys, text: &str, Paste { restore, target }: Paste) -> Result<(), NotPasted> {
  static NEXT: AtomicU64 = AtomicU64::new(1);
  if target.is_none() && !(sys.hidden)() {
    return Err(NotPasted::Unknown);
  }

  if !(sys.accessibility)() {
    return Err(NotPasted::NoAccess);
  }

  // The user's clipboard, not the text of a paste still waiting for its restore.
  settle(sys, None);
  // Nothing of it could be read (Paste from Other Apps not allowed): a restore would empty it.
  let snapshot = restore.then(sys.snapshot).flatten().filter(|s| !s.unreadable());

  // Right before the paste: the snapshot may take a while.
  if let Some(target) = &target {
    check_focus(sys, target)?;
  }

  let id = NEXT.fetch_add(1, Ordering::Relaxed);
  let after = move |delay| (sys.after)(delay, Box::new(move || settle(sys, Some(id))));
  match snapshot {
    Some(snapshot) => {
      let offer = (sys.offer)(text, Some(Box::new(move || after(RESTORE))));
      let offer = offer.ok_or(NotPasted::NoClipboard)?;
      PENDING.set(Some((id, offer, snapshot)));
      after(CAP);
    }
    None if !restore => (sys.write)(text),
    // Nothing to restore: the text stays, marked for clipboard managers to skip.
    None => _ = (sys.offer)(text, None).ok_or(NotPasted::NoClipboard)?,
  }

  let key = paste_key(&target);
  (sys.post)(key).map_err(|why| {
    settle(sys, Some(id));
    NotPasted::NotSent { key: key.label(), why }
  })
}

/// Whether the paste may go where `target` was: the focus is still there and not in a password
/// field.
fn check_focus(sys: Sys, target: &Focused) -> Result<(), NotPasted> {
  let now = (sys.focus)().ok_or(NotPasted::Unknown)?;
  if target.moved(&now) {
    return Err(NotPasted::FocusMoved { from: target.name.clone(), to: now.name });
  }

  // The app told its window at the start: it must tell it now too. An app that answers at all
  // must tell its focused element too; one that never did can't be checked, unless Secure Input is
  // on: then it may well be a password field.
  let known = target.window.is_ok();
  let (what, why) = match (&now.window, &now.secure) {
    (Err(why), _) if known => ("focused window", why),
    (_, Ok(true)) => return Err(NotPasted::SecureField),
    (_, Err(why)) if known || (sys.secure_input)() => ("focused element", why),
    _ => return Ok(()),
  };

  Err(NotPasted::Unanswered { app: now.name.clone(), what, why: why.clone() })
}

/// The shortcut that pastes into `target`.
fn paste_key(target: &Option<Focused>) -> Shortcut {
  let terminal = target.as_ref().is_some_and(|t| TERMINALS.contains(&t.name.as_str()));
  match cfg!(target_os = "linux") && terminal {
    true => Shortcut { mods: PASTE.mods | Modifiers::SHIFT, ..PASTE },
    false => PASTE,
  }
}

/// The clipboard of paste `id` (any for `None`) comes back if it still waits, and only while the
/// clipboard holds its offer: what was copied since stays.
fn settle(sys: Sys, id: Option<u64>) {
  let pending = PENDING.with_borrow_mut(|p| p.take_if(|p| id.is_none_or(|id| p.0 == id)));
  if let Some((_, offer, snapshot)) = pending {
    (sys.give_back)(offer, snapshot);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  type Timer = (Duration, Box<dyn FnOnce() + Send>);

  /// The clipboard (its text, and the offer it holds), the app in focus, the keys posted and
  /// the timers, on the test's thread.
  #[derive(Default)]
  struct World {
    clip: String,
    holder: Option<u64>,
    offers: u64,
    on_read: Option<Box<dyn FnMut()>>,
    restored: usize,
    posted: Vec<Shortcut>,
    post_error: Option<String>,
    focus: Option<Focused>,
    hidden: bool,
    denied: bool,
    timers: Vec<Timer>,
  }

  thread_local! {
    static WORLD: RefCell<World> = RefCell::default();
  }

  fn world<R>(f: impl FnOnce(&mut World) -> R) -> R {
    WORLD.with_borrow_mut(f)
  }

  fn write(text: &str) {
    world(|w| (w.clip, w.holder, w.on_read) = (text.into(), None, None));
  }

  const FAKE: Sys = Sys {
    snapshot: || {
      let clip = world(|w| w.clip.clone().into_bytes());
      Some(Snapshot { items: vec![vec![("text".into(), clip)]], missing: 0 })
    },
    offer: |text, on_read| {
      world(|w| {
        w.offers += 1;
        (w.clip, w.holder, w.on_read) = (text.into(), Some(w.offers), on_read);
        Some(Offer(w.offers))
      })
    },
    write,
    give_back: |offer, snapshot| {
      if world(|w| w.holder == Some(offer.0)) {
        write(&String::from_utf8(snapshot.items[0][0].1.clone()).unwrap());
        world(|w| w.restored += 1);
      }
    },
    post: |key| {
      world(|w| match w.post_error.clone() {
        Some(e) => Err(e),
        None => {
          let _: () = w.posted.push(key);
          Ok(())
        }
      })
    },
    focus: || world(|w| w.focus.clone()),
    hidden: || world(|w| w.hidden),
    accessibility: || world(|w| !w.denied),
    secure_input: || false,
    after: |delay, f| world(|w| w.timers.push((delay, f))),
  };

  fn app(pid: u32, name: &str) -> Focused {
    Focused { pid, name: name.into(), window: Ok(None), secure: Ok(false) }
  }

  /// A clipboard holding "mine", `name` in focus.
  fn setup(name: &str) -> Paste {
    PENDING.take();
    world(|w| *w = World { clip: "mine".into(), focus: Some(app(7, name)), ..World::default() });
    Paste { restore: true, target: Some(app(7, name)) }
  }

  /// Runs the timers of `delay` due now.
  fn fire(delay: Duration) {
    let (due, rest) =
      world(|w| std::mem::take(&mut w.timers)).into_iter().partition(|t| t.0 == delay);
    world(|w| w.timers = rest);
    for (_, f) in due {
      f();
    }
  }

  /// An app reads the offer.
  fn read() {
    let mut on_read = world(|w| w.on_read.take()).unwrap();
    on_read();
    world(|w| w.on_read = Some(on_read));
  }

  fn clip() -> (String, usize) {
    world(|w| (w.clip.clone(), w.restored))
  }

  #[test]
  fn offers_the_text_and_posts_the_shortcut() {
    let opts = setup("TextEdit");
    assert_eq!(paste(FAKE, "hello", opts), Ok(()));
    assert_eq!(
      world(|w| (w.clip.clone(), w.holder, w.posted.clone())),
      ("hello".into(), Some(1), vec![PASTE])
    );
    let mods = if cfg!(target_os = "macos") { Modifiers::META } else { Modifiers::CONTROL };
    assert_eq!(PASTE, Shortcut { key: Key::Char('v'), mods });
  }

  #[test]
  fn restores_after_a_read() {
    let opts = setup("TextEdit");
    paste(FAKE, "hello", opts).unwrap();
    read();
    read();
    fire(CAP);
    assert_eq!(clip(), ("mine".into(), 1), "the cap comes first here");

    let opts = setup("TextEdit");
    paste(FAKE, "hello", opts).unwrap();
    read();
    assert_eq!(clip(), ("hello".into(), 0), "not at the read itself");
    fire(RESTORE);
    assert_eq!(clip(), ("mine".into(), 1));
    fire(CAP);
    assert_eq!(clip(), ("mine".into(), 1), "once");
  }

  #[test]
  fn restores_after_the_cap_when_nothing_reads() {
    let opts = setup("TextEdit");
    paste(FAKE, "hello", opts).unwrap();
    fire(RESTORE);
    assert_eq!(clip(), ("hello".into(), 0));
    fire(CAP);
    assert_eq!(clip(), ("mine".into(), 1));
  }

  #[test]
  fn what_was_copied_meanwhile_stays() {
    let opts = setup("TextEdit");
    paste(FAKE, "hello", opts).unwrap();
    write("copied");
    fire(CAP);
    assert_eq!(clip(), ("copied".into(), 0));
  }

  #[test]
  fn a_second_paste_snapshots_the_users_clipboard() {
    let opts = setup("TextEdit");
    paste(FAKE, "one", opts.clone()).unwrap();
    paste(FAKE, "two", opts).unwrap();
    assert_eq!(clip(), ("two".into(), 1), "the first one's restored first");
    fire(CAP);
    assert_eq!(clip(), ("mine".into(), 2));
  }

  #[test]
  fn kept_without_restore() {
    let opts = setup("TextEdit");
    paste(FAKE, "hello", Paste { restore: false, ..opts }).unwrap();
    assert_eq!(world(|w| (w.clip.clone(), w.holder, w.timers.len())), ("hello".into(), None, 0));
  }

  #[test]
  fn refused_where_the_focus_is_not() {
    let refused = |opts: Paste| {
      let e = paste(FAKE, "hello", opts).unwrap_err();
      assert_eq!(world(|w| (w.clip.clone(), w.posted.len())), ("mine".into(), 0), "{e}");
      e
    };
    let opts = setup("TextEdit");
    world(|w| w.focus = Some(app(8, "Mail")));
    let moved = NotPasted::FocusMoved { from: "TextEdit".into(), to: "Mail".into() };
    assert_eq!(refused(opts), moved);
    assert_eq!(moved.to_string(), "the focus moved from TextEdit to Mail");

    let opts = setup("TextEdit");
    world(|w| w.focus.as_mut().unwrap().secure = Ok(true));
    assert_eq!(refused(opts), NotPasted::SecureField);

    let opts = setup("TextEdit");
    world(|w| w.focus.as_mut().unwrap().window = Err("busy".into()));
    let e = refused(opts);
    assert_eq!(e.to_string(), "TextEdit did not tell its focused window (busy)");

    let opts = setup("TextEdit");
    world(|w| w.focus = None);
    assert_eq!(refused(opts), NotPasted::Unknown);

    let opts = setup("TextEdit");
    world(|w| w.denied = true);
    assert_eq!(refused(opts), NotPasted::NoAccess);

    setup("TextEdit");
    assert_eq!(refused(Paste { restore: true, target: None }), NotPasted::Unknown);
  }

  #[test]
  fn goes_where_the_focus_is_when_the_os_hides_it() {
    setup("TextEdit");
    world(|w| (w.focus, w.hidden) = (None, true));
    assert_eq!(paste(FAKE, "hello", Paste { restore: true, target: None }), Ok(()));
    assert_eq!(world(|w| w.posted.clone()), [PASTE]);
  }

  #[test]
  fn restored_at_once_when_the_shortcut_fails() {
    let opts = setup("TextEdit");
    world(|w| w.post_error = Some("no XTest".into()));
    let e = paste(FAKE, "hello", opts).unwrap_err();
    assert_eq!(e, NotPasted::NotSent { key: PASTE.label(), why: "no XTest".into() });
    assert_eq!(clip(), ("mine".into(), 1));
  }

  #[test]
  fn a_pending_restore_runs_at_quit() {
    let opts = setup("TextEdit");
    paste(FAKE, "hello", opts).unwrap();
    settle(FAKE, None);
    assert_eq!(clip(), ("mine".into(), 1));
    fire(CAP);
    assert_eq!(clip(), ("mine".into(), 1));
  }

  #[test]
  fn terminals_paste_with_shift_on_linux() {
    let shift = cfg!(target_os = "linux");
    let key = |name: &str| paste_key(&Some(app(1, name))).mods.contains(Modifiers::SHIFT);
    assert_eq!(key("gnome-terminal-"), shift);
    assert_eq!(key("konsole"), shift);
    assert!(!key("firefox"));
    assert_eq!(paste_key(&None), PASTE);
  }

  /// The system's paste refuses a thread other than the main one, before it reaches anything.
  #[test]
  fn refused_off_the_main_thread() {
    let opts = Paste { restore: true, target: None };
    let got = std::thread::spawn(move || text("hello", opts)).join().unwrap();
    assert_eq!(got, Err(NotPasted::NotMain));
  }
}
