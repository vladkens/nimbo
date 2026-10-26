// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The headless harness: runs an app with no window on the same renderer, offscreen, at a
//! given size and scale, on virtual time and an in-memory clipboard. Scripted input drives it;
//! it renders PNG shots ([crate::png]).

use std::path::Path;
use std::time::Duration;

use keyboard_types::{Code, Modifiers};

use crate::app::{App, Clock, Event, Ime, MemClipboard, MouseButton, Runtime};
use crate::cpu::{self, Pixels};
use crate::file_dialog::FileDialog;
use crate::keys::{self, Key, KeyEvent, Os};
use crate::scene::{Point, Rect, Size};
use crate::text::edit::{Editor, utf16_len};
use crate::ui::{Action, Id, Kind, Role};

/// A display refresh at 120 Hz, the step of [Harness::wait].
pub const TICK: Duration = Duration::from_nanos(8_333_333);

pub struct Harness<A> {
  pub rt: Runtime<A>,
  pub cpu: cpu::Renderer,
}

impl<A: App> Harness<A> {
  pub fn new(app: A, size: Size, scale: f32) -> Self {
    let clock = Clock::Virtual(Duration::ZERO);
    let rt = Runtime::new(app, size, scale, clock, Box::new(MemClipboard::default()));
    Self { rt, cpu: cpu::Renderer::new() }
  }

  pub fn app(&self) -> &A {
    &self.rt.app
  }

  /// The app to change directly; the next [Self::frame] builds a frame.
  pub fn app_mut(&mut self) -> &mut A {
    self.rt.redraw();
    &mut self.rt.app
  }

  // MARK: Input

  /// Plays platform `os`'s keys from now on: what `cmd` in bindings is, and the commands keys
  /// nobody used run ([Os::command]); the host's by default.
  pub fn set_os(&mut self, os: Os) {
    self.rt.ui.os = os;
  }

  pub fn event(&mut self, ev: Event) -> bool {
    self.rt.event(ev)
  }

  /// Answers the file panel the app asked for ([crate::Ui::file_dialog]) as the person would,
  /// with `answer` (`None` cancels), and returns what it asked; `None` and no answer when it
  /// asked for none.
  pub fn file_dialog(&mut self, answer: Option<&Path>) -> Option<FileDialog> {
    let asked = self.rt.take_file_dialog()?;
    self.event(Event::FileDialog(answer.map(Path::to_path_buf)));
    Some(asked)
  }

  pub fn move_to(&mut self, x: f32, y: f32) -> bool {
    self.event(Event::MouseMove { pos: Point::new(x, y), mods: Modifiers::empty() })
  }

  /// A left click at (x, y): move, press, release. Returns whether any of them was used.
  pub fn click(&mut self, x: f32, y: f32) -> bool {
    let moved = self.move_to(x, y);
    let down = self.press(x, y);
    let up = self.release(x, y);
    moved | down | up
  }

  /// A right click at (x, y): move, press, release, as for a context menu. Returns whether any
  /// of them was used.
  pub fn right_click(&mut self, x: f32, y: f32) -> bool {
    let (pos, button, mods) = (Point::new(x, y), MouseButton::Right, Modifiers::empty());
    let moved = self.move_to(x, y);
    let down = self.event(Event::MouseDown { pos, button, clicks: 1, mods });
    moved | down | self.event(Event::MouseUp { pos, button, mods })
  }

  /// Presses the left button at (x, y).
  pub fn press(&mut self, x: f32, y: f32) -> bool {
    self.press_with(x, y, 1, Modifiers::empty())
  }

  /// Presses the left button at (x, y) as the `clicks`-th click in a row, with `mods` held.
  pub fn press_with(&mut self, x: f32, y: f32, clicks: u32, mods: Modifiers) -> bool {
    let (pos, button) = (Point::new(x, y), MouseButton::Left);
    self.event(Event::MouseDown { pos, button, clicks, mods })
  }

  /// Releases the left button at (x, y).
  pub fn release(&mut self, x: f32, y: f32) -> bool {
    self.event(Event::MouseUp {
      pos: Point::new(x, y),
      button: MouseButton::Left,
      mods: Modifiers::empty(),
    })
  }

  /// Clicks the centre of an element of the last frame; panics when it isn't there.
  pub fn click_id(&mut self, id: Id) -> bool {
    let r = self.rect(id).unwrap_or_else(|| panic!("no element {id:?}"));
    self.click(r.x + r.w / 2.0, r.y + r.h / 2.0)
  }

  /// Presses and releases `key` with `mods` as on a US keyboard (Shift-1 is `'!'`, Shift-A
  /// `'A'`); returns whether the press was used. Unused, it goes through text input as in a
  /// window: [Self::key_event].
  pub fn key(&mut self, key: impl Into<Key>, mods: Modifiers) -> bool {
    self.key_event(us_key(key.into(), mods))
  }

  /// Presses and releases a key as a window does: the key goes through bindings to the focused
  /// element and the app; unused, the platform's text input would make it text or a command,
  /// here [Os::command] of the platform played does. Returns whether any of it was used.
  pub fn key_event(&mut self, k: KeyEvent) -> bool {
    let mut used = self.event(Event::KeyDown(k));
    if !used {
      used = match (k.text, self.rt.ui.os.command(&k)) {
        (Some(c), _) => self.event(Event::Text(c.to_string())),
        (None, Some(cmd)) => self.event(Event::Command(cmd)),
        (None, None) => false,
      };
    }

    self.event(Event::KeyUp(KeyEvent { repeat: false, ..k }));
    used
  }

  /// Acts on `id` (its first node's role) as assistive technology would ([Event::A11y]), then
  /// lets two display refreshes pass, which build the frames that show it (a menu is placed in
  /// its second); returns whether it was taken, not refused.
  pub fn act(&mut self, id: Id, action: Action) -> bool {
    let role = self.rt.ui.a11y_tree().get(id).map_or(Role::Group, |n| n.props.role);
    let taken = self.event(Event::A11y { id, role, action });
    self.wait(2 * TICK);
    taken
  }

  /// Types `s` as text input would: one [Event::Text]. Returns whether it was used.
  pub fn type_text(&mut self, s: &str) -> bool {
    self.event(Event::Text(s.into()))
  }

  /// An input method marks `s` in the focused field (composing), its caret at the end, as
  /// `setMarkedText:selectedRange:replacementRange:` does.
  pub fn ime_mark(&mut self, s: &str) -> bool {
    let n = utf16_len(s);
    self.event(Event::Ime(Ime::Mark { text: s.into(), sel: n..n, replace: None }))
  }

  /// An input method commits `s` in place of the marked text (`insertText:replacementRange:`).
  pub fn ime_commit(&mut self, s: &str) -> bool {
    self.event(Event::Ime(Ime::Commit { text: s.into(), replace: None }))
  }

  /// The pointer leaves the view.
  pub fn leave(&mut self) -> bool {
    self.event(Event::MouseExit)
  }

  /// Moves virtual time on without building frames.
  pub fn advance(&mut self, dt: Duration) {
    self.rt.advance(dt);
  }

  /// Lets `dt` of virtual time pass as a window would: a display refresh every [TICK], each
  /// building a frame when one is needed (animations, momentum, timers). Returns the frames
  /// built.
  pub fn wait(&mut self, dt: Duration) -> usize {
    let (end, mut frames) = (self.rt.now() + dt, 0);
    while self.rt.now() < end {
      self.advance(TICK.min(end - self.rt.now()));
      frames += self.frame() as usize;
    }

    frames
  }

  /// The focused element.
  pub fn focused(&self) -> Option<Id> {
    self.rt.ui.focus()
  }

  /// An element's rect in the last frame built.
  pub fn rect(&self, id: Id) -> Option<Rect> {
    self.rt.ui.last.rect(id)
  }

  /// The text of a text element in the last frame built.
  pub fn text(&self, id: Id) -> Option<&str> {
    let t = &self.rt.ui.last;
    match t.get(id)?.kind {
      Kind::Text { range, .. } => Some(t.str(range.0, range.1)),
      _ => None,
    }
  }

  /// The editor of a text field or selectable text.
  pub fn editor(&self, id: Id) -> Option<&Editor> {
    self.rt.ui.editor(id)
  }

  /// The last frame as assistive technology sees it, one line per node indented by depth:
  /// `switch "Open at login" off` ([crate::ui::A11yTree]).
  pub fn a11y(&self) -> String {
    self.rt.ui.a11y_tree().to_string()
  }

  // MARK: Output

  /// Builds a frame if anything asked for one; returns whether it did. A window size the app
  /// asked for ([crate::Ui::set_window_size]) applies first, as a window would resize, and
  /// again after a frame that asked for one, which is then built again at the new size.
  pub fn frame(&mut self) -> bool {
    self.apply_resize();
    if !self.rt.needs_frame() {
      return false;
    }

    self.build();
    true
  }

  /// Builds a frame, and again at the size it asked for if it did; returns whether the last one
  /// dropped glyphs (a full atlas) and must be built again.
  fn build(&mut self) -> bool {
    let mut again = self.rt.frame();
    self.cpu.sync(&mut self.rt.text, &mut self.rt.images);
    if self.apply_resize() {
      again = self.rt.frame();
      self.cpu.sync(&mut self.rt.text, &mut self.rt.images);
    }

    again
  }

  /// Resizes the view to the size the app asked for, if it did; returns whether it did.
  fn apply_resize(&mut self) -> bool {
    let Some(size) = self.rt.take_resize() else { return false };
    let scale = self.rt.scale;
    self.rt.resize(size, scale);
    true
  }

  /// Renders the current state offscreen and reads the pixels back. A frame that dropped glyphs
  /// is built once more first, as a window's next refresh would (never more: an animation
  /// isn't run on).
  pub fn render(&mut self) -> Image {
    self.apply_resize();
    if self.rt.needs_frame() && self.build() {
      self.build();
    }

    let (size, scale) = (self.rt.size, self.rt.scale);
    let pixels = self.cpu.snapshot(&self.rt.scene, size, scale);
    Image { scale, pixels }
  }

  pub fn shot(&mut self, path: &Path) -> Result<(), String> {
    self.render().save_png(path)
  }
}

// MARK: US keyboard

/// The event AppKit reports for `key` with `mods` on a US keyboard. A character may be given
/// shifted or not: `Char('1')` or `Char('!')` with Shift is `'!'`.
pub fn us_key(key: Key, mods: Modifiers) -> KeyEvent {
  let (key, code, mods) = match key {
    Key::Char(c) => match keys::us_code(c) {
      Some((code, shifted)) => {
        let mods = if shifted { mods | Modifiers::SHIFT } else { mods };
        let c = keys::us_char(code, mods.shift()).unwrap_or(c);
        (Key::Char(c), code, mods)
      }
      None => (key, Code::Unidentified, mods),
    },
    k => (k, k.code().unwrap_or(Code::Unidentified), mods),
  };

  let text = match key {
    Key::Char(c) if (mods - Modifiers::SHIFT).is_empty() => Some(c),
    _ => None,
  };

  KeyEvent { key, text, code, mods, repeat: false }
}

/// Offscreen frame pixels with their backing scale.
pub struct Image {
  pub scale: f32,
  pub pixels: Pixels,
}

impl Image {
  /// RGBA at a point (in points, like events).
  pub fn pixel(&self, x: f32, y: f32) -> [u8; 4] {
    let px = ((x * self.scale) as usize).min(self.pixels.w - 1);
    let py = ((y * self.scale) as usize).min(self.pixels.h - 1);
    let i = (py * self.pixels.w + px) * 4;
    let [b, g, r, a] = self.pixels.data[i..i + 4].try_into().unwrap();
    [r, g, b, a]
  }

  /// Writes a PNG, tagged sRGB.
  pub fn save_png(&self, path: &Path) -> Result<(), String> {
    crate::png::write(path, &self.pixels)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{App, CMD, Font, Frame, NamedKey, Rect};

  /// Lines of text, each at its size.
  struct Lines(Vec<(String, f32)>);

  impl App for Lines {
    fn paint(&mut self, f: &mut Frame) {
      f.rect(Rect::new(0.0, 0.0, f.size.w, f.size.h), [1.0; 4], 0.0);
      let mut y = 0.0;
      for (s, size) in &self.0 {
        y += size * 1.2;
        f.text(0.0, y, s, Font::ui(*size), [0.0, 0.0, 0.0, 1.0]);
      }
    }
  }

  #[test]
  fn a_snapshot_has_glyphs_a_full_atlas_dropped() {
    let hello = || vec![("Hello, world".to_string(), 13.0)];
    let size = Size::new(4000.0, 900.0);
    // Earlier frames fill the atlas with large glyphs; the next one's don't fit until it resets.
    let abc: String = ('A'..='Z').chain('a'..='z').collect();
    let big = (0..5).map(|i| (abc.clone(), 100.0 + 20.0 * i as f32)).collect();
    let mut h = Harness::new(Lines(big), size, 2.0);
    h.render();
    h.rt.redraw();
    h.render();
    h.app_mut().0 = hello();
    h.rt.redraw();
    let img = h.render();
    let fresh = Harness::new(Lines(hello()), size, 2.0).render();
    assert!(img.pixels.data == fresh.pixels.data, "glyphs missing from the snapshot");
  }

  #[test]
  fn us_keyboard() {
    let ev = us_key(Key::Char('1'), Modifiers::SHIFT);
    let want = (Key::Char('!'), Some('!'), Code::Digit1, Modifiers::SHIFT);
    assert_eq!((ev.key, ev.text, ev.code, ev.mods), want);
    let ev = us_key(Key::Char('A'), Modifiers::empty());
    assert_eq!(
      (ev.key, ev.text, ev.code, ev.mods.shift()),
      (Key::Char('A'), Some('A'), Code::KeyA, true)
    );
    let ev = us_key(Key::Char('z'), CMD);
    assert_eq!((ev.key, ev.text, ev.code), (Key::Char('z'), None, Code::KeyZ));
    assert_eq!(us_key(Key::Char(' '), Modifiers::empty()).code, Code::Space);
    assert_eq!(us_key(Key::Named(NamedKey::Tab), Modifiers::SHIFT).code, Code::Tab);
    assert_eq!(us_key(Key::Char('é'), Modifiers::empty()).code, Code::Unidentified);
  }
}
