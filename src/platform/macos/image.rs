// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Images decoded by ImageIO ([crate::Bitmap]): the thumbnail of the first image at the size
//! (`CGImageSourceCreateThumbnailAtIndex`, which a JPEG decodes at a fraction of its full size),
//! turned upright by its orientation, drawn into premultiplied sRGB RGBA. ImageIO and
//! CoreGraphics' bitmap contexts are thread-safe: any thread.

use std::ffi::c_void;

use objc2_core_foundation::{
  CFBoolean, CFData, CFDictionary, CFNumber, CFString, CFType, CGPoint, CGRect, CGSize,
  kCFAllocatorNull,
};
use objc2_core_graphics::{
  CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, kCGColorSpaceSRGB,
};
use objc2_image_io::{
  CGImageSource, kCGImagePropertyPixelHeight, kCGImagePropertyPixelWidth,
  kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform,
  kCGImageSourceThumbnailMaxPixelSize,
};

use crate::bitmap::{size, zeroed};

/// The first image in `file` with its longer side at most `side`: its size and premultiplied
/// RGBA pixels.
pub fn decode(file: &[u8], side: u32) -> Result<(u32, u32, Vec<u8>), String> {
  // SAFETY: the data borrows `file`, which outlives it, the source and the thumbnail (drawn
  // here); the options are CFString keys.
  let image = unsafe {
    let data =
      CFData::with_bytes_no_copy(None, file.as_ptr(), file.len() as isize, kCFAllocatorNull)
        .ok_or("can't read the image")?;
    let src = CGImageSource::with_data(&data, None).ok_or("not an image")?;
    // Its size from the header, checked before anything is decoded.
    let props = src.properties_at_index(0, None).ok_or("not an image the system reads")?;
    let props = props.cast_unchecked::<CFString, CFNumber>();
    let dim = |k| props.get(k).and_then(|n| n.as_i64()).and_then(|v| u32::try_from(v).ok());
    let (w, h) = (dim(kCGImagePropertyPixelWidth), dim(kCGImagePropertyPixelHeight));
    size(w.ok_or("an image of no size")?, h.ok_or("an image of no size")?, side)?;
    let yes = CFBoolean::new(true);
    let max = CFNumber::new_i64(side.min(i32::MAX as u32) as i64);
    let keys = [
      kCGImageSourceCreateThumbnailFromImageAlways,
      kCGImageSourceCreateThumbnailWithTransform,
      kCGImageSourceThumbnailMaxPixelSize,
    ];
    let values: [&CFType; 3] = [yes, yes, &max];
    let opts = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
    src.thumbnail_at_index(0, Some(opts.as_opaque())).ok_or("not an image the system reads")?
  };

  let (w, h) = (CGImage::width(Some(&image)), CGImage::height(Some(&image)));
  // ImageIO's thumbnail of a picture smaller than the side is the picture: never upscaled.
  let mut px = zeroed(u32::try_from(w).unwrap_or(u32::MAX), u32::try_from(h).unwrap_or(u32::MAX))?;
  let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }));
  let info = CGImageAlphaInfo::PremultipliedLast.0;
  let ptr = px.as_mut_ptr().cast::<c_void>();
  // SAFETY: `px` holds `h` rows of `w * 4` bytes and outlives the context.
  let ctx = unsafe { CGBitmapContextCreate(ptr, w, h, 8, w * 4, space.as_deref(), info) }
    .ok_or("can't draw the image")?;
  let rect = CGRect::new(CGPoint::ZERO, CGSize::new(w as f64, h as f64));
  CGContext::draw_image(Some(&ctx), rect, Some(&image));
  drop(ctx);
  Ok((w as u32, h as u32, px))
}
