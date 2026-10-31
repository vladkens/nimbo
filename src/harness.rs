// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The headless harness: runs an app with no window on the same renderer, offscreen, at a
//! given size and scale, on virtual time and an in-memory clipboard. Scripted input drives it;
//! it renders PNG shots ([crate::png]).

use std::path::Path;
use std::time::Duration;

use keyboard_types::{Code, Modifiers};

use crate::app::{App, Clipboard, Clock, Event, Ime, MemClipboard, MouseButton, Runtime, Waker};
use crate::cpu;
pub use crate::cpu::Pixels;
use crate::file_dialog::FileDialog;
use crate::keys::{self, Key, KeyEvent, Os};
use crate::menu::Menu;
use crate::scene::{Point, Rect, Size};
use crate::text::TextSystem;
use crate::text::edit::{Editor, utf16_len};
use crate::ui::{A11yTree, Action, Cursor, Id, Kind, Role, Scroll, TextInput};

/// A display refresh at 120 Hz, the step of [Harness::wait].
pub const TICK: Duration = Duration::from_nanos(8_333_333);

/// Runs an app headless: scripted input in, app state, the last frame's elements and rendered
/// pixels out, all from the public API, so the frame's internals can change under the tests.
pub struct Harness<A> {
  pub(crate) rt: Runtime<A>,
  pub(crate) cpu: cpu::Renderer,
  /// Made at the first [Self::render_metal], then synced with every frame built.
  #[cfg(all(feature = "metal", target_os = "macos"))]
  metal: Option<crate::platform::macos::metal::render::Renderer>,
}

impl<A: App> Harness<A> {
  pub fn new(app: A, size: Size, scale: f32) -> Self {
    let clock = Clock::Virtual(Duration::ZERO);
    let rt = Runtime::new(app, size, scale, clock, Box::new(MemClipboard::default()));
    Self {
      rt,
      cpu: cpu::Renderer::new(),
      #[cfg(all(feature = "metal", target_os = "macos"))]
      metal: None,
    }
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
  /// Only those: `CMD`/`CTRL`, `KeyEvent::is`, `Shortcut::parse` and a list's Cmd-click still
  /// follow the host.
  pub fn set_os(&mut self, os: Os) {
    self.rt.ui.os = os;
  }

  /// Plays the window's title bar over the content ([crate::Config::unified]): its height and
  /// the window's title, which [crate::Ui::titlebar] reports; the next frame shows it.
  pub fn set_titlebar(&mut self, bar: Option<(f32, &'static str)>) {
    self.rt.set_titlebar(bar);
    self.rt.redraw();
  }

  /// Renders transparent where the app draws nothing, as an overlay's window does
  /// ([crate::Config::overlay]), instead of on black.
  pub fn set_transparent(&mut self, yes: bool) {
    self.cpu.transparent = yes;
  }

  /// Gives the element `id` the key focus, or takes it from all (`None`); the next frame shows
  /// it.
  pub fn set_focus(&mut self, id: Option<Id>) {
    self.rt.ui.set_focus(id);
    self.rt.redraw();
  }

  /// Resizes the view to `size` points at `scale`, as a window resized or moved to another
  /// screen does.
  pub fn resize(&mut self, size: Size, scale: f32) {
    self.rt.resize(size, scale);
  }

  /// Asks for a frame though nothing changed, as a window's expose does.
  pub fn redraw(&mut self) {
    self.rt.redraw();
  }

  /// Wakes the app from another thread, as [crate::Proxy] does: [Self::wait] builds the frame.
  pub fn waker(&self) -> &Waker {
    self.rt.waker()
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

  /// Plays a platform that shows context menus itself: they are asked for, not drawn, and
  /// [Harness::context_menu] answers them.
  pub fn native_menus(&mut self) {
    self.rt.set_native_menus(true);
  }

  /// Answers the context menu the app asked for ([crate::Ui::context_menu]) as the person
  /// would, choosing the item of id `answer` (`None` dismisses it), and returns where and what
  /// it asked; `None` and no answer when it asked for none.
  pub fn context_menu(&mut self, answer: Option<&str>) -> Option<(Point, Menu)> {
    let (on, at, menu) = self.rt.take_context_menu()?;
    self.event(Event::Menu { on, item: answer.map(str::to_owned) });
    Some((at, menu))
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
  /// `switch "Open at login" off` ([Self::a11y_tree]).
  pub fn a11y(&self) -> String {
    self.a11y_tree().to_string()
  }

  /// The last frame as assistive technology sees it: its nodes, roles, names and values.
  pub fn a11y_tree(&self) -> A11yTree {
    self.rt.ui.a11y_tree()
  }

  /// The view's size in points: the config's, or the one the app or [Self::resize] set.
  pub fn size(&self) -> Size {
    self.rt.size
  }

  /// The pointer in points, `None` when it is outside the view.
  pub fn mouse(&self) -> Option<Point> {
    self.rt.mouse
  }

  /// The interactive elements under the pointer, innermost first.
  pub fn hover(&self) -> &[Id] {
    self.rt.ui.hover()
  }

  /// The pointer's cursor: the pressed or hovered element's.
  pub fn cursor(&self) -> Cursor {
    self.rt.cursor()
  }

  /// A scroll area's offset, viewport and content as of the last frame and the input since
  /// ([crate::Ui::scroll]): the offset includes wheel, keys and `scroll_to` that came after the
  /// frame, clamped to the last frame's content.
  pub fn scroll(&self, id: Id) -> Option<Scroll> {
    self.rt.ui.scroll(id)
  }

  /// Whether the menu of `id` is open.
  pub fn menu_open(&self, id: Id) -> bool {
    self.rt.ui.menu_open(id)
  }

  /// The elements the last frame built: a list builds only the rows in view.
  pub fn built(&self) -> usize {
    self.rt.ui.last.nodes.len()
  }

  /// The focused text field as an input method sees it ([crate::ui::TextInput]); `None` unless
  /// an editable field that takes input methods has the focus. Runs the view and the layout
  /// first when something changed since the last layout, as a window's input method query does.
  pub fn text_input(&mut self) -> Option<TextInput<'_>> {
    self.rt.text_input()
  }

  /// The clipboard the app reads and writes: in memory, never the system's.
  pub fn clipboard(&mut self) -> &mut dyn Clipboard {
    self.rt.clipboard()
  }

  /// The text system the frames shape and measure with, at the harness's scale.
  pub fn text_system(&mut self) -> &mut TextSystem {
    &mut self.rt.text
  }

  /// Whether the next display refresh would build a frame: something changed, an animation
  /// runs, a timer is due or a waker fired.
  pub fn needs_frame(&mut self) -> bool {
    self.rt.needs_frame()
  }

  /// Time until the next timer the app or a widget set (a caret's blink), if any.
  pub fn due_in(&self) -> Option<Duration> {
    self.rt.due_in()
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
    self.sync();
    if self.apply_resize() {
      again = self.rt.frame();
      self.sync();
    }

    again
  }

  /// Hands the frame's new glyphs and images to the renderers, the CPU's last: its sync drains
  /// them.
  fn sync(&mut self) {
    #[cfg(all(feature = "metal", target_os = "macos"))]
    if let Some(m) = &mut self.metal {
      let done = m.sync_pending(self.rt.text.pending(), &self.rt.images);
      done.unwrap_or_else(|e| panic!("nimbo: Metal sync: {e}"));
    }

    self.cpu.sync(&mut self.rt.text, &mut self.rt.images);
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

  /// The frame the last [Self::render] built, drawn by the Metal renderer as a window with
  /// `NIMBO_RENDERER=metal` draws it; `None` without Metal (no device, a failed frame), the
  /// reason on stderr. Made at the first call with the CPU renderer's glyphs and images, the
  /// renderer is then synced with every frame built.
  #[cfg(all(feature = "metal", target_os = "macos"))]
  pub fn render_metal(&mut self) -> Option<Image> {
    if self.metal.is_none() {
      let (u, i) = self.cpu.contents();
      let made = crate::platform::macos::metal::render::Renderer::new()
        .and_then(|mut m| m.sync_pending(&u, &i).map(|_| m));
      self.metal = made.map_err(|e| eprintln!("nimbo: no Metal renderer: {e}")).ok();
    }

    let m = self.metal.as_mut()?;
    m.transparent = self.cpu.transparent;
    let pixels = m.snapshot(&self.rt.scene, self.rt.size, self.rt.scale);
    let pixels = pixels.map_err(|e| eprintln!("nimbo: no Metal frame: {e}")).ok()?;
    Some(Image { scale: self.rt.scale, pixels })
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

  #[cfg(all(feature = "metal", target_os = "macos"))]
  #[test]
  fn metal_only_with_the_first_metal_frame() {
    use crate::platform::macos::metal::Gpu;

    let mut h = Harness::new(Lines(vec![("Hello".to_string(), 13.0)]), Size::new(200.0, 50.0), 2.0);
    h.render();
    assert!(!Gpu::made(), "a CPU frame made a Metal device");
    assert!(h.render_metal().is_some());
    assert!(Gpu::made());
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
