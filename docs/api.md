# nimbo and SwiftUI

Each building block of nimbo beside its SwiftUI counterpart, as the gallery shows it live (`cargo run --release --example gallery`, a page per block in its variants). A block aims at its SwiftUI counterpart's behaviour and API shape without copying it 1:1. The pictures are the gallery's, in the light appearance, kept on the repo's `assets` branch: `make api-shots` makes them and this file again from the gallery's table (`examples/gallery.rs`), so a block changes there.

On macOS SwiftUI's `List` has neither of the grouped styles (`InsetGroupedListStyle` and `GroupedListStyle` are iOS's), so there is no name to take: nimbo's `ListStyle::Grouped` is named after the grouped `Form` (`.formStyle(.grouped)`), whose plates its sections share.

## Layout

### Stacks

SwiftUI: `HStack(spacing: 8) { … }; VStack { … }`

nimbo: `ui.row(Style::new().gap(8.0), |ui| { … }); ui.col(style, |ui| { … })`

![Stacks](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/stacks.png)

### Grid

SwiftUI: `Grid { GridRow { … } }`

nimbo: `ui.grid(3, Style::new().gap(8.0), |ui| { … })`

![Grid](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/grid.png)

### Spacer

SwiftUI: `Spacer()`

nimbo: `ui.spacer()`

![Spacer](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/spacer.png)

### Divider

SwiftUI: `Divider()`

nimbo: `ui.divider()`

![Divider](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/divider.png)

### Scroll view

SwiftUI: `ScrollView(.vertical) { … }`

nimbo: `ui.scroll_area(Style::new().h(px(80.0)).scroll(Axes::VERTICAL), |ui| { … })`

![Scroll view](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/scroll-view.png)

### Overlay

SwiftUI: `.overlay(alignment: .topTrailing) { … }`

nimbo: `ui.float(Float::parent().attach(UnitPoint::TOP_TRAILING).anchor(UnitPoint::TOP_TRAILING)).el(style)`

![Overlay](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/overlay.png)

## Text

### Text

SwiftUI: `Text("Title").font(.headline).foregroundStyle(.secondary)`

nimbo: `ui.text("Title", TextStyle::new(t.bold, t.muted))`

![Text](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/text.png)

### Line limit

SwiftUI: `Text(path).lineLimit(1).truncationMode(.middle)`

nimbo: `ui.text(path, t.body().line_limit(1).truncation(TruncationMode::Middle))`

![Line limit](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/line-limit.png)

### Underline

SwiftUI: `Text("WER").underline(pattern: .dot)`

nimbo: `ui.text("WER", s.underline(LinePattern::Dot)); s.strikethrough(LinePattern::Solid)`

![Underline](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/underline.png)

### Selectable text

SwiftUI: `Text(log).textSelection(.enabled)`

nimbo: `ui.selectable(&log)`

![Selectable text](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/selectable-text.png)

### Link

SwiftUI: `Link("Learn more", destination: url)`

nimbo: `if ui.link("Learn more…").clicked { nimbo::open::url(URL)? }`

![Link](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/link.png)

### Image

SwiftUI: `Image(systemName: "gear")`

nimbo: `ui.icon(Icon::GEAR, 20.0, t.text)`

![Image](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/image.png)

## Controls

### Button

SwiftUI: `Button("Save") { … }.buttonStyle(.borderedProminent)`

nimbo: `Button::new("Save").style(ButtonStyle::BorderedProminent).show(ui)`

![Button](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/button.png)

### Button role

SwiftUI: `Button("Delete", role: .destructive) { … }`

nimbo: `Button::new("Delete").role(ButtonRole::Destructive).show(ui)`

![Button role](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/button-role.png)

### Icon button

SwiftUI: `Button { Image(systemName: "trash") }.buttonStyle(.borderless)`

nimbo: `ui.icon_button(Icon::TRASH)`

![Icon button](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/icon-button.png)

### Switch

SwiftUI: `Toggle("Live", isOn: $live)`

nimbo: `ui.toggle(&mut self.live, "Live")`

![Switch](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/switch.png)

### Check box

SwiftUI: `Toggle("Wrap", isOn: $wrap).toggleStyle(.checkbox)`

nimbo: `ui.checkbox(&mut self.wrap, "Wrap")`

![Check box](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/check-box.png)

### Slider

SwiftUI: `Slider(value: $zoom, in: 0...200)`

nimbo: `ui.slider(&mut self.zoom, 0.0..=200.0)`

![Slider](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/slider.png)

### Slider steps

SwiftUI: `Slider(value: $level, in: 10...90, step: 10) { … } tick: { v in SliderTick(v) { … } }`

nimbo: `Slider::new(&mut self.level, 10.0..=90.0).step(10.0).tick_label(|v| (v == 50.0).then(|| "50%".into())).show(ui)`

![Slider steps](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/slider-steps.png)

### Picker

SwiftUI: `Picker("Size", selection: $size) { …; Divider(); … }`

nimbo: `Picker::new(&mut self.size).options(&A).divider().options(&B).show(ui)`

![Picker](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/picker.png)

### Segmented picker

SwiftUI: `Picker(…) { … }.pickerStyle(.segmented)`

nimbo: `ui.segmented(&mut self.view, &[Tag::new(0, "List").count(128), Tag::new(1, "Grid")])`

![Segmented picker](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/segmented-picker.png)

### Text field

SwiftUI: `TextField("Name", text: $name)`

nimbo: `ui.field(&mut self.name, "Name")`

![Text field](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/text-field.png)

### Secure field

SwiftUI: `SecureField("Password", text: $key)`

nimbo: `ui.password_field(&mut self.key, "Password")`

![Secure field](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/secure-field.png)

### Search field

SwiftUI: `.searchable(text: $query)`

nimbo: `ui.search(&mut self.query, "Search")`

![Search field](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/search-field.png)

### Progress

SwiftUI: `ProgressView(value: 0.42)`

nimbo: `ui.progress(Some(0.42))`

![Progress](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/progress.png)

### Spinner

SwiftUI: `ProgressView()`

nimbo: `ui.spinner()`

![Spinner](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/spinner.png)

### Badge

SwiftUI: `.badge("New")`

nimbo: `ui.badge("New", Tone::Accent)`

![Badge](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/badge.png)

### Shortcut recorder

SwiftUI: `KeyboardShortcut (SwiftUI has no recorder)`

nimbo: `ui.shortcut(&mut self.hotkey)`

![Shortcut recorder](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/shortcut-recorder.png)

### Help marks

SwiftUI: `Image(systemName: "info.circle").help(tip); HelpLink { … }`

nimbo: `ui.help(tip).info_button(); ui.help(why).warning_button(); ui.help_button()`

![Help marks](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/help-marks.png)

## Form

### Section

SwiftUI: `Section { … } header: { … } footer: { … }`

nimbo: `ui.form_section(&fs, Section::new(header).footer("…"), |ui| { … })`

![Section](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/section.png)

### Form row

SwiftUI: `LabeledContent { Toggle(…) } label: { Text(title); Text(subtitle) }`

nimbo: `ui.form_row(&fs, FormRow::new("Open at login").caption("…"), |ui| ui.toggle(&mut on, ""))`

![Form row](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/form-row.png)

### Composed label

SwiftUI: `label: { Text(name); Text("New").badge(…) }`

nimbo: `FormRow::new(|ui: &mut Ui, s: TextStyle| { ui.text(name, s); ui.control_size(ControlSize::Small).badge("New", Tone::Accent); })`

![Composed label](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/composed-label.png)

### Row tile

SwiftUI: `Label { Text(title) } icon: { Image(systemName: "folder") }`

nimbo: `FormRow::new("Data").tile(Icon::FOLDER)`

![Row tile](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/row-tile.png)

### Disclosure

SwiftUI: `DisclosureGroup("Other models") { … }`

nimbo: `ui.form_disclosure(&fs, FormRow::new("Other models"), |ui| { … })`

![Disclosure](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/disclosure.png)

### Footer

SwiftUI: `HelpLink { … } at a pane's end`

nimbo: `ui.help("Help").form_footer(&fs, |ui| _ = ui.button("Reset All…"))`

![Footer](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/footer.png)

## List

### List

SwiftUI: `List(items, selection: $sel) { … }`

nimbo: `List::new(n, 24.0).selection(&mut sel).show(ui, |ui, row| { … })`

![List](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/list.png)

### List sections

SwiftUI: `Section(header:) in a List; .listStyle(…)`

nimbo: `List::new(n, 32.0).sections(&starts, 24.0, &head).list_style(ListStyle::Grouped)`

![List sections](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/list-sections.png)

## Containers

### Tabs

SwiftUI: `TabView { … }`

nimbo: `Tabs::new(&mut self.tab, &["One", "Two"]).show(ui, |ui, i| { … })`

![Tabs](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/tabs.png)

### Split

SwiftUI: `HSplitView { … }`

nimbo: `Split::new(Axis::Horizontal).ratio(&mut self.side).show(ui, |ui, pane| { … })`

![Split](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/split.png)

### Menu

SwiftUI: `Menu("Actions") { Button(…) }`

nimbo: `ui.menu("Actions", |ui| { ui.menu_item("Copy"); })`

![Menu](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/menu.png)

### Lazy column

SwiftUI: `LazyVStack { ForEach(…) }`

nimbo: `ui.virtual_col(heights, |i| i, |ui, i| { … })`

![Lazy column](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/lazy-column.png)

## Presentation

### Alert

SwiftUI: `.alert("Delete?", isPresented: $ask) { Button("Delete", role: .destructive) }`

nimbo: `Dialog::new("Delete?").button("Delete", ButtonRole::Destructive).show(ui, &mut self.ask)`

![Alert](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/alert.png)

### Context menu

SwiftUI: `.contextMenu { Button("Copy") { … } }`

nimbo: `ui.context_menu(r.id, || Menu::new(vec![Item::button(Action::Copy, "Copy")]))`

![Context menu](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/context-menu.png)

### Tooltip

SwiftUI: `.help("Saves it")`

nimbo: `ui.help("Saves it").button("Save")`

![Tooltip](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/tooltip.png)

## Modifiers

### Disabled

SwiftUI: `.disabled(true)`

nimbo: `ui.disabled(true).col(Style::new(), |ui| { … })`

![Disabled](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/disabled.png)

### Control size

SwiftUI: `.controlSize(.small)`

nimbo: `ui.control_size(ControlSize::Small).row(Style::new(), |ui| { … })`

![Control size](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/control-size.png)

### Accessibility label

SwiftUI: `.accessibilityLabel("Reset")`

nimbo: `ui.a11y_label("Reset").icon_button(Icon::RESET)`

![Accessibility label](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/accessibility-label.png)

### Keyboard shortcut

SwiftUI: `.keyboardShortcut("z", modifiers: .command)`

nimbo: `ui.keymap(&[bind(Shortcut::char('z').cmd(), Command::Undo)]).col(…)`

![Keyboard shortcut](https://raw.githubusercontent.com/vladkens/nimbo/assets/api/keyboard-shortcut.png)
