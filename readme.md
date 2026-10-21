<p align="center"><img src="docs/brand/header-iris.png" alt="nimbo" width="600"></p>

A small, fast, custom-drawn desktop UI toolkit for Rust: one look on macOS, Windows and Linux, drawn on the CPU by default, over a thin native layer and each system's own text stack.

Work in progress: the toolkit is moving here from its research repository.

It runs on macOS 14 or later; an app built with it sets `MACOSX_DEPLOYMENT_TARGET=14.0`, as this repository does in `.cargo/config.toml`.

On Windows an app built with it sets `#![cfg_attr(windows, windows_subsystem = "windows")]` in its `main.rs`, as the examples do, so that no console opens next to its window.

On Linux it needs GTK 3 at run time (`libgtk-3.so.0`, on every desktop install), which it loads for the window, as Sublime Text does; nothing is linked at build time. It runs on Wayland and X11, and `GDK_BACKEND=x11` picks X11 in a Wayland session.

Run an example with `cargo run --example counter` (a count with buttons on the element tree: hover, press, Tab focus, the theme), `cargo run --example shapes` (every shape the renderer draws), `cargo run --example input` (a square to drag, move with the arrows, resize with the wheel, copy and paste), `cargo run --example lists` (a list of 100,000 rows with selection and keys, a menu, a pop-up button, tabs, a split and a scroll area with fading edges) `cargo run --example fields` (text fields, a password, notes over lines and selectable text, with the system's input methods) or `cargo run --example text` (the system fonts in sizes, weights and scripts, emoji, subpixel positions), or render it without a window: `cargo run --example shapes -- --shot shapes.png`, or with `--appearance light`.

## License

MIT
