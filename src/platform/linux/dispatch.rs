// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Wayland objects' events into the window's state: the shell's configures (size, and focus
//! from the activated state), the surface's scale and outputs (as they come and go), frame
//! callbacks, buffers coming back, the seat's pointer, pinch and keyboard, and the clipboard's
//! offers.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use wayland_client::globals::GlobalListContents;
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::WlDataDeviceManager;
use wayland_client::protocol::wl_data_offer::{self, WlDataOffer};
use wayland_client::protocol::wl_data_source::{self, WlDataSource};
use wayland_client::protocol::wl_keyboard::{self, KeyState, WlKeyboard};
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_pointer::{self, ButtonState, WlPointer};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::{self, Capability, WlSeat};
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::{self, WlSurface};
use wayland_client::{
  Connection, Dispatch, Proxy, QueueHandle, WEnum, delegate_noop, event_created_child,
};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::{
  self, WpFractionalScaleV1,
};
use wayland_protocols::wp::pointer_gestures::zv1::client::zwp_pointer_gesture_pinch_v1::{
  self, ZwpPointerGesturePinchV1,
};
use wayland_protocols::wp::pointer_gestures::zv1::client::zwp_pointer_gestures_v1::ZwpPointerGesturesV1;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

use super::shm::Busy;
use super::{Output, Repeat, Win, keymap};
use crate::app::{Event, Mods, Phase};
use crate::scene::{Point, Size};

delegate_noop!(Win: ignore WlCompositor);
delegate_noop!(Win: ignore WlShm);
delegate_noop!(Win: ignore WlShmPool);
delegate_noop!(Win: ignore WpViewporter);
delegate_noop!(Win: ignore WpViewport);
delegate_noop!(Win: ignore WpFractionalScaleManagerV1);
delegate_noop!(Win: ignore WlDataDeviceManager);
delegate_noop!(Win: ignore WpCursorShapeManagerV1);
delegate_noop!(Win: ignore WpCursorShapeDeviceV1);
delegate_noop!(Win: ignore ZwpPointerGesturesV1);

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
        win.focus(activated(&states));
      }
      xdg_toplevel::Event::Close => win.closed = true,
      _ => {}
    }
  }
}

impl Dispatch<WlSurface, ()> for Win {
  fn event(
    win: &mut Self,
    surface: &WlSurface,
    ev: wl_surface::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    // The cursor's surface ([super::cursor]) has its own outputs: only the window's count.
    if *surface != win.surface {
      return;
    }

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

impl Dispatch<WlSeat, ()> for Win {
  fn event(
    win: &mut Self,
    seat: &WlSeat,
    ev: wl_seat::Event,
    _: &(),
    _: &Connection,
    qh: &QueueHandle<Self>,
  ) {
    let wl_seat::Event::Capabilities { capabilities: WEnum::Value(caps) } = ev else { return };
    // Devices go and come back (a VNC client's virtual pointer and keyboard last as long as its
    // connection): a removed one is let go, so the next one is picked up.
    if !caps.contains(Capability::Pointer)
      && let Some(pointer) = win.pointer.take()
    {
      if let Some(dev) = win.shape_device.take() {
        dev.destroy();
      }

      if let Some(pinch) = win._pinch.take().filter(|p| p.version() >= 2) {
        pinch.destroy();
      }

      if pointer.version() >= 3 {
        pointer.release();
      }

      win.enter_serial = None;
    }

    if !caps.contains(Capability::Keyboard)
      && let Some(keyboard) = win.keyboard.take()
    {
      if keyboard.version() >= 3 {
        keyboard.release();
      }

      win.repeat = None;
      win.mods = Mods::NONE;
      win.send(Event::Mods(Mods::NONE));
    }

    if caps.contains(Capability::Pointer) && win.pointer.is_none() {
      let pointer = seat.get_pointer(qh, ());
      win.shape_device = win.shapes.as_ref().map(|m| m.get_pointer(&pointer, qh, ()));
      win._pinch = win.gestures.as_ref().map(|g| g.get_pinch_gesture(&pointer, qh, ()));
      win.pointer = Some(pointer);
    }

    if caps.contains(Capability::Keyboard) && win.keyboard.is_none() {
      win.keyboard = Some(seat.get_keyboard(qh, ()));
    }
  }
}

impl Dispatch<WlPointer, ()> for Win {
  fn event(
    win: &mut Self,
    pointer: &WlPointer,
    ev: wl_pointer::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    let mods = win.mods;
    match ev {
      wl_pointer::Event::Enter { serial, surface_x, surface_y, .. } => {
        win.enter_serial = Some(serial);
        win.mouse = Point::new(surface_x as f32, surface_y as f32);
        win.show_cursor(serial);

        win.send(Event::MouseMove { pos: win.mouse, mods });
      }
      wl_pointer::Event::Leave { .. } => {
        win.enter_serial = None;
        win.send(Event::MouseExit);
      }
      wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
        win.mouse = Point::new(surface_x as f32, surface_y as f32);
        win.send(Event::MouseMove { pos: win.mouse, mods });
      }
      wl_pointer::Event::Button { serial, button, state, .. } => {
        let down = state == WEnum::Value(ButtonState::Pressed);
        if down {
          win.clip.borrow_mut().serial = serial;
        }

        win.button(button, down);
      }
      wl_pointer::Event::Axis { axis: WEnum::Value(axis), value, .. } => {
        win.axes.value[(axis as usize).min(1)] += value;
        win.axes.any = true;
      }
      wl_pointer::Event::AxisSource { axis_source: WEnum::Value(s) } => win.axes.source = Some(s),
      wl_pointer::Event::AxisStop { axis: WEnum::Value(axis), .. } => {
        win.axes.stop[(axis as usize).min(1)] = true;
        win.axes.any = true;
      }
      wl_pointer::Event::AxisDiscrete { axis: WEnum::Value(axis), discrete } => {
        win.axes.v120[(axis as usize).min(1)] += discrete * 120;
      }
      wl_pointer::Event::AxisValue120 { axis: WEnum::Value(axis), value120 } => {
        win.axes.v120[(axis as usize).min(1)] += value120;
      }
      wl_pointer::Event::Frame => win.scroll(),
      _ => {}
    }
    // Before version 5 there are no frames: each axis event is one.
    if pointer.version() < 5 && win.axes.any {
      win.scroll();
    }
  }
}

impl Dispatch<ZwpPointerGesturePinchV1, ()> for Win {
  fn event(
    win: &mut Self,
    _: &ZwpPointerGesturePinchV1,
    ev: zwp_pointer_gesture_pinch_v1::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    let (pos, mods) = (win.mouse, win.mods);
    let (delta, phase) = match ev {
      zwp_pointer_gesture_pinch_v1::Event::Begin { .. } => {
        win.pinch_scale = 1.0;
        (0.0, Phase::Began)
      }
      // The scale is the gesture's since it began; the delta is the change since the last
      // update, as a fraction of it (×1.5 then ×2 is +50%, then +33%).
      zwp_pointer_gesture_pinch_v1::Event::Update { scale, .. } => {
        let d = if win.pinch_scale > 0.0 { scale / win.pinch_scale - 1.0 } else { 0.0 };
        win.pinch_scale = scale;
        (d as f32, Phase::Changed)
      }
      zwp_pointer_gesture_pinch_v1::Event::End { cancelled, .. } => {
        (0.0, if cancelled != 0 { Phase::Cancelled } else { Phase::Ended })
      }
      _ => return,
    };

    win.send(Event::Magnify { pos, delta, phase, mods });
  }
}

impl Dispatch<WlKeyboard, ()> for Win {
  fn event(
    win: &mut Self,
    _: &WlKeyboard,
    ev: wl_keyboard::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    match ev {
      wl_keyboard::Event::Keymap { fd, size, .. } => win.kb = keymap(fd, size as usize),
      wl_keyboard::Event::Enter { .. } => win.focus(true),
      wl_keyboard::Event::Leave { .. } => {
        win.repeat = None;
        win.mods = Mods::NONE;
        win.send(Event::Mods(Mods::NONE));
        win.focus(false);
      }
      wl_keyboard::Event::Key { serial, key, state, .. } => {
        let down = state == WEnum::Value(KeyState::Pressed);
        if down {
          win.clip.borrow_mut().serial = serial;
        }

        let (rate, delay) = win.repeat_info;
        let repeats = rate > 0 && win.kb.as_ref().is_some_and(|kb| kb.repeats(key));
        let at = Instant::now() + Duration::from_millis(delay.max(0) as u64);
        win.repeat = Repeat::after(win.repeat.take(), key, down, repeats, at);
        win.key(key, down, false);
      }
      wl_keyboard::Event::Modifiers {
        mods_depressed, mods_latched, mods_locked, group, ..
      } => {
        let Some(kb) = &mut win.kb else { return };
        kb.modifiers(mods_depressed, mods_latched, mods_locked, group);
        let mods = kb.mods();
        if mods != win.mods {
          win.mods = mods;
          win.send(Event::Mods(mods));
        }

        // The modifier keys that changed them come after.
        win.modifier_keys();
      }
      wl_keyboard::Event::RepeatInfo { rate, delay } => {
        win.repeat_info = (rate, delay);
        // A rate of 0 turns repeat off, for the key held now too.
        if rate <= 0 {
          win.repeat = None;
        }
      }
      _ => {}
    }
  }
}

impl Dispatch<WlDataDevice, ()> for Win {
  fn event(
    win: &mut Self,
    _: &WlDataDevice,
    ev: wl_data_device::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    match ev {
      wl_data_device::Event::DataOffer { id } => win.clip.borrow_mut().offers.push((id, vec![])),
      wl_data_device::Event::Selection { id } => win.clip.borrow_mut().select(id),
      _ => {}
    }
  }

  event_created_child!(Win, WlDataDevice, [
    wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, ()),
  ]);
}

impl Dispatch<WlDataOffer, ()> for Win {
  fn event(
    win: &mut Self,
    offer: &WlDataOffer,
    ev: wl_data_offer::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    if let wl_data_offer::Event::Offer { mime_type } = ev
      && let Some((_, types)) = win.clip.borrow_mut().offers.iter_mut().find(|(o, _)| o == offer)
    {
      types.push(mime_type);
    }
  }
}

impl Dispatch<WlDataSource, ()> for Win {
  fn event(
    win: &mut Self,
    source: &WlDataSource,
    ev: wl_data_source::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    match ev {
      wl_data_source::Event::Send { fd, .. } => win.clip.borrow_mut().send(source, fd),
      wl_data_source::Event::Cancelled => win.clip.borrow_mut().cancelled(source),
      _ => {}
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
