// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The app shell on real AppKit without showing anything: an accessory app whose start opens a
//! hidden window (made, never ordered in), reaches its app from outside, finds it again by id,
//! runs work through the proxy, closes it, and quits before the loop would start. A quit the
//! system asks for (`applicationShouldTerminate:`, sent here by hand) is answered «later»; the
//! quit hook runs once as `nimbo::app` ends.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use nimbo::{App, AppInfo, Config, Presence};
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationTerminateReply};

#[derive(Default)]
struct Counter {
  n: u32,
}

impl App for Counter {}

struct Other;

impl App for Other {}

pub fn main() {
  let mtm = MainThreadMarker::new().expect("main thread");
  let policy = || NSApplication::sharedApplication(mtm).activationPolicy();
  let closed = Rc::new(Cell::new(0));
  let quits = Rc::new(Cell::new(0));
  let info = AppInfo { id: "com.example.shell-test", name: "Test", presence: Presence::Accessory };
  let mut ran = false;
  nimbo::app(info, |main| {
    ran = true;
    assert_eq!(policy(), NSApplicationActivationPolicy::Accessory);
    let cfg = Config::new("Hidden", 200.0, 100.0).hidden();
    let w = main.open(&cfg, Counter::default());
    assert!(w.is_open());
    // Hidden, it puts no icon in the Dock.
    assert_eq!(policy(), NSApplicationActivationPolicy::Accessory);
    assert_eq!(w.with(|a| (a.n += 2, a.n).1), Some(2));
    let again = main.window::<Counter>(w.id()).expect("found by id");
    assert_eq!(again.with(|a| a.n), Some(2));
    assert!(main.window::<Other>(w.id()).is_none(), "another type");
    let other = main.open(&cfg, Other);
    assert_ne!(other.id(), w.id());

    // On the main thread the proxy runs at once; from another one it waits for the loop, which
    // does not run here, so the call times out.
    let proxy = main.proxy();
    let id = w.id();
    let got =
      proxy.call(move |m| m.window::<Counter>(id).and_then(|w| w.with(|a| a.n)), Duration::ZERO);
    assert_eq!(got, Some(Some(2)));
    let off = std::thread::spawn(move || proxy.call(|_| 1, Duration::from_millis(20)));
    assert_eq!(off.join().unwrap(), None);

    let c = closed.clone();
    w.on_close(move |_| c.set(c.get() + 1));
    w.close();
    assert!(!w.is_open() && !again.is_open());
    assert_eq!((w.with(|a| a.n), closed.get()), (None, 1));
    w.close();
    assert_eq!(closed.get(), 1, "closes once");
    assert!(other.is_open());

    let q = quits.clone();
    main.on_quit(move |_| q.set(q.get() + 1));
    // What `terminate:` asks; the answer comes later, from the loop, which does not run here.
    let ns_app = NSApplication::sharedApplication(mtm);
    let delegate = ns_app.delegate().expect("the shell's delegate");
    assert!(!main.terminating());
    let reply: NSApplicationTerminateReply =
      unsafe { msg_send![&*delegate, applicationShouldTerminate: &*ns_app] };
    assert_eq!(reply, NSApplicationTerminateReply::TerminateLater);
    assert!(main.terminating());
    assert_eq!(quits.get(), 0, "not from the delegate itself");
    main.quit();
  });
  assert!(ran);
  assert_eq!(quits.get(), 1, "the quit hook runs as the app ends");
  println!("shell scenario: ok");
}
