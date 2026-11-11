// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What the app asked of its window in an event or a frame, done after it, outside the
//! runtime's borrow: the file panel ([crate::Ui::file_dialog]), a context menu
//! ([crate::Ui::context_menu]) and a new content size ([crate::Ui::set_window_size]).

use objc2::DefinedClass;
use objc2_app_kit::NSEvent;
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::super::app::menu::popup;
use super::super::input::{button, flags_mods};
use super::super::{files, overlay};
use super::View;
use crate::app::Event;
use crate::scene::Point;

impl View {
  /// Shows the file panel the app asked for ([crate::Runtime::take_file_dialog]).
  pub(super) fn apply_file_dialog(&self) {
    let d = match self.ivars().state.try_borrow_mut().as_deref_mut() {
      Ok(Some(st)) => st.rt.take_file_dialog(),
      _ => None,
    };

    if let (Some(d), Some(window)) = (d, self.window()) {
      files::show(self, &window, d);
    }
  }

  /// Pops up the context menu the app asked for ([crate::app::Runtime::take_context_menu]) and
  /// sends the answer once it closes: frames go on meanwhile (the display link runs in every mode).
  /// The menu took the press's release: the app gets it after the menu.
  pub(super) fn show_menu(&self) {
    let want = match self.ivars().state.try_borrow_mut().as_deref_mut() {
      Ok(Some(st)) => st.rt.take_context_menu(),
      _ => None,
    };

    let Some((on, at, menu)) = want else { return };
    // The press at the menu's point; one at another is no context for it.
    let press = self.ivars().press.take().filter(|e| {
      let p = self.location(e);
      (p.x - at.x).abs().max((p.y - at.y).abs()) < 1.0
    });
    // The window lives while its menu is open, even if the app closes it meanwhile.
    let _window = self.window();
    let item = popup(self, press.as_deref(), at, &menu);
    // Any button the menu took the release of: the right one, the left of a Control-click.
    let held = self.ivars().held.get();
    let gone = held & !(NSEvent::pressedMouseButtons() as u64);
    if gone != 0 {
      let p = self.window().map_or(NSPoint::ZERO, |w| w.mouseLocationOutsideOfEventStream());
      let p = self.convertPoint_fromView(p, None);
      let (pos, mods) =
        (Point::new(p.x as f32, p.y as f32), flags_mods(NSEvent::modifierFlags_class()));
      self.ivars().held.set(held & !gone);
      for n in (0..64).filter(|n| gone >> n & 1 != 0) {
        self.send(Event::MouseUp { pos, button: button(n), mods });
      }
    }

    self.send(Event::Menu { on, item });
  }

  /// Resizes the window's content to the size the app asked for
  /// ([crate::app::Runtime::take_resize]), keeping its top edge where it is, as preferences windows
  /// do when their tab changes, or an overlay at its anchor on its screen. The
  /// least content size comes down with it; a fixed window stays fixed for the user. Outside the
  /// runtime's borrow: the new frame size draws at once ([Self::render]).
  pub(super) fn apply_resize(&self) {
    // Out of its window, the size waits for it: asked again, the same size would be nothing new.
    let Some(window) = self.window() else { return };
    let want = match self.ivars().state.try_borrow_mut().as_deref_mut() {
      Ok(Some(st)) => st.rt.take_resize(),
      _ => None,
    };

    let Some(size) = want else { return };
    let old = window.frame();
    let content = window.contentRectForFrameRect(old);
    let rect = NSRect::new(content.origin, NSSize::new(size.w as f64, size.h as f64));
    let mut frame = window.frameRectForContentRect(rect);
    frame.origin.y = old.origin.y + old.size.height - frame.size.height;
    let min = window.contentMinSize();
    let (w, h) = (min.width.min(size.w as f64), min.height.min(size.h as f64));
    window.setContentMinSize(NSSize::new(w, h));
    window.setFrame_display(frame, true);
    if let Some(anchor) = self.ivars().overlay.get() {
      overlay::place(&window, anchor, false);
    }
  }
}
