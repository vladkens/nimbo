<p align="center"><img src="docs/brand/header-iris.png" alt="nimbo" width="600"></p>

A small, fast, custom-drawn desktop UI toolkit for Rust: one look on macOS, Windows and Linux, drawn on the CPU by default, over a thin native layer and each system's own text stack.

Work in progress: the toolkit is moving here from its research repository.

It runs on macOS 14 or later; an app built with it sets `MACOSX_DEPLOYMENT_TARGET=14.0`, as this repository does in `.cargo/config.toml`.

Run an example with `cargo run --example shapes` (every shape the renderer draws) or `cargo run --example input` (a square to drag, move with the arrows, resize with the wheel, copy and paste), or render it without a window: `cargo run --example shapes -- --shot shapes.png`, or with `--appearance light`.

## License

MIT
