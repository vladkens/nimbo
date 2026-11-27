// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A messenger with people and bots, the window app in breadth: a unified window with a sidebar
//! of chats (each with its avatar, last message, time and unread count, a bot marked so) and
//! toolbars in the title bar, the sidebar sliding in and out from its toggle and dragged wider
//! or narrower between its bounds; search across every chat; a lazy column of thousands of
//! messages of mixed heights that stays at the bottom as new ones come; a composer that grows
//! with its text, sends on Return, takes emoji from the system's picker, IME and files picked in
//! the open panel or dropped on the chat; a context menu on a message; a contact's card beside
//! the chat, which exports the chat through the save panel and clears it after asking. People
//! and bots answer from a thread of their own, the chat showing "typing…" meanwhile, and Herald
//! posts the news by itself now and then; the data is seeded, the same each run.
//!
//! Around the window: the menu bar's commands and their keys, a tray icon with the unread count
//! and a menu, a settings window in System Settings' look (open at login, a global shortcut that
//! shows the window, the language), one instance (a second launch brings the window forward),
//! English and Russian (`t!`), and names for every control to assistive technology.
//!
//! `cargo run --release --example messenger [-- settings] [--shot <file.png>]`: `settings` shots
//! the settings window; a shot of the messenger takes `--chat <n>` (the chat open, Alice's by
//! default), `--profile` (the contact's card beside it), `--draft <text>` (typed in the
//! composer) and `--search <text>` (in the sidebar's field).

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
  Align, App, AppInfo, Axes, Axis, Binding, Bitmap, Button, ButtonConfiguration, ButtonRole,
  ButtonStyle, Color, Command, Commands, Config, ContentMode, ControlSize, Cx, Dialog, Event,
  FileDialog, Float, FormRow, FormStyle, Hotkey, Icon, Id, Item, Key, List, ListConfiguration,
  ListStyle, Locale, Localized, Main, Menu, NamedKey, Picker, Point, Proxy, Row, Selection, Shadow,
  Shortcut, Split, Style, Tag, TextField, TextStyle, ToolbarItemPlacement, Transition, Ui,
  UnitPoint, Weight, Window, alpha, autostart, character_palette, grow, mix, pct, px, rgb, t, tn,
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
/// The contact's card's width, and the least the chat beside it keeps; narrower, the card goes
/// over the chat.
pub const PANEL_W: f32 = 260.0;
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
/// The sidebar's least and most width, as Messages' on macOS 15, and its share at first.
pub const SIDE_MIN: f32 = 220.0;
pub const SIDE_MAX: f32 = 380.0;
const SIDE: f32 = 0.32;
/// A chat's row in the sidebar: a 44-point avatar beside the name and two lines of the last
/// message; the rows and the search field inset 10 from the sidebar's edges, as Messages'.
pub const ROW_H: f32 = 64.0;
const AVATAR: f32 = 44.0;
const INSET: f32 = 10.0;
/// The selected row's corners.
const ROW_RADIUS: f32 = 10.0;
/// The widest a bubble gets, its padding and its corners, and the space between bubbles.
const BUBBLE_W: f32 = 380.0;
const PAD: [f32; 2] = [12.0, 7.0];
const BUBBLE_RADIUS: f32 = 15.0;
const SPACING: f32 = 6.0;
/// A file's chip and an image's thumbnail in a bubble.
const CHIP_H: f32 = 26.0;
const THUMB: (f32, f32) = (200.0, 130.0);
/// How many messages the archive chat holds.
pub const ARCHIVE: usize = 3000;
/// How often Herald posts the news by itself.
pub const NEWS_EVERY: Duration = Duration::from_secs(20);

pub const PAPERCLIP: Icon =
  Icon::new("M20 11l-8 8a5 5 0 0 1-7-7l8-8a3.5 3.5 0 0 1 5 5l-8 8a2 2 0 0 1-3-3l7-7");
pub const ARROW_UP: Icon = Icon::new("M12 19V5M6 11l6-6 6 6");
/// A bell struck through: a muted chat.
pub const MUTED: Icon = Icon::new(
  "M18 16.5H5.5c1.2-1.2 1.8-2.8 1.8-4.5V10a4.7 4.7 0 0 1 7.6-3.7M18 11v1c0 1.7.6 3.3 1.8 4.5M10 \
   19.5a2.2 2.2 0 0 0 4 0M4 4l16 16",
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

msgid "online"
msgstr "в сети"

msgid "last seen recently"
msgstr "был(а) недавно"

msgid "Bio"
msgstr "О себе"

msgid "Description"
msgstr "Описание"

msgid "Username"
msgstr "Имя пользователя"

msgid "Messages"
msgstr "Сообщения"

msgid "Emoji & Symbols"
msgstr "Эмодзи и символы"

msgid "No messages here yet"
msgstr "Здесь пока нет сообщений"

msgid "Designs the windows you look at."
msgstr "Рисует окна, на которые вы смотрите."

msgid "Ports it all to Windows, one key at a time."
msgstr "Переносит всё на Windows, клавишу за клавишей."

msgid "Posts the build's news now and then."
msgstr "Время от времени пишет новости сборки."
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
  /// Posts the news by itself ([Messenger::post_news]).
  Herald,
}

/// Who a chat is with: a person, who answers in a few words, or a bot, in its own way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contact {
  Person,
  Bot(Bot),
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

/// What a person says back.
const REPLIES: [&str; 6] = [
  "Sounds good!",
  "Ha, nice 😄",
  "Let me check and get back to you.",
  "👍",
  "On it.",
  "Can we talk after lunch?",
];

/// Herald's news, in turn.
pub const NEWS: [&str; 6] = [
  "Build 1.4.2 passed on macOS, Windows and Linux.",
  "Scrolling the archive now takes 0.2 ms a frame.",
  "New in the gallery: a date picker in segments.",
  "Cold start is down to 38 ms on an M1.",
  "The emoji button opens the system's picker now.",
  "Dark mode follows the system's accent colour.",
];

/// What you and Alice said, and you and Ken: (yours, the text).
const ALICE: [(bool, &str); 6] = [
  (false, "Morning! Did the new build land?"),
  (true, "It did, and it's 2 MB smaller."),
  (false, "Nice. The sidebar feels snappier too."),
  (true, "Lazy column: thousands of rows, no wait."),
  (false, "Can you send me the shot of the dark mode?"),
  (false, "The one with the profile open, if you have it 🙏"),
];

const KEN: [(bool, &str); 4] = [
  (true, "How's the Windows port going?"),
  (false, "The emoji panel opens now. Win+. was the trick."),
  (true, "Great, ship it."),
  (false, "Will do after the tests pass."),
];

/// A contact's answer to `text`, the `n`th it gives.
pub fn answer(who: Contact, text: &str, n: usize) -> String {
  let bot = match who {
    Contact::Person => return REPLIES[(text.len() + n) % REPLIES.len()].into(),
    Contact::Bot(bot) => bot,
  };

  match bot {
    Bot::Echo => text.chars().rev().collect(),
    Bot::Oracle => FORTUNES[(text.len() + n) % FORTUNES.len()].into(),
    Bot::Abacus => sum(text).map_or("Send me a sum, like 2 + 2.".into(), |v| format!("= {v}")),
    Bot::Archive => format!("Kept. That makes {} messages here.", n + 1),
    Bot::Herald => "I post the news by myself: stay tuned.".into(),
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
  pub handle: &'static str,
  pub about: Localized,
  pub contact: Contact,
  /// A person seen online now; a bot always answers.
  pub online: bool,
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
  fn new(
    name: &'static str,
    handle: &'static str,
    about: Localized,
    contact: Contact,
    tint: Color,
  ) -> Self {
    let (msgs, draft, files) = (Vec::new(), String::new(), Vec::new());
    Self {
      name,
      handle,
      about,
      contact,
      online: false,
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

  pub fn is_bot(&self) -> bool {
    matches!(self.contact, Contact::Bot(_))
  }

  fn push(&mut self, next: &mut u64, mine: bool, text: String, files: Vec<Attachment>) {
    let at = self.msgs.last().map_or(0, |m| m.at + 1);
    *next += 1;
    self.msgs.push(Msg { id: *next, mine, text, files, at });
  }

  /// The last message's text, its lines joined, for the sidebar: "You: " before yours.
  fn snippet(&self) -> String {
    let m = self.msgs.last();
    let text = m.map_or(String::new(), |m| match m.text.is_empty() {
      true => m.files.first().map_or(String::new(), Attachment::name),
      false => m.text.split_whitespace().collect::<Vec<_>>().join(" "),
    });
    match m.is_some_and(|m| m.mine) {
      true => format!("{}: {text}", t!("You")),
      false => text,
    }
  }

  /// Under the name: "typing…", "bot", "online" or "last seen recently".
  fn status(&self) -> Localized {
    match (self.pending > 0, self.is_bot(), self.online) {
      (true, ..) => t!("typing…"),
      (_, true, _) => t!("bot"),
      (_, _, true) => t!("online"),
      _ => t!("last seen recently"),
    }
  }
}

/// `9:00` and on, for a message's minute.
pub fn clock(at: u32) -> String {
  let m = 9 * 60 + at;
  format!("{}:{:02}", m / 60 % 24, m % 60)
}

/// The seeded chats, people and bots: a few short ones and the archive's thousands.
pub fn chats() -> (Vec<Chat>, u64) {
  let mut next = 0;
  let mut seed = Seed(0x9e37_79b9_7f4a_7c15);
  let bot =
    |name, handle, about, bot, tint| Chat::new(name, handle, about, Contact::Bot(bot), tint);
  let mut bots = [
    bot("Echo", "@echo_bot", t!("Repeats what you say, backwards."), Bot::Echo, rgb(0x3b82f6)),
    bot(
      "Oracle",
      "@oracle_bot",
      t!("Tells your fortune, one line at a time."),
      Bot::Oracle,
      rgb(0xa855f7),
    ),
    bot(
      "Abacus",
      "@abacus_bot",
      t!("Adds, subtracts, multiplies and divides."),
      Bot::Abacus,
      rgb(0x30a46c),
    ),
    bot(
      "Archive",
      "@archive_bot",
      t!("Keeps everything, thousands of messages."),
      Bot::Archive,
      rgb(0xf5a524),
    ),
  ];
  for (i, c) in bots.iter_mut().enumerate() {
    let Contact::Bot(bot) = c.contact else { continue };
    let n = if bot == Bot::Archive { ARCHIVE } else { 4 + 2 * i };
    // You ask, the bot answers you; the archive keeps both.
    for k in 0..n {
      let mine = if bot == Bot::Archive { seed.below(3) == 0 } else { k % 2 == 0 };
      let text = match (bot, mine) {
        (Bot::Archive, _) => prose(&mut seed),
        (Bot::Abacus, true) => {
          format!("{} * {} + {}", 2 + seed.below(20), seed.below(10), seed.below(100))
        }
        (Bot::Echo, true) => ECHOES[seed.below(ECHOES.len())].into(),
        (_, true) => prose(&mut seed),
        (_, false) => {
          answer(c.contact, &c.msgs.last().map_or(String::new(), |m| m.text.clone()), k)
        }
      };

      c.push(&mut next, mine, text, Vec::new());
    }
  }

  let person = |name, handle, about, tint, said: &[(bool, &str)], next: &mut u64| {
    let mut c = Chat::new(name, handle, about, Contact::Person, tint);
    for &(mine, text) in said {
      c.push(next, mine, text.into(), Vec::new());
    }

    c
  };
  let mut alice = person(
    "Alice Moreau",
    "@alice",
    t!("Designs the windows you look at."),
    rgb(0xec4899),
    &ALICE,
    &mut next,
  );
  alice.online = true;
  let mut ken = person(
    "Ken Tanaka",
    "@ken",
    t!("Ports it all to Windows, one key at a time."),
    rgb(0x0ea5e9),
    &KEN,
    &mut next,
  );
  ken.muted = true;

  let mut herald = bot(
    "Herald",
    "@herald_bot",
    t!("Posts the build's news now and then."),
    Bot::Herald,
    rgb(0xef4444),
  );
  for news in &NEWS[..3] {
    herald.push(&mut next, false, (*news).into(), Vec::new());
  }

  let [echo, mut oracle, abacus, archive] = bots;
  (herald.unread, oracle.unread) = (1, 2);
  (vec![alice, herald, echo, oracle, abacus, ken, archive], next)
}

/// Where each contact is in the seeded chats.
pub const ALICE_AT: usize = 0;
pub const HERALD_AT: usize = 1;
pub const ECHO_AT: usize = 2;
pub const ORACLE_AT: usize = 3;
pub const ABACUS_AT: usize = 4;
pub const KEN_AT: usize = 5;
pub const ARCHIVE_AT: usize = 6;

// MARK: Bots

struct Ask {
  chat: usize,
  who: Contact,
  text: String,
  n: usize,
}

pub struct Reply {
  pub chat: usize,
  pub text: String,
}

/// The contacts' thread: it answers each question after `delay`, in order, and wakes the window.
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
        let text = answer(a.who, &a.text, a.n);
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
  /// The sidebar's share of the window, dragged between [SIDE_MIN] and [SIDE_MAX] wide.
  pub side: f32,
  pub profile: bool,
  /// Files dragged over the chat.
  pub dragging: bool,
  pub confirm: bool,
  /// How long a contact takes to answer.
  pub delay: Duration,
  /// The window's proxy to the main thread: the tray's count follows the unread, and Herald
  /// posts on the app's loop ([news]); without one (the harness), on the window's clock.
  pub proxy: Option<Proxy>,
  /// The news Herald posted since the app started.
  pub news: usize,
  /// When Herald posts next on the window's clock.
  due: Option<Duration>,
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
      side: SIDE,
      profile: false,
      dragging: false,
      confirm: false,
      delay: Duration::from_millis(900),
      proxy: None,
      news: 0,
      due: None,
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

  /// Sends the composer's text and files, and asks the contact.
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
      _ = b.asks.send(Ask { chat: self.open, who: c.contact, text, n });
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

  /// Starts the contacts with the first build. In the app an answer goes to the main loop
  /// ([answered]), drawn or not; in the harness it wakes this window, whose build takes it in,
  /// and Herald posts on the window's clock. A focused window reads the open chat.
  fn receive(&mut self, ui: &mut Ui) {
    if self.bots.is_none() {
      let waker = ui.waker().clone();
      self.bots = Some(match self.proxy.clone() {
        Some(p) => Bots::start(self.delay, move || p.post(answered)),
        None => Bots::start(self.delay, move || waker.wake()),
      });
    }

    let shown = ui.window_focused();
    if self.proxy.is_none() {
      let now = ui.now();
      let due = *self.due.get_or_insert(now + NEWS_EVERY);
      if now >= due {
        self.post_news(shown);
        self.due = Some(now + NEWS_EVERY);
      }

      ui.request_frame_in(self.due.map_or(NEWS_EVERY, |d| d - now));
    }

    self.take_answers(shown);
  }

  /// Takes the contacts' answers in (`shown`: the window has focus, so the open chat is read):
  /// a chat not shown counts them unread unless muted.
  pub fn take_answers(&mut self, shown: bool) {
    let replies: Vec<Reply> =
      self.bots.as_ref().map_or(Vec::new(), |b| b.replies.try_iter().collect());
    for r in replies {
      let c = &mut self.chats[r.chat];
      c.pending = c.pending.saturating_sub(1);
      self.came(r.chat, r.text, shown);
    }

    // What the focused window shows is read, also what came while it was in the background.
    if shown {
      self.chats[self.open].unread = 0;
    }

    self.tell();
  }

  /// Herald posts its next news, unasked (`shown`: the window has focus).
  pub fn post_news(&mut self, shown: bool) {
    let text = NEWS[(3 + self.news) % NEWS.len()];
    self.news += 1;
    self.came(HERALD_AT, text.into(), shown);
    self.tell();
  }

  /// A message came to chat `i`: unread unless the focused window shows it, or it is muted.
  fn came(&mut self, i: usize, text: String, shown: bool) {
    let c = &mut self.chats[i];
    c.push(&mut self.next, false, text, Vec::new());
    if i != self.open || !shown {
      c.unread += !c.muted as usize;
    }
  }

  /// The tray's count, when it changed.
  fn tell(&mut self) {
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

/// A contact's picture: a person's initials in a circle, a bot's first letter in a rounded
/// square, as an app's icon.
fn avatar(ui: &mut Ui, name: &str, tint: Color, bot: bool, d: f32) {
  let radius = if bot { (d * 0.28).round() } else { d / 2.0 };
  let s = Style::new().w(px(d)).h(px(d)).radius(radius).bg(tint).center();
  let letters = name.split_whitespace().filter_map(|w| w.chars().next());
  let letters: String = letters.take(if bot { 1 } else { 2 }).collect();
  let t = ui.theme();
  let size = if bot { d * 0.46 } else { d * 0.38 };
  ui.row(s, |ui| ui.text(&letters, TextStyle::new(t.bold().size(size), [1.0; 4]).nowrap()));
}

/// "bot" in a small tag after a bot's name, in `ink` on a tint of it.
fn bot_tag(ui: &mut Ui, ink: Color) {
  let t = ui.theme();
  let font = t.small().size(10.0).weight(Weight::Semibold);
  let tag = Style::new().h(px(15.0)).pad_xy(4.0, 0.0).radius(4.0).center().bg(alpha(ink, 0.16));
  ui.row(tag, |ui| ui.text(t!("bot"), TextStyle::new(font, ink).nowrap()));
}

/// The unread count in a pill: the accent's, grey for a muted chat; inverted on the selection.
fn count(ui: &mut Ui, n: usize, muted: bool, selected: bool) {
  let (t, ct) = (ui.theme(), ui.controls_theme());
  let (bg, ink) = match (selected, muted) {
    (true, _) => (ct.on_accent(), ct.accent()),
    (false, true) => (t.tertiary(), [1.0; 4]),
    (false, false) => (t.accent(), [1.0; 4]),
  };

  let font = t.small().weight(Weight::Semibold);
  let pill = Style::new().h(px(20.0)).min_w(20.0).pad_xy(6.0, 0.0).radius(10.0).center().bg(bg);
  ui.row(pill, |ui| ui.text(n.to_string(), TextStyle::new(font, ink).nowrap()));
}

/// The chat list's look: rows on the sidebar's ground, the selected one in the accent in
/// rounded corners wherever the focus is (grey only while the window is in the background, as
/// macOS's selections are), a light fill fading in under the pointer.
struct ChatRows;

impl ListStyle for ChatRows {
  fn make_body<'a>(&self, ui: &mut Ui<'a>, mut c: ListConfiguration<'_, 'a>) {
    let (t, accent) = (ui.theme(), ui.controls_theme().accent());
    ui.scroll_area(c.frame(), |ui| {
      c.rows(ui, |ui, mut r| {
        let hot = ui.hot(r.id(), r.is_hovered() && !r.is_selected());
        let bg = if r.is_selected() { accent } else { alpha(t.text(), 0.05 * hot) };
        ui.row(r.frame().radius(ROW_RADIUS).bg(bg), |ui| r.content(ui));
      })
    });
  }
}

/// The text colours of a row: its own and the secondary, white on the selection.
fn inks(ui: &Ui, row: Row) -> (Color, Color) {
  let (t, ct) = (ui.theme(), ui.controls_theme());
  match row.selected {
    true => (ct.on_accent(), alpha(ct.on_accent(), 0.8)),
    false => (t.text(), t.secondary()),
  }
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
      ui.col(Style::new().w(grow()).pads([0.0, INSET, 8.0, INSET]), |ui| {
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
    let list = List::new(chats.len(), ROW_H).selection(&mut self.sel);

    let frame = Style::new().grow().pads([0.0, INSET, 8.0, INSET]);
    let row = Style::new().w(grow()).pad_xy(10.0, 0.0).gap(10.0).align(Align::Start, Align::Center);
    let ui = ui.list_style(&ChatRows).a11y_label(&t!("Chats")).id(CHATS);
    let r = list.style(frame).row_style(row).show(ui, |ui, row| {
      let c = &chats[row.index];
      let typing = c.pending > 0;
      let what = if typing { t!("typing…").as_str().to_owned() } else { c.snippet() };

      // Read as "Oracle, bot, A frame you never…, 2 unread", not letter by letter.
      let mut name = c.name.to_owned();
      if c.is_bot() {
        name += &format!(", {}", t!("bot"));
      }

      name += &format!(", {what}");
      if c.unread > 0 {
        name += &format!(", {}", tn!("{n} unread", "{n} unread", c.unread));
      }

      if let Some(a) = ui.a11y_of(row.id) {
        a.name(&name);
      }

      let (ink, dim) = inks(ui, row);
      avatar(ui, c.name, c.tint, c.is_bot(), AVATAR);
      // As high as the name and two lines whatever the message's are: the names line up.
      let line = ui.line_height(t.body());
      ui.col(Style::new().w(grow()).h(px(3.0 * line + 2.0)).gap(2.0), |ui| {
        ui.row(Style::new().w(grow()).gap(5.0).align(Align::Start, Align::Center), |ui| {
          ui.row(Style::new().clip(), |ui| {
            ui.text(c.name, TextStyle::new(t.bold(), ink).line_limit(1))
          });
          if c.is_bot() {
            bot_tag(ui, dim);
          }

          if c.muted {
            ui.icon(MUTED, 13.0, dim);
          }

          ui.spacer();
          let at = c.msgs.last().map_or(String::new(), |m| clock(m.at));
          ui.text(at, TextStyle::new(t.font().size(12.0), dim).nowrap());
        });
        ui.row(Style::new().w(grow()).gap(8.0).align(Align::Start, Align::Start), |ui| {
          let ink = if typing && !row.selected { t.accent() } else { dim };
          ui.row(Style::new().w(grow()).clip(), |ui| {
            ui.text(&what, TextStyle::new(t.font(), ink).line_limit(2))
          });
          if c.unread > 0 {
            count(ui, c.unread, c.muted, row.selected);
          }
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
      let empty = Style::new().w(grow()).pads([16.0, INSET + 10.0, 0.0, INSET + 10.0]);
      ui.col(empty, |ui| ui.text(t!("No messages found"), t.muted()));
      return;
    }

    let chats = &self.chats;
    let list = List::new(hits.len(), 52.0).selection(&mut self.hit);

    let frame = Style::new().grow().pads([0.0, INSET, 8.0, INSET]);
    let row = Style::new().w(grow()).pad_xy(10.0, 0.0).align(Align::Start, Align::Center);
    let ui = ui.list_style(&ChatRows).a11y_label(&t!("Found")).id(HITS);
    let r = list.style(frame).row_style(row).show(ui, |ui, row| {
      let (c, i) = hits[row.index];
      let (ink, dim) = inks(ui, row);

      ui.col(Style::new().w(grow()).gap(2.0), |ui| {
        ui.row(Style::new().w(grow()).gap(8.0), |ui| {
          ui.row(Style::new().w(grow()).clip(), |ui| {
            ui.text(chats[c].name, TextStyle::new(t.bold(), ink).line_limit(1))
          });
          ui.text(clock(chats[c].msgs[i].at), TextStyle::new(t.font().size(12.0), dim).nowrap());
        });
        ui.row(Style::new().w(grow()).clip(), |ui| {
          ui.text(&chats[c].msgs[i].text, TextStyle::new(t.font(), dim).line_limit(1))
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

/// The send button: a circle of the accent with a white arrow, as Messages' is; grey with
/// nothing to send.
struct Round;

impl ButtonStyle for Round {
  fn make_body(&self, ui: &mut Ui<'_>, mut c: ButtonConfiguration<'_>) {
    let t = ui.theme();
    let hot = ui.hot(c.id(), c.is_hovered());
    let bg = match (c.is_enabled(), c.is_pressed()) {
      (false, _) => t.raised(),
      (true, true) => mix(t.accent(), [0.0, 0.0, 0.0, 1.0], 0.2),
      (true, false) => mix(t.accent(), t.accent_hover(), hot),
    };

    let ink = if c.is_enabled() { [1.0; 4] } else { t.tertiary() };
    let d = t.sizes().control_h() + 4.0;
    let s = Style::new().w(px(d)).h(px(d)).radius(d / 2.0).center().bg(bg);
    ui.row(s, |ui| c.label(ui, TextStyle::new(t.bold().size(13.0), ink)));
  }
}

/// A row of a plate that acts, as a grouped form's buttons are: its label at the leading edge in
/// the accent, red for a destructive one, a fill fading in under the pointer.
struct PlateButton;

impl ButtonStyle for PlateButton {
  fn make_body(&self, ui: &mut Ui<'_>, mut c: ButtonConfiguration<'_>) {
    let (t, fs) = (ui.theme(), FormStyle::plates(&ui.theme()));
    let hot = ui.hot(c.id(), c.is_hovered());
    let k = if c.is_pressed() { 0.1 } else { 0.05 * hot };
    let ink = if c.role() == Some(ButtonRole::Destructive) { t.danger() } else { t.accent() };
    let s = fs.row().align(Align::Start, Align::Center).bg(alpha(t.text(), k));
    let s = if c.is_enabled() { s } else { s.opacity(t.disabled()) };
    ui.row(s, |ui| c.label(ui, fs.label().color(ink).line_limit(1)));
  }
}

/// A hairline between two rows of a plate, inset as a grouped form's.
fn hairline(ui: &mut Ui, fs: &FormStyle) {
  let [l, r] = fs.inset();
  let line = Style::new().w(grow()).h(px(fs.hairline())).pads([0.0, r, 0.0, l]);
  ui.row(line, |ui| ui.el(Style::new().grow().bg(fs.divider())));
}

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
    let scroll = Style::new().grow().pad_xy(16.0, 12.0).scroll(Axes::VERTICAL);
    if msgs.is_empty() {
      let empty = Style::new().grow().center();
      ui.with_id(area).col(empty, |ui| ui.text(t!("No messages here yet"), t.muted()));
      return;
    }

    ui.with_id(area).default_scroll_anchor(UnitPoint::BOTTOM).scroll_area(scroll, |ui| {
      ui.with_id(column).lazy_col(
        Style::new().w(grow()).gap(SPACING),
        msgs.len(),
        |i| msgs[i].id,
        |ui, i| {
          let m = &msgs[i];
          let id = Id::of(BUBBLE).key(m.id);
          let side = if m.mine { Align::End } else { Align::Start };
          let (bg, ink) = if m.mine { (t.accent(), [1.0; 4]) } else { (t.raised(), t.text()) };
          ui.row(Style::new().w(grow()).align(side, Align::Start), |ui| {
            let bubble = Style::new().max_w(max).pad_xy(PAD[0], PAD[1]).gap(6.0);
            let who = if m.mine { t!("You").to_string() } else { c.name.to_owned() };
            ui.a11y(id, Role::Group).name(&format!("{who}, {}", clock(m.at)));

            ui.with_id(id).col(bubble.radius(BUBBLE_RADIUS).bg(bg), |ui| {
              for f in &m.files {
                match &f.image {
                  Some(b) => {
                    _ = ui
                      .aspect_ratio(ContentMode::Fill)
                      .image(b, Style::new().w(px(THUMB.0)).h(px(THUMB.1)).radius(10.0))
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

              // The time at the end of the last line, as Telegram puts it.
              let line = Style::new().gap(8.0).align(Align::End, Align::End);
              ui.row(if m.text.is_empty() { line.w(grow()) } else { line }, |ui| {
                if !m.text.is_empty() {
                  ui.text(&m.text, t.body().color(ink));
                }

                let small = TextStyle::new(t.font().size(11.0), alpha(ink, 0.65));
                ui.text(clock(m.at), small.nowrap());
              });
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

  /// The composer: the files to send, then the attach button, the field that grows up to six
  /// lines, the emoji button (where the OS has a picker) and the send button.
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
    // As high as the send button at least.
    let least = t.sizes().control_h() + 4.0;
    let h = (text_h + 10.0).clamp(least, 6.0 * line + 10.0);

    let mut send = false;
    let bar = Style::new().w(grow()).pads([10.0, 12.0, 12.0, 12.0]).gap(8.0);

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
              ui.text(f.name(), t.label());
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

      // The buttons sit on the middle of the field's last line, as it grows.
      let slot = Style::new().h(px(least)).center();
      ui.row(Style::new().w(grow()).gap(6.0).align(Align::Start, Align::End), |ui| {
        ui.row(slot, |ui| {
          let ui = ui.a11y_label(&t!("Attach Files")).help(&t!("Attach Files")).id(ATTACH);
          if ui.icon_button(PAPERCLIP).clicked {
            self.asked = Some(Asked::Attach(self.open));
            ui.file_dialog(FileDialog::open(&t!("Attach Files")));
          }
        });

        ui.col(Style::new().w(grow()).h(px(h)), |ui| {
          TextField::new(&mut c.draft, &t!("Message")).axis(Axis::Vertical).show(ui.with_id(field))
        });
        // The system's picker, for the field: none where the OS has none (Linux).
        if character_palette::available().is_ok() {
          ui.row(slot, |ui| {
            let ui = ui.a11y_label(&t!("Emoji")).help(&t!("Emoji & Symbols")).id(EMOJI);
            if ui.icon_button(Icon::SMILE).clicked {
              ui.focus(Some(field));
              ui.character_palette();
            }
          });
        }

        let empty = c.draft.trim().is_empty() && c.files.is_empty();
        let ui = ui.disabled(empty).a11y_label(&t!("Send")).help(&t!("Send")).id(SEND);
        send |= ui.button_style(&Round).button(ARROW_UP).clicked;
      });
    });
    send |= ui.command(Id::of(COMPOSER)) == Some(Command::Action("send"));
    if send {
      self.send();
    }
  }

  /// The contact's card, as in Messages and Telegram: the avatar, name and status in the
  /// middle, then plates of who it is, mute, and what to do with the chat; over the chat's
  /// trailing edge (`over`) where the window is too narrow for both.
  fn panel(&mut self, ui: &mut Ui, over: bool) {
    let (t, fs) = (ui.theme(), FormStyle::plates(&ui.theme()));
    let c = &mut self.chats[self.open];
    if over {
      let at = Float::parent().attach(UnitPoint::TOP_TRAILING).anchor(UnitPoint::TOP_TRAILING);
      ui.float(at.z(5));
    }

    let card = Style::new().w(px(PANEL_W)).h(grow()).bg(t.bg());
    let card =
      if over { card.h(pct(100.0)).shadow(Shadow::new(0.0, 12.0, t.shadow())) } else { card };
    let page = Style::new().w(grow()).pads([24.0, 12.0, 16.0, 12.0]).gap(12.0);
    let mut export = false;

    ui.id(PANEL).col(card, |ui| {
      ui.scroll_area(Style::new().grow(), |ui| {
        ui.col(page.align(Align::Center, Align::Start), |ui| {
          ui.col(Style::new().w(grow()).gap(4.0).align(Align::Center, Align::Start), |ui| {
            avatar(ui, c.name, c.tint, c.is_bot(), 80.0);
            ui.el(Style::new().h(px(6.0)));
            ui.text(c.name, TextStyle::new(t.bold().size(17.0), t.text()).line_limit(2));
            let ink = if c.pending > 0 || c.online { t.accent() } else { t.secondary() };
            ui.text(c.status(), TextStyle::new(t.font(), ink).line_limit(1));
          });
          ui.el(Style::new().h(px(4.0)));

          let about = if c.is_bot() { t!("Description") } else { t!("Bio") };
          ui.form_section(&fs, "", |ui| {
            ui.form_row(&fs, FormRow::new(c.about.as_str()).caption(about), |_| {});
            ui.form_row(&fs, FormRow::new(c.handle).caption(t!("Username")), |_| {});
            ui.form_row(&fs, FormRow::new(t!("Messages")), |ui| {
              ui.text(c.msgs.len().to_string(), TextStyle::new(t.font(), t.secondary()).nowrap())
            });
          });
          ui.form_section(&fs, "", |ui| {
            ui.form_row(&fs, FormRow::new(t!("Mute")), |ui| ui.id(MUTE).toggle(&mut c.muted, ()))
          });

          ui.col(fs.group().clip(), |ui| {
            let ui_ = ui.button_style(&PlateButton).id(EXPORT);
            export = Button::new(t!("Export Chat…")).show(ui_).clicked;
            hairline(ui, &fs);

            let clear = Button::new(t!("Clear History…")).role(ButtonRole::Destructive);
            let ui = ui.disabled(c.msgs.is_empty()).button_style(&PlateButton).id(CLEAR);
            if clear.show(ui).clicked {
              self.confirm = true;
            }
          });
        });
      });
    });

    if export {
      self.asked = Some(Asked::Export(self.open));
      let name = format!("{}.txt", self.chats[self.open].name);
      ui.file_dialog(FileDialog::save(&t!("Export Chat…")).name(&name));
    }

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
    let (name, status, lit) = (c.name, c.status(), c.pending > 0 || c.online);

    ui.col(Style::new().grow().pads([bar, 0.0, 0.0, 0.0]), |ui| {
      ui.toolbar(|ui| {
        ui.toolbar_item(ToolbarItemPlacement::Principal, |ui| {
          ui.col(Style::new().align(Align::Center, Align::Center), |ui| {
            ui.text(name, TextStyle::new(t.bold(), t.text()).line_limit(1));
            let ink = if lit { t.accent() } else { t.secondary() };
            ui.text(status, TextStyle::new(t.font().size(11.0), ink).line_limit(1));
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

      // Sliding, the sidebar takes that part of its share, the share kept for when it is out;
      // dragged, it stays between its bounds, the chat keeping its least width.
      let mut side = std::mem::take(&mut self.side);
      let mut sliding = side * out;
      let ratio = if out < 1.0 { &mut sliding } else { &mut side };
      let split = Split::navigation().ratio(ratio).min(SIDE_MIN * out, CHAT_MIN);
      let r = split.max(SIDE_MAX, f32::INFINITY).show(ui.id(SPLIT), |ui, pane| match pane {
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
      avatar(ui, "Messenger", t.accent(), true, 64.0);
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

/// Herald posts the news, every [NEWS_EVERY] while the app runs, a window open or not, and the
/// tray counts it; an open window redraws.
fn news(_: &mut Main) {
  let after = |d| _ = PROXY.with(|p| p.get().map(|p| p.after(d, news)));
  // Busy (a nested loop): again a moment later.
  let busy = Duration::from_millis(100);
  let Ok((m, w)) = STATE.with(|s| s.try_borrow().map(|s| (s.messenger.clone(), s.window.clone())))
  else {
    return after(busy);
  };
  let w = w.filter(|w| w.is_open());
  let Some(m) = m else { return };
  let Ok(mut m) = m.try_borrow_mut() else { return after(busy) };
  let shown = w.is_some() && m.focused;
  m.post_news(shown);
  drop(m);
  if let Some(w) = w {
    w.redraw();
  }

  after(NEWS_EVERY);
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
  main.proxy().after(NEWS_EVERY, news);
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
    // The shot's state: the chat open (Alice's), the card beside it, a draft and a search.
    let mut value = |flag: &str| {
      let at = args.iter().position(|a| a == flag)?;
      let v = args.get(at + 1).cloned();
      args.drain(at..(at + 2).min(args.len()));
      v
    };
    let chat = value("--chat").and_then(|v| v.parse().ok()).unwrap_or(ALICE_AT);
    let (draft, search) = (value("--draft"), value("--search"));
    let profile = args.iter().position(|a| a == "--profile").map(|at| args.remove(at)).is_some();

    return example::run(config(), Messenger::default(), &args, |h| {
      h.frame();
      h.app_mut().open_chat(chat);
      h.app_mut().profile = profile;
      h.frame();
      if let Some(d) = draft {
        h.click_id(field(chat));
        h.type_text(&d);
      }

      if let Some(q) = search {
        h.click_id(Id::of(SEARCH));
        h.type_text(&q);
      }

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
