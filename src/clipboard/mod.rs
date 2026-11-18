// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The clipboard: what text edits copy to and paste from ([Clipboard], an in-memory one for
//! tests), and the system clipboard outside a window ([SystemClipboard]): plain text as each window
//! has it, and for a paste of the app's own into another app, the whole clipboard taken and put
//! back ([Snapshot]) around text that tells when it is read ([SystemClipboard::offer]).
//!
//! A paste goes: [SystemClipboard::snapshot], [SystemClipboard::offer] the text, post the paste
//! shortcut ([crate::keys::post]), and once the text was read (or after a while) put the snapshot
//! back with [SystemClipboard::restore] if the clipboard still [holds](SystemClipboard::holds) the
//! offer: what the user copied meanwhile stays. [crate::paste::text] does it all.
//!
//! Per OS (all called on the main thread, while the app runs on Windows and Linux): macOS the
//! general `NSPasteboard`, read off the main thread within a second unless it holds the app's own
//! promise, every item with every type (UTIs), its `changeCount`, and an offer as a promise
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

mod system;

pub use system::{Offer, Snapshot, SystemClipboard};

pub trait Clipboard {
  fn read(&self) -> Option<String>;
  fn write(&mut self, s: &str);

  /// Whether it holds text, without reading it (macOS asks the user before an app reads what
  /// another one copied): an edit menu's Paste.
  fn has_text(&self) -> bool {
    self.read().is_some_and(|s| !s.is_empty())
  }
}

/// An in-memory clipboard: the harness uses it so tests never touch the real pasteboard.
#[derive(Default)]
pub struct MemClipboard(pub Option<String>);

impl Clipboard for MemClipboard {
  fn read(&self) -> Option<String> {
    self.0.clone()
  }

  fn write(&mut self, s: &str) {
    self.0 = Some(s.to_owned());
  }
}

/// An in-memory one, until the runtime puts its own in.
impl Default for Box<dyn Clipboard> {
  fn default() -> Self {
    Box::new(MemClipboard::default())
  }
}
