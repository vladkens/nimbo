# Several hotkeys

Goal: an app hears several global hotkeys at once, each edge saying which. wavo-app's dictation gets more shortcuts, each with its own language and model (vladkens/wavo-app#73): ⌥Space dictates Russian with gigaam-v3, ⌥⇧Space English with parakeet-v2. The person, 2026-11-23, after a week of the keyboard layout choosing the language: it takes switching the layout all the time; a shortcut per language instead, decided on the result.

Today (main at d3b589b): one hotkey per app. `Hotkeys::set(&str)` parses and arms one `Hotkey`; `Edge` carries no index; macOS keeps one chord in `Shared::chord`, Carbon's `CHORD_ID = 1`; Windows one `RegisterHotKey` and one [Lone] key; X11 one grab; the portal one shortcut `"hotkey"`.

Not in this work: keys taken while a hotkey is held and fn as a chord modifier (#66, closed 2026-11-18: not needed).

## Decisions

- `Hotkeys::set(&mut self, hotkeys: &[Option<Hotkey>]) -> Result<(), String>` replaces `set(&str)`: each place is one hotkey, `None` an empty place (a row the app has with no shortcut recorded yet), so the index stays the app's. The app parses its texts (`Hotkey::parse`) and shows a parse error itself. At most `hotkey::MAX` (8); the rest and a repeat of an earlier one are refused alone, the others taken; the `Err` is the first refusal.
- Idempotent per place: the portable `Hotkeys` hands the platform only what changed, `imp.set(which, Option<Hotkey>)`, so an app calling `set` on every settings change keeps a held hotkey held; a place the OS refused is tried again at the next `set`. `pause` clears every place and arms them again after, as today.
- `Edge::Down { which, at, repeat }`, `Edge::Up { which, at }`, `Edge::Aborted { which }`; `Escape` as is. `which` is the place in `set`.
- `Status::slots: Vec<Slot>`, `Slot { hotkey, error, fallback, fallback_error }`, replace `hotkey`, `error`, `fallback`, `fallback_error`: each place's own (Carbon or `RegisterHotKey` holds it, or why not).
- macOS: `chords`, `down` and `since` become arrays of `MAX` atomics; `judge` runs every modifier-alone place, then the chords, then Escape. One event can tell two edges (right ⌥ alone held, then ⌥Space: `Aborted` of the first, `Down` of the second): `Verdict` carries `[Option<Hit>; 2]`. Escape after a place's release or abort on the same event stays the app's, as today. Carbon registers one hotkey per chord place, id `1 + which`, Escape at `0x100`. The hotkey thread dedupes by edge kind and place, and counts Carbon's repeats per place.
- Windows: `RegisterHotKey(1 + which)` per chord place, each held key polled by one timer; [Lone] watches up to `MAX` keys, one held at a time (a second one goes down with a modifier held, which is no press).
- X11: a grab per chord place; X keeps one grab per key and mask, so ungrabbing one place keeps what another or Escape still holds.
- Wayland: one `BindShortcuts` of every place (`hotkey` for place 0, `hotkey-2`… after), a changed list binds anew in a new session (the desktop may ask again); `Activated` and `Deactivated` by id.

## Tasks

- [x] 1. Portable API: `src/hotkey.rs` (`MAX`, `Edge`, `Slot`, `Status`, `set`, the diff), the contract in `src/platform/mod.rs`, `examples/tray.rs`. Pure tests of the diff, duplicates, `MAX`, the pause.
- [x] 2. macOS: the tap's places and two-edge verdicts, Carbon per place, the hotkey thread. `judge` tests: two chords, a chord and a modifier alone with its modifier (two edges), two modifiers alone, Escape with places; `route`'s dedupe per place.
- [x] 3. Windows: places, one timer, [Lone] over several keys. Tests of [Lone].
- [x] 4. Linux: X11 grabs per place (the `Keys` tests), the portal's shortcuts per place (the id of each, unit-tested).
- [ ] 5. Checks: `make check`, `make test`; wavo-app built against this branch; the VMs (below). 2026-11-23: `make check` (with Windows' and Linux' clippy) passes; `make test` too but `the_api_doc_is_the_gallerys` (photo.png, as on main here); the Windows and Linux tests are built, not run (the hosts were off; CI runs Linux').

## Checks

- Here: `make check`, `make test`, `uv run scripts/lint.py size --check`.
- VMs: macOS (events posted below the tap), Windows 11 and Ubuntu (X11, Wayland) with wavo-app's two shortcuts, `WAVO_FAKE_MIC`, the dictation's model and language read from `history.db`.
