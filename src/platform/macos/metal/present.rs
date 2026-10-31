// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Metal renderer's window path: a frame that changed drawn in full on the GPU into a free
//! surface of the window's [Presenter] (through a texture over it, made with it), waited for and
//! shown as the CPU path shows its frames. A frame whose hash (the instances, their images, the
//! size, the scale and the glyphs' and images' generation) equals the shown one's isn't drawn. A
//! failed frame isn't shown: the window draws on the CPU from then on.

use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use super::render::Renderer;
use crate::cpu;
use crate::platform::macos::surface::Presenter;
use crate::scene::{Images, Scene, Size};
use crate::text::{FxHasher, TextSystem};

#[cfg(test)]
thread_local! {
  /// A test asks for the next frame to fail.
  static FAIL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A window's Metal renderer and the frame it shows.
pub(crate) struct Metal {
  renderer: Renderer,
  /// The hash of the frame in the shown surface.
  shown: u64,
}

impl Metal {
  /// Metal for a window, its device named on stderr; `None` when it can't be had, and why.
  pub fn new(transparent: bool) -> Option<Self> {
    let renderer = Renderer::new().map_err(|e| eprintln!("nimbo: renderer cpu, no Metal: {e}"));
    let mut renderer = renderer.ok()?;
    eprintln!("nimbo: renderer metal ({})", renderer.gpu.name());
    renderer.transparent = transparent;
    Some(Self { renderer, shown: 0 })
  }

  /// [Presenter::present] on the GPU, `self` taken out of `p` and put back unless the frame
  /// failed: then the frame isn't shown, the CPU renderer takes over for good (every glyph
  /// rasterized again, the images this keeps, then those a failed sync left in the queue) and
  /// the frame is drawn again ([Presenter::starved]).
  pub fn present(
    mut self,
    p: &mut Presenter,
    scene: &Scene,
    text: &mut TextSystem,
    images: &mut Images,
    size: Size,
    scale: f32,
  ) -> bool {
    match self.draw(p, scene, text, images, size, scale) {
      Ok(shown) => {
        p.metal = Some(self);
        shown
      }
      Err(e) => {
        eprintln!("nimbo: a Metal frame failed, drawing on the CPU from now on: {e}");
        text.reset_atlases();
        p.renderer.sync(text, &mut self.renderer.kept());
        p.renderer.sync(text, images);
        p.surfaces.iter_mut().for_each(|s| s.tex = None);
        p.starved = true;
        false
      }
    }
  }

  fn draw(
    &mut self,
    p: &mut Presenter,
    scene: &Scene,
    text: &mut TextSystem,
    images: &mut Images,
    size: Size,
    scale: f32,
  ) -> Result<bool, String> {
    self.renderer.sync(text, images)?;
    if size.w <= 0.0 || size.h <= 0.0 {
      return Ok(false);
    }

    let (w, h) = cpu::Renderer::pixel_size(size, scale);
    p.resize(w, h);
    let hash = self.hash(scene, (w, h), scale);
    if p.front.is_some() && hash == self.shown {
      return Ok(false);
    }

    let Some(i) = p.free(w, h) else { return Ok(false) };
    let s = &mut p.surfaces[i];
    let tex = match s.tex.clone() {
      Some(tex) => tex,
      None => s.tex.insert(self.renderer.target(s)?).clone(),
    };
    #[cfg(test)]
    if FAIL.take() {
      return Err("a failure a test asked for".into());
    }

    let t = Instant::now();
    let wait = self.renderer.encode(&tex, scene, size, scale)?;
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    let times = match p.trace {
      true => format!(", encode {:.3} ms, GPU wait {:.3} ms", ms(t.elapsed() - wait), ms(wait)),
      false => String::new(),
    };

    p.show(i, scale, &times);
    self.shown = hash;
    Ok(true)
  }

  /// What the frame draws, hashed: frames of one hash draw the same pixels.
  fn hash(&self, scene: &Scene, px: (usize, usize), scale: f32) -> u64 {
    let r = &self.renderer;
    let mut h = FxHasher::default();
    (px, scale.to_bits(), r.generation, r.transparent).hash(&mut h);
    for l in scene.layers() {
      l.clip.map(|c| [c.x, c.y, c.w, c.h].map(f32::to_bits)).hash(&mut h);
      [l.quads.len(), l.images.len(), l.lines.len(), l.glyphs.len(), l.emoji.len()].hash(&mut h);
      l.image_ids.hash(&mut h);
      // SAFETY: the instances are `repr(C)` structs of `f32`s with no padding: every byte is set.
      let parts = unsafe {
        [bytes(&l.quads), bytes(&l.images), bytes(&l.lines), bytes(&l.glyphs), bytes(&l.emoji)]
      };
      parts.into_iter().for_each(|b| h.write(b));
    }

    h.finish()
  }
}

impl Presenter {
  /// The view pauses: every surface but the shown one goes, so a still window keeps one.
  pub fn idle(&mut self) {
    if let (Some(_), Some(f)) = (&self.metal, self.front) {
      self.surfaces.swap(0, f);
      self.surfaces.truncate(1);
      self.front = Some(0);
    }
  }
}

/// The bytes of `v`.
///
/// # Safety
///
/// `T` has no padding: every byte of `v` is initialized.
unsafe fn bytes<T>(v: &[T]) -> &[u8] {
  unsafe { std::slice::from_raw_parts(v.as_ptr().cast(), size_of_val(v)) }
}

#[cfg(test)]
mod tests {
  use objc2_metal::MTLTexture;
  use objc2_quartz_core::CALayer;

  use super::*;
  use crate::platform::macos::metal::Gpu;
  use crate::platform::macos::surface::tests::{pixels, present, scene, shown};
  use crate::platform::macos::surface::{IOSurfaceDecrementUseCount, IOSurfaceIncrementUseCount};
  use crate::scene::{Frame, ImageId, Rect};
  use crate::text::Font;

  const SIZE: Size = Size { w: 150.0, h: 90.0 };

  fn metal() -> Presenter {
    let p = Presenter::new(CALayer::new(), false, false, true);
    assert!(p.metal.is_some(), "no Metal");
    p
  }

  /// Within the parity tolerance of the CPU's frame: no channel more than 2 off, at most 0.5%
  /// of the pixels.
  fn assert_close(got: &[u8], want: &[u8], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}");
    let max = got.iter().zip(want).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
    let n = got.chunks(4).zip(want.chunks(4)).filter(|(a, b)| a != b).count();
    assert!(max <= 2 && n * 4 * 200 <= got.len(), "{what}: max {max}, {n} pixels");
  }

  #[test]
  fn metal_surfaces_show_the_frame() {
    let (mut p, mut cpu) = (metal(), cpu::Renderer::new());
    for (x, new) in [(10.0, true), (10.0, false), (60.5, true), (61.0, true)] {
      let s = scene(x);
      assert_eq!(present(&mut p, &s, SIZE, 2.0), new, "x = {x}: shown");
      assert_close(&shown(&mut p), &cpu.snapshot(&s, SIZE, 2.0).data, &format!("x = {x}"));
    }

    assert!(!present(&mut p, &scene(61.0), SIZE, 2.0), "an unchanged frame is shown again");
    assert_eq!(p.surfaces.len(), 2);
    assert!(p.surfaces.iter().all(|s| s.tex.is_some()), "a texture per surface");
  }

  #[test]
  fn metal_leaves_busy_surfaces_alone() {
    let mut p = metal();
    let busy = |p: &Presenter, i: usize| unsafe { IOSurfaceIncrementUseCount(&p.surfaces[i].obj) };
    assert!(present(&mut p, &scene(10.0), SIZE, 2.0));
    assert!(present(&mut p, &scene(20.0), SIZE, 2.0));
    busy(&p, 0);
    busy(&p, 1);
    assert!(present(&mut p, &scene(30.0), SIZE, 2.0));
    assert_eq!((p.surfaces.len(), p.front), (3, Some(2)));

    // All three busy: the frame waits and no surface changes.
    busy(&p, 2);
    let before: Vec<Vec<u8>> = (0..3).map(|i| pixels(&mut p, i)).collect();
    assert!(!present(&mut p, &scene(40.0), SIZE, 2.0));
    assert!(p.starved);
    assert_eq!((0..3).map(|i| pixels(&mut p, i)).collect::<Vec<_>>(), before);

    // One comes back: the frame draws into it.
    unsafe { IOSurfaceDecrementUseCount(&p.surfaces[0].obj) };
    assert!(present(&mut p, &scene(40.0), SIZE, 2.0));
    assert!(!p.starved && p.front == Some(0));
    let want = cpu::Renderer::new().snapshot(&scene(40.0), SIZE, 2.0).data;
    assert_close(&shown(&mut p), &want, "after the wait");
    for i in 0..3 {
      while p.surfaces[i].in_use() {
        unsafe { IOSurfaceDecrementUseCount(&p.surfaces[i].obj) };
      }
    }
  }

  #[test]
  fn metal_makes_new_surfaces_for_a_new_size() {
    let mut p = metal();
    assert!(present(&mut p, &scene(10.0), SIZE, 2.0));
    assert!(present(&mut p, &scene(20.0), SIZE, 2.0));
    let size = Size::new(160.5, 90.0);
    assert!(present(&mut p, &scene(20.0), size, 2.0), "the same scene at a new size");
    assert_eq!((p.surfaces.len(), p.front), (1, Some(0)));
    let s = &p.surfaces[0];
    let tex = s.tex.as_ref().map(|t| (t.width(), t.height()));
    assert_eq!(((s.w, s.h), tex), ((321, 180), Some((321, 180))));
    let want = cpu::Renderer::new().snapshot(&scene(20.0), size, 2.0).data;
    assert_close(&shown(&mut p), &want, "the new size");
  }

  #[test]
  fn metal_idle_keeps_the_shown_surface() {
    let mut p = metal();
    assert!(present(&mut p, &scene(10.0), SIZE, 2.0));
    assert!(present(&mut p, &scene(20.0), SIZE, 2.0));
    let was = shown(&mut p);
    p.idle();
    assert_eq!((p.surfaces.len(), p.front, shown(&mut p)), (1, Some(0), was));
    assert!(!present(&mut p, &scene(20.0), SIZE, 2.0), "the shown frame is shown again");

    // The next frame draws in full into a new surface.
    assert!(present(&mut p, &scene(30.0), SIZE, 2.0));
    assert_eq!((p.surfaces.len(), p.front), (2, Some(1)));
    let want = cpu::Renderer::new().snapshot(&scene(30.0), SIZE, 2.0).data;
    assert_close(&shown(&mut p), &want, "after idle");
  }

  /// `scene(x)` with a line of text and an image, as a view builds it.
  fn build(x: f32, text: &mut TextSystem, images: &mut Images, id: ImageId) -> Scene {
    let mut scene = scene(x);
    let now = Duration::ZERO;
    let mut f = Frame {
      size: SIZE,
      scale: 2.0,
      mouse: None,
      now,
      scene: &mut scene,
      text,
      images,
      animate: false,
    };
    f.text(10.0, 75.0, "Metal 1234", Font::ui(13.0), [1.0; 4]);
    f.image(id, Rect::new(110.0, 10.0, 30.0, 30.0), 1.0);
    scene
  }

  #[test]
  fn metal_is_made_only_for_a_metal_window() {
    // The CPU path, with the feature built in: no device on this thread.
    let mut p = Presenter::new(CALayer::new(), false, false, false);
    assert!(present(&mut p, &scene(10.0), SIZE, 2.0));
    assert!(!Gpu::made(), "the CPU path made a Metal device");

    // A Metal window makes it; its CPU renderer then holds no atlas or image pixels.
    let mut p = metal();
    assert!(Gpu::made());
    let (mut text, mut images) = (TextSystem::new(2.0), Images::default());
    let id = images.add(1, 1, &[255; 4]);
    let s = build(10.0, &mut text, &mut images, id);
    assert!(p.present(&s, &mut text, &mut images, SIZE, 2.0));
    let (uploads, kept) = p.renderer.contents();
    assert!(uploads.is_empty() && kept.added.is_empty(), "the CPU renderer holds pixels");
  }

  #[test]
  fn metal_falls_back_to_the_cpu_for_good() {
    let mut p = metal();
    let (mut text, mut images) = (TextSystem::new(2.0), Images::default());
    let rgba: Vec<u8> = (0..16u8).flat_map(|i| [i * 16, 255 - i * 16, 128, 255]).collect();
    let id = images.add(4, 4, &rgba);
    let s = build(10.0, &mut text, &mut images, id);
    assert!(p.present(&s, &mut text, &mut images, SIZE, 2.0));
    let was = shown(&mut p);

    // A failed frame: not shown, the CPU from now on, and the frame asked for again.
    FAIL.set(true);
    let s = build(20.0, &mut text, &mut images, id);
    assert!(!p.present(&s, &mut text, &mut images, SIZE, 2.0));
    assert!(p.metal.is_none() && p.starved);
    assert_eq!(shown(&mut p), was, "the failed frame isn't shown");

    // Built again, on the CPU: as a CPU renderer synced from the start draws it.
    let s = build(20.0, &mut text, &mut images, id);
    assert!(s.layers()[0].glyphs.len() >= 9 && s.layers()[0].images.len() == 1);
    assert!(p.present(&s, &mut text, &mut images, SIZE, 2.0));
    let (mut text, mut images) = (TextSystem::new(2.0), Images::default());
    let id = images.add(4, 4, &rgba);
    let s = build(20.0, &mut text, &mut images, id);
    let mut cpu = cpu::Renderer::new();
    cpu.sync(&mut text, &mut images);
    assert!(shown(&mut p) == cpu.snapshot(&s, SIZE, 2.0).data, "the CPU's frame");
  }
}
