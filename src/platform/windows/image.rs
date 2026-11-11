// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Images decoded by WIC ([crate::Bitmap]): the first frame from memory, shrunk by WIC's Fant
//! scaler as it is read, converted to premultiplied RGBA, then turned by its
//! `System.Photo.Orientation` ([orient]). The COM interfaces are called by vtable slot as in
//! [super::text], the order of `wincodec.h`. WIC's factory is free-threaded: any thread, which
//! joins COM's multithreaded apartment for the call when it is in none.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use windows_sys::Win32::System::Com::StructuredStorage::{PROPVARIANT, PropVariantClear};
use windows_sys::Win32::System::Com::{
  CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows_sys::Win32::System::Variant::VT_UI2;
use windows_sys::core::{GUID, HRESULT};

use super::text::{Com, call, out};
use super::wide;
use crate::bitmap::{orient, size, zeroed};

const CLSID_IMAGING_FACTORY: u128 = 0xcacaf262_9370_4615_a13b_9f5539da4c0a;
const IID_IMAGING_FACTORY: u128 = 0xec5ec8a9_c395_4314_9c77_54d7a935ff70;
const PIXEL_FORMAT_PRGBA: u128 = 0x3cc4a650_a527_4d37_a916_3142c7ebedba;

// Vtable slots (IUnknown is 0–2). IWICImagingFactory: CreateDecoderFromStream,
// CreateFormatConverter, CreateBitmapScaler, CreateStream; IWICStream: InitializeFromMemory;
// IWICBitmapDecoder: GetFrame; IWICBitmapSource: GetSize, CopyPixels, and the Initialize of
// each source over another (scaler, converter); IWICBitmapFrameDecode: GetMetadataQueryReader;
// IWICMetadataQueryReader: GetMetadataByName.
const DECODER_FROM_STREAM: usize = 4;
const FORMAT_CONVERTER: usize = 10;
const SCALER: usize = 11;
const STREAM: usize = 14;
const FROM_MEMORY: usize = 16;
const GET_FRAME: usize = 13;
const GET_SIZE: usize = 3;
const COPY_PIXELS: usize = 7;
const INITIALIZE: usize = 8;
const QUERY_READER: usize = 8;
const BY_NAME: usize = 5;

/// `WICDecodeMetadataCacheOnDemand`, `WICBitmapInterpolationModeFant`, `WICBitmapDitherTypeNone`,
/// `WICBitmapPaletteTypeCustom`.
const ON_DEMAND: i32 = 0;
const FANT: i32 = 3;
const NO_DITHER: i32 = 0;
const CUSTOM_PALETTE: i32 = 0;

pub fn decode(file: &[u8], side: u32) -> Result<(u32, u32, Vec<u8>), String> {
  // COM for the call: a thread in another apartment has it already (RPC_E_CHANGED_MODE).
  let init = unsafe { CoInitializeEx(null(), COINIT_MULTITHREADED as u32) } >= 0;
  let image = read(file, side);
  if init {
    unsafe { CoUninitialize() };
  }

  let (w, h, px, orientation) = image?;
  orient(w, h, px, orientation)
}

const UNREAD: &str = "not an image the system reads";

/// `hr` as a result: failed, the image isn't one WIC reads.
fn ok(hr: HRESULT) -> Result<(), String> {
  if hr < 0 { Err(UNREAD.into()) } else { Ok(()) }
}

/// The first frame's size and premultiplied RGBA pixels as stored, at most `side` on the longer
/// side, and its EXIF orientation. A GIF's first frame is that frame alone, not placed in the
/// rest of its screen.
fn read(file: &[u8], side: u32) -> Result<(u32, u32, Vec<u8>, u16), String> {
  let len = u32::try_from(file.len()).map_err(|_| UNREAD)?;
  let (clsid, iid) = (GUID::from_u128(CLSID_IMAGING_FACTORY), GUID::from_u128(IID_IMAGING_FACTORY));
  let factory =
    out(|p| unsafe { CoCreateInstance(&clsid, null_mut(), CLSCTX_INPROC_SERVER, &iid, p) });
  let factory = factory.ok_or("no WIC")?;
  let new =
    |slot| out(|p| unsafe { call!(factory, slot, (*mut *mut c_void) -> HRESULT; p) }).ok_or(UNREAD);
  // SAFETY: each call is its interface's method at that slot; the stream reads `file`, which
  // outlives it and everything decoded from it.
  unsafe {
    let stream = new(STREAM)?;
    ok(call!(stream, FROM_MEMORY, (*const u8, u32) -> HRESULT; file.as_ptr(), len))?;
    let decoder = out(|p| {
      call!(factory, DECODER_FROM_STREAM, (*mut c_void, *const GUID, i32, *mut *mut c_void) -> HRESULT;
        stream.ptr(), null(), ON_DEMAND, p)
    });
    let decoder = decoder.ok_or(UNREAD)?;
    let frame = out(|p| call!(decoder, GET_FRAME, (u32, *mut *mut c_void) -> HRESULT; 0, p));
    let frame = frame.ok_or(UNREAD)?;
    // Open: WIC's HEIF decoder may turn the picture by its `irot` and `imir` and still report
    // the orientation, turning it twice; it needs a run on Windows.
    let orientation = orientation(&frame);
    let (mut w, mut h) = (0u32, 0u32);
    ok(call!(frame, GET_SIZE, (*mut u32, *mut u32) -> HRESULT; &mut w, &mut h))?;
    // Its size from the header, checked before anything is decoded.
    let (fw, fh) = size(w, h, side)?;
    let mut source = frame;
    if (fw, fh) != (w, h) {
      let scaler = new(SCALER)?;
      ok(
        call!(scaler, INITIALIZE, (*mut c_void, u32, u32, i32) -> HRESULT; source.ptr(), fw, fh, FANT),
      )?;
      source = scaler;
    }

    let converter = new(FORMAT_CONVERTER)?;
    let format = GUID::from_u128(PIXEL_FORMAT_PRGBA);
    ok(
      call!(converter, INITIALIZE, (*mut c_void, *const GUID, i32, *mut c_void, f64, i32) -> HRESULT;
      source.ptr(), &format, NO_DITHER, null_mut(), 0.0, CUSTOM_PALETTE),
    )?;
    let mut px = zeroed(fw, fh)?;
    // At most `MAX_PIXELS` × 4 bytes: within a u32.
    let (stride, bytes) = (fw * 4, u32::try_from(px.len()).map_err(|_| UNREAD)?);
    ok(call!(converter, COPY_PIXELS, (*const c_void, u32, u32, *mut u8) -> HRESULT;
      null(), stride, bytes, px.as_mut_ptr()))?;
    Ok((fw, fh, px, orientation))
  }
}

/// The frame's EXIF orientation, 1 (upright) when it has none.
fn orientation(frame: &Com) -> u16 {
  let Some(reader) =
    out(|p| unsafe { call!(frame, QUERY_READER, (*mut *mut c_void) -> HRESULT; p) })
  else {
    return 1;
  };

  let name = wide("System.Photo.Orientation");
  // SAFETY: an empty PROPVARIANT is all zeros; GetMetadataByName fills it, PropVariantClear frees
  // it.
  let mut v: PROPVARIANT = unsafe { std::mem::zeroed() };
  let got = unsafe {
    call!(reader, BY_NAME, (*const u16, *mut PROPVARIANT) -> HRESULT; name.as_ptr(), &mut v)
  };
  let s = unsafe { v.Anonymous.Anonymous };
  let o = if got >= 0 && s.vt == VT_UI2 { unsafe { s.Anonymous.uiVal } } else { 1 };
  unsafe { PropVariantClear(&mut v) };
  o
}
