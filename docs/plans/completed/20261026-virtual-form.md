# Virtual form lists

Done: 2026-10-26

wavo-app's History page shows every dictation, 86,000 and more, newest first, a plate a day, in the page's scroll area; today it pages 50 at a time behind "Show More" because every row is laid out and drawn each frame. Asked from wavo-app: only the rows in view (and a margin) built, the rest space of the right height, so the scrollbar, the wheel, paging and `scroll_to` work as for the whole list; ids and hover stable; inside the existing scroll area and form styles.

`List` virtualizes only as its own scroll area. The page needs it inside another one, with section titles and plates' rounded ends scrolling along.

- [x] Content built in part (`Tree::partial`) is a block inside a scroll area, not only the area itself: the runtime places the block in the area's content from the layout before it checks what it covers.
- [x] `Ui::virtual_col`: a column of items of known heights inside the nearest scroll area, building those in view and a margin between two spacers; the block's place from the last frame; a row revealed on request.
- [x] `Ui::form_list`: form sections as such a column: each title (with its note) and each row an item of its own height, a row drawing its piece of the plate (rounded at the section's ends, a hairline above all but the first), so nothing out of view is built; rows keyed by the app for stable ids and hover.
- [x] An example of 86,000 rows a day at a time in the settings look; tests: only the rows in view built, scrolling to the end builds the last ones, the scroll height that of every row, hover and selection kept across scrolling, a revealed row in view.
- [x] The frame's cost with 86,000 rows, from the harness's frame stats.
- [x] `make check`, `make test`; shots.
- [x] Asked by wavo-app meanwhile: a window's fitted size kept until the view is in its window and applied before it shows (macOS); `nimbo::activity` against App Nap and idle sleep.

## Result

`Ui::virtual_col` and `Ui::form_list` build only what a scroll area shows. A form list places its items a section at a time, so a frame costs the sections, not the rows: on an M2 with 500,000 rows, view and layout take 0.41 ms (p50), 0.84 ms with the CPU render at 2x, about 220 nodes; summing every row's height instead took 4.2 ms. Past about 16.7M points of content (≈ 300k rows of 53 points) `f32` coordinates step in 2 points, so the end of such a list scrolls coarsely; lifting that takes `f64` scroll offsets in the layout. The widgets budget went from 3000 to 3200.
