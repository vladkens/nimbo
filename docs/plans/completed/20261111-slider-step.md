# Slider steps with tick marks

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

## Result

A stepped slider in SwiftUI's shape, its steps marked as macOS marks them. Skipped: SwiftUI's `minimumValueLabel` and `maximumValueLabel` (an app's texts beside the slider do it), hiding single ticks (`tick:` returning nil), and AppKit's ticks above the track. Tick labels are a `String`, not `impl Into<Content>`: each is placed by its width, measured before the layout, which only a text has, and `Content` borrows its text, so `format!` in the closure would need an owned variant.
