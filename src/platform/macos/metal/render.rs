// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Metal renderer's drawing: the scene encoded as one render pass into an IOSurface's
//! texture, one instanced draw per kind of primitive per layer (and per run of one image) under
//! the layer's scissor, then waited for: one frame in flight, so one instance buffer and no
//! upload races a frame. Glyph atlases keep their full width and grow in height as they fill;
//! images keep their pixels too, for the CPU renderer should a frame fail ([super::present]).

use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::{Duration, Instant};

use objc2::encode::{Encoding, RefEncode};
use objc2::msg_send;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_core_foundation::CFType;
use objc2_metal::{
  MTLBlitCommandEncoder, MTLBuffer, MTLClearColor, MTLCommandBuffer, MTLCommandBufferStatus,
  MTLCommandEncoder, MTLCommandQueue, MTLDevice, MTLLoadAction, MTLOrigin, MTLPixelFormat,
  MTLPrimitiveType, MTLRegion, MTLRenderCommandEncoder, MTLRenderPassDescriptor,
  MTLRenderPipelineState, MTLResourceOptions, MTLScissorRect, MTLSize, MTLStorageMode,
  MTLStoreAction, MTLTexture, MTLTextureDescriptor, MTLTextureUsage,
};

use super::Gpu;
use crate::cpu::raster::IRect;
use crate::cpu::{self, Pixels};
use crate::platform::macos::surface::Surface;
use crate::scene::{ImageUpload, Images, Layer, Quad, Rect, Scene, Segment, Size, Sprite};
use crate::text::{ATLAS_SIZE, COLOR_ATLAS_SIZE, TextSystem, Upload};

/// Rows a new atlas texture starts with, coverage and colour (256 KB each), doubled as glyphs
/// reach past them, up to the atlas's height.
const ATLAS_ROWS: [usize; 2] = [128, 64];
/// The instance buffer's least length; it grows to a power of two.
const MIN_BUFFER: usize = 64 * 1024;
/// Each kind's instances start at a multiple of this in the buffer.
const ALIGN: usize = 256;

pub(crate) type Texture = ProtocolObject<dyn MTLTexture>;
/// A render target over a window's surface.
pub(crate) type Target = Retained<Texture>;
type Buffer = ProtocolObject<dyn MTLBuffer>;
type Pipeline = ProtocolObject<dyn MTLRenderPipelineState>;

/// Per-frame constants: `Globals` in the shaders.
#[repr(C)]
struct Globals {
  target: [f32; 2],
  scale: f32,
  _pad: f32,
}

/// `IOSurfaceRef` as Metal's selector takes it (`^{__IOSurface=}`), for the one call that needs
/// it.
#[repr(C)]
struct IOSurfaceRef([u8; 0]);

// SAFETY: the encoding of a pointer to the opaque `struct __IOSurface`, as IOSurfaceRef.h has it.
unsafe impl RefEncode for IOSurfaceRef {
  const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("__IOSurface", &[]));
}

pub(crate) struct Renderer {
  pub(super) gpu: Rc<Gpu>,
  /// Clears to transparent instead of opaque black: an overlay's frame.
  pub transparent: bool,
  /// The coverage and colour atlases, made at their first upload.
  atlases: [Option<Retained<Texture>>; 2],
  /// The atlas epoch each atlas holds: coverage, colour.
  epochs: [u32; 2],
  /// Each image's texture and its pixels as added, kept for the CPU renderer should it take over.
  images: Vec<Option<(Retained<Texture>, ImageUpload)>>,
  /// Bumped when atlas or image pixels change under the same coordinates (an atlas reset, an
  /// image id reused): a frame drawn before differs even with the same instances.
  pub generation: u64,
  buffer: Option<Retained<Buffer>>,
}

impl Renderer {
  /// A renderer on the calling thread's [Gpu]; why there is none.
  pub fn new() -> Result<Self, String> {
    let gpu = Gpu::get()?;
    let (atlases, epochs, images, buffer) = ([None, None], [0; 2], Vec::new(), None);
    Ok(Self { gpu, transparent: false, atlases, epochs, images, generation: 0, buffer })
  }

  /// Copies new glyph bitmaps into the atlases and adds or drops images, draining both queues;
  /// on an error they are left for the renderer that takes over.
  pub fn sync(&mut self, text: &mut TextSystem, images: &mut Images) -> Result<(), String> {
    self.sync_pending(text.pending(), images)?;
    text.uploads().for_each(drop);
    images.added.clear();
    images.removed.clear();
    Ok(())
  }

  /// [Self::sync] without draining, for a renderer that syncs beside another.
  pub fn sync_pending(&mut self, uploads: &[Upload], images: &Images) -> Result<(), String> {
    for u in uploads {
      let (i, bpp) = if u.color { (1, 4) } else { (0, 1) };
      let tex = self.reserve(i, u.y + u.h)?;
      replace(&tex, (u.x, u.y, u.w, u.h), bpp, &u.data);
      if u.epoch != self.epochs[i] {
        self.epochs[i] = u.epoch;
        self.generation += 1;
      }
    }

    for id in &images.removed {
      if let Some(t) = self.images.get_mut(id.0 as usize) {
        *t = None;
      }

      self.generation += 1;
    }

    for u in &images.added {
      let i = u.id.0 as usize;
      match self.images.len() <= i {
        true => self.images.resize_with(i + 1, || None),
        false => self.generation += 1,
      }

      let format = MTLPixelFormat::RGBA8Unorm;
      let tex = (u.w > 0 && u.h > 0).then(|| self.texture(format, u.w, u.h)).transpose()?;
      self.images[i] = tex.map(|tex| {
        replace(&tex, (0, 0, u.w, u.h), 4, &u.data);
        (tex, u.clone())
      });
    }

    Ok(())
  }

  /// The images it keeps, as added: what the CPU renderer needs to take over.
  pub fn kept(&self) -> Images {
    let mut images = Images::default();
    images.added = self.images.iter().flatten().map(|(_, u)| u.clone()).collect();
    images
  }

  /// A texture of the atlas `i` with at least `rows` rows: made zeroed, or grown by doubling its
  /// height, the rows it had copied over and the new ones zeroed.
  fn reserve(&mut self, i: usize, rows: usize) -> Result<Retained<Texture>, String> {
    let had = self.atlases[i].as_ref().map_or(0, |t| t.height());
    if let Some(t) = &self.atlases[i]
      && rows <= had
    {
      return Ok(t.clone());
    }

    let (w, bpp, format) = match i {
      0 => (ATLAS_SIZE, 1, MTLPixelFormat::R8Unorm),
      _ => (COLOR_ATLAS_SIZE, 4, MTLPixelFormat::RGBA8Unorm),
    };

    let mut h = had.max(ATLAS_ROWS[i]);
    while h < rows.min(w) {
      h *= 2;
    }

    let tex = self.texture(format, w, h)?;
    replace(&tex, (0, had, w, tex.height() - had), bpp, &vec![0; w * (tex.height() - had) * bpp]);
    if let Some(old) = self.atlases[i].take() {
      let cmd = self.gpu.queue.commandBuffer().ok_or("no Metal command buffer")?;
      let blit = cmd.blitCommandEncoder().ok_or("no Metal blit encoder")?;
      let origin = MTLOrigin { x: 0, y: 0, z: 0 };
      let size = MTLSize { width: w, height: had, depth: 1 };
      // SAFETY: both textures are 2D with one slice and level, the region within each.
      unsafe {
        blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
          &old, 0, 0, origin, size, &tex, 0, 0, origin,
        )
      };
      if !self.gpu.apple {
        blit.synchronizeResource(ProtocolObject::from_ref(&*tex));
      }

      blit.endEncoding();
      // Uploads into the new rows go after the copy, which must not overwrite them.
      finish(&cmd)?;
    }

    self.atlases[i] = Some(tex.clone());
    Ok(tex)
  }

  /// A texture the shaders sample: `Shared` on an Apple-family GPU, else `Managed`.
  fn texture(
    &self,
    format: MTLPixelFormat,
    w: usize,
    h: usize,
  ) -> Result<Retained<Texture>, String> {
    let desc = descriptor(format, w, h, MTLTextureUsage::ShaderRead, self.storage());
    self.gpu.device.newTextureWithDescriptor(&desc).ok_or_else(|| format!("no {w}x{h} texture"))
  }

  fn storage(&self) -> MTLStorageMode {
    if self.gpu.apple { MTLStorageMode::Shared } else { MTLStorageMode::Managed }
  }

  /// A render target over the surface's pixels, which it keeps alive.
  pub(super) fn target(&self, s: &Surface) -> Result<Retained<Texture>, String> {
    let (format, usage) = (MTLPixelFormat::BGRA8Unorm, MTLTextureUsage::RenderTarget);
    let desc = descriptor(format, s.w, s.h, usage, self.storage());
    let io = (&*s.obj as *const CFType).cast::<IOSurfaceRef>();
    let device: &super::Device = &self.gpu.device;
    // SAFETY: `obj` is an IOSurface of `w` x `h` BGRA pixels, as the descriptor says; the method
    // returns a +1 reference or nil.
    let tex: Option<Retained<Texture>> =
      unsafe { msg_send![device, newTextureWithDescriptor: &*desc, iosurface: io, plane: 0usize] };
    tex.ok_or_else(|| format!("no texture over a {}x{} IOSurface", s.w, s.h))
  }

  /// Draws the scene into `target` (its pixel size the frame's: [cpu::Renderer::pixel_size])
  /// and waits for the GPU; how long it waited, or the command buffer's error if it failed.
  pub fn encode(
    &mut self,
    target: &Texture,
    scene: &Scene,
    size: Size,
    scale: f32,
  ) -> Result<Duration, String> {
    autoreleasepool(|_| self.pass(target, scene, size, scale))
  }

  /// [Self::encode] within an autorelease pool.
  fn pass(
    &mut self,
    target: &Texture,
    scene: &Scene,
    size: Size,
    scale: f32,
  ) -> Result<Duration, String> {
    let bytes = |n: usize, item: usize| (n * item).next_multiple_of(ALIGN);
    let (quad, sprite, seg) = (size_of::<Quad>(), size_of::<Sprite>(), size_of::<Segment>());
    let len = |l: &Layer| {
      let sprites: usize =
        [&l.images, &l.glyphs, &l.emoji].iter().map(|s| bytes(s.len(), sprite)).sum();
      bytes(l.quads.len(), quad) + bytes(l.lines.len(), seg) + sprites
    };
    let buf = self.instances(scene.layers().iter().map(len).sum())?;
    let base = buf.contents().as_ptr().cast::<u8>();

    let pass = MTLRenderPassDescriptor::renderPassDescriptor();
    // SAFETY: a pass has colour attachment 0.
    let att = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
    att.setTexture(Some(target));
    att.setLoadAction(MTLLoadAction::Clear);
    att.setStoreAction(MTLStoreAction::Store);
    let alpha = if self.transparent { 0.0 } else { 1.0 };
    att.setClearColor(MTLClearColor { red: 0.0, green: 0.0, blue: 0.0, alpha });
    let cmd = self.gpu.queue.commandBuffer().ok_or("no Metal command buffer")?;
    let enc = cmd.renderCommandEncoderWithDescriptor(&pass).ok_or("no Metal render encoder")?;
    let (w, h) = (target.width(), target.height());
    let globals = Globals { target: [w as f32, h as f32], scale, _pad: 0.0 };
    let ptr = NonNull::from(&globals).cast::<c_void>();
    // SAFETY: the bytes are copied into the command buffer now.
    unsafe {
      enc.setVertexBytes_length_atIndex(ptr, size_of::<Globals>(), 0);
      enc.setFragmentBytes_length_atIndex(ptr, size_of::<Globals>(), 0);
    }

    let mut offset = 0;
    // Copies `count` instances into the buffer; returns their offset.
    let mut copy = |src: *const u8, count: usize, item: usize| {
      let at = offset;
      // SAFETY: `instances` made the buffer at least `len` bytes, every layer's instances.
      unsafe { std::ptr::copy_nonoverlapping(src, base.add(at), count * item) };
      offset += bytes(count, item);
      at
    };
    let draw = |pipeline: &Pipeline, tex: Option<&Texture>, at: usize, count: usize| {
      enc.setRenderPipelineState(pipeline);
      // SAFETY: `at` and `count` lie within the buffer; the shaders read `count` instances.
      unsafe {
        enc.setVertexBuffer_offset_atIndex(Some(&buf), at, 1);
        enc.setFragmentBuffer_offset_atIndex(Some(&buf), at, 1);
        enc.setFragmentTexture_atIndex(tex, 0);
        enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
          MTLPrimitiveType::TriangleStrip,
          0,
          4,
          count,
        );
      }
    };
    let frame = IRect::new(0, 0, w as i32, h as i32);
    let full = Rect::new(0.0, 0.0, size.w, size.h);
    let gpu = &self.gpu;
    for layer in scene.layers() {
      let r = cpu::scissor(layer.clip.unwrap_or(full), scale, frame);
      if r.is_empty() {
        continue;
      }

      let (x, y) = (r.x0 as usize, r.y0 as usize);
      let (width, height) = ((r.x1 - r.x0) as usize, (r.y1 - r.y0) as usize);
      enc.setScissorRect(MTLScissorRect { x, y, width, height });
      if !layer.quads.is_empty() {
        let at = copy(layer.quads.as_ptr().cast(), layer.quads.len(), quad);
        draw(&gpu.quads, None, at, layer.quads.len());
      }

      if !layer.images.is_empty() {
        let at = copy(layer.images.as_ptr().cast(), layer.images.len(), sprite);
        // One draw per run of sprites of one image.
        for (i, run) in runs(&layer.image_ids) {
          if let Some(Some((tex, _))) = self.images.get(layer.image_ids[i].0 as usize) {
            draw(&gpu.images, Some(tex), at + i * sprite, run);
          }
        }
      }

      if !layer.lines.is_empty() {
        let at = copy(layer.lines.as_ptr().cast(), layer.lines.len(), seg);
        draw(&gpu.lines, None, at, layer.lines.len());
      }

      for (sprites, i, pipeline) in
        [(&layer.glyphs, 0, &gpu.glyphs), (&layer.emoji, 1, &gpu.images)]
      {
        if let Some(tex) = &self.atlases[i]
          && !sprites.is_empty()
        {
          let at = copy(sprites.as_ptr().cast(), sprites.len(), sprite);
          draw(pipeline, Some(tex), at, sprites.len());
        }
      }
    }

    enc.endEncoding();
    finish(&cmd)
  }

  /// The instance buffer, grown to a power of two when the frame needs more than `len` bytes.
  fn instances(&mut self, len: usize) -> Result<Retained<Buffer>, String> {
    if !self.buffer.as_ref().is_some_and(|b| b.length() >= len) {
      let len = len.max(MIN_BUFFER).next_power_of_two();
      let opts =
        MTLResourceOptions::StorageModeShared | MTLResourceOptions::CPUCacheModeWriteCombined;
      self.buffer = self.gpu.device.newBufferWithLength_options(len, opts);
    }

    self.buffer.clone().ok_or_else(|| format!("no Metal buffer of {len} bytes"))
  }

  /// Renders the whole frame into an IOSurface of its own and reads its pixels back.
  pub fn snapshot(&mut self, scene: &Scene, size: Size, scale: f32) -> Result<Pixels, String> {
    let (w, h) = cpu::Renderer::pixel_size(size, scale);
    let mut s = Surface::new(w, h).ok_or("no IOSurface")?;
    let target = self.target(&s)?;
    self.encode(&target, scene, size, scale)?;
    drop(target);
    let mut data = Vec::with_capacity(w * h * 4);
    s.with_pixels(|px, stride| {
      for row in px.chunks(stride).take(h) {
        data.extend(row[..w].iter().flat_map(|p| p.to_le_bytes()));
      }
    });
    Ok(Pixels { w, h, data })
  }
}

/// Runs of one id: the index each starts at and its length.
fn runs<T: PartialEq>(ids: &[T]) -> impl Iterator<Item = (usize, usize)> {
  let mut i = 0;
  std::iter::from_fn(move || {
    let start = i;
    let n = ids.get(start..)?.iter().take_while(|&id| *id == ids[start]).count();
    i += n;
    (n > 0).then_some((start, n))
  })
}

/// Commits the command buffer and waits for it: how long, or its error if it failed.
fn finish(cmd: &ProtocolObject<dyn MTLCommandBuffer>) -> Result<Duration, String> {
  cmd.commit();
  let t = Instant::now();
  cmd.waitUntilCompleted();
  match cmd.status() {
    MTLCommandBufferStatus::Completed => Ok(t.elapsed()),
    s => Err(cmd.error().map_or(format!("{s:?}"), |e| e.localizedDescription().to_string())),
  }
}

/// Copies rows of `bpp`-byte pixels into the region `(x, y, w, h)` of `tex`.
fn replace(tex: &Texture, (x, y, w, h): (usize, usize, usize, usize), bpp: usize, data: &[u8]) {
  if w == 0 || h == 0 {
    return;
  }

  let region =
    MTLRegion { origin: MTLOrigin { x, y, z: 0 }, size: MTLSize { width: w, height: h, depth: 1 } };
  let bytes = NonNull::new(data.as_ptr().cast_mut().cast::<c_void>()).unwrap();
  // SAFETY: `data` holds `h` rows of `w * bpp` bytes, the region lies within the texture.
  unsafe { tex.replaceRegion_mipmapLevel_withBytes_bytesPerRow(region, 0, bytes, w * bpp) };
}

fn descriptor(
  format: MTLPixelFormat,
  w: usize,
  h: usize,
  usage: MTLTextureUsage,
  storage: MTLStorageMode,
) -> Retained<MTLTextureDescriptor> {
  // SAFETY: a 2D texture of a valid format and a non-zero size.
  let desc = unsafe {
    MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
      format, w, h, false,
    )
  };
  desc.setUsage(usage);
  desc.setStorageMode(storage);
  desc
}

#[cfg(test)]
mod tests {
  use std::mem::offset_of;

  use super::*;
  use crate::cpu::raster::{pack, premul};
  use crate::scene::{Frame, ImageId, Point};
  use crate::text::Font;

  fn renderer() -> Renderer {
    Renderer::new().unwrap_or_else(|e| panic!("Metal: {e}"))
  }

  /// A frame's pixels, packed as the CPU renderer's.
  fn packed(p: &Pixels) -> Vec<u32> {
    p.data.as_chunks::<4>().0.iter().map(|&b| u32::from_le_bytes(b)).collect()
  }

  /// The largest channel difference, the share of pixels that differ and the 99th percentile
  /// of their largest channel difference.
  fn parity(a: &Pixels, b: &Pixels) -> (u8, f64, u8) {
    assert_eq!((a.w, a.h), (b.w, b.h));
    let mut diffs: Vec<u8> = (a.data.as_chunks::<4>().0.iter().zip(b.data.as_chunks::<4>().0))
      .map(|(p, q)| (0..4).map(|i| p[i].abs_diff(q[i])).max().unwrap())
      .filter(|&d| d > 0)
      .collect();
    diffs.sort_unstable();
    let p99 = diffs.get(diffs.len() * 99 / 100).copied().unwrap_or(0);
    let share = diffs.len() as f64 / (a.w * a.h) as f64;
    (diffs.last().copied().unwrap_or(0), share, p99)
  }

  /// Within the parity tolerance: no channel more than 2 off, at most `share` of the pixels.
  fn assert_close(cpu: &Pixels, gpu: &Pixels, share: f64, what: &str) {
    let (max, got, p99) = parity(cpu, gpu);
    println!("{what}: max {max}, {:.3}% of pixels, p99 {p99}", got * 100.0);
    assert!(max <= 2 && got <= share, "{what}: max {max}, {:.3}% of pixels", got * 100.0);
  }

  fn draw(scene: &Scene, w: f32, h: f32, transparent: bool) -> Vec<u32> {
    let mut r = renderer();
    r.transparent = transparent;
    packed(&r.snapshot(scene, Size::new(w, h), 1.0).unwrap())
  }

  #[test]
  fn instances_match_the_shaders_structs() {
    // Offsets of the MSL structs: float4 at 16-byte boundaries, float2 at 8.
    assert_eq!(
      (size_of::<Quad>(), offset_of!(Quad, blur), offset_of!(Quad, gradient)),
      (96, 80, 84)
    );
    let quad = [offset_of!(Quad, color), offset_of!(Quad, border_color), offset_of!(Quad, radii)];
    assert_eq!((quad, offset_of!(Quad, borders)), ([16, 32, 48], 64));
    assert_eq!(
      (size_of::<Sprite>(), offset_of!(Sprite, uv), offset_of!(Sprite, color)),
      (48, 16, 32)
    );
    let seg = [offset_of!(Segment, b), offset_of!(Segment, color), offset_of!(Segment, width)];
    assert_eq!((size_of::<Segment>(), seg, offset_of!(Segment, round)), (48, [8, 16, 32], 36));
    assert_eq!((size_of::<Globals>(), offset_of!(Globals, scale)), (16, 8));
  }

  #[test]
  fn borders_that_meet_leave_no_fill() {
    let (fill, border) = ([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
    let mut s = Scene::default();
    s.quad(Quad::new(Rect::new(0.0, 0.0, 5.0, 5.0), fill).border(3.0, border));
    assert_eq!(draw(&s, 5.0, 5.0, true)[12], pack(premul(border)));
  }

  #[test]
  fn uneven_borders_round_the_inner_corner_they_border() {
    let (fill, border) = ([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
    let q = Quad::new(Rect::new(0.0, 0.0, 20.0, 20.0), fill).radius(10.0);
    let mut s = Scene::default();
    s.quad(q.borders([0.0, 0.0, 0.0, 8.0], border));
    let px = draw(&s, 20.0, 20.0, true);
    for (y, row) in px.chunks(20).enumerate().take(4) {
      assert!(row[9..].iter().all(|p| p & 0xff == 0), "row {y}: {:08x?}", &row[9..]);
    }
  }

  #[test]
  fn empty_quads_draw_nothing() {
    let mut s = Scene::default();
    for rect in [Rect::new(1.5, 1.0, 0.0, 4.0), Rect::new(1.0, 1.5, 4.0, 0.0)] {
      s.quad(Quad::new(rect, [1.0; 4]));
      s.quad(Quad::new(rect, [1.0; 4]).blur(1.0));
    }

    assert_eq!(draw(&s, 5.0, 5.0, true), [0; 25]);
  }

  #[test]
  fn a_point_draws_only_with_round_caps() {
    let a = Point::new(2.5, 2.5);
    let seg = |round| Segment { a, b: a, color: [1.0; 4], width: 2.0, round, _pad: [0.0; 2] };
    let point = |round| {
      let mut s = Scene::default();
      s.segment(seg(round));
      draw(&s, 5.0, 5.0, true)
    };
    assert_eq!(point(0.0), [0; 25]);
    assert_eq!(point(1.0)[12], !0);
  }

  #[test]
  fn metal_draws_as_the_cpu() {
    let (mut cpu, mut gpu) = (cpu::Renderer::new(), renderer());
    let fractional = [(Size::new(150.7, 90.3), 1.0), (Size::new(150.7, 90.3), 1.5)];
    let sizes = [
      (Size::new(300.0, 200.0), 1.0),
      (Size::new(300.0, 200.0), 1.5),
      (Size::new(300.0, 200.0), 2.0),
    ];
    for (size, scale) in sizes.into_iter().chain(fractional) {
      for t in [0.0, 3.25, 40.0, 41.5] {
        let s = cpu::tests::scene(t);
        let want = cpu.snapshot(&s, size, scale);
        let got = gpu.snapshot(&s, size, scale).unwrap();
        assert_close(&want, &got, 0.005, &format!("{size:?} at {scale}, t = {t}"));
      }
    }
  }

  #[test]
  fn a_transparent_frame_clears_to_nothing() {
    let mut s = Scene::default();
    s.quad(Quad::new(Rect::new(0.0, 0.0, 10.0, 10.0), [1.0, 0.0, 0.0, 0.5]));
    let px = draw(&s, 20.0, 10.0, true);
    assert_eq!(px[15], 0);
    let [b, g, r, a] = px[5].to_le_bytes();
    assert_eq!((b, g), (0, 0));
    assert!(a.abs_diff(128) <= 1 && r.abs_diff(a) <= 1, "premultiplied: {r} of {a}");
  }

  #[test]
  fn image_removed_before_its_first_sync() {
    let mut r = renderer();
    let (mut text, mut images) = (TextSystem::new(2.0), Images::default());
    let a = images.add(1, 1, &[255; 4]);
    images.remove(a);
    let b = images.add(2, 1, &[255; 8]);
    let c = images.add(1, 1, &[255; 4]);
    images.remove(c);
    r.sync(&mut text, &mut images).unwrap();
    assert_eq!((a, r.images.len()), (b, 1));
    assert!(r.images[0].is_some() && images.added.is_empty());
  }

  /// A 2x2 coverage glyph at `y` in the atlas, drawn at the frame's top left.
  fn glyph(r: &mut Renderer, epoch: u32, y: usize, value: u8) -> u32 {
    let u = Upload { color: false, epoch, x: 4, y, w: 2, h: 2, data: vec![value; 4] };
    r.sync_pending(&[u], &Images::default()).unwrap();
    let mut s = Scene::default();
    let uv = Rect::new(4.0, y as f32, 2.0, 2.0);
    s.glyph(Sprite { rect: Rect::new(0.0, 0.0, 2.0, 2.0), uv, color: [1.0; 4] });
    packed(&r.snapshot(&s, Size::new(4.0, 4.0), 1.0).unwrap())[0]
  }

  #[test]
  fn an_atlas_reset_redraws_the_reused_slot() {
    let mut r = renderer();
    assert_eq!(glyph(&mut r, 0, 0, 255), !0);
    let generation = r.generation;
    // A new epoch: the slot holds another glyph, and frames drawn before differ.
    assert_eq!(glyph(&mut r, 1, 0, 0), 0xff00_0000);
    assert!(r.generation > generation);
  }

  #[test]
  fn an_atlas_grows_and_keeps_its_glyphs() {
    let mut r = renderer();
    glyph(&mut r, 0, 0, 255);
    let atlas = |r: &Renderer| r.atlases[0].as_ref().map(|t| (t.width(), t.height()));
    assert_eq!(atlas(&r), Some((ATLAS_SIZE, ATLAS_ROWS[0])));
    // Past the first rows: twice the height, the first glyph still there.
    assert_eq!(glyph(&mut r, 0, 200, 128), 0xff80_8080);
    assert_eq!(atlas(&r), Some((ATLAS_SIZE, 256)));
    assert_eq!(glyph(&mut r, 0, 0, 255), !0);
    let mut s = Scene::default();
    let uv = Rect::new(0.0, 100.0, 2.0, 2.0);
    s.glyph(Sprite { rect: Rect::new(0.0, 0.0, 2.0, 2.0), uv, color: [1.0; 4] });
    let px = packed(&r.snapshot(&s, Size::new(4.0, 4.0), 1.0).unwrap());
    assert_eq!(px[0], 0xff00_0000, "the new rows are zeroed");
  }

  #[test]
  fn atlases_start_small_and_the_colour_one_with_the_first_emoji() {
    let mut r = renderer();
    let (mut text, mut images) = (TextSystem::new(2.0), Images::default());
    let sizes =
      |r: &Renderer| r.atlases.each_ref().map(|t| t.as_ref().map(|t| (t.width(), t.height())));
    assert_eq!(sizes(&r), [None, None], "atlases before the first glyph");
    let mut line = |r: &mut Renderer, s: &str| {
      let mut scene = Scene::default();
      let mut f = Frame {
        size: Size::new(400.0, 40.0),
        scale: 2.0,
        mouse: None,
        now: Duration::ZERO,
        scene: &mut scene,
        text: &mut text,
        images: &mut images,
        animate: false,
      };
      f.text(10.0, 30.0, s, Font::ui(13.0), [1.0; 4]);
      r.sync(&mut text, &mut images).unwrap();
    };

    // Text: the coverage atlas at its first rows, no colour atlas.
    line(&mut r, "The quick brown fox jumps over the lazy dog 0123456789");
    assert_eq!(sizes(&r), [Some((ATLAS_SIZE, ATLAS_ROWS[0])), None]);
    // The first emoji makes the colour atlas, at its first rows too.
    line(&mut r, "Done \u{1F600}");
    assert_eq!(sizes(&r)[1], Some((COLOR_ATLAS_SIZE, ATLAS_ROWS[1])));
  }

  /// A 26x37 sprite, opaque inside a soft-edged ellipse, its colour across it.
  fn bunny() -> Vec<u8> {
    let (w, h) = (26, 37);
    let mut px = Vec::with_capacity(w * h * 4);
    for y in 0..h {
      for x in 0..w {
        let (u, v) =
          ((x as f32 + 0.5) / w as f32 * 2.0 - 1.0, (y as f32 + 0.5) / h as f32 * 2.0 - 1.0);
        let a = ((1.0 - (u * u + v * v).sqrt()) * 8.0).clamp(0.0, 1.0);
        px.extend([(x * 9) as u8, (y * 6) as u8, 200, (a * 255.0) as u8]);
      }
    }

    px
  }

  #[test]
  fn metal_draws_ten_thousand_sprites_as_the_cpu() {
    let (mut cpu, mut gpu) = (cpu::Renderer::new(), renderer());
    let (mut text, mut images) = (TextSystem::new(1.0), Images::default());
    let id: ImageId = images.add(26, 37, &bunny());
    gpu.sync_pending(text.pending(), &images).unwrap();
    cpu.sync(&mut text, &mut images);
    let mut s = Scene::default();
    s.quad(Quad::new(Rect::new(0.0, 0.0, 800.0, 600.0), [0.9, 0.9, 0.95, 1.0]));
    let mut seed = 7u32;
    let mut rand = |n: u32| {
      seed ^= seed << 13;
      seed ^= seed >> 17;
      seed ^= seed << 5;
      (seed % (n * 100)) as f32 / 100.0
    };
    for _ in 0..10_000 {
      let rect = Rect::new(rand(780) - 6.0, rand(570) - 6.0, 26.0, 37.0);
      s.image(id, Sprite { rect, uv: Rect::new(0.0, 0.0, 26.0, 37.0), color: [1.0; 4] });
    }

    let size = Size::new(800.0, 600.0);
    let want = cpu.snapshot(&s, size, 1.0);
    let got = gpu.snapshot(&s, size, 1.0).unwrap();
    let (max, share, p99) = parity(&want, &got);
    println!("10,000 sprites: max {max}, {:.3}% of pixels, p99 {p99}", share * 100.0);
    // The GPU's bilinear weights are coarser than `sample`'s: at positions other than quarters
    // of a pixel, a few pixels of each sprite differ by 1 (5.4% of this frame on an M2).
    assert!(max <= 2 && share <= 0.08 && p99 <= 1, "max {max}, {:.3}%, p99 {p99}", share * 100.0);
  }

  /// The probe for a new system (the macOS VM): a render target over an IOSurface of an even and
  /// an odd width, the window server's view of it free once the GPU is done.
  #[test]
  fn metal_renders_into_an_iosurface() {
    let (mut cpu, mut gpu) = (cpu::Renderer::new(), renderer());
    println!("device: {}, Apple family: {}", gpu.gpu.name(), gpu.gpu.apple);
    for w in [300, 301] {
      let size = Size::new(w as f32, 200.0);
      let s = cpu::tests::scene(7.0);
      let mut surface = Surface::new(w, 200).unwrap();
      let target = gpu.target(&surface).unwrap();
      gpu.encode(&target, &s, size, 1.0).unwrap();
      assert!(!surface.in_use(), "{w} px: IOSurfaceIsInUse after the wait");
      let mut data = Vec::new();
      surface.with_pixels(|px, stride| {
        println!("{w} px: {} bytes per row", stride * 4);
        for row in px.chunks(stride).take(200) {
          data.extend(row[..w].iter().flat_map(|p| p.to_le_bytes()));
        }
      });
      drop(target);
      let got = Pixels { w, h: 200, data };
      assert_close(&cpu.snapshot(&s, size, 1.0), &got, 0.005, &format!("{w} px"));
    }
  }

  #[test]
  fn runs_of_one_id() {
    assert_eq!(runs(&[1, 1, 2, 1]).collect::<Vec<_>>(), [(0, 2), (2, 1), (3, 1)]);
    assert_eq!(runs::<u8>(&[]).count(), 0);
  }
}
