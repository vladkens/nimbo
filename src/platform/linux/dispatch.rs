// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Wayland objects' events into the window's state: the shell's configures (size, and focus
//! from the activated state), the surface's scale and outputs (as they come and go), frame
//! callbacks and buffers coming back.

use std::sync::atomic::Ordering;

use wayland_client::globals::GlobalListContents;
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::{self, WlSurface};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::{
  self, WpFractionalScaleV1,
};
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

use super::shm::Busy;
use super::{Output, Win};
use crate::app::Event;
use crate::scene::Size;

delegate_noop!(Win: ignore WlCompositor);
delegate_noop!(Win: ignore WlShm);
delegate_noop!(Win: ignore WlShmPool);
delegate_noop!(Win: ignore WpViewporter);
delegate_noop!(Win: ignore WpViewport);
delegate_noop!(Win: ignore WpFractionalScaleManagerV1);

/// Outputs connected and disconnected after the start: a new one may have another scale.
impl Dispatch<WlRegistry, GlobalListContents> for Win {
  fn event(
    win: &mut Self,
    registry: &WlRegistry,
    ev: wl_registry::Event,
    _: &GlobalListContents,
    _: &Connection,
    qh: &QueueHandle<Self>,
  ) {
    match ev {
      wl_registry::Event::Global { name, interface, version } if interface == "wl_output" => {
        win.outputs.push(Output::bind(registry, name, version, qh));
      }
      wl_registry::Event::GlobalRemove { name } => {
        let Some(i) = win.outputs.iter().position(|o| o.name == name) else { return };
        let o = win.outputs.remove(i);
        win.entered.retain(|e| *e != o.obj);
        if o.obj.version() >= 3 {
          o.obj.release();
        }
      }
      _ => return,
    }

    win.rt.redraw();
  }
}

impl Dispatch<XdgWmBase, ()> for Win {
  fn event(
    _: &mut Self,
    wm: &XdgWmBase,
    ev: xdg_wm_base::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    if let xdg_wm_base::Event::Ping { serial } = ev {
      wm.pong(serial);
    }
  }
}

impl Dispatch<XdgSurface, ()> for Win {
  fn event(
    win: &mut Self,
    xdg: &XdgSurface,
    ev: xdg_surface::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    if let xdg_surface::Event::Configure { serial } = ev {
      xdg.ack_configure(serial);
      if let Some(s) = win.pending_size.take() {
        win.size = s;
      }

      win.configured = true;
      win.acked = true;
      win.rt.redraw();
    }
  }
}

impl Dispatch<XdgToplevel, ()> for Win {
  fn event(
    win: &mut Self,
    _: &XdgToplevel,
    ev: xdg_toplevel::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    match ev {
      xdg_toplevel::Event::Configure { width, height, states } => {
        let last = win.pending_size.unwrap_or(win.size);
        win.pending_size = Some(configured(width, height, last));
        let on = activated(&states);
        if on != win.focused {
          win.focused = on;
          win.rt.event(Event::Focus(on));
        }
      }
      xdg_toplevel::Event::Close => win.closed = true,
      _ => {}
    }
  }
}

impl Dispatch<WlSurface, ()> for Win {
  fn event(
    win: &mut Self,
    _: &WlSurface,
    ev: wl_surface::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    match ev {
      wl_surface::Event::Enter { output } => win.entered.push(output),
      wl_surface::Event::Leave { output } => win.entered.retain(|o| *o != output),
      wl_surface::Event::PreferredBufferScale { factor } => win.preferred = Some(factor),
      _ => return,
    }

    win.rt.redraw();
  }
}

impl Dispatch<WlOutput, ()> for Win {
  fn event(
    win: &mut Self,
    output: &WlOutput,
    ev: wl_output::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    if let wl_output::Event::Scale { factor } = ev
      && let Some(o) = win.outputs.iter_mut().find(|o| o.obj == *output)
    {
      o.scale = factor;
      win.rt.redraw();
    }
  }
}

impl Dispatch<WpFractionalScaleV1, ()> for Win {
  fn event(
    win: &mut Self,
    _: &WpFractionalScaleV1,
    ev: wp_fractional_scale_v1::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    if let wp_fractional_scale_v1::Event::PreferredScale { scale } = ev {
      win.scale120 = Some(scale);
      win.rt.redraw();
    }
  }
}

impl Dispatch<WlCallback, ()> for Win {
  fn event(
    win: &mut Self,
    _: &WlCallback,
    ev: wl_callback::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    if let wl_callback::Event::Done { .. } = ev {
      win.frame_pending = false;
    }
  }
}

impl Dispatch<WlBuffer, Busy> for Win {
  fn event(
    win: &mut Self,
    _: &WlBuffer,
    ev: wl_buffer::Event,
    busy: &Busy,
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    if let wl_buffer::Event::Release = ev {
      busy.store(false, Ordering::Release);
      if win.present.starved {
        win.rt.redraw();
      }
    }
  }
}

/// The size an `xdg_toplevel.configure` asks for: a side of 0 is left to us, so it keeps `last`.
fn configured(width: i32, height: i32, last: Size) -> Size {
  let side = |n: i32, last: f32| if n > 0 { n as f32 } else { last };
  Size::new(side(width, last.w), side(height, last.h))
}

/// Whether a configure's states (native-endian `u32`s) hold `activated`: the window has the
/// focus.
fn activated(states: &[u8]) -> bool {
  let activated = xdg_toplevel::State::Activated as u32;
  states.as_chunks::<4>().0.iter().any(|s| u32::from_ne_bytes(*s) == activated)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_configure_sets_each_side_it_gives() {
    let last = Size::new(480.0, 320.0);
    assert_eq!(configured(0, 0, last), last);
    assert_eq!(configured(640, 0, last), Size::new(640.0, 320.0));
    assert_eq!(configured(0, 200, last), Size::new(480.0, 200.0));
    assert_eq!(configured(800, 600, last), Size::new(800.0, 600.0));
  }

  #[test]
  fn activated_is_the_focus() {
    let states = |s: &[u32]| s.iter().flat_map(|n| n.to_ne_bytes()).collect::<Vec<u8>>();
    assert!(!activated(&[]));
    assert!(!activated(&states(&[1, 3])));
    assert!(activated(&states(&[1, 4])));
  }
}
