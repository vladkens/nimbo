// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Wayland objects' events into the window's state: the shell's configures, the surface's
//! scale and outputs, frame callbacks and buffers coming back.

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
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::{
  self, WpFractionalScaleV1,
};
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

use super::Win;
use super::shm::Busy;
use crate::scene::Size;

delegate_noop!(Win: ignore WlCompositor);
delegate_noop!(Win: ignore WlShm);
delegate_noop!(Win: ignore WlShmPool);
delegate_noop!(Win: ignore WpViewporter);
delegate_noop!(Win: ignore WpViewport);
delegate_noop!(Win: ignore WpFractionalScaleManagerV1);

impl Dispatch<WlRegistry, GlobalListContents> for Win {
  fn event(
    _: &mut Self,
    _: &WlRegistry,
    _: wl_registry::Event,
    _: &GlobalListContents,
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
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
      // 0 × 0 leaves the size to us: the configured one, or the last.
      xdg_toplevel::Event::Configure { width, height, .. } if width > 0 && height > 0 => {
        win.pending_size = Some(Size::new(width as f32, height as f32));
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
      && let Some(o) = win.outputs.iter_mut().find(|o| o.0 == *output)
    {
      o.1 = factor;
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
