// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Activities on macOS ([crate::activity]): `NSProcessInfo`'s, user-initiated.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSActivityOptions, NSObjectProtocol, NSProcessInfo, NSString};

/// An activity's token.
pub struct Token(Retained<ProtocolObject<dyn NSObjectProtocol>>);

// SAFETY: `NSProcessInfo` begins and ends activities from any thread; the token is only handed
// back to it.
unsafe impl Send for Token {}

pub fn begin(reason: &str) -> Token {
  let (info, why) = (NSProcessInfo::processInfo(), NSString::from_str(reason));
  Token(info.beginActivityWithOptions_reason(NSActivityOptions::UserInitiated, &why))
}

pub fn end(t: Token) {
  unsafe { NSProcessInfo::processInfo().endActivity(&t.0) };
}
