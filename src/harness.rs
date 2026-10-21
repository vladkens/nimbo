// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The headless harness: runs an app with no window on the same renderer, offscreen, at a
//! given size and scale, on virtual time. Scripted events drive it; it renders PNG shots
//! ([crate::png]).

use std::path::Path;
use std::time::Duration;

use crate::app::{App, Clock, Event, Runtime};
use crate::cpu::{self, Pixels};
use crate::scene::Size;

/// A display refresh at 120 Hz, the step of [Harness::wait].
pub const TICK: Duration = Duration::from_nanos(8_333_333);

pub struct Harness<A> {
  pub rt: Runtime<A>,
  pub cpu: cpu::Renderer,
}

impl<A: App> Harness<A> {
  pub fn new(app: A, size: Size, scale: f32) -> Self {
    let clock = Clock::Virtual(Duration::ZERO);
    let rt = Runtime::new(app, size, scale, clock);
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

  pub fn event(&mut self, ev: Event) -> bool {
    self.rt.event(ev)
  }

  /// Moves virtual time on without building frames.
  pub fn advance(&mut self, dt: Duration) {
    self.rt.advance(dt);
  }

  /// Lets `dt` of virtual time pass as a window would: a display refresh every [TICK], each
  /// building a frame when one is needed (an animation). Returns the frames built.
  pub fn wait(&mut self, dt: Duration) -> usize {
    let (end, mut frames) = (self.rt.now() + dt, 0);
    while self.rt.now() < end {
      self.advance(TICK.min(end - self.rt.now()));
      frames += self.frame() as usize;
    }

    frames
  }

  // MARK: Output

  /// Builds a frame if anything asked for one; returns whether it did.
  pub fn frame(&mut self) -> bool {
    if !self.rt.needs_frame() {
      return false;
    }

    self.rt.frame();
    self.cpu.sync(&mut self.rt.images);
    true
  }

  /// Renders the current state offscreen and reads the pixels back.
  pub fn render(&mut self) -> Image {
    self.frame();
    let (size, scale) = (self.rt.size, self.rt.scale);
    let pixels = self.cpu.snapshot(&self.rt.scene, size, scale);
    Image { scale, pixels }
  }

  pub fn shot(&mut self, path: &Path) -> Result<(), String> {
    self.render().save_png(path)
  }
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
