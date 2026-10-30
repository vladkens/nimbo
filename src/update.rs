// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! An app's update as the person sees it: one menu item and a window, as Sparkle's ([dialog]),
//! over a [Release] the app fills in from its updater. nimbo neither checks, downloads nor
//! installs: an updater such as shipo (<https://github.com/vladkens/shipo>) does, on a thread of
//! the app's own, and the app brings each result back to [dialog::Updater].

pub mod dialog;

/// A newer version on offer, as the window shows it; the app keeps its updater's own release to
/// download when [dialog::Updater::install] names this one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Release {
  pub version: String,
  /// The release's notes, a link the window offers ([dialog::Act::Notes]); none shows no link.
  pub notes: Option<String>,
  /// The download's size in bytes, shown with its progress; 0 shows none.
  pub size: u64,
}

impl Release {
  /// A release of `version` alone: no notes, no size.
  pub fn new(version: &str) -> Self {
    Self { version: version.into(), ..Self::default() }
  }
}
