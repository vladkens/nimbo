# Layout of wrapping text: the least width once, wrapped lines kept, the second view pass

Done: 2026-10-31

From the redraw research of 2026-10-30 (main at 88536ac): a long document of wrapped text (300 paragraphs, about 30k words) costs 4.8 ms of layout on every frame, about 80% of it finding each text's least width by measuring every grapheme of every word; the rest is wrapping again each frame what didn't change. Scroll and resize frames often run the view and layout twice. Approved: the least width made cheap, wrapped lines kept across frames, the second pass only when needed; no public API change, the code budgets as they are.

- [x] Baseline with the scratch bench (release, harness, medians): the long document at 1440×900@2x and 2560×1440@2x (frame, hover, scroll, live resize, first frame), 8,000 rows without a list, 500 widgets, the examples.
- [x] A text's least width computed once per font and line of text and kept (`text::Memo`, owned by `UiState`), by the same loop as before; tests: the same sizes and lines with and without it, with the fake measure and with the platform's shaper, and no grapheme measured on a second layout.
- [x] Wrapped lines kept per font, line of text and width (and whether it wraps and covers the text), dropped once over a cap of 4096 when a layout didn't use them, as the shaped-line cache does; tests: the same lines at several widths and back, a second layout measures nothing, the cache stays bounded.
- [x] `src/ui/layout.rs` was past ~800 lines of code (834): its text part moved to `src/ui/layout/wrap.rs`.
- [x] The second view pass: counted, in the bench, every frame whose hover moved under the new layout and which ids it changed. Every changed id was one the view had asked about: the hover only lands on interactive elements, and each one's `Response` carries `hovered` (`Ui::interactive`), so no pass can be skipped with the same frame. Left as it is (see Result).
- [x] After: the same bench, the same machine, runs interleaved with the baseline; the pixels of the first frame and of 40 frames of a live resize hashed, before and after.
- [x] `make check`, `make test`, `make size ARGS=--check`; the PR with the numbers and the `--map` lines.

## Result

The long document's steady layout went from 4.8 to 0.21 ms: a frame for a hover, an animation tick or a redraw is 0.5 ms instead of 5.1 at 1440×900@2x, scrolling 1.9 ms instead of 6.6 (4.9 instead of 9.5 at 5K, now mostly raster), and a live resize through widths it hasn't seen 5.4 ms instead of 14.0 (layout 3.6 instead of 11.9). The pixels are the same as before at the first frame and at each of 40 widths, on both sizes; the examples' harness shots are byte-identical. The first frame is unchanged (about 105–115 ms): it is the shaping of each wrapped line, which the lines' widths and ink come from; shaping a paragraph once and breaking it by cluster advances would give widths that can differ in the last bits, so it isn't done here. 8,000 rows without a list and 500 widgets are unchanged: their texts don't wrap. Code: ui 3893 / 3900, text 1174 / 1200, no budget raised.

The second view pass is unchanged: a pass skipped because only `Response` asked about the id would leave an app that reads `Response::hovered` a frame behind, and predicting the hover before the first pass (a scroll moves the last frame's rects) is more code than the ui budget's 7 lines left. With the text kept the second pass is cheap where it ran on text: the document's resize frames lay out twice in 0.47 ms in all.

Medians of 3 interleaved rounds (load average 4.6–7.8 from other work on the machine), Apple M2, ms per frame, the frame's whole CPU time (view, layout, paint, renderer prepare and raster on 4 threads):

| View | Scenario | Before | After |
| --- | --- | --- | --- |
| Document, 1440×900@2x | redraw (hover or animation tick) | 5.10 (layout 4.81) | 0.51 (layout 0.21) |
| | hover | 5.12 | 0.62 |
| | scroll | 6.59 | 1.88 |
| | live resize, 10 widths in turn | 11.53 (p90 18.05) | 2.34 (p90 10.10) |
| | live resize, every width new | 13.96 (layout 11.86) | 5.44 (layout 3.64) |
| | first frame | 114.9 | 111.5 |
| Document, 2560×1440@2x | redraw | 5.36 | 0.87 |
| | hover | 5.45 | 0.94 |
| | scroll | 9.48 | 4.91 |
| | live resize, 10 widths in turn | 14.74 (p90 18.81) | 5.85 (p90 10.65) |
| | live resize, every width new | 15.77 | 7.78 |
| | first frame | 104.1 | 105.5 |
| 8,000 rows without a list, 1440×900@2x | redraw / hover / scroll (layout, 5 rounds) | 3.48 / 3.38 / 6.12 | 3.16 / 3.02 / 6.56 |
| 500 widgets, 1440×900@2x | redraw / scroll / typing | 0.25 / 1.15 / 0.39 | 0.26 / 1.50 / 0.38 |
| settings, gallery, lists, history, text | every scenario, every size | 0.01–2.9, cold 0.5–14.5 | within noise of before |

The rows' and widgets' layout and view times are the same code path before and after (their fastest runs: 2.78 vs 2.80 ms of layout); the differences in their totals are the load on the raster threads.
