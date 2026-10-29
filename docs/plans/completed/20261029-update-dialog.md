# The update's menu item and window

Done: 2026-10-29

An app that updates in place shows it as Sparkle does: one item in its menu that says what the updater does, and the window it opens, never by itself. wavo-app's own state and submenu (vladkens/wavo-app#42) move here.

- [x] `update::dialog`: `Updater`, the state without the network (the daily check, the person's, the answer, install, later, skip, progress, the restart, a failure, the window closed), with wavo-app's tests.
- [x] `menu_item`: Check for Updates… or what goes on; it opens the window.
- [x] `view`: the app's icon, the offer with Release notes and Check for updates automatically, Skip This Version / Remind Me Later / Install Update; Checking, the download's bar, the restart, up to date, a failure with Retry.
- [x] `examples/updater.rs` in each state, `tests/updater.rs` through the harness; the shots read.
- [x] The update budget 500 → 800.

## Result

`src/update/dialog.rs`, 250 lines of code: a window of 500 × 196 points whose buttons span its width under the icon and the text, as Sparkle's. Checked in the harness in every state, light and dark not compared with Sparkle's at the same scale.
