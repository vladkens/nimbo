# A form row's footer and a slider that fills its width

Done: 2026-11-11

- [x] SwiftUI's forms read: a `Slider` takes the width it is offered; a `VStack` of a toggle and a slider in a grouped `Section` is one row, no separator between them
- [x] The slider grows to the width it is offered, at least 160 points: 160 among a form row's controls (rigid), the dots and tick labels placed along the last frame's width, the view run again when the layout changes it (`Ui::settle`)
- [x] `FormRow::footer(impl Into<Content>)`: what goes under the row's label and controls, across its width, in its piece of the plate, right under the description; a text in the caption's style, dimmed with the row
- [x] The gallery's sliders kept 160 wide in their demos (their pictures unchanged); a "Row footer" block in the table, `docs/api.md` and its picture (`make api-shots`)
- [x] Harness test: a footer slider spans the row from the plate's padding to the switch's right edge, the plate's fill unbroken above it, disabled with its switch

## Result

A row such as KeepingYouAwake's battery level, a switch with a full-width stepped slider under it, built from `FormRow::new(..).footer(|ui, _| ..)` and a plain `Slider`. Skipped: a general width modifier (SwiftUI's `.frame(width:)`): a fixed-width `ui.col` around the slider does it.
