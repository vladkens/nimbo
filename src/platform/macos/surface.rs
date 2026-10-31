// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The CPU renderer's window path: frames drawn straight into IOSurfaces that become the view
//! layer's `contents`, which the window server composites without a copy. A surface the window
//! server still reads is left alone (a second, at most a third, is made; with all three busy the
//! frame waits, [Presenter::starved]); each keeps the tile hashes of the frame it holds, so only
//! the tiles that changed since are drawn into it.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::time::{SystemTime, UNIX_EPOCH};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_quartz_core::{CALayer, CATransaction};

use crate::cpu::Renderer;
use crate::scene::{Scene, Size};

// IOSurface.framework, the few calls used (<IOSurface/IOSurfaceRef.h>).
#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
  fn IOSurfaceCreate(properties: &CFDictionary) -> Option<NonNull<CFType>>;
  fn IOSurfaceLock(surface: &CFType, options: u32, seed: *mut u32) -> i32;
  fn IOSurfaceUnlock(surface: &CFType, options: u32, seed: *mut u32) -> i32;
  fn IOSurfaceGetBaseAddress(surface: &CFType) -> *mut c_void;
  fn IOSurfaceGetBytesPerRow(surface: &CFType) -> usize;
  fn IOSurfaceIsInUse(surface: &CFType) -> u8;
  #[cfg(test)]
  fn IOSurfaceIncrementUseCount(surface: &CFType);
  #[cfg(test)]
  fn IOSurfaceDecrementUseCount(surface: &CFType);
}

/// Surfaces at most: one shown, one drawn, one more while the window server holds both.
const SURFACES: usize = 3;
/// `'BGRA'`, the pixel format code of `kCVPixelFormatType_32BGRA`.
const BGRA: i32 = 0x4247_5241;

pub(super) struct Surface {
  pub obj: CFRetained<CFType>,
  pub w: usize,
  pub h: usize,
  /// The tile hashes of the frame it holds.
  tiles: Vec<u64>,
}

impl Surface {
  pub fn new(w: usize, h: usize) -> Option<Self> {
    let key = |s: &'static str| CFString::from_static_str(s);
    let keys =
      ["IOSurfaceWidth", "IOSurfaceHeight", "IOSurfaceBytesPerElement", "IOSurfacePixelFormat"]
        .map(key);
    let vals = [w as i32, h as i32, 4, BGRA].map(CFNumber::new_i32);
    let keys: Vec<&CFString> = keys.iter().map(|k| &**k).collect();
    let vals: Vec<&CFNumber> = vals.iter().map(|v| &**v).collect();
    let props = CFDictionary::from_slices(&keys, &vals);
    // SAFETY: the dictionary holds the properties IOSurfaceCreate documents; it returns a +1
    // reference or null.
    let ptr = unsafe { IOSurfaceCreate(props.as_opaque()) }?;
    let obj = unsafe { CFRetained::from_raw(ptr) };
    Some(Self { obj, w, h, tiles: Vec::new() })
  }

  pub fn in_use(&self) -> bool {
    unsafe { IOSurfaceIsInUse(&self.obj) != 0 }
  }

  /// Runs `f` on the locked pixels: rows of `stride` pixels.
  pub fn with_pixels(&mut self, f: impl FnOnce(&mut [u32], usize)) {
    let s = &*self.obj;
    unsafe { IOSurfaceLock(s, 0, std::ptr::null_mut()) };
    let stride = unsafe { IOSurfaceGetBytesPerRow(s) } / 4;
    let base = unsafe { IOSurfaceGetBaseAddress(s) }.cast::<u32>();
    // SAFETY: a locked surface's base address holds `h` rows of `bytes per row` bytes, 4-byte
    // aligned, which nothing else writes while it is locked.
    let px = unsafe { std::slice::from_raw_parts_mut(base, stride * self.h) };
    f(px, stride);
    unsafe { IOSurfaceUnlock(s, 0, std::ptr::null_mut()) };
  }
}

pub struct Presenter {
  pub renderer: Renderer,
  /// Logs each frame set as the layer's contents, as `nimbo: presented <unix ms>` on stderr
  /// (`NIMBO_TRACE`): when it is committed, not when it reaches the screen.
  pub trace: bool,
  layer: Retained<CALayer>,
  surfaces: Vec<Surface>,
  /// The surface the layer shows.
  front: Option<usize>,
  /// A frame waits: the window server reads every surface. The caller draws it again later.
  pub starved: bool,
}

impl Presenter {
  pub fn new(layer: Retained<CALayer>) -> Self {
    let surfaces = Vec::new();
    Self { renderer: Renderer::new(), trace: false, layer, surfaces, front: None, starved: false }
  }

  /// Draws the scene into a surface the window server isn't reading and shows it; nothing when
  /// the frame equals the one shown, or when every surface is busy ([Presenter::starved]).
  /// Returns whether a frame was shown.
  pub fn present(&mut self, scene: &Scene, size: Size, scale: f32) -> bool {
    self.starved = false;
    if size.w <= 0.0 || size.h <= 0.0 {
      return false;
    }

    let r = &mut self.renderer;
    r.prepare(scene, size, scale);
    let (w, h) = r.size();
    if self.surfaces.first().is_some_and(|s| (s.w, s.h) != (w, h)) {
      self.surfaces.clear();
      self.front = None;
    }

    if let Some(f) = self.front
      && r.damage(&self.surfaces[f].tiles).is_empty()
    {
      return false;
    }
    let free =
      (0..self.surfaces.len()).find(|&i| Some(i) != self.front && !self.surfaces[i].in_use());
    let i = match free {
      Some(i) => i,
      None if self.surfaces.len() < SURFACES => match Surface::new(w, h) {
        Some(s) => {
          self.surfaces.push(s);
          self.surfaces.len() - 1
        }
        None => return false,
      },
      // Every surface is busy: drawing into one would tear the frame the window server reads
      // (a lock isn't its fence).
      None => {
        self.starved = true;
        return false;
      }
    };

    let s = &mut self.surfaces[i];
    let rects = r.damage(&s.tiles);
    s.with_pixels(|px, stride| r.draw(scene, px, stride, &rects));
    s.tiles.clear();
    s.tiles.extend_from_slice(r.tiles());
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    let obj: &AnyObject = s.obj.as_ref();
    // SAFETY: a layer's contents may be an IOSurface (CALayer.h: "an IOSurfaceRef").
    unsafe { self.layer.setContents(Some(obj)) };
    self.layer.setContentsScale(scale as f64);
    CATransaction::commit();
    self.front = Some(i);
    if self.trace {
      let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
      eprintln!("nimbo: presented {:.3}", t.as_secs_f64() * 1e3);
    }

    true
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scene::{Quad, Rect};

  fn scene(x: f32) -> Scene {
    let mut s = Scene::default();
    s.quad(Quad::new(Rect::new(0.0, 0.0, 150.0, 90.0), [0.1, 0.2, 0.3, 1.0]));
    s.quad(Quad::new(Rect::new(x, 20.0, 40.0, 30.0), [0.9, 0.5, 0.1, 0.8]).radius(6.0));
    s
  }

  /// The pixels of the shown surface, rows without padding.
  fn shown(p: &mut Presenter) -> Vec<u8> {
    pixels(p, p.front.unwrap())
  }

  fn pixels(p: &mut Presenter, i: usize) -> Vec<u8> {
    let s = &mut p.surfaces[i];
    let mut out = Vec::new();
    let w = s.w;
    s.with_pixels(|px, stride| {
      for row in px.chunks(stride) {
        out.extend(row[..w].iter().flat_map(|p| p.to_le_bytes()));
      }
    });
    out
  }

  #[test]
  fn surfaces_show_the_frame() {
    let mut p = Presenter::new(CALayer::new());
    let (size, scale) = (Size::new(150.0, 90.0), 2.0);
    for x in [10.0, 10.0, 60.5, 61.0] {
      let s = scene(x);
      p.present(&s, size, scale);
      let want = p.renderer.snapshot(&s, size, scale).data;
      assert!(shown(&mut p) == want, "x = {x}");
    }

    assert!(!p.present(&scene(61.0), size, scale), "an unchanged frame is shown again");
  }

  #[test]
  fn busy_surfaces_are_left_alone() {
    let mut p = Presenter::new(CALayer::new());
    let (size, scale) = (Size::new(150.0, 90.0), 2.0);
    let busy = |p: &Presenter, i: usize| unsafe { IOSurfaceIncrementUseCount(&p.surfaces[i].obj) };
    // Two surfaces while the window server reads none; a third while it reads both.
    assert!(p.present(&scene(10.0), size, scale));
    assert!(p.present(&scene(20.0), size, scale));
    assert_eq!(p.surfaces.len(), 2);
    busy(&p, 0);
    busy(&p, 1);
    assert!(p.present(&scene(30.0), size, scale));
    assert_eq!((p.surfaces.len(), p.front), (3, Some(2)));

    // All three busy: the frame waits and no surface changes.
    busy(&p, 2);
    let before: Vec<Vec<u8>> = (0..3).map(|i| pixels(&mut p, i)).collect();
    assert!(!p.present(&scene(40.0), size, scale));
    assert!(p.starved);
    assert_eq!((0..3).map(|i| pixels(&mut p, i)).collect::<Vec<_>>(), before);

    // One comes back: the frame draws into it.
    unsafe { IOSurfaceDecrementUseCount(&p.surfaces[0].obj) };
    assert!(p.present(&scene(40.0), size, scale));
    assert!(!p.starved && p.front == Some(0));
    let want = p.renderer.snapshot(&scene(40.0), size, scale).data;
    assert!(shown(&mut p) == want);
    for i in 0..3 {
      while p.surfaces[i].in_use() {
        unsafe { IOSurfaceDecrementUseCount(&p.surfaces[i].obj) };
      }
    }
  }
}
