# nimbo

A small custom-drawn desktop UI toolkit in Rust: its own CPU renderer draws every widget over a thin native layer for the window, input and text. The crate's design is in the rustdoc of `src/lib.rs`, the platform contract in the docs of `src/platform/mod.rs`, what each file does in its `//!` doc (`make size ARGS=--map` lists them), the commands in the `Makefile` and each script's docstring, the plans in `docs/plans/`.

## Boundaries

- macOS (Apple Silicon) comes first, then Windows and Linux. macOS 14 is the minimum (`MACOSX_DEPLOYMENT_TARGET` in `.cargo/config.toml`), with no fallbacks for older systems. The CPU renderer draws everywhere.
- Every crate needs a reason: the `objc2` family on macOS, `windows-sys` on Windows, none on Linux (GTK 3 and the system libraries load at run time), plus a handful of small crates. Change dependencies only with `cargo add` / `cargo remove` / `cargo upgrade`, never by hand. When the crate count changes, update the readme's Size line (macOS / Windows / Linux, nimbo excluded), counted per target with `cargo tree -e normal --target <triple> --prefix none --no-dedupe | sort -u | grep -v '^nimbo ' | wc -l` for `aarch64-apple-darwin`, `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu`. Frameworks with hundreds of crates (GPUI, egui, iced) are references to read, not to build on.
- Raising a code budget (`BUDGETS` in `scripts/size.py`) is a decision recorded in the PR.
- Keep third-party checkouts in `3rd/` (gitignored), never modify them, and name the source file in a comment when porting from one, keeping its license.
- Open real windows on the developer's machine only with their permission: one example, 1–3 launches, never in a loop.
- Judge changes on feel, measured where possible: input-to-photon latency, frame time while scrolling and resizing, cold start, memory, text quality (shaping, subpixel positioning, IME), build time.

## Code

- OS-dependent code lives only in `src/platform/<os>/`.
- Widgets and containers are add-ons over the `src/ui/` core: they keep their build-time state in `Ui::slot` and content built in part in `Tree::partial`, never in fields the core adds for them.
- Every styled widget is a headless behaviour (hover, press, click, focus, keys, the value it edits) plus drawing from the theme, so an app can draw its own look over the same behaviour.
- Reach a window's state only through `try_borrow*` or the platform's helper over it (macOS `View`, Windows `with_window`, Linux `with`), never `borrow_mut` in a callback: the system calls back in from nested run loops (panels, permission prompts, Finder). When it is busy, defer the work, never drop it.
- Start every `.rs` file with `// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo`, then a `//!` doc whose first sentence says what the file does; a PR's description carries the `make size ARGS=--map` lines of the files it adds or changes.
- Keep a file within ~800 lines of code (not counting tests, docs, comments and blanks; `make size ARGS=--files` counts them) by splitting it into submodules.
- Helper scripts go in `scripts/`: Python run with `uv run`, kebab-case names.

## UI

- The look follows macOS System Settings, measured from screenshots, and must also look right on Windows and Linux.
- A control's width never changes for the text beside it; notes and descriptions wrap onto their own lines and the row grows.
- Rows in a list keep one height in every state (idle, in progress, failed, selected).
- Every clickable element shows the pointing hand and a hover (the toolbar's tabs only the hand, as in System Settings); transitions are animated, 120–170 ms.
- A mark or icon is optically centred like its neighbours; check its alignment against them, not alone.

## Tests

- Tests are ordinary `#[test]` functions; `Cargo.toml` has no `[[test]]` sections. An example's tests are in `tests/<example>.rs`, one per thing they check: each includes the example with `#[path]`, drives it through the harness and asserts app state and pixels (helpers in `tests/common/`); pure logic gets inline unit tests. They run in parallel on libtest's threads, so the harness and what it reaches stay thread-safe. They never show a window or a status item, nor start an event tap.
- Verify every visual change by looking at its harness shot yourself (`cargo run --example <name> -- --shot <file.png>`, `--appearance light|dark`): open the image, cropped for text detail, and compare it with the macOS references (`docs/settings.png`) at the same scale when the look is meant to match.
- A PR that touches the macOS accessibility adapter runs `scripts/ax-check.py`; harness tests assert the model with `Harness::a11y()`.
- A PR that touches a platform's event loop or present path runs `make pace` on each backend it touches: GTK on Wayland and X11 (`GDK_BACKEND=wayland|x11`), Windows; CI runs it on X11 under Xvfb.

## Workflow

- For multi-step work keep a checklist in `docs/plans/yyyymmdd-<name>.md` and tick it during the work; when done add `Done: yyyy-mm-dd` and a short `## Result`, and move it to `docs/plans/completed/`.
- `make check` (it needs nightly rustfmt and `rustup target add x86_64-pc-windows-msvc x86_64-unknown-linux-gnu`) and `make test` pass before a commit; `make prepare` formats the code, adds the blank lines `make check` asks for and applies clippy's fixes.
- A commit is one short lowercase line, `<area>: <what changed>`, with no body; docs go in the same commit.
