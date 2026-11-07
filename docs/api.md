# nimbo and SwiftUI

Each building block of nimbo beside its SwiftUI counterpart, as the gallery shows it live (`cargo run --release --example gallery`, a page per block in its variants). A block aims at its SwiftUI counterpart's behaviour and API shape without copying it 1:1. The pictures are the gallery's, in the light appearance: `make api-shots` makes them and this file again from the gallery's table (`examples/gallery.rs`), so a block changes there.

On macOS SwiftUI's `List` has neither of the grouped styles (`InsetGroupedListStyle` and `GroupedListStyle` are iOS's), so there is no name to take: nimbo's `ListStyle::Grouped` is named after the grouped `Form` (`.formStyle(.grouped)`), whose plates its sections share.

## Layout

### Stacks

SwiftUI: `HStack(spacing: 8) { … }; VStack { … }`

nimbo: `ui.row(Style::new().gap(8.0), |ui| { … }); ui.col(style, |ui| { … })`

![Stacks](api/stacks.png)

### Grid

SwiftUI: `Grid { GridRow { … } }`

nimbo: `ui.grid(3, Style::new().gap(8.0), |ui| { … })`

![Grid](api/grid.png)

### Spacer

SwiftUI: `Spacer()`

nimbo: `ui.spacer()`

![Spacer](api/spacer.png)

### Divider

SwiftUI: `Divider()`

nimbo: `ui.divider()`

![Divider](api/divider.png)

### Scroll view

SwiftUI: `ScrollView(.vertical) { … }`

nimbo: `ui.scroll_area(Style::new().h(px(80.0)).scroll(Axes::VERTICAL), |ui| { … })`

![Scroll view](api/scroll-view.png)

### Overlay

SwiftUI: `.overlay(alignment: .topTrailing) { … }`

nimbo: `ui.float(Float::parent().attach(UnitPoint::TOP_TRAILING).anchor(UnitPoint::TOP_TRAILING)).el(style)`

![Overlay](api/overlay.png)

## Text

### Text

SwiftUI: `Text("Title").font(.headline).foregroundStyle(.secondary)`

nimbo: `ui.text("Title", TextStyle::new(t.bold, t.muted))`

![Text](api/text.png)

### Line limit

SwiftUI: `Text(path).lineLimit(1).truncationMode(.middle)`

nimbo: `ui.text(path, t.body().line_limit(1).truncation(TruncationMode::Middle))`

![Line limit](api/line-limit.png)

### Underline

SwiftUI: `Text("WER").underline(pattern: .dot)`

nimbo: `ui.text("WER", s.underline(LinePattern::Dot)); s.strikethrough(LinePattern::Solid)`

![Underline](api/underline.png)

### Selectable text

SwiftUI: `Text(log).textSelection(.enabled)`

nimbo: `ui.selectable(&log)`

![Selectable text](api/selectable-text.png)

### Link

SwiftUI: `Link("Learn more", destination: url)`

nimbo: `if ui.link("Learn more…").clicked { nimbo::open::url(URL)? }`

![Link](api/link.png)

### Image

SwiftUI: `Image(systemName: "gear")`

nimbo: `ui.icon(Icon::GEAR, 20.0, t.text)`

![Image](api/image.png)

## Controls

### Button

SwiftUI: `Button("Save") { … }.buttonStyle(.borderedProminent)`

nimbo: `Button::new("Save").style(ButtonStyle::BorderedProminent).show(ui)`

![Button](api/button.png)

### Button role

SwiftUI: `Button("Delete", role: .destructive) { … }`

nimbo: `Button::new("Delete").role(ButtonRole::Destructive).show(ui)`

![Button role](api/button-role.png)

### Icon button

SwiftUI: `Button { Image(systemName: "trash") }.buttonStyle(.borderless)`

nimbo: `ui.icon_button(Icon::TRASH)`

![Icon button](api/icon-button.png)

### Switch

SwiftUI: `Toggle("Live", isOn: $live)`

nimbo: `ui.toggle(&mut self.live, "Live")`

![Switch](api/switch.png)

### Check box

SwiftUI: `Toggle("Wrap", isOn: $wrap).toggleStyle(.checkbox)`

nimbo: `ui.checkbox(&mut self.wrap, "Wrap")`

![Check box](api/check-box.png)

### Slider

SwiftUI: `Slider(value: $zoom, in: 0...200)`

nimbo: `ui.slider(&mut self.zoom, 0.0..=200.0)`

![Slider](api/slider.png)

### Picker

SwiftUI: `Picker("Size", selection: $size) { …; Divider(); … }`

nimbo: `Picker::new(&mut self.size).options(&A).divider().options(&B).show(ui)`

![Picker](api/picker.png)

### Segmented picker

SwiftUI: `Picker(…) { … }.pickerStyle(.segmented)`

nimbo: `ui.segmented(&mut self.view, &[(0, "List"), (1, "Grid")])`

![Segmented picker](api/segmented-picker.png)

### Text field

SwiftUI: `TextField("Name", text: $name)`

nimbo: `ui.field(&mut self.name, "Name")`

![Text field](api/text-field.png)

### Secure field

SwiftUI: `SecureField("Password", text: $key)`

nimbo: `ui.password_field(&mut self.key, "Password")`

![Secure field](api/secure-field.png)

### Search field

SwiftUI: `.searchable(text: $query)`

nimbo: `ui.search(&mut self.query, "Search")`

![Search field](api/search-field.png)

### Progress

SwiftUI: `ProgressView(value: 0.42)`

nimbo: `ui.progress(Some(0.42))`

![Progress](api/progress.png)

### Spinner

SwiftUI: `ProgressView()`

nimbo: `ui.spinner()`

![Spinner](api/spinner.png)

### Badge

SwiftUI: `.badge("New")`

nimbo: `ui.badge("New", Tone::Accent)`

![Badge](api/badge.png)

### Shortcut recorder

SwiftUI: `KeyboardShortcut (SwiftUI has no recorder)`

nimbo: `ui.shortcut(&mut self.hotkey)`

![Shortcut recorder](api/shortcut-recorder.png)

### Help marks

SwiftUI: `Image(systemName: "info.circle").help(tip); HelpLink { … }`

nimbo: `ui.help(tip).info_button(); ui.help(why).warning_button(); ui.help_button()`

![Help marks](api/help-marks.png)

## Form

### Section

SwiftUI: `Section { … } header: { … } footer: { … }`

nimbo: `ui.form_section(&fs, Section::new(header).footer("…"), |ui| { … })`

![Section](api/section.png)

### Form row

SwiftUI: `LabeledContent { Toggle(…) } label: { Text(title); Text(subtitle) }`

nimbo: `ui.form_row(&fs, FormRow::new("Open at login").caption("…"), |ui| ui.toggle(&mut on, ""))`

![Form row](api/form-row.png)

### Composed label

SwiftUI: `label: { Text(name); Text("New").badge(…) }`

nimbo: `FormRow::new(|ui: &mut Ui, s: TextStyle| { ui.text(name, s); ui.control_size(ControlSize::Small).badge("New", Tone::Accent); })`

![Composed label](api/composed-label.png)

### Row tile

SwiftUI: `Label { Text(title) } icon: { Image(systemName: "folder") }`

nimbo: `FormRow::new("Data").tile(Icon::FOLDER)`

![Row tile](api/row-tile.png)

### Disclosure

SwiftUI: `DisclosureGroup("Other models") { … }`

nimbo: `ui.form_disclosure(&fs, FormRow::new("Other models"), |ui| { … })`

![Disclosure](api/disclosure.png)

### Footer

SwiftUI: `HelpLink { … } at a pane's end`

nimbo: `ui.help("Help").form_footer(&fs, |ui| _ = ui.button("Reset All…"))`

![Footer](api/footer.png)

## List

### List

SwiftUI: `List(items, selection: $sel) { … }`

nimbo: `List::new(n, 24.0).selection(&mut sel).show(ui, |ui, row| { … })`

![List](api/list.png)

### List sections

SwiftUI: `Section(header:) in a List; .listStyle(…)`

nimbo: `List::new(n, 32.0).sections(&starts, 24.0, &head).list_style(ListStyle::Grouped)`

![List sections](api/list-sections.png)

## Containers

### Tabs

SwiftUI: `TabView { … }`

nimbo: `Tabs::new(&mut self.tab, &["One", "Two"]).show(ui, |ui, i| { … })`

![Tabs](api/tabs.png)

### Split

SwiftUI: `HSplitView { … }`

nimbo: `Split::new(Axis::Horizontal).ratio(&mut self.side).show(ui, |ui, pane| { … })`

![Split](api/split.png)

### Menu

SwiftUI: `Menu("Actions") { Button(…) }`

nimbo: `ui.menu("Actions", |ui| { ui.menu_item("Copy"); })`

![Menu](api/menu.png)

### Lazy column

SwiftUI: `LazyVStack { ForEach(…) }`

nimbo: `ui.virtual_col(heights, |i| i, |ui, i| { … })`

![Lazy column](api/lazy-column.png)

## Presentation

### Alert

SwiftUI: `.alert("Delete?", isPresented: $ask) { Button("Delete", role: .destructive) }`

nimbo: `Dialog::new("Delete?").button("Delete", ButtonRole::Destructive).show(ui, &mut self.ask)`

![Alert](api/alert.png)

### Context menu

SwiftUI: `.contextMenu { Button("Copy") { … } }`

nimbo: `ui.context_menu(r.id, || Menu::new(vec![Item::button(Action::Copy, "Copy")]))`

![Context menu](api/context-menu.png)

### Tooltip

SwiftUI: `.help("Saves it")`

nimbo: `ui.help("Saves it").button("Save")`

![Tooltip](api/tooltip.png)

## Modifiers

### Disabled

SwiftUI: `.disabled(true)`

nimbo: `ui.disabled(true).col(Style::new(), |ui| { … })`

![Disabled](api/disabled.png)

### Control size

SwiftUI: `.controlSize(.small)`

nimbo: `ui.control_size(ControlSize::Small).row(Style::new(), |ui| { … })`

![Control size](api/control-size.png)

### Accessibility label

SwiftUI: `.accessibilityLabel("Reset")`

nimbo: `ui.a11y_label("Reset").icon_button(Icon::RESET)`

![Accessibility label](api/accessibility-label.png)

### Keyboard shortcut

SwiftUI: `.keyboardShortcut("z", modifiers: .command)`

nimbo: `ui.keymap(&[bind(Shortcut::char('z').cmd(), Command::Undo)]).col(…)`

![Keyboard shortcut](api/keyboard-shortcut.png)
