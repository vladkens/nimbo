// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Settings windows in macOS 26 System Settings' look ([FormStyle::plates], measured from
//! its screenshots): toolbar tabs of an icon over a label ([Ui::toolbar_tabs]), and pages
//! of sections, each a bold title (and an optional grey line under it) over a grey rounded plate
//! of rows. A row is a label with an optional ⓘ tooltip and a grey description under it at the
//! left, the controls at the right, a leading tile (a letter or an icon) when it lists things,
//! and a hairline above it inset from both sides of the plate. A footer row under the last plate
//! holds the page's buttons and a round "?" that opens its help ([Ui::form_footer],
//! [Ui::form_help]). Rows pitch 37.5 points, 53 with a one-line description, over
//! [super::Sizes::PLATES]; draw it with [super::Theme::PLATES_LIGHT] or
//! [super::Theme::PLATES_DARK], or the OS's of the two ([super::Theme::system], the default).
//!
//! The controls keep their own width ([super::Dim::Rigid]) and sit at the row's right edge
//! whatever is beside them, centred on the row's height; the label and its description
//! take the rest and wrap onto as many lines as they need, the row growing taller, as System
//! Settings does.
//!
//! ```ignore
//! let fs = FormStyle::plates(&ui.theme());
//! ui.set_window_size(680.0, self.page.height());
//! ui.id("tabs").toolbar_tabs(&fs, &mut self.page, &[(Page::General, Icon::GEAR, "General"), ..]);
//! ui.form_scroll(&fs, Style::new().grow(), |ui| ui.col(fs.page, |ui| {
//!   ui.form_section(&fs, "Startup", |ui| {
//!     ui.form_row(&fs, FormRow::new("On launch").info("What opens when the app starts…"), |ui| {
//!       ui.id("launch").select(&mut self.launch, &LAUNCH);
//!     });
//!     let row = FormRow::new("Language").caption("Applies after a restart.");
//!     ui.form_row(&fs, row, |ui| ui.select(&mut self.lang, &LANGS));
//!   });
//!   ui.form_help(&fs);
//! }));
//! ```
//!
//! A disabled row dims its label and disables every widget in it ([Ui::disabled_scope]). The
//! toolbar is a pick ([Ui::pick_behavior]): a click, or the arrows while it has focus (Tab), choose
//! a tab.

use std::ops::Range;

use super::containers::lazy::Spans;
use super::theme::Theme;
use super::widgets::Icon;
use super::{
  Align, Dim, Float, Id, Response, Shadow, State, Style, TextStyle, Ui, grow, px, rigid,
};
use crate::scene::{Color, Frame, Quad, Rect, alpha, mix, rgb};
use crate::text::{Font, Weight};

/// A settings window's look; see the module's docs. Plain values, so an app adjusts one:
/// `FormStyle { inset: [0.0, 0.0], ..FormStyle::plates(t) }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormStyle {
  /// A section's title, and the note at its right ("4 files · 2.2 GB").
  pub head: TextStyle,
  pub note: TextStyle,
  /// Around a section's title: top, right, bottom, left; in by the row's padding, so the title
  /// lines up with the labels. The page's first section has no top.
  pub head_pad: [f32; 4],
  /// A grey line under a section's title ([Ui::form_section_sub]), this far below it.
  pub subtitle: TextStyle,
  pub subtitle_gap: f32,
  /// A section's plate of rows: fill and radius.
  pub group: Style,
  /// A row's box: least height (of a first row; the others add the hairline), padding at the
  /// sides, gap between the tile, the label and the controls.
  pub row: Style,
  /// Above and below the label and its description; the controls centre on the label's first
  /// line, as far from the row's top.
  pub pad_y: f32,
  /// Between the controls at a row's right.
  pub gap: f32,
  /// The hairline above every row but the first: its colour and thickness, in from the plate's
  /// left and right.
  pub divider: Color,
  pub hairline: f32,
  pub inset: [f32; 2],
  /// The label; it wraps unless the row is [FormRow::one_line].
  pub label: TextStyle,
  /// The grey description under a label; it wraps.
  pub caption: TextStyle,
  /// A row's leading tile: a letter or an icon, and their look; the tile is
  /// centred in the row, [FormStyle::tile_x] in from the plate.
  pub tile: Style,
  pub tile_x: f32,
  pub tile_text: TextStyle,
  pub tile_icon: Color,
  /// The page under the toolbar: padding and the gap between sections.
  pub page: Style,
  /// The toolbar of tabs, and a tab: its box, label, icon size and stroke, and the selected
  /// one: the tab on a raised tile (fill, hairline, shadow), or on a glass one when
  /// `tab_glass` is set, its icon and label in the accent. A grey tile under the pointer. Under
  /// a title bar the app draws ([crate::Config::unified]) the toolbar starts with the title,
  /// centred in a line as high as the title bar, and the tabs right under it, without the top
  /// padding.
  pub toolbar: Style,
  pub title: TextStyle,
  pub tab: Style,
  pub tab_text: TextStyle,
  pub tab_icon: f32,
  pub tab_stroke: f32,
  pub tab_on: Style,
  pub tab_glass: Option<Glass>,
  /// The footer under the last plate ([Ui::form_footer]): its box, right-aligned buttons and
  /// their gap.
  pub footer: Style,
  /// The ⓘ after a label ([FormRow::info]): its size, as [Ui::info_button] draws it.
  pub info: f32,
  /// A selected row's fill ([FormRow::selected]).
  pub selected: Color,
  /// The window's ground, under the page.
  pub bg: Color,
  /// How far the page's content fades out under the edges of its scroll area
  /// ([Ui::form_scroll], [Ui::fade_edges]); 0 for none.
  pub fade: f32,
}

/// The selected toolbar tab as macOS 26's Liquid Glass in dark ([Theme::glass]), measured
/// from an AppKit preferences window (Shottr's, at 2x over a `#1e1e1e` bar): a tile filled
/// `#1a1a1a`; its rim lit to `#595959` along the top and bottom edges, a point thick, the light
/// fading round the corners into a half-point `#060606` down the sides; inside the lit edges a
/// glow from `#282828` that fades into the fill within 6 points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glass {
  /// The bar under the tile.
  pub ground: Color,
  /// The tile inside its rim, opaque.
  pub fill: Color,
  /// The rim down the sides.
  pub rim: Color,
  /// The light along the top and bottom edges and the glow inside them, at their brightest.
  pub light: Color,
  pub glow: Color,
}

impl Glass {
  /// Draws the tile in `r`, its corners `radius`, at `opacity`. The light and the glow are the
  /// soft shadows of strips beyond the top and bottom edges, so they fade round the corners and
  /// down the sides; the tile's rect clips them and a ring of the ground clears its corners.
  pub fn paint(&self, f: &mut Frame, r: Rect, radius: f32, opacity: f32) {
    let o = |c: Color| alpha(c, c[3] * opacity);
    let edges = |f: &mut Frame, r: Rect, inset: f32, blur: f32, c: Color| {
      let (x, w) = (r.x + inset, r.w - 2.0 * inset);
      f.quad(Quad::new(Rect::new(x, r.y - 10.0, w, 10.0), c).blur(blur));
      f.quad(Quad::new(Rect::new(x, r.y + r.h, w, 10.0), c).blur(blur));
    };
    f.push_clip(r);
    f.quad(Quad::new(r, o(self.rim)).radius(radius));
    edges(f, r, 2.5, 6.0, o(self.light));

    let inner = Rect::new(r.x + 0.5, r.y + 0.75, r.w - 1.0, r.h - 1.5);
    let fill = mix(self.ground, self.fill, opacity);
    f.quad(Quad::new(inner, fill).radius(radius - 0.6));
    edges(f, inner, radius, 6.0, o(self.glow));

    let m = 4.0;
    let ring = Rect::new(r.x - m, r.y - m, r.w + 2.0 * m, r.h + 2.0 * m);
    f.quad(Quad::new(ring, [0.0; 4]).radius(radius + m).border(m, self.ground));
    f.pop_clip();
  }
}

const WHITE: Color = [1.0; 4];
const BLACK: Color = [0.0, 0.0, 0.0, 1.0];

impl FormStyle {
  /// macOS 26 System Settings' plates under toolbar tabs, as measured:
  /// bold 13-point titles 10 over plates of `#f8f8f8` (radius 12, no border), 10 apart, 30
  /// from the plate above to the next title; rows padded 10 at the sides and around the label,
  /// at least 36.5 high (37.5 with the hairline), 52 with a one-line 11-point description;
  /// hairlines of black 4% (white 5%) inset 10; a 24-point tile at 12 with the label at 48; a
  /// footer 20 under the last plate, and 20 under the page. The toolbar as an AppKit preferences
  /// window's (Shottr's, macOS 26): the title bold 13 in the labels' grey, centred in the title
  /// bar; tabs 48 high and at least 56 wide, right under the title bar (8 under the window's own)
  /// and 8 over a half-point line, half a point apart; a 26-point icon 3 from the tab's top, the
  /// label 11-point medium (AppKit's labels read that heavy) with its baseline at 40.5; the
  /// selected tab on [Glass] in dark ([Theme::glass]).
  pub const fn plates(t: &Theme) -> Self {
    let h = t.sizes.control_h;
    let small = t.small;
    let tab = Style::new()
      .min_w(56.0)
      .h(px(48.0))
      .pads([3.0, 7.0, 0.0, 7.0])
      .align(Align::Center, Align::Start)
      .gap(0.5)
      .radius(8.0);
    let lift = Shadow::new(1.0, 2.5, alpha(t.shadow, t.shadow[3] * 0.7));
    let (ink, rim) = if t.dark { (WHITE, 0.12) } else { (BLACK, 0.11) };
    let glass = Glass {
      ground: t.bg,
      fill: mix(t.bg, BLACK, 0.13),
      rim: alpha(BLACK, 0.8),
      light: alpha(WHITE, 0.6),
      glow: alpha(WHITE, 0.13),
    };
    let (tab_on, tab_glass) = match t.glass && t.dark {
      true => (tab, Some(glass)),
      false => (tab.bg(t.tab_tile()).border(0.5, alpha(ink, rim)).shadow(lift), None),
    };

    let (divider, toolbar_line) = match t.dark {
      true => (alpha(WHITE, 0.05), rgb(0x323232)),
      false => (alpha(BLACK, 0.04), rgb(0xe8e8e8)),
    };

    Self {
      head: TextStyle::new(t.bold, t.text).ellipsis(),
      note: TextStyle::new(small, t.muted).nowrap(),
      head_pad: [20.0, 10.0, 10.0, 10.0],
      subtitle: TextStyle::new(small, t.muted).line_h(14.0),
      subtitle_gap: 1.5,
      group: Style::new().w(grow()).bg(t.plate).radius(12.0),
      row: Style::new().w(grow()).min_h(36.5).pad_xy(10.0, 0.0).gap(10.0),
      pad_y: 10.0,
      gap: 8.0,
      divider,
      hairline: 1.0,
      inset: [10.0, 10.0],
      label: TextStyle::new(t.font, t.text).line_h(16.0),
      caption: TextStyle::new(small, t.muted).line_h(14.0),
      tile: Style::new().w(px(h)).h(px(h)).center().radius(6.0).bg(t.raised),
      tile_x: 12.0,
      tile_text: TextStyle::new(Font { size: 11.0, ..t.font }.weight(Weight::Semibold), t.muted)
        .nowrap(),
      tile_icon: t.muted,
      page: Style::new().w(grow()).pads([12.0, 20.0, 20.0, 20.0]).gap(10.0),
      toolbar: Style::new()
        .w(grow())
        .pads([8.0, 0.0, 8.5, 0.0])
        .gap(0.5)
        .center()
        .bg(t.bg)
        .borders([0.0, 0.0, 0.5, 0.0], toolbar_line),
      title: TextStyle::new(t.bold, t.muted).nowrap(),
      tab,
      tab_text: TextStyle::new(Font { size: 11.0, ..t.font }.weight(Weight::Medium), t.muted)
        .nowrap(),
      tab_icon: 26.0,
      tab_stroke: 1.4,
      tab_on,
      tab_glass,
      footer: Style::new()
        .w(grow())
        .pads([10.0, 0.0, 0.0, 0.0])
        .gap(10.0)
        .align(Align::End, Align::Center),
      info: (t.font.size + 3.0).round(),
      selected: t.ghost,
      bg: t.bg,
      fade: 14.0,
    }
  }
}

/// A row's leading tile ([FormRow::tile]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tile<'s> {
  /// A letter or two: an account's initials.
  Text(&'s str),
  Icon(Icon),
}

/// A row of a form section ([Ui::form_row]): its label, an optional ⓘ tooltip, description and
/// leading tile, and whether it is disabled.
#[derive(Clone, Copy, Debug, Default)]
pub struct FormRow<'s> {
  label: &'s str,
  info: Option<&'s str>,
  caption: Option<&'s str>,
  tile: Option<Tile<'s>>,
  disabled: bool,
  one_line: bool,
  selected: bool,
}

impl<'s> FormRow<'s> {
  pub fn new(label: &'s str) -> Self {
    Self { label, ..Self::default() }
  }

  /// An ⓘ after the label showing `tip` on hover: only where one line can't explain.
  pub fn info(self, tip: &'s str) -> Self {
    Self { info: Some(tip), ..self }
  }

  /// Grey text under the label, wrapping onto as many lines as it needs.
  pub fn caption(self, caption: &'s str) -> Self {
    Self { caption: Some(caption), ..self }
  }

  /// A tile before the label: a letter or an icon.
  pub fn tile(self, tile: Tile<'s>) -> Self {
    Self { tile: Some(tile), ..self }
  }

  /// Dims the label and disables the row's widgets when `yes`.
  pub fn disabled(self, yes: bool) -> Self {
    Self { disabled: yes, ..self }
  }

  /// The label stays on one line, ending in "…" where it doesn't fit (a file's path).
  pub fn one_line(self) -> Self {
    Self { one_line: true, ..self }
  }

  /// Fills the row ([FormStyle::selected]) and reads it selected when `yes`: the current one of
  /// rows a click selects.
  pub fn selected(self, yes: bool) -> Self {
    Self { selected: yes, ..self }
  }
}

impl Ui<'_> {
  /// The page's scroll area ([Ui::scroll_area] in `style`), its content fading out under the
  /// edges into the ground ([FormStyle::fade]).
  pub fn form_scroll<R>(
    &mut self,
    fs: &FormStyle,
    style: Style,
    f: impl FnOnce(&mut Self) -> R,
  ) -> Response {
    if fs.fade > 0.0 {
      self.fade_edges(fs.bg, fs.fade);
    }

    self.scroll_area(style, f)
  }

  /// A titled plate of rows ([Ui::form_row]) built by `rows`; an empty title shows none.
  pub fn form_section<R>(
    &mut self,
    fs: &FormStyle,
    title: &str,
    rows: impl FnOnce(&mut Self) -> R,
  ) -> Response {
    self.form_section_with(fs, title, |_| {}, rows)
  }

  /// [Ui::form_section] with what `note` builds at the right of the title: a count in
  /// [FormStyle::note], a spinner.
  pub fn form_section_with<R>(
    &mut self,
    fs: &FormStyle,
    title: &str,
    note: impl FnOnce(&mut Self),
    rows: impl FnOnce(&mut Self) -> R,
  ) -> Response {
    self.section(fs, title, "", note, rows)
  }

  /// [Ui::form_section] with a grey line under the title ([FormStyle::subtitle]): what the
  /// section's settings do, as System Settings' Notifications explains its own.
  pub fn form_section_sub<R>(
    &mut self,
    fs: &FormStyle,
    title: &str,
    subtitle: &str,
    rows: impl FnOnce(&mut Self) -> R,
  ) -> Response {
    self.section(fs, title, subtitle, |_| {}, rows)
  }

  fn section<R>(
    &mut self,
    fs: &FormStyle,
    title: &str,
    subtitle: &str,
    note: impl FnOnce(&mut Self),
    rows: impl FnOnce(&mut Self) -> R,
  ) -> Response {
    let fs = *fs;
    // The page's first section starts at the page's top; the others 30 under the plate above.
    let first = self.st.stack.last().is_none_or(|&(_, n)| n == 0);
    self.col(Style::new().w(grow()), |ui| {
      if !title.is_empty() {
        ui.form_head(&fs, title, subtitle, if first { 0.0 } else { fs.head_pad[0] }, note);
      }

      ui.col(fs.group.clip(), rows);
    })
  }

  /// A section's title line: the title (and `subtitle` under it), what `note` builds at its
  /// right, `top` over it and [FormStyle::head_pad] around the rest.
  fn form_head(
    &mut self,
    fs: &FormStyle,
    title: &str,
    sub: &str,
    top: f32,
    note: impl FnOnce(&mut Self),
  ) {
    let line = self.head_line(fs);
    let [_, r, b, l] = fs.head_pad;
    let head = Style::new().w(grow()).pads([top, r, b, l]).gap(12.0);
    self.row(head.align(Align::Start, Align::Start), |ui| {
      ui.col(Style::new().w(grow()).gap(fs.subtitle_gap), |ui| {
        ui.text(title, fs.head);
        if !sub.is_empty() {
          ui.text(sub, fs.subtitle);
        }
      });
      // On the title's line.
      ui.row(Style::new().min_h(line).align(Align::Start, Align::Center), note);
    });
  }

  /// A section title's line height.
  fn head_line(&mut self, fs: &FormStyle) -> f32 {
    match fs.head.line_h > 0.0 {
      true => fs.head.line_h,
      false => self.metrics(fs.head.font).line_height(),
    }
  }

  /// A row of the section being built: the tile, the label (with its ⓘ and description) at the
  /// left, what `control` builds at the right, 8 apart, and a hairline above it unless it is the
  /// first. The controls keep their width and place at the right edge, centred on the row's
  /// height; the label and description wrap in what is left, the row growing taller. A
  /// disabled row's text is [super::Theme::tertiary]. The ⓘ is `id.key("info")` of the row's
  /// id, the label's text `id.key("label")` and the description's `id.key("caption")`. The row
  /// takes no input, its controls do, unless [FormStyle::row] is [Style::hit]: then it is a row
  /// a click selects, named by its label and description for assistive technology. Else the
  /// first control recorded in it with no name of its own takes them as its name and
  /// description ([Ui::a11y]), and they are not read again.
  pub fn form_row<R>(
    &mut self,
    fs: &FormStyle,
    row: FormRow,
    control: impl FnOnce(&mut Self) -> R,
  ) -> Response {
    self.form_row_with(fs, row, |_| {}, control)
  }

  /// [Ui::form_row] with what `below` builds under the description, as wide as the label: a
  /// task's progress bar, a row's own buttons.
  pub fn form_row_with<R>(
    &mut self,
    fs: &FormStyle,
    row: FormRow,
    below: impl FnOnce(&mut Self),
    control: impl FnOnce(&mut Self) -> R,
  ) -> Response {
    let fs = *fs;
    let t = self.theme();
    let id = self.take_id();
    let first = self.st.stack.last().is_none_or(|&(_, n)| n == 0);
    let off = row.disabled || self.scope_off;
    let dim = |s: TextStyle| if off { s.color(t.tertiary) } else { s };
    let (line, hair, [l, r]) = (fs.divider, fs.hairline, fs.inset);
    let top = if first { 0.0 } else { hair };
    let hairline = move |f: &mut Frame, rect: Rect| {
      if !first {
        f.rect(Rect::new(rect.x + l, rect.y, (rect.w - l - r).max(0.0), hair), line, 0.0);
      }
    };
    let style = fs.row.min_h(fs.row.min[1] + top).align(Align::Start, Align::Start);
    let style = if row.selected { style.bg(fs.selected) } else { style };
    let text = [row.label, row.caption.unwrap_or_default()];
    if fs.row.hit {
      self.a11y_row(id, text).state(State::Selected, row.selected).state(State::Disabled, off);
    }

    // The first line's height: the least the controls' slot is, with the padding around it.
    let line_h = match fs.label.line_h > 0.0 {
      true => fs.label.line_h,
      false => self.metrics(fs.label.font).line_height(),
    };

    let pad_x = fs.row.pad[3];
    self.disabled_scope(row.disabled, |ui| {
      ui.next_id = Some((id, false));
      ui.canvas_row(style, hairline, |ui| {
        if let Some(tile) = row.tile {
          let x = (fs.tile_x - pad_x).max(0.0);
          let slot =
            Style::new().h(grow()).pads([top, x, 0.0, x]).align(Align::Start, Align::Center);
          ui.row(slot, |ui| match tile {
            Tile::Text(s) => _ = ui.row(fs.tile, |ui| ui.text(s, fs.tile_text)),
            Tile::Icon(i) => {
              let h = if let Dim::Px(h) = fs.tile.size[1] { h } else { 24.0 };
              let size = (h * 0.6).round();
              _ = ui.row(fs.tile, |ui| ui.icon_stroke(i, size, fs.tile_icon, 1.6));
            }
          });
        }

        let label = Style::new().w(grow()).gap(2.0).pads([top + fs.pad_y, 0.0, fs.pad_y, 0.0]);
        ui.col(label, |ui| {
          ui.row(Style::new().w(grow()).gap(6.0).align(Align::Start, Align::Center), |ui| {
            let s = if row.one_line { fs.label.ellipsis() } else { fs.label };
            ui.next_id = Some((id.key("label"), false));
            ui.text(row.label, dim(s));
            if let Some(tip) = row.info {
              let info = id.key("info");
              let shift = ui.mark_shift(fs.label);
              let size = Style::new().w(px(fs.info)).h(px(fs.info));
              let at = Float::parent().offset(0.0, shift).clip();
              ui.row(size, |ui| _ = ui.with_id(info).float(at).info_button());
              ui.tooltip(info, tip);
            }
          });
          if let Some(c) = row.caption {
            ui.next_id = Some((id.key("caption"), false));
            ui.text(c, dim(fs.caption));
          }

          below(ui);
        });
        // A slot as high as the row, at least the label's first line and the padding around
        // it, holding the controls centred: on the label and its description, as the tile.
        let slot = Style::new()
          .w(rigid())
          .h(grow())
          .min_h(top + 2.0 * fs.pad_y + line_h)
          .pads([top, 0.0, 0.0, 0.0])
          .align(Align::End, Align::Center);
        let names = (!fs.row.hit).then(|| ui.form_names(id, text));
        ui.col(slot, |ui| {
          ui.row(Style::new().gap(fs.gap).align(Align::Start, Align::Center), control)
        });
        if let Some(n) = names {
          ui.set_slot(n);
        }
      })
    })
  }

  /// How far below the middle of a one-line label in `style` a mark centred beside it goes to
  /// sit on the middle of its capitals (the cap height), where the row's controls centre: on
  /// the x-height a circle reads dropped below a label with capitals and ascenders.
  fn mark_shift(&mut self, style: TextStyle) -> f32 {
    let m = self.metrics(style.font);
    if m.ascent == 0.0 {
      return 0.0;
    }

    let line = if style.line_h > 0.0 { style.line_h } else { m.line_height() };
    let baseline = (line - m.ascent - m.descent) / 2.0 + m.ascent;
    baseline - m.cap_height / 2.0 - line / 2.0
  }

  /// The page's footer under its last plate ([FormStyle::footer]): what `buttons` builds,
  /// right-aligned, then the round "?" ([Ui::help_button]) at the plate's right edge, as System
  /// Settings ends a pane. The response is the "?"'s: `clicked` opens the page's help.
  pub fn form_footer(&mut self, fs: &FormStyle, buttons: impl FnOnce(&mut Self)) -> Response {
    let id = self.take_id();
    let mut r = Response::default();
    // The row by a key of its own: the "?" has the id.
    self.next_id = Some((id.key("footer"), false));
    self.row(fs.footer, |ui| {
      buttons(ui);
      r = ui.with_id(id).help_button();
    });
    r
  }

  /// The footer with the "?" alone ([Ui::form_footer]).
  pub fn form_help(&mut self, fs: &FormStyle) -> Response {
    self.form_footer(fs, |_| {})
  }

  /// A toolbar of tabs, one per `(value, icon, label)`, centred: the icon over the label, the
  /// one equal to `cur` raised on a tile, in the accent ([FormStyle::tab_on]); no hover fill, as
  /// System Settings' toolbar, only the pointing hand. A click picks a tab; so do Left and Right
  /// while the toolbar has focus (Tab gives it, ringing the selected tab). `changed` when `cur`
  /// changed. Tabs are `id.key(i)`. Under a title bar the app draws ([Ui::titlebar]) the window's
  /// title tops it, and a press on it anywhere but a tab moves the window.
  pub fn toolbar_tabs<T: PartialEq + Copy>(
    &mut self,
    fs: &FormStyle,
    cur: &mut T,
    items: &[(T, Icon, &str)],
  ) -> Response {
    let fs = *fs;
    let t = self.theme();
    let id = self.take_id();
    let mut at = items.iter().position(|o| o.0 == *cur);
    let b = self.pick_behavior(id, items.len(), |i| id.key(i), &mut at);
    if b.changed
      && let Some(i) = at
    {
      *cur = items[i].0;
    }
    let titlebar = self.titlebar();
    let mut head = Style { gap: 0.0, ..fs.toolbar };
    if titlebar.is_some() {
      head.pad[0] = 0.0;
      self.drags_window();
    }

    let bar = Style::new().w(grow()).gap(fs.toolbar.gap).align(Align::Center, Align::Start);
    let bar = match b.disabled {
      true => bar.opacity(t.disabled),
      false => bar.tab_focusable_quiet(),
    };

    let ring = b.focused.then_some((t.focus_ring().color, t.focus_ring().width));
    let mut r = Response::default();
    self.next_id = Some((id.key("head"), false));
    self.col(head, |ui| {
      if let Some((h, title)) = titlebar {
        ui.row(Style::new().w(grow()).h(px(h)).center().clip(), |ui| ui.text(title, fs.title));
        ui.drags_window();
      }

      r = ui.with_id(id).row(bar, |ui| {
        for (i, &(v, icon, label)) in items.iter().enumerate() {
          let tab = id.key(i);
          let on = v == *cur;
          // No hover fill, as System Settings' toolbar: the pointing hand says it's a tab.
          let (tile, color) = match on {
            true => (fs.tab_on, t.accent),
            false => (fs.tab, fs.tab_text.color),
          };

          let radius = fs.tab.radii[0];
          let ring = ring.filter(|_| on);
          let glass = fs.tab_glass.filter(|_| on);
          let opacity = if b.disabled { t.disabled } else { 1.0 };
          let paint = move |f: &mut Frame, r: Rect| {
            if let Some(g) = glass {
              g.paint(f, r, radius, opacity);
            }

            if let Some((c, w)) = ring {
              let o = Rect::new(r.x - w, r.y - w, r.w + 2.0 * w, r.h + 2.0 * w);
              f.quad(Quad::new(o, [0.0; 4]).radius(radius + w).border(w, c));
            }
          };
          let s = b.pointer(tile);
          ui.with_id(tab).canvas_col(s, paint, |ui| {
            // Canvases draw past the bar's opacity: the icon dims itself.
            ui.disabled(b.disabled).icon_stroke(icon, fs.tab_icon, color, fs.tab_stroke);
            ui.text(label, fs.tab_text.color(color));
          });
        }
      });
    });
    (r.changed, r.clicked) = (b.changed, b.changed);
    r
  }
}

impl<'a> Ui<'a> {
  /// Form sections built in view only, for a long page in a [Ui::form_scroll] (a
  /// [Ui::virtual_col]): a section for each of `groups`, its title (with what `note(ui, g)`
  /// builds at its right) over a plate of its rows, each `row_h` high without the hairline
  /// above it and built with `row(ui, g, i)` as [Ui::form_row]'s are. Each row is its piece of
  /// the plate: rounded at the section's ends, the hairline above all but the first; keyed by
  /// `key(g, i)`, so its ids, hover and focus stay as rows come and go above it. Returns the
  /// column's items built: each section's title, then its rows.
  pub fn form_list(
    &mut self,
    fs: &FormStyle,
    groups: &[(&str, usize)],
    row_h: f32,
    key: &dyn Fn(usize, usize) -> u64,
    mut note: impl FnMut(&mut Ui<'a>, usize),
    mut row: impl FnMut(&mut Ui<'a>, usize, usize),
  ) -> Range<usize> {
    let fs = *fs;
    let first = self.st.stack.last().is_none_or(|&(_, n)| n == 0);
    // A title's top: the page's gap and the title's padding, none for the page's first.
    let top = |g: usize| if g == 0 && first { 0.0 } else { fs.page.gap + fs.head_pad[0] };
    let head_h = self.head_line(&fs) + fs.head_pad[2];
    let (row_h, hair) = (row_h as f64, fs.hairline as f64);
    let mut sec = Sections { starts: vec![0], tops: vec![0.0], heads: vec![], row_h, hair };
    for (g, &(_, n)) in groups.iter().enumerate() {
      let head = (top(g) + head_h) as f64;
      sec.heads.push(head);
      sec.starts.push(sec.starts[g] + 1 + n);
      sec.tops.push(sec.tops[g] + head + sec.rows(n));
    }

    let (r, [l, rr], line) = (fs.group.radii[0], fs.inset, fs.divider);
    // A title by its section, a row by the app's key.
    let item = |k: usize| {
      let g = sec.starts.partition_point(|&s| s <= k) - 1;
      (g, k - sec.starts[g])
    };
    let keys = |k| match item(k) {
      (g, 0) => (0u8, g as u64),
      (g, i) => (1, key(g, i - 1)),
    };
    let id = self.take_id();
    self.lazy_col(id, &sec, keys, |ui, k| {
      let (g, i) = item(k);
      let (title, n) = groups[g];
      if i == 0 {
        return ui.form_head(&fs, title, "", top(g), |ui| note(ui, g));
      }

      let (i, hair) = (i - 1, if i > 1 { fs.hairline } else { 0.0 });
      let radii = [(i == 0) as u8 as f32 * r, (i + 1 == n) as u8 as f32 * r];
      let piece = fs.group.h(grow()).pads([hair, 0.0, 0.0, 0.0]).clip();
      let piece = Style { radii: [radii[0], radii[0], radii[1], radii[1]], ..piece };
      let paint = move |f: &mut Frame, at: Rect| {
        f.rect(Rect::new(at.x + l, at.y, (at.w - l - rr).max(0.0), hair), line, 0.0);
      };
      ui.canvas_col(piece, paint, |ui| row(ui, g, i));
    })
  }

  /// Scrolls row `i` of section `g` of form list `id` into view in its next build
  /// ([Ui::reveal_item]); `groups` as the list has them.
  pub fn reveal_form_row(&mut self, id: Id, groups: &[(&str, usize)], g: usize, i: usize) {
    let before: usize = groups[..g].iter().map(|&(_, n)| 1 + n).sum();
    self.reveal_item(id, before + 1 + i);
  }
}

/// A [Ui::form_list]'s items, placed a section at a time, so a frame costs its sections, not its
/// rows: where each section's items start, its top, and its title's height, then its rows, a
/// row and a hairline apart (the first without one).
struct Sections {
  starts: Vec<usize>,
  tops: Vec<f64>,
  heads: Vec<f64>,
  row_h: f64,
  hair: f64,
}

impl Sections {
  /// The height of a section's first `r` rows.
  fn rows(&self, r: usize) -> f64 {
    r as f64 * self.row_h + r.saturating_sub(1) as f64 * self.hair
  }
}

impl Spans for Sections {
  fn len(&self) -> usize {
    self.starts[self.starts.len() - 1]
  }

  fn top(&self, i: usize) -> f64 {
    let g = self.starts.partition_point(|&s| s <= i) - 1;
    match i - self.starts[g] {
      0 => self.tops[g],
      j => self.tops[g] + self.heads[g] + self.rows(j - 1),
    }
  }

  fn at(&self, y: f64) -> usize {
    let g = self.tops.partition_point(|&t| t <= y).saturating_sub(1);
    if g + 1 >= self.tops.len() {
      return self.len();
    }

    // In the title, or in row `r` (past its rows, the next section's title).
    let y = y - self.tops[g] - self.heads[g];
    let r = match y {
      y if y < 0.0 => return self.starts[g],
      y if y < self.row_h => 0,
      y => 1 + ((y - self.row_h) / (self.row_h + self.hair)) as usize,
    };

    (self.starts[g] + 1 + r).min(self.starts[g + 1])
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::App;
  use crate::harness::Harness;
  use crate::scene::{Point, Size};
  use crate::ui::Id;

  /// A section of one row and a footer with the "?" alone, nothing named.
  #[derive(Default)]
  struct Page(bool);

  impl App for Page {
    fn view(&mut self, ui: &mut Ui) {
      let fs = FormStyle::plates(&ui.theme());
      ui.col(fs.page, |ui| {
        ui.form_section(&fs, "Section", |ui| {
          ui.form_row(&fs, FormRow::new("Row").disabled(true), |ui| {
            ui.id("t").toggle(&mut self.0, "")
          });
        });
        ui.form_help(&fs);
      });
    }
  }

  #[test]
  fn an_unnamed_footer_and_a_disabled_row() {
    let mut h = Harness::new(Page::default(), Size::new(400.0, 300.0), 1.0);
    h.frame();
    // The row's control is disabled with it.
    h.click_id(Id::of("t"));
    assert!(!h.app().0);
  }

  /// Toolbar tabs, disabled or not, in the light theme.
  struct Bar(bool);

  impl App for Bar {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_theme(Theme::PLATES_LIGHT);
      let fs = FormStyle::plates(&ui.theme());
      let tabs = [(0, Icon::GEAR, "General"), (1, Icon::INFO, "About")];
      ui.col(Style::new().grow().bg(fs.bg), |ui| {
        ui.disabled(self.0).id("bar").toolbar_tabs(&fs, &mut 0, &tabs);
      });
    }
  }

  /// Placed a section at a time, items lie where summing every height puts them, for sections
  /// of many rows, none and one.
  #[test]
  fn sections_place_their_items_as_their_heights_do() {
    let (counts, heads) = ([3, 0, 1, 5], [30.0, 40.5, 40.5, 40.5]);
    let (row_h, hair) = (52.0, 1.0);
    let mut sec = Sections { starts: vec![0], tops: vec![0.0], heads: vec![], row_h, hair };
    let mut ends: Vec<f64> = vec![];
    for (g, &n) in counts.iter().enumerate() {
      sec.heads.push(heads[g]);
      sec.starts.push(sec.starts[g] + 1 + n);
      sec.tops.push(sec.tops[g] + heads[g] + sec.rows(n));
      let hs =
        std::iter::once(heads[g]).chain((0..n).map(|i| row_h + if i > 0 { hair } else { 0.0 }));
      for h in hs {
        ends.push(ends.last().unwrap_or(&0.0) + h);
      }
    }

    assert_eq!(sec.len(), ends.len());
    for i in 0..=ends.len() {
      assert_eq!(sec.top(i), if i == 0 { 0.0 } else { ends[i - 1] }, "top of {i}");
    }

    for y in (-40..1200).map(|y| y as f64 * 0.5) {
      assert_eq!(sec.at(y), ends.partition_point(|&e| e <= y), "at {y}");
    }
  }

  #[test]
  fn a_disabled_toolbar_dims_its_icons() {
    // The unselected tab's icon, above its label.
    let darkest = |off: bool| {
      let mut h = Harness::new(Bar(off), Size::new(300.0, 80.0), 2.0);
      h.frame();
      let r = h.rect(Id::of("bar").key(1)).unwrap();
      let img = h.render();
      let px = (0..(r.w * 2.0) as i32).flat_map(|x| (0..56).map(move |y| (x, y)));
      px.map(|(x, y)| img.pixel(r.x + x as f32 / 2.0, r.y + y as f32 / 2.0)[0]).min().unwrap()
    };
    let (on, off) = (darkest(false), darkest(true));
    assert!(off > on + 30, "the icon at {on} enabled, {off} disabled");
  }

  #[test]
  fn tabs_without_a_name_build_under_a_title_bar() {
    struct Unnamed;
    impl App for Unnamed {
      fn view(&mut self, ui: &mut Ui) {
        let fs = FormStyle::plates(&ui.theme());
        ui.toolbar_tabs(&fs, &mut 0, &[(0, Icon::GEAR, "General")]);
      }
    }

    let mut h = Harness::new(Unnamed, Size::new(200.0, 120.0), 1.0);
    h.rt.set_titlebar(Some((32.0, "Preferences")));
    h.frame();
    assert!(h.rt.drags_window(Point::new(5.0, 16.0)));
  }

  #[test]
  fn tabs_sit_halfway_under_the_os_title_bar_and_never_drag_the_window() {
    let mut h = Harness::new(Bar(false), Size::new(300.0, 120.0), 2.0);
    h.frame();
    let tab = h.rect(Id::of("bar").key(1)).unwrap();
    assert_eq!(tab.y, 8.0, "8 over the tabs");
    // 8 under them the hairline, over the window's ground.
    let img = h.render();
    let at = |dy: f32| img.pixel(5.0, tab.y + tab.h + dy)[0];
    assert!(at(8.25) < at(4.0), "the hairline at {}, the ground at {}", at(8.25), at(4.0));
    assert!(!h.rt.drags_window(Point::new(5.0, 20.0)));
  }

  #[test]
  fn under_its_own_title_bar_the_toolbar_has_the_title_and_drags_the_window() {
    let mut h = Harness::new(Bar(false), Size::new(300.0, 120.0), 2.0);
    h.rt.set_titlebar(Some((32.0, "Preferences")));
    h.frame();
    let tab = h.rect(Id::of("bar").key(1)).unwrap();
    assert_eq!(tab.y, 32.0, "the tabs right under the title bar");
    // The title's ink in the middle of the line.
    let img = h.render();
    let ink =
      (0..600).filter_map(|x| (0..64).map(|y| img.pixel(x as f32 / 2.0, y as f32 / 2.0)[0]).min());
    assert!(ink.min().unwrap() < 200, "the title in the title bar");
    assert!(h.rt.drags_window(Point::new(150.0, 16.0)), "the title line drags the window");
    assert!(h.rt.drags_window(Point::new(5.0, tab.y + 10.0)), "so does the bar beside the tabs");
    assert!(!h.rt.drags_window(Point::new(tab.x + 10.0, tab.y + 10.0)), "a tab is clicked");
  }
}
