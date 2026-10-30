# The updater and the packer moved to shipo

Done: 2026-10-30

The in-place updater (`src/update.rs` and its platform code) and the packer (`scripts/pack.py`) now live in shipo (https://github.com/vladkens/shipo), decided by the person on 2026-10-30. nimbo keeps the update's menu item and window, which no longer depend on an updater.

- [x] `src/update.rs` (check, download, install, finish, supported, dev, `Feed`, `Staged`), `http` and `update` on each OS, `linux/app/process.rs`, their contract in `src/platform/mod.rs`, `examples/update.rs`: removed.
- [x] `scripts/pack.py` and its doctest in `make check`: removed. CI never packed.
- [x] `ed25519-compact` and `objc2-security` removed; `objc2-foundation` without `NSURLSession`, `NSURLRequest`, `NSURLResponse`, `NSDate`, `block2`; `windows-sys` without `Win32_Networking_WinHttp`. Crates 22 → 20 (`objc2-security` was only the updater's; Windows 6 → 5, Linux 4 → 3).
- [x] `update::Release` of nimbo's own (version, notes link, size); `Updater::checked` takes `Result<Option<Release>, String>`, `failed` and `relaunched` an error's text; the notes link shows only with notes.
- [x] `uninstall`'s `app_of` and `remove` back as they were before the updater.
- [x] Budgets: update 800 → 300, app/macos 1900 → 1800, app/windows 1800 → 1700, app/linux 2500 → 2400.
- [x] `agents.md` names the dialog alone.

## Result

`src/update.rs` holds the release type and `src/update/dialog.rs` the window, 275 lines of code between them; an app fills the release from `shipo::Release` and passes shipo's 404, `Feed` and `Signature` errors as no update. The shots of the offer and the download read.
