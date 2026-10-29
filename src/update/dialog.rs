// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The update as the person sees it, after Sparkle: one item in the app's menu says what the
//! updater does and opens its window, which offers the release, shows the download and the
//! restart, or why it failed. Nothing opens by itself: an update found waits in the menu item.
//!
//! [Updater] is the state without the network: the app runs [check](super::check),
//! [download](super::download) and [install](super::install) on threads of its own and brings
//! their results back to it; [menu_item] and [view] draw it.

use std::io::{self, ErrorKind};
use std::time::{Duration, SystemTime};

use super::Release;
use crate::tray::Item;
use crate::{Align, Button, Config, Png, Style, TextStyle, Ui, grow, px};

/// An automatic check once a day; one that got no answer is tried again at the next [TICK].
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
/// How often the app asks whether a check is [Updater::due]: a timer would stop while the Mac
/// sleeps.
pub const TICK: Duration = Duration::from_secs(60 * 60);

/// The window's content size in points.
pub const SIZE: (f32, f32) = (500.0, 196.0);

/// The ids of the window's controls.
pub const INSTALL: &str = "update-install";
pub const LATER: &str = "update-later";
pub const SKIP: &str = "update-skip";
pub const NOTES: &str = "update-notes";
pub const RETRY: &str = "update-retry";
pub const CLOSE: &str = "update-close";
pub const AUTO: &str = "update-auto";

/// `v1.2.0`, as an app shows a version.
fn shown(version: &str) -> String {
  format!("v{}", version.trim_start_matches('v'))
}

/// An error's text as a sentence: its first letter capital.
fn sentence(s: &str) -> String {
  let mut chars = s.chars();
  chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

// MARK: State

/// What the updater shows, in its menu item and its window.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
  /// Nothing: no answer yet, nothing newer, or the newer one put off or skipped.
  Idle,
  /// A check the person asked for runs; an automatic one shows nothing.
  Checking,
  /// The check the person asked for found nothing newer.
  UpToDate,
  Available(Release),
  /// With the whole percent done.
  Downloading(Release, u8),
  /// Downloaded: the app puts it in place once it can, and the new version starts.
  Restarting(Release),
  /// Why the check, the download or the install failed: with the release when it was its
  /// download or install, which Retry downloads again; else Retry checks again.
  Failed(Option<Release>, String),
}

impl State {
  /// The release being downloaded or put in place.
  fn busy(&self) -> bool {
    matches!(self, State::Downloading(..) | State::Restarting(_))
  }
}

/// The state, and when to check.
#[derive(Debug)]
pub struct Updater {
  pub state: State,
  /// A check runs: whether the person waits for it in the window.
  checking: Option<bool>,
  /// When a check last got an answer.
  answered: Option<SystemTime>,
  /// The app can't update itself ([super::supported] at launch, or a dev build's check): nothing
  /// shows and no check runs.
  off: bool,
}

impl Updater {
  /// `on`: [super::supported] at launch.
  pub fn new(on: bool) -> Self {
    Self { state: State::Idle, checking: None, answered: None, off: !on }
  }

  /// What the menu item and the window show, `None` when the app can't update.
  pub fn showing(&self) -> Option<&State> {
    (!self.off).then_some(&self.state)
  }

  /// An automatic check: none answered yet, or the last a day before `now` (or after it, the clock
  /// set back).
  pub fn due(&self, now: SystemTime) -> bool {
    let old = |t: SystemTime| now.duration_since(t).map_or(true, |d| d >= DAY);
    !self.off && self.checking.is_none() && self.answered.is_none_or(old)
  }

  /// Whether to start a check: not while one runs, which the person's joins, or while an update is
  /// on its way. The person's shows Checking.
  pub fn check(&mut self, manual: bool) -> bool {
    if self.off || self.state.busy() {
      return false;
    }

    if manual {
      self.state = State::Checking;
    }

    let start = self.checking.is_none();
    self.checking = Some(manual || self.checking == Some(true));
    start
  }

  /// The menu item opened the window: whether to start a check, when there is nothing to show
  /// but how one goes.
  pub fn open(&mut self) -> bool {
    let nothing = matches!(self.state, State::Idle | State::UpToDate | State::Failed(None, _));
    nothing && self.check(true)
  }

  /// A check's answer at `now`. A dev build's (`Unsupported`) turns the updater off. A missing feed
  /// (404) and one whose signature or content is wrong count as no update. A newer release shows
  /// unless it is the version `skipped` and nobody asked; nothing newer and an error show only to
  /// the person waiting in the window.
  pub fn checked(
    &mut self,
    found: io::Result<Option<Release>>,
    skipped: Option<&str>,
    now: SystemTime,
  ) {
    let Some(manual) = self.checking.take() else { return };
    let found = match found {
      Err(e) if e.kind() == ErrorKind::Unsupported => {
        (self.off, self.state) = (true, State::Idle);
        return;
      }
      Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::InvalidData) => Ok(None),
      found => found,
    };

    if found.is_ok() {
      self.answered = Some(now);
    }

    if self.state.busy() {
      return;
    }

    self.state = match found {
      Ok(Some(r)) if manual || skipped != Some(&r.version) => State::Available(r),
      Ok(_) if manual => State::UpToDate,
      Err(e) if manual => State::Failed(None, e.to_string()),
      _ => return,
    };
  }

  /// Install, or Retry after a failed download or install: the release to download.
  pub fn install(&mut self) -> Option<Release> {
    let (State::Available(r) | State::Failed(Some(r), _)) = &self.state else { return None };
    let r = r.clone();
    self.state = State::Downloading(r.clone(), 0);
    Some(r)
  }

  /// Remind Me Later: the next daily check offers it again.
  pub fn later(&mut self) {
    if matches!(self.state, State::Available(_) | State::Failed(..)) {
      self.state = State::Idle;
    }
  }

  /// Skip This Version: the version to save, which automatic checks pass over from then on.
  pub fn skip(&mut self) -> Option<String> {
    let State::Available(r) = &self.state else { return None };
    let version = r.version.clone();
    self.state = State::Idle;
    Some(version)
  }

  /// The download's whole percent ([percent]).
  pub fn progress(&mut self, done: u8) {
    if let State::Downloading(_, at) = &mut self.state {
      *at = done;
    }
  }

  pub fn downloaded(&mut self) {
    if let State::Downloading(r, _) = &self.state {
      self.state = State::Restarting(r.clone());
    }
  }

  /// The download or the install failed.
  pub fn failed(&mut self, e: &io::Error) {
    if let State::Downloading(r, _) | State::Restarting(r) = &self.state {
      self.state = State::Failed(Some(r.clone()), e.to_string());
    }
  }

  /// The update that started this app failed ([super::finish]): this one is the old version
  /// again, and Retry checks again.
  pub fn relaunched(&mut self, e: &io::Error) {
    self.state = State::Failed(None, e.to_string());
  }

  /// The window closed: what it said about a check is done with, and a check the person waited
  /// for goes on quietly; a release on offer or on its way stays in the menu item.
  pub fn closed(&mut self) {
    if self.checking == Some(true) {
      self.checking = Some(false);
    }

    if matches!(self.state, State::Checking | State::UpToDate | State::Failed(None, _)) {
      self.state = State::Idle;
    }
  }
}

/// The whole percent of `done` bytes of `total`: a download tells the app only when it changes.
pub fn percent(done: u64, total: u64) -> u8 {
  (done.saturating_mul(100) / total.max(1)).min(100) as u8
}

// MARK: Menu

/// The app's one menu item for the updater, `id`: Check for Updates… while there is nothing to
/// show, else what goes on. Each opens the window ([Updater::open]).
pub fn menu_item(id: &str, state: &State) -> Item {
  let label = match state {
    State::Idle | State::UpToDate => "Check for Updates…".to_owned(),
    State::Checking => "Checking for Updates…".to_owned(),
    State::Available(r) => format!("Update Available: {}…", shown(&r.version)),
    State::Downloading(_, done) => format!("Downloading Update: {done}%"),
    State::Restarting(_) => "Restarting to Update…".to_owned(),
    State::Failed(..) => "Update Failed…".to_owned(),
  };

  Item::new(id, &label)
}

// MARK: Window

/// What the person chose in the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
  Install,
  /// The download again, or the check when it was the check that failed.
  Retry,
  /// Remind Me Later.
  Later,
  /// Skip This Version.
  Skip,
  /// The release's notes, which the app opens.
  Notes,
  /// OK, Cancel, Hide: the window closes.
  Close,
  /// Check for updates automatically, set or cleared.
  Auto(bool),
}

/// The app the window is about.
#[derive(Clone, Copy)]
pub struct About<'a> {
  pub name: &'a str,
  /// The running version.
  pub version: &'a str,
  /// Shown at 64 points.
  pub icon: Option<Png>,
}

/// The window: a fixed size, `title` its title.
pub fn config(title: &'static str) -> Config {
  Config::new(title, SIZE.0, SIZE.1).fixed()
}

/// The window's content for `state`: the app's icon, what goes on and its buttons; returns what
/// the person chose. `auto`: whether the app checks by itself, a checkbox under an offer.
pub fn view(ui: &mut Ui, app: &About, state: &State, auto: bool) -> Option<Act> {
  let t = ui.theme();
  let (name, current) = (app.name, shown(app.version));
  let (title, text) = match state {
    State::Idle | State::Checking => ("Checking for updates…".to_owned(), String::new()),
    State::UpToDate => {
      ("You're up to date!".to_owned(), format!("{name} {current} is the newest version."))
    }
    State::Available(r) => (
      format!("A new version of {name} is available!"),
      format!("{name} {} is out, you have {current}. Install it now?", shown(&r.version)),
    ),
    State::Downloading(..) => ("Downloading the update…".to_owned(), String::new()),
    State::Restarting(r) => (
      "Installing the update…".to_owned(),
      format!("{name} restarts as {} once it can.", shown(&r.version)),
    ),
    State::Failed(Some(_), why) => ("The update failed".to_owned(), sentence(why)),
    State::Failed(None, why) => ("Couldn't check for updates".to_owned(), sentence(why)),
  };

  let mut act = None;
  let page = Style::new().grow().pads([20.0, 20.0, 16.0, 20.0]).gap(12.0).bg(t.bg);
  ui.col(page, |ui| {
    ui.row(Style::new().w(grow()).gap(16.0), |ui| {
      if let Some(icon) = app.icon {
        // 128 px: 64 points at 2x.
        ui.png_image(icon.side(128), Style::new().w(px(64.0)).h(px(64.0)));
      }

      ui.col(Style::new().w(grow()).gap(6.0), |ui| {
        ui.text(&title, TextStyle::new(t.bold, t.text).ellipsis());
        if !text.is_empty() {
          ui.text(&text, t.body());
        }

        match state {
          State::Downloading(r, done) => {
            ui.el(Style::new().h(px(2.0)));
            ui.progress(Some(f32::from(*done) / 100.0));
            if r.size > 0 {
              let size = |b: u64| format!("{:.1} MB", b as f64 / 1e6);
              let of = format!("{} of {}", size(r.size * u64::from(*done) / 100), size(r.size));
              ui.text(&of, t.muted());
            }
          }
          State::Idle | State::Checking | State::Restarting(_) => {
            ui.el(Style::new().h(px(2.0)));
            ui.progress(None);
          }
          State::Available(_) => {
            if ui.id(NOTES).link("Release notes", t.body().color(t.link)).clicked {
              act = Some(Act::Notes);
            }

            let mut on = auto;
            if ui.id(AUTO).checkbox(&mut on, "Check for updates automatically").changed {
              act = Some(Act::Auto(on));
            }
          }
          State::UpToDate | State::Failed(..) => {}
        }
      });
    });

    ui.spacer();
    // Under the text and the icon, the window's width: Sparkle's bar.
    let bar = Style::new().w(grow()).gap(8.0).align(Align::Start, Align::Center);
    ui.row(bar, |ui| {
      let mut button = |ui: &mut Ui, id, label, then, primary| {
        let b = if primary { Button::new(label).primary() } else { Button::new(label) };
        if b.show(ui.id(id)).clicked {
          act = Some(then);
        }
      };
      match state {
        State::Available(_) => {
          button(ui, SKIP, "Skip This Version", Act::Skip, false);
          ui.spacer();
          button(ui, LATER, "Remind Me Later", Act::Later, false);
          button(ui, INSTALL, "Install Update", Act::Install, true);
        }
        State::Failed(..) => {
          ui.spacer();
          button(ui, CLOSE, "Close", Act::Close, false);
          button(ui, RETRY, "Retry", Act::Retry, true);
        }
        State::UpToDate => {
          ui.spacer();
          button(ui, CLOSE, "OK", Act::Close, true);
        }
        State::Idle | State::Checking => {
          ui.spacer();
          button(ui, CLOSE, "Cancel", Act::Close, false);
        }
        State::Downloading(..) | State::Restarting(_) => {
          ui.spacer();
          button(ui, CLOSE, "Hide", Act::Close, false);
        }
      }
    });
  });
  act
}

#[cfg(test)]
mod tests {
  use super::*;

  const NOW: SystemTime = SystemTime::UNIX_EPOCH;

  fn r(v: &str) -> Release {
    Release::test(v)
  }

  fn found(v: &str) -> io::Result<Option<Release>> {
    Ok(Some(r(v)))
  }

  fn error(kind: ErrorKind) -> io::Result<Option<Release>> {
    Err(io::Error::new(kind, "offline"))
  }

  /// `u` after an automatic or a manual check answered `found`.
  fn checked(
    u: &mut Updater,
    manual: bool,
    found: io::Result<Option<Release>>,
    skip: Option<&str>,
  ) {
    assert!(u.check(manual));
    u.checked(found, skip, NOW);
  }

  #[test]
  fn the_percent_is_whole() {
    let got = [(0, 100), (419, 1_000), (999, 1_000), (1_000, 1_000), (5, 0), (u64::MAX, 2)];
    assert_eq!(got.map(|(done, total)| percent(done, total)), [0, 41, 99, 100, 100, 100]);
  }

  #[test]
  fn an_automatic_check_is_due_daily_and_after_a_failure() {
    let mut u = Updater::new(true);
    assert!(u.due(NOW), "at launch");
    assert!(u.check(false));
    assert!(!u.due(NOW) && !u.check(false), "one at a time");
    u.checked(error(ErrorKind::TimedOut), None, NOW);
    assert!(u.due(NOW), "no answer: again at the next tick");

    checked(&mut u, false, Ok(None), None);
    assert!(!u.due(NOW + DAY - Duration::from_secs(1)) && u.due(NOW + DAY));
    assert!(u.due(NOW - Duration::from_secs(1)), "the clock set back");
  }

  /// An app that can't update shows nothing and checks nothing: as said at launch, or as its
  /// check says for a dev build.
  #[test]
  fn an_app_that_cant_update_shows_nothing() {
    let mut u = Updater::new(false);
    assert_eq!(u.showing(), None);
    assert!(!u.due(NOW) && !u.check(true) && !u.open());

    let mut u = Updater::new(true);
    assert_eq!(u.showing(), Some(&State::Idle));
    checked(&mut u, true, error(ErrorKind::Unsupported), None);
    assert_eq!(u.showing(), None);
    assert!(!u.due(NOW) && !u.check(true));
  }

  #[test]
  fn an_automatic_check_shows_only_what_is_new() {
    let mut u = Updater::new(true);
    checked(&mut u, false, Ok(None), None);
    checked(&mut u, false, error(ErrorKind::ConnectionRefused), None);
    checked(&mut u, false, found("1.2.0"), Some("1.2.0"));
    assert_eq!(u.state, State::Idle, "nothing new, offline, skipped: quiet");

    checked(&mut u, false, found("1.3.0"), Some("1.2.0"));
    assert_eq!(u.state, State::Available(r("1.3.0")));
    u.later();
    assert_eq!(u.state, State::Idle);
    checked(&mut u, false, found("1.3.0"), Some("1.2.0"));
    assert_eq!(u.state, State::Available(r("1.3.0")), "put off until the next check");
    assert_eq!(u.skip().as_deref(), Some("1.3.0"));
    assert_eq!((u.state.clone(), u.skip()), (State::Idle, None));
  }

  /// The menu item opens the window with a check when there is nothing else to show; the check
  /// says how it went until the window closes.
  #[test]
  fn opening_the_window_checks_and_says_how_it_went() {
    let mut u = Updater::new(true);
    assert!(u.open());
    assert_eq!(u.state, State::Checking);
    u.checked(Ok(None), None, NOW);
    assert_eq!(u.state, State::UpToDate);
    u.closed();
    assert_eq!(u.state, State::Idle);

    // A missing feed and a wrong signature are no update; another error shows, Retry checks again.
    for kind in [ErrorKind::NotFound, ErrorKind::InvalidData] {
      assert!(u.open());
      u.checked(error(kind), None, NOW);
      assert_eq!(u.state, State::UpToDate, "{kind:?}");
    }

    assert!(u.open());
    u.checked(error(ErrorKind::TimedOut), None, NOW);
    assert_eq!(u.state, State::Failed(None, "offline".into()));
    assert_eq!(u.install(), None, "nothing to download: Retry checks again");
    assert!(u.open());
    u.checked(found("1.2.0"), Some("1.2.0"), NOW);
    assert_eq!(u.state, State::Available(r("1.2.0")), "asked, a skipped one shows too");
    assert!(!u.open(), "the offer shows as it is");
    u.closed();
    assert_eq!(u.state, State::Available(r("1.2.0")), "the menu item keeps it");
  }

  /// The window closed during the person's check: it goes on quietly.
  #[test]
  fn a_check_left_goes_on_quietly() {
    let mut u = Updater::new(true);
    assert!(u.open());
    u.closed();
    assert_eq!(u.state, State::Idle);
    assert!(!u.check(false), "it still runs");
    u.checked(Ok(None), None, NOW);
    assert_eq!(u.state, State::Idle, "nothing newer: quiet");

    assert!(u.open());
    u.closed();
    u.checked(found("1.2.0"), None, NOW);
    assert_eq!(u.state, State::Available(r("1.2.0")), "a release still shows in the menu");
  }

  #[test]
  fn the_persons_check_joins_an_automatic_one() {
    let mut u = Updater::new(true);
    assert!(u.check(false));
    assert!(!u.open(), "no second thread");
    assert_eq!(u.state, State::Checking);
    u.checked(Ok(None), None, NOW);
    assert_eq!(u.state, State::UpToDate);
  }

  #[test]
  fn an_install_downloads_restarts_or_fails_with_retry() {
    let mut u = Updater::new(true);
    assert_eq!(u.install(), None);
    checked(&mut u, false, found("1.2.0"), None);
    assert_eq!(u.install(), Some(r("1.2.0")));
    assert_eq!(u.state, State::Downloading(r("1.2.0"), 0));
    assert!(!u.check(true) && !u.check(false) && !u.open(), "no check while it downloads");
    u.progress(percent(41_990, 100_000));
    assert_eq!(u.state, State::Downloading(r("1.2.0"), 41));
    u.closed();
    assert_eq!(u.state, State::Downloading(r("1.2.0"), 41), "hidden, it goes on");

    let e = io::Error::other("1.2.0 is not the file");
    u.failed(&e);
    assert_eq!(u.state, State::Failed(Some(r("1.2.0")), "1.2.0 is not the file".into()));
    assert_eq!(u.install(), Some(r("1.2.0")), "Retry");
    u.downloaded();
    assert_eq!(u.state, State::Restarting(r("1.2.0")));
    u.failed(&e);
    u.later();
    assert_eq!(u.state, State::Idle, "a failure put off");

    // The update that started this app failed: Retry checks again.
    u.relaunched(&io::Error::other("the new app did not start"));
    assert_eq!(u.state, State::Failed(None, "the new app did not start".into()));
    assert_eq!(u.install(), None);
    u.later();

    // A check answered during the download leaves it be.
    checked(&mut u, false, found("1.2.0"), None);
    assert!(u.check(false));
    u.install();
    u.checked(found("1.3.0"), None, NOW);
    assert_eq!(u.state, State::Downloading(r("1.2.0"), 0));
  }

  #[test]
  fn the_menu_item_says_what_goes_on() {
    let label = |s: State| menu_item("update", &s).label;
    assert_eq!(label(State::Idle), "Check for Updates…");
    assert_eq!(label(State::UpToDate), "Check for Updates…");
    assert_eq!(label(State::Checking), "Checking for Updates…");
    assert_eq!(label(State::Available(r("1.2.0"))), "Update Available: v1.2.0…");
    assert_eq!(label(State::Downloading(r("1.2.0"), 42)), "Downloading Update: 42%");
    assert_eq!(label(State::Restarting(r("1.2.0"))), "Restarting to Update…");
    assert_eq!(label(State::Failed(None, "offline".into())), "Update Failed…");
    assert_eq!(menu_item("update", &State::Idle).id, "update");
  }
}
