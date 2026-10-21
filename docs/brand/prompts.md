# How the logo was made

Drawn by Codex (image generation) in two rounds, 2026-10-21: six ideas with two variants each, then the chosen idea in six colours. Each idea ran as its own `codex exec` in a folder of its own, with the brief, the idea and wavo's logo attached as a reference; round 2 resumed the session that drew the chosen idea. The result is `header-iris.png` (README header, 1200×300) and `mark-iris.png` (512×512) in this folder; every other variant and colour is on the `brand-drafts` branch.

## Round 1: the brief

> **nimbo** is a new open-source Rust library: a small, fast, custom-drawn desktop UI toolkit (windows, buttons, lists, charts) that looks the same on macOS, Windows and Linux. It starts instantly, draws every frame in time, and stays tiny. The name comes from "nimbus": a cloud, and a halo (in Spanish and Portuguese *nimbo* is a rain cloud or a halo); it also hints at speed and lightness, like a fast flying broom — but no Harry Potter elements (no brooms, wands, lightning scars, wizard hats).
>
> It is a sibling of **wavo** (a speech engine): the attached reference shows wavo's logo — glossy liquid-glass 3D letters with an iridescent pink → violet → blue → cyan gradient. nimbo may share that premium, modern Apple-like feel, but should have its own identity.
>
> Deliverables: a README header, 1600×400 px PNG with a real transparent background: the mark plus the wordmark "nimbo" (lowercase, rounded, bold, clean), readable on white and on GitHub's dark background (#0d1117); and the mark alone as a square icon, 512×512 PNG, transparent. Generate 2 variants of the idea. Keep it simple and bold: it must still read as a 32 px favicon.

The ideas, one run each:

- **halo-cloud**: a soft, rounded little cloud with a thin glowing halo ring floating just above it.
- **swift-cloud**: a compact cloud moving fast, with two or three smooth speed streaks trailing behind it.
- **window-cloud**: a cloud whose outline is an app window, a rounded window frame with three small dots in its top bar, softened into a cloud silhouette. **The one chosen** (its second variant): a piece of interface in the logo says at once that nimbo is a UI toolkit.
- **halo-o**: wordmark-led, "nimbo" where the final "o" is a glowing halo ring.
- **glass-cloud**: a glossy liquid-glass 3D cloud in the wavo family style.
- **pixel-cloud**: a cloud built from a few crisp rounded tiles, flat and sharp.

## Round 2: the colours

> Keep window-cloud-2's exact concept and shape language (soft glossy 3D, rounded, friendly) and play with colour: sky (sky blue to white), mint (mint/teal to white), dawn (peach to pink), iris (wavo's iridescent pink → violet → blue → cyan, as a sibling brand), graphite (calm graphite/white with one accent colour on the three title-bar dots), indigo (a deeper indigo → violet, more contrast on white). For each, a README header 1600×400 (mark plus the "nimbo" wordmark) and the mark alone 512×512, both PNG with real transparency, readable on white and on #0d1117.

The images were then scaled down and quantized with `pngquant --quality 70-90`.
