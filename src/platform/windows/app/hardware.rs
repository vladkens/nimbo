// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Hardware on Windows ([crate::hardware]): an audio endpoint's transport by its PnP enumerator,
//! from its property store (the COM interfaces called by vtable slot as in [super::super::text],
//! the order of `mmdeviceapi.h` and `propsys.h`); the lid isn't read yet.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::PROPERTYKEY;
use windows_sys::Win32::System::Com::StructuredStorage::{PROPVARIANT, PropVariantClear};
use windows_sys::Win32::System::Com::{
  CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize, STGM_READ,
};
use windows_sys::Win32::System::Variant::VT_LPWSTR;
use windows_sys::core::{GUID, HRESULT};

use super::super::text::{Com, call, out};
use super::super::wide;
use crate::hardware::Transport;

const CLSID_DEVICE_ENUMERATOR: u128 = 0xbcde0395_e52f_467c_8e3d_c4579291692e;
const IID_DEVICE_ENUMERATOR: u128 = 0xa95664d2_9614_4f35_a746_de8db63617e6;
/// `DEVPKEY_Device_EnumeratorName`, as a property store takes it.
const ENUMERATOR_NAME: PROPERTYKEY =
  PROPERTYKEY { fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 24 };

// Vtable slots (IUnknown is 0–2): IMMDeviceEnumerator's GetDevice, IMMDevice's
// OpenPropertyStore, IPropertyStore's GetValue.
const GET_DEVICE: usize = 5;
const OPEN_PROPERTY_STORE: usize = 4;
const GET_VALUE: usize = 5;

pub fn transport(id: &str) -> Transport {
  // COM for the call: a thread in another apartment has it already (RPC_E_CHANGED_MODE).
  let init = unsafe { CoInitializeEx(null(), COINIT_MULTITHREADED as u32) } >= 0;
  let enumerator = enumerator(id);
  if init {
    unsafe { CoUninitialize() };
  }

  enumerator.map_or(Transport::Unknown, |e| crate::hardware::pnp(&e))
}

pub fn lid_closed() -> Option<bool> {
  None
}

/// The endpoint's PnP enumerator: `BTHENUM`, `BTHHFENUM`, `USB`, `HDAUDIO`…
fn enumerator(id: &str) -> Option<String> {
  let (clsid, iid) =
    (GUID::from_u128(CLSID_DEVICE_ENUMERATOR), GUID::from_u128(IID_DEVICE_ENUMERATOR));
  let devices = out(|p| unsafe { CoCreateInstance(&clsid, null_mut(), CLSCTX_ALL, &iid, p) })?;
  let id = wide(id);
  let device = out(|p| unsafe {
    call!(devices, GET_DEVICE, (*const u16, *mut *mut c_void) -> HRESULT; id.as_ptr(), p)
  })?;
  let store = out(|p| unsafe {
    call!(device, OPEN_PROPERTY_STORE, (u32, *mut *mut c_void) -> HRESULT; STGM_READ, p)
  })?;
  // SAFETY: an empty PROPVARIANT is all zeros; GetValue fills it, PropVariantClear frees it.
  let mut v: PROPVARIANT = unsafe { std::mem::zeroed() };
  let got = unsafe {
    call!(store, GET_VALUE, (*const PROPERTYKEY, *mut PROPVARIANT) -> HRESULT; &ENUMERATOR_NAME, &mut v)
  };
  if got < 0 {
    return None;
  }

  let s = unsafe { v.Anonymous.Anonymous };
  let p = unsafe { s.Anonymous.pwszVal };
  let name = (s.vt == VT_LPWSTR && !p.is_null()).then(|| {
    // SAFETY: a NUL-terminated string the store allocated, copied before it is freed.
    let n = (0..).take_while(|&i| unsafe { *p.add(i) } != 0).count();
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, n) })
  });
  unsafe { PropVariantClear(&mut v) };
  name
}
