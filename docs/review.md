# What reviews missed

Bugs that agents' reviews and nimbo's tests missed, each a pattern with the case that showed it. Read it before reviewing or changing an API or a platform's input code; add a pattern when a bug of a new kind turns up.

## One of a thing an app wants several of

An API that holds one of a thing an app could want several of becomes a rewrite later. `Hotkeys::set(&str)` held one hotkey per app, and every backend hard-coded that: macOS one chord and Carbon's id 1, Windows one `RegisterHotKey` and one watched key, X11 one grab, the portal one shortcut id. Its edges did not say which hotkey either. wavo-app's shortcut per language needed all four backends rewritten to places (#90).

Ask of every new API: can an app want two of these at once (hotkeys, tray items, windows, timers, portal sessions)? Then it takes a list or is made per instance, and each callback says which.

## Every press gets its release

An app that starts something on a press (a held recording) stops it on the release: a release lost leaves it running. Check every path that drops, replaces or redirects where releases come from while a key is down.

- X11: the keyboard grab ends when the key whose press began it is released, and the keys held with it send their releases to the focused window. F5 down, F6 down, F5 up: F6's release never came and F6 stayed held (#90).
- Wayland's portal binds every shortcut in one session. Any change closes it and binds in a new one, and the old session's `Deactivated` was dropped as not ours: a held place lost its release when another place changed (#90).
- A held key whose binding changes meanwhile (paused while a recorder records) still ends with its release on the place its press had.

## Changes applied one by one

Applying a list of changes in order can collide with itself (all #90):

- A swap `[a, b]` → `[b, a]` registered place 0's new hotkey while place 1 still held it. Carbon refused the duplicate, `set` returned `Ok`, and one hotkey was gone. Let every old binding go first, then take the new ones.
- A shorter list: the diff walked the old length, and an index into the new list panicked.
- A place left as it was but refused now, because an earlier place took its hotkey: a diff by "changed or not" alone skipped it.

## Work deferred to later

Work deferred to the main loop's idle (the portal's rebind) runs after the events already queued. Until it runs, the old state still answers: a press of a shortcut just removed or paused went to the app. And it may run after what it was for is gone: XWayland's grab, started meanwhile as the fallback, was reset to off by the rebind (#90). Check what answers in between, and that the deferred work still applies when it runs.

## Tested on the platform

A unit test of a state machine proves the machine, not the OS. None of the cases above showed in nimbo's tests until a test was written for it, and a grab's, a tap's or a portal's timing shows only on the real system. A change to a platform's input runs on that platform (`~/Code/vm`) before it is called done, and the PR says what was not run.
