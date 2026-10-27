// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The system clipboard outside a window: plain text as each window has it, and for a paste of
//! the app's own into another app, the whole clipboard taken and put back ([Snapshot]) around
//! text that tells when it is read ([SystemClipboard::offer]).
//!
//! A paste goes: [SystemClipboard::snapshot], [SystemClipboard::offer] the text, post the paste
//! shortcut ([crate::keys::post]), and once the text was read (or after a while) put the snapshot
//! back with [SystemClipboard::restore] if the clipboard still [holds](SystemClipboard::holds) the
//! offer: what the user copied meanwhile stays. [crate::paste::text] does it all.
//!
//! Per OS (all on the main thread, while the app runs on Windows and Linux): macOS the general
//! `NSPasteboard`, every item with every type (UTIs), its `changeCount`, and an offer as a promise
//! its owner provides when read. Windows the clipboard through the app's hidden window, its
//! formats as one item (format names, `#<n>` for the standard ones; GDI handles such as
//! `CF_BITMAP` aren't bytes and are left out: `CF_DIB` brings them back), the sequence number, and
//! an offer by delayed rendering (`WM_RENDERFORMAT`). Linux GTK's CLIPBOARD selection: its targets
//! as one item, no change count (X11 and Wayland keep none), an offer served by GTK, which
//! asks for it at each read, and a restore served by the app, kept after it quits only where a
//! clipboard manager takes it. On Wayland, where GTK can't set the clipboard without the keyboard
//! focus, the RemoteDesktop portal's clipboard does the same (its types MIME types) once the
//! desktop gave it, its session started by the first call on Wayland (the desktop asks the user
//! once).

use crate::app::Clipboard;
use crate::platform::{clipboard, paste as imp};

/// The system clipboard, plain text, as each window's runtime has it: for code outside a window
/// (a tray menu's Copy, a paste of the app's own). On the main thread, while the app runs
/// ([crate::app()]): Windows writes through the app's hidden window, Linux through GTK, which also
/// serves the text to other apps; with no app running it reads nothing and writes nothing there.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClipboard;

impl Clipboard for SystemClipboard {
  fn read(&self) -> Option<String> {
    clipboard()?.read()
  }

  fn write(&mut self, s: &str) {
    if let Some(mut c) = clipboard() {
      c.write(s);
    }
  }
}

/// Every item on the clipboard with the bytes of each of its types, to put back as it was.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
  /// Each item's types (the OS's names: see the module docs) with their bytes.
  pub items: Vec<Vec<(String, Vec<u8>)>>,
  /// Types whose bytes could not be read: the owner gave none, the OS did not let the app read
  /// them ([SystemClipboard::readable]), or they are not bytes (Windows' GDI handles).
  pub missing: usize,
}

impl Snapshot {
  /// The bytes of every type of every item.
  pub fn bytes(&self) -> usize {
    self.items.iter().flatten().map(|(_, data)| data.len()).sum()
  }

  /// The clipboard had contents but none could be read: restoring this would empty it.
  pub fn unreadable(&self) -> bool {
    self.items.is_empty() && self.missing > 0
  }
}

/// Text [SystemClipboard::offer] put on the clipboard, for [SystemClipboard::holds].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offer(pub(crate) u64);

impl SystemClipboard {
  /// A number that changes whenever any app writes to the clipboard (macOS `changeCount`,
  /// Windows' sequence number), to tell whether it changed between two looks; `None` where the
  /// OS keeps none (Linux) or the app isn't running (Windows).
  pub fn change_count(&self) -> Option<u64> {
    imp::change_count()
  }

  /// The app may read what other apps put on the clipboard without the OS asking the user each
  /// time: on macOS 15.4+ unless Privacy & Security › Paste from Other Apps has it ask or deny
  /// (`NSPasteboard.accessBehavior`, which asks nothing to read; never decided, the first read
  /// asks once, and the answer stays); always elsewhere ([crate::permissions::Pane::Pasteboard]).
  pub fn readable(&self) -> bool {
    imp::readable()
  }

  /// The clipboard's contents now; it may take a while (an app that provides its data lazily).
  /// `None` where it can't be reached (no app running on Windows and Linux) or, on macOS, when it
  /// takes over a second (an app that promised its data and doesn't answer); an owner that does
  /// not answer counts as [missing](Snapshot::missing) elsewhere.
  pub fn snapshot(&self) -> Option<Snapshot> {
    imp::snapshot()
  }

  /// Replaces the clipboard's contents with `snapshot`'s (an empty one empties it); false if the
  /// OS did not take them all. Linux keeps serving them from the app.
  pub fn restore(&self, snapshot: Snapshot) -> bool {
    imp::restore(snapshot)
  }

  /// Puts `text` on the clipboard for a paste of the app's own, marked so clipboard managers and
  /// history skip it (macOS `org.nspasteboard.TransientType` and `ConcealedType`; Windows
  /// `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` and
  /// `CanUploadToCloudClipboard` off; Linux `x-kde-passwordManagerHint`). With `on_read` the text
  /// is handed out only when an app reads it, and `on_read` runs on the main thread at each read
  /// the OS asks the app for (macOS and Windows ask once and keep the text; GTK asks each time),
  /// any reader's (a clipboard manager that ignores the markers too), while the reader waits:
  /// post what it does to the clipboard ([crate::Proxy]), as the reader holds it open on
  /// Windows. `None` where it can't be reached.
  pub fn offer(&self, text: &str, on_read: Option<Box<dyn FnMut()>>) -> Option<Offer> {
    imp::offer(text, on_read).map(Offer)
  }

  /// The clipboard still holds `offer`: nothing was written since, by any app or this one.
  pub fn holds(&self, offer: Offer) -> bool {
    imp::holds(offer.0)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn snapshots_count_their_bytes() {
    let item = |types: &[(&str, &[u8])]| -> Vec<(String, Vec<u8>)> {
      types.iter().map(|(t, d)| (t.to_string(), d.to_vec())).collect()
    };
    let snapshot = Snapshot {
      items: vec![item(&[("a", b"xyz"), ("b", b"")]), item(&[("c", b"12")])],
      missing: 1,
    };
    assert_eq!(snapshot.bytes(), 5);
    assert!(!snapshot.unreadable());
    assert!(Snapshot { missing: 2, ..Snapshot::default() }.unreadable());
    assert!(!Snapshot::default().unreadable(), "an empty clipboard comes back empty");
  }
}
