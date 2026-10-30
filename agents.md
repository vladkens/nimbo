# nimbo

A small, fast, custom-drawn desktop UI toolkit in Rust, in the spirit of Sublime Merge: a UI drawn by the toolkit over a thin native layer and the OS text stack, starting instantly, drawing every frame within vsync, small and fast to build. Agents carry out each step end to end by following this file.

## Scope

- macOS (Apple Silicon) first, then Windows and Linux. macOS 14 is the minimum (`MACOSX_DEPLOYMENT_TARGET` in `.cargo/config.toml`), with no fallbacks for older systems. The CPU renderer draws everywhere. OS code lives only in `src/platform/<os>/`.
- Every crate needs a reason: the `objc2` family on macOS, `windows-sys` on Windows, none on Linux (GTK 3 and the system libraries, loaded at run time), plus a handful of small crates; report the crate count (`cargo tree --prefix none | sort -u | wc -l`) when it changes. Frameworks with hundreds of crates (GPUI, egui, iced) are references to read, not to build on. Change dependencies only with `cargo add` / `cargo remove` / `cargo upgrade`.
- Judge changes on feel, measured where possible: input-to-photon latency, frame time while scrolling and resizing, cold start, memory, text quality (shaping, subpixel positioning, IME), build time.
- The look follows macOS System Settings, measured from screenshots, and must also look right on Windows and Linux.

## Toolkit

- The library starts small and grows when an app needs more: widgets, containers and platforms come as add-ons over the core, not baked into it.
- `src/app.rs` the seams an app sees (`App`, `Event`, `Runtime`, clock, clipboard); `src/instance.rs` one instance and commands from a second launch, `src/paste.rs` a paste into another app that gives the clipboard back, `src/uninstall.rs` undoing what nimbo set up, `src/hardware.rs` how an audio device is attached and the laptop's lid; `src/update/dialog.rs` an update's menu item and window as Sparkle's, over a release the app's updater finds (nimbo neither checks nor installs); `src/keys.rs` key events, bindings and commands; `src/scene.rs` the draw list and paint API; `src/cpu/` the CPU renderer; `src/text/` portable text over the platform's backend; `src/harness.rs` the headless runner.
- `src/ui/` the element tree: layout, paint, input, focus, the per-id store, headless behaviours, the theme and the widgets. Add-ons keep their build-time state in `Ui::slot` and content built in part in `Tree::partial`, so the core never names them.
- `src/ui/a11y.rs` the accessibility model: each behaviour records its role, name, value and states by id while the view builds (widgets refine them, `Ui::a11y_label` names what nothing else does), assembled from the last frame on demand; assistive actions come in as `Event::A11y` through the input paths. `src/platform/macos/a11y.rs` exposes it to VoiceOver and the other AX clients (an `NSAccessibilityElement` per node, the notifications from a diff after each frame); Windows and Linux follow on the same model.
- `Theme` is a plain value: an app edits the default plates look (`Theme::PLATES_LIGHT` / `PLATES_DARK`) or builds its own and sets it with `Ui::set_theme`.
- `src/platform/<os>/` everything OS-dependent, its contract in the docs of `src/platform/mod.rs`: AppKit and CoreText, Win32 and DirectWrite, GTK 3 (on Wayland or X11) with fontconfig, FreeType and HarfBuzz, all loaded at run time on Linux.
- Examples are `examples/<name>.rs` and start through `nimbo::run`, which opens a window, or with `--shot <file.png>` renders in the harness instead (`--appearance light|dark`).

## Code

- Start every `.rs` file with `// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo`.
- A file stays within ~800 lines of code, not counting tests, doc comments, comments and blanks; split it into submodules before it grows past that. `make size` counts them.
- Each part has a code budget (`BUDGETS` in `scripts/size.py`: each platform, `ui/`, its widgets and containers, the accessibility model, `text/` and the rest of the core), checked by `make size ARGS=--check` (and `make check`); raising one is a decision recorded in the PR.
- A statement after an `if`, `for`, `while`, `loop` or `match` block starts after a blank line, as do the other logical steps; rustfmt never adds them, `scripts/blank-lines.py` checks it.
- Every file starts with a `//!` doc whose first sentence says what the file does; `make size ARGS=--map` lists the files by it, and a PR's description carries that list for the files it adds or changes.
- A window's state is reached only through `try_borrow*` or the platform's helper that uses it (macOS `View`, Windows `with_window`, Linux `with`), never `borrow_mut` in a callback: the system calls back in from nested run loops (panels, permission prompts, Finder) while a callback up the stack holds it. Busy, the work is deferred, not dropped: events queue, frames and wakes run again once it is free.
- Every styled widget is a headless behaviour (hover, press, click, focus, keys, the value it edits) plus drawing from the theme, so an app can draw its own look over the same behaviour.
- Keep third-party checkouts in `3rd/` (gitignored), never modify them, and name the source file in a comment when porting from one, keeping its license.
- Helper scripts go in `scripts/`, in kebab-case, in Python run with `uv run`.

## UI rules

- A control's width never changes for the text beside it; notes and descriptions wrap onto their own lines and the row grows.
- Rows in a list keep one height in every state (idle, in progress, failed, selected).
- Every clickable element shows the pointing hand and a hover (the toolbar's tabs only the hand, as System Settings' toolbar); transitions are short (120–170 ms) and animated.
- A mark or icon is optically centred like its neighbours; check alignment against them, not alone.

## Testing

- `make check` (nightly fmt, the blank-line check, clippy with `-D warnings`, and clippy for Windows and Linux) and `make test` pass before a commit; `make prepare` formats, adds the blank lines and applies clippy fixes.
- Tests are ordinary `#[test]` functions, and `Cargo.toml` has no `[[test]]` sections. An example's tests are in `tests/<example>.rs`, one per thing they check: each includes the example with `#[path]`, drives it through the harness and asserts app state and pixels headless (pixel helpers in `tests/common/`); pure logic gets inline unit tests. They run in parallel on libtest's threads, so the harness and what it reaches stay thread-safe. Tests never show a window, a status item or start an event tap; checks that need real OS windows run through `machine-check`.
- `scripts/ax-check.py` (`uv run scripts/ax-check.py [example...]`) checks what the macOS accessibility API sees, in the macOS VM (`~/Code/vm`, `./vm prepare macos --test-host`): it builds the examples and `scripts/ax-dump.swift` (an AX client, in Swift as the API is C and Objective-C) on the host, runs them in the guest, dumps the trees, acts on controls and counts the notifications. A PR that touches the macOS adapter runs it; harness tests assert the model with `Harness::a11y()`.
- Verify every visual change by opening its harness shot with Read, cropped for text detail; compare with the macOS references at the same scale when the look is meant to match.
- A PR that touches a platform's event loop or present path runs `scripts/pace.py` (`make pace`) on each backend it touches: Wayland and X11 (GTK's, `GDK_BACKEND=wayland|x11`), Windows; CI runs it on X11 under Xvfb.
- Real windows on the developer's machine open only with their permission, one example, 1–3 launches, never in a loop.
- Never run Instruments or `xcrun xctrace`: they can freeze the window server.

## Workflow

- For multi-step work keep a checklist in `docs/plans/yyyymmdd-<name>.md`, tick it during the work, and when done add `Done: yyyy-mm-dd`, a short `## Result`, and move it to `docs/plans/completed/`.
- Commit only when the person asks: one short lowercase line, no body, docs in the same commit.
- An orchestrating agent writes each task, verifies every report itself (the code, the numbers, the shots) and keeps the person informed.
- Report a blocker the moment you hit it (a missing permission, tool or source); don't build a degraded version around it.
- Ask the person only when a choice changes the direction or the request is ambiguous; otherwise decide and say so.
- Verify claims against the current source (docs, repository, the code itself), not memory; link the source and date what can go stale.
