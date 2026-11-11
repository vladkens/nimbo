# Slider steps and a form row's footer

Done: 2026-11-11

- [x] SwiftUI's and AppKit's forms read: `Slider(value:in:step:)` (VoiceOver steps by it), macOS 26's `Slider(…, step:, tick:)` with a `SliderTick(v) { label }` per step and ticks drawn at every step; AppKit's `numberOfTickMarks`, `allowsTickMarkValuesOnly`, `tickMarkPosition`
- [x] AppKit's look measured from an `NSSlider` with tick marks drawn offscreen: 2-point round dots in the tertiary label colour, their bottoms at the knob's, the one under the knob hidden; the track where it was
- [x] `Slider::new(&mut v, range).step(s).tick_label(|v| …).show(ui)`, `ui.slider` the one with no step; `slider_behavior` takes the step
- [x] A press, a click and a drag land on the nearest step, never past the last one (a range the step doesn't divide ends there); the arrows and assistive technology's increment move one, Up raising it as AppKit's slider does
- [x] Steps made only when their dots are drawn (a few dozen at most), a step that isn't finite or positive ignored
- [x] Dots under the track at every step (none closer than 4 points), labels under the steps the closure names, centred on their dots within the slider's width, one that would run into the last left out
- [x] With labels, as much room over the track as under it; in a form, such a slider goes in its row's footer (a form row lays its line out as before, so an app's rows don't move)
- [x] The gallery's table, `docs/api.md` and its picture (`make api-shots`)
- [x] Harness tests: snapping on a drag and a click, the arrows' and increment's steps, the dots and labels in both appearances, the value read
- [x] SwiftUI's forms read: a `Slider` takes the width it is offered; a `VStack` of a toggle and a slider in a grouped `Section` is one row, no separator between them
- [x] The slider grows to the width it is offered, at least 160 points: 160 among a form row's controls (rigid), the dots and tick labels placed along the last frame's width, the view run again when the layout changes it (`Ui::settle`)
- [x] `FormRow::footer(impl Into<Content>)`: what goes under the row's label and controls, across its width, in its piece of the plate, right under the description; a text in the caption's style, dimmed with the row
- [x] The gallery's sliders kept 160 wide in their demos (their pictures unchanged); a "Row footer" block in the table, `docs/api.md` and its picture (`make api-shots`)
- [x] Harness test: a footer slider spans the row from the plate's padding to the switch's right edge, the plate's fill unbroken above it, disabled with its switch

## Result

A stepped slider in SwiftUI's shape, its steps marked as macOS marks them. Skipped: SwiftUI's `minimumValueLabel` and `maximumValueLabel` (an app's texts beside the slider do it), hiding single ticks (`tick:` returning nil), and AppKit's ticks above the track. Tick labels are a `String`, not `impl Into<Content>`: each is placed by its width, measured before the layout, which only a text has, and `Content` borrows its text, so `format!` in the closure would need an owned variant.

A row such as KeepingYouAwake's battery level, a switch with a full-width stepped slider under it, built from `FormRow::new(..).footer(|ui, _| ..)` and a plain `Slider`. Skipped: a general width modifier (SwiftUI's `.frame(width:)`): a fixed-width `ui.col` around the slider does it.
