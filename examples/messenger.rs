// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A messenger with bots, the window app in breadth: a unified window with a sidebar of chats
//! and toolbars in the title bar, the sidebar sliding in and out from its toggle; search across
//! every chat; a lazy column of thousands of messages of mixed heights that stays at the bottom
//! as new ones come; a composer that grows with its text, sends on Return, takes emoji, IME and
//! files picked in the open panel or dropped on the chat; a context menu on a message; a
//! contact's profile beside the chat, which exports the chat through the save panel and clears
//! it after asking. Bots answer from a thread of their own, the chat showing "typing…" meanwhile;
//! the data is seeded, the same each run.
//!
//! Around the window: the menu bar's commands and their keys, a tray icon with the unread count
//! and a menu, a settings window in System Settings' look (open at login, a global shortcut that
//! shows the window, the language), one instance (a second launch brings the window forward),
//! English and Russian (`t!`), and names for every control to assistive technology.
//!
//! `cargo run --release --example messenger [-- settings] [--shot <file.png>]`: `settings` shots
//! the settings window.

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "common/example.rs"]
mod example;

use std::cell::{Cell, OnceCell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use nimbo::a11y::Role;
use nimbo::hotkey::{self, Edge, Hotkeys};
use nimbo::instance::{self, Claim, Instance};
use nimbo::tray::{Tray, TrayIcon};
use nimbo::{
  Align, App, AppInfo, Axes, Axis, Binding, Bitmap, BorderedProminentButtonStyle,
  BorderlessButtonStyle, ButtonRole, Color, Command, Commands, Config, ContentMode, ControlSize,
  Cx, Dialog, Event, FileDialog, Float, FormRow, FormStyle, Hotkey, Icon, Id, Item, Key, List,
  Locale, Localized, Main, Menu, NamedKey, Picker, Point, Proxy, Selection, Shadow, Shortcut,
  Split, Style, Tag, TextField, TextStyle, ToolbarItemPlacement, Transition, Ui, UnitPoint, Window,
  alpha, autostart, grow, pct, px, rgb, t, tn,
};

pub const INFO: AppInfo = AppInfo::new("com.example.nimbo-messenger", "Messenger");

pub const TOGGLE: &str = "toggle";
pub const SPLIT: &str = "split";
pub const SEARCH: &str = "search";
pub const CHATS: &str = "chats";
pub const HITS: &str = "hits";
pub const MESSAGES: &str = "messages";
pub const COLUMN: &str = "column";
pub const DROP: &str = "drop";
pub const COMPOSER: &str = "composer";
pub const FIELD: &str = "field";
/// The profile's width, and the least the chat beside it keeps; narrower, the profile goes over.
const PANEL_W: f32 = 240.0;
const CHAT_MIN: f32 = 320.0;

/// Chat `i`'s composer field.
pub fn field(i: usize) -> Id {
  Id::of(FIELD).key(i)
}
pub const ATTACH: &str = "attach";
pub const EMOJI: &str = "emoji";
pub const SEND: &str = "send";
pub const PROFILE: &str = "profile";
pub const PANEL: &str = "panel";
pub const MUTE: &str = "mute";
pub const EXPORT: &str = "export";
pub const CLEAR: &str = "clear";
pub const CONFIRM: &str = "confirm";
/// A message's bubble: `Id::of(BUBBLE).key(message id)`.
pub const BUBBLE: &str = "bubble";

/// The sidebar's slide, 150 ms from in to out.
const SLIDE: Transition = Transition::smooth(1000.0 / 150.0);
/// The widest a bubble gets, and its padding.
const BUBBLE_W: f32 = 380.0;
const PAD: f32 = 8.0;
/// A file's chip and an image's thumbnail in a bubble.
const CHIP_H: f32 = 26.0;
const THUMB: (f32, f32) = (200.0, 130.0);
/// How many messages the archive chat holds.
pub const ARCHIVE: usize = 3000;

pub const EMOJIS: [&str; 8] = ["👍", "😀", "🎉", "❤️", "🤔", "🙏", "🚀", "☕"];
pub const PAPERCLIP: Icon =
  Icon::new("M20 11l-8 8a5 5 0 0 1-7-7l8-8a3.5 3.5 0 0 1 5 5l-8 8a2 2 0 0 1-3-3l7-7");
pub const ARROW_UP: Icon = Icon::new("M12 19V5M6 11l6-6 6 6");
pub const SMILE: Icon = Icon::new(
  "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18ZM8.5 14.5a4.5 4.5 0 0 0 7 0M9 9.5h.01M15 9.5h.01",
);
/// The tray's icon: a speech bubble.
pub const BUBBLE_ICON: &str =
  "M3 4h12a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H8l-4 3v-3H3a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1Z";

/// The Russian catalog (`--locale ru`).
pub const RU: &str = r#"
msgid ""
msgstr "Plural-Forms: nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);\n"

msgid "Search"
msgstr "Поиск"

msgid "Message"
msgstr "Сообщение"

msgid "typing…"
msgstr "печатает…"

msgid "Chats"
msgstr "Чаты"

msgid "Found"
msgstr "Найдено"

msgid "bot"
msgstr "бот"

msgid "You"
msgstr "Вы"

msgid "Attach Files"
msgstr "Прикрепить файлы"

msgid "Send"
msgstr "Отправить"

msgid "Emoji"
msgstr "Эмодзи"

msgid "Profile"
msgstr "Профиль"

msgid "Hide Sidebar"
msgstr "Скрыть боковую панель"

msgid "Show Sidebar"
msgstr "Показать боковую панель"

msgid "Copy"
msgstr "Скопировать"

msgid "Delete"
msgstr "Удалить"

msgid "Mute"
msgstr "Без звука"

msgid "Export Chat…"
msgstr "Экспорт чата…"

msgid "Clear History…"
msgstr "Очистить историю…"

msgid "Clear the history?"
msgstr "Очистить историю?"

msgid "Every message in this chat goes, for good."
msgstr "Все сообщения этого чата удалятся насовсем."

msgid "Clear"
msgstr "Очистить"

msgid "Drop files to send them"
msgstr "Отпустите файлы, чтобы отправить"

msgid "No messages found"
msgstr "Сообщений не найдено"

msgid "Repeats what you say, backwards."
msgstr "Повторяет сказанное задом наперёд."

msgid "Tells your fortune, one line at a time."
msgstr "Предсказывает судьбу, по строчке за раз."

msgid "Adds, subtracts, multiplies and divides."
msgstr "Складывает, вычитает, умножает и делит."

msgid "Keeps everything, thousands of messages."
msgstr "Хранит всё, тысячи сообщений."

msgid "{n} message"
msgid_plural "{n} messages"
msgstr[0] "{n} сообщение"
msgstr[1] "{n} сообщения"
msgstr[2] "{n} сообщений"

msgid "{n} unread"
msgid_plural "{n} unread"
msgstr[0] "{n} непрочитанное"
msgstr[1] "{n} непрочитанных"
msgstr[2] "{n} непрочитанных"

msgid "Open Messenger"
msgstr "Открыть Messenger"

msgid "Settings…"
msgstr "Настройки…"

msgid "Quit"
msgstr "Выйти"

msgid "Chat"
msgstr "Чат"

msgid "Find"
msgstr "Найти"

msgid "Next Chat"
msgstr "Следующий чат"

msgid "Previous Chat"
msgstr "Предыдущий чат"

msgid "General"
msgstr "Основные"

msgid "About"
msgstr "О программе"

msgid "Startup"
msgstr "Запуск"

msgid "Open at login"
msgstr "Открывать при входе"

msgid "Language"
msgstr "Язык"

msgid "System Default"
msgstr "Как в системе"

msgid "Applies at once to every window."
msgstr "Сразу для всех окон."

msgid "Shortcut"
msgstr "Сочетание клавиш"

msgid "Show Messenger"
msgstr "Показать Messenger"

msgid "Works in every app, while this one runs."
msgstr "Работает в любом приложении, пока это запущено."

msgid "Back to the default shortcut"
msgstr "Вернуть сочетание по умолчанию"

msgid "Reset"
msgstr "Сбросить"

msgid "Chats with bots, made with nimbo."
msgstr "Чаты с ботами, сделано на nimbo."
"#;

// MARK: Data

/// A small seeded generator (xorshift): the same history each run.
pub struct Seed(u64);

impl Seed {
  pub fn step(&mut self) -> u64 {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    self.0
  }

  pub fn below(&mut self, n: usize) -> usize {
    (self.step() % n as u64) as usize
  }
}

const WORDS: [&str; 32] = [
  "the", "queue", "build", "window", "frame", "lazy", "column", "bot", "answers", "slowly",
  "today", "tomorrow", "renders", "text", "with", "a", "seed", "kept", "every", "message",
  "scrolls", "down", "fast", "and", "still", "small", "native", "menu", "tray", "file", "drop",
  "light",
];

/// Seeded prose: 1 to 4 sentences of 3 to 13 words.
pub fn prose(seed: &mut Seed) -> String {
  let sentences = 1 + seed.below(4);
  let mut out = String::new();
  for s in 0..sentences {
    let n = 3 + seed.below(11);
    let words: Vec<&str> = (0..n).map(|_| WORDS[seed.below(WORDS.len())]).collect();
    let line = words.join(" ");
    let mut c = line.chars();
    let first = c.next().map(|f| f.to_uppercase().collect::<String>()).unwrap_or_default();
    out += &format!("{}{first}{}.", if s == 0 { "" } else { " " }, c.as_str());
  }

  out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bot {
  Echo,
  Oracle,
  Abacus,
  Archive,
}

/// What you said to Echo before.
const ECHOES: [&str; 5] =
  ["hello", "racecar", "Was it a car or a cat I saw?", "stressed", "drawer"];

const FORTUNES: [&str; 6] = [
  "A frame you never draw is the fastest one.",
  "Your list will scroll smoothly tonight.",
  "Someone will ask for a dark theme. Say yes.",
  "The bug is in the code you trust most.",
  "Patience: the reply is on its way.",
  "A small binary brings great joy.",
];

/// A bot's answer to `text`, the `n`th it gives.
pub fn answer(bot: Bot, text: &str, n: usize) -> String {
  match bot {
    Bot::Echo => text.chars().rev().collect(),
    Bot::Oracle => FORTUNES[(text.len() + n) % FORTUNES.len()].into(),
    Bot::Abacus => sum(text).map_or("Send me a sum, like 2 + 2.".into(), |v| format!("= {v}")),
    Bot::Archive => format!("Kept. That makes {} messages here.", n + 1),
  }
}

/// `2 + 3 * 4`, left to right; `None` unless numbers and operators.
pub fn sum(text: &str) -> Option<f64> {
  let ops = ["+", "-", "*", "/"];
  let spaced = ops.iter().fold(text.to_owned(), |s, op| s.replace(op, &format!(" {op} ")));
  let mut it = spaced.split_whitespace();
  let mut v: f64 = it.next()?.parse().ok()?;
  while let Some(op) = it.next() {
    let x: f64 = it.next()?.parse().ok()?;
    v = match op {
      "+" => v + x,
      "-" => v - x,
      "*" => v * x,
      "/" => v / x,
      _ => return None,
    };
  }

  Some(v)
}

/// A file sent with a message: its name, and its picture when it is one.
#[derive(Clone)]
pub struct Attachment {
  pub path: PathBuf,
  pub image: Option<Bitmap>,
}

impl Attachment {
  /// The file at `path`, decoded when it is a picture.
  pub fn new(path: PathBuf) -> Self {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let picture = matches!(ext.as_str(), "png" | "jpg" | "jpeg");
    let image =
      picture.then(|| std::fs::read(&path).ok().and_then(|b| Bitmap::decode(&b, 400).ok()));
    Self { path, image: image.flatten() }
  }

  pub fn name(&self) -> String {
    self.path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned())
  }
}

#[derive(Clone)]
pub struct Msg {
  pub id: u64,
  pub mine: bool,
  pub text: String,
  pub files: Vec<Attachment>,
  /// Minutes since the history began, at 9:00.
  pub at: u32,
}

pub struct Chat {
  pub name: &'static str,
  pub about: Localized,
  pub bot: Bot,
  pub tint: Color,
  pub msgs: Vec<Msg>,
  pub unread: usize,
  /// Answers asked for and not come yet: "typing…".
  pub pending: usize,
  pub muted: bool,
  pub draft: String,
  pub files: Vec<Attachment>,
  /// The messages last frame: one sent brings the chat to its bottom.
  seen: usize,
}

impl Chat {
  fn new(name: &'static str, about: Localized, bot: Bot, tint: Color) -> Self {
    let (msgs, draft, files) = (Vec::new(), String::new(), Vec::new());
    Self {
      name,
      about,
      bot,
      tint,
      msgs,
      unread: 0,
      pending: 0,
      muted: false,
      draft,
      files,
      seen: 0,
    }
  }

  fn push(&mut self, next: &mut u64, mine: bool, text: String, files: Vec<Attachment>) {
    let at = self.msgs.last().map_or(0, |m| m.at + 1);
    *next += 1;
    self.msgs.push(Msg { id: *next, mine, text, files, at });
  }

  /// The last message's text in a line, for the sidebar.
  fn snippet(&self) -> String {
    let m = self.msgs.last();
    let text = m.map_or(String::new(), |m| match m.text.is_empty() {
      true => m.files.first().map_or(String::new(), Attachment::name),
      false => m.text.lines().next().unwrap_or("").to_owned(),
    });
    match m.is_some_and(|m| m.mine) {
      true => format!("{}: {text}", t!("You")),
      false => text,
    }
  }
}

/// `9:00` and on, for a message's minute.
pub fn clock(at: u32) -> String {
  let m = 9 * 60 + at;
  format!("{}:{:02}", m / 60 % 24, m % 60)
}

/// The seeded chats: a few short ones and the archive's thousands.
pub fn chats() -> (Vec<Chat>, u64) {
  let mut next = 0;
  let mut seed = Seed(0x9e37_79b9_7f4a_7c15);
  let mut list = vec![
    Chat::new("Echo", t!("Repeats what you say, backwards."), Bot::Echo, rgb(0x3b82f6)),
    Chat::new("Oracle", t!("Tells your fortune, one line at a time."), Bot::Oracle, rgb(0xa855f7)),
    Chat::new("Abacus", t!("Adds, subtracts, multiplies and divides."), Bot::Abacus, rgb(0x30a46c)),
    Chat::new(
      "Archive",
      t!("Keeps everything, thousands of messages."),
      Bot::Archive,
      rgb(0xf5a524),
    ),
  ];
  for (i, c) in list.iter_mut().enumerate() {
    let n = if c.bot == Bot::Archive { ARCHIVE } else { 4 + 2 * i };
    // You ask, the bot answers you; the archive keeps both.
    for k in 0..n {
      let mine = if c.bot == Bot::Archive { seed.below(3) == 0 } else { k % 2 == 0 };
      let text = match (c.bot, mine) {
        (Bot::Archive, _) => prose(&mut seed),
        (Bot::Abacus, true) => {
          format!("{} * {} + {}", 2 + seed.below(20), seed.below(10), seed.below(100))
        }
        (Bot::Echo, true) => ECHOES[seed.below(ECHOES.len())].into(),
        (_, true) => prose(&mut seed),
        (bot, false) => answer(bot, &c.msgs.last().map_or(String::new(), |m| m.text.clone()), k),
      };

      c.push(&mut next, mine, text, Vec::new());
    }
  }

  list[1].unread = 2;
  (list, next)
}

// MARK: Bots

struct Ask {
  chat: usize,
  bot: Bot,
  text: String,
  n: usize,
}

pub struct Reply {
  pub chat: usize,
  pub text: String,
}

/// The bots' thread: it answers each question after `delay`, in order, and wakes the window.
struct Bots {
  asks: Sender<Ask>,
  replies: Receiver<Reply>,
}

impl Bots {
  /// Each answer calls `wake`: the app's main loop takes it in ([answered]).
  fn start(delay: Duration, wake: impl Fn() + Send + 'static) -> Self {
    let (asks, questions) = channel::<Ask>();
    let (answers, replies) = channel();
    let run = move || {
      for a in questions {
        std::thread::sleep(delay);
        let text = answer(a.bot, &a.text, a.n);
        if answers.send(Reply { chat: a.chat, text }).is_err() {
          break;
        }

        wake();
      }
    };
    std::thread::Builder::new().name("bots".into()).spawn(run).expect("the bots' thread");
    Self { asks, replies }
  }
}

// MARK: The window

/// The window's keys, which the menu bar shows on macOS: they come as commands.
pub const KEYS: &[Binding] = &[
  Binding::new(Shortcut::char('f').cmd(), Command::Action("find")),
  Binding::new(Shortcut::char(']').cmd(), Command::Action("next")),
  Binding::new(Shortcut::char('[').cmd(), Command::Action("previous")),
  Binding::new(Shortcut::char('i').cmd(), Command::Action("profile")),
  Binding::new(Shortcut::char('s').cmd().ctrl(), Command::Action("sidebar")),
];

/// Return sends; Shift-Return breaks the line, as the field does.
const SEND_KEYS: &[Binding] =
  &[Binding::new(Shortcut::new(Key::Named(NamedKey::Enter)), Command::Action("send"))];

/// What a message's menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsgAction {
  Copy,
  Delete,
}

/// What the last panel was asked for, and for which chat: it may answer after another opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asked {
  Attach(usize),
  Export(usize),
}

pub struct Messenger {
  pub chats: Vec<Chat>,
  pub open: usize,
  pub sel: Selection,
  pub query: String,
  pub hit: Selection,
  pub sidebar: bool,
  pub side: f32,
  pub profile: bool,
  /// Files dragged over the chat.
  pub dragging: bool,
  pub confirm: bool,
  /// How long a bot takes to answer.
  pub delay: Duration,
  /// The window's proxy to the main thread: the tray's count follows the unread.
  pub proxy: Option<Proxy>,
  next: u64,
  bots: Option<Bots>,
  asked: Option<Asked>,
  /// A message to scroll to, found by the search.
  reveal: Option<usize>,
  /// The unread count the tray shows.
  told: usize,
  /// The search field takes the focus on the next build.
  find: bool,
  /// The window has the keys: what it shows of the open chat is read.
  pub focused: bool,
}

impl Default for Messenger {
  fn default() -> Self {
    let (chats, next) = chats();
    let mut sel = Selection::default();
    sel.select(0, 0);
    Self {
      chats,
      open: 0,
      sel,
      query: String::new(),
      hit: Selection::default(),
      sidebar: true,
      side: 0.32,
      profile: false,
      dragging: false,
      confirm: false,
      delay: Duration::from_millis(900),
      proxy: None,
      next,
      bots: None,
      asked: None,
      reveal: None,
      told: 0,
      find: false,
      focused: true,
    }
  }
}

/// A window over the messenger, which the app keeps for its whole life: the chats, drafts and
/// bots of a closed window are there for the next, which opens at the bottom of each chat.
pub struct Shown(pub Rc<RefCell<Messenger>>);

impl Shown {
  pub fn new(m: Rc<RefCell<Messenger>>) -> Self {
    Self(m)
  }
}

impl App for Shown {
  fn keymap(&self) -> &[Binding] {
    KEYS
  }

  fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
    self.0.try_borrow_mut().is_ok_and(|mut m| m.event(cx, ev))
  }

  fn view(&mut self, ui: &mut Ui) {
    if let Ok(mut m) = self.0.try_borrow_mut() {
      m.build(ui);
    }
  }
}

impl Messenger {
  pub fn unread(&self) -> usize {
    self.chats.iter().map(|c| c.unread).sum()
  }

  /// Answers asked for and not come yet, in every chat.
  pub fn pending(&self) -> usize {
    self.chats.iter().map(|c| c.pending).sum()
  }

  /// Opens chat `i`: its unread are read.
  pub fn open_chat(&mut self, i: usize) {
    self.open = i.min(self.chats.len() - 1);
    self.sel.select(self.open as u64, self.open);
    self.chats[self.open].unread = 0;
  }

  /// A command from the window's keys or the menu bar.
  pub fn run(&mut self, name: &str) {
    // The menu bar's items reach the window past its dialog: while it asks, nothing moves.
    if self.confirm {
      return;
    }

    let n = self.chats.len();
    match name {
      "next" => self.open_chat((self.open + 1) % n),
      "previous" => self.open_chat((self.open + n - 1) % n),
      "profile" => self.profile ^= true,
      "sidebar" => self.sidebar ^= true,
      "find" => (self.sidebar, self.find) = (true, true),
      _ => {}
    }
  }

  /// Sends the composer's text and files, and asks the bot.
  pub fn send(&mut self) {
    let c = &mut self.chats[self.open];
    let text = c.draft.trim().to_owned();
    if text.is_empty() && c.files.is_empty() {
      return;
    }

    let files = std::mem::take(&mut c.files);
    c.draft.clear();
    c.push(&mut self.next, true, text.clone(), files);
    c.pending += 1;
    let n = c.msgs.len();
    if let Some(b) = &self.bots {
      _ = b.asks.send(Ask { chat: self.open, bot: c.bot, text, n });
    }
  }

  /// Files picked or dropped: the composer takes them.
  pub fn attach(&mut self, chat: usize, paths: &[PathBuf]) {
    let files = &mut self.chats[chat].files;
    files.extend(paths.iter().cloned().map(Attachment::new));
  }

  /// The chat as text, for the save panel's file.
  pub fn transcript(&self, i: usize) -> String {
    let c = &self.chats[i];
    let who = |m: &Msg| if m.mine { t!("You").to_string() } else { c.name.to_owned() };
    let lines = c.msgs.iter().map(|m| format!("[{}] {}: {}\n", clock(m.at), who(m), m.text));
    lines.collect()
  }

  /// Messages matching the search: (chat, message), newest first, up to 200.
  pub fn hits(&self) -> Vec<(usize, usize)> {
    let q = self.query.trim().to_lowercase();
    if q.is_empty() {
      return Vec::new();
    }

    let all = self.chats.iter().enumerate().flat_map(|(c, chat)| {
      let found =
        chat.msgs.iter().enumerate().rev().filter(|(_, m)| m.text.to_lowercase().contains(&q));
      found.map(move |(i, _)| (c, i))
    });
    all.take(200).collect()
  }

  /// Starts the bots with the first build. In the app an answer goes to the main loop
  /// ([answered]), drawn or not; with no app around it (the harness) it wakes this window, whose
  /// build takes it in. Either way a focused window reads the open chat.
  fn receive(&mut self, ui: &mut Ui) {
    if self.bots.is_none() {
      let waker = ui.waker().clone();
      self.bots = Some(match self.proxy.clone() {
        Some(p) => Bots::start(self.delay, move || p.post(answered)),
        None => Bots::start(self.delay, move || waker.wake()),
      });
    }

    self.take_answers(ui.window_focused());
  }

  /// Takes the bots' answers in (`shown`: the window has focus, so the open chat is read): a
  /// chat not shown counts them unread unless muted.
  pub fn take_answers(&mut self, shown: bool) {
    let replies: Vec<Reply> =
      self.bots.as_ref().map_or(Vec::new(), |b| b.replies.try_iter().collect());
    for r in replies {
      let c = &mut self.chats[r.chat];
      c.pending = c.pending.saturating_sub(1);
      c.push(&mut self.next, false, r.text, Vec::new());
      if r.chat != self.open || !shown {
        c.unread += !c.muted as usize;
      }
    }

    // What the focused window shows is read, also what came while it was in the background.
    if shown {
      self.chats[self.open].unread = 0;
    }

    // The tray's count, when it changed.
    let unread = self.unread();
    if unread != self.told {
      self.told = unread;
      if let Some(p) = &self.proxy {
        p.post(move |_| tray_count(unread));
      }
    }
  }
}

// MARK: Sidebar

/// A round avatar with the name's first letter.
fn avatar(ui: &mut Ui, name: &str, tint: Color, d: f32) {
  let s = Style::new().w(px(d)).h(px(d)).radius(d / 2.0).bg(tint).center();
  let letter = name.chars().next().unwrap_or('?').to_string();
  let t = ui.theme();
  ui.row(s, |ui| ui.text(&letter, TextStyle::new(t.bold().size(d * 0.42), [1.0; 4])));
}

impl Messenger {
  /// The sidebar column's toolbar: the toggle at its trailing edge, by the divider, as in Notes
  /// on macOS 26; hidden, right of the traffic lights.
  fn toggle(&mut self, ui: &mut Ui) {
    ui.toolbar(|ui| {
      ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
        let tip = if self.sidebar { t!("Hide Sidebar") } else { t!("Show Sidebar") };
        if ui.help(&tip).id(TOGGLE).icon_button(Icon::SIDEBAR).clicked {
          self.sidebar ^= true;
        }
      });
    });
  }

  fn sidebar(&mut self, ui: &mut Ui, bar: f32) {
    let t = ui.theme();
    ui.col(Style::new().grow().bg(t.panel()), |ui| {
      self.toggle(ui);
      ui.el(Style::new().h(px(bar)));
      ui.col(Style::new().w(grow()).pads([0.0, 10.0, 8.0, 10.0]), |ui| {
        // System Settings' sidebar field, the large one.
        ui.control_size(ControlSize::Large).id(SEARCH).search_field(&mut self.query, &t!("Search"))
      });
      match self.query.trim().is_empty() {
        true => self.chat_list(ui),
        false => self.hit_list(ui),
      }
    });
  }

  fn chat_list(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    let chats = &self.chats;
    let list = List::new(chats.len(), 56.0).selection(&mut self.sel);
    let r = list.show(ui.a11y_label(&t!("Chats")).id(CHATS), |ui, row| {
      let c = &chats[row.index];
      // Read as "Oracle, A frame you never…, 2 unread", not letter by letter.
      let what = if c.pending > 0 { t!("typing…").as_str().to_owned() } else { c.snippet() };
      let mut name = format!("{}, {what}", c.name);
      if c.unread > 0 {
        name += &format!(", {}", tn!("{n} unread", "{n} unread", c.unread));
      }

      if let Some(a) = ui.a11y_of(row.id) {
        a.name(&name);
      }

      ui.row(Style::new().w(grow()).gap(10.0).align(Align::Start, Align::Center), |ui| {
        avatar(ui, c.name, c.tint, 36.0);
        ui.col(Style::new().w(grow()).gap(2.0), |ui| {
          ui.row(Style::new().w(grow()), |ui| {
            ui.text(c.name, TextStyle::new(t.bold(), t.text()).line_limit(1));
            ui.spacer();
            ui.text(c.msgs.last().map_or(String::new(), |m| clock(m.at)), t.muted().nowrap());
          });
          ui.row(Style::new().w(grow()).gap(6.0), |ui| {
            if c.pending > 0 {
              ui.text(t!("typing…"), TextStyle::new(t.font(), t.accent()).line_limit(1));
            } else {
              ui.row(Style::new().w(grow()).clip(), |ui| {
                ui.text(c.snippet(), t.muted().line_limit(1))
              });
            }

            if c.unread > 0 {
              ui.spacer();
              ui.badge(c.unread, nimbo::Tone::Accent);
            }
          });
        });
      });
    });
    if r.changed
      && let Some(i) = self.sel.cursor_index(self.chats.len(), &|i| i as u64)
    {
      self.open_chat(i);
    }
  }

  fn hit_list(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    let hits = self.hits();
    if hits.is_empty() {
      ui.col(Style::new().grow().pad(16.0), |ui| ui.text(t!("No messages found"), t.muted()));
      return;
    }

    let chats = &self.chats;
    let list = List::new(hits.len(), 48.0).selection(&mut self.hit);
    let r = list.show(ui.a11y_label(&t!("Found")).id(HITS), |ui, row| {
      let (c, i) = hits[row.index];
      ui.col(Style::new().w(grow()).gap(2.0), |ui| {
        ui.text(
          format_args!("{} · {}", chats[c].name, clock(chats[c].msgs[i].at)),
          t.muted().nowrap(),
        );
        ui.row(Style::new().w(grow()).clip(), |ui| {
          ui.text(&chats[c].msgs[i].text, t.label().line_limit(1))
        });
      });
    });
    if r.changed
      && let Some(k) = self.hit.cursor_index(hits.len(), &|i| i as u64)
    {
      let (c, i) = hits[k];
      self.open_chat(c);
      self.reveal = Some(i);
    }
  }
}

// MARK: Chat

impl Messenger {
  /// The open chat's messages, lazily, opening at the bottom and kept there as new ones come
  /// while it is there; a bubble's menu copies or deletes it.
  fn messages(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    let (area, column) = (Id::of(MESSAGES).key(self.open), Id::of(COLUMN).key(self.open));
    let w = ui.rect(area).map_or(BUBBLE_W, |r| r.w - 32.0);
    let max = (w * 0.75).min(BUBBLE_W);
    let c = &mut self.chats[self.open];
    // A message sent brings it back to the bottom.
    if c.msgs.len() > c.seen && c.msgs.last().is_some_and(|m| m.mine) {
      ui.scroll_to(area, Point::new(0.0, f32::MAX));
    }

    c.seen = c.msgs.len();
    if let Some(i) = self.reveal.take() {
      ui.reveal_item(column, i);
    }

    let mut act = None;
    let msgs = &c.msgs;
    let scroll = Style::new().grow().pad_xy(16.0, 8.0).scroll(Axes::VERTICAL);
    ui.with_id(area).default_scroll_anchor(UnitPoint::BOTTOM).scroll_area(scroll, |ui| {
      ui.with_id(column).lazy_col(
        Style::new().w(grow()),
        msgs.len(),
        |i| msgs[i].id,
        |ui, i| {
          let m = &msgs[i];
          let id = Id::of(BUBBLE).key(m.id);
          let side = if m.mine { Align::End } else { Align::Start };
          let (bg, ink) = if m.mine { (t.accent(), [1.0; 4]) } else { (t.raised(), t.text()) };

          // 6 points under each message.
          let row = Style::new().w(grow()).pads([0.0, 0.0, 6.0, 0.0]);
          ui.row(row.align(side, Align::Start), |ui| {
            let bubble = Style::new().max_w(max).pad(PAD).gap(4.0).radius(12.0).bg(bg);
            let who = if m.mine { t!("You").to_string() } else { c.name.to_owned() };
            ui.a11y(id, Role::Group).name(&format!("{who}, {}", clock(m.at)));

            ui.with_id(id).col(bubble, |ui| {
              for f in &m.files {
                match &f.image {
                  Some(b) => {
                    _ = ui
                      .aspect_ratio(ContentMode::Fill)
                      .image(b, Style::new().w(px(THUMB.0)).h(px(THUMB.1)).radius(8.0))
                  }
                  None => {
                    _ = ui.row(
                      Style::new().h(px(CHIP_H)).gap(6.0).align(Align::Start, Align::Center),
                      |ui| {
                        ui.icon(Icon::DOC, 16.0, ink);
                        ui.text(f.name(), TextStyle::new(t.font(), ink).line_limit(1));
                      },
                    )
                  }
                }
              }

              if !m.text.is_empty() {
                ui.text(&m.text, t.body().color(ink));
              }
            });
          });

          let menu = || {
            let copy = Item::button(MsgAction::Copy, &t!("Copy"));
            Menu::new(vec![
              copy,
              Item::button(MsgAction::Delete, &t!("Delete")).role(ButtonRole::Destructive),
            ])
          };
          if let Some(a) = ui.context_menu(id, menu) {
            act = Some((i, a));
          }
        },
      );
    });

    match act {
      Some((i, MsgAction::Copy)) => {
        let text = self.chats[self.open].msgs[i].text.clone();
        ui.clipboard().write(&text);
      }
      Some((i, MsgAction::Delete)) => {
        let c = &mut self.chats[self.open];
        c.msgs.remove(i);
        c.seen = c.msgs.len();
      }
      None => {}
    }
  }

  /// The composer: the files to send, then the attach and emoji buttons, the field that grows
  /// up to six lines and the send button.
  fn composer(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    // Each chat's field keeps its own edits and undo; the focus follows to the chat opened.
    let (field, n) = (field(self.open), self.chats.len());
    if ui.focus_id().is_some_and(|f| f != field && (0..n).any(|k| f == self::field(k))) {
      ui.focus(Some(field));
    }

    let c = &mut self.chats[self.open];
    let w = ui.rect(field).map_or(300.0, |r| r.w);
    let style = t.body();
    let line = ui.line_height(style);
    let text_h =
      ui.measure_wrapped(if c.draft.is_empty() { " " } else { &c.draft }, style, w - 16.0).h;
    let h = (text_h + 10.0).clamp(t.sizes().control_h(), 6.0 * line + 10.0);

    let mut send = false;
    let bar = Style::new().w(grow()).pads([6.0, 12.0, 12.0, 12.0]).gap(6.0);

    ui.keymap(SEND_KEYS).id(COMPOSER).col(bar, |ui| {
      if !c.files.is_empty() {
        let mut gone = None;
        ui.row(Style::new().w(grow()).gap(6.0), |ui| {
          for (i, f) in c.files.iter().enumerate() {
            let chip = Style::new()
              .h(px(CHIP_H))
              .pads([0.0, 4.0, 0.0, 8.0])
              .gap(4.0)
              .radius(6.0)
              .bg(t.raised());
            ui.key(i).row(chip.align(Align::Start, Align::Center), |ui| {
              ui.text(f.name(), t.label().line_limit(1));
              if ui.a11y_label(&t!("Delete")).key("x").icon_button(Icon::X).clicked {
                gone = Some(i);
              }
            });
          }
        });
        if let Some(i) = gone {
          c.files.remove(i);
        }
      }

      ui.row(Style::new().w(grow()).gap(6.0).align(Align::Start, Align::End), |ui| {
        if ui
          .a11y_label(&t!("Attach Files"))
          .help(&t!("Attach Files"))
          .id(ATTACH)
          .icon_button(PAPERCLIP)
          .clicked
        {
          self.asked = Some(Asked::Attach(self.open));
          ui.file_dialog(FileDialog::open(&t!("Attach Files")));
        }

        ui.button_style(&BorderlessButtonStyle).a11y_label(&t!("Emoji")).id(EMOJI).menu(
          "🙂",
          |ui| {
            for e in EMOJIS {
              if ui.key(e).menu_item(e).clicked {
                c.draft.push_str(e);
              }
            }
          },
        );
        ui.col(Style::new().w(grow()).h(px(h)), |ui| {
          TextField::new(&mut c.draft, &t!("Message")).axis(Axis::Vertical).show(ui.with_id(field))
        });
        let empty = c.draft.trim().is_empty() && c.files.is_empty();
        send |= ui
          .disabled(empty)
          .a11y_label(&t!("Send"))
          .help(&t!("Send"))
          .id(SEND)
          .button_style(&BorderedProminentButtonStyle)
          .button(ARROW_UP)
          .clicked;
      });
    });
    send |= ui.command(Id::of(COMPOSER)) == Some(Command::Action("send"));
    if send {
      self.send();
    }
  }

  /// The contact's card beside the chat: who it is, mute, export and clear.
  /// Beside the chat, or over its trailing edge (`over`) where the window is too narrow for both.
  fn panel(&mut self, ui: &mut Ui, over: bool) {
    let t = ui.theme();
    let c = &mut self.chats[self.open];
    let card =
      Style::new().w(px(PANEL_W)).h(grow()).pad(16.0).gap(10.0).align(Align::Center, Align::Start);
    if over {
      let at = Float::parent().attach(UnitPoint::TOP_TRAILING).anchor(UnitPoint::TOP_TRAILING);
      ui.float(at.z(5));
    }

    let card =
      if over { card.h(pct(100.0)).shadow(Shadow::new(0.0, 12.0, t.shadow())) } else { card };
    ui.id(PANEL).col(card.bg(t.panel()), |ui| {
      avatar(ui, c.name, c.tint, 72.0);
      ui.text(c.name, TextStyle::new(t.bold().size(17.0), t.text()));
      ui.text(c.about, t.body().color(t.secondary()));
      ui.text(tn!("{n} message", "{n} messages", c.msgs.len()), t.muted());

      ui.row(Style::new().w(grow()).align(Align::Start, Align::Center), |ui| {
        ui.text(t!("Mute"), t.label());
        ui.spacer();
        ui.id(MUTE).toggle(&mut c.muted, ())
      });
      if ui.id(EXPORT).button(t!("Export Chat…")).clicked {
        self.asked = Some(Asked::Export(self.open));
        ui.file_dialog(FileDialog::save(&t!("Export Chat…")).name(&format!("{}.txt", c.name)));
      }

      if ui.disabled(c.msgs.is_empty()).id(CLEAR).button(t!("Clear History…")).clicked {
        self.confirm = true;
      }
    });

    let d = Dialog::new(t!("Clear the history?").as_str());
    let d = d.message(t!("Every message in this chat goes, for good.").as_str());
    let d = d.button(t!("Clear").as_str(), ButtonRole::Destructive);
    if d.show(ui.id(CONFIRM), &mut self.confirm) == Some(nimbo::Answer::Action) {
      let c = &mut self.chats[self.open];
      (c.msgs, c.seen) = (Vec::new(), 0);
    }
  }

  /// The chat column under its toolbar: the messages, the drop zone over them, the composer.
  fn chat(&mut self, ui: &mut Ui, bar: f32) {
    let t = ui.theme();
    let c = &self.chats[self.open];
    let (name, typing) = (c.name, c.pending > 0);
    ui.col(Style::new().grow().pads([bar, 0.0, 0.0, 0.0]), |ui| {
      ui.toolbar(|ui| {
        ui.toolbar_item(ToolbarItemPlacement::Principal, |ui| {
          ui.col(Style::new().align(Align::Center, Align::Center), |ui| {
            ui.text(name, TextStyle::new(t.bold(), t.text()));
            match typing {
              true => ui.text(t!("typing…"), TextStyle::new(t.font().size(11.0), t.accent())),
              false => ui.text(t!("bot"), TextStyle::new(t.font().size(11.0), t.secondary())),
            }
          });
        });
        ui.toolbar_item(ToolbarItemPlacement::PrimaryAction, |ui| {
          if ui
            .a11y_label(&t!("Profile"))
            .help(&t!("Profile"))
            .id(PROFILE)
            .icon_button(Icon::INFO)
            .clicked
          {
            self.profile ^= true;
          }
        });
      });

      let row = Id::of(PANEL).key("row");
      let narrow = ui.rect(row).is_some_and(|r| r.w < PANEL_W + 1.0 + CHAT_MIN);
      ui.with_id(row).row(Style::new().grow(), |ui| {
        ui.col(Style::new().grow(), |ui| {
          let lit = ui.hot(Id::of(DROP), self.dragging);
          let zone = Style::new()
            .grow()
            .border(2.0, alpha(t.accent(), lit))
            .bg(alpha(t.accent(), 0.08 * lit));
          ui.id(DROP).col(zone, |ui| self.messages(ui));
          if self.dragging {
            ui.text(t!("Drop files to send them"), TextStyle::new(t.font(), t.accent()));
          }

          ui.el(Style::new().w(grow()).h(px(1.0)).bg(t.line()));
          self.composer(ui);
        });
        if self.profile && !narrow {
          ui.el(Style::new().w(px(1.0)).h(grow()).bg(t.line()));
        }

        if self.profile {
          self.panel(ui, narrow);
        }
      });
    });
  }
}

impl App for Messenger {
  fn keymap(&self) -> &[Binding] {
    KEYS
  }

  fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
    let zone = cx.hovered(Id::of(DROP));
    match ev {
      Event::DragOver { .. } => self.dragging = zone,
      Event::DragExit => self.dragging = false,
      Event::Drop { paths, .. } => {
        if zone {
          self.attach(self.open, paths);
        }

        self.dragging = false;
      }
      Event::FileDialog(path) => match (self.asked.take(), path) {
        (Some(Asked::Attach(i)), Some(p)) => self.attach(i, std::slice::from_ref(p)),
        (Some(Asked::Export(i)), Some(p)) => {
          if let Err(e) = std::fs::write(p, self.transcript(i)) {
            eprintln!("export: {e}");
          }
        }
        _ => {}
      },
      Event::Command(Command::Action(name)) => self.run(name),
      Event::Focus(on) => {
        self.focused = *on;
        return false;
      }
      _ => return false,
    }

    true
  }

  fn view(&mut self, ui: &mut Ui) {
    self.build(ui);
  }
}

impl Messenger {
  /// The window's view, also for [Shown]'s.
  pub fn build(&mut self, ui: &mut Ui) {
    ui.follow_system();
    self.receive(ui);
    if std::mem::take(&mut self.find) {
      ui.focus(Some(Id::of(SEARCH)));
    }

    let (t, bar) = (ui.theme(), ui.toolbar_height());
    // How far the sidebar is out: it slides in and out in 150 ms.
    let out = ui.animate(Id::of(SPLIT).key("out"), self.sidebar as u8 as f32, SLIDE);
    ui.col(Style::new().grow().bg(t.bg()), |ui| {
      // Hidden, the sidebar's column is no wider than nothing, its toolbar there all the same.
      if out == 0.0 {
        return ui.row(Style::new().grow(), |ui| {
          ui.col(Style::new().w(px(0.0)).h(grow()), |ui| self.toggle(ui));
          self.chat(ui, bar);
        });
      }

      // Sliding, the sidebar takes that part of its share, the share kept for when it is out.
      let mut side = std::mem::take(&mut self.side);
      let mut sliding = side * out;
      let ratio = if out < 1.0 { &mut sliding } else { &mut side };
      let split = Split::navigation().ratio(ratio).min(220.0 * out, 360.0);
      let r = split.show(ui.id(SPLIT), |ui, pane| match pane {
        0 => self.sidebar(ui, bar),
        _ => self.chat(ui, bar),
      });
      self.side = side;
      r
    });
  }
}

// MARK: Settings

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
  General,
  About,
}

/// The pages' tabs, in the language chosen.
pub fn pages() -> [Tag<'static, Page>; 2] {
  [
    Tag::new(Page::General, t!("General").as_str()).icon(Icon::GEAR),
    Tag::new(Page::About, t!("About").as_str()).icon(Icon::INFO),
  ]
}

/// The languages: the system's, then each catalog's, each in itself.
pub fn langs() -> [Tag<'static, Option<&'static str>>; 3] {
  let system = Tag::new(None, t!("System Default").as_str());
  [system, Tag::new(Some("en"), "English"), Tag::new(Some("ru"), "Русский")]
}

/// Shows the window from any app, out of the box.
pub const DEFAULT_HOTKEY: &str = "ctrl+option+m";
pub const LOGIN: &str = "login";
pub const LANG: &str = "lang";
pub const HOTKEY: &str = "hotkey";
pub const RESET: &str = "reset";

/// The settings window: open at login, the language, the global shortcut; About.
pub struct Settings {
  pub page: Page,
  /// `None` where the app can't (a bare binary on macOS).
  pub login: Option<bool>,
  pub lang: Option<&'static str>,
  pub hotkey: Option<Hotkey>,
  /// What changes outside the window: autostart, the locale, the hotkey.
  pub proxy: Option<Proxy>,
  /// Pauses the app's hotkeys while the recorder records, so their chord reaches it.
  pub pause_hotkeys: fn(bool),
  paused: bool,
}

impl Default for Settings {
  fn default() -> Self {
    let hotkey = Hotkey::parse(DEFAULT_HOTKEY).ok();
    let (page, pause_hotkeys) = (Page::General, |_| {});
    Self { page, login: Some(false), lang: None, hotkey, proxy: None, pause_hotkeys, paused: false }
  }
}

impl Settings {
  fn general(&mut self, ui: &mut Ui, fs: &FormStyle) {
    ui.form_section(fs, t!("Startup").as_str(), |ui| {
      if let Some(login) = &mut self.login {
        let row = FormRow::new(t!("Open at login"));
        if ui.form_row(fs, row, |ui| ui.id(LOGIN).toggle(login, ())).changed {
          let on = *login;
          self.post(move |_| _ = autostart::set(&INFO, on).map_err(|e| eprintln!("login: {e}")));
        }
      }

      let row = FormRow::new(t!("Language")).caption(t!("Applies at once to every window."));
      let mut lang = self.lang;
      ui.form_row(fs, row, |ui| {
        let all = langs();
        let langs = Picker::new(&mut lang).options(&all[..1]).divider().options(&all[1..]);
        langs.show(ui.id(LANG))
      });
      if lang != self.lang {
        self.lang = lang;
        let locale = lang.map_or(Locale::System, Locale::Tag);
        self.post(move |main| {
          chose_lang(lang);
          main.set_locale(locale);
          menus(main);
        });
      }
    });

    let default = Hotkey::parse(DEFAULT_HOTKEY).ok();
    let section =
      nimbo::Section::new(t!("Shortcut")).footer(t!("Works in every app, while this one runs."));
    ui.form_section(fs, section, |ui| {
      let before = self.hotkey;
      ui.form_row(fs, FormRow::new(t!("Show Messenger")), |ui| {
        ui.id(HOTKEY).hotkey_recorder(&mut self.hotkey);
        let ui = ui.disabled(self.hotkey == default).a11y_label(&t!("Reset"));
        if ui.help(&t!("Back to the default shortcut")).id(RESET).icon_button(Icon::RESET).clicked {
          self.hotkey = default;
        }
      });
      if self.hotkey != before {
        let h = self.hotkey;
        self.post(move |_| set_hotkey(h, true));
      }
    });
  }

  fn about(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    let head = Style::new().w(grow()).pad(24.0).gap(8.0).align(Align::Center, Align::Start);
    ui.col(head, |ui| {
      avatar(ui, "Messenger", t.accent(), 64.0);
      ui.text("Messenger", TextStyle::new(t.bold().size(20.0), t.text()));
      ui.text("1.0.0", t.muted());
      ui.text(t!("Chats with bots, made with nimbo."), t.label());
      ui.link("github.com/vladkens/nimbo");
    });
  }

  fn post(&self, f: impl FnOnce(&mut Main) + Send + 'static) {
    if let Some(p) = &self.proxy {
      p.post(f);
    }
  }
}

impl App for Settings {
  /// The window losing the keys lets the hotkeys go at once: hidden or covered, it may not run
  /// its view again until it shows.
  fn event(&mut self, _: &mut Cx, ev: &Event) -> bool {
    if *ev == Event::Focus(false) && self.paused {
      (self.pause_hotkeys)(false);
      self.paused = false;
    }

    false
  }

  fn view(&mut self, ui: &mut Ui) {
    ui.follow_system();
    ui.fit_window_height(560.0, 200.0, 600.0);
    let fs = FormStyle::plates(&ui.theme());
    ui.col(Style::new().grow().bg(fs.bg()), |ui| {
      ui.id("tabs").toolbar_tabs(&fs, &mut self.page, &pages());
      ui.id(("page", self.page as u8)).form_scroll(&fs, Style::new().grow(), |ui| {
        match self.page {
          Page::General => ui.col(fs.page(), |ui| self.general(ui, &fs)),
          Page::About => ui.col(Style::new().w(grow()), |ui| self.about(ui)),
        }
      });
    });

    // Also when the recorder lost its focus with the page.
    let recording = ui.focused(Id::of(HOTKEY)) && ui.window_focused();
    if recording != self.paused {
      (self.pause_hotkeys)(recording);
      self.paused = recording;
    }
  }
}

// MARK: The app

/// What the tray's menu does; its status line does nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
  None,
  Open,
  Settings,
  Quit,
}

/// The tray's menu: the unread count, then opening the window and settings, and quitting.
pub fn tray_menu(unread: usize) -> Menu<TrayAction> {
  Menu::new(vec![
    Item::button(TrayAction::None, &tn!("{n} unread", "{n} unread", unread)).enabled(false),
    Item::button(TrayAction::Open, &t!("Open Messenger")),
    Item::button(TrayAction::Settings, &t!("Settings…")).key(Shortcut::char(',').cmd()),
    Item::Divider,
    Item::button(TrayAction::Quit, &t!("Quit")).key(Shortcut::char('q').cmd()),
  ])
}

/// The menu bar's Chat menu, its items the window's commands.
pub fn chat_menu() -> Menu<&'static str> {
  let item = |name: &'static str, label: Localized| {
    let key = KEYS.iter().find(|b| b.command == Command::Action(name)).map(|b| b.keys);
    let item = Item::button(name, &label);
    match key {
      Some(k) => item.key(k),
      None => item,
    }
  };
  Menu::new(vec![
    item("find", t!("Find")),
    Item::Divider,
    item("next", t!("Next Chat")),
    item("previous", t!("Previous Chat")),
    Item::Divider,
    item("profile", t!("Profile")),
    item("sidebar", t!("Show Sidebar")),
  ])
}

#[derive(Default)]
struct State {
  window: Option<Window<Shown>>,
  messenger: Option<Rc<RefCell<Messenger>>>,
  settings: Option<Window<Settings>>,
  tray: Option<Tray<TrayAction>>,
  hotkeys: Option<Hotkeys>,
  /// What settings chose, shown when they open again.
  lang: Option<&'static str>,
  hotkey: Option<Hotkey>,
}

thread_local! {
  /// The app's state on the main thread, where the tray, the hotkey and the instance post.
  static STATE: RefCell<State> = RefCell::new(State::default());
  /// The way back to the main loop, for work that finds the state busy.
  static PROXY: OnceCell<Proxy> = const { OnceCell::new() };
  /// What Settings asked of the hotkeys last.
  static PAUSED: Cell<bool> = const { Cell::new(false) };
}

/// Pauses the hotkeys while Settings records one, or lets them go again; with the state busy (a
/// nested loop), from the main loop once it is free, as asked last by then.
pub fn pause_hotkeys(paused: bool) {
  PAUSED.set(paused);
  STATE.with(|s| match s.try_borrow_mut() {
    Ok(mut s) => _ = s.hotkeys.as_mut().map(|h| h.pause(paused)),
    Err(_) => _ = PROXY.with(|p| p.get().map(|p| p.post(|_| pause_hotkeys(PAUSED.get())))),
  });
}

/// The unread count beside the tray's icon.
fn tray_count(unread: usize) {
  let tray = STATE.with(|s| s.try_borrow().ok().and_then(|s| s.tray.clone()));
  if let Some(tray) = tray {
    tray.set_title(&if unread == 0 { String::new() } else { unread.to_string() });
    tray.set_menu(&tray_menu(unread));
  }
}

/// The menu bar's and the tray's menus in the app's language: at launch, again when it changes.
fn menus(main: &mut Main) {
  let commands = Commands::new().menu(&t!("Chat"), chat_menu()).settings("settings");
  main.set_commands(&commands, |main, name| match name {
    "settings" => settings(main),
    name => {
      let w = STATE.with(|s| s.borrow().window.clone());
      if let Some(w) = w {
        w.with(|w| _ = w.0.try_borrow_mut().map(|mut m| m.run(name)));
      }
    }
  });
  let m = STATE.with(|s| s.borrow().messenger.clone());
  tray_count(m.and_then(|m| m.try_borrow().ok().map(|m| m.unread())).unwrap_or(0));
}

/// Binds the global shortcut that shows the window, or none; `asked` when the user chose it.
pub fn set_hotkey(h: Option<Hotkey>, asked: bool) {
  STATE.with(|s| {
    let Ok(mut s) = s.try_borrow_mut() else { return };
    s.hotkey = h;
    let Some(hk) = s.hotkeys.as_mut() else { return };
    if let Err(e) = hk.set(&[h]) {
      eprintln!("hotkey: {e}");
    }

    // The listener starts when the user chooses a hotkey that needs it, not unasked at launch:
    // Wayland's portal binds any only through it (and asks to approve), and a modifier alone
    // needs it on macOS (with Accessibility access) and Windows. Elsewhere a chord needs none.
    let needs = hotkey::start_asks() || matches!(h, Some(Hotkey::Modifier(_)));
    if asked
      && h.is_some()
      && needs
      && let Err(e) = hk.start()
    {
      eprintln!("hotkey: {e}");
    }
  });
}

/// Remembers the language settings chose.
pub fn chose_lang(lang: Option<&'static str>) {
  STATE.with(|s| s.borrow_mut().lang = lang);
}

/// The settings window's app, on what is chosen now.
pub fn settings_app(login: Option<bool>, proxy: Option<Proxy>) -> Settings {
  let (lang, hotkey) = STATE.with(|s| (s.borrow().lang, s.borrow().hotkey));
  Settings { login, lang, hotkey, proxy, pause_hotkeys, ..Settings::default() }
}

/// An answer came: the app takes it in, whether a window draws or not (macOS draws none that is
/// minimized, covered or on another Space), and the tray counts it; an open window redraws.
fn answered(_: &mut Main) {
  // Busy (a nested loop): again from the main loop once it is free.
  let again = || _ = PROXY.with(|p| p.get().map(|p| p.post(answered)));
  let Ok((m, w)) = STATE.with(|s| s.try_borrow().map(|s| (s.messenger.clone(), s.window.clone())))
  else {
    return again();
  };
  let w = w.filter(|w| w.is_open());
  let Some(m) = m else { return };
  let Ok(mut m) = m.try_borrow_mut() else { return again() };
  let shown = w.is_some() && m.focused;
  m.take_answers(shown);
  drop(m);
  if let Some(w) = w {
    w.redraw();
  }
}

/// Shows the window, opening it when closed.
fn show(main: &mut Main) {
  let open = STATE.with(|s| s.borrow().window.clone()).filter(|w| w.is_open());
  if let Some(w) = open {
    return w.front();
  }

  let Some(m) = STATE.with(|s| s.borrow().messenger.clone()) else { return };
  let w = main.open(&config(), Shown::new(m));
  STATE.with(|s| s.borrow_mut().window = Some(w));
}

/// Shows the settings window, opening it when closed.
fn settings(main: &mut Main) {
  let open = STATE.with(|s| s.borrow().settings.clone()).filter(|w| w.is_open());
  if let Some(w) = open {
    return w.front();
  }

  let login = autostart::state(&INFO).ok().map(|s| s.wanted());
  let app = settings_app(login, Some(main.proxy()));
  let cfg = Config::new("Settings", 560.0, 360.0).fixed().unified().localizations(&[("ru", RU)]);
  let w = main.open(&cfg, app);
  // Closed while recording, the hotkeys work again.
  w.on_close(|_| pause_hotkeys(false));
  STATE.with(|s| s.borrow_mut().settings = Some(w));
}

pub fn config() -> Config {
  Config::new("Messenger", 900.0, 600.0)
    .min_size(560.0, 380.0)
    .unified()
    .localizations(&[("ru", RU)])
}

fn start(main: &mut Main, instance: Option<Instance>) {
  PROXY.with(|p| _ = p.set(main.proxy()));
  // The chats live as long as the app: a window is a view over them.
  let m = Messenger { proxy: Some(main.proxy()), ..Messenger::default() };
  STATE.with(|s| s.borrow_mut().messenger = Some(Rc::new(RefCell::new(m))));
  // The catalogs before any window: the menus are built in the user's language.
  main.set_localizations(&[("ru", RU)]);
  let tray =
    Tray::new(TrayIcon::Path { size: 18.0, d: BUBBLE_ICON.into(), even_odd: false }, "Messenger");
  tray.on_item(|main, _, a| match a {
    TrayAction::Open => show(main),
    TrayAction::Settings => settings(main),
    TrayAction::Quit => main.quit(),
    TrayAction::None => {}
  });

  // The shortcut's edges come on a thread of their own: a press shows the window.
  let proxy = main.proxy();
  let hotkeys = Hotkeys::new(move |edge| {
    if let Edge::Down { repeat: false, .. } = edge {
      proxy.post(show);
    }
  });
  STATE.with(|s| (s.borrow_mut().tray, s.borrow_mut().hotkeys) = (Some(tray), Some(hotkeys)));
  menus(main);
  set_hotkey(Hotkey::parse(DEFAULT_HOTKEY).ok(), false);

  // A second launch, or a click on the Dock's icon, brings the window forward.
  main.on_reopen(show);
  if let Some(instance) = instance
    && let Err(e) = main.serve(instance, |_, _| {})
  {
    eprintln!("instance: {e}");
  }

  main.on_quit(|_| STATE.with(|s| *s.borrow_mut() = State::default()));
  show(main);
}

fn main() {
  let mut args: Vec<String> = std::env::args().skip(1).collect();
  if args.first().is_some_and(|a| a == "settings") {
    args.remove(0);
    let cfg = Config::new("Settings", 560.0, 360.0).fixed().unified().localizations(&[("ru", RU)]);
    return example::run(cfg, Settings::default(), &args, |h| _ = h.frame());
  }

  if args.iter().any(|a| a == "--shot") {
    // The shot: the archive open, a question to Abacus typed, the profile beside the chat.
    return example::run(config(), Messenger::default(), &args, |h| {
      h.frame();
      h.app_mut().open_chat(2);
      h.app_mut().profile = true;
      h.frame();
      h.click_id(field(2));
      h.type_text("12 * 7 + 1");
      h.wait(Duration::from_millis(300));
    });
  }

  // One instance: a second launch asks the first to show its window, and ends.
  let instance = match instance::claim(&INFO) {
    Ok(Claim::Primary(instance)) => Some(instance),
    Ok(Claim::Running) => {
      return instance::send(&INFO, instance::OPEN).unwrap_or_else(|e| eprintln!("instance: {e}"));
    }
    Err(e) => {
      eprintln!("instance: {e}");
      None
    }
  };

  nimbo::app(INFO, |main| start(main, instance));
}
