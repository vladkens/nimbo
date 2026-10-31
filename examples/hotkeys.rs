// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A demo of both ways to pick a dictation model from the hotkey ([nimbo::hotkey]): three
//! hotkeys, each with a model of its own, and three keys taken while any of them is held, the
//! first key of a dictation picking its model (fn, then Z: Z's model; fn-Z never types a z in
//! the app in front). Both work at once. Under the recorders: the model in use, the last edges
//! (also printed to stderr as `hotkeys: Down 0`, `hotkeys: Key 1`, by their index in
//! [Hotkeys::set]) and the hotkeys' status with Start for the event tap (macOS: it needs
//! Accessibility access; chords work without it, keys don't). While a recorder records, the
//! hotkeys are paused.
//!
//! `cargo run --release --example hotkeys [-- --shot <file.png>]`; the shot is a dictation by
//! fn whose first key was Z, then X ignored.

// A Windows app, not a console one: no terminal opens with it.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use nimbo::hotkey::{Edge, Hotkeys, Listener, Status};
use nimbo::{
  App, AppInfo, Config, FormRow, FormStyle, Hotkey, Main, Presence, Style, Ui, Window, grow, px,
};

pub const INFO: AppInfo =
  AppInfo { id: "com.example.nimbo-hotkeys", name: "Nimbo Hotkeys", presence: Presence::Regular };
pub const HOTKEY: &str = "hotkey";
pub const KEY: &str = "key";
pub const MODEL: &str = "model";
pub const LOG: &str = "log";
pub const START: &str = "start";
pub const LISTENER: &str = "listener";
pub const KEYS_STATUS: &str = "keys-status";
/// The edges shown, the last at the bottom.
pub const ROWS: usize = 8;
/// A log row's height: one in every state.
pub const ROW_H: f32 = 18.0;

/// The demo's state: the recorders, the dictation under way and what the hotkeys said.
pub struct Demo {
  pub hotkeys: [Option<Hotkey>; 3],
  pub keys: [Option<Hotkey>; 3],
  /// The recorder of each hotkey and key as last handed to [Hotkeys::set], by its index there.
  pub sent: (Vec<usize>, Vec<usize>),
  /// Why `set` refused one.
  pub error: Option<String>,
  /// The hotkeys held, a bit per recorder, and whether a key picked the model since the first
  /// went down.
  pub held: u8,
  pub keyed: bool,
  pub model: String,
  pub log: VecDeque<String>,
  pub status: Option<Status>,
  /// A recorder records: the hotkeys are paused.
  pub paused: bool,
}

impl Default for Demo {
  fn default() -> Self {
    let hk = |t: &str| Hotkey::parse(t).ok();
    let mut demo = Self {
      hotkeys: [hk("fn"), hk("option+space"), hk("f2")],
      keys: [hk("z"), hk("x"), hk("1")],
      sent: Default::default(),
      error: None,
      held: 0,
      keyed: false,
      model: "Hotkey 1's model".into(),
      log: VecDeque::new(),
      status: None,
      paused: false,
    };
    demo.apply();
    demo
  }
}

thread_local! {
  /// The hotkeys, made in `main` only: tests have none.
  static HOTKEYS: RefCell<Option<Hotkeys>> = RefCell::default();
  /// The window, for the edges posted to the main thread.
  static WINDOW: RefCell<Option<Window<Demo>>> = RefCell::default();
  /// Edges not yet handed to the window (it was busy), in order; a retry is set.
  static EDGES: RefCell<(VecDeque<Edge>, bool)> = RefCell::default();
}

/// `f` on the hotkeys, unless there are none (tests) or they are busy.
fn hotkeys<R>(f: impl FnOnce(&mut Hotkeys) -> R) -> Option<R> {
  HOTKEYS.with(|h| h.try_borrow_mut().ok()?.as_mut().map(f))
}

/// An edge as stderr has it: the slot's or key's index in [Hotkeys::set].
pub fn describe(e: Edge) -> String {
  match e {
    Edge::Down { which, repeat: false, .. } => format!("Down {which}"),
    Edge::Down { which, repeat: true, .. } => format!("Down {which} repeat"),
    Edge::Up { which, .. } => format!("Up {which}"),
    Edge::Key { index, .. } => format!("Key {index}"),
    Edge::Escape => "Escape".into(),
    Edge::Aborted { which } => format!("Aborted {which}"),
  }
}

impl Demo {
  /// Hands the recorders' hotkeys and keys to [Hotkeys::set], the empty ones left out.
  pub fn apply(&mut self) {
    let set = |list: &[Option<Hotkey>; 3]| -> (Vec<Hotkey>, Vec<usize>) {
      list.iter().enumerate().filter_map(|(i, h)| Some(((*h)?, i))).unzip()
    };
    let ((hot, hot_at), (keys, keys_at)) = (set(&self.hotkeys), set(&self.keys));
    self.sent = (hot_at, keys_at);
    self.error = hotkeys(|h| h.set(&hot, &keys)).and_then(Result::err);
  }

  /// The app's own hotkey state machine: a press starts a dictation with its hotkey's model; the
  /// first key after it picks the key's model, later ones are ignored until the release or the
  /// abort.
  pub fn edge(&mut self, e: Edge) {
    let hotkey = |which: usize| self.sent.0.get(which).copied();
    let line = match e {
      Edge::Down { which, repeat: false, .. } => {
        let Some(i) = hotkey(which) else { return };
        // A dictation starts with the first hotkey down; another one meanwhile joins it.
        if self.held == 0 {
          (self.keyed, self.model) = (false, format!("Hotkey {}'s model", i + 1));
        }

        self.held |= 1 << i;
        format!("Down: hotkey {}", i + 1)
      }
      Edge::Down { .. } => return,
      Edge::Up { which, .. } | Edge::Aborted { which } => {
        let i = hotkey(which).map_or(0, |i| i + 1);
        let done = if matches!(e, Edge::Up { .. }) { "Up" } else { "Aborted" };
        if let Some(i) = hotkey(which) {
          self.held &= !(1 << i);
        }

        format!("{done}: hotkey {i}")
      }
      Edge::Key { index, .. } => {
        let Some(i) = self.sent.1.get(index).copied() else { return };
        let name = self.keys[i].map(|k| k.label()).unwrap_or_default();
        let first = self.held != 0 && !self.keyed;
        if first {
          self.keyed = true;
          self.model = format!("Key {}'s model ({name})", i + 1);
        }

        format!("Key {} ({name}){}", i + 1, if first { "" } else { ": ignored" })
      }
      Edge::Escape => "Escape".into(),
    };

    if self.log.len() == ROWS {
      self.log.pop_front();
    }

    self.log.push_back(line);
  }

  fn recorders(&mut self, ui: &mut Ui, fs: &FormStyle) {
    let notes: Vec<String> = (0..3).map(|i| self.note(i)).collect();
    let mut recording = false;
    ui.form_section_sub(fs, "Hotkeys", "Each picks a model of its own.", |ui| {
      for (i, note) in notes.iter().enumerate() {
        let label = format!("Hotkey {}", i + 1);
        let row = FormRow::new(&label).caption(note);
        ui.form_row(fs, row, |ui| {
          let r = ui.id((HOTKEY, i as u8)).shortcut(&mut self.hotkeys[i]);
          recording |= r.recording;
          if r.changed {
            self.apply();
          }
        });
      }
    });
    let sub = "Taken while a hotkey is held; the first per dictation picks the model.";
    ui.form_section_sub(fs, "Keys while held", sub, |ui| {
      for i in 0..3 {
        let label = format!("Key {}", i + 1);
        ui.form_row(fs, FormRow::new(&label), |ui| {
          let r = ui.id((KEY, i as u8)).shortcut_key(&mut self.keys[i]);
          recording |= r.recording;
          if r.changed {
            self.apply();
          }
        });
      }
    });
    if recording != self.paused {
      self.paused = recording;
      hotkeys(|h| h.pause(recording));
    }
  }

  /// What the status says of hotkey recorder `i`.
  fn note(&self, i: usize) -> String {
    let at = self.sent.0.iter().position(|&r| r == i);
    let slot = at.and_then(|at| self.status.as_ref()?.slots.get(at));
    match (self.hotkeys[i], slot) {
      (None, _) => "None".into(),
      (Some(_), None) => "Not armed (no hotkeys here)".into(),
      (Some(_), Some(s)) if let Some(e) = &s.error => format!("Not armed: {e}"),
      (Some(_), Some(s)) if s.fallback => "Armed, with Carbon's fallback".into(),
      (Some(_), Some(s)) => match &s.fallback_error {
        Some(e) => format!("Armed, the event tap's only: {e}"),
        None => "Armed, the event tap's only".into(),
      },
    }
  }

  fn status(&mut self, ui: &mut Ui, fs: &FormStyle) {
    let s = self.status.clone().unwrap_or_default();
    let listener = match (&self.status, s.listener) {
      (None, _) => "No hotkeys in this harness".to_owned(),
      (_, Listener::Failed) => format!("Failed: {}", s.listener_error.as_deref().unwrap_or("")),
      (_, l) => format!("{l:?}"),
    };

    let keys = s.keys_error.clone().unwrap_or_else(|| "Taken while a hotkey is held".into());
    let secure = if s.secure_input { "On: the event tap hears nothing" } else { "Off" };
    let set = self.error.clone().unwrap_or_else(|| "None".into());
    ui.form_section(fs, "Status", |ui| {
      ui.form_row(fs, FormRow::new("Event tap").caption(&listener), |ui| {
        if ui.id(START).button("Start").clicked
          && let Some(Err(e)) = hotkeys(|h| h.start())
        {
          self.error = Some(e);
        }
      });
      ui.form_row(fs, FormRow::new("Keys"), |ui| ui.id(KEYS_STATUS).text(&keys, fs.note));
      ui.form_row(fs, FormRow::new("Secure Input"), |ui| ui.text(secure, fs.note));
      ui.form_row(fs, FormRow::new("Refused"), |ui| ui.text(&set, fs.note));
    });
  }
}

impl App for Demo {
  fn view(&mut self, ui: &mut Ui) {
    if let Some(s) = hotkeys(|h| h.status()) {
      self.status = Some(s);
    }

    let fs = FormStyle::plates(&ui.theme());
    ui.col(Style::new().grow().bg(fs.bg), |ui| {
      ui.form_scroll(&fs, Style::new().grow(), |ui| {
        ui.col(fs.page.pads([20.0, 20.0, 20.0, 20.0]).w(grow()), |ui| {
          self.recorders(ui, &fs);
          ui.form_section(&fs, "Dictation", |ui| {
            let model = self.model.clone();
            let row = FormRow::new("Model").caption("The first key per dictation wins.");
            ui.form_row(&fs, row, |ui| ui.id(MODEL).text(&model, fs.note));
            ui.form_row(&fs, FormRow::new("Edges"), |ui| {
              ui.col(Style::new().w(px(220.0)), |ui| {
                for r in 0..ROWS {
                  let line = self.log.get(r).map_or("", String::as_str);
                  ui.row(Style::new().h(px(ROW_H)).w(grow()), |ui| {
                    ui.id((LOG, r as u8)).text(line, fs.note.nowrap());
                  });
                }
              });
            });
          });
          self.status(ui, &fs);
        });
      });
    });
  }
}

pub fn config() -> Config {
  Config::new("Hotkeys", 560.0, 880.0)
}

/// An edge, on the main thread: queued, then the queue to the window in order, which redraws.
fn deliver(main: &mut Main, edge: Edge) {
  EDGES.with(|q| q.borrow_mut().0.push_back(edge));
  drain(main);
}

/// Hands the queued edges to the window in order; busy (a nested run loop holds it), the rest
/// wait for one retry a moment later, never dropped nor reordered.
fn drain(main: &mut Main) {
  let window = WINDOW.with(|w| w.try_borrow().ok().and_then(|w| w.clone()));
  let Some(w) = window.filter(|w| w.is_open()) else {
    return EDGES.with(|q| q.borrow_mut().0.clear());
  };
  while let Some(edge) = EDGES.with(|q| q.borrow().0.front().copied()) {
    if w.with(|demo| demo.edge(edge)).is_none() {
      if !EDGES.with(|q| std::mem::replace(&mut q.borrow_mut().1, true)) {
        main.proxy().after(Duration::from_millis(10), |main| {
          EDGES.with(|q| q.borrow_mut().1 = false);
          drain(main);
        });
      }

      return;
    }

    EDGES.with(|q| q.borrow_mut().0.pop_front());
  }
}

/// The status read again every second (the tap starts a moment after Start, Secure Input comes
/// and goes).
fn tick(main: &mut Main) {
  if let Some(w) = WINDOW.with(|w| w.try_borrow().ok().and_then(|w| w.clone())) {
    w.redraw();
  }

  main.proxy().after(Duration::from_secs(1), tick);
}

fn start(main: &mut Main) {
  let proxy = main.proxy();
  let hotkeys = Hotkeys::new(move |edge| {
    eprintln!("hotkeys: {}", describe(edge));
    proxy.post(move |main| deliver(main, edge));
  });
  HOTKEYS.with(|h| *h.borrow_mut() = Some(hotkeys));
  let w = main.open(&config(), Demo::default());
  w.on_close(|main| main.quit());
  WINDOW.with(|s| *s.borrow_mut() = Some(w));
  tick(main);
  main.on_quit(|_| _ = HOTKEYS.with(|h| h.borrow_mut().take()));
}

fn main() {
  if std::env::args().any(|a| a == "--shot") {
    // fn held, Z picked its model, X came after: ignored; then fn up.
    let at = Instant::now();
    let mut demo = Demo::default();
    for e in [Edge::Down { which: 0, at, repeat: false }, Edge::Key { index: 0, at }] {
      demo.edge(e);
    }

    demo.edge(Edge::Key { index: 1, at });
    demo.edge(Edge::Up { which: 0, at });
    return nimbo::run(config(), demo, |h| _ = h.frame());
  }

  nimbo::app(INFO, start);
}
