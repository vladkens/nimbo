# Escape capture and run with arguments

Two pieces an app on the app platform (#15) still needs: taking Escape from every app for a while, to cancel what the hotkey started, and opening a window or rendering a shot with arguments the app parsed itself.

- [x] `Hotkeys::escape(on)`, `Edge::Escape`, `Status::escape` and `Status::escape_error`.
- [x] macOS: the event tap swallows Escape with its repeats and release (one `owned` rule for the chord and Escape); Carbon registers Escape and ⇧⎋ while on, which also keeps it from the app in front.
- [x] Windows: the low-level hook swallows Escape (`Lone::escape`, `Lone::judge`).
- [x] Linux: the portal can't; `Status::escape_error` says so.
- [x] Tests of the tap's and the hook's decisions, the hotkey thread's dedupe, Linux's status.
- [x] `run_args(cfg, app, script, &args)`, with `run` over it; its doc snippet and a unit test of the shot path.
- [ ] PR, reviews.
- [ ] Checks: the Windows hook in a desktop session.

