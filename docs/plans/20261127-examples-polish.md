# The examples, polished

The owner ran #123's examples in real windows on 2026-11-27 and found them broken or crude. Everything below is to be fixed and checked by the manager itself: harness tests for logic, shots at real size in light and dark looked at closely, and timings measured, not only `make check` and `make test`. The work lands in #123 (branch `examples`): library fixes as their own commits (one unit each, only `src/` plus its tests, the gallery's table and `docs/api.md`), example changes as their own.

## Library

- [ ] Dock: a click on the Dock icon brings back a minimized or closed window when the app set no `on_reopen` (`applicationShouldHandleReopen` answers `false` always, `src/platform/macos/shell.rs`); counter can't be restored after minimizing
- [ ] Selection: a click elsewhere in the window clears a field's or selectable text's selection; the window losing focus keeps it (seen in the gallery's code box)
- [ ] Grid: one `gap(8.0)` gives ~28 px between columns and ~12 px between rows; find why; SwiftUI's `Grid(horizontalSpacing:verticalSpacing:)` takes both
- [ ] The gallery's sidebar list: clicking items ("Button", "Controls") makes the selected row's fill vanish for a few frames every few seconds
- [ ] Text field: pasting a huge text must not stall the app (cost of layout/shaping per frame); check what SwiftUI/AppKit do and do the sensible thing
- [ ] Lazy column of mixed heights: opening a chat of 3,000 messages takes seconds because every row's height is measured before the first frame; estimated heights for rows not yet shown, measured as they show, so a long list opens as fast as a short one; a width change must not re-measure everything
- [ ] Scrolling: the chat doesn't scroll with the mouse wheel (likely the example's hand-made stick-to-bottom; a library scroll anchor at the bottom, SwiftUI's `defaultScrollAnchor(.bottom)`, so examples need no workaround)
- [ ] Styled text: runs of different colours and weights in one paragraph (SwiftUI `AttributedString` / `Text` concatenation), enough for syntax-coloured code in the gallery and a coloured diff in custom-look
- [ ] Emoji: the system's emoji and symbols palette (macOS `orderFrontCharacterPalette:`, Windows Win+. panel) instead of a hand-made menu
- [ ] Dates: a date input (SwiftUI `DatePicker`) so the flight booker isn't free text

## Gallery

- [ ] Each block's code is real, multi-line, formatted Rust that builds what the page shows, syntax-coloured, not a one-liner with `…`; `docs/api.md` follows
- [ ] Stacks: HStack and VStack as two blocks, each showing its own thing
- [ ] Spacer and Divider: show what they do (a spacer pushing items apart, dividers between items in a row and a column)

## 7GUIs

- [ ] Temperature: units as °C and °F, sane number formatting
- [ ] Flight: a real date input; an invalid date (`11.03.12`) is rejected and shown invalid
- [ ] CRUD: the filter and the list make sense to a user (filter by name, as typed); no silent duplicates
- [ ] Cells: a click selects a cell and shows it, arrows move, Return or typing edits, Escape cancels, as in a spreadsheet; double click also edits

## Custom look

- [ ] A copy of Sublime Merge's look: a small git client (commit list, a coloured diff, a file list), dense and dark, its own palette and styles

## Messenger

- [ ] Opening the archive chat is instant (see the lazy column)
- [ ] The chat scrolls with the mouse wheel and trackpad
- [ ] The chat list looks like a modern messenger: rounded selection, sane gaps, no focus-dependent selection colour
- [ ] The sidebar's top (traffic lights, toggle, search) sits right, and the sidebar's width can be dragged
- [ ] The profile panel: proper layout, gaps and shapes
- [ ] Bots and people: some contacts are people, some bots; at least one bot posts by itself now and then, so the app is visibly alive
- [ ] Emoji from the system palette, not a plain menu

## Other

- [ ] `cargo` warns that `package.homepage` is redundant: from the main checkout's Cargo.toml (#122 removes it); nothing to do once #122 is merged
