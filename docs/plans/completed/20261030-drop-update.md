# The updater and the packer moved to shipo, the window to the app

Done: 2026-10-30

The in-place updater (`src/update.rs` and its platform code) and the packer (`scripts/pack.py`) now live in shipo (https://github.com/vladkens/shipo), and the update's menu item and window (Sparkle's) in wavo-app, decided by the person on 2026-10-30: nimbo knows nothing of releases or updates. An app builds that window from nimbo's general UI API (`Main::open` with a `Config`, `Ui` and its widgets, `tray::Item`, `Png`).

- [x] `src/update.rs` (check, download, install, finish, supported, dev, `Feed`, `Staged`), `http` and `update` on each OS, `linux/app/process.rs`, their contract in `src/platform/mod.rs`, `examples/update.rs`: removed.
- [x] `scripts/pack.py` and its doctest in `make check`: removed. CI never packed.
- [x] `ed25519-compact` and `objc2-security` removed; `objc2-foundation` without `NSURLSession`, `NSURLRequest`, `NSURLResponse`, `NSDate`, `block2`; `windows-sys` without `Win32_Networking_WinHttp`. Crates per target, nimbo excluded (the count in `agents.md`): 15 → 13 on macOS (`objc2-security` was only the updater's), 5 → 4 on Windows, 3 → 2 on Linux.
- [x] `uninstall` as it was before the updater: `app_of`, the `Env` type and `retry` private again (`remove` was always public).
- [x] The window moved to wavo-app: `src/update.rs` (`Release`), `src/update/dialog.rs`, `examples/updater.rs`, `tests/updater.rs` removed, and `pub mod update` from `lib.rs`.
- [x] Every API the window used stays: compiled as code of another crate against `nimbo::` alone, it builds and its tests pass, so nothing had to be made public.
- [x] Budgets: update 800 → gone, app/macos 1900 → 1800, app/windows 1800 → 1700, app/linux 2500 → 2400.
- [x] `agents.md` no longer names an update.

## Result

nimbo has no `update` module and no word of releases; wavo-app keeps the window over shipo, and moves its `pack.py` use and `[package.metadata.nimbo]` to shipo's (`[package.metadata.shipo]`).
