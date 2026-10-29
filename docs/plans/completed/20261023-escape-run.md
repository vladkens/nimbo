# Escape capture and run with arguments

Done: 2026-10-23

Two pieces an app on the app platform (#15) still needs: taking Escape from every app for a while, to cancel what the hotkey started, and opening a window or rendering a shot with arguments the app parsed itself.

- [x] `Hotkeys::escape(on)`, `Edge::Escape`, `Status::escape` and `Status::escape_error`.
- [x] macOS: the event tap swallows Escape with its repeats and release (one `owned` rule for the chord and Escape); Carbon registers Escape and ⇧⎋ while on, which also keeps it from the app in front.
- [x] Windows: the low-level hook swallows Escape (`Lone::escape`, `Lone::judge`).
- [x] Linux: the portal can't; `Status::escape_error` says so.
- [x] Tests of the tap's and the hook's decisions, the hotkey thread's dedupe, Linux's status.
- [x] `run_args(cfg, app, script, &args)`, with `run` over it; its doc snippet and a unit test of the shot path.
- [x] PR (#19), reviews: Codex (three fixed), Copilot out of quota.
The Windows hook was not checked on its own in a desktop session: it runs in wavo-app's Windows builds since (Escape cancels a dictation).

## Result

Escape is taken from every app while on (macOS tap and Carbon, the Windows hook; Linux says it can't), and `run_args` opens a window or renders a shot with arguments the app parsed. Merged in #19.
