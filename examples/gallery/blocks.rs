// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The gallery's table: every building block by group, with SwiftUI's line and nimbo's; the
//! sidebar, the pages and `docs/api.md` are made from it.

/// A building block: its group, name, SwiftUI's line and nimbo's.
pub struct Block {
  pub group: &'static str,
  pub name: &'static str,
  pub swiftui: &'static str,
  pub nimbo: &'static str,
}

impl Block {
  /// Its name in lowercase with dashes: its picture's file name and its argument.
  pub fn slug(&self) -> String {
    let s: String = self.name.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    s.to_lowercase().split_whitespace().collect::<Vec<_>>().join("-")
  }
}

const fn b(
  group: &'static str,
  name: &'static str,
  swiftui: &'static str,
  nimbo: &'static str,
) -> Block {
  Block { group, name, swiftui, nimbo }
}

pub const GROUPS: [&str; 8] =
  ["Layout", "Text", "Controls", "Form", "List", "Containers", "Presentation", "Modifiers"];

#[rustfmt::skip]
pub const BLOCKS: &[Block] = &[
  b("Layout", "Stacks", "HStack(spacing: 8) { … }; VStack { … }", "ui.row(Style::new().gap(8.0), |ui| { … }); ui.col(style, |ui| { … })"),
  b("Layout", "Grid", "Grid { GridRow { … } }", "ui.grid(3, Style::new().gap(8.0), |ui| { … })"),
  b("Layout", "Spacer", "Spacer()", "ui.spacer()"),
  b("Layout", "Divider", "Divider()", "ui.divider()"),
  b("Layout", "Scroll view", "ScrollView(.vertical) { … }", "ui.scroll_area(Style::new().h(px(80.0)).scroll(Axes::VERTICAL), |ui| { … })"),
  b("Layout", "Overlay", ".overlay(alignment: .topTrailing) { … }", "ui.float(Float::parent().attach(UnitPoint::TOP_TRAILING).anchor(UnitPoint::TOP_TRAILING)).el(style)"),
  b("Text", "Text", "Text(\"Title\").font(.headline).foregroundStyle(.secondary)", "ui.text(\"Title\", TextStyle::new(t.bold(), t.secondary()))"),
  b("Text", "Localized text", "Text(\"Keep the history\"); String(localized: \"\\(name) is not on disk\")", "ui.text(t!(\"Keep the history\"), s); t!(\"{name} is not on disk\", name); tn!(\"{n} file\", \"{n} files\", count)"),
  b("Text", "Line limit", "Text(path).lineLimit(1).truncationMode(.middle)", "ui.text(path, t.body().line_limit(1).truncation(TruncationMode::Middle))"),
  b("Text", "Underline", "Text(\"WER\").underline(pattern: .dot)", "ui.text(\"WER\", s.underline(LinePattern::Dot)); s.strikethrough(LinePattern::Solid)"),
  b("Text", "Selectable text", "Text(log).textSelection(.enabled)", "ui.selectable_text(&log, t.body())"),
  b("Text", "Link", "Link(\"Learn more\", destination: url)", "if ui.link(\"Learn more…\").clicked { nimbo::open::url(URL)? }"),
  b("Text", "Image", "Image(systemName: \"gear\")", "ui.icon(Icon::GEAR, 20.0, t.text())"),
  b("Text", "Label content", "Label { Text(title) } icon: { Image(systemName: \"folder\") }; configuration.label in a style", "let label: Content = Icon::FOLDER.into(); label.build(ui.with_id(id.key(\"label\")), t.label())"),
  b("Text", "Photo", "Image(nsImage: NSImage(data: file)!).resizable().aspectRatio(contentMode: .fill).frame(width: 80, height: 80).clipped()", "let photo = Bitmap::decode(&file, 240)?; ui.aspect_ratio(ContentMode::Fill).image(&photo, Style::new().w(px(80.0)).h(px(80.0)))"),
  b("Controls", "Button", "Button(\"Save\") { … }.buttonStyle(.borderedProminent)", "ui.button_style(&BorderedProminentButtonStyle).button(\"Save\")"),
  b("Controls", "Button role", "Button(\"Delete\", role: .destructive) { … }", "Button::new(\"Delete\").role(ButtonRole::Destructive).show(ui)"),
  b("Controls", "Icon button", "Button { Image(systemName: \"trash\") }.buttonStyle(.borderless)", "ui.icon_button(Icon::TRASH)"),
  b("Controls", "Button style", "HStack { Button(\"Cut\") { … }; Button(\"Copy\") { … } }.buttonStyle(.borderless)", "ui.button_style(&BorderlessButtonStyle).row(style, |ui| { ui.button(\"Cut\"); ui.button(\"Copy\"); }); ui.button(\"Paste\")"),
  b("Controls", "Custom button style", "struct Pill: ButtonStyle { func makeBody(configuration: Configuration) -> some View { configuration.label.padding(…).background(…) } }", "impl ButtonStyle for Pill { fn make_body(&self, ui: &mut Ui, mut c: ButtonConfiguration) { ui.row(style, |ui| c.label(ui, s)); } } ui.button_style(&Pill).button(\"Pill\")"),
  b("Controls", "Switch", "Toggle(\"Live\", isOn: $live)", "ui.toggle(&mut self.live, \"Live\")"),
  b("Controls", "Check box", "Toggle(\"Wrap\", isOn: $wrap).toggleStyle(.checkbox)", "ui.toggle_style(&CheckboxToggleStyle).toggle(&mut self.wrap, \"Wrap\")"),
  b("Controls", "Button toggle", "Toggle(\"Bold\", isOn: $bold).toggleStyle(.button)", "ui.toggle_style(&ButtonToggleStyle).toggle(&mut self.bold, \"Bold\")"),
  b("Controls", "Toggle style", "VStack { Toggle(\"Wrap\", …); Toggle(\"Numbers\", …) }.toggleStyle(.checkbox)", "ui.toggle_style(&CheckboxToggleStyle).col(style, |ui| { ui.toggle(&mut self.wrap, \"Wrap\"); ui.toggle(&mut self.numbers, \"Numbers\"); }); ui.toggle(&mut self.live, \"Live\")"),
  b("Controls", "Custom toggle style", "struct Dot: ToggleStyle { func makeBody(configuration: Configuration) -> some View { HStack { Circle()…; configuration.label } } }", "impl ToggleStyle for Dot { fn make_body(&self, ui: &mut Ui, mut c: ToggleConfiguration) { ui.row(style, |ui| { ui.el(dot(c.is_on())); c.label(ui, s); }); } } ui.toggle_style(&Dot).toggle(&mut self.live, \"Live\"); ui.toggle_style(&PlainToggleStyle).toggle(&mut self.wrap, \"Plain\")"),
  b("Controls", "Slider", "Slider(value: $zoom, in: 0...200)", "ui.slider(&mut self.zoom, 0.0..=200.0)"),
  b("Controls", "Slider steps", "Slider(value: $level, in: 10...90, step: 10) { … } tick: { v in SliderTick(v) { … } }", "Slider::new(&mut self.level, 10.0..=90.0).step(10.0).tick_label(|v| (v == 50.0).then(|| \"50%\".into())).show(ui)"),
  b("Controls", "Slider style", "Slider(value: $zoom, in: 0...200) (SwiftUI has no slider style)", "ui.slider_style(&PlainSliderStyle).col(style, |ui| { ui.slider(&mut self.zoom, 0.0..=200.0); }); ui.slider(&mut self.zoom, 0.0..=200.0)"),
  b("Controls", "Custom slider style", "struct Meter: ProgressViewStyle { … } (SwiftUI has no slider style)", "impl SliderStyle for Meter { fn make_body(&self, ui: &mut Ui, c: SliderConfiguration) { let k = c.fraction(); c.root(ui).canvas(style, move |f, r| { … }); } } Slider::new(&mut self.level, 10.0..=90.0).step(10.0).show(ui.slider_style(&Meter))"),
  b("Controls", "Picker", "Picker(\"Size\", selection: $size) { …; Divider(); … }", "Picker::new(&mut self.size).options(&A).divider().options(&B).show(ui)"),
  b("Controls", "Segmented picker", "Picker(…) { … }.pickerStyle(.segmented)", "ui.picker_style(&SegmentedPickerStyle).picker(&mut self.view, &[Tag::new(0, \"List\").count(128), Tag::new(1, \"Grid\")])"),
  b("Controls", "Picker style", "VStack { Picker(…) { … } }.pickerStyle(.inline); Picker(…) { … }", "ui.picker_style(&InlinePickerStyle).row(style, |ui| _ = ui.picker(&mut self.size, &SIZES)); ui.picker(&mut self.size, &SIZES)"),
  b("Controls", "Custom picker style", "struct Chips: PickerStyle (SwiftUI's is closed: its styles are the system's)", "impl PickerStyle for Chips { fn make_body(&self, ui: &mut Ui, mut c: PickerConfiguration) { let b = c.segments(ui); ui.row(style, |ui| for i in 0..c.len() { ui.with_id(c.item_id(i)).row(b.pointer(chip), |ui| ui.text(c.label(i), s)); }); } } ui.picker_style(&Chips).picker(&mut self.view, &VIEWS)"),
  b("Controls", "Text field", "TextField(\"Name\", text: $name)", "ui.text_field(&mut self.name, \"Name\")"),
  b("Controls", "Multi-line field", "TextField(\"Notes\", text: $notes, axis: .vertical)", "TextField::new(&mut self.notes, \"Notes\").axis(Axis::Vertical).show(ui)"),
  b("Controls", "Secure field", "SecureField(\"Password\", text: $key)", "ui.secure_field(&mut self.key, \"Password\")"),
  b("Controls", "Search field", ".searchable(text: $query)", "ui.search_field(&mut self.query, \"Search\")"),
  b("Controls", "Text field style", "VStack { TextField(\"Name\", text: $name); TextField(\"City\", text: $city) }.textFieldStyle(.plain)", "ui.text_field_style(&PlainTextFieldStyle).col(style, |ui| { ui.text_field(&mut self.name, \"Name\"); ui.text_field(&mut self.city, \"City\"); }); ui.text_field(&mut self.email, \"E-mail\")"),
  b("Controls", "Custom text field style", "struct Underline: TextFieldStyle { func _body(configuration: TextField<_Label>) -> some View { configuration.padding(…).overlay(alignment: .bottom) { … } } }", "impl TextFieldStyle for Underline { fn make_body(&self, ui: &mut Ui, mut c: TextFieldConfiguration) { ui.row(style.scroll(c.axis().into()), |ui| c.text(ui, &fs)); } } ui.text_field_style(&Underline).text_field(&mut self.city, \"City\")"),
  b("Controls", "Progress", "ProgressView(value: 0.42)", "ui.progress(Some(0.42))"),
  b("Controls", "Spinner", "ProgressView()", "ui.spinner()"),
  b("Controls", "Badge", ".badge(\"New\")", "ui.badge(\"New\", Tone::Accent)"),
  b("Controls", "Hotkey recorder", "KeyboardShortcut (SwiftUI has no recorder)", "ui.hotkey_recorder(&mut self.hotkey)"),
  b("Controls", "Control behavior", "Button(action:) { … }.buttonStyle(MyStyle()), a look of the app's own", "let b = ui.button_behavior(id); ui.control(&b).canvas(style, move |f, r| { … }); if b.clicked { … }"),
  b("Controls", "Help marks", "Image(systemName: \"info.circle\").help(tip); HelpLink { … }", "ui.help(tip).info_button(); ui.help(why).warning_button(); ui.help_button()"),
  b("Form", "Section", "Section { … } header: { … } footer: { … }", "ui.form_section(&fs, Section::new(header).footer(\"…\"), |ui| { … })"),
  b("Form", "Form row", "LabeledContent { Toggle(isOn: $on) {} } label: { Text(title); Text(subtitle) }", "ui.form_row(&fs, FormRow::new(\"Open at login\").caption(\"…\"), |ui| ui.toggle(&mut on, ()))"),
  b("Form", "Composed label", "label: { Text(name); Text(\"New\").badge(…) }", "FormRow::new(|ui: &mut Ui, s: TextStyle| { ui.text(name, s); ui.control_size(ControlSize::Small).badge(\"New\", Tone::Accent); })"),
  b("Form", "Row tile", "Label { Text(title) } icon: { Image(systemName: \"folder\") }", "FormRow::new(\"Data\").tile(Icon::FOLDER)"),
  b("Form", "Row footer", "Section { VStack { Toggle(…); Slider(…) } }", "FormRow::new(\"Turn off below 30%\").footer(|ui: &mut Ui, _: TextStyle| _ = ui.slider(&mut level, 10.0..=90.0))"),
  b("Form", "Disclosure", "DisclosureGroup(\"Other models\") { … }", "ui.form_disclosure(&fs, FormRow::new(\"Other models\"), |ui| { … })"),
  b("Form", "Toolbar tabs", "TabView(selection: $page) { General().tabItem { Label(\"General\", systemImage: \"gear\") }.tag(Page.general) } in a Settings scene", "ui.toolbar_tabs(&fs, &mut self.page, &[Tag::new(Page::General, \"General\").icon(Icon::GEAR), Tag::new(Page::About, \"About\").icon(Icon::INFO)])"),
  b("Form", "Footer", "HelpLink { … } at a pane's end", "ui.help(\"Help\").form_footer(&fs, |ui| _ = ui.button(\"Reset All…\"))"),
  b("List", "List", "List(items, selection: $sel) { … }", "List::new(n, 24.0).selection(&mut sel).show(ui, |ui, row| { … })"),
  b("List", "List sections", "Section(header:) in a List; .listStyle(…)", "List::new(n, 32.0).sections(&starts, 24.0, &head).show(ui.list_style(&InsetGroupedListStyle), |ui, row| { … })"),
  b("List", "List style", "List(items, selection: $sel) { … }.listStyle(.plain)", "list.show(ui.list_style(&PlainListStyle), |ui, row| { … }); InsetListStyle by default, InsetGroupedListStyle"),
  b("List", "Custom list style", "struct Stripes: ListStyle (SwiftUI's has no public makeBody)", "impl ListStyle for Stripes { fn make_body<'a>(&self, ui: &mut Ui<'a>, mut c: ListConfiguration<'_, 'a>) { ui.scroll_area(c.frame(), |ui| c.rows(ui, |ui, mut r| _ = ui.row(r.frame().bg(fill), |ui| r.content(ui)))); } }"),
  b("List", "List behavior", "List(rows, selection: $sel) { … }.listRowBackground(…)", "let b = ui.list_behavior(id, ListRows::new(n, 22.0), &mut sel, &key); ui.with_id(id).scroll_area(b.style(style), |ui| ui.list_rows(&b, |ui, i, k| { … }))"),
  b("Containers", "Tabs", "TabView { … }", "Tabs::new(&mut self.tab, &[\"One\", \"Two\"]).show(ui, |ui, i| { … })"),
  b("Containers", "Split", "HSplitView { … }", "Split::new(Axis::Horizontal).ratio(&mut self.side).show(ui, |ui, pane| { … })"),
  b("Containers", "Navigation split", "NavigationSplitView { … } detail: { … }", "Split::navigation().ratio(&mut self.side).show(ui, |ui, pane| { … })"),
  b("Containers", "Toolbar", "Detail().toolbar { ToolbarItem(placement: .principal) { … }; ToolbarItem(placement: .primaryAction) { … } } in a NavigationSplitView", "ui.toolbar(|ui| ui.toolbar_item(ToolbarItemPlacement::PrimaryAction, |ui| _ = ui.icon_button(Icon::DOC))) in a column"),
  b("Containers", "Unified toolbar", ".windowToolbarStyle(.unified)", "ui.window_toolbar_style(WindowToolbarStyle::Unified); ui.row(band, |ui| { … }) at the window's top"),
  b("Containers", "Menu", "Menu(\"Actions\") { Button(…) }", "ui.menu(\"Actions\", |ui| { ui.menu_item(\"Copy\"); })"),
  b("Containers", "Lazy column", "LazyVStack { ForEach(…) }", "ui.virtual_col(heights, |i| i, |ui, i| { … })"),
  b("Containers", "Scroll behavior", "ScrollView { … }.scrollIndicators(.hidden).onScrollGeometryChange(…) { … }", "let b = ui.scroll_behavior(id, Axes::VERTICAL); ui.with_id(id).col(style.scroll(Axes::VERTICAL), |ui| { …; if let Some(bar) = b.bar(Axis::Vertical) { ui.with_id(b.thumb(Axis::Vertical)).float(at).el(knob) } })"),
  b("Containers", "Scroll indicators", "NSScroller.knobStyle = .light (AppKit; SwiftUI has none)", "ui.set_environment(Some(ScrollerKnob { color: red, ..ScrollerKnob::DARK })); ui.scroll_area(style, |ui| { … })"),
  b("Containers", "Stack queries", "Divider() follows its stack's axis (SwiftUI has no API for it)", "let line = match ui.parent_axis() { Some(Axis::Horizontal) => v, _ => h }; ui.child_count() == 0"),
  b("Presentation", "Alert", ".alert(\"Delete?\", isPresented: $ask) { Button(\"Delete\", role: .destructive) }", "Dialog::new(\"Delete?\").button(\"Delete\", ButtonRole::Destructive).show(ui, &mut self.ask)"),
  b("Presentation", "Context menu", ".contextMenu { Button(\"Copy\") { … } }", "ui.context_menu(r.id, || Menu::new(vec![Item::button(Action::Copy, \"Copy\")])); match ui.native_menu(id, at, menu) { NativeMenu::Unavailable => /* draw it */, NativeMenu::Unanswered => {}, NativeMenu::Answered(a) => … }"),
  b("Presentation", "Menu bar", ".commands { CommandMenu(\"Track\") { Button(\"Play\") { … } }; CommandGroup(replacing: .newItem) { … } }", "main.set_commands(&Commands::new().menu(\"Track\", Menu::new(vec![Item::button(Act::Play, \"Play\")])).replacing(CommandGroupPlacement::NewItem, items), |main, act| …)"),
  b("Presentation", "Tooltip", ".help(\"Saves it\")", "ui.help(\"Saves it\").button(\"Save\"); ui.help_style(&PlainHelpStyle) for the window, TooltipHelpStyle by default"),
  b("Presentation", "Custom help style", ".help(\"Saves it\") (SwiftUI has no help style)", "impl HelpStyle for Note { fn make_body(&self, ui: &mut Ui, c: HelpConfiguration) { ui.row(style.opacity(c.opacity()), |ui| ui.text(c.text(), s)); } } ui.help_style(&Note)"),
  b("Modifiers", "Disabled", ".disabled(true)", "ui.disabled(true).col(Style::new(), |ui| { … })"),
  b("Modifiers", "Control size", ".controlSize(.small)", "ui.control_size(ControlSize::Small).row(Style::new(), |ui| { … })"),
  b("Modifiers", "Accessibility label", ".accessibilityLabel(\"Reset\")", "ui.a11y_label(\"Reset\").icon_button(Icon::RESET)"),
  b("Modifiers", "Keyboard shortcut", ".keyboardShortcut(\"z\", modifiers: .command)", "ui.keymap(&[Binding::new(Shortcut::char('z').cmd(), Command::Undo)]).col(…)"),
  b("Modifiers", "Environment", ".environment(\\.tint, .green) { … }; @Environment(\\.tint) var tint", "ui.environment(Tint(t.success())).row(style, |ui| { … }); ui.set_environment(Some(Tint(t.success()))) for the window; let tint = ui.environment_value::<Tint>()"),
  b("Modifiers", "Group", "Group { Button(\"Save\"); Toggle(…) }.disabled(true)", "ui.disabled(true).group(|ui| { ui.button(\"Save\"); ui.toggle(&mut on, ()); }); ui.button(\"Open\")"),
  b("Modifiers", "Identity", "Box().id(item.id); .allowsHitTesting(false)", "ui.key(item.id).row(style, |ui| { … }); ui.with_id(id.key(\"box\")).allows_hit_testing(false).col(style, |ui| { … })"),
  b("Modifiers", "Focus and pointer", ".focusable(); .pointerStyle(.link)", "ui.focusable(true).pointer_style(Cursor::Pointer).row(style, |ui| { … })"),
  b("Modifiers", "Widget parts", ".environment(\\.isEnabled, true); .help(tip) on a part", "let tip = ui.take_help(); if let Some(tip) = &tip { ui.help(tip); } ui.environment(Enabled(true)).button(\"Undo\")"),
  b("Modifiers", "Frame request", "TimelineView(.animation) { context in … }", "ui.request_frame(); let k = ui.now().as_secs_f32() % 1.0"),
  b("Modifiers", "Accessibility pair", ".accessibilityLabeledPair(role: .label, id: \"volume\", in: ns)", "ui.a11y_labeled_pair(LabeledPairRole::Label, id, |ui| ui.text(\"Volume\", s)); ui.a11y_labeled_pair(LabeledPairRole::Content, id, |ui| ui.slider(&mut v, 0.0..=1.0))"),
  b("Modifiers", "Controls theme", "@Environment(\\.controlActiveState) var state", "let t = ui.controls_theme(); ui.el(Style::new().bg(t.accent())); let grey = ui.theme().inactive().accent()"),
  b("Modifiers", "Color scheme", "@Environment(\\.colorScheme) var colorScheme", "let dark = ui.color_scheme().is_dark()"),
];
