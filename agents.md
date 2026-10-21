# nimbo

A small, fast, custom-drawn desktop UI toolkit in Rust, in the spirit of Sublime Merge: a UI drawn by the toolkit over a thin native layer and the OS text stack, starting instantly, drawing every frame within vsync, small and fast to build. Agents carry out each step end to end by following this file.

## Scope

- macOS (Apple Silicon) first, then Windows and Linux. macOS 14 is the minimum (`MACOSX_DEPLOYMENT_TARGET` in `.cargo/config.toml`), with no fallbacks for older systems. The CPU renderer draws everywhere. OS code lives only in `src/platform/<os>/`.
- Every crate needs a reason: the `objc2` family on macOS, `windows-sys` on Windows, a minimal Wayland client on Linux, plus a handful of small crates; report the crate count (`cargo tree --prefix none | sort -u | wc -l`) when it changes. Frameworks with hundreds of crates (GPUI, egui, iced) are references to read, not to build on. Change dependencies only with `cargo add` / `cargo remove` / `cargo upgrade`.
- Judge changes on feel, measured where possible: input-to-photon latency, frame time while scrolling and resizing, cold start, memory, text quality (shaping, subpixel positioning, IME), build time.
- The look follows macOS System Settings, measured from screenshots, and must also look right on Windows and Linux.

## Toolkit

- The library starts small and grows when an app needs more: widgets, containers and platforms come as add-ons over the core, not baked into it.
- `src/app.rs` the seams an app sees (`App`, `Event`, `Runtime`, clock); `src/scene.rs` the draw list and paint API; `src/cpu/` the CPU renderer; `src/harness.rs` the headless runner.
- `src/platform/<os>/` everything OS-dependent, its contract in the docs of `src/platform/mod.rs`: AppKit, Win32, Wayland or X11 (XCB, loaded at run time).
- Examples are `examples/<name>.rs` and start through `nimbo::run`, which opens a window, or with `--shot <file.png>` renders in the harness instead (`--appearance light|dark`).

## Code

- Start every `.rs` file with `// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo`.
- A file stays within ~800 lines of code, not counting tests, doc comments, comments and blanks; split it into submodules before it grows past that. `make size` counts them.
- A statement after an `if`, `for`, `while`, `loop` or `match` block starts after a blank line, as do the other logical steps; rustfmt never adds them, `scripts/blank-lines.py` checks it.
- Every file starts with a `//!` doc whose first sentence says what the file does; `make size ARGS=--map` lists the files by it, and a PR's description carries that list for the files it adds or changes.
- Every styled widget is a headless behaviour (hover, press, click, focus, keys, the value it edits) plus drawing from the theme, so an app can draw its own look over the same behaviour.
- Keep third-party checkouts in `3rd/` (gitignored), never modify them, and name the source file in a comment when porting from one, keeping its license.
- Helper scripts go in `scripts/`, in kebab-case, in Python run with `uv run`.

## UI rules

- A control's width never changes for the text beside it; notes and descriptions wrap onto their own lines and the row grows.
- Rows in a list keep one height in every state (idle, in progress, failed, selected).
- Every clickable element shows the pointing hand and a hover; transitions are short (120–170 ms) and animated.
- A mark or icon is optically centred like its neighbours; check alignment against them, not alone.

## Testing

- `make check` (nightly fmt, the blank-line check, clippy with `-D warnings`, and clippy for Windows and Linux) and `make test` pass before a commit; `make prepare` formats, adds the blank lines and applies clippy fixes.
- Harness scenarios are `harness = false` test targets in `tests/` that include their example with `#[path]` and assert app state and pixels headless; pure logic gets inline unit tests. Tests never show a window, a status item or start an event tap.
- Verify every visual change by opening its harness shot with Read, cropped for text detail; compare with the macOS references at the same scale when the look is meant to match.
- A PR that touches a platform's event loop or present path runs `scripts/pace.py` (`make pace`) on each backend it touches: Wayland, X11, Windows; CI runs it on X11 under Xvfb.
- Real windows on the developer's machine open only with their permission, one example, 1–3 launches, never in a loop.
- Never run Instruments or `xcrun xctrace`: they can freeze the window server.

## Workflow

- For multi-step work keep a checklist in `docs/plans/yyyymmdd-<name>.md`, tick it during the work, and when done add `Done: yyyy-mm-dd`, a short `## Result`, and move it to `docs/plans/completed/`.
- Commit only when the person asks: one short lowercase line, no body, docs in the same commit.
- An orchestrating agent writes each task, verifies every report itself (the code, the numbers, the shots) and keeps the person informed.
- Report a blocker the moment you hit it (a missing permission, tool or source); don't build a degraded version around it.
- Ask the person only when a choice changes the direction or the request is ambiguous; otherwise decide and say so.
- Verify claims against the current source (docs, repository, the code itself), not memory; link the source and date what can go stale.
