// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Overlays on Linux ([crate::Config::overlay]): a window with an RGBA visual that GTK paints
//! with the frame's alpha, an empty input shape (the mouse goes through it), no focus, kept above
//! and on every workspace, out of the taskbar and pager.
//!
//! On X11 it is a popup (override-redirect: no window manager frame, focus or stacking), moved
//! to its anchor in the work area of the monitor with the pointer at each show. Wayland lets no
//! app place itself: with `libgtk-layer-shell` (the wlroots compositors, KDE) it is a layer
//! surface on the overlay layer, anchored to the edge with its margin, without the keyboard, on
//! the output the compositor picks (usually the focused one); without it (GNOME) a borderless
//! toplevel that the compositor places, and may focus and stack, as any window.

use std::ffi::c_int;
use std::sync::OnceLock;

use super::dl::{Lib, fns};
use super::{FALSE, P, S, TRUE, lib};
use crate::scene::Size;
use crate::shell::Anchor;

fns!(Gdk {
  fn g_type_check_instance_is_a(instance: P, ty: usize) -> c_int;
  fn gtk_widget_get_screen(w: P) -> P;
  fn gdk_screen_get_rgba_visual(screen: P) -> P;
  fn gtk_widget_set_visual(w: P, visual: P);
  fn gtk_widget_set_app_paintable(w: P, on: c_int);
  fn gtk_widget_input_shape_combine_region(w: P, region: P);
  fn cairo_region_create() -> P;
  fn cairo_region_destroy(region: P);
  fn gtk_window_set_decorated(w: P, on: c_int);
  fn gtk_window_set_accept_focus(w: P, on: c_int);
  fn gtk_window_set_focus_on_map(w: P, on: c_int);
  fn gtk_window_set_keep_above(w: P, on: c_int);
  fn gtk_window_stick(w: P);
  fn gtk_window_set_skip_taskbar_hint(w: P, on: c_int);
  fn gtk_window_set_skip_pager_hint(w: P, on: c_int);
  fn gtk_window_set_type_hint(w: P, hint: c_int);
  fn gtk_window_move(w: P, x: c_int, y: c_int);
  fn gtk_widget_get_window(w: P) -> P;
  fn gdk_window_raise(w: P);
  fn gdk_display_get_default() -> P;
  fn gdk_display_get_default_seat(display: P) -> P;
  fn gdk_seat_get_pointer(seat: P) -> P;
  fn gdk_device_get_position(device: P, screen: *mut P, x: *mut c_int, y: *mut c_int);
  fn gdk_display_get_monitor_at_point(display: P, x: c_int, y: c_int) -> P;
  fn gdk_display_get_monitor_at_window(display: P, window: P) -> P;
  fn gdk_monitor_get_workarea(monitor: P, rect: *mut [c_int; 4]);
});

// gtk-layer-shell 0.6's headers (<gtk-layer-shell/gtk-layer-shell.h>).
fns!(LayerShell {
  fn gtk_layer_is_supported() -> c_int;
  fn gtk_layer_init_for_window(w: P);
  fn gtk_layer_set_namespace(w: P, name: S);
  fn gtk_layer_set_layer(w: P, layer: c_int);
  fn gtk_layer_set_anchor(w: P, edge: c_int, on: c_int);
  fn gtk_layer_set_margin(w: P, edge: c_int, margin: c_int);
  fn gtk_layer_set_keyboard_interactivity(w: P, on: c_int);
});

/// `GTK_WINDOW_POPUP`, `GDK_WINDOW_TYPE_HINT_NOTIFICATION`.
const POPUP: c_int = 1;
const NOTIFICATION: c_int = 11;
/// `GTK_LAYER_SHELL_LAYER_OVERLAY`, `GTK_LAYER_SHELL_EDGE_TOP` and `_BOTTOM`.
const OVERLAY: c_int = 3;
const TOP: c_int = 2;
const BOTTOM: c_int = 3;

fn gdk() -> &'static Gdk {
  static GDK: OnceLock<Option<Gdk>> = OnceLock::new();
  GDK.get_or_init(|| Gdk::load(&lib()?)).as_ref().expect("GTK is loaded before a window opens")
}

/// The display is Wayland's: GDK's Wayland backend is built in and opened it.
pub fn wayland() -> bool {
  let Some(lib) = lib() else { return false };
  // SAFETY: GObject's type getter of GDK's Wayland display, a `GType` (a `gsize`).
  let get = unsafe { lib.sym::<unsafe extern "C" fn() -> usize>("gdk_wayland_display_get_type\0") };
  let g = gdk();
  get.is_some_and(
    |f| unsafe { (g.g_type_check_instance_is_a)((g.gdk_display_get_default)(), f()) } != 0,
  )
}

/// The layer shell, where the compositor has it (Wayland only).
fn layer_shell() -> Option<&'static LayerShell> {
  static LS: OnceLock<Option<LayerShell>> = OnceLock::new();
  let ls = LS.get_or_init(|| LayerShell::load(&Lib::open(&[c"libgtk-layer-shell.so.0"])?));
  ls.as_ref().filter(|ls| unsafe { (ls.gtk_layer_is_supported)() } != 0)
}

/// The kind of GtkWindow an overlay is: a popup on X11, a toplevel on Wayland.
pub fn kind() -> c_int {
  if wayland() { super::WINDOW_TOPLEVEL } else { POPUP }
}

/// Sets overlay `window` up before it shows: see the module docs.
pub fn setup(window: P, anchor: Anchor) {
  let g = gdk();
  // SAFETY: GTK's calls on a window of ours not shown yet.
  unsafe {
    let visual = (g.gdk_screen_get_rgba_visual)((g.gtk_widget_get_screen)(window));
    if !visual.is_null() {
      (g.gtk_widget_set_visual)(window, visual);
    }

    (g.gtk_widget_set_app_paintable)(window, TRUE);
    let none = (g.cairo_region_create)();
    (g.gtk_widget_input_shape_combine_region)(window, none);
    (g.cairo_region_destroy)(none);
    (g.gtk_window_set_decorated)(window, FALSE);
    (g.gtk_window_set_accept_focus)(window, FALSE);
    (g.gtk_window_set_focus_on_map)(window, FALSE);
    (g.gtk_window_set_keep_above)(window, TRUE);
    (g.gtk_window_stick)(window);
    (g.gtk_window_set_skip_taskbar_hint)(window, TRUE);
    (g.gtk_window_set_skip_pager_hint)(window, TRUE);
    (g.gtk_window_set_type_hint)(window, NOTIFICATION);
  }

  // Not loaded on X11, where it has nothing to do.
  let Some(ls) = wayland().then(layer_shell).flatten() else { return };
  // SAFETY: the layer shell's calls on a toplevel not realized yet.
  unsafe {
    (ls.gtk_layer_init_for_window)(window);
    (ls.gtk_layer_set_namespace)(window, c"overlay".as_ptr());
    (ls.gtk_layer_set_layer)(window, OVERLAY);
    (ls.gtk_layer_set_keyboard_interactivity)(window, FALSE);
    let edge = match anchor {
      Anchor::Top(m) => Some((TOP, m)),
      Anchor::Center => None,
      Anchor::Bottom(m) => Some((BOTTOM, m)),
    };

    if let Some((edge, margin)) = edge {
      (ls.gtk_layer_set_anchor)(window, edge, TRUE);
      (ls.gtk_layer_set_margin)(window, edge, margin.round() as c_int);
    }
  }
}

/// Raises overlay `window`, shown already, over the others (X11; the compositor's on Wayland).
pub fn raise(window: P) {
  let g = gdk();
  let w = unsafe { (g.gtk_widget_get_window)(window) };
  if !w.is_null() {
    unsafe { (g.gdk_window_raise)(w) };
  }
}

/// Moves overlay `window` of `size` to `anchor` in the work area of the monitor with the
/// pointer, or of its own unless `pointer`; nothing on Wayland, where the compositor places it.
pub fn place(window: P, size: Size, anchor: Anchor, pointer: bool) {
  if wayland() {
    return;
  }

  let g = gdk();
  let mut area = [0; 4];
  // SAFETY: GDK's calls on the default display and a window of ours; out-pointers to locals.
  unsafe {
    let display = (g.gdk_display_get_default)();
    let monitor = match pointer {
      true => {
        let pointer = (g.gdk_seat_get_pointer)((g.gdk_display_get_default_seat)(display));
        let (mut x, mut y) = (0, 0);
        (g.gdk_device_get_position)(pointer, std::ptr::null_mut(), &mut x, &mut y);
        (g.gdk_display_get_monitor_at_point)(display, x, y)
      }
      false => match (g.gtk_widget_get_window)(window) {
        w if w.is_null() => return,
        w => (g.gdk_display_get_monitor_at_window)(display, w),
      },
    };

    if monitor.is_null() {
      return;
    }

    (g.gdk_monitor_get_workarea)(monitor, &mut area);
  }

  let [x, y, w, h] = area;
  let at = anchor.offset(Size::new(w as f32, h as f32), size);
  unsafe { (g.gtk_window_move)(window, x + at.x as c_int, y + at.y as c_int) };
}
