<div align="center">

<img src="docs/brand/header-iris.png" alt="nimbo" width="600" />

**A small, fast, custom-drawn desktop UI toolkit for Rust**

[<img src="https://badges.ws/crates/v/nimbo" alt="version" />](https://crates.io/crates/nimbo)
[<img src="https://badges.ws/crates/docs/nimbo" alt="docs" />](https://docs.rs/nimbo)
[<img src="https://badges.ws/github/license/vladkens/nimbo" alt="license" />](https://github.com/vladkens/nimbo/blob/main/LICENSE)
[<img src="https://badges.ws/badge/follow-@uid127-black" alt="follow @uid127" />](https://x.com/uid127)

</div>

`nimbo` is a desktop UI toolkit for Rust that draws its own widgets, on the CPU, with one look on macOS, Windows and Linux. Work in progress: the API changes from one pull request to the next.

## 🌟 Features

- 🖌️ **One look** on macOS, Windows and Linux, after macOS System Settings
- 🔤 **The system's text**: shaping, emoji, input methods
- 🖥️ **Drawn on the CPU**: no GPU stack to ship
- 🧩 **Widgets**: buttons, switches, fields, lists of 100k rows, tabs, dialogs
- 🛎️ **Tray apps**: tray menu, global hotkeys, launch at login, overlays
- 📂 **Files**: open and save panels, drag and drop
- 🧪 **Headless tests**: click, type and check pixels without a window
- 📦 **Few crates**: 13 on macOS, 4 on Windows, 2 on Linux

<div align="center">
  <img src="docs/settings.png" alt="the settings example in a macOS window" width="700" />
</div>

## 🤖 How it's made

nimbo is written by AI agents (Claude Code, Codex) following [agents.md](agents.md); I choose what to build and review every pull request. Every visual change is checked in a harness shot, against macOS screenshots at the same scale where the look is meant to match, and a change to a platform's event loop has its frame pacing measured on each backend it touches. This isn't a weekend prompt: I've written software by hand for years — [macmon](https://github.com/vladkens/macmon) (Rust, Apple Silicon system monitor), [twscrape](https://github.com/vladkens/twscrape) (Python, X/Twitter scraping) and [ecloop](https://github.com/vladkens/ecloop) (C, low-level elliptic-curve math) — and agents are how I build now, as with [wavo](https://github.com/vladkens/wavo) (Rust, local speech-to-text on the GPU).

## 📥 Installation

nimbo isn't released yet; take it from git:

```sh
cargo add nimbo --git https://github.com/vladkens/nimbo
```

<details>
<summary>macOS</summary>

It runs on macOS 14 or later; an app built with it sets `MACOSX_DEPLOYMENT_TARGET=14.0`, as this repository does in `.cargo/config.toml`.

</details>

<details>
<summary>Windows</summary>

An app built with it sets `#![cfg_attr(windows, windows_subsystem = "windows")]` in its `main.rs`, as the examples do, so that no console opens next to its window.

</details>

<details>
<summary>Linux</summary>

It needs GTK 3 at run time (`libgtk-3.so.0`, on every desktop install), which it loads for the window, as Sublime Text does; nothing is linked at build time. It runs on Wayland and X11, and `GDK_BACKEND=x11` picks X11 in a Wayland session.

An overlay (`Config::overlay`) is placed at its anchor on X11; on Wayland it needs `libgtk-layer-shell` (wlroots compositors, KDE) to sit above other windows at its anchor, else the compositor places it as any window (GNOME). A tray icon shows where the panel hosts StatusNotifierItems (KDE Plasma, Xfce, Cinnamon, MATE, LXQt; GNOME with the AppIndicator extension, which Ubuntu ships on), and global hotkeys need the GlobalShortcuts portal (KDE Plasma, GNOME 48+), which asks the user to approve them. The open and save panels are the desktop's FileChooser portal, which GNOME and KDE ship.

</details>

## 🚀 Usage

An app builds its frame as an element tree in `view`, and reads clicks where it builds the buttons:

```rust
use nimbo::{App, Config, Font, Style, TextStyle, Ui};

#[derive(Default)]
struct Counter {
  count: i64,
}

impl App for Counter {
  fn view(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    ui.col(Style::new().grow().center().gap(16.0).bg(t.bg), |ui| {
      ui.text(self.count, TextStyle::new(Font::ui(48.0), t.text));
      if ui.button("Add one").clicked {
        self.count += 1;
      }
    });
  }
}

fn main() {
  nimbo::run(Config::new("Counter", 320.0, 240.0), Counter::default(), |_| {});
}
```

`nimbo::run` opens a window; with `--shot counter.png` it renders the frame in the headless harness instead (dark unless `--appearance light`), after the script in its last argument has clicked and typed.

### Examples

```sh
cargo run --release --example gallery                          # every widget, light and dark side by side
cargo run --release --example gallery -- --shot gallery.png    # the same, rendered without a window
```

| Example | What it shows |
|---|---|
| `counter` | a count with buttons on the element tree: hover, press, Tab focus, the theme |
| `gallery` | every widget, in light and dark side by side over the same state |
| `settings` | a settings window in macOS System Settings' look: the title over toolbar tabs, pages of plates |
| `files` | Open… and Save… with the system's panels, and a zone to drop files on, lit while they are dragged over it |
| `lists` | a list of 100,000 rows with selection and keys, a menu, a pop-up button, tabs, a split and a scroll area with fading edges |
| `fields` | text fields, a password, notes over lines and selectable text, with the system's input methods |
| `text` | the system fonts in sizes, weights and scripts, emoji, subpixel positions |
| `shapes` | every shape the renderer draws |
| `input` | a square to drag, move with the arrows, resize with the wheel, copy and paste |
| `tray` | no window of its own: a tray icon whose menu opens the counter, a global hotkey that opens and closes it, launch at login |
| `hud` | an overlay: a recording pill over every app at the bottom of the screen with the pointer, never taking the focus, the mouse going through it |

## 📏 Size

Measured on an **Apple M2**, rustc 1.99, 2026-10-25, without a compiler cache:

| | |
|---|---:|
| Clean build, `counter` example, release | 10.1 s |
| Clean build, `counter` example, debug | 8.3 s |
| `counter` binary, release (stripped) | 1.1 MB (0.87 MB) |
| Crates in the build, nimbo excluded: macOS / Windows / Linux | 13 / 4 / 2 |

## 🤝 Contributing

Pull requests are welcome, and so are your agents: point them at [agents.md](agents.md). `make check` and `make test` pass before a commit; `make prepare` formats the code.

## 🙏 Credits

- [dwrite-hlsl](https://github.com/lhecker/dwrite-hlsl) (MIT): DirectWrite's gamma ratios, used to blend text on Windows.

## 📝 License

Distributed under the [MIT License](LICENSE).

## 🔍 See also

- [zed-industries/zed](https://github.com/zed-industries/zed) – GPUI, the GPU-drawn toolkit of the Zed editor.
- [emilk/egui](https://github.com/emilk/egui) – An immediate-mode GUI in Rust, for the web and native.
- [iced-rs/iced](https://github.com/iced-rs/iced) – A cross-platform GUI library in Rust, inspired by Elm.
- [Sublime Merge](https://www.sublimemerge.com) – The feel nimbo aims for: a custom-drawn UI that starts instantly.
