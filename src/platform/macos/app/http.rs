// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A file fetched over HTTP on macOS: an `NSURLSession` download task, its file moved into place
//! by the completion handler, which runs on the session's queue while the caller waits. The shared
//! session follows redirects by its defaults, to http too: the feed's signature and the file's
//! hash guard what comes.

use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;
use std::{fs, io};

use block2::RcBlock;
use objc2_foundation::{
  NSError, NSHTTPURLResponse, NSString, NSURL, NSURLRequest, NSURLRequestCachePolicy,
  NSURLResponse, NSURLSession,
};

pub fn get(url: &str, to: &Path, max: u64) -> io::Result<()> {
  let bad = || io::Error::new(io::ErrorKind::InvalidInput, format!("{url} is not a URL"));
  let u = NSURL::URLWithString(&NSString::from_str(url)).ok_or_else(bad)?;
  let policy = NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData;
  let request = NSURLRequest::requestWithURL_cachePolicy_timeoutInterval(&u, policy, 60.0);
  let (tx, rx) = mpsc::channel();
  let (link, to) = (url.to_owned(), to.to_owned());
  let done = RcBlock::new(move |file: *mut NSURL, res: *mut NSURLResponse, err: *mut NSError| {
    // SAFETY: each is null or an object that lives through the call.
    let (file, res, err) = unsafe { (file.as_ref(), res.as_ref(), err.as_ref()) };
    let code =
      res.and_then(|r| r.downcast_ref::<NSHTTPURLResponse>()).map_or(0, |r| r.statusCode());
    let r = match (err, file.and_then(|f| f.path())) {
      (Some(e), _) => Err(io::Error::other(format!("{link}: {}", e.localizedDescription()))),
      (None, None) => Err(io::Error::other(format!("{link}: no file"))),
      (None, Some(from)) => crate::platform::status(&link, code as u32).and_then(|_| {
        // The task's file is removed once the handler returns; it may be on another volume.
        let from = from.to_string();
        let fits = fs::metadata(&from)?.len() <= max;
        fits.then_some(()).ok_or_else(|| crate::platform::too_big(&link, max))?;
        fs::rename(&from, &to).or_else(|_| fs::copy(&from, &to).map(drop))
      }),
    };

    let _ = tx.send(r);
  });

  // SAFETY: the handler only owns sendable values (a channel's sender, two strings).
  let task = unsafe {
    NSURLSession::sharedSession().downloadTaskWithRequest_completionHandler(&request, &done)
  };
  task.resume();
  // Stopped once past `max`; the handler checks the file it got.
  let never = || io::Error::other(format!("{url}: the task never ended"));
  loop {
    match rx.recv_timeout(Duration::from_millis(50)) {
      Ok(r) => return r,
      Err(RecvTimeoutError::Timeout) if task.countOfBytesReceived() as u64 <= max => {}
      Err(RecvTimeoutError::Timeout) => break task.cancel(),
      Err(RecvTimeoutError::Disconnected) => return Err(never()),
    }
  }

  Err(crate::platform::too_big(url, max))
}
