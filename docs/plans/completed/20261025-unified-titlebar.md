# Unified title bar

Done: 2026-10-26

A settings window like a native macOS preferences window (`NSWindowToolbarStyle.preference`, Shottr's): the title centred on the line of the traffic lights, the title bar and the tabs one surface with only a hairline under the tabs, the tabs balanced between the title line and the hairline. Asked from wavo-app (screenshot of Wavo over Shottr, macOS 26).

Measured on that screenshot (points): title bar 32 high, lights and title centred in it; the tab tile (48) starts right under it, 8 from the tile to the hairline; the title bold 13 in the tab labels' grey.

- [x] `Config::unified()`: the content runs under the title bar; macOS a full-size content view under a transparent title bar with the title hidden, Windows and Linux keep their title bar (no-op)
- [x] the runtime knows the title bar's height and the window's title (`Ui::titlebar`, `Ui::window_title`); the harness gives a unified window macOS's 32
- [x] `Ui::drags_window` (a prefix, as `Ui::id`: a flag in `Style` grows a node past five cache lines): a press on such an element (none of its interactive children) moves the window, a double-click zooms or minimizes as the user's setting says
- [x] `toolbar_tabs`: in a unified window the title line on top, the tabs right under it, the header drags the window; in any other the tabs balanced (8 over, 8 under)
- [x] the settings example unified; tests for the title line, the drag area and the balance
- [x] `make check`, `make test`, `make size ARGS=--check` (budgets raised: macOS 1730 → 1755, ui 3700 → 3725, widgets 2940 → 2965, core 3490 → 3505); shots light and dark
- [x] one real launch of `settings` (the owner's): the title on the lights' line, the header one with the tabs
- [x] from that launch: a row's controls centred on the row, not its first line (as the tile); the ↗ icon as large as its neighbours (it took a third of its box); 20 under the page, not 10

## Result

`Config::unified()` gives a settings window the look of AppKit's preferences window on macOS: the content under a transparent title bar, `toolbar_tabs` drawing the title centred in it with the tabs right under it, the header dragging the window (`Ui::drags_window`, a double-click as the system says). Windows and Linux keep their title bar; there the tabs sit 8 under it and 8 over the hairline. A row's controls centre on the row, the ↗ icon is as large as its neighbours, and a page ends 20 over the window's edge. Budgets raised: macOS +25, ui +25, widgets +25, core +15.
