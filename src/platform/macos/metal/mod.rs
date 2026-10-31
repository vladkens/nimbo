// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The Metal renderer, built with the `metal` feature and chosen per window with
//! `NIMBO_RENDERER=metal`: the scene drawn on the GPU into the same IOSurfaces the CPU renderer
//! fills ([render]). Its shaders (`shaders.metal`) are the twin of [crate::cpu::raster], function
//! by function, so both draw the same pixels; the CPU renderer stays the default and the
//! reference. The device, its queue and the pipelines ([Gpu]) are made once per thread, on the
//! first Metal frame, so an app that never draws with Metal never touches it.

#![allow(dead_code, reason = "only its tests draw with it until the harness and windows do")]

pub(crate) mod render;

use std::cell::OnceCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::{
  MTLBlendFactor, MTLCommandQueue, MTLCompileOptions, MTLCreateSystemDefaultDevice, MTLDevice,
  MTLLanguageVersion, MTLLibrary, MTLPixelFormat, MTLRenderPipelineDescriptor,
  MTLRenderPipelineState,
};

const SHADERS: &str = include_str!("shaders.metal");

pub(crate) type Device = ProtocolObject<dyn MTLDevice>;
type Pipeline = ProtocolObject<dyn MTLRenderPipelineState>;

/// A thread's Metal device, its queue and the four pipelines, all premultiplied blending
/// (One, OneMinusSourceAlpha) into `BGRA8Unorm`: no `_sRGB`, as the CPU renderer blends in sRGB
/// space.
pub(crate) struct Gpu {
  pub device: Retained<Device>,
  pub queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
  /// The GPU shares the CPU's memory: textures it samples are `Shared`, else `Managed`.
  pub unified: bool,
  pub quads: Retained<Pipeline>,
  pub lines: Retained<Pipeline>,
  pub glyphs: Retained<Pipeline>,
  pub images: Retained<Pipeline>,
}

thread_local! {
  static GPU: OnceCell<Result<Rc<Gpu>, String>> = const { OnceCell::new() };
}

impl Gpu {
  /// The calling thread's, made on first use; why there is none (no device, a shader that
  /// doesn't compile), once per thread.
  pub fn get() -> Result<Rc<Gpu>, String> {
    GPU.with(|g| g.get_or_init(|| objc2::rc::autoreleasepool(|_| Self::new()).map(Rc::new)).clone())
  }

  /// The device's name, for the line naming the renderer (`nimbo: renderer metal (<name>)`).
  pub fn name(&self) -> String {
    self.device.name().to_string()
  }

  fn new() -> Result<Self, String> {
    let device = MTLCreateSystemDefaultDevice().ok_or("no Metal device")?;
    let queue = device.newCommandQueue().ok_or("no Metal command queue")?;
    let options = MTLCompileOptions::new();
    // macOS 11's language, so every system compiles the same one.
    options.setLanguageVersion(MTLLanguageVersion::Version2_3);
    // Exact maths, as `raster.rs` does it: with fast math up to 0.9% of the test scene's pixels
    // differ by 1, over the parity tolerance. Its successor, `mathMode`, is macOS 15's.
    #[allow(deprecated)]
    options.setFastMathEnabled(false);
    let source = NSString::from_str(SHADERS);
    let library = device
      .newLibraryWithSource_options_error(&source, Some(&options))
      .map_err(|e| format!("Metal shaders: {}", e.localizedDescription()))?;
    let pipeline = |vs: &str, fs: &str| {
      let desc = MTLRenderPipelineDescriptor::new();
      let f = |name| library.newFunctionWithName(&NSString::from_str(name));
      desc.setVertexFunction(f(vs).as_deref());
      desc.setFragmentFunction(f(fs).as_deref());
      // SAFETY: a pipeline has colour attachment 0.
      let att = unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) };
      att.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
      att.setBlendingEnabled(true);
      att.setSourceRGBBlendFactor(MTLBlendFactor::One);
      att.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
      att.setSourceAlphaBlendFactor(MTLBlendFactor::One);
      att.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
      let state = device.newRenderPipelineStateWithDescriptor_error(&desc);
      state.map_err(|e| format!("Metal pipeline {fs}: {}", e.localizedDescription()))
    };

    Ok(Self {
      unified: device.hasUnifiedMemory(),
      quads: pipeline("quad_vs", "quad_fs")?,
      lines: pipeline("line_vs", "line_fs")?,
      glyphs: pipeline("sprite_vs", "glyph_fs")?,
      images: pipeline("sprite_vs", "image_fs")?,
      device,
      queue,
    })
  }
}
