// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The view's display link, which calls [View::on_tick] at the display's refresh while unpaused,
//! and the timer wake ([Wake]). On macOS 14+ it is NSView's `CADisplayLink`: it follows the view's
//! screen and its preferred rate (60–120 Hz on ProMotion). Below 14, or with `NIMBO_CVDISPLAYLINK`
//! set (read when a link is made; the fallback's pacing checked on a current Mac, as `NIMBO_TRACE`
//! logs frames), a `CVDisplayLink` per display ticks the views subscribed to it through a
//! coalescing dispatch source each on the main queue; its link starts with the first subscriber
//! and stops with the last, so a still window has no wakeups.
//!
//! The fallback is written from Apple's `CoreVideo/CVDisplayLink.h`; no code is taken from the
//! `display-link` crate (BrainiumLLC, MIT) or from Zed's GPUI (`gpui_macos/src/display_link.rs`,
//! Apache-2.0), whose crash lessons it follows: a display's link is made once and never released
//! (`CVDisplayLinkStop` returns before its thread is done), the callback's context is the display
//! id and it touches only a static registry (a late callback finds nothing to free), and no
//! CoreVideo call is made while the registry's lock is held. CoreVideo's functions (deprecated
//! since macOS 15) are looked up with `dlsym`, never linked; missing, the views tick every 1/60 s
//! on a dispatch timer, said once on stderr.

use std::cell::{Cell, OnceCell};
use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{
  _dispatch_source_type_data_add, _dispatch_source_type_timer, DispatchObject, DispatchQueue,
  DispatchRetained, DispatchSource, DispatchTime, dispatch_source_type_t,
};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::NSScreen;
use objc2_foundation::{NSNumber, NSRunLoop, NSRunLoopCommonModes, ns_string};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange};

use super::view::{View, WeakView};

/// What paces a view's frames.
pub(super) enum Link {
  /// NSView's display link (macOS 14+), calling the view's `tick:`.
  Ca(Retained<CADisplayLink>),
  /// The `CVDisplayLink` of the window's display, below 14.
  Cv(CvLink),
}

impl Link {
  /// A paused link for `view`.
  pub(super) fn new(view: &View) -> Self {
    let cv = std::env::var_os("NIMBO_CVDISPLAYLINK").is_some();
    if objc2::available!(macos = 14.0) && !cv {
      Self::Ca(display_link(view))
    } else {
      Self::Cv(CvLink::new(view))
    }
  }

  pub(super) fn set_paused(&self, paused: bool) {
    match self {
      Self::Ca(link) => link.setPaused(paused),
      Self::Cv(link) => link.set_paused(paused),
    }
  }

  pub(super) fn is_paused(&self) -> bool {
    match self {
      Self::Ca(link) => link.isPaused(),
      Self::Cv(link) => !matches!(link.pace.get(), Pace::Display(_) | Pace::Timer),
    }
  }

  /// Stops the link for good: NSView's releases the view it retains.
  pub(super) fn invalidate(&self) {
    match self {
      Self::Ca(link) => link.invalidate(),
      Self::Cv(link) => link.invalidate(),
    }
  }

  /// The display's refresh period in seconds, 0 while unknown.
  fn duration(&self) -> f64 {
    match self {
      Self::Ca(link) => link.duration(),
      Self::Cv(link) => link.duration(),
    }
  }

  /// The window moved to another screen: a running `CVDisplayLink` subscription moves with it.
  /// NSView's link follows by itself.
  pub(super) fn screen_changed(&self) {
    if let Self::Cv(link) = self {
      link.screen_changed();
    }
  }
}

/// A display link calling `tick:`, at up to 120 Hz, on the main run loop in every mode.
fn display_link(view: &View) -> Retained<CADisplayLink> {
  let link = unsafe { view.displayLinkWithTarget_selector(view, sel!(tick:)) };
  link.setPreferredFrameRateRange(CAFrameRateRange {
    minimum: 60.0,
    maximum: 120.0,
    preferred: 120.0,
  });
  unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
  link
}

/// The display's refresh interval from its link (a 120 Hz one's until it has run).
pub(super) fn refresh(link: &Link) -> Duration {
  let d = link.duration();
  let d = if d > 0.0 { d.clamp(1.0 / 120.0, 1.0 / 24.0) } else { 1.0 / 120.0 };
  Duration::from_secs_f64(d)
}

// MARK: CVDisplayLink

/// What ticks a [CvLink].
#[derive(Clone, Copy, PartialEq)]
enum Pace {
  Off,
  /// Subscribed to this display's link.
  Display(u32),
  /// A dispatch timer every 1/60 s: CoreVideo or a display is missing.
  Timer,
  /// Invalidated: never runs again.
  Gone,
}

/// A view's subscription to its display's `CVDisplayLink`.
pub(super) struct CvLink {
  view: WeakView,
  /// Ticks the view on the main queue: the callbacks that came meanwhile make one tick.
  source: DispatchRetained<DispatchSource>,
  /// The 1/60 s timer, made when first needed.
  timer: OnceCell<Wake>,
  pace: Cell<Pace>,
  /// The display last subscribed to, whose refresh period paces the timer frames.
  display: Cell<Option<u32>>,
}

impl CvLink {
  fn new(view: &View) -> Self {
    let ty = (&raw const _dispatch_source_type_data_add).cast_mut();
    let source = source(ty, DispatchQueue::main(), ticks(view.weak()));
    // Resumed at once: a source let go while inactive crashes.
    source.activate();
    let (timer, pace, display) = (OnceCell::new(), Cell::new(Pace::Off), Cell::new(None));
    Self { view: view.weak(), source, timer, pace, display }
  }

  fn set_paused(&self, paused: bool) {
    match (self.pace.get(), paused) {
      (Pace::Off, false) => self.run(),
      (Pace::Display(_) | Pace::Timer, true) => self.halt(),
      _ => {}
    }
  }

  /// Subscribes to the window's display, the main one without a window; the timer when
  /// CoreVideo or a display is missing.
  fn run(&self) {
    let pace = match (core_video(), self.screen()) {
      (Some(cv), Some(id)) if subscribe(cv, id, &self.source) => Pace::Display(id),
      _ => {
        let view = self.view.clone();
        let timer = self.timer.get_or_init(|| Wake::new(DispatchQueue::main(), ticks(view)));
        timer.repeat(Duration::from_secs(1) / 60);
        Pace::Timer
      }
    };

    if let Pace::Display(id) = pace {
      self.display.set(Some(id));
    }

    self.pace.set(pace);
  }

  fn halt(&self) {
    match self.pace.replace(Pace::Off) {
      Pace::Display(id) => unsubscribe(id, &self.source),
      Pace::Timer => self.timer.get().into_iter().for_each(Wake::disarm),
      Pace::Off | Pace::Gone => {}
    }
  }

  fn invalidate(&self) {
    self.halt();
    self.pace.set(Pace::Gone);
    self.source.cancel();
  }

  fn screen_changed(&self) {
    let moved = match self.pace.get() {
      Pace::Display(id) => self.screen() != Some(id),
      Pace::Timer => true,
      Pace::Off | Pace::Gone => false,
    };

    if moved {
      self.halt();
      self.run();
    }
  }

  /// The display id of the window's screen, or of the main screen.
  fn screen(&self) -> Option<u32> {
    let mtm = MainThreadMarker::new()?;
    let window = self.view.get(mtm).load().and_then(|v| v.window());
    let screen =
      window.and_then(|w| w.screen()).or_else(|| NSScreen::screens(mtm).firstObject())?;
    let id = screen.deviceDescription().objectForKey(ns_string!("NSScreenNumber"))?;
    Some(id.downcast::<NSNumber>().ok()?.unsignedIntValue())
  }

  /// The display's measured refresh period, its nominal one until it has run; the timer's.
  fn duration(&self) -> f64 {
    if self.pace.get() == Pace::Timer {
      return 1.0 / 60.0;
    }

    let id = self.display.get();
    let link = id.and_then(|id| displays().iter().find(|d| d.id == id).map(|d| d.link));
    let (Some(cv), Some(link)) = (core_video(), link) else { return 0.0 };
    // SAFETY: a link made by `make`, never released.
    let (actual, nominal) = unsafe { ((cv.actual)(link.0), (cv.nominal)(link.0)) };
    let known = nominal.flags & INDEFINITE == 0 && nominal.scale > 0;
    let nominal = if known { nominal.value as f64 / nominal.scale as f64 } else { 0.0 };
    if actual > 0.0 { actual } else { nominal }
  }
}

impl Drop for CvLink {
  fn drop(&mut self) {
    self.halt();
    self.source.cancel();
  }
}

/// A handler that ticks the view, if it is still there.
fn ticks(view: WeakView) -> impl Fn() + 'static {
  move || {
    let Some(mtm) = MainThreadMarker::new() else { return };
    if let Some(view) = view.get(mtm).load() {
      view.on_tick();
    }
  }
}

/// A display's link and the sources of the views subscribed to it. The link is made the first
/// time a view subscribes to the display and never released.
struct Display {
  id: u32,
  link: LinkRef,
  views: Vec<DispatchRetained<DispatchSource>>,
}

/// A `CVDisplayLinkRef`, which CoreVideo lets any thread use.
#[derive(Clone, Copy)]
struct LinkRef(Ref);

// SAFETY: CVDisplayLink is a thread-safe CoreFoundation type.
unsafe impl Send for LinkRef {}

static DISPLAYS: Mutex<Vec<Display>> = Mutex::new(Vec::new());

/// The registry, also after a panic elsewhere: the callback must not panic.
fn displays() -> MutexGuard<'static, Vec<Display>> {
  DISPLAYS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Adds the view's `source` to display `id`'s link, starting it for the first subscriber; false
/// when CoreVideo can't make the display's link.
fn subscribe(cv: &CoreVideo, id: u32, source: &DispatchRetained<DispatchSource>) -> bool {
  // Only the main thread changes the registry: no other adds the display meanwhile.
  let known = displays().iter().find(|d| d.id == id).map(|d| d.link);
  let Some(link) = known.or_else(|| make(cv, id)) else { return false };
  let first = {
    let mut displays = displays();
    if known.is_none() {
      displays.push(Display { id, link, views: Vec::new() });
    }

    let Some(d) = displays.iter_mut().find(|d| d.id == id) else { return false };
    d.views.push(source.clone());
    d.views.len() == 1
  };

  if first {
    // SAFETY: a link made by `make`, its callback set.
    unsafe { (cv.start)(link.0) };
  }

  true
}

/// Takes the view's `source` off display `id`'s link, stopping it after the last subscriber.
fn unsubscribe(id: u32, source: &DispatchSource) {
  let last = {
    let mut displays = displays();
    let Some(d) = displays.iter_mut().find(|d| d.id == id) else { return };
    let had = d.views.len();
    d.views.retain(|s| !std::ptr::eq(&**s, source));
    (d.views.len() < had && d.views.is_empty()).then_some(d.link)
  };

  if let (Some(cv), Some(link)) = (core_video(), last) {
    // SAFETY: a link made by `make`.
    unsafe { (cv.stop)(link.0) };
  }
}

/// Display `id`'s link, its callback set with the id as its context.
fn make(cv: &CoreVideo, id: u32) -> Option<LinkRef> {
  let mut link = std::ptr::null_mut();
  // SAFETY: an out pointer for the new link; the context is a number, never dereferenced.
  let made = unsafe { (cv.create)(id, &mut link) } == SUCCESS && !link.is_null();
  let set = made && unsafe { (cv.set_output)(link, output, id as usize as *mut c_void) } == SUCCESS;
  set.then_some(LinkRef(link))
}

/// The display's callback, on CoreVideo's thread: ticks the sources of the views on it. It never
/// panics and touches nothing but the registry.
unsafe extern "C" fn output(
  _: Ref,
  _: *const Time,
  _: *const Time,
  _: u64,
  _: *mut u64,
  cx: Ref,
) -> i32 {
  let id = cx as usize as u32;
  for d in displays().iter().filter(|d| d.id == id) {
    d.views.iter().for_each(|s| s.merge_data(1));
  }

  SUCCESS
}

// CoreVideo/CVReturn.h, CVBase.h.
const SUCCESS: i32 = 0;
const INDEFINITE: i32 = 1;

/// `CVTime`.
#[repr(C)]
struct CvTime {
  value: i64,
  scale: i32,
  flags: i32,
}

/// A `CVDisplayLinkRef`; a `CVTimeStamp`, never read.
type Ref = *mut c_void;
type Time = c_void;
type Output = unsafe extern "C" fn(Ref, *const Time, *const Time, u64, *mut u64, Ref) -> i32;

/// The `CVDisplayLink` functions nimbo calls, as `CVDisplayLink.h` declares them.
struct CoreVideo {
  create: unsafe extern "C" fn(u32, *mut Ref) -> i32,
  set_output: unsafe extern "C" fn(Ref, Output, Ref) -> i32,
  start: unsafe extern "C" fn(Ref) -> i32,
  stop: unsafe extern "C" fn(Ref) -> i32,
  actual: unsafe extern "C" fn(Ref) -> f64,
  nominal: unsafe extern "C" fn(Ref) -> CvTime,
}

// libSystem, <dlfcn.h>.
unsafe extern "C" {
  fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
  fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

const RTLD_LAZY: c_int = 0x1;
const RTLD_LOCAL: c_int = 0x4;

/// CoreVideo's functions, looked up once; `None` (said once) when any is missing.
fn core_video() -> Option<&'static CoreVideo> {
  static TABLE: OnceLock<Option<CoreVideo>> = OnceLock::new();
  let table = TABLE.get_or_init(|| {
    let table = load();
    if table.is_none() {
      eprintln!("nimbo: no CVDisplayLink in CoreVideo: frames are paced at 60 Hz");
    }

    table
  });
  table.as_ref()
}

fn load() -> Option<CoreVideo> {
  let path = c"/System/Library/Frameworks/CoreVideo.framework/CoreVideo";
  // SAFETY: a NUL-terminated path; the handle stays open for the process.
  let lib =
    Some(unsafe { dlopen(path.as_ptr(), RTLD_LAZY | RTLD_LOCAL) }).filter(|l| !l.is_null())?;
  // SAFETY: each function has the type its field gives, as `CVDisplayLink.h` declares it.
  unsafe {
    Some(CoreVideo {
      create: sym(lib, c"CVDisplayLinkCreateWithCGDisplay")?,
      set_output: sym(lib, c"CVDisplayLinkSetOutputCallback")?,
      start: sym(lib, c"CVDisplayLinkStart")?,
      stop: sym(lib, c"CVDisplayLinkStop")?,
      actual: sym(lib, c"CVDisplayLinkGetActualOutputVideoRefreshPeriod")?,
      nominal: sym(lib, c"CVDisplayLinkGetNominalOutputVideoRefreshPeriod")?,
    })
  }
}

/// Function `name` of the open library `lib`, as an `F`.
///
/// # Safety
///
/// `F` is a function pointer of the function's type.
unsafe fn sym<F: Copy>(lib: *mut c_void, name: &CStr) -> Option<F> {
  // SAFETY: an open handle and a NUL-terminated name; `F` is pointer-sized, as the caller vouches.
  let p = unsafe { dlsym(lib, name.as_ptr()) };
  (!p.is_null()).then(|| unsafe { std::mem::transmute_copy::<*mut c_void, F>(&p) })
}

// MARK: Timers

/// A source of type `ty` on `queue` calling `handler`, not yet activated.
fn source(
  ty: dispatch_source_type_t,
  queue: &DispatchQueue,
  handler: impl Fn() + 'static,
) -> DispatchRetained<DispatchSource> {
  // SAFETY: a timer or data source takes no handle or mask.
  let source = unsafe { DispatchSource::new(ty, 0, 0, Some(queue)) };
  let block = RcBlock::new(handler);
  // SAFETY: dispatch copies the block; it runs on `queue`, the thread `handler` was made for.
  unsafe { source.set_event_handler_with_block(RcBlock::as_ptr(&block)) };
  source
}

/// One timer on a queue, set again for each new deadline: it fires only at the last one, and
/// not at all once disarmed. `dispatch_after` blocks can't be taken back: each superseded one
/// would still wake the process when its time came.
pub(super) struct Wake(DispatchRetained<DispatchSource>);

impl Wake {
  pub(super) fn new(queue: &DispatchQueue, fired: impl Fn() + 'static) -> Self {
    let ty = (&raw const _dispatch_source_type_timer).cast_mut();
    let wake = Self(source(ty, queue, fired));
    wake.disarm();
    wake.0.activate();
    wake
  }

  /// Fires once, `after` from now (with dispatch_after's leeway: a tenth, within 1 ms and 60 s),
  /// instead of at any deadline set before.
  pub(super) fn arm(&self, after: Duration) {
    let at = DispatchTime::try_from(after).unwrap_or(DispatchTime::NOW);
    let leeway = (after / 10).clamp(Duration::from_millis(1), Duration::from_secs(60));
    self.0.set_timer(at, DispatchTime::FOREVER.0, leeway.as_nanos() as u64);
  }

  /// Fires `every` from now on, until disarmed or set again.
  fn repeat(&self, every: Duration) {
    let at = DispatchTime::try_from(every).unwrap_or(DispatchTime::NOW);
    self.0.set_timer(at, every.as_nanos() as u64, (every / 10).as_nanos() as u64);
  }

  pub(super) fn disarm(&self) {
    self.0.set_timer(DispatchTime::FOREVER, DispatchTime::FOREVER.0, 0);
  }
}

impl Drop for Wake {
  fn drop(&mut self) {
    self.0.cancel();
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};

  use super::*;

  /// A timer wake set again fires only at its last deadline, and not at all once disarmed: a
  /// superseded deadline wakes nothing.
  #[test]
  fn a_wake_set_again_fires_once() {
    let fired = Arc::new(AtomicUsize::new(0));
    let n = fired.clone();
    let queue = DispatchQueue::new("nimbo.wake", None);
    let wake = Wake::new(&queue, move || {
      n.fetch_add(1, Ordering::SeqCst);
    });
    let ms = Duration::from_millis;
    for t in [20, 40, 60] {
      wake.arm(ms(t));
    }

    std::thread::sleep(ms(250));
    assert_eq!(fired.load(Ordering::SeqCst), 1);

    wake.arm(ms(20));
    wake.disarm();
    std::thread::sleep(ms(100));
    assert_eq!(fired.load(Ordering::SeqCst), 1);
  }

  /// CoreVideo's display link functions are found without linking it.
  #[test]
  fn core_video_is_found() {
    assert!(core_video().is_some());
  }
}
