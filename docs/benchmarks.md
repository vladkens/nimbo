# Benchmarks

nimbo against Slint, iced, egui and GPUI, in real windows on an Apple M2, measured on 2026-10-31. Each toolkit is shown with its CPU renderer next to its GPU renderer where it has both. nimbo's GPU rows come from a Metal renderer that is **not released**: it lives on the `feat/metal` branch, behind a `metal` cargo feature and opt-in at run time (`NIMBO_RENDERER=metal`). What ships today is the CPU renderer.

## Setup

- **Machine.** Apple M2 (4 performance and 4 efficiency cores, 10-core GPU), 24 GB, macOS 27.0.1. One external 4K display at 1920×1080 pt, scale 2, 60 Hz (no ProMotion), so nothing here measures 120 Hz pacing.
- **Load.** Nothing else ran but a terminal, Finder, a Docker VM (about 20% of one core) and other agent sessions; the 1-minute load average was 1.2–3.2 across runs (median 2.0–2.3, the machine's own background about 1.5). Before each toolkit the runner waited for the load to drop under 2.5.
- **Sizes.** Window content 1440×900 pt (2880×1800 px); the bunnies were also run once at 1880×930 pt (3760×1860 px).
- **Builds.** Release builds, all made before measuring:
  - nimbo: `feat/metal` at ad9a3f2 (main as of #64 plus the Metal renderer), with the `metal` feature. The same binary draws on the CPU by default or with Metal under `NIMBO_RENDERER=metal`; its CPU path is main's CPU renderer.
  - Slint 1.18.1 (winit): `software`, or `femtovg` on OpenGL.
  - iced 0.14.0: `tiny-skia` (CPU), or `wgpu` 27 on Metal.
  - egui/eframe 0.37.0: `wgpu` 30 on Metal, or `glow` on OpenGL.
  - GPUI from gpui-kit 0.8.1 (Metal only).
- **Scenes**, the same content in every toolkit:
  - bunnies: N sprites of 26×37 pt from a 52×74 RGBA texture, moving with pixi's bunnymark physics;
  - list: 200 rows of 24 pt, a label and a muted size, a hover background;
  - doc: 300 wrapped paragraphs (about 30k words), 13 pt text on 19 pt lines, in a scroll area.

The bench apps and the runner aren't in this repository; they are available on request.

## Method

- **fps and frame interval** over 6 s from the first frame; "steady fps" counts the frames after the first second over their span. Frames are counted from nimbo's presents (`NIMBO_TRACE`), Slint femtovg's rendering notifier, and otherwise each toolkit's per-frame callback.
- **CPU %** is the process's user and system time over wall time (100% is one core). It doesn't include GPU work.
- **Peak MB** is the process's lifetime peak physical footprint.
- **Typical UI**: each scene is driven by the mouse at 60 events/s for 5 s: hover moves one row per event, scroll moves 40 px per event, and resize changes the width through Accessibility in a sawtooth, synchronously, so a slow app completes fewer steps.
- **Cold start**: from `posix_spawn` to the window in the window list, and to the first content pixels.
- **Idle**: the list window left for 4 s, then measured for 10 s.
- **Repetitions**: 3 per cell (cold start 5, the 1880×930 bunnies 1); the tables show medians. The bunnies sweep stops at the first N under 8 fps.

## Bunnies, 1440×900 pt

Steady fps:

| renderer | 100 | 300 | 1 000 | 3 000 | 10 000 | 30 000 | 100 000 |
|---|---:|---:|---:|---:|---:|---:|---:|
| nimbo · CPU | 60 | 60 | 47 | 17 | 5 | – | – |
| nimbo · Metal (feat/metal, not released) | 60 | 60 | 60 | 60 | 60 | 59 | 22 |
| Slint · software | 61 | 53 | 37 | 19 | 10 | 9 | – |
| Slint · femtovg (OpenGL) | 60 | 60 | 60 | 60 | 36 | 12 | 3 |
| iced · tiny-skia (CPU) | 49 | 37 | 20 | 9 | 3 | – | – |
| iced · wgpu (Metal) | 60 | 60 | 60 | 60 | 58 | 32 | 15 |
| egui · wgpu (Metal) | 60 | 60 | 60 | 60 | 60 | 60 | 60 |
| egui · glow (OpenGL) | 60 | 61 | 61 | 119* | 120* | 119* | 61 |
| GPUI · Metal | 60 | 60 | 60 | 60 | 60 | 22 | 5 |

\* egui's glow loop runs unthrottled once the GPU is busy: these are callbacks per second, not presented frames (the display shows at most 60).

Process CPU % / peak footprint MB:

| renderer | 100 | 300 | 1 000 | 3 000 | 10 000 | 30 000 | 100 000 |
|---|---:|---:|---:|---:|---:|---:|---:|
| nimbo · CPU | 50% / 79 | 139% / 79 | 349% / 79 | 378% / 80 | 378% / 81 | – | – |
| nimbo · Metal (feat/metal, not released) | 1% / 91 | 1% / 91 | 1% / 91 | 2% / 92 | 2% / 93 | 5% / 103 | 6% / 140 |
| Slint · software | 98% / 121 | 97% / 121 | 97% / 121 | 97% / 122 | 98% / 126 | 97% / 137 | – |
| Slint · femtovg (OpenGL) | 3% / 174 | 6% / 182 | 16% / 209 | 49% / 288 | 105% / 857 | 105% / 2132 | 101% / 4503 |
| iced · tiny-skia (CPU) | 98% / 124 | 98% / 124 | 98% / 124 | 98% / 125 | 98% / 129 | – | – |
| iced · wgpu (Metal) | 4% / 446 | 4% / 347 | 4% / 210 | 5% / 297 | 8% / 221 | 9% / 258 | 13% / 1109 |
| egui · wgpu (Metal) | 3% / 136 | 3% / 136 | 4% / 127 | 5% / 109 | 10% / 127 | 24% / 152 | 80% / 669 |
| egui · glow (OpenGL) | 2% / 165 | 2% / 165 | 3% / 166 | 8% / 166 | 17% / 173 | 43% / 209 | 75% / 769 |
| GPUI · Metal | 2% / 99 | 2% / 99 | 4% / 102 | 14% / 102 | 68% / 113 | 98% / 141 | 98% / 222 |

nimbo's Metal renderer spends 0.02–0.16 ms a frame encoding, then waits for the GPU on the main thread: 4.1 ms at 100 sprites, 13 ms at 10k, 42 ms at 100k.

At 1880×930 pt the results are close to these (nimbo CPU 43 fps at 1k), except for the software renderers that redraw the whole window: Slint software and iced tiny-skia lose 10–20% at small N, while nimbo's CPU renderer doesn't, because it draws only the damaged tiles.

## Typical UI, 1440×900 pt

Slint software and iced have no per-frame hook in these scenes, so only their CPU and memory are shown.

Hover, list scene:

| renderer | frames/s | interval med / p90 ms | CPU ms/event | CPU % | wakeups/s | footprint MB end / +3 s | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|
| nimbo · CPU | 60 | 16.7 / 16.7 | 0.91 | 6 | 121 | 80 / 80 | 80 |
| nimbo · Metal (feat/metal, not released) | 59 | 16.7 / 17.1 | 1.01 | 6 | 124 | 93 / 46 | 131 |
| Slint · software | – | – | 8.75 | 52 | 61 | 104 / 104 | 124 |
| Slint · femtovg (OpenGL) | 52 | 16.7 / 33.4 | 0.97 | 6 | 122 | 157 / 142 | 159 |
| iced · tiny-skia (CPU) | – | – | 16.66 | 100 | 4 | 108 / 108 | 128 |
| iced · wgpu (Metal) | – | – | 0.65 | 4 | 34 | 81 / 73 | 81 |
| egui · wgpu (Metal) | 60 | 16.7 / 17.1 | 0.81 | 5 | 1 | 90 / 82 | 129 |
| egui · glow (OpenGL) | 66 | 16.7 / 16.8 | 0.65 | 4 | 62 | 161 / 147 | 176 |
| GPUI · Metal | 60 | 16.7 / 17.5 | 2.40 | 14 | 108 | 105 / 98 | 105 |

Scroll, doc scene:

| renderer | frames/s | interval med / p90 ms | CPU ms/event | CPU % | wakeups/s | footprint MB end / +3 s | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|
| nimbo · CPU | 59 | 16.7 / 16.8 | 6.17 | 37 | 121 | 90 / 90 | 90 |
| nimbo · Metal (feat/metal, not released) | 60 | 16.7 / 16.8 | 1.36 | 8 | 121 | 102 / 55 | 122 |
| Slint · software | – | – | 13.31 | 80 | 30 | 110 / 110 | 130 |
| Slint · femtovg (OpenGL) | 60 | 16.7 / 16.8 | 1.73 | 10 | 122 | 175 / 161 | 175 |
| iced · tiny-skia (CPU) | – | – | 16.20 | 97 | 2 | 169 / 169 | 189 |
| iced · wgpu (Metal) | – | – | 0.81 | 5 | 64 | 143 / 135 | 143 |
| egui · wgpu (Metal) | 60 | 16.7 / 17.1 | 1.06 | 6 | 1 | 136 / 128 | 182 |
| egui · glow (OpenGL) | 121* | 6.6 / 13.3 | 1.68 | 10 | 62 | 207 / 193 | 222 |
| GPUI · Metal | 60 | 16.7 / 17.6 | 2.85 | 17 | 109 | 114 / 107 | 114 |

Live resize, doc scene (steps: size changes completed in 5 s, out of 300):

| renderer | steps | frames/s | interval med / p90 ms | CPU ms/step | CPU % | footprint MB end / +3 s | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|
| nimbo · CPU | 300 | 62 | 15.7 / 19.0 | 17.84 | 107 | 104 / 67 | 143 |
| nimbo · Metal (feat/metal, not released) | 291 | 60 | 15.7 / 18.9 | 11.76 | 68 | 136 / 72 | 154 |
| Slint · software | 300 | – | – | 15.01 | 90 | 132 / 132 | 153 |
| Slint · femtovg (OpenGL) | 300 | 60 | 16.8 / 28.0 | 6.71 | 40 | 199 / 165 | 252 |
| iced · tiny-skia (CPU) | 88 | – | – | 55.93 | 98 | 175 / 175 | 196 |
| iced · wgpu (Metal) | 161 | – | – | 8.20 | 27 | 164 / 136 | 204 |
| egui · wgpu (Metal) | 24 | 9 | 29.7 / 36.9 | 31.94 | 15 | 210 / 222 | 237 |
| egui · glow (OpenGL) | 171 | 33 | 32.2 / 35.2 | 26.27 | 86 | 266 / 250 | 346 |
| GPUI · Metal | 300 | 59 | 16.7 / 17.5 | 7.00 | 42 | 113 / 87 | 154 |

## Cold start, list scene

Median (min–max) ms over 5 runs, including process launch and the system's window-open animation:

| renderer | window on screen | first content pixels |
|---|---:|---:|
| nimbo · CPU | 164 (156–172) | 201 (196–214) |
| nimbo · Metal (feat/metal, not released) | 172 (166–185) | 214 (208–225) |
| Slint · software | 198 (184–213) | 234 (230–245) |
| Slint · femtovg (OpenGL) | 257 (253–265) | 299 (296–306) |
| iced · tiny-skia (CPU) | 196 (191–202) | 241 (236–245) |
| iced · wgpu (Metal) | 184 (182–191) | 230 (229–236) |
| egui · wgpu (Metal) | 158 (155–159) | 214 (200–226) |
| egui · glow (OpenGL) | 181 (179–203) | 214 (212–222) |
| GPUI · Metal | 174 (172–181) | 215 (206–217) |

nimbo's own first present comes 83–110 ms after launch; Metal adds 0–10 ms.

## Idle, a still list window

| renderer | CPU % | wakeups/s | footprint MB | resident MB |
|---|---:|---:|---:|---:|
| nimbo · CPU | 0.01 | 0.4 | 40.8 | 107.1 |
| nimbo · Metal (feat/metal, not released) | 0.01 | 0.6 | 45.4 | 91.2 |
| Slint · software | 0.01 | 0.5 | 103.4 | 173.8 |
| Slint · femtovg (OpenGL) | 0.02 | 0.6 | 141.9 | 104.8 |
| iced · tiny-skia (CPU) | 0.02 | 0.4 | 107.8 | 174.1 |
| iced · wgpu (Metal) | 0.02 | 0.7 | 72.8 | 99.7 |
| egui · wgpu (Metal) | 0.02 | 0.6 | 80.9 | 109.9 |
| egui · glow (OpenGL) | 0.01 | 0.6 | 144.7 | 111.4 |
| GPUI · Metal | 0.11 | 60.7 | 77.1 | 100.3 |

Footprint is the number to compare: resident memory counts the shared system frameworks. GPUI keeps its display link running in a still window, hence its 60 wakeups a second.

## Caveats

- The frames come from different sources (presents, a rendering notifier or per-frame callbacks), and Slint software and iced have none in the UI scenes.
- CPU % is per process and hides GPU work. nimbo's CPU renderer draws on 4 threads: 349% for 47 fps at 1k sprites, against Slint software's 97% for 37 fps, so per core Slint's software renderer is more efficient at sprites; nimbo's is faster in wall time and in the UI scenes, where it draws only the damaged tiles.
- Resize is driven through Accessibility, and a slow app throttles the driver (egui wgpu completed 24 resizes in 5 s).
- One machine, one 60 Hz display, a low but not zero load.

## Conclusion

- **nimbo's CPU renderer**, the one that ships, is the fastest software renderer here at UI work: 0.9 ms of CPU per hover event (GPU toolkits 0.65–2.4 ms, Slint software 8.8, iced tiny-skia 16.7) and 6.2 ms per scroll event (the other CPU renderers 13–16). It shows its first pixels first (201 ms) and has the smallest idle footprint (40.8 MB; the GPU toolkits 73–145, the other CPU renderers 103–108). Its weak spot is many moving sprites: 60 fps up to 300, then 47 at 1k, 17 at 3k and 5 at 10k, on 3.5–3.8 cores.
- **The Metal renderer in development** removes that weak spot: 60 fps up to 30k sprites (59) at 1–5% CPU, and less CPU on scroll (6.2 to 1.4 ms per event) and resize (17.8 to 11.8 ms per step). Only egui wgpu goes further (60 fps at 100k, at 80% CPU). Idle, it is the smallest of the GPU renderers here (45 MB). Its cost today is the synchronous wait for the GPU on the main thread.
