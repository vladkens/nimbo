# Field keys and the edit menu

Done: 2026-10-26

Two text-field bugs from wavo-app's History search on Windows: Ctrl+V pastes nothing, and a right-click does nothing.

- [x] `keys::Os`: the keyboard conventions (macOS, Windows, Linux), chosen by the platform at build time and kept by the runtime, so the harness checks every OS's keys on any host: `cmd` in bindings, the commands of keys nobody used (AppKit's, Windows', GTK's).
- [x] The PC's text keys in one place for Windows and Linux: Ctrl+C/X/V/A/Z, Ctrl+Y and Ctrl+Shift+Z, Shift+Insert, Ctrl+Insert, Shift+Delete, Ctrl+arrows, Home/End, Ctrl+Home/End, Ctrl+Backspace/Delete; macOS keeps its own.
- [x] The edit menu on a secondary click in a text field and selectable text: Cut, Copy, Paste, Select All, enabled by state; at the pointer; the field keeps its focus look while it is open.
- [x] Tests: every OS's keys in the harness, the menu's items and runs.
- [x] `make check`, `make test`, shots of the menu; PR.

## Result

Bindings and the commands of unused keys follow `keys::Os`, which the runtime keeps (`Os::CURRENT`) and the harness sets (`Harness::set_os`); Windows' and Linux's text keys are one table in `keys.rs` (Linux gained Ctrl+Y and the Insert/Delete keys). Played as Windows in the harness, Ctrl+V pastes into a field and into a search, empty or with text: the binding path was already right there (`cmd` was Ctrl), so the Windows failure lies below the runtime (key translation, the input method or the clipboard) and needs the VM. A right-click opens the edit menu at the pointer (`Ui::menu_popup_at`), its Paste enabled from `Clipboard::has_text`, which macOS and Windows answer without reading the text; the field keeps its ring while the menu has the focus. An open menu now grabs the keys, so Escape closes it before an app's own Escape binding. The ui budget went from 3800 to 3900, the widgets one from 3300 to 3400.
