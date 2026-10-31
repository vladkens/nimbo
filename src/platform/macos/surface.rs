// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The window path: frames drawn straight into IOSurfaces that become the view layer's
//! `contents`, which the window server composites without a copy. A surface the window server
//! still reads is left alone (a second, at most a third, is made; with all three busy the frame
//! waits, [Presenter::starved]). On the CPU each keeps the tile hashes of the frame it holds, so
//! only the tiles that changed since are drawn into it; with `NIMBO_RENDERER=metal` (the `metal`
//! feature) the GPU draws every changed frame into them (`metal/present.rs`).

// Without the `metal` feature there is no Metal path: no value of either, the fields always
// `None`.
#[cfg(not(feature = "metal"))]
use std::convert::{Infallible as Metal, Infallible as Target};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::time::{SystemTime, UNIX_EPOCH};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_quartz_core::{CALayer, CATransaction};

#[cfg(feature = "metal")]
use super::metal::{present::Metal, render::Target};
use crate::cpu::Renderer;
use crate::scene::{Images, Scene, Size};
use crate::text::TextSystem;

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
  pub(crate) fn IOSurfaceIncrementUseCount(surface: &CFType);
  #[cfg(test)]
  pub(crate) fn IOSurfaceDecrementUseCount(surface: &CFType);
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
  /// The Metal render target over it, made at its first Metal frame.
  #[cfg_attr(not(feature = "metal"), allow(dead_code, reason = "Metal's"))]
  pub tex: Option<Target>,
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
    Some(Self { obj, w, h, tiles: Vec::new(), tex: None })
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
  pub(super) trace: bool,
  layer: Retained<CALayer>,
  pub(super) surfaces: Vec<Surface>,
  /// The surface the layer shows.
  pub(super) front: Option<usize>,
  /// A frame waits: the window server reads every surface, or Metal failed and the CPU takes
  /// over. The caller draws it again later.
  pub starved: bool,
  /// The Metal path, while the window draws with it.
  #[cfg_attr(not(feature = "metal"), allow(dead_code, reason = "Metal's"))]
  pub(super) metal: Option<Metal>,
}

impl Presenter {
  /// Draws with Metal when `metal` asks for it and it can be had, else on the CPU; stderr names
  /// the renderer when Metal was asked for.
  pub fn new(layer: Retained<CALayer>, transparent: bool, trace: bool, metal: bool) -> Self {
    let mut renderer = Renderer::new();
    renderer.transparent = transparent;
    #[cfg(feature = "metal")]
    let metal = metal.then(|| Metal::new(transparent)).flatten();
    #[cfg(not(feature = "metal"))]
    let metal = metal.then(|| eprintln!("nimbo: renderer cpu, built without the metal feature"));
    #[cfg(not(feature = "metal"))]
    let metal = metal.and(None);
    Self { renderer, trace, layer, surfaces: Vec::new(), front: None, starved: false, metal }
  }

  /// Copies new glyphs and images into the renderer that draws (only it: the CPU renderer holds
  /// none while Metal draws), then draws the scene into a surface the window server isn't
  /// reading and shows it; nothing when the frame equals the one shown, or when every surface is
  /// busy ([Presenter::starved]). Returns whether a frame was shown.
  pub fn present(
    &mut self,
    scene: &Scene,
    text: &mut TextSystem,
    images: &mut Images,
    size: Size,
    scale: f32,
  ) -> bool {
    self.starved = false;
    #[cfg(feature = "metal")]
    if let Some(m) = self.metal.take() {
      return m.present(self, scene, text, images, size, scale);
    }

    self.renderer.sync(text, images);
    if size.w <= 0.0 || size.h <= 0.0 {
      return false;
    }

    self.renderer.prepare(scene, size, scale);
    let (w, h) = self.renderer.size();
    self.resize(w, h);
    if let Some(f) = self.front
      && self.renderer.damage(&self.surfaces[f].tiles).is_empty()
    {
      return false;
    }

    let Some(i) = self.free(w, h) else { return false };
    let (s, r) = (&mut self.surfaces[i], &mut self.renderer);
    let rects = r.damage(&s.tiles);
    s.with_pixels(|px, stride| r.draw(scene, px, stride, &rects));
    s.tiles.clear();
    s.tiles.extend_from_slice(r.tiles());
    self.show(i, scale, "");
    true
  }

  /// Drops the surfaces when the frame is another size in pixels than `w` x `h`.
  pub(super) fn resize(&mut self, w: usize, h: usize) {
    if self.surfaces.first().is_some_and(|s| (s.w, s.h) != (w, h)) {
      self.surfaces.clear();
      self.front = None;
    }
  }

  /// A surface the window server isn't reading, made when there is none and fewer than
  /// [SURFACES]; `None` when none can be made or every one is busy ([Self::starved]): drawing
  /// into one would tear the frame the window server reads (a lock isn't its fence).
  pub(super) fn free(&mut self, w: usize, h: usize) -> Option<usize> {
    let ss = &self.surfaces;
    match (0..ss.len()).find(|&i| Some(i) != self.front && !ss[i].in_use()) {
      Some(i) => Some(i),
      None if ss.len() < SURFACES => {
        self.surfaces.push(Surface::new(w, h)?);
        Some(self.surfaces.len() - 1)
      }
      None => {
        self.starved = true;
        None
      }
    }
  }

  /// Sets surface `i` as the layer's contents; the trace's line ends with `extra`.
  pub(super) fn show(&mut self, i: usize, scale: f32, extra: &str) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    let obj: &AnyObject = self.surfaces[i].obj.as_ref();
    // SAFETY: a layer's contents may be an IOSurface (CALayer.h: "an IOSurfaceRef").
    unsafe { self.layer.setContents(Some(obj)) };
    self.layer.setContentsScale(scale as f64);
    CATransaction::commit();
    self.front = Some(i);
    if self.trace {
      let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
      eprintln!("nimbo: presented {:.3}{extra}", t.as_secs_f64() * 1e3);
    }
  }

  /// The view pauses: the CPU path keeps its surfaces, which hold its tile hashes (the Metal
  /// path drops all but the shown one).
  #[cfg(not(feature = "metal"))]
  pub fn idle(&mut self) {}
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::scene::{Quad, Rect};

  pub(crate) fn scene(x: f32) -> Scene {
    let mut s = Scene::default();
    s.quad(Quad::new(Rect::new(0.0, 0.0, 150.0, 90.0), [0.1, 0.2, 0.3, 1.0]));
    s.quad(Quad::new(Rect::new(x, 20.0, 40.0, 30.0), [0.9, 0.5, 0.1, 0.8]).radius(6.0));
    s
  }

  /// Presents a frame with no text or images.
  pub(crate) fn present(p: &mut Presenter, s: &Scene, size: Size, scale: f32) -> bool {
    p.present(s, &mut TextSystem::new(scale), &mut Images::default(), size, scale)
  }

  /// The pixels of the shown surface, rows without padding.
  pub(crate) fn shown(p: &mut Presenter) -> Vec<u8> {
    pixels(p, p.front.unwrap())
  }

  pub(crate) fn pixels(p: &mut Presenter, i: usize) -> Vec<u8> {
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
    let mut p = Presenter::new(CALayer::new(), false, false, false);
    let (size, scale) = (Size::new(150.0, 90.0), 2.0);
    for x in [10.0, 10.0, 60.5, 61.0] {
      let s = scene(x);
      present(&mut p, &s, size, scale);
      let want = p.renderer.snapshot(&s, size, scale).data;
      assert!(shown(&mut p) == want, "x = {x}");
    }

    assert!(!present(&mut p, &scene(61.0), size, scale), "an unchanged frame is shown again");
  }

  #[test]
  fn busy_surfaces_are_left_alone() {
    let mut p = Presenter::new(CALayer::new(), false, false, false);
    let (size, scale) = (Size::new(150.0, 90.0), 2.0);
    let busy = |p: &Presenter, i: usize| unsafe { IOSurfaceIncrementUseCount(&p.surfaces[i].obj) };
    // Two surfaces while the window server reads none; a third while it reads both.
    assert!(present(&mut p, &scene(10.0), size, scale));
    assert!(present(&mut p, &scene(20.0), size, scale));
    assert_eq!(p.surfaces.len(), 2);
    busy(&p, 0);
    busy(&p, 1);
    assert!(present(&mut p, &scene(30.0), size, scale));
    assert_eq!((p.surfaces.len(), p.front), (3, Some(2)));

    // All three busy: the frame waits and no surface changes.
    busy(&p, 2);
    let before: Vec<Vec<u8>> = (0..3).map(|i| pixels(&mut p, i)).collect();
    assert!(!present(&mut p, &scene(40.0), size, scale));
    assert!(p.starved);
    assert_eq!((0..3).map(|i| pixels(&mut p, i)).collect::<Vec<_>>(), before);

    // One comes back: the frame draws into it.
    unsafe { IOSurfaceDecrementUseCount(&p.surfaces[0].obj) };
    assert!(present(&mut p, &scene(40.0), size, scale));
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
