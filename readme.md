<div align="center">

<img src="docs/brand/header-iris.png" alt="nimbo" width="600" />

**A small, fast, custom-drawn desktop UI toolkit for Rust**

[<img src="https://badges.ws/crates/v/nimbo" alt="version" />](https://crates.io/crates/nimbo)
[<img src="https://badges.ws/crates/docs/nimbo" alt="docs" />](https://docs.rs/nimbo)
[<img src="https://badges.ws/github/license/vladkens/nimbo" alt="license" />](https://github.com/vladkens/nimbo/blob/main/LICENSE)
[<img src="https://badges.ws/badge/follow-@uid127-black" alt="follow @uid127" />](https://x.com/uid127)

</div>

`nimbo` is a desktop UI toolkit for Rust that draws every widget itself, on the CPU, over a thin native layer for the window, the input and the text: AppKit and CoreText on macOS, Win32 and DirectWrite on Windows, GTK 3 with FreeType and HarfBuzz on Linux. The look follows macOS System Settings, measured from its screenshots, in light and dark. A settings window, a tray app or a HUD takes a few hundred lines and 13 crates, and the whole toolkit builds from scratch in about 10 seconds.

## 🌟 Features

- 🖌️ **One look on three systems**, measured from macOS System Settings, in light and dark and the system's accent colour
- 🔤 **The system's text stack**: CoreText, DirectWrite or FreeType with HarfBuzz; shaping, subpixel positioning, colour emoji, input methods
- 🖥️ **A CPU renderer**: no GPU, shaders or drivers to ship; frames paced to the display, an idle window asleep
- 🧩 **Widgets as headless behaviours** plus a theme: buttons, switches, fields, selects, tabs, splits, dialogs, a list of 100k rows; or your own look over the same behaviour
- 🛎️ **Tray and HUD apps**: a tray menu, global hotkeys, launch at login, overlays that never take the focus, the system's permissions
- 📂 **Files**: the system's open and save panels, files dragged onto a window
- 🧪 **A headless harness**: every example renders to a PNG without a window; tests click, type and check pixels, in parallel
- 📦 **13 crates on macOS, 4 on Windows, 2 on Linux**: GTK is loaded at run time, nothing to link

<div align="center">
  <img src="docs/settings.png" alt="the settings example in a macOS window" width="700" />
</div>

## 🧭 Why nimbo

It started with a dictation app. The one in use, Handy, is Tauri over a webview and idles at **565 MB**. The rewrite in egui on wgpu pulled in 226 crates and an 18 MB binary, and still didn't look like macOS: egui draws its own text, and its "strong" text changed the colour, not the weight. Four more tries put the same app on egui (glow, then wgpu) and on GPUI (directly, then through a component kit): **337 to 911 crates** in the lockfile, GPUI building only with a vendored patch, and a GPU surface and a text stack of their own for what was a settings window. The same window in plain AppKit idled at **13 MB**.

Native controls on each OS kept those 13 MB, but every feature was built three times, 79% of the code was per-platform glue, and a waveform had nowhere to go. So the next toolkit went Sublime Text's way: draw the UI yourself, take the window, the input and the text from the OS. On Metal it opened a window in under 200 ms (Zed: 299 ms), idled at 0.08% CPU and drew text within a few percent of AppKit's coverage. Then came Windows and Linux, where Vulkan alone cost **55–148 MB** of memory; the CPU renderer cost 5 MB, drew a full 2560×1600 frame within 8.3 ms on 8 threads, and an interactive one in 0.02–0.7 ms with damage tracking. The CPU became the renderer everywhere, and GTK 3 loaded at run time replaced a Wayland and X11 layer of our own: 4847 lines and 15 crates down to 2158 lines and none. nimbo is that toolkit, rebuilt as a library.

| | Crates | Clean build | Binary | Accessibility |
|---|---:|---:|---:|---|
| GPUI | 391 | 56 s | 5.0 MiB | AccessKit |
| Slint | 302 | 42 s | 14.7 MiB | AccessKit |
| Tauri | 204 | 36 s | 8.0 MiB + webview | the webview's |
| egui | 156 | 27 s | 12.0 MiB | AccessKit |
| iced | 140 | 22 s | 9.9 MiB | none yet |
| **nimbo** | **13** | **10 s** | **1.1 MiB** | none yet |

The others: a small todo app, clean release build on an M4 Pro, from the [RCN study](https://github.com/GoldStrikeArch/rust-gui-desktop-ecosystem-state/blob/main/report/10-empirical-results.md) (2026-07-07); nimbo: the `counter` example on an M2, measured here (2026-10-25), crates on macOS. What nimbo doesn't have yet: accessibility, rich text, mobile and web.

## 🤖 How it's made

nimbo is written by AI agents (Claude Code, Codex) following [agents.md](agents.md); I choose what to build and review every pull request. Every visual change is checked in a harness shot, against macOS screenshots at the same scale where the look is meant to match, and a change to a platform's event loop has its frame pacing measured on each backend it touches. This isn't a weekend prompt: I've written software by hand for years — [macmon](https://github.com/vladkens/macmon) (Rust, Apple Silicon system monitor), [twscrape](https://github.com/vladkens/twscrape) (Python, X/Twitter scraping) and [ecloop](https://github.com/vladkens/ecloop) (C, low-level elliptic-curve math) — and agents are how I build now, as with [wavo](https://github.com/vladkens/wavo) (Rust, local speech-to-text on the GPU).

## 📥 Installation

nimbo isn't released yet; take it from git:

```sh
cargo add nimbo --git https://github.com/vladkens/nimbo
```

<details>
<summary>Requirements per OS</summary>

- **macOS** 14 or later: an app sets `MACOSX_DEPLOYMENT_TARGET=14.0`, as `.cargo/config.toml` here does.
- **Windows**: `#![cfg_attr(windows, windows_subsystem = "windows")]` in `main.rs`, so no console opens with the window.
- **Linux**: GTK 3 at run time (every desktop has it), on Wayland or X11. Overlays on Wayland need `libgtk-layer-shell`, the tray icon a panel with StatusNotifierItems, hotkeys and file panels the desktop's portals (GNOME, KDE).

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

`nimbo::run` opens the window; `-- --shot counter.png` renders it to a file without one.

### Examples

```sh
cargo run --release --example gallery                          # every widget, light and dark side by side
cargo run --release --example gallery -- --shot gallery.png    # the same, rendered without a window
```

- `counter`: buttons, hover, focus
- `gallery`: every widget, light and dark
- `settings`: a macOS-style settings window
- `files`: open and save panels, drag and drop
- `lists`: a list of 100,000 rows, menus, tabs, a split
- `fields`: text fields and input methods
- `text`: fonts, scripts, emoji
- `shapes`: what the renderer draws
- `input`: mouse, keys, clipboard
- `tray`: a tray icon, a global hotkey
- `hud`: an overlay over every app

## 📏 Size

Measured on an Apple M2, rustc 1.99, 2026-10-25, without a compiler cache:

- **10.1 s** a clean release build of the `counter` example (8.3 s debug)
- **1.1 MB** its release binary (0.87 MB stripped)
- **13 / 4 / 2** crates in the build on macOS / Windows / Linux, nimbo excluded

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
