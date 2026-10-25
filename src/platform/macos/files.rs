// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Files on macOS: the open and save panels a window asks for ([crate::FileDialog]), as a sheet
//! over it, and files dragged over a view or dropped on it.

use std::path::PathBuf;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject};
use objc2::{ClassType, Message, msg_send};
use objc2_app_kit::{
  NSDragOperation, NSDraggingInfo, NSModalResponse, NSModalResponseOK, NSOpenPanel, NSSavePanel,
  NSWindow,
};
use objc2_foundation::{NSArray, NSString, NSURL};

use super::view::View;
use crate::scene::Point;
use crate::{Event, FileDialog};

/// Shows `d` as a sheet over `window` and sends its answer to `view` as [Event::FileDialog]:
/// `None` at once while a sheet is up already.
pub fn show(view: &View, window: &NSWindow, d: FileDialog) {
  if window.attachedSheet().is_some() {
    view.send(Event::FileDialog(None));
    return;
  }

  let panel = match d.save {
    true => NSSavePanel::savePanel(super::mtm()),
    false => Retained::into_super(NSOpenPanel::openPanel(super::mtm())),
  };

  let ns = |s: &str| NSString::from_str(s);
  panel.setMessage(Some(&ns(&d.title)));
  if let Some(name) = &d.name {
    panel.setNameFieldStringValue(&ns(name));
  }

  if let Some(dir) = &d.dir {
    panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&ns(&dir.to_string_lossy()))));
  }

  // A content type per extension; AppKit loads UniformTypeIdentifiers, which has the class.
  if let Some(ut) = AnyClass::get(c"UTType").filter(|_| !d.types.is_empty()) {
    let types: Vec<Retained<AnyObject>> = (d.types.iter())
      .filter_map(|t| unsafe { msg_send![ut, typeWithFilenameExtension: &*ns(t)] })
      .collect();
    let _: () =
      unsafe { msg_send![&panel, setAllowedContentTypes: &*NSArray::from_retained_slice(&types)] };
  }

  let (view, picked) = (view.retain(), panel.clone());
  let done = RcBlock::new(move |r: NSModalResponse| {
    view.send(Event::FileDialog((r == NSModalResponseOK).then(|| path(picked.URL()?)).flatten()));
  });
  panel.beginSheetModalForWindow_completionHandler(window, &done);
}

/// A file URL's path.
fn path(url: Retained<NSURL>) -> Option<PathBuf> {
  Some(PathBuf::from(url.path().filter(|_| url.isFileURL())?.to_string()))
}

impl View {
  /// Files dragged over the view, or dropped (`drop`): [Event::DragOver] or [Event::Drop] at the
  /// pointer, as a copy; a drag without files is refused.
  pub fn drag(&self, info: &ProtocolObject<dyn NSDraggingInfo>, drop: bool) -> NSDragOperation {
    let classes = NSArray::from_slice(&[NSURL::class()]);
    let urls = unsafe { info.draggingPasteboard().readObjectsForClasses_options(&classes, None) };
    let paths: Vec<_> =
      urls.into_iter().flatten().filter_map(|o| path(o.downcast::<NSURL>().ok()?)).collect();
    if paths.is_empty() {
      return NSDragOperation::None;
    }

    let p = self.convertPoint_fromView(info.draggingLocation(), None);
    let pos = Point::new(p.x as f32, p.y as f32);
    self.send(if drop { Event::Drop { pos, paths } } else { Event::DragOver { pos, paths } });
    NSDragOperation::Copy
  }
}
