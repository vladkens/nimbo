// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The container blocks, built live: tabs, splits, toolbars, a menu, a lazy column, scrolling of
//! the app's own and stack queries.

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
            let ground = if pane == 0 { t.panel } else { t.bg };
            ui.col(Style::new().grow().pad(8.0).bg(ground), |ui| {
              ui.text(["Sidebar", "Detail"][pane], t.label())
            });
          });
        });
      }),

      // In a window's title bar on macOS ([nimbo::Config::unified]: `examples/files`); here in
      // a bar of its own, a sidebar's column with its toggle beside the detail's with its action.
      "Toolbar" => Self::variants(ui, Shown::Plain, |ui| {
        let bar = Style::new().w(px(360.0)).h(px(52.0)).radius(8.0).pad(1.0).clip().bg(t.bg);
        ui.row(bar.border(1.0, t.line), |ui| {
          Split::navigation().ratio(&mut self.bar_side).show(ui, |ui, pane| {
            let ground = if pane == 0 { t.panel } else { t.bg };
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
        let band = Style::new().w(px(360.0)).h(px(52.0)).radius(8.0).pad_xy(12.0, 0.0).bg(t.panel);
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
          ui.virtual_col((0..1000).map(|_| 20.0), |i| i, item);
        });
      }),

      // A bar of its own over the area's behaviour: a thin accent thumb, always shown.
      "Scroll behavior" => Self::variants(ui, Shown::Alone, |ui| {
        let id = Id::of("own scroll");
        let b = ui.scroll_behavior(id, Axes::VERTICAL);
        let area = Style::new().w(px(200.0)).h(px(60.0)).pad_xy(8.0, 4.0).radius(6.0).bg(t.panel);
        let area = area.scroll(Axes::VERTICAL);
        ui.with_id(id).col(area, |ui| {
          (1..=8).for_each(|i| ui.text(format_args!("Line {i}"), t.label()));
          let Some(bar) = b.bar(Axis::Vertical) else { return };
          let at = Float::parent().attach(UnitPoint::TOP_TRAILING).anchor(UnitPoint::TOP_TRAILING);
          let at = at.offset(-ScrollBar::INSET, ScrollBar::INSET + bar.pos);
          let thumb = Style::new().w(px(4.0)).h(px(bar.thumb)).radius(2.0).bg(t.accent);
          ui.with_id(b.thumb(Axis::Vertical)).float(at).el(thumb);
        });
      }),

      // Red knobs in this area, put back after it for the gallery's other areas.
      "Scroll indicators" => Self::variants(ui, Shown::Alone, |ui| {
        let red = ScrollerKnob { color: alpha(t.danger, 0.6), ..ScrollerKnob::DARK };
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
