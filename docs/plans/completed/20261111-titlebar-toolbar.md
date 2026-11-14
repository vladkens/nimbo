# Toolbar in the title bar

Done: 2026-11-11

A sidebar toggle in the title bar as Finder's, Notes' and Messages': right of the traffic lights, on their middle, there with the sidebar shown or hidden, and other items at the trailing end, in SwiftUI's shape (`.toolbar { ToolbarItem(placement: .navigation) { … } }`). The messenger demo had put its toggle in the sidebar's content, under and off the lights.

Measured with AppKit's own toolbar on macOS 27 (a reference app on another Mac, frames from `standardWindowButton` and the toolbar's views): a unified toolbar makes the title bar 52 high, the lights 14 wide, 9 apart, 19 from the top-left corner, their middle at 26; the sidebar toggle's tile starts 17 after the zoom button (96), the next item 8 after it, the trailing ones end 8 from the edge; in full screen the toolbar sits in a window of its own, the first item 10 in. With an empty toolbar every press in the bar reaches the content view.

- [x] `TitleBar` (height, title, the window's buttons) for `Ui::titlebar`, read from AppKit before each frame (the lights' frames, none in full screen)
- [x] `Ui::toolbar` and `Ui::toolbar_item` with `ToolbarItemPlacement::{Navigation, PrimaryAction}`, `Ui::toolbar_height`: a band over the top of the parent, leading items 17 right of the buttons on their middle, trailing ones 8 from the end, Large controls; under a title bar the empty band moves the window
- [x] macOS: an empty unified `NSToolbar` (icon only, no separator) while the view builds a toolbar, hidden in full screen; the first frame opens with it
- [x] Windows and Linux keep their title bar: the band is the content's top row, items 10 and 8 from its ends, moving nothing
- [x] the harness plays macOS's bars (32, or 52 with a toolbar, and the lights)
- [x] `Icon::SIDEBAR`; the gallery's Toolbar block; `examples/notes.rs` with its tests
- [x] real windows on another Mac (macOS 27): light, dark, sidebar hidden, full screen, a click on the toggle, a press and a double-click on the empty bar
- [x] from review: the band moves the window only beside the buttons (not in full screen); the lights watched (`NSViewFrameDidChangeNotification` on the zoom button) so a layout after the frame is drawn; a failed full screen shows the toolbar again; a toolbar identifier per window; `TitleBar` non-exhaustive; the notes sidebar slides in 150 ms
- [x] `make check` (but the budgets), `make test`

## Result

`ui.toolbar(|ui| ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| …))` puts ordinary elements in the title bar of a unified window. On macOS 27 the toggle's glyph sits on the traffic lights' middle (26) and its tile 17 after them, as Notes' does, in light and dark, with the sidebar shown or hidden; in full screen it moves to 10 from the edge; a press on the empty bar moves the window and a double-click zooms it as the system says. Windows and Linux keep their own title bar, the toolbar being the top row under it. The macOS VM couldn't start (this Mac denies the agent's Apple events to System Events), so the windows ran on another Mac over SSH, captured from inside the app.
