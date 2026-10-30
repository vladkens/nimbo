# The pace check's still window, timed from the first frame

Done: 2026-10-30

`make pace` failed now and then in CI (GTK on X11 under Xvfb) with wakeups while still: fc81899 "60.0 fps (178 frames), 13 wakeups", 6929b48 "56.7 fps (170 frames), 112 wakeups", and before them 5e03723 (29) and 869f546 (49), 4 of the 172 runs with a pace result since 2026-10-21; the same code passed on its other runs.

- [x] CI history: the pace line of every run since 2026-10-21. The failures are spread over unrelated changes (search, the context menus, the updater's removal), none of which touched the Linux event loop or its timers.
- [x] `scripts/pace.py`: the still window began 4.5 s after the launch, which assumed the first frame within 1.5 s (the bar moves for 3 s from it).
- [x] The wakeups are the bar's frames: about 2 context switches a frame on the main thread (122 a second in the Ubuntu VM while it moves; 2.06–2.12 a frame on 18 CI launches, from a second diagnostic branch, also deleted; 0 on every thread once still), so 13–112 wakeups are 0.1–1 s of the animation inside the still window: first frames about 1.6–2.5 s after the launch.
- [x] CI, on a diagnostic branch (deleted afterwards): the first, cold launch on a runner showed its first frame 135–1228 ms after the launch (28 runners), each later launch 33–50 ms (116); once the bar stopped, 0 wakeups on every launch (144) and on every thread. strace of the cold launches: the time is spread over loading the GTK stack, with no wait on D-Bus (no portal or a11y bus on the runners: ServiceUnknown at once).
- [x] Reproduced in the Ubuntu VM by delaying the launch: 1.6 s gives 22 wakeups, 2.5 s 134, 3.7 s 248, the CI failures' wakeups; timed from the first frame each passes. A delay past 3.5 s also cuts the old run short (55.1 fps, 165 frames at 3.7 s), but none of the CI failures went that far: 6929b48's first frame came about 2.5 s after the launch, its run ended after the animation, and its 56.7 fps (170 frames) was the slow runner's own pace, as were 869f546's 58.0 and 5e03723's 58.8.
- [x] `scripts/pace.py` waits for the first frame (15 s at most) and times the still window and the end of the run from it; it prints how long the first frame took, and on a failure the frames presented in the still window; it fails when the app ends before the run does (with its exit code) or its count of switches goes down.

## Result

Not an idle wakeup in nimbo: the check timed the still window from the launch, and a slow cold start left the bar moving in it. It now counts from the first frame. Passes in the Ubuntu VM on Xvfb, GNOME's X11 session and a headless GNOME Shell on Wayland. Not addressed: the 55 fps floor can still flake on a slow runner (6929b48 drew 56.7, 1.7 above it).
