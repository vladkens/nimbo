// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The clipboard for a paste of the app's own on Windows ([crate::clipboard]), opened for the
//! shell's hidden window ([super::shell]): its formats as one item, registered ones by name and
//! the rest as `#<n>`; GDI handles (bitmaps, metafiles, palettes) and private formats aren't
//! memory and count as missing, and the ones the system makes from another (ANSI text from
//! Unicode, `CF_DIB` from `CF_DIBV5`) are left for it to make again. The window's plain text
//! ([super::super::WinClipboard]) reads and writes through [Open], [bytes] and [set] too. An offer
//! with a callback is delayed rendering: the text set as null and rendered at `WM_RENDERFORMAT`
//! (the first read: the system keeps it then), or at `WM_RENDERALLFORMATS` as the app quits;
//! `WM_DESTROYCLIPBOARD` (anyone emptying the clipboard) ends the offer.

use std::cell::{Cell, RefCell};
use std::ptr::null_mut;
use std::time::Duration;

use windows_sys::Win32::Foundation::{GlobalFree, HWND};
use windows_sys::Win32::System::DataExchange::{
  CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW,
  GetClipboardOwner, GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW,
  SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
  GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};

use super::super::{CF_TEXT_UTF16, wide};
use super::shell::host;
use crate::clipboard::Snapshot;

/// `CF_BITMAP`, `CF_METAFILEPICT`, `CF_PALETTE`, `CF_ENHMETAFILE` and their display forms: GDI
/// handles, not memory.
const HANDLES: [u32; 7] = [2, 3, 9, 14, 0x82, 0x83, 0x8E];
/// Clipboard history and monitors skip the text: the first by its presence, the others as 0.
const MARKERS: [&str; 3] = [
  "ExcludeClipboardContentFromMonitorProcessing",
  "CanIncludeInClipboardHistory",
  "CanUploadToCloudClipboard",
];

type OnRead = Box<dyn FnMut()>;

/// The offer on the clipboard: its number, and the text with its callback until rendered.
struct Offered {
  id: u64,
  pending: Option<(Vec<u16>, OnRead)>,
}

thread_local! {
  static OFFER: RefCell<Option<Offered>> = const { RefCell::new(None) };
  static NEXT: Cell<u64> = const { Cell::new(1) };
}

/// The clipboard, open for a window (the shell's unless said) until dropped.
pub(in super::super) struct Open;

impl Open {
  pub fn new() -> Option<Self> {
    Self::by(host())
  }

  /// Open for `owner`, tried a few times: another process may hold it a moment (a VM's clipboard
  /// sync opens it after every change), as Chromium tries 5 times 5 ms apart.
  pub fn by(owner: HWND) -> Option<Self> {
    let mut tries = (0..5u64).map(|i| std::thread::sleep(Duration::from_millis(5 * i.min(1))));
    tries.any(|()| !owner.is_null() && unsafe { OpenClipboard(owner) } != 0).then_some(Open)
  }
}

impl Drop for Open {
  fn drop(&mut self) {
    unsafe { CloseClipboard() };
  }
}

pub fn change_count() -> Option<u64> {
  (!host().is_null()).then(|| unsafe { GetClipboardSequenceNumber() } as u64)
}

pub fn holds(id: u64) -> bool {
  OFFER.with_borrow(|o| o.as_ref().is_some_and(|o| o.id == id))
}

pub fn readable() -> bool {
  true
}

pub fn snapshot() -> Option<Snapshot> {
  let _open = Open::new()?;
  let mut formats = Vec::new();
  let mut format = unsafe { EnumClipboardFormats(0) };
  while format != 0 {
    formats.push(format);
    format = unsafe { EnumClipboardFormats(format) };
  }

  let (mut types, mut missing) = (Vec::new(), 0);
  for &format in formats.iter().filter(|&&f| !synthesized(f, &formats)) {
    match memory(format).then(|| bytes(format)).flatten() {
      Some(data) => types.push((name(format), data)),
      None => missing += 1,
    }
  }

  let items = if types.is_empty() { Vec::new() } else { vec![types] };
  Some(Snapshot { items, missing })
}

pub fn restore(snapshot: Snapshot) -> bool {
  let Some(_open) = Open::new() else { return false };
  unsafe { EmptyClipboard() };
  let types = snapshot.items.iter().flatten();
  types.fold(true, |all, (name, data)| set(format(name), data) && all)
}

pub fn offer(text: &str, on_read: Option<OnRead>) -> Option<u64> {
  let _open = Open::new()?;
  // Ends the offer before (`WM_DESTROYCLIPBOARD`).
  unsafe { EmptyClipboard() };
  for marker in MARKERS {
    set(format(marker), &0u32.to_ne_bytes());
  }

  let text = wide(text);
  let pending = match on_read {
    Some(on_read) => {
      unsafe { SetClipboardData(CF_TEXT_UTF16, null_mut()) };
      Some((text, on_read))
    }
    None => {
      set(CF_TEXT_UTF16, &utf16_bytes(&text));
      None
    }
  };

  let id = NEXT.replace(NEXT.get() + 1);
  OFFER.set(Some(Offered { id, pending }));
  Some(id)
}

/// The shell's window `hwnd` got `WM_RENDERFORMAT` (someone reads the text, the clipboard open
/// for them) or, with `all`, `WM_RENDERALLFORMATS` (it is being destroyed as the app quits, after
/// [host] went: opened here, if still its own).
pub(super) fn render(hwnd: HWND, all: bool) {
  let pending = OFFER.with_borrow_mut(|o| o.as_mut()?.pending.take());
  let Some((text, mut on_read)) = pending else { return };
  if !all {
    set(CF_TEXT_UTF16, &utf16_bytes(&text));
    return on_read();
  }

  if unsafe { GetClipboardOwner() } == hwnd
    && let Some(_open) = Open::by(hwnd)
  {
    set(CF_TEXT_UTF16, &utf16_bytes(&text));
  }
}

/// `WM_DESTROYCLIPBOARD`: the clipboard was emptied, the offer is gone.
pub(super) fn destroyed() {
  OFFER.take();
}

/// The system makes `format` from another of `all`: ANSI and OEM text from Unicode, a DIB from a
/// DIBV5.
fn synthesized(format: u32, all: &[u32]) -> bool {
  match format {
    1 | 7 => all.contains(&CF_TEXT_UTF16),
    8 => all.contains(&17),
    _ => false,
  }
}

/// The format's data is memory (an `HGLOBAL`), not a GDI handle or a private format's anything.
fn memory(format: u32) -> bool {
  !HANDLES.contains(&format) && !(0x200..=0x3FF).contains(&format)
}

/// `format`'s data; the clipboard is open.
pub(in super::super) fn bytes(format: u32) -> Option<Vec<u8>> {
  let data = unsafe { GetClipboardData(format) };
  let p = unsafe { GlobalLock(data) }.cast::<u8>();
  if p.is_null() {
    return None;
  }

  // SAFETY: the locked allocation holds `GlobalSize` bytes.
  let out = unsafe { std::slice::from_raw_parts(p, GlobalSize(data)) }.to_vec();
  unsafe { GlobalUnlock(data) };
  Some(out)
}

/// Sets `format` to a copy of `data`; the clipboard is open.
pub(in super::super) fn set(format: u32, data: &[u8]) -> bool {
  let mem = unsafe { GlobalAlloc(GMEM_MOVEABLE, data.len().max(1)) };
  let p = unsafe { GlobalLock(mem) }.cast::<u8>();
  if p.is_null() {
    unsafe { GlobalFree(mem) };
    return false;
  }

  unsafe {
    std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
    GlobalUnlock(mem);
  }

  // The clipboard owns the memory once it takes it.
  let taken = !unsafe { SetClipboardData(format, mem) }.is_null();
  if !taken {
    unsafe { GlobalFree(mem) };
  }

  taken
}

/// A registered format's name, else its number as `#<n>`.
fn name(format: u32) -> String {
  let mut buf = [0u16; 256];
  let n = unsafe { GetClipboardFormatNameW(format, buf.as_mut_ptr(), buf.len() as i32) };
  match n > 0 {
    true => String::from_utf16_lossy(&buf[..n as usize]),
    false => format!("#{format}"),
  }
}

/// The format [name] gave.
fn format(name: &str) -> u32 {
  match name.strip_prefix('#').and_then(|n| n.parse().ok()) {
    Some(n) => n,
    None => unsafe { RegisterClipboardFormatW(wide(name).as_ptr()) },
  }
}

pub(in super::super) fn utf16_bytes(text: &[u16]) -> Vec<u8> {
  text.iter().flat_map(|u| u.to_ne_bytes()).collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Names and numbers go both ways; registering a name asks nothing of the clipboard.
  #[test]
  fn formats_by_name() {
    assert_eq!(name(CF_TEXT_UTF16), "#13");
    assert_eq!(format("#13"), CF_TEXT_UTF16);
    let html = format("HTML Format");
    assert!(html >= 0xC000);
    assert_eq!(name(html), "HTML Format");
    assert!(memory(CF_TEXT_UTF16) && memory(html));
    assert!(!memory(2) && !memory(0x200) && !memory(0x3FF));
    assert!(synthesized(1, &[13, 1, 7]) && !synthesized(1, &[1]));
    assert!(synthesized(8, &[17, 8]) && !synthesized(17, &[17, 8]));
  }
}
