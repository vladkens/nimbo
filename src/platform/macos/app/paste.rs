// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The general pasteboard for a paste of the app's own ([crate::clipboard]): snapshots of every
//! item and type, the change count, the access behaviour of macOS 15.4+, and an offer: types
//! declared with an [Owner] that hands the text out when someone reads it
//! (`pasteboard:provideDataForType:`, which tells the app it was read; the pasteboard server asks
//! once and keeps the data). Owners live a few offers longer than their promise: AppKit may still
//! tell one that it lost the pasteboard after that.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, Message, define_class, msg_send, sel};
use objc2_app_kit::{
  NSPasteboard, NSPasteboardAccessBehavior, NSPasteboardItem, NSPasteboardTypeString,
  NSPasteboardWriting,
};
use objc2_foundation::{NSArray, NSData, NSObject, NSObjectProtocol, NSString};

use super::super::{READ_WAIT, within};
use crate::clipboard::Snapshot;

/// Owners kept alive after their offer (see the module docs).
const KEPT: usize = 4;
const MARKERS: [&str; 2] = ["org.nspasteboard.TransientType", "org.nspasteboard.ConcealedType"];

type OnRead = Box<dyn FnMut()>;

thread_local! {
  static OWNERS: RefCell<VecDeque<Retained<Owner>>> = const { RefCell::new(VecDeque::new()) };
  /// The general pasteboard's change count at the app's last promise: it holds the promise while
  /// the count is the same (a read handing the text out leaves it).
  static PROMISED: Cell<Option<u64>> = const { Cell::new(None) };
}

pub struct OwnerIvars {
  /// The text while the promise is on the pasteboard, and who hears it read.
  text: RefCell<Option<Retained<NSString>>>,
  on_read: RefCell<Option<OnRead>>,
}

define_class!(
  // SAFETY: NSObject has no subclassing requirements, and `Owner` does not implement `Drop`.
  #[unsafe(super = NSObject)]
  #[name = "NimboPasteboardOwner"]
  #[ivars = OwnerIvars]
  pub struct Owner;

  // SAFETY: no requirements.
  unsafe impl NSObjectProtocol for Owner {}

  impl Owner {
    /// NSPasteboardOwner: someone reads a promised type; the markers are handed out empty.
    #[unsafe(method(pasteboard:provideDataForType:))]
    fn provide(&self, pasteboard: &NSPasteboard, kind: &NSString) {
      let ivars = self.ivars();
      // SAFETY: a static of the framework.
      let text = kind.isEqualToString(unsafe { NSPasteboardTypeString });
      let data = ivars.text.borrow().clone().filter(|_| text);
      pasteboard.setString_forType(&data.unwrap_or_default(), kind);
      // Once: the pasteboard server keeps the text and asks no more.
      let on_read = if text { ivars.on_read.take() } else { None };
      if let Some(mut on_read) = on_read {
        on_read();
      }
    }

    /// Someone else wrote to the pasteboard (or a restore did): the promise is gone.
    #[unsafe(method(pasteboardChangedOwner:))]
    fn changed_owner(&self, _pasteboard: &NSPasteboard) {
      self.ivars().text.take();
      self.ivars().on_read.take();
    }
  }
);

impl Owner {
  fn new(text: &str, on_read: OnRead) -> Retained<Self> {
    let (text, on_read) =
      (RefCell::new(Some(NSString::from_str(text))), RefCell::new(Some(on_read)));
    let this = Self::alloc().set_ivars(OwnerIvars { text, on_read });
    // SAFETY: NSObject's `init`.
    unsafe { msg_send![super(this), init] }
  }
}

fn general() -> Retained<NSPasteboard> {
  NSPasteboard::generalPasteboard()
}

pub fn change_count() -> Option<u64> {
  Some(general().changeCount() as u64)
}

pub fn holds(count: u64) -> bool {
  change_count() == Some(count)
}

pub fn readable() -> bool {
  may_read(&general())
}

/// `accessBehavior` is macOS 15.4's: before it, reading never asks.
fn may_read(pasteboard: &NSPasteboard) -> bool {
  use NSPasteboardAccessBehavior as Behavior;

  !pasteboard.respondsToSelector(sel!(accessBehavior))
    || !matches!(pasteboard.accessBehavior(), Behavior::Ask | Behavior::AlwaysDeny)
}

/// As [read]: `None` when an app that promised its data doesn't answer within [READ_WAIT], so the
/// paste goes without a restore.
pub fn snapshot() -> Option<Snapshot> {
  read(|| take(&general()))
}

/// `f`'s read of the general pasteboard: on the main thread while it holds the app's own promise,
/// whose owner answers there (AppKit's objects aren't for other threads), and in process, never
/// waiting on another app; off it within [READ_WAIT] otherwise.
pub(crate) fn read<T: Send + 'static>(f: fn() -> T) -> Option<T> {
  match PROMISED.get() == change_count() {
    true => Some(f()),
    false => within(READ_WAIT, f),
  }
}

pub fn restore(snapshot: Snapshot) -> bool {
  put(&general(), &snapshot)
}

pub fn offer(text: &str, on_read: Option<OnRead>) -> Option<u64> {
  Some(offer_on(&general(), text, on_read))
}

fn take(pasteboard: &NSPasteboard) -> Snapshot {
  let mut snapshot = Snapshot::default();
  for item in pasteboard.pasteboardItems().iter().flat_map(|items| items.iter()) {
    let mut types = Vec::new();
    for kind in item.types().iter() {
      match item.dataForType(&kind) {
        Some(data) => types.push((kind.to_string(), data.to_vec())),
        None => snapshot.missing += 1,
      }
    }

    if !types.is_empty() {
      snapshot.items.push(types);
    }
  }

  snapshot
}

fn put(pasteboard: &NSPasteboard, snapshot: &Snapshot) -> bool {
  pasteboard.clearContents();
  if snapshot.items.is_empty() {
    return true;
  }

  let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = (snapshot.items.iter())
    .map(|types| {
      let item = NSPasteboardItem::new();
      for (kind, data) in types {
        item.setData_forType(&NSData::with_bytes(data), &NSString::from_str(kind));
      }

      ProtocolObject::from_retained(item)
    })
    .collect();
  pasteboard.writeObjects(&NSArray::from_retained_slice(&items))
}

/// Puts `text` with the markers on `pasteboard`, promised when there is `on_read`; the change
/// count after.
fn offer_on(pasteboard: &NSPasteboard, text: &str, on_read: Option<OnRead>) -> u64 {
  // SAFETY: a static of the framework.
  let string = unsafe { NSPasteboardTypeString };
  let markers = MARKERS.map(NSString::from_str);
  let types =
    NSArray::from_retained_slice(&[string.retain(), markers[0].clone(), markers[1].clone()]);
  let Some(on_read) = on_read else {
    // SAFETY: no owner: the data of every declared type is set right below.
    unsafe { pasteboard.declareTypes_owner(&types, None) };
    for marker in &markers {
      pasteboard.setData_forType(Some(&NSData::new()), marker);
    }

    pasteboard.setString_forType(&NSString::from_str(text), string);
    return pasteboard.changeCount() as u64;
  };

  let owner = Owner::new(text, on_read);
  let object: &AnyObject = &owner;
  // SAFETY: the owner provides every declared type and lives while its promise is on the
  // pasteboard (and a few offers longer: [OWNERS]).
  let count = unsafe { pasteboard.declareTypes_owner(&types, Some(object)) };
  if std::ptr::eq(pasteboard, &*general()) {
    PROMISED.set(Some(count as u64));
  }

  OWNERS.with_borrow_mut(|owners| {
    owners.push_back(owner);
    if owners.len() > KEPT {
      owners.pop_front();
    }
  });
  count as u64
}

#[cfg(test)]
mod tests {
  use std::cell::Cell;
  use std::rc::Rc;

  use super::*;

  /// A pasteboard of its own, by a name of the test's: the user's clipboard is never touched
  /// (`pasteboardWithUniqueName` handed two tests running at once the same one).
  struct Private(Retained<NSPasteboard>);

  impl Private {
    fn new(test: &str) -> Self {
      let name = NSString::from_str(&format!("nimbo-{test}-{}", std::process::id()));
      Self(NSPasteboard::pasteboardWithName(&name))
    }
  }

  impl Drop for Private {
    fn drop(&mut self) {
      // SAFETY: no arguments; the pasteboard is not used afterwards.
      let _: () = unsafe { msg_send![&self.0, releaseGlobally] };
    }
  }

  #[test]
  fn a_snapshot_restores_every_item_and_type() {
    let pb = Private::new("snapshot");
    let png = [0x89, b'P', b'N', b'G', 0, 1, 2, 255];
    let item = |types: &[(&str, &[u8])]| -> Vec<(String, Vec<u8>)> {
      types.iter().map(|(t, d)| (t.to_string(), d.to_vec())).collect()
    };
    let want = Snapshot {
      items: vec![
        item(&[("public.utf8-plain-text", "текст ✓".as_bytes()), ("public.html", b"<b>x</b>")]),
        item(&[("public.png", &png), ("com.example.private", &[])]),
      ],
      missing: 0,
    };
    assert!(put(&pb.0, &want));
    assert_eq!(take(&pb.0), want, "byte for byte");
    let name = pb.0.name().to_string();
    let off_main = within(READ_WAIT, move || {
      take(&NSPasteboard::pasteboardWithName(&NSString::from_str(&name)))
    });
    assert_eq!(off_main.as_ref(), Some(&want), "as [snapshot] reads it");
    assert!(may_read(&pb.0), "a pasteboard of its own may be read");

    let before = pb.0.changeCount();
    offer_on(&pb.0, "other", None);
    assert_ne!(pb.0.changeCount(), before);
    assert_ne!(take(&pb.0), want);
    assert!(put(&pb.0, &want));
    assert_eq!(take(&pb.0), want);

    // An empty pasteboard comes back empty.
    assert!(put(&pb.0, &Snapshot::default()));
    assert_eq!(take(&pb.0), Snapshot::default());
  }

  /// The text is handed out at the read, which the callback hears; the markers come along.
  #[test]
  fn an_offer_tells_when_it_is_read() {
    let pb = Private::new("offer");
    let reads = Rc::new(Cell::new(0));
    let counted = reads.clone();
    let count = offer_on(&pb.0, "текст", Some(Box::new(move || counted.set(counted.get() + 1))));
    assert_eq!(pb.0.changeCount() as u64, count);
    assert_eq!(reads.get(), 0, "nothing read yet");

    // The request the pasteboard server sends on a read, here and not from a test thread's read,
    // which AppKit warns about.
    // SAFETY: a static of the framework.
    let string = unsafe { NSPasteboardTypeString };
    let owner = OWNERS.with_borrow(|o| o.back().cloned()).unwrap();
    let provide = |kind: &NSString| {
      let _: () = unsafe { msg_send![&owner, pasteboard: &*pb.0, provideDataForType: kind] };
    };
    provide(&NSString::from_str(MARKERS[0]));
    assert_eq!(reads.get(), 0, "a marker is not the text");
    provide(string);
    assert_eq!(reads.get(), 1);
    assert_eq!(pb.0.stringForType(string).map(|s| s.to_string()).as_deref(), Some("текст"));
    let types = pb.0.types().unwrap().iter().map(|t| t.to_string()).collect::<Vec<_>>();
    assert!(MARKERS.iter().all(|m| types.iter().any(|t| t == m)), "{types:?}");
    assert!(put(&pb.0, &Snapshot::default()));
    assert_eq!(pb.0.stringForType(string), None);
    assert!(owner.ivars().on_read.borrow().is_none(), "the promise went with the restore");
  }
}
