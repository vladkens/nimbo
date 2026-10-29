# Build crate

Done: 2026-10-29

Goal: `nimbo-build`, a crate an app's `build.rs` takes as a build-dependency, so the Windows exe's icon, version resource and manifest and nimbo's RGBA window icon come from the app's `[package.metadata.nimbo]` (the config `scripts/pack.py` reads) in one line, and wavo-app drops its own build code.

## Decisions

- `build/` of this repo, a Cargo workspace member (`[workspace] members = ["build"]`), so `make check` and `make test` (now `--workspace`, `fmt --all`) cover it; nimbo's own crate count stays 22. Never in an app's binary.
- Its dependencies are `png`, `ico` and `embed-resource` as wavo-app uses them, and `toml`, which embed-resource already pulls in (it reads `Cargo.toml` itself), so reading the config adds no crate: lighter than `cargo metadata` and a JSON parser.
- `nimbo_build::app()`: `Meta::load()` (`id`, `name`, `icon`, `publisher`, the version from `CARGO_PKG_VERSION`), `window_icon(png, 256)`, `windows_resources(&meta)`. The publisher falls back as pack.py's installer does: the first author, then the id's owner.
- `window_icon` crops to the tile (wavo-app's `cropped()`); `rgba(png, side, file)` keeps the whole image, for wavo-app's about icon.
- Windows: one `.rc` with the `.ico` (16, 24, 32, 48, 64, 256) as icon 1, VERSIONINFO (numbers only, `1.2.0-3-gabc` is 1.2.0.0; CompanyName, FileDescription, ProductName, LegalCopyright, OriginalFilename `<Name>.exe`, as pack.py names it), and the manifest as resource 1 of type 24: `asInvoker`, `supportedOS` Windows 10/11, `PerMonitorV2` with `true/pm`, Common Controls v6. Numbers in place of winver.h's names, as llvm-rc doesn't preprocess.
- nimbo's `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` then fails with ERROR_ACCESS_DENIED, its result ignored, the awareness the same: no conflict.
- Without a resource compiler a debug build (a check of a Windows target on another host) warns and goes on, as wavo-app's `manifest_optional` did; a release build stops rather than ship an exe without its resources, and so does a failed compile.
- wavo-app's git-describe version and about icon stay its own.

## Checklist

- [x] The crate, the workspace, its dependencies (`cargo add -p nimbo-build`)
- [x] `app`, `Meta`, `window_icon`, `rgba`, `windows_resources`, unit tests
- [x] Budget `build` in `scripts/size.py`, `build/` in the blank-line check, the Makefile on the workspace
- [x] A scratch app: host build, `cargo xwin build --target x86_64-pc-windows-msvc`, the PE's resources

## Result

A scratch app with `nimbo_build::app()` built for macOS (`window-icon.rgba` 262144 bytes, 256 × 256 × 4, cropped) and cross-built with cargo-xwin and llvm-rc: `llvm-readobj --coff-resources` and a parse show the icon group (16, 24, 32, 48, 64, 256), VERSIONINFO 1.4.2.0 from `1.4.2-7-gabcdef` with its strings, and the one manifest. The crate has 26 dependencies on a macOS or Linux host, 37 on Windows; its code fits a budget of 200.
