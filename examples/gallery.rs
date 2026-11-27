// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A catalog of nimbo's building blocks, each beside its SwiftUI counterpart: a sidebar of every
//! block by group (layout, text, controls, form, list, containers, presentation, modifiers) and a
//! page for the one chosen, with the SwiftUI line, the nimbo line and the block live in its
//! variants (enabled and disabled, small and large where it has sizes), in the system's light or
//! dark. The same table makes `docs/api.md` and its pictures (`make api-shots`). After the
//! table, a Canvas page draws into the frame (shapes, text in fonts and scripts) and shows the
//! raw pointer and keys.
//!
//! `cargo run --release --example gallery [-- <block>] [--shot <file.png>]`: with a block's slug
//! the window is that block's variants alone, as high as they are; `canvas` opens the Canvas
//! page; `pace` is the pacing scene `make pace` measures; `--api` prints `docs/api.md`, `--list`
//! the slugs.
//!
//! The table is in [blocks], the blocks built live in a module per group in [pages], the Canvas
//! page and the pacing scene in [canvas].

// A Windows app, not a console one: no terminal opens with its window.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod blocks {
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
      let s: String =
        self.name.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
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
  b("Layout", "Scroll anchor", "ScrollView { … }.defaultScrollAnchor(.bottom)", "ui.default_scroll_anchor(UnitPoint::BOTTOM).scroll_area(style, |ui| { … })"),
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
  b("Containers", "Lazy column", "LazyVStack { ForEach(…) }", "ui.lazy_col(n, |i| i, |ui, i| { … })"),
  b("Containers", "Scroll behavior", "ScrollView { … }.scrollIndicators(.hidden).onScrollGeometryChange(…) { … }", "let b = ui.scroll_behavior(id, Axes::VERTICAL); ui.with_id(id).col(style.scroll(Axes::VERTICAL), |ui| { …; if let Some(bar) = b.bar(Axis::Vertical) { ui.with_id(b.thumb(Axis::Vertical)).float(at).el(knob) } })"),
  b("Containers", "Scroll indicators", "NSScroller.knobStyle = .light (AppKit; SwiftUI has none)", "ui.set_environment(Some(ScrollerKnob::DARK.color(red))); ui.scroll_area(style, |ui| { … })"),
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
}
#[path = "common/example.rs"]
mod example;
mod pages {
  //! The blocks built live, a module per group: [Gallery::demo] builds the one shown in its
  //! variants.

  mod containers {
    //! The container blocks, built live: tabs, splits, toolbars, a menu, a lazy column, scrolling
    //! of the app's own and stack queries.

    use nimbo::{
      Align, Axes, Axis, Float, Icon, Id, ScrollBar, ScrollerKnob, Split, Style, Tabs,
      ToolbarItemPlacement, Ui, UnitPoint, WindowToolbarStyle, alpha, px,
    };

    use super::{Gallery, Shown, line};

    impl Gallery {
      /// The live container block `name`.
      pub(super) fn containers(&mut self, ui: &mut Ui, name: &str) {
        let t = ui.theme();
        match name {
          "Tabs" => Self::variants(ui, Shown::Plain, |ui| {
            ui.col(Style::new().w(px(260.0)).h(px(70.0)), |ui| {
              Tabs::new(&mut self.tab, &["General", "Advanced"]).show(ui, |ui, i| {
                ui.text(["The general page", "The advanced page"][i], t.label());
              });
            });
          }),
          "Split" => Self::variants(ui, Shown::Plain, |ui| {
            ui.row(Style::new().w(px(260.0)).h(px(60.0)), |ui| {
              Split::new(Axis::Horizontal).ratio(&mut self.side).show(ui, |ui, pane| {
                ui.text(["Sidebar", "Content"][pane], t.label());
              });
            });
          }),

          "Navigation split" => Self::variants(ui, Shown::Plain, |ui| {
            ui.row(Style::new().w(px(260.0)).h(px(60.0)), |ui| {
              Split::navigation().ratio(&mut self.side).show(ui, |ui, pane| {
                let ground = if pane == 0 { t.panel() } else { t.bg() };
                ui.col(Style::new().grow().pad(8.0).bg(ground), |ui| {
                  ui.text(["Sidebar", "Detail"][pane], t.label())
                });
              });
            });
          }),

          // In a window's title bar on macOS ([nimbo::Config::unified]: `examples/messenger`); here
          // in a bar of its own, a sidebar's column with its toggle beside the detail's with its
          // action.
          "Toolbar" => Self::variants(ui, Shown::Plain, |ui| {
            let bar = Style::new().w(px(360.0)).h(px(52.0)).radius(8.0).pad(1.0).clip().bg(t.bg());
            ui.row(bar.border(1.0, t.line()), |ui| {
              Split::navigation().ratio(&mut self.bar_side).show(ui, |ui, pane| {
                let ground = if pane == 0 { t.panel() } else { t.bg() };
                ui.col(Style::new().grow().bg(ground), |ui| {
                  // The sidebar's toggle at its trailing edge; the detail's title in its centre and
                  // its action at its end.
                  ui.toolbar(|ui| match pane {
                    0 => ui.toolbar_item(ToolbarItemPlacement::Navigation, |ui| {
                      _ = ui.help("Hide Sidebar").icon_button(Icon::SIDEBAR)
                    }),
                    _ => {
                      ui.toolbar_item(ToolbarItemPlacement::Principal, |ui| {
                        ui.text("Notes", t.label())
                      });
                      ui.toolbar_item(ToolbarItemPlacement::PrimaryAction, |ui| {
                        _ = ui.help("New Note").icon_button(Icon::DOC)
                      });
                    }
                  });
                });
              });
            });
          }),

          // A band of the app's own: at a window's top it would ask for a toolbar's title bar.
          "Unified toolbar" => Self::variants(ui, Shown::Alone, |ui| {
            let band =
              Style::new().w(px(360.0)).h(px(52.0)).radius(8.0).pad_xy(12.0, 0.0).bg(t.panel());
            ui.window_toolbar_style(WindowToolbarStyle::Unified);
            ui.row(band.align(Align::Start, Align::Center).gap(8.0), |ui| {
              ui.text("Notes", t.label());
              ui.spacer();
              _ = ui.help("New Note").icon_button(Icon::DOC);
            });
          }),

          // Each stack's axis and children so far, read where the text goes.
          "Stack queries" => Self::variants(ui, Shown::Alone, |ui| {
            let read = |ui: &mut Ui| {
              let (axis, n) = (ui.parent_axis(), ui.child_count());
              if let Some(axis) = axis {
                ui.text(format_args!("{axis:?}, {n} before"), t.label());
              }
            };
            ui.row(line(8.0), |ui| (0..2).for_each(|_| read(ui)));
            ui.col(Style::new().gap(4.0), |ui| (0..2).for_each(|_| read(ui)));
          }),

          "Menu" => Self::variants(ui, Shown::Plain, |ui| {
            ui.menu("Actions", |ui| {
              ui.menu_item("Copy");
              ui.menu_item("Paste");
            });
          }),
          "Lazy column" => Self::variants(ui, Shown::Plain, |ui| {
            ui.scroll_area(Style::new().w(px(200.0)).h(px(60.0)), |ui| {
              let item = |ui: &mut Ui, i: usize| ui.text(format_args!("Item {i}"), t.label());
              ui.lazy_col(1000, |i| i, item);
            });
          }),

          // A bar of its own over the area's behaviour: a thin accent thumb, always shown.
          "Scroll behavior" => Self::variants(ui, Shown::Alone, |ui| {
            let id = Id::of("own scroll");
            let b = ui.scroll_behavior(id, Axes::VERTICAL);
            let area =
              Style::new().w(px(200.0)).h(px(60.0)).pad_xy(8.0, 4.0).radius(6.0).bg(t.panel());
            let area = area.scroll(Axes::VERTICAL);
            ui.with_id(id).col(area, |ui| {
              (1..=8).for_each(|i| ui.text(format_args!("Line {i}"), t.label()));
              let Some(bar) = b.bar(Axis::Vertical) else { return };
              let at =
                Float::parent().attach(UnitPoint::TOP_TRAILING).anchor(UnitPoint::TOP_TRAILING);
              let at = at.offset(-ScrollBar::INSET, ScrollBar::INSET + bar.pos);
              let thumb = Style::new().w(px(4.0)).h(px(bar.thumb)).radius(2.0).bg(t.accent());
              ui.with_id(b.thumb(Axis::Vertical)).float(at).el(thumb);
            });
          }),

          // Red knobs in this area, put back after it for the gallery's other areas.
          "Scroll indicators" => Self::variants(ui, Shown::Alone, |ui| {
            let red = ScrollerKnob::DARK.color(alpha(t.danger(), 0.6));
            let old = ui.set_environment(Some(red));
            ui.scroll_area(Style::new().w(px(200.0)).h(px(60.0)), |ui| {
              (1..=8).for_each(|i| ui.text(format_args!("Line {i}"), t.label()))
            });
            ui.set_environment(old);
          }),
          _ => unreachable!("{name} is not a container block"),
        }
      }
    }
  }
  mod controls {
    //! The control blocks, built live: buttons, toggles, sliders, pickers and text fields in the
    //! system's styles and in the gallery's own, progress, a badge, a hotkey recorder, a control
    //! drawn on a button's behaviour and help marks.

    use nimbo::{
      Align, Axis, BorderedProminentButtonStyle, BorderlessButtonStyle, Button,
      ButtonConfiguration, ButtonRole, ButtonStyle, ButtonToggleStyle, CheckboxToggleStyle,
      FieldStyle, Icon, InlinePickerStyle, Picker, PickerConfiguration, PickerStyle,
      PlainSliderStyle, PlainTextFieldStyle, PlainToggleStyle, Quad, Rect, SegmentedPickerStyle,
      Slider, SliderConfiguration, SliderStyle, Style, Tag, TextField, TextFieldConfiguration,
      TextFieldStyle, TextStyle, ToggleConfiguration, ToggleStyle, Tone, Ui, alpha, grow, mix, px,
    };

    use super::{Gallery, SIZES, Shown, line};

    /// A button style of the gallery's own: a capsule tinted with the accent, deeper while held,
    /// faint when disabled.
    struct Pill;

    impl ButtonStyle for Pill {
      fn make_body(&self, ui: &mut Ui, mut c: ButtonConfiguration) {
        let t = ui.theme();
        let (k, ink) = match (c.is_enabled(), c.is_pressed()) {
          (false, _) => (0.06, alpha(t.accent(), 0.4)),
          (true, pressed) => (if pressed { 0.3 } else { 0.15 }, t.accent()),
        };

        let s = Style::new().h(px(24.0)).pad_xy(12.0, 0.0).gap(5.0).center().radius(12.0);
        ui.row(s.bg(alpha(t.accent(), k)), |ui| c.label(ui, TextStyle::new(t.font(), ink)));
      }
    }

    /// A toggle style of the gallery's own: a dot in the accent before the label, filled while on,
    /// ringed while off; faint when disabled.
    struct Dot;

    impl ToggleStyle for Dot {
      fn make_body(&self, ui: &mut Ui, mut c: ToggleConfiguration) {
        let t = ui.theme();
        let k = if c.is_enabled() { 1.0 } else { 0.4 };
        let dot =
          Style::new().w(px(12.0)).h(px(12.0)).radius(6.0).border(1.5, alpha(t.accent(), k));
        let dot = if c.is_on() { dot.bg(alpha(t.accent(), k)) } else { dot };
        ui.row(Style::new().gap(6.0).center(), |ui| {
          ui.el(dot);
          c.label(ui, TextStyle::new(t.font(), alpha(t.text(), k)));
        });
      }
    }

    /// A slider style of the gallery's own: a capsule filled in the accent up to the value, a notch
    /// at each step between its ends, no knob; faint when disabled.
    struct Meter;

    impl SliderStyle for Meter {
      fn make_body(&self, ui: &mut Ui, c: SliderConfiguration) {
        let t = ui.theme();
        let a = if c.is_enabled() { 1.0 } else { 0.4 };
        let (k, track, fill, notch) =
          (c.fraction(), alpha(t.accent(), 0.15 * a), alpha(t.accent(), a), alpha(t.bg(), 0.8));
        let notches: Vec<f32> =
          c.steps().map(|(_, p)| p).filter(|p| *p > 0.0 && *p < 1.0).collect();
        c.root(ui).canvas(Style::new().w(grow()).h(px(8.0)), move |f, r| {
          f.rect(r, track, 4.0);
          f.rect(Rect::new(r.x, r.y, r.w * k, r.h), fill, 4.0);
          for p in &notches {
            f.rect(Rect::new(r.x + r.w * p - 0.5, r.y, 1.0, r.h), notch, 0.0);
          }
        });
      }
    }

    /// A text field style of the gallery's own: the text over a line, the accent's while focused,
    /// which rings nothing else.
    struct Underline;

    impl TextFieldStyle for Underline {
      fn make_body(&self, ui: &mut Ui, mut c: TextFieldConfiguration) {
        let t = ui.theme();
        let line = if c.is_focused() { t.accent() } else { t.line() };
        let s =
          Style::new().w(grow()).h(px(24.0)).pad_xy(2.0, 0.0).borders([0.0, 0.0, 1.0, 0.0], line);
        let s = s.align(Align::Start, Align::Center).focusable_quiet().scroll(c.axis().into());
        let s = if c.is_enabled() { s } else { s.opacity(t.disabled()) };
        let text = FieldStyle::new(TextStyle::new(t.font(), t.text())).placeholder(t.faint());
        ui.row(s, |ui| c.text(ui, &text.caret(t.accent())));
      }
    }

    /// A picker style of the gallery's own: the options as capsules, the selected one filled with
    /// the accent, faint when disabled.
    struct Chips;

    impl PickerStyle for Chips {
      fn make_body(&self, ui: &mut Ui, mut c: PickerConfiguration) {
        let t = ui.theme();
        let b = c.segments(ui);
        let k = if c.is_enabled() { 1.0 } else { 0.4 };
        ui.row(Style::new().gap(4.0), |ui| {
          for i in 0..c.len() {
            let on = c.selection() == Some(i);
            let (bg, ink) = match on {
              true => (alpha(t.accent(), k), alpha(t.on_accent(), k)),
              false => (alpha(t.accent(), 0.12 * k), alpha(t.accent(), k)),
            };

            let chip = Style::new().h(px(22.0)).pad_xy(10.0, 0.0).center().radius(11.0).bg(bg);
            ui.with_id(c.item_id(i))
              .row(b.pointer(chip), |ui| ui.text(c.label(i), TextStyle::new(t.font(), ink)));
          }
        });
      }
    }

    /// The views a picker shows.
    const VIEWS: [Tag<u8>; 3] = [Tag::new(0, "List"), Tag::new(1, "Grid"), Tag::new(2, "Columns")];

    impl Gallery {
      /// The live control block `name`.
      pub(super) fn controls(&mut self, ui: &mut Ui, name: &str) {
        let t = ui.theme();
        match name {
          "Button" => Self::variants(ui, Shown::Sized, |ui| {
            ui.button("Bordered");
            ui.button_style(&BorderedProminentButtonStyle).button("Prominent");
            ui.button_style(&BorderlessButtonStyle).button("Borderless");
          }),
          "Button role" => Self::variants(ui, Shown::Sized, |ui| {
            Button::new("Delete").role(ButtonRole::Destructive).show(ui);
            let red = Button::new("Delete").role(ButtonRole::Destructive);
            red.show(ui.button_style(&BorderedProminentButtonStyle));
          }),

          "Icon button" => Self::variants(ui, Shown::Sized, |ui| {
            ui.a11y_label("Reset").icon_button(Icon::RESET);
            ui.icon_button(Icon::TRASH);
          }),

          // Two buttons borderless by their row, the one after it in the window's style.
          "Button style" => Self::variants(ui, Shown::Sized, |ui| {
            ui.button_style(&BorderlessButtonStyle).row(line(10.0), |ui| {
              ui.button("Cut");
              ui.button("Copy");
            });
            ui.button("Paste");
          }),
          "Custom button style" => Self::variants(ui, Shown::Plain, |ui| {
            ui.button_style(&Pill).button("Pill");
            Button::new("Open").icon(Icon::FOLDER).show(ui.button_style(&Pill));
          }),

          "Switch" => Self::variants(ui, Shown::Sized, |ui| _ = ui.toggle(&mut self.live, "Live")),
          "Check box" => Self::variants(ui, Shown::Plain, |ui| {
            _ = ui.toggle_style(&CheckboxToggleStyle).toggle(&mut self.wrap, "Wrap")
          }),
          "Button toggle" => Self::variants(ui, Shown::Sized, |ui| {
            ui.toggle_style(&ButtonToggleStyle).toggle(&mut self.bold, "Bold");
            ui.toggle_style(&ButtonToggleStyle).toggle(&mut self.italic, "Italic");
          }),

          // Two check boxes by their column, the switch after it in the window's style.
          "Toggle style" => Self::variants(ui, Shown::Plain, |ui| {
            ui.toggle_style(&CheckboxToggleStyle).col(Style::new().gap(6.0), |ui| {
              ui.toggle(&mut self.wrap, "Wrap");
              ui.toggle(&mut self.numbers, "Numbers");
            });
            ui.toggle(&mut self.live, "Live");
          }),
          "Custom toggle style" => Self::variants(ui, Shown::Plain, |ui| {
            ui.toggle_style(&Dot).toggle(&mut self.live, "Live");
            ui.toggle_style(&PlainToggleStyle).toggle(&mut self.wrap, "Plain");
          }),

          // A slider fills the width it is offered: 160 here.
          "Slider" => Self::variants(ui, Shown::Plain, |ui| {
            ui.col(Style::new().w(px(160.0)), |ui| {
              _ = ui.a11y_label("Zoom").slider(&mut self.zoom, 0.0..=200.0)
            });
            ui.text(format_args!("{:.0}%", self.zoom), t.muted().nowrap());
          }),
          "Slider steps" => Self::variants(ui, Shown::Plain, |ui| {
            let label = |v: f32| [10.0, 50.0, 90.0].contains(&v).then(|| format!("{v}%"));
            let s = Slider::new(&mut self.level, 10.0..=90.0).step(10.0).tick_label(label);
            ui.col(Style::new().w(px(160.0)), |ui| _ = s.show(ui.a11y_label("Level")));
            ui.text(format_args!("{:.0}%", self.level), t.muted().nowrap());
          }),

          // A plain slider by its column, the one after it in the window's style.
          "Slider style" => Self::variants(ui, Shown::Plain, |ui| {
            ui.col(Style::new().w(px(160.0)).gap(8.0), |ui| {
              ui.slider_style(&PlainSliderStyle).col(Style::new().w(grow()), |ui| {
                _ = ui.a11y_label("Zoom").slider(&mut self.zoom, 0.0..=200.0)
              });
              _ = ui.a11y_label("Zoom").slider(&mut self.zoom, 0.0..=200.0);
            });
          }),
          "Custom slider style" => Self::variants(ui, Shown::Plain, |ui| {
            let s = Slider::new(&mut self.level, 10.0..=90.0).step(10.0);
            ui.col(Style::new().w(px(160.0)), |ui| {
              _ = s.show(ui.slider_style(&Meter).a11y_label("Level"))
            });
            ui.text(format_args!("{:.0}%", self.level), t.muted().nowrap());
          }),

          "Picker" => Self::variants(ui, Shown::Plain, |ui| {
            let p = Picker::new(&mut self.size).options(&SIZES[..1]).divider();
            p.options(&SIZES[1..]).show(ui.a11y_label("Size"));
          }),
          "Segmented picker" => Self::variants(ui, Shown::Plain, |ui| {
            let views = [
              Tag::new(0, "List").count(128),
              Tag::new(1, "Grid").count(12),
              Tag::new(2, "Columns").count(3),
            ];
            ui.picker_style(&SegmentedPickerStyle).picker(&mut self.view, &views);
          }),

          // Pickers inline by their row, the one after it in the window's style.
          "Picker style" => Self::variants(ui, Shown::Plain, |ui| {
            ui.picker_style(&InlinePickerStyle).row(line(10.0), |ui| {
              _ = ui.a11y_label("Size").picker(&mut self.size, &SIZES);
            });
            ui.a11y_label("Size").picker(&mut self.size, &SIZES);
          }),
          "Custom picker style" => Self::variants(ui, Shown::Plain, |ui| {
            ui.picker_style(&Chips).picker(&mut self.view, &VIEWS);
          }),

          "Text field" => Self::variants(ui, Shown::Plain, |ui| {
            ui.col(Style::new().w(px(200.0)), |ui| _ = ui.text_field(&mut self.name, "Name"));
          }),
          // A multi-line field fills the height its parent gives it.
          "Multi-line field" => Self::variants(ui, Shown::Plain, |ui| {
            ui.col(Style::new().w(px(200.0)).h(px(60.0)), |ui| {
              _ = TextField::new(&mut self.notes, "Notes").axis(Axis::Vertical).show(ui)
            });
          }),
          "Secure field" => Self::variants(ui, Shown::Plain, |ui| {
            ui.col(Style::new().w(px(200.0)), |ui| _ = ui.secure_field(&mut self.key, "Password"));
          }),
          "Search field" => Self::variants(ui, Shown::Plain, |ui| {
            ui.col(Style::new().w(px(200.0)), |ui| _ = ui.search_field(&mut self.query, "Search"));
          }),

          // Two fields plain by their column, the one after it in the window's style.
          "Text field style" => Self::variants(ui, Shown::Plain, |ui| {
            let col = Style::new().w(px(120.0)).gap(6.0);
            ui.text_field_style(&PlainTextFieldStyle).col(col, |ui| {
              ui.text_field(&mut self.name, "Name");
              ui.text_field(&mut self.city, "City");
            });
            ui.col(Style::new().w(px(160.0)), |ui| _ = ui.text_field(&mut self.email, "E-mail"));
          }),
          "Custom text field style" => Self::variants(ui, Shown::Plain, |ui| {
            ui.col(Style::new().w(px(200.0)), |ui| {
              _ = ui.text_field_style(&Underline).text_field(&mut self.city, "City")
            });
          }),

          "Progress" => Self::variants(ui, Shown::Plain, |ui| {
            ui.col(Style::new().w(px(150.0)), |ui| ui.progress(Some(0.42)));
            ui.col(Style::new().w(px(150.0)), |ui| ui.progress(None));
          }),
          "Spinner" => Self::variants(ui, Shown::Sized, |ui| _ = ui.spinner()),
          "Badge" => Self::variants(ui, Shown::Sized, |ui| {
            ui.badge(128, Tone::Neutral);
            ui.badge("New", Tone::Accent);
            ui.badge("+5", Tone::Success);
            ui.badge("-4", Tone::Danger);
            ui.badge("Conflict", Tone::Warning);
          }),

          "Hotkey recorder" => Self::variants(ui, Shown::Plain, |ui| {
            ui.hotkey_recorder(&mut self.hotkey);
            // The ⊗ that clears one, which the plates themes leave to a reset button.
            let t = ui.theme();
            ui.set_theme(t.with_hotkey_recorder_clear(true));
            ui.hotkey_recorder(&mut self.hotkey);
            ui.set_theme(t);
          }),

          // A round button the app draws on a button's behaviour: Tab focuses it, the hand over it.
          "Control behavior" => Self::variants(ui, Shown::Plain, |ui| {
            let id = ui.take_id();
            let b = ui.button_behavior(id);
            let fill = match (b.pressed, b.hovered) {
              (true, _) => t.accent_hover(),
              (_, true) => mix(t.accent(), t.accent_hover(), 0.5),
              _ => t.accent(),
            };

            let (fill, mark) = match b.disabled {
              true => (alpha(fill, t.disabled()), alpha([1.0; 4], t.disabled())),
              false => (fill, [1.0; 4]),
            };

            let ring = if b.focused { t.focus() } else { [0.0; 4] };
            let s = Style::new().w(px(28.0)).h(px(28.0));

            ui.control(&b).canvas(s, move |f, r| {
              f.quad(Quad::new(r, fill).radius(14.0).border(2.0, ring));
              Icon::PLAY.draw(f, Rect::new(r.x + 7.0, r.y + 7.0, 14.0, 14.0), mark);
            });
          }),

          "Help marks" => Self::variants(ui, Shown::Plain, |ui| {
            ui.help("An ⓘ explains what one line can't").info_button();
            ui.help("A warning tells what is wrong").warning_button();
            ui.help("Opens the page's help").help_button();
          }),
          _ => unreachable!("{name} is not a control block"),
        }
      }
    }
  }
  mod form {
    //! The form blocks, built live in the form's look on its ground: a section, rows (a composed
    //! label, a tile, a footer), a disclosure, a pane's footer and the toolbar's tabs.

    use nimbo::{
      ControlSize, FormRow, FormStyle, Icon, LinePattern, Section, Slider, Style, Tag, TextStyle,
      Tone, Ui, grow,
    };

    use super::{Gallery, Shown};

    impl Gallery {
      /// A form block in the form's look, on its ground.
      pub(super) fn form(&mut self, ui: &mut Ui, name: &str) {
        let fs = FormStyle::plates(&ui.theme());
        let (on, level, tab) = (&mut self.on, &mut self.level, &mut self.tab);
        Self::variants(ui, Shown::Plain, |ui| {
          let row = |ui: &mut Ui, on: &mut bool, level: &mut f32| match name {
            "Form row" => {
              let row =
                FormRow::new("Open at login").caption("Starts with your Mac, in the menu bar.");
              ui.form_row(&fs, row, |ui| ui.toggle(on, ()))
            }

            "Composed label" => {
              let label = |ui: &mut Ui, s: TextStyle| {
                ui.text("gigaam-v3", s.line_limit(1));
                let ui = ui.help("Best in Russian").control_size(ControlSize::Small);
                ui.badge("Recommended", Tone::Accent);
              };
              let caption = |ui: &mut Ui, s: TextStyle| {
                ui.row(Style::new().w(grow()), |ui| {
                  ui.help("Errors per 100 words").text("WER 4.7%", s.underline(LinePattern::Dot));
                  ui.text(" · 274 MB", s);
                });
              };
              ui.form_row(&fs, FormRow::new(label).caption(caption), |ui| ui.button("Use"))
            }
            "Row tile" => {
              let row = FormRow::new("Data folder").caption("~/Library/Application Support");
              ui.form_row(&fs, row.tile(Icon::FOLDER), |ui| ui.button("Show"))
            }

            "Row footer" => {
              let (name, off) = (format!("Turn off below {level:.0}%"), !*on);
              let footer = |ui: &mut Ui, _: TextStyle| {
                let label = |v: f32| [10.0, 50.0, 90.0].contains(&v).then(|| format!("{v}%"));
                let s = Slider::new(level, 10.0..=90.0).step(10.0).tick_label(label);
                s.show(ui.disabled(off).a11y_label("Level"));
              };
              let row = FormRow::new(&name).caption("Only on battery.").footer(footer);
              ui.form_row(&fs, row, |ui| ui.toggle(on, ()))
            }
            "Disclosure" => {
              let row = FormRow::new("Other models").caption("2 more");
              ui.form_disclosure(&fs, row, |ui| {
                ui.form_row(&fs, FormRow::new("whisper-tiny"), |_| {})
              })
            }
            _ => ui.form_row(&fs, FormRow::new("Row"), |ui| ui.toggle(on, ())),
          };

          let page = Style::new().w(grow()).gap(10.0).pad(10.0).radius(12.0).bg(fs.bg());
          ui.col(page, |ui| match name {
            "Section" => {
              let head = |ui: &mut Ui, s: TextStyle| {
                ui.text("Storage", s);
                ui.spacer();
                ui.text("640 MB used", fs.note());
              };
              let section = Section::new(head).footer("Kept until you clear it.");
              _ = ui.form_section(&fs, section, |ui| row(ui, on, level));
            }

            "Toolbar tabs" => {
              let tabs =
                [Tag::new(0, "General").icon(Icon::GEAR), Tag::new(1, "About").icon(Icon::INFO)];
              _ = ui.toolbar_tabs(&fs, tab, &tabs);
            }
            "Footer" => {
              ui.form_section(&fs, "Section", |ui| row(ui, on, level));
              ui.help("Help on these settings").form_footer(&fs, |ui| _ = ui.button("Reset All…"));
            }
            _ => _ = ui.form_section(&fs, "", |ui| row(ui, on, level)),
          });
        });
      }
    }
  }
  mod layout {
    //! The layout blocks, built live: stacks, a grid, a spacer, dividers, a scroll view and an
    //! overlay.

    use nimbo::{Axes, Float, Style, Ui, UnitPoint, grow, px};

    use super::{Gallery, Shown, line, swatch};

    impl Gallery {
      /// The live layout block `name`.
      pub(super) fn layout(&mut self, ui: &mut Ui, name: &str) {
        let t = ui.theme();
        match name {
          "Stacks" => Self::variants(ui, Shown::Alone, |ui| {
            ui.row(Style::new().gap(8.0), |ui| (0..3).for_each(|_| _ = ui.el(swatch(t.accent()))));
            ui.col(Style::new().gap(4.0), |ui| (0..2).for_each(|_| _ = ui.el(swatch(t.raised()))));
          }),
          "Grid" => Self::variants(ui, Shown::Alone, |ui| {
            let grid = Style::new().w(px(120.0)).gap(6.0);
            ui.grid(3, grid, |ui| (0..6).for_each(|_| _ = ui.el(swatch(t.raised()))));
          }),

          "Spacer" => Self::variants(ui, Shown::Alone, |ui| {
            ui.row(Style::new().w(grow()), |ui| {
              ui.text("Left", t.label());
              ui.spacer();
              ui.text("Right", t.label());
            });
          }),

          "Divider" => Self::variants(ui, Shown::Alone, |ui| {
            ui.row(line(8.0).h(px(24.0)), |ui| {
              ui.text("One", t.label());
              ui.divider();
              ui.text("Two", t.label());
            });
            ui.col(Style::new().w(px(120.0)).gap(4.0), |ui| {
              ui.text("Above", t.label());
              ui.divider();
              ui.text("Below", t.label());
            });
          }),
          "Scroll view" => Self::variants(ui, Shown::Alone, |ui| {
            let area = Style::new().w(px(200.0)).h(px(60.0)).scroll(Axes::VERTICAL);
            ui.scroll_area(area, |ui| {
              (1..=8).for_each(|i| ui.text(format_args!("Line {i}"), t.label()))
            });
          }),

          // Opens at its end, and stays there as lines come.
          "Scroll anchor" => Self::variants(ui, Shown::Alone, |ui| {
            let area = Style::new().w(px(200.0)).h(px(60.0));
            ui.default_scroll_anchor(UnitPoint::BOTTOM).scroll_area(area, |ui| {
              (1..=8).for_each(|i| ui.text(format_args!("Line {i}"), t.label()))
            });
          }),

          "Overlay" => Self::variants(ui, Shown::Alone, |ui| {
            ui.row(swatch(t.raised()).w(px(80.0)).h(px(48.0)), |ui| {
              let at = UnitPoint::TOP_TRAILING;
              let dot = swatch(t.accent()).w(px(12.0)).h(px(12.0));
              ui.float(Float::parent().attach(at).anchor(at).offset(-4.0, 4.0)).el(dot);
            });
          }),
          _ => unreachable!("{name} is not a layout block"),
        }
      }
    }
  }
  mod list {
    //! The list blocks, built live: a list, its styles and one of the gallery's own, rows of the
    //! app's own over the list's behaviour, and sections.

    use nimbo::{
      Align, Id, InsetGroupedListStyle, List, ListConfiguration, ListRows, ListStyle,
      PlainListStyle, Row, Style, TextStyle, Ui, alpha, grow, px,
    };

    use super::{Gallery, Shown};

    /// A list style of the gallery's own: on a rounded panel, every other row tinted, the selected
    /// one a rounded tile of the accent, fainter while the list is not focused; faded when
    /// disabled.
    struct Stripes;

    impl ListStyle for Stripes {
      fn make_body<'a>(&self, ui: &mut Ui<'a>, mut c: ListConfiguration<'_, 'a>) {
        let t = ui.theme();
        let frame = c.frame().radius(8.0).bg(t.panel());
        let frame = if c.is_enabled() { frame } else { frame.opacity(t.disabled()) };
        ui.scroll_area(frame, |ui| {
          c.rows(ui, |ui, mut r| {
            let fill = match (r.is_selected(), r.index() % 2) {
              (true, _) => alpha(t.accent(), if r.is_focused() { 0.35 } else { 0.15 }),
              (false, 1) => alpha(t.text(), 0.04),
              _ => [0.0; 4],
            };

            ui.row(r.frame().radius(6.0).bg(fill), |ui| r.content(ui));
          })
        });
      }
    }

    impl Gallery {
      /// The live list block `name`.
      pub(super) fn list(&mut self, ui: &mut Ui, name: &str) {
        let t = ui.theme();
        match name {
          "List" => Self::variants(ui, Shown::Plain, |ui| {
            let list = List::new(40, 22.0).selection(&mut self.rows);
            let list = list.style(Style::new().w(px(220.0)).h(px(88.0)));
            list.show(ui, |ui, row| ui.text(format_args!("Row {}", row.index + 1), t.label()));
          }),

          // Plain, the app draws the selection: its text in the accent. The same rows in the
          // window's inset style beside it.
          "List style" => Self::variants(ui, Shown::Plain, |ui| {
            let frame = Style::new().w(px(160.0)).h(px(88.0));
            let text = |row: Row| match row.selected {
              true => TextStyle::new(t.font(), t.accent()),
              false => t.label(),
            };
            let [plain_rows, rows] = &mut self.styled_rows;
            let plain = List::new(40, 22.0).style(frame).selection(plain_rows);
            plain.show(ui.list_style(&PlainListStyle), |ui, row| {
              ui.text(format_args!("Row {}", row.index + 1), text(row));
            });

            let inset = List::new(40, 22.0).style(frame).selection(rows);
            inset.show(ui, |ui, row| ui.text(format_args!("Row {}", row.index + 1), t.label()));
          }),
          "Custom list style" => Self::variants(ui, Shown::Plain, |ui| {
            // Padded in the list's box, where its keys scroll the rows to.
            let list = List::new(40, 24.0).style(Style::new().w(px(220.0)).h(px(96.0)).pad(4.0));
            let list = list.selection(&mut self.styled_rows[1]);
            let ui = ui.list_style(&Stripes);
            list.show(ui, |ui, row| ui.text(format_args!("Row {}", row.index + 1), t.label()));
          }),

          // Rows of its own over the list's behaviour: the selected one filled with the accent.
          "List behavior" => Self::variants(ui, Shown::Alone, |ui| {
            let (id, key) = (Id::of("own list"), |i: usize| i as u64);
            let b = ui.list_behavior(id, ListRows::new(40, 22.0), &mut self.own_rows, &key);
            let sel = b.selection();

            // The box's focus and the rows built in view, their ids the list's.
            let style = Style::new().w(px(220.0)).h(px(96.0)).pad(4.0).radius(8.0).bg(t.panel());
            let style = b.style(style);
            ui.with_id(id).scroll_area(style, |ui| {
              ui.list_rows(&b, |ui, i, k| {
                let on = sel.contains(k);
                let row = Style::new().w(grow()).h(px(22.0)).pad_xy(8.0, 0.0).radius(5.0);
                let fill = if on { t.accent() } else { [0.0; 4] };
                let row = row.align(Align::Start, Align::Center).bg(fill);
                let text = TextStyle::new(t.font(), if on { t.on_accent() } else { t.text() });
                ui.with_id(id.key(("row", k)))
                  .row(row, |ui| ui.text(format_args!("Row {}", i + 1), text));
              });
            });
          }),

          "List sections" => Self::variants(ui, Shown::Plain, |ui| {
            let head = |ui: &mut Ui, g: usize| ui.text(["Today", "Yesterday"][g], t.section());
            let list = List::new(6, 30.0).sections(&[0, 3], 24.0, &head).selection(&mut self.rows);
            let list = list.style(Style::new().w(px(320.0)).h(px(290.0)));
            let ui = ui.list_style(&InsetGroupedListStyle);
            list
              .show(ui, |ui, row| ui.text(format_args!("Dictation {}", row.index + 1), t.label()));
          }),
          _ => unreachable!("{name} is not a list block"),
        }
      }
    }
  }
  mod modifiers {
    //! The modifier blocks, built live: disabled, control size, accessibility, keymaps, the
    //! environment, groups, identity, focus and the pointer, widget parts, frame requests and the
    //! theme.

    use nimbo::a11y::LabeledPairRole;
    use nimbo::{Color, Cursor, Enabled, EnvironmentValue, Icon, Id, Shortcut, Tone, Ui, px};

    use super::{Gallery, Shown, line, swatch};

    /// The environment block's value: the tint of the swatches built while it is there, the
    /// theme's raised colour by default.
    #[derive(Clone, Copy)]
    pub struct Tint(pub Color);

    impl EnvironmentValue for Tint {
      fn default_value(ui: &Ui) -> Self {
        Self(ui.theme().raised())
      }
    }

    impl Gallery {
      /// The live modifier block `name`.
      pub(super) fn modifiers(&mut self, ui: &mut Ui, name: &str) {
        let t = ui.theme();
        match name {
          "Disabled" => Self::variants(ui, Shown::Plain, |ui| {
            ui.button("Save");
            ui.toggle(&mut self.on, ());
          }),
          "Control size" => Self::variants(ui, Shown::Sized, |ui| {
            ui.button("Save");
            ui.toggle(&mut self.on, ());
            ui.badge("Beta", Tone::Neutral);
          }),
          "Accessibility label" => Self::variants(ui, Shown::Plain, |ui| {
            _ = ui.a11y_label("Reset").icon_button(Icon::RESET)
          }),

          // Swatches that take their colour from the environment, put for the row of the last two.
          "Environment" => Self::variants(ui, Shown::Alone, |ui| {
            let tile = |ui: &mut Ui| _ = ui.el(swatch(ui.environment_value::<Tint>().0));
            tile(ui);
            ui.environment(Tint(t.success())).row(line(10.0), |ui| (0..2).for_each(|_| tile(ui)));
          }),

          // Two widgets side by side disabled as one, the button after them enabled.
          "Group" => Self::variants(ui, Shown::Alone, |ui| {
            ui.disabled(true).group(|ui| {
              ui.button("Save");
              ui.toggle(&mut self.on, ());
            });
            ui.button("Open");
          }),

          // Keyed boxes: a key of its own each, the last a ready id.
          "Identity" => Self::variants(ui, Shown::Alone, |ui| {
            (0..3).for_each(|k| _ = ui.key(k).el(swatch(t.raised())));
            ui.with_id(Id::of("identity").key("box"))
              .allows_hit_testing(false)
              .el(swatch(t.accent()));
          }),

          // A swatch Tab focuses with the hand over it, and a focusable one Tab passes by.
          "Focus and pointer" => Self::variants(ui, Shown::Alone, |ui| {
            let id = Id::of("focus and pointer");
            ui.with_id(id).focusable(true).pointer_style(Cursor::Pointer).el(swatch(t.accent()));
            ui.with_id(id.key("not")).focusable(false).el(swatch(t.raised()).focusable());
          }),

          // A part enabled in a disabled container, with the tip the widget was given.
          "Widget parts" => Self::variants(ui, Shown::Plain, |ui| {
            ui.button("Save");
            let tip = ui.help("Always enabled").take_help();
            if let Some(tip) = &tip {
              ui.help(tip);
            }

            ui.environment(Enabled(true)).button("Undo");
          }),

          // A bar that moves every frame.
          "Frame request" => Self::variants(ui, Shown::Alone, |ui| {
            ui.request_frame();
            let k = ui.now().as_secs_f32() % 1.0;
            ui.el(swatch(t.accent()).w(px(28.0 + 120.0 * k)));
          }),

          // The slider named by the text before it; the badge by its own text.
          "Accessibility pair" => Self::variants(ui, Shown::Plain, |ui| {
            let pair = Id::of("volume");
            ui.a11y_labeled_pair(LabeledPairRole::Label, pair, |ui| ui.text("Volume", t.label()));
            ui.a11y_labeled_pair(LabeledPairRole::Content, pair, |ui| {
              _ = ui.slider(&mut self.level, 0.0..=100.0);
            });
            ui.badge("New", Tone::Accent);
          }),

          // The accent while the window is key, a grey while it isn't, as the switch beside it.
          "Controls theme" => Self::variants(ui, Shown::Alone, |ui| {
            let c = ui.controls_theme();
            ui.el(swatch(c.accent()));
            ui.toggle(&mut self.on, ());
          }),

          // Light or dark as drawn: the theme's when the app set one, else the OS's.
          "Color scheme" => Self::variants(ui, Shown::Alone, |ui| {
            let name = if ui.color_scheme().is_dark() { "Dark" } else { "Light" };
            ui.text(name, t.label());
          }),

          _ => Self::variants(ui, Shown::Plain, |ui| {
            let keys = Shortcut::char('z').cmd().label();
            ui.text(format_args!("{keys} undoes, in the window's keymap"), t.label());
          }),
        }
      }
    }
  }
  mod presentation {
    //! The presentation blocks, built live: an alert, a context menu, the menu bar, tooltips and a
    //! help style of the gallery's own.

    use nimbo::{
      Answer, ButtonRole, Dialog, HelpConfiguration, HelpStyle, Icon, Item, Menu, Style, TextStyle,
      Ui,
    };

    use super::{DIALOG, Gallery, Shown};

    /// The menu bar the "Menu bar" block's line makes.
    const MENU_BAR: &str = "File, Edit, View, Track, Window and Help in the menu bar (macOS)";

    /// What the context menu does.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Action {
      Copy,
      Delete,
    }

    /// A help style of the gallery's own: the tip on the accent in small white text.
    struct Note;

    impl HelpStyle for Note {
      fn make_body(&self, ui: &mut Ui, c: HelpConfiguration) {
        let t = ui.theme();
        let s = Style::new().pad_xy(8.0, 4.0).radius(6.0).bg(t.accent()).opacity(c.opacity());
        ui.row(s, |ui| ui.text(c.text(), TextStyle::new(t.small(), t.on_accent())));
      }
    }

    impl Gallery {
      /// The live presentation block `name`.
      pub(super) fn presentation(&mut self, ui: &mut Ui, name: &str) {
        let t = ui.theme();
        match name {
          "Alert" => {
            Self::variants(ui, Shown::Plain, |ui| {
              if ui.button("Delete…").clicked {
                self.ask = true;
              }

              ui.text(format_args!("Deleted {} times", self.deleted), t.muted().nowrap());
            });
            let d =
              Dialog::new("Delete the file?").message("It moves to the Trash.").icon(Icon::TRASH);
            let d = d.button("Delete", ButtonRole::Destructive);
            if d.show(ui.id(DIALOG), &mut self.ask) == Some(Answer::Action) {
              self.deleted += 1;
            }
          }
          "Context menu" => Self::variants(ui, Shown::Plain, |ui| {
            let r = ui.button("Right-click me");
            let menu = || {
              let delete = Item::button(Action::Delete, "Delete").role(ButtonRole::Destructive);
              Menu::new(vec![Item::button(Action::Copy, "Copy"), Item::Divider, delete])
            };
            if ui.context_menu(r.id, menu) == Some(Action::Copy) {
              self.copied += 1;
            }
          }),

          "Menu bar" => Self::variants(ui, Shown::Alone, |ui| ui.text(MENU_BAR, t.label())),
          "Tooltip" => {
            Self::variants(ui, Shown::Plain, |ui| _ = ui.help("Saves it").button("Save"))
          }
          // The window's tips in the gallery's style while the page shows.
          "Custom help style" => Self::variants(ui, Shown::Alone, |ui| {
            ui.help_style(&Note);
            ui.help("Saves it").button("Save");
            ui.help("Opens it").button("Open");
          }),
          _ => unreachable!("{name} is not a presentation block"),
        }
      }
    }
  }
  mod text {
    //! The text blocks, built live: text styles, localized lines, truncation, underlines,
    //! selectable text, a link, icons, a label's content and a photo.

    use nimbo::{
      Content, ContentMode, Icon, Id, LinePattern, Style, TextStyle, TruncationMode, Ui, px, t, tn,
    };

    use super::{Gallery, Shown};

    impl Gallery {
      /// The live text block `name`.
      pub(super) fn text(&mut self, ui: &mut Ui, name: &str) {
        let t = ui.theme();
        match name {
          "Text" => Self::variants(ui, Shown::Alone, |ui| {
            ui.text("Title", TextStyle::new(t.bold(), t.text()));
            ui.text("Body", t.label());
            ui.text("Secondary", t.muted());
          }),
          "Localized text" => Self::variants(ui, Shown::Alone, |ui| {
            let name = "parakeet";
            ui.col(Style::new().gap(4.0), |ui| {
              ui.text(t!("Keep the history"), t.label());
              ui.text(t!("{name} is not on disk", name), t.label());
              ui.text(tn!("{n} file", "{n} files", 3), t.muted());
            });
          }),

          "Line limit" => Self::variants(ui, Shown::Alone, |ui| {
            let path = "/Users/alex/Library/Application Support/Example/cache.db";
            ui.col(Style::new().w(px(300.0)).gap(4.0), |ui| {
              for mode in [TruncationMode::Head, TruncationMode::Middle, TruncationMode::Tail] {
                ui.text(path, t.body().truncation(mode));
              }
            });
          }),
          "Underline" => Self::variants(ui, Shown::Alone, |ui| {
            let patterns = [
              (LinePattern::Solid, "solid"),
              (LinePattern::Dot, "dot"),
              (LinePattern::Dash, "dash"),
              (LinePattern::DashDot, "dashDot"),
              (LinePattern::DashDotDot, "dashDotDot"),
            ];
            for (p, n) in patterns {
              ui.text(n, t.label().underline(p));
            }

            ui.text("struck", t.label().strikethrough(LinePattern::Solid));
          }),
          "Selectable text" => Self::variants(ui, Shown::Plain, |ui| {
            ui.col(Style::new().w(px(300.0)), |ui| {
              _ = ui.selectable_text("Select this text and copy it.", t.body())
            });
          }),
          "Link" => Self::variants(ui, Shown::Plain, |ui| _ = ui.link("Learn more…")),
          "Image" => Self::variants(ui, Shown::Alone, |ui| {
            for icon in [Icon::GEAR, Icon::FOLDER, Icon::TRASH, Icon::INFO] {
              ui.icon(icon.stroke(1.6), 20.0, t.text());
            }
          }),

          // What a label shows, a text and an icon, built in a text style as a look would.
          "Label content" => Self::variants(ui, Shown::Alone, |ui| {
            let id = Id::of("label content");
            let labels: [Content; 3] =
              [Icon::FOLDER.into(), "Data folder".into(), Icon::INFO.into()];
            for (k, label) in labels.into_iter().enumerate() {
              label.build(ui.with_id(id.key(k)), t.label());
            }
          }),

          "Photo" => Self::variants(ui, Shown::Alone, |ui| {
            // Stretched to its box, then in its aspect: filling a square, fitting it.
            if let Some(photo) = &self.photo {
              ui.image(photo, Style::new().w(px(120.0)).h(px(80.0)));
              let square = Style::new().w(px(80.0)).h(px(80.0)).bg(t.panel());
              ui.aspect_ratio(ContentMode::Fill).image(photo, square);
              ui.aspect_ratio(ContentMode::Fit).image(photo, square);
            }
          }),
          _ => unreachable!("{name} is not a text block"),
        }
      }
    }
  }

  use nimbo::{Align, Color, ControlSize, Style, Ui, grow, px};

  use super::{Block, DIALOG, Gallery, SIZES};

  /// The variants a block shows: itself alone (layout and text, which nothing disables); as it is
  /// and disabled; and at each size too.
  #[derive(Clone, Copy, PartialEq, Eq)]
  enum Shown {
    Alone,
    Plain,
    Sized,
  }

  /// A line of widgets, `gap` apart, centred on each other.
  fn line(gap: f32) -> Style {
    Style::new().gap(gap).align(Align::Start, Align::Center)
  }

  /// A square swatch of `c`.
  fn swatch(c: Color) -> Style {
    Style::new().w(px(28.0)).h(px(28.0)).radius(6.0).bg(c)
  }

  impl Gallery {
    /// Builds the block's variants with `f`: a labelled line each, keyed apart; disabled by
    /// [Ui::disabled] and sized by [Ui::control_size] on the line.
    fn variants(ui: &mut Ui, shown: Shown, mut f: impl FnMut(&mut Ui)) {
      let t = ui.theme();
      let sizes = [(ControlSize::Small, "Small"), (ControlSize::Large, "Large")];
      let sizes = if shown == Shown::Sized { &sizes[..] } else { &[] };
      if shown == Shown::Alone {
        ui.row(line(10.0).w(grow()), f);
        return;
      }

      ui.col(Style::new().w(grow()).gap(12.0), |ui| {
        let put = |ui: &mut Ui, k: &str, f: &mut dyn FnMut(&mut Ui)| {
          ui.key(k).row(line(12.0).w(grow()), |ui| {
            ui.col(Style::new().w(px(64.0)), |ui| ui.text(k, t.muted().nowrap()));
            ui.row(line(10.0).w(grow()), f);
          });
        };
        put(ui, "Enabled", &mut f);
        ui.disabled(true).col(Style::new().w(grow()), |ui| put(ui, "Disabled", &mut f));
        for &(size, k) in sizes {
          ui.control_size(size).col(Style::new().w(grow()), |ui| put(ui, k, &mut f));
        }
      });
    }

    /// The live block `b`, built by its group's module.
    pub(super) fn demo(&mut self, ui: &mut Ui, b: &Block) {
      match b.group {
        "Layout" => self.layout(ui, b.name),
        "Text" => self.text(ui, b.name),
        "Controls" => self.controls(ui, b.name),
        "Form" => self.form(ui, b.name),
        "List" => self.list(ui, b.name),
        "Containers" => self.containers(ui, b.name),
        "Presentation" => self.presentation(ui, b.name),
        // Modifiers, the last group.
        _ => self.modifiers(ui, b.name),
      }
    }
  }
}

pub mod canvas {
  //! The Canvas page, out of the table: drawing into the frame (shapes, text in fonts and
  //! scripts) and raw input (the pointer and keys as [App::event] gets them, before any widget).
  //! And the pacing scene, `gallery pace`, which `make pace` measures in a window.

  use std::f32::consts::TAU;
  use std::time::{Duration, Instant};

  use nimbo::{
    App, Axis, Color, Cx, Event, Font, Frame, Id, Point, Quad, Rect, Shortcut, Size, Style,
    TextStyle, Ui, Weight, alpha, grow, px, rgb,
  };

  use super::Gallery;

  /// The pad that shows the raw pointer and keys.
  pub const PAD: &str = "pad";
  const RED: Color = rgb(0xe5484d);
  const BLUE: Color = rgb(0x3b82f6);
  const GREEN: Color = rgb(0x30a46c);
  const AMBER: Color = rgb(0xf5a524);

  /// What the pad saw: where the pointer is, the presses and the last drag, the last event.
  #[derive(Default)]
  pub struct Pad {
    pub at: Option<Point>,
    pub presses: Vec<Point>,
    pub drag: Vec<Point>,
    pub held: bool,
    pub last: String,
  }

  /// Lines of text: sizes and weights, the monospaced font, then scripts.
  const LINES: [(&str, Font); 8] = [
    ("Regular 11 · The quick brown fox jumps over the lazy dog", Font::ui(11.0)),
    ("Medium 13 · The quick brown fox", Font::ui(13.0).weight(Weight::Medium)),
    ("Bold 20 · Hamburgefonstiv", Font::ui(20.0).weight(Weight::Bold)),
    ("Mono 13 · let x = [0O, 1lI];", Font::mono(13.0)),
    ("Héllo, naïve · Привет, мир · Γειά σου", Font::ui(15.0)),
    ("cafe\u{301} = café · e\u{301}\u{302} · Q\u{302}", Font::ui(15.0)),
    ("Emoji 🚀 🎨 👍🏽 ❤️", Font::ui(15.0)),
    ("漢字 かな カナ 한글", Font::ui(15.0)),
  ];

  impl Gallery {
    /// Raw input for the pad, on the Canvas page: the pointer over it or dragging from it, and
    /// any key no widget used.
    pub fn pad_event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
      let r = cx.rect(Id::of(PAD)).unwrap_or_default();
      let over = |p: Point| r.contains(p);
      let pad = &mut self.pad;
      match ev {
        Event::MouseMove { pos, .. } if pad.held || over(*pos) => {
          pad.at = Some(*pos);
          if pad.held {
            pad.drag.push(*pos);
          }
        }

        Event::MouseDown { pos, clicks, .. } if over(*pos) => {
          (pad.held, pad.drag) = (true, vec![*pos]);
          pad.presses = [&pad.presses[pad.presses.len().saturating_sub(11)..], &[*pos]].concat();
          pad.last = format!("press at {:.0}, {:.0} · click {clicks}", pos.x - r.x, pos.y - r.y);
        }

        Event::MouseUp { .. } if pad.held => pad.held = false,
        Event::KeyDown(k) => {
          let mut s = Shortcut::new(k.key);
          s.mods = k.mods;
          pad.last = format!("key {}{}", s.label(), if k.repeat { " · repeat" } else { "" });
        }

        _ => return false,
      }

      true
    }

    /// The Canvas page: shapes, text and the pad, each a canvas the page draws into.
    pub(super) fn canvas(&mut self, ui: &mut Ui) {
      let t = ui.theme();
      let (ink, card, edge, ground) = (t.text(), t.raised(), t.line(), t.bg());
      let title = TextStyle::new(t.bold().size(20.0), t.text());
      ui.col(Style::new().w(grow()).pad(20.0).gap(10.0), |ui| {
        ui.text("Canvas", title);
        ui.text(
          "ui.canvas(style, |f, r| …): a frame to draw into, at the element's rect",
          t.muted(),
        );
        ui.text("SHAPES", t.section());
        ui.canvas(Style::new().w(px(480.0)).h(px(96.0)), move |f, r| {
          shapes(f, r, [ink, card, edge, ground])
        });

        ui.text("TEXT", t.section());
        ui.canvas(Style::new().w(px(480.0)).h(px(208.0)), move |f, r| {
          let mut y = r.y;
          for (s, font) in LINES {
            y += f.metrics(font).ascent + 8.0;
            f.text(r.x, y, s, font, ink);
            y += f.metrics(font).descent;
          }
        });

        ui.text("POINTER AND KEYS", t.section());
        let pad = &self.pad;
        let (at, presses, drag) = (pad.at, pad.presses.clone(), pad.drag.clone());
        let accent = t.accent();
        ui.id(PAD).canvas(Style::new().w(px(480.0)).h(px(120.0)), move |f, r| {
          f.quad(Quad::new(r, card).radius(8.0).border(1.0, edge));
          f.push_clip(r);
          for (i, p) in presses.iter().enumerate() {
            let k = (i + 1) as f32 / presses.len() as f32;
            f.rect(Rect::new(p.x - 4.0, p.y - 4.0, 8.0, 8.0), alpha(accent, k), 4.0);
          }

          f.polyline(&drag, 2.0, accent);
          if let Some(p) = at {
            f.line(Point::new(r.x, p.y), Point::new(r.x + r.w, p.y), 1.0, alpha(ink, 0.3));
            f.line(Point::new(p.x, r.y), Point::new(p.x, r.y + r.h), 1.0, alpha(ink, 0.3));
          }

          f.pop_clip();
        });
        let last = if pad.last.is_empty() { "Move, press, drag or type" } else { &pad.last };
        ui.id(PAD.to_owned() + "-last").text(last, t.muted());
      });
    }
  }

  /// Rects plain and rounded, a bordered card, a shadowed one (on the opaque ground, as the
  /// raised colour lets a shadow through), gradients, lines, a clipped wave.
  fn shapes(f: &mut Frame, r: Rect, [ink, card, edge, ground]: [Color; 4]) {
    let at = |i: f32| Rect::new(r.x + 8.0 + i * 68.0, r.y + 8.0, 56.0, 56.0);
    f.rect(at(0.0), RED, 0.0);
    f.rect(at(1.0), BLUE, 16.0);
    f.quad(Quad::new(at(2.0), GREEN).radii([16.0, 0.0, 16.0, 0.0]));
    f.quad(Quad::new(at(3.0), card).radius(10.0).border(2.0, edge));
    f.shadow(Rect { y: at(4.0).y + 4.0, ..at(4.0) }, 10.0, 10.0, [0.0, 0.0, 0.0, 0.4]);
    f.quad(Quad::new(at(4.0), ground).radius(10.0).borders([0.0, 0.0, 0.0, 4.0], AMBER));
    f.quad(Quad::new(at(5.0), BLUE).radius(10.0).gradient(GREEN, Axis::Vertical));
    f.quad(Quad::new(at(6.0), RED).radius(10.0).gradient(alpha(RED, 0.0), Axis::Horizontal));
    for (i, w) in [0.5, 1.0, 2.0, 4.0].into_iter().enumerate() {
      let y = r.y + 76.0 + i as f32 * 5.0;
      f.line(Point::new(r.x + 8.0, y), Point::new(r.x + 196.0, y), w, ink);
    }

    let panel = Rect::new(r.x + 212.0, r.y + 70.0, 260.0, 24.0);
    f.quad(Quad::new(panel, card).border(1.0, edge));
    f.push_clip(Rect::new(panel.x + 1.0, panel.y + 1.0, panel.w - 2.0, panel.h - 2.0));
    let wave: Vec<Point> = (0..=48)
      .map(|i| {
        let k = i as f32 / 48.0;
        Point::new(panel.x + k * panel.w, panel.y + 12.0 - 18.0 * (k * 3.0 * TAU).sin())
      })
      .collect();
    f.polyline(&wave, 2.0, AMBER);
    f.pop_clip();
  }

  pub const PACE_SIZE: Size = Size::new(320.0, 80.0);
  /// How long the bar moves, from the first frame.
  pub const RUN: Duration = Duration::from_secs(3);
  /// The CPU time each moving frame spends before it paints.
  pub const WORK: Duration = Duration::from_millis(8);
  /// The bar's width; it crosses the window once a second.
  pub const BAR_W: f32 = 40.0;

  /// The pacing scene: a bar slides for [RUN], each frame spending [WORK] of CPU time first, then
  /// all stands still and the loop sleeps. Paced from their start, frames come once per refresh
  /// (60 fps at 60 Hz); from their end, every 16.7 + 8 ms (about 40).
  #[derive(Default)]
  pub struct Pace {
    /// The first frame's time.
    start: Option<Duration>,
    /// Frames painted while moving.
    pub frames: usize,
  }

  impl App for Pace {
    fn paint(&mut self, f: &mut Frame) {
      let start = *self.start.get_or_insert(f.now());
      let t = f.now().saturating_sub(start);
      if t < RUN {
        let busy = Instant::now();
        while busy.elapsed() < WORK {
          std::hint::spin_loop();
        }

        self.frames += 1;
        f.animate();
      }

      f.rect(Rect::new(0.0, 0.0, f.size().w, f.size().h), rgb(0x1c1c1e), 0.0);
      let x = t.min(RUN).as_secs_f32().fract() * (f.size().w - BAR_W);
      f.rect(Rect::new(x, 0.0, BAR_W, f.size().h), BLUE, 0.0);
    }
  }
}

use nimbo::{
  App, Bitmap, CMD, Config, Cx, Event, Font, HelpStyle, Hotkey, Id, Key, List, Selection, Style,
  Tag, TextStyle, Ui, grow, px,
};

pub use self::blocks::{BLOCKS, Block, GROUPS};
pub use self::canvas::{PACE_SIZE, PAD, Pace, Pad};

/// The Canvas page's index, after the table's blocks: out of the table and `docs/api.md`.
pub const CANVAS: usize = BLOCKS.len();

pub const SIDEBAR: &str = "sidebar";
pub const PAGE: &str = "page";
pub const DIALOG: &str = "dialog";
/// A block alone ([Gallery::only]).
pub const BLOCK: &str = "block";
/// The width of a page, and of a block's picture.
pub const WIDTH: f32 = 520.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
  Small,
  Medium,
  Large,
}

pub const SIZES: [Tag<Size>; 3] = [
  Tag::new(Size::Small, "Small"),
  Tag::new(Size::Medium, "Medium"),
  Tag::new(Size::Large, "Large"),
];

pub struct Gallery {
  /// The block shown, an index into [BLOCKS], or [CANVAS].
  pub page: usize,
  pub sel: Selection,
  /// One block alone, as high as its variants: a picture for `docs/api.md`.
  pub only: bool,
  pub live: bool,
  pub wrap: bool,
  pub numbers: bool,
  pub bold: bool,
  pub italic: bool,
  pub zoom: f32,
  pub level: f32,
  pub size: Size,
  pub view: u8,
  pub name: String,
  pub key: String,
  pub query: String,
  /// The text field blocks' others, empty at first.
  pub notes: String,
  pub city: String,
  pub email: String,
  pub hotkey: Option<Hotkey>,
  pub tab: usize,
  pub side: f32,
  /// The toolbar's sidebar share.
  pub bar_side: f32,
  pub rows: Selection,
  /// The list behaviour block's, its second row selected at first.
  pub own_rows: Selection,
  /// The list style blocks' plain and styled lists', their second row selected at first.
  pub styled_rows: [Selection; 2],
  pub ask: bool,
  pub deleted: usize,
  pub copied: usize,
  pub on: bool,
  /// The photo block's, decoded by the OS at twice its size in points.
  pub photo: Option<Bitmap>,
  /// The Canvas page's raw input.
  pub pad: Pad,
}

/// The localized text block's Russian (`--locale ru`).
pub const RU: &str = r#"
msgid ""
msgstr "Plural-Forms: nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);\n"

msgid "Keep the history"
msgstr "Хранить историю"

msgid "{name} is not on disk"
msgstr "{name} нет на диске"

msgid "{n} file"
msgid_plural "{n} files"
msgstr[0] "{n} файл"
msgstr[1] "{n} файла"
msgstr[2] "{n} файлов"
"#;

/// A sunset drawn for the photo block, 240 × 160.
pub const SUNSET: &[u8] = include_bytes!("../tests/images/sunset.jpg");

/// A selection of the second row.
fn second() -> Selection {
  let mut s = Selection::default();
  s.select(1, 1);
  s
}

impl Default for Gallery {
  fn default() -> Self {
    Self {
      page: 0,
      sel: Selection::default(),
      only: false,
      live: true,
      wrap: false,
      numbers: true,
      bold: true,
      italic: false,
      zoom: 60.0,
      level: 30.0,
      size: Size::Medium,
      view: 0,
      name: "Alex".into(),
      key: "secret".into(),
      query: "Switches".into(),
      notes: String::new(),
      city: String::new(),
      email: String::new(),
      hotkey: Hotkey::parse("ctrl+option+space").ok(),
      tab: 0,
      side: 0.4,
      bar_side: 0.4,
      rows: Selection::default(),
      own_rows: second(),
      styled_rows: [second(), second()],
      ask: false,
      deleted: 0,
      copied: 0,
      on: true,
      photo: Bitmap::decode(SUNSET, 240).ok(),
      pad: Pad::default(),
    }
  }
}

impl Gallery {
  /// The gallery on `page`, alone when `only`.
  pub fn on(page: usize, only: bool) -> Self {
    Self { page, only, ..Self::default() }
  }

  /// The page of the block shown: its name, SwiftUI's and nimbo's lines, then the block live.
  fn page(&mut self, ui: &mut Ui) {
    if self.page == CANVAS {
      ui.id(PAGE).scroll_area(Style::new().grow(), |ui| self.canvas(ui));
      return;
    }

    let (t, block) = (ui.theme(), &BLOCKS[self.page]);
    let mono = TextStyle::new(Font::mono(12.0), t.text());
    let code = Style::new().w(grow()).pad(10.0).gap(4.0).radius(8.0).bg(t.raised());
    ui.id(PAGE).scroll_area(Style::new().grow(), |ui| {
      ui.col(Style::new().w(grow()).pad(20.0).gap(14.0), |ui| {
        ui.text(block.name, TextStyle::new(t.bold().size(20.0), t.text()));
        ui.col(code, |ui| {
          ui.text("SWIFTUI", t.section());
          ui.selectable_text(block.swiftui, mono);
          ui.text("NIMBO", t.section());
          ui.selectable_text(block.nimbo, mono);
        });
        self.demo(ui, block);
      });
    });
  }

  /// The sidebar: every block by group, then the Canvas page; the one shown selected.
  fn sidebar(&mut self, ui: &mut Ui) {
    let t = ui.theme();
    let mut starts: Vec<usize> =
      GROUPS.iter().filter_map(|g| BLOCKS.iter().position(|b| b.group == *g)).collect();
    starts.push(CANVAS);
    let head =
      |ui: &mut Ui, g: usize| ui.text(GROUPS.get(g).copied().unwrap_or("Drawing"), t.section());
    let list = List::new(CANVAS + 1, 24.0).sections(&starts, 26.0, &head);
    let list =
      list.selection(&mut self.sel).style(Style::new().w(px(200.0)).h(grow()).bg(t.panel()));
    list.show(ui.id(SIDEBAR), |ui, row| {
      ui.text(BLOCKS.get(row.index).map_or("Canvas", |b| b.name), t.label())
    });
    if let Some(i) = self.sel.cursor_index(CANVAS + 1, &|i| i as u64) {
      self.page = i;
    }
  }
}

impl App for Gallery {
  fn event(&mut self, cx: &mut Cx, ev: &Event) -> bool {
    self.page == CANVAS && self.pad_event(cx, ev)
  }

  fn view(&mut self, ui: &mut Ui) {
    // The system's look and the shell's tips, whatever a block set last frame.
    ui.follow_system();
    ui.set_environment::<&dyn HelpStyle>(None);
    let t = ui.theme();
    if self.only {
      ui.fit_window_height(WIDTH, 1.0, 900.0);
      let block = &BLOCKS[self.page];
      ui.id(BLOCK).col(Style::new().w(grow()).pad(20.0).bg(t.bg()), |ui| self.demo(ui, block));
      return;
    }

    if self.sel.is_empty() {
      self.sel.select(self.page as u64, self.page);
    }

    ui.row(Style::new().grow().bg(t.bg()), |ui| {
      self.sidebar(ui);
      ui.col(Style::new().grow(), |ui| self.page(ui));
    });
  }
}

/// Where `docs/api.md`'s pictures are: the repo's `assets` branch, which `make api-shots` fills.
pub const PICTURES: &str = "https://raw.githubusercontent.com/vladkens/nimbo/assets/api";

/// `docs/api.md`: per group, each block's SwiftUI line, nimbo's and its picture.
pub fn api_md() -> String {
  let mut out = String::from(
    "# nimbo and SwiftUI\n\n\
     Each building block of nimbo beside its SwiftUI counterpart, as the gallery shows it live \
     (`cargo run --release --example gallery`, a page per block in its variants). A block aims \
     at its SwiftUI counterpart's behaviour and API shape without copying it 1:1. The pictures \
     are the gallery's, in the light appearance, kept on the repo's `assets` branch: `make \
     api-shots` makes them and this file again from the gallery's table \
     (`examples/gallery.rs`), so a block changes there.\n\n\
     On macOS SwiftUI's `List` has neither of the grouped styles (`InsetGroupedListStyle` and \
     `GroupedListStyle` are iOS's): nimbo's `InsetGroupedListStyle` takes the iOS name for the \
     plates of the grouped `Form` (`.formStyle(.grouped)`), which its sections share.\n",
  );
  for g in GROUPS {
    out += &format!("\n## {g}\n");
    for b in BLOCKS.iter().filter(|b| b.group == g) {
      let (name, slug) = (b.name, b.slug());
      out += &format!(
        "\n### {name}\n\nSwiftUI: `{}`\n\nnimbo: `{}`\n\n![{name}]({PICTURES}/{slug}.png)\n",
        b.swiftui, b.nimbo
      );
    }
  }

  out
}

fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  match args.first().map(String::as_str) {
    Some("--api") => return print!("{}", api_md()),
    Some("--list") => return BLOCKS.iter().for_each(|b| println!("{}", b.slug())),
    _ => {}
  }

  // The pacing scene alone, for `make pace`.
  if args.first().is_some_and(|a| a == "pace") {
    let cfg = Config::new("pace", PACE_SIZE.w, PACE_SIZE.h).fixed();
    return example::run(cfg, Pace::default(), &args[1..], |_| {});
  }

  // A block's slug first: that block alone; `canvas`: the gallery on the Canvas page.
  let first = args.first().map(String::as_str);
  let page = first.and_then(|a| BLOCKS.iter().position(|b| b.slug() == a));
  let canvas = first == Some("canvas");
  let rest = &args[(page.is_some() || canvas) as usize..];
  let app = Gallery::on(if canvas { CANVAS } else { page.unwrap_or(0) }, page.is_some());
  let (w, h) = if page.is_some() { (WIDTH, 300.0) } else { (WIDTH + 200.0, 600.0) };
  let cfg = Config::new("Gallery", w, h).localizations(&[("ru", RU)]);
  // The Canvas page's shot: a few presses, a drag across the pad and a key.
  example::run(cfg, app, rest, |h| {
    h.frame();
    let Some(r) = h.rect(Id::of(PAD)).filter(|_| canvas) else { return };
    for (x, y) in [(40.0, 30.0), (90.0, 80.0), (150.0, 40.0)] {
      h.click(r.x + x, r.y + y);
    }

    h.press(r.x + 200.0, r.y + 90.0);
    for i in 1..=24 {
      let k = i as f32 / 24.0;
      h.move_to(r.x + 200.0 + 220.0 * k, r.y + 60.0 + 30.0 * (k * 6.0).cos());
    }

    h.release(r.x + 420.0, r.y + 60.0 + 30.0 * 6f32.cos());
    h.move_to(r.x + 330.0, r.y + 40.0);
    h.key(Key::Char('k'), CMD);
    h.frame();
  });
}
