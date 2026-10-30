// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Files on Windows: the open and save dialogs a window asks for ([crate::FileDialog]), owned by
//! it, and the files dragged onto it, which an IDropTarget of ours registered for the window
//! reads from the drag's data object (CF_HDROP).
//!
//! The COM interfaces are called through their vtables by slot number as in [super::text] (the
//! order of `shobjidl_core.h`, `objidl.h`, `oleidl.h`).

use std::cell::{Cell, RefCell};
use std::ffi::{OsString, c_void};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, POINT, POINTL};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::System::Com::*;
use windows_sys::Win32::System::Ole::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
use windows_sys::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::core::{GUID, HRESULT};

use super::text::{Com, E_NOINTERFACE, IID_IUNKNOWN, call, iid, out};
use super::{modal, wide, with_window};
use crate::Event;

const IID_FILE_DIALOG: u128 = 0x42f85136_db7e_439c_85f1_e4075d135fc8;
const IID_SHELL_ITEM: u128 = 0x43826d1e_e718_42ee_bc55_a1e261c37bfe;
const IID_DROP_TARGET: u128 = 0x00000122_0000_0000_c000_000000000046;

// Vtable slots (IUnknown is 0–2): IModalWindow's Show, then IFileDialog's; IShellItem's;
// IDataObject's.
const SHOW: usize = 3;
const SET_FILE_TYPES: usize = 4;
const SET_OPTIONS: usize = 9;
const GET_OPTIONS: usize = 10;
const SET_FOLDER: usize = 12;
const SET_FILE_NAME: usize = 15;
const SET_TITLE: usize = 17;
const GET_RESULT: usize = 20;
const SET_DEFAULT_EXTENSION: usize = 22;
const ITEM_DISPLAY_NAME: usize = 5;
const DATA_GET: usize = 3;

// MARK: Dialogs

/// Shows the dialog window `hwnd` asked for ([crate::Runtime::take_file_dialog]) and sends it
/// the answer as [Event::FileDialog]: `None` at once while one is up (it disables the window);
/// whether it asked. Outside the window's borrow: the dialog runs the thread's messages until
/// it closes, the window's frames coming from its modal timer meanwhile.
pub(super) fn apply(hwnd: HWND) -> bool {
  let Some(d) = with_window(hwnd, |w| w.rt.take_file_dialog()).flatten() else { return false };
  let path = (unsafe { IsWindowEnabled(hwnd) } != 0).then(|| {
    modal(1);
    let path = show(hwnd, &d);
    modal(-1);
    path
  });
  with_window(hwnd, |w| w.send(Event::FileDialog(path.flatten())));
  true
}

/// Runs `d` owned by `hwnd`: the path picked, `None` when cancelled.
fn show(hwnd: HWND, d: &crate::FileDialog) -> Option<PathBuf> {
  let (clsid, iid) = (if d.save { FileSaveDialog } else { FileOpenDialog }, IID_FILE_DIALOG);
  let iid = GUID::from_u128(iid);
  let dialog =
    out(|p| unsafe { CoCreateInstance(&clsid, null_mut(), CLSCTX_INPROC_SERVER, &iid, p) })?;
  let set =
    |slot, s: &str| unsafe { call!(dialog, slot, (*const u16) -> HRESULT; wide(s).as_ptr()) };
  set(SET_TITLE, &d.title);
  // Only file system paths, as the shell items picked are read as one.
  let mut options = 0u32;
  unsafe {
    call!(dialog, GET_OPTIONS, (*mut u32) -> HRESULT; &mut options);
    call!(dialog, SET_OPTIONS, (u32) -> HRESULT; options | FOS_FORCEFILESYSTEM);
  }

  // One filter of all the types, "txt, md" for "*.txt;*.md"; a save adds the first one.
  let specs: Vec<String> = d.types.iter().map(|t| format!("*.{t}")).collect();
  let (name, spec) = (wide(&d.types.join(", ")), wide(&specs.join(";")));
  let filter = COMDLG_FILTERSPEC { pszName: name.as_ptr(), pszSpec: spec.as_ptr() };
  if let Some(ext) = d.types.first() {
    unsafe {
      call!(dialog, SET_FILE_TYPES, (u32, *const COMDLG_FILTERSPEC) -> HRESULT; 1, &filter)
    };
    if d.save {
      set(SET_DEFAULT_EXTENSION, ext);
    }
  }

  if let Some(name) = &d.name {
    set(SET_FILE_NAME, name);
  }

  if let Some(dir) = &d.dir {
    let (dir, iid) = (wide(&dir.to_string_lossy()), GUID::from_u128(IID_SHELL_ITEM));
    let item = out(|p| unsafe { SHCreateItemFromParsingName(dir.as_ptr(), null_mut(), &iid, p) });
    item.map(|f| unsafe { call!(dialog, SET_FOLDER, (*mut c_void) -> HRESULT; f.ptr()) });
  }

  // Cancelled: HRESULT_FROM_WIN32(ERROR_CANCELLED).
  if unsafe { call!(dialog, SHOW, (HWND) -> HRESULT; hwnd) } < 0 {
    return None;
  }

  let item = out(|p| unsafe { call!(dialog, GET_RESULT, (*mut *mut c_void) -> HRESULT; p) })?;
  let mut s = null_mut::<u16>();
  unsafe {
    call!(item, ITEM_DISPLAY_NAME, (i32, *mut *mut u16) -> HRESULT; SIGDN_FILESYSPATH, &mut s)
  };
  if s.is_null() {
    return None;
  }

  // SAFETY: a NUL-terminated string the shell allocated, freed after the copy.
  let n = (0..).take_while(|&i| unsafe { *s.add(i) } != 0).count();
  let path = PathBuf::from(OsString::from_wide(unsafe { std::slice::from_raw_parts(s, n) }));
  unsafe { CoTaskMemFree(s.cast()) };
  Some(path)
}

// MARK: Drops

/// Our IDropTarget for a window: the files of the drag over it, read as it enters. Counted:
/// the window holds a reference from [register] until [unregister], OLE its own while it calls
/// it, and the last one let go frees it.
#[repr(C)]
pub(super) struct Target {
  vtbl: &'static TargetVtbl,
  hwnd: HWND,
  paths: RefCell<Vec<PathBuf>>,
  refs: Cell<u32>,
}

pub(super) type This = *const Target;
type Data = *mut c_void;
type Effect = *mut u32;

#[repr(C)]
struct TargetVtbl {
  query: unsafe extern "system" fn(This, *const GUID, *mut *mut c_void) -> HRESULT,
  add_ref: unsafe extern "system" fn(This) -> u32,
  release: unsafe extern "system" fn(This) -> u32,
  enter: unsafe extern "system" fn(This, Data, u32, POINTL, Effect) -> HRESULT,
  over: unsafe extern "system" fn(This, u32, POINTL, Effect) -> HRESULT,
  leave: unsafe extern "system" fn(This) -> HRESULT,
  drop: unsafe extern "system" fn(This, Data, u32, POINTL, Effect) -> HRESULT,
}

static TARGET: TargetVtbl =
  TargetVtbl { query, add_ref, release, enter, over, leave, drop: dropped };

/// Starts OLE on this thread, once (it lasts as long as the app): the apartment drops need, so
/// it comes before any other COM start here ([super::app::hardware]).
pub(super) fn ole() {
  thread_local!(static OLE: Cell<bool> = const { Cell::new(false) });
  if !OLE.replace(true) {
    unsafe { OleInitialize(null()) };
  }
}

/// Takes files dragged onto window `hwnd`, OLE started on its thread first ([ole]), which calls
/// the target there; the window's reference.
pub(super) fn register(hwnd: HWND) -> Cell<This> {
  ole();

  let paths = RefCell::default();
  let target = Box::into_raw(Box::new(Target { vtbl: &TARGET, hwnd, paths, refs: Cell::new(1) }));
  unsafe { RegisterDragDrop(hwnd, target.cast()) };
  Cell::new(target)
}

/// Stops taking drops on `hwnd` and lets go of the window's reference, once.
pub(super) fn unregister(hwnd: HWND, target: &Cell<This>) {
  unsafe { RevokeDragDrop(hwnd) };
  let t = target.replace(null());
  if !t.is_null() {
    unsafe { release(t) };
  }
}

unsafe extern "system" fn query(this: This, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
  let ok = matches!(iid(unsafe { &*riid }), IID_IUNKNOWN | IID_DROP_TARGET);
  unsafe { *out = if ok { this.cast_mut().cast() } else { null_mut() } };
  // The interface given out is a reference, which its caller releases.
  if ok {
    unsafe { add_ref(this) };
    0
  } else {
    E_NOINTERFACE
  }
}

unsafe extern "system" fn add_ref(this: This) -> u32 {
  let refs = unsafe { &(*this).refs };
  refs.set(refs.get() + 1);
  refs.get()
}

unsafe extern "system" fn release(this: This) -> u32 {
  let refs = unsafe { &(*this).refs };
  let n = refs.get() - 1;
  refs.set(n);
  if n == 0 {
    // SAFETY: made by [register] with `Box::into_raw`; no reference is left.
    drop(unsafe { Box::from_raw(this.cast_mut()) });
  }

  n
}

unsafe extern "system" fn enter(this: This, data: Data, _: u32, at: POINTL, e: Effect) -> HRESULT {
  unsafe { *(*this).paths.borrow_mut() = paths(data) };
  unsafe { drag(this, at, e, false) }
}

unsafe extern "system" fn over(this: This, _: u32, at: POINTL, e: Effect) -> HRESULT {
  unsafe { drag(this, at, e, false) }
}

unsafe extern "system" fn leave(this: This) -> HRESULT {
  let t = unsafe { &*this };
  t.paths.take();
  with_window(t.hwnd, |w| w.send(Event::DragExit));
  0
}

unsafe extern "system" fn dropped(this: This, _: Data, _: u32, at: POINTL, e: Effect) -> HRESULT {
  unsafe { drag(this, at, e, true) }
}

/// Files dragged over the window at screen point `at`, or dropped (`drop`): [Event::DragOver]
/// or [Event::Drop] at it, as a copy; a drag without files, or one that can't copy, is refused.
unsafe fn drag(this: This, at: POINTL, effect: Effect, drop: bool) -> HRESULT {
  let t = unsafe { &*this };
  let paths = if drop { t.paths.take() } else { t.paths.borrow().clone() };
  let ok = !paths.is_empty() && unsafe { *effect } & DROPEFFECT_COPY != 0;
  unsafe { *effect = if ok { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
  if !ok {
    return 0;
  }

  let mut p = POINT { x: at.x, y: at.y };
  unsafe { ScreenToClient(t.hwnd, &mut p) };
  with_window(t.hwnd, |w| {
    let pos = w.point((p.x, p.y));
    w.send(if drop { Event::Drop { pos, paths } } else { Event::DragOver { pos, paths } })
  });
  0
}

/// The files a data object holds (CF_HDROP); empty when it holds none.
fn paths(data: Data) -> Vec<PathBuf> {
  let Some(data) = Com::borrowed(data) else { return Vec::new() };
  let (aspect, tymed) = (DVASPECT_CONTENT, TYMED_HGLOBAL as u32);
  let format =
    FORMATETC { cfFormat: CF_HDROP, dwAspect: aspect, lindex: -1, tymed, ptd: null_mut() };
  let mut medium = STGMEDIUM::default();
  let hr = unsafe {
    call!(data, DATA_GET, (*const FORMATETC, *mut STGMEDIUM) -> HRESULT; &format, &mut medium)
  };
  if hr < 0 {
    return Vec::new();
  }

  // The count with index 0xFFFFFFFF, a file's length without its NUL with no buffer.
  let hdrop = unsafe { medium.u.hGlobal };
  let n = unsafe { DragQueryFileW(hdrop, u32::MAX, null_mut(), 0) };
  let paths = (0..n).map(|i| {
    let mut buf = vec![0u16; unsafe { DragQueryFileW(hdrop, i, null_mut(), 0) } as usize + 1];
    let len = unsafe { DragQueryFileW(hdrop, i, buf.as_mut_ptr(), buf.len() as u32) };
    PathBuf::from(OsString::from_wide(&buf[..len as usize]))
  });
  let paths = paths.collect();
  unsafe { ReleaseStgMedium(&mut medium) };
  paths
}
