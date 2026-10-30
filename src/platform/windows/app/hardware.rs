// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Hardware on Windows ([crate::hardware]): an audio endpoint's transport by its PnP enumerator,
//! from its property store (the COM interfaces called by vtable slot as in [super::super::text],
//! the order of `mmdeviceapi.h` and `propsys.h`), the lid while it is watched, and their changes
//! ([crate::system]): an `IMMNotificationClient` of our own and `GUID_LIDSWITCH_STATE_CHANGE`
//! to the shell's window.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU8, Ordering};

use windows_sys::Win32::Foundation::{E_NOINTERFACE, PROPERTYKEY};
use windows_sys::Win32::System::Com::StructuredStorage::{PROPVARIANT, PropVariantClear};
use windows_sys::Win32::System::Com::{
  CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize, STGM_READ,
};
use windows_sys::Win32::System::Power::{
  HPOWERNOTIFY, POWERBROADCAST_SETTING, RegisterPowerSettingNotification,
  UnregisterPowerSettingNotification,
};
use windows_sys::Win32::System::Variant::VT_LPWSTR;
use windows_sys::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_WINDOW_HANDLE;
use windows_sys::core::{GUID, HRESULT, PCWSTR};

use super::super::text::{Com, call, iid, out};
use super::super::{files, wide};
use crate::hardware::Transport;
use crate::system::{Change, notice};

const CLSID_DEVICE_ENUMERATOR: u128 = 0xbcde0395_e52f_467c_8e3d_c4579291692e;
const IID_DEVICE_ENUMERATOR: u128 = 0xa95664d2_9614_4f35_a746_de8db63617e6;
/// `DEVPKEY_Device_EnumeratorName`, as a property store takes it.
const ENUMERATOR_NAME: PROPERTYKEY =
  PROPERTYKEY { fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 24 };

// Vtable slots (IUnknown is 0–2): IMMDeviceEnumerator's GetDevice and its notification
// callback's (un)registration, IMMDevice's OpenPropertyStore, IPropertyStore's GetValue.
const GET_DEVICE: usize = 5;
const REGISTER: usize = 6;
const UNREGISTER: usize = 7;
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

/// The lid as the last power notification told it, while it is watched: `None` before, and
/// until Windows finds a lid (a desktop never).
pub fn lid_closed() -> Option<bool> {
  match LID.load(Ordering::Acquire) {
    OPEN => Some(false),
    CLOSED => Some(true),
    _ => None,
  }
}

fn devices() -> Option<Com> {
  let (clsid, iid) =
    (GUID::from_u128(CLSID_DEVICE_ENUMERATOR), GUID::from_u128(IID_DEVICE_ENUMERATOR));
  out(|p| unsafe { CoCreateInstance(&clsid, null_mut(), CLSCTX_ALL, &iid, p) })
}

/// The endpoint's PnP enumerator: `BTHENUM`, `BTHHFENUM`, `USB`, `HDAUDIO`…
fn enumerator(id: &str) -> Option<String> {
  let devices = devices()?;
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

// MARK: Changes

const IID_UNKNOWN: u128 = 0x00000000_0000_0000_c000_000000000046;
const IID_NOTIFICATION_CLIENT: u128 = 0x7991eec9_7e89_4d85_8390_6c703cec60c0;
/// `GUID_LIDSWITCH_STATE_CHANGE` (WinNT.h; in windows-sys under `Win32_System_SystemServices`).
const LID_SWITCH: u128 = 0xba3e0f4d_b817_4094_a2d1_d56379e6a0f3;
/// `eCapture` of `EDataFlow`.
const CAPTURE: i32 = 1;

/// The lid while watched: unknown, open or closed, from the power notification.
static LID: AtomicU8 = AtomicU8::new(UNKNOWN);
const UNKNOWN: u8 = 0;
const OPEN: u8 = 1;
const CLOSED: u8 = 2;

thread_local! {
  /// The device enumerator our client is registered with, while the audio kinds are watched.
  static DEVICES: RefCell<Option<Com>> = const { RefCell::new(None) };
  static POWER: Cell<HPOWERNOTIFY> = const { Cell::new(0) };
}

type This = *mut c_void;

/// `IMMNotificationClient`'s vtable, in `mmdeviceapi.h`'s order.
#[repr(C)]
struct ClientVtbl {
  query: unsafe extern "system" fn(This, *const GUID, *mut *mut c_void) -> HRESULT,
  add_ref: unsafe extern "system" fn(This) -> u32,
  release: unsafe extern "system" fn(This) -> u32,
  state: unsafe extern "system" fn(This, PCWSTR, u32) -> HRESULT,
  added: unsafe extern "system" fn(This, PCWSTR) -> HRESULT,
  removed: unsafe extern "system" fn(This, PCWSTR) -> HRESULT,
  default: unsafe extern "system" fn(This, i32, i32, PCWSTR) -> HRESULT,
  property: unsafe extern "system" fn(This, PCWSTR, PROPERTYKEY) -> HRESULT,
}

/// The one client: static, so its references aren't counted and it is never freed. Its methods
/// come on the audio service's threads and only notice: they must never block or wait.
#[repr(C)]
struct Client(&'static ClientVtbl);

static CLIENT: Client = Client(&ClientVtbl {
  query,
  add_ref: one,
  release: one,
  state,
  added: endpoint,
  removed: endpoint,
  default,
  property,
});

fn client() -> *mut c_void {
  (&raw const CLIENT).cast_mut().cast()
}

unsafe extern "system" fn query(this: This, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
  let ok = matches!(iid(unsafe { &*riid }), IID_UNKNOWN | IID_NOTIFICATION_CLIENT);
  unsafe { *out = if ok { this } else { null_mut() } };
  if ok { 0 } else { E_NOINTERFACE }
}

unsafe extern "system" fn one(_: This) -> u32 {
  1
}

/// An endpoint came, went or changed state (a Bluetooth headset's as it connects): render ones
/// too, as the callback doesn't tell.
unsafe extern "system" fn endpoint(_: This, _: PCWSTR) -> HRESULT {
  notice(Change::AudioInputs);
  0
}

unsafe extern "system" fn state(this: This, id: PCWSTR, _: u32) -> HRESULT {
  unsafe { endpoint(this, id) }
}

unsafe extern "system" fn default(_: This, flow: i32, _: i32, _: PCWSTR) -> HRESULT {
  if let Some(kind) = default_changed(flow) {
    notice(kind);
  }

  0
}

/// Property values change often (volume, names): nothing here.
unsafe extern "system" fn property(_: This, _: PCWSTR, _: PROPERTYKEY) -> HRESULT {
  0
}

/// The kind a change of the default endpoint of `flow` tells: inputs only, for any role.
fn default_changed(flow: i32) -> Option<Change> {
  (flow == CAPTURE).then_some(Change::DefaultInput)
}

/// Watches `kind` on the main thread: our client registered for both audio kinds, the lid's
/// power notification to the shell's window.
pub fn watch(kind: Change) -> Result<(), String> {
  match kind {
    Change::AudioInputs | Change::DefaultInput => watch_audio(),
    Change::Lid => watch_lid(),
  }
}

fn watch_audio() -> Result<(), String> {
  if DEVICES.with_borrow(Option::is_some) {
    return Ok(());
  }

  // OLE's apartment, which the windows' drops need: a COM start of our own in the MTA here
  // would make OLE's fail when a tray app's first window opens after this.
  files::ole();
  let devices = devices().ok_or("no MMDeviceEnumerator")?;
  let hr = unsafe { call!(devices, REGISTER, (*mut c_void) -> HRESULT; client()) };
  if hr < 0 {
    return Err(format!("RegisterEndpointNotificationCallback failed ({hr:#x})"));
  }

  DEVICES.set(Some(devices));
  Ok(())
}

/// The lid's state to the shell's window: Windows tells the current one once it finds a lid,
/// then each change.
fn watch_lid() -> Result<(), String> {
  if POWER.get() != 0 {
    return Ok(());
  }

  let guid = GUID::from_u128(LID_SWITCH);
  let host = super::shell::host();
  let h = unsafe { RegisterPowerSettingNotification(host, &guid, DEVICE_NOTIFY_WINDOW_HANDLE) };
  if h == 0 {
    return Err("RegisterPowerSettingNotification failed".into());
  }

  POWER.set(h);
  Ok(())
}

/// Stops both: the client unregistered (never from its own callbacks), the lid forgotten.
pub fn unwatch() {
  if let Some(devices) = DEVICES.take() {
    unsafe { call!(devices, UNREGISTER, (*mut c_void) -> HRESULT; client()) };
  }

  let h = POWER.replace(0);
  if h != 0 {
    unsafe { UnregisterPowerSettingNotification(h) };
  }

  LID.store(UNKNOWN, Ordering::Release);
}

/// `WM_POWERBROADCAST`'s `PBT_POWERSETTINGCHANGE` to the shell's window: the lid's state kept,
/// and a change noticed (the first value, sent as it registers, only kept).
pub(super) fn power(setting: &POWERBROADCAST_SETTING) {
  if lid_open(setting).is_some_and(lid) {
    notice(Change::Lid);
  }
}

/// Whether the lid is open (its `Data`: 0 closed, 1 open) when `setting` is the lid's.
fn lid_open(setting: &POWERBROADCAST_SETTING) -> Option<bool> {
  let ours = iid(&setting.PowerSetting) == LID_SWITCH && setting.DataLength >= 4;
  // SAFETY: a DWORD of `DataLength` bytes follows the header.
  ours.then(|| unsafe { setting.Data.as_ptr().cast::<u32>().read_unaligned() } != 0)
}

/// Keeps the lid open or closed: whether it changed from a known state.
fn lid(open: bool) -> bool {
  let new = if open { OPEN } else { CLOSED };
  let old = LID.swap(new, Ordering::AcqRel);
  old != UNKNOWN && old != new
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn audio_changes() {
    assert_eq!(default_changed(CAPTURE), Some(Change::DefaultInput));
    assert_eq!(default_changed(0), None, "render");
    let mut out = null_mut();
    let riid = GUID::from_u128(IID_NOTIFICATION_CLIENT);
    assert_eq!(unsafe { (CLIENT.0.query)(client(), &riid, &mut out) }, 0);
    assert_eq!(out, client());
    let other = GUID::from_u128(IID_DEVICE_ENUMERATOR);
    assert_eq!(unsafe { (CLIENT.0.query)(client(), &other, &mut out) }, E_NOINTERFACE);
    // Nothing watched: the callbacks notice, which is ignored.
    assert_eq!(unsafe { (CLIENT.0.added)(client(), null()) }, 0);
    assert_eq!(unsafe { (CLIENT.0.default)(client(), CAPTURE, 0, null()) }, 0);
  }

  /// The power notification's lid: the first value kept, then a change told once.
  #[test]
  fn the_lid_from_power_settings() {
    let setting = |guid: u128, v: u32| {
      let mut s = [0u8; size_of::<POWERBROADCAST_SETTING>() + 4];
      let p = s.as_mut_ptr().cast::<POWERBROADCAST_SETTING>();
      unsafe {
        (*p).PowerSetting = GUID::from_u128(guid);
        (*p).DataLength = 4;
        (*p).Data.as_mut_ptr().cast::<u32>().write_unaligned(v);
      }

      s
    };
    let read = |s: &[u8]| lid_open(unsafe { &*s.as_ptr().cast::<POWERBROADCAST_SETTING>() });
    assert_eq!(read(&setting(LID_SWITCH, 1)), Some(true));
    assert_eq!(read(&setting(LID_SWITCH, 0)), Some(false));
    assert_eq!(read(&setting(IID_UNKNOWN, 0)), None);
    assert_eq!(lid_closed(), None);
    assert!(!lid(true), "the first value only kept");
    assert_eq!(lid_closed(), Some(false));
    assert!(lid(false));
    assert!(!lid(false));
    assert_eq!(lid_closed(), Some(true));
    LID.store(UNKNOWN, Ordering::Release);
  }

  /// A tray app watches before its first window: the window's drop target still registers, as
  /// OLE's apartment came first.
  #[test]
  fn drops_after_watching_audio() {
    use windows_sys::Win32::System::Ole::RevokeDragDrop;
    use windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow;

    struct Still;
    impl crate::App for Still {}

    std::thread::spawn(|| {
      let watched = watch(Change::AudioInputs);
      let cfg = crate::Config::new("drops", 200.0, 100.0);
      let hwnd = super::super::super::create(&cfg, Box::new(Still));
      // Registered by the window: revoked here once.
      assert_eq!(unsafe { RevokeDragDrop(hwnd) }, 0, "watched: {watched:?}");
      unsafe { DestroyWindow(hwnd) };
      unwatch();
    })
    .join()
    .unwrap();
  }
}
