// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Settings windows in macOS 26 System Settings' look ([FormStyle::plates], measured from
//! its screenshots), as SwiftUI's `Form` with `.formStyle(.grouped)`: toolbar tabs of an icon
//! over a label ([Ui::toolbar_tabs]), and pages of sections ([Ui::form_section], SwiftUI's
//! `Section { } header: { } footer: { }`), each a bold header over a grey rounded plate of rows
//! and an optional grey footer under it. A row ([Ui::form_row], SwiftUI's `LabeledContent`) is
//! its label and a grey description under it at the left, both a text or what a closure builds
//! ([Content]: a name and badges, an ⓘ with [Ui::help] on it), the controls at the right, a
//! leading tile (letters or an icon) when it lists things, and a hairline above it inset from both
//! sides of the plate. A footer row under the last plate holds the page's buttons and a round "?"
//! that opens its help ([Ui::form_footer]). Rows pitch 37.5 points, 53 with a one-line
//! description, over [super::Sizes::PLATES]; draw it with [super::Theme::PLATES_LIGHT] or
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
//! ui.id("tabs").toolbar_tabs(&fs, &mut self.page, &[TabItem::new(Page::General, Icon::GEAR, "General"), ..]);
//! ui.form_scroll(&fs, Style::new().grow(), |ui| ui.col(fs.page, |ui| {
//!   ui.form_section(&fs, "Startup", |ui| {
//!     let label = |ui: &mut Ui, s: TextStyle| {
//!       ui.text("On launch", s);
//!       ui.help("What opens when the app starts…").info_button();
//!     };
//!     ui.form_row(&fs, FormRow::new(label), |ui| ui.id("launch").picker(&mut self.launch, &LAUNCH));
//!     let row = FormRow::new("Language").caption("Applies after a restart.");
//!     ui.form_row(&fs, row, |ui| ui.picker(&mut self.lang, &LANGS));
//!   });
//!   ui.form_footer(&fs, |_| {});
//! }));
//! ```
//!
//! A disabled row ([Ui::disabled] on it) dims its label and disables every widget in it but its
//! ⓘ or warning mark, whose tooltip reads all the same. The toolbar is a pick
//! ([Ui::pick_behavior]): a click, or the arrows while it has focus (Tab), choose a tab.

use std::cell::Cell;
use std::f32::consts::FRAC_PI_2;
use std::fmt;
use std::rc::Rc;

use super::theme::Theme;
use super::widgets::{Icon, MarkShift};
use super::{
  Align, Cursor, Id, Response, Role, Shadow, State, Style, TextStyle, Ui, grow, px, rigid,
};
use crate::scene::{Color, Frame, Point, Quad, Rect, alpha, mix, rgb};
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
  /// A grey line under a section's title in its header ([Section]), this far below it.
  pub subtitle: TextStyle,
  pub subtitle_gap: f32,
  /// Around a section's footer ([Section::footer]): top, right, bottom, left.
  pub foot_pad: [f32; 4],
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
  /// The label; it wraps (`label.line_limit(1)` keeps one line).
  pub label: TextStyle,
  /// The grey description under a label; it wraps.
  pub caption: TextStyle,
  /// A row's leading tile ([FormRow::tile]): its box, centred in the row, [FormStyle::tile_x] in
  /// from the plate; its text, and an icon's size (in the text's colour).
  pub tile: Style,
  pub tile_x: f32,
  pub tile_text: TextStyle,
  pub tile_icon: f32,
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
  /// The page's footer under the last plate ([Ui::form_footer]): its box, right-aligned buttons
  /// and their gap.
  pub footer: Style,
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

/// A row's piece of a section's plate, as a grouped form's row and a grouped list's
/// ([super::ListStyle::Grouped]) draw it: a fill over the whole piece, rounded where the plate
/// is (`radii`), and the hairline at its top, [FormStyle::inset] in from the plate's sides.
#[derive(Clone, Copy)]
pub(crate) struct Piece {
  pub fill: Color,
  pub radii: [f32; 4],
  pub line: Color,
  pub inset: [f32; 2],
  pub hair: f32,
}

impl Piece {
  pub fn paint(&self, f: &mut Frame, at: Rect) {
    if self.fill[3] > 0.0 {
      f.quad(Quad::new(at, self.fill).radii(self.radii));
    }

    if self.line[3] > 0.0 {
      let [l, r] = self.inset;
      f.rect(Rect::new(at.x + l, at.y, (at.w - l - r).max(0.0), self.hair), self.line, 0.0);
    }
  }
}

/// What a section's plate tells the rows built on it ([Ui::form_section]): the row held down
/// last, by the node count its build ended at, and where it puts its bottom radius. A row built
/// right after it, the count unchanged, is the one under it; the plate ends with it when the
/// section's rows end at that count, nothing built after it.
#[derive(Default)]
struct Plate {
  held: Option<(u32, Rc<Cell<f32>>)>,
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
    let (ink, rim) = if t.scheme.is_dark() { (WHITE, 0.12) } else { (BLACK, 0.11) };
    let glass = Glass {
      ground: t.bg,
      fill: mix(t.bg, BLACK, 0.13),
      rim: alpha(BLACK, 0.8),
      light: alpha(WHITE, 0.6),
      glow: alpha(WHITE, 0.13),
    };
    let (tab_on, tab_glass) = match t.glass && t.scheme.is_dark() {
      true => (tab, Some(glass)),
      false => (tab.bg(t.tab_tile()).border(0.5, alpha(ink, rim)).shadow(lift), None),
    };

    let (divider, toolbar_line) = match t.scheme.is_dark() {
      true => (alpha(WHITE, 0.05), rgb(0x323232)),
      false => (alpha(BLACK, 0.04), rgb(0xe8e8e8)),
    };

    Self {
      head: TextStyle::new(t.bold, t.text).line_limit(1),
      note: TextStyle::new(small, t.muted).nowrap(),
      head_pad: [20.0, 10.0, 10.0, 10.0],
      subtitle: TextStyle::new(small, t.muted).line_h(14.0),
      subtitle_gap: 1.5,
      foot_pad: [6.0, 10.0, 0.0, 10.0],
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
      tile_icon: (h * 0.6).round(),
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
      bg: t.bg,
      fade: 14.0,
    }
  }
}

/// What a part of a form shows, a row's label, description or tile ([FormRow]) or a section's
/// header or footer ([Section]), as SwiftUI's labels take a string or a view (egui's
/// `impl Into<WidgetText>`): a text, an icon, or what a closure builds in the part's text style
/// (dimmed in a disabled row, as SwiftUI passes a foreground style down); from a `&str`, a
/// `&String`, an [Icon] or `FnOnce(&mut Ui, TextStyle)`, whose parameters need their types
/// written. A closure's texts take the style or one made from it (`s.line_limit(1)`,
/// `s.underline(LinePattern::Dot)` on a term with [Ui::help]); its widgets dim themselves in a
/// disabled row; a canvas of its own reads [Ui::scope_disabled].
#[derive(Default)]
pub enum Content<'s> {
  #[default]
  None,
  Text(&'s str),
  Icon(Icon),
  Built(Build<'s>),
}

/// What a [Content::Built] runs: it builds the content in its style.
pub type Build<'s> = Box<dyn FnOnce(&mut Ui, TextStyle) + 's>;

impl Content<'_> {
  fn is_none(&self) -> bool {
    matches!(self, Content::None | Content::Text(""))
  }

  /// Builds it in `style`, a text or an icon as `id`, an icon `size` points square stroked
  /// `stroke` wide.
  fn build(self, ui: &mut Ui, style: TextStyle, id: Id, size: f32, stroke: f32) {
    match self {
      Content::None => {}
      Content::Text(s) => {
        ui.next_id = Some((id, false));
        ui.text(s, style);
      }
      Content::Icon(i) => {
        ui.next_id = Some((id, false));
        ui.icon(i.default_stroke(stroke), size, style.color);
      }
      Content::Built(f) => f(ui, style),
    }
  }
}

impl<'s> From<&'s str> for Content<'s> {
  fn from(s: &'s str) -> Self {
    Content::Text(s)
  }
}

impl<'s> From<&'s String> for Content<'s> {
  fn from(s: &'s String) -> Self {
    Content::Text(s)
  }
}

impl From<Icon> for Content<'_> {
  fn from(i: Icon) -> Self {
    Content::Icon(i)
  }
}

impl<'s, F: FnOnce(&mut Ui, TextStyle) -> R + 's, R> From<F> for Content<'s> {
  fn from(f: F) -> Self {
    Content::Built(Box::new(move |ui, s| _ = f(ui, s)))
  }
}

/// Its text, `…` for one built.
impl fmt::Debug for Content<'_> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      Content::None => f.write_str(""),
      Content::Text(s) => f.write_str(s),
      Content::Icon(i) => write!(f, "{:?}", i.name()),
      Content::Built(_) => f.write_str("…"),
    }
  }
}

/// A row of a form section ([Ui::form_row]), as SwiftUI's `LabeledContent`: its label,
/// description, leading tile and footer, each [Content].
#[derive(Debug, Default)]
pub struct FormRow<'s> {
  label: Content<'s>,
  caption: Content<'s>,
  tile: Content<'s>,
  footer: Content<'s>,
  /// Held down: a [Ui::form_disclosure]'s.
  pressed: bool,
}

impl<'s> FormRow<'s> {
  /// A row labelled by a text or what a closure builds on the label's line in the label's style
  /// ([FormStyle::label]), as SwiftUI's label of views: a name, then tags of what the row is
  /// (a [Ui::badge] at [super::ControlSize::Small] keeps the row's height), then an ⓘ or a
  /// warning mark with its tip ([Ui::info_button], [Ui::warning_button] with [Ui::help]), which
  /// sits on the middle of the name's capitals. Its plain texts (those with no [Ui::help] and in
  /// no widget) are its name: they name the row's first control with no name of its own, and are
  /// not read again; what else it holds is read after that control, in its order.
  ///
  /// ```ignore
  /// FormRow::new(|ui: &mut Ui, s: TextStyle| {
  ///   ui.text("gigaam-v3", s.line_limit(1));
  ///   ui.help("Best in Russian").control_size(ControlSize::Small).badge("Recommended", Tone::Accent);
  /// })
  /// ```
  pub fn new(label: impl Into<Content<'s>>) -> Self {
    Self { label: label.into(), ..Self::default() }
  }

  /// The description under the label in [FormStyle::caption], wrapping onto as many lines as it
  /// needs: a text or what a closure builds (a term with a tooltip, a task's progress under its
  /// text). Its plain texts describe the control the label names, as its name, and are not read
  /// again; what else it holds is read after that control.
  ///
  /// ```ignore
  /// FormRow::new("parakeet-ultra").caption(|ui: &mut Ui, s: TextStyle| {
  ///   ui.help("Errors per 100 words").text("WER 4.7%", s.underline(LinePattern::Dot));
  ///   ui.text(" · 740 MB", s);
  /// })
  /// ```
  pub fn caption(self, caption: impl Into<Content<'s>>) -> Self {
    Self { caption: caption.into(), ..self }
  }

  /// A tile before the label, as SwiftUI's `Label`'s icon: letters (an account's initials) in
  /// [FormStyle::tile_text], an [Icon] [FormStyle::tile_icon] points square in its colour, or
  /// what a closure builds.
  pub fn tile(self, tile: impl Into<Content<'s>>) -> Self {
    Self { tile: tile.into(), ..self }
  }

  /// What goes under the label and the controls, in the row across its whole width with no
  /// hairline between, as a `VStack` of a control and a view under it is one row of SwiftUI's
  /// grouped form: a text in [FormStyle::caption], or what a closure builds (a slider over the
  /// row's width setting what the row's switch turns on).
  ///
  /// ```ignore
  /// let row = FormRow::new("Turn off below 50%").footer(|ui: &mut Ui, _: TextStyle| {
  ///   ui.disabled(!on).slider(&mut level, 10.0..=90.0);
  /// });
  /// ui.form_row(&fs, row, |ui| ui.toggle(&mut on, ""));
  /// ```
  pub fn footer(self, footer: impl Into<Content<'s>>) -> Self {
    Self { footer: footer.into(), ..self }
  }
}

/// A toolbar tab ([Ui::toolbar_tabs]): its value, icon and label, as SwiftUI's
/// `.tabItem { Label(label, systemImage:) }` and `.tag(value)`; `const`, so a toolbar is a
/// constant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabItem<'s, T> {
  pub value: T,
  pub icon: Icon,
  pub label: &'s str,
}

impl<'s, T> TabItem<'s, T> {
  pub const fn new(value: T, icon: Icon, label: &'s str) -> Self {
    Self { value, icon, label }
  }
}

/// A form section's header and footer ([Ui::form_section]), as SwiftUI's
/// `Section { } header: { } footer: { }`: each [Content], the header in [FormStyle::head] on the
/// title's line (a title, a note at the right after a [Ui::spacer], a grey line under it), the
/// footer in [FormStyle::caption] under the plate. A string is a section with that title.
#[derive(Debug, Default)]
pub struct Section<'s> {
  header: Content<'s>,
  footer: Content<'s>,
}

impl<'s> Section<'s> {
  pub fn new(header: impl Into<Content<'s>>) -> Self {
    Self { header: header.into(), footer: Content::None }
  }

  pub fn footer(self, footer: impl Into<Content<'s>>) -> Self {
    Self { footer: footer.into(), ..self }
  }
}

impl<'s> From<&'s str> for Section<'s> {
  fn from(title: &'s str) -> Self {
    Section::new(title)
  }
}

impl<'s> From<&'s String> for Section<'s> {
  fn from(title: &'s String) -> Self {
    Section::new(title)
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

  /// A section: its header ([Section], or a title) over a plate of the rows `rows` builds
  /// ([Ui::form_row]), its footer under it, as SwiftUI's `Section { } header: { } footer: { }`
  /// in a grouped form. An empty header shows none.
  pub fn form_section<'s, R>(
    &mut self,
    fs: &FormStyle,
    section: impl Into<Section<'s>>,
    rows: impl FnOnce(&mut Self) -> R,
  ) -> Response {
    let (fs, Section { header, footer }) = (*fs, section.into());
    // The page's first section starts at the page's top; the others 30 under the plate above.
    let first = self.st.stack.last().is_none_or(|&(_, n)| n == 0);
    let id = self.take_id();
    self.with_id(id).col(Style::new().w(grow()), |ui| {
      if !header.is_none() {
        let [top, r, b, l] = fs.head_pad;
        let head = Style::new().w(grow()).pads([if first { 0.0 } else { top }, r, b, l]);
        let head = head.gap(12.0).align(Align::Start, Align::Center);
        let line = ui.head_line(&fs);
        ui.row(head.min_h(line), |ui| header.build(ui, fs.head, id.key("header"), line, 1.6));
      }

      let outer = ui.set_slot(Some(Plate::default()));
      ui.col(fs.group.clip(), rows);
      // A row held down that the plate ends with, nothing built after it, rounds its fill as
      // the plate.
      let n = ui.st.tree.nodes.len() as u32;
      if let Some((_, end)) = ui.set_slot(outer).and_then(|p| p.held).filter(|h| h.0 == n) {
        end.set(fs.group.radii[0]);
      }

      if !footer.is_none() {
        let pad = Style::new().w(grow()).pads(fs.foot_pad);
        ui.col(pad, |ui| footer.build(ui, fs.caption, id.key("footer"), 12.0, 1.6));
      }
    })
  }

  /// A section title's line height.
  fn head_line(&mut self, fs: &FormStyle) -> f32 {
    self.line_height(fs.head)
  }

  /// A row of the section being built, as SwiftUI's `LabeledContent` in a grouped form: the
  /// tile, the label and its description at the left ([FormRow]), what `control` builds at the
  /// right, 8 apart, the row's footer under them across its width, and a hairline above it unless
  /// it is the first, held down (a [Ui::form_disclosure]'s) or right under a row held down. The
  /// controls keep their width and place at the right edge, centred on the row's height; the label
  /// and description wrap in what is left, the row growing taller. [Ui::disabled] on it dims its
  /// label and description to [super::Theme::tertiary] and disables its widgets but an ⓘ or warning
  /// mark (in a row a click selects, assistive technology reads it disabled with its row). A text
  /// label is `id.key("label")` of the row's id, a text description `id.key("caption")`, a text
  /// or icon tile `id.key("tile")`, a text footer `id.key("footer")`. The row takes no input, its
  /// controls do, unless [FormStyle::row] is [Style::hit]: then it is a row a click selects, named
  /// by its label's plain texts and described by its description's. Else the first control recorded
  /// in it with no name of its own takes them as its name and description ([Ui::a11y]); either way
  /// they are not read again, and the label's and description's other elements are read after the
  /// one they name.
  pub fn form_row<R>(
    &mut self,
    fs: &FormStyle,
    row: FormRow,
    control: impl FnOnce(&mut Self) -> R,
  ) -> Response {
    let fs = *fs;
    let t = self.theme();
    let id = self.take_id();
    let off = self.take_disabled();
    let first = self.st.stack.last().is_none_or(|&(_, n)| n == 0);
    let dim = |s: TextStyle| if off { s.color(t.tertiary) } else { s };
    let top = if first { 0.0 } else { fs.hairline };
    // Held down, a fill over its whole piece of the plate, as a grouped list's selection:
    // rounded at the plate's top when first, at its bottom when the plate ends with it (the
    // section says so once its rows are built); no hairline beside it.
    let pressed = row.pressed;
    let end = pressed.then(|| Rc::new(Cell::new(0.0)));
    let held = end.clone();
    let at = self.st.tree.nodes.len() as u32;
    let plate = self.slot::<Plate>().and_then(|p| p.held.as_ref());
    let above = plate.is_some_and(|h| h.0 == at);

    let corner = if first { fs.group.radii[0] } else { 0.0 };
    let piece = Piece {
      fill: if pressed { t.pressed } else { [0.0; 4] },
      radii: [corner; 4],
      line: if first || pressed || above { [0.0; 4] } else { fs.divider },
      inset: fs.inset,
      hair: fs.hairline,
    };
    let hairline = move |f: &mut Frame, at: Rect| {
      let end = end.as_ref().map_or(0.0, |e| e.get());
      Piece { radii: [corner, corner, end, end], ..piece }.paint(f, at)
    };
    let style = fs.row.min_h(fs.row.min[1] + top).align(Align::Start, Align::Start);
    // The first line's height: the least the controls' slot is, with the padding around it.
    let line_h = self.line_height(fs.label);

    let (pad_x, shift) = (fs.row.pad[3], self.mark_shift(fs.label));
    let FormRow { label, caption, tile, footer, .. } = row;
    // The line of the tile, the label and the controls, as high as the row's least height; the
    // footer under it.
    let line = Style::new().w(grow()).min_h(style.min[1]).gap(style.gap);
    let r = self.disabled_scope(off, |ui| {
      ui.next_id = Some((id, false));
      ui.canvas_col(style.gap(0.0), hairline, |ui| {
        ui.row(line, |ui| {
          if !tile.is_none() {
            let x = (fs.tile_x - pad_x).max(0.0);
            let slot =
              Style::new().h(grow()).pads([top, x, 0.0, x]).align(Align::Start, Align::Center);
            ui.row(slot, |ui| {
              let k = id.key("tile");
              ui.row(fs.tile, |ui| tile.build(ui, fs.tile_text, k, fs.tile_icon, 1.6))
            });
          }

          let (mut name, mut about) = (0..0, 0..0);
          let col = Style::new().w(grow()).gap(2.0).pads([top + fs.pad_y, 0.0, fs.pad_y, 0.0]);
          ui.col(col, |ui| {
            ui.row(Style::new().w(grow()).gap(6.0).align(Align::Start, Align::Center), |ui| {
              // A mark on the line sits on the capitals' middle.
              let outer = ui.set_slot(Some(MarkShift(shift)));
              let at = ui.st.tree.nodes.len() as u32;
              label.build(ui, dim(fs.label), id.key("label"), line_h, 1.6);
              name = at..ui.st.tree.nodes.len() as u32;
              ui.set_slot(outer);
            });
            let at = ui.st.tree.nodes.len() as u32;
            caption.build(ui, dim(fs.caption), id.key("caption"), 12.0, 1.6);
            about = at..ui.st.tree.nodes.len() as u32;
          });
          // The names it replaces, put back once the row or its first control took them.
          let mut outer = Some(ui.form_names(name, about));
          if fs.row.hit {
            ui.a11y_row(id, Role::Row).state(State::Disabled, off);
            ui.set_slot(outer.take().flatten());
          }

          // A slot as high as the row, at least the label's first line and the padding around
          // it, holding the controls centred: on the label and its description, as the tile.
          let slot = Style::new()
            .w(rigid())
            .h(grow())
            .min_h(top + 2.0 * fs.pad_y + line_h)
            .pads([top, 0.0, 0.0, 0.0])
            .align(Align::End, Align::Center);
          ui.col(slot, |ui| {
            ui.row(Style::new().gap(fs.gap).align(Align::Start, Align::Center), control)
          });
          if let Some(outer) = outer {
            ui.set_slot(outer);
          }
        });
        if !footer.is_none() {
          // Right under the description, 2 apart as it is from the label: over the line's padding.
          let pad = Style::new().w(grow()).pads([2.0 - fs.pad_y, 0.0, fs.pad_y, 0.0]);
          ui.col(pad, |ui| footer.build(ui, dim(fs.caption), id.key("footer"), 12.0, 1.6));
        }
      })
    });
    // Held down, where its build ended, for the row under it and the section's end.
    let n = self.st.tree.nodes.len() as u32;
    if let Some(end) = held
      && let Some(p) = self.slot::<Plate>()
    {
      p.held = Some((n, end));
    }

    r
  }

  /// A row that shows the rows `rows` builds under it, in its plate, while expanded, as SwiftUI's
  /// `DisclosureGroup` in a `Form`: [Ui::form_row]'s label, description and tile, a chevron at
  /// the right turning from right to down ([super::Theme::fade]), the pointing hand and, held
  /// down, a fill over its piece of the plate as a grouped list's selection, as System
  /// Settings' rows that open more, which show no hover. A click, or Space while it has focus,
  /// toggles it; it starts collapsed and keeps its state under its id, the chevron
  /// `id.key("chevron")`. The rows show and go at once, not sliding as SwiftUI's. Disabled
  /// ([Ui::disabled] on it or a container around it), it dims, takes no focus and doesn't toggle.
  /// Assistive technology reads a button named by the label's plain texts, described by the
  /// description's, [State::Expanded] while open, disabled with it; the button holds them, so their
  /// other elements (a badge) are not read. `changed` when it toggled.
  pub fn form_disclosure<R>(
    &mut self,
    fs: &FormStyle,
    row: FormRow,
    rows: impl FnOnce(&mut Self) -> R,
  ) -> Response {
    let (id, t) = (self.take_id(), self.theme());
    // Disabled before the behaviour: it reads so and lets go of the focus.
    let b = self.button_behavior(id);
    let (off, clicked, pressed) = (b.disabled, b.clicked, b.pressed);
    let open = self.state::<Disclosed>(id);
    open.0 ^= clicked;
    let open = open.0;
    let turn = self.animate(id.key("turn"), if open { 1.0 } else { 0.0 }, t.fade);
    let style = match off {
      true => fs.row.hit(),
      false => fs.row.hit().tab_focusable().cursor(Cursor::Pointer),
    };

    let ink = if off { t.tertiary } else { t.muted };
    let row = FormRow { pressed, ..row };
    let fs = FormStyle { row: style, ..*fs };
    let mut r = self.disabled(off).with_id(id).form_row(&fs, row, |ui| {
      // Unnamed, a click on it is the row's.
      ui.next_id = Some((id.key("chevron"), false));
      ui.canvas(Style::new().w(px(14.0)).h(px(14.0)), move |f, r| chevron(f, r, turn, ink))
    });
    self.a11y(id, Role::Button).state(State::Expanded, open);

    if open {
      rows(self);
    }

    r.changed = clicked;
    r
  }

  /// How far below the middle of a one-line label in `style` a mark centred beside it goes to
  /// sit on the middle of its capitals (the cap height), where the row's controls centre: on
  /// the x-height a circle reads dropped below a label with capitals and ascenders.
  fn mark_shift(&mut self, style: TextStyle) -> f32 {
    let m = self.metrics(style.font);
    if m.ascent == 0.0 {
      return 0.0;
    }

    let line = style.line_h.unwrap_or(m.line_height());
    let baseline = (line - m.ascent - m.descent) / 2.0 + m.ascent;
    baseline - m.cap_height / 2.0 - line / 2.0
  }

  /// The page's footer under its last plate ([FormStyle::footer]): what `buttons` builds,
  /// right-aligned, then the round "?" ([Ui::help_button]) at the plate's right edge, as System
  /// Settings ends a pane, SwiftUI's `HelpLink`. The response is the "?"'s: `clicked` opens the
  /// page's help; [Ui::help] on the footer is the "?"'s tip.
  pub fn form_footer(&mut self, fs: &FormStyle, buttons: impl FnOnce(&mut Self)) -> Response {
    let (id, help) = (self.take_id(), self.next_help.take());
    let mut r = Response::default();
    // The row by a key of its own: the "?" has the id.
    self.next_id = Some((id.key("footer"), false));
    self.row(fs.footer, |ui| {
      buttons(ui);
      ui.next_help = help;
      r = ui.with_id(id).help_button();
    });
    r
  }

  /// A toolbar of tabs, one per [TabItem], centred, as a `TabView` of `.tabItem`s in a `Settings`
  /// scene: the icon over the label, the one equal to `cur` raised on a tile, in the accent
  /// ([FormStyle::tab_on]); no hover fill, as System Settings' toolbar, only the pointing hand. A
  /// click picks a tab; so do Left and Right while the toolbar has focus (Tab gives it, ringing the
  /// selected tab). `changed` when `cur` changed. Tabs are `id.key(i)`. Under a title bar the app
  /// draws ([Ui::titlebar]) the window's title tops it, and a press on it anywhere but a tab moves
  /// the window.
  pub fn toolbar_tabs<T: PartialEq + Copy>(
    &mut self,
    fs: &FormStyle,
    cur: &mut T,
    items: &[TabItem<'_, T>],
  ) -> Response {
    let fs = *fs;
    let t = self.theme();
    let id = self.take_id();
    let mut at = items.iter().position(|o| o.value == *cur);
    let b = self.pick_behavior(id, items.len(), |i| id.key(i), &mut at);
    if b.changed
      && let Some(i) = at
    {
      *cur = items[i].value;
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
        for (i, &TabItem { value: v, icon, label }) in items.iter().enumerate() {
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
            ui.disabled(b.disabled).icon(icon.default_stroke(fs.tab_stroke), fs.tab_icon, color);
            ui.text(label, fs.tab_text.color(color));
          });
        }
      });
    });
    (r.changed, r.clicked) = (b.changed, b.changed);
    r
  }
}

/// Whether a [Ui::form_disclosure] is expanded, under its id.
#[derive(Default)]
struct Disclosed(bool);

/// A chevron pointing right in the middle of `r`, `turn` of the way to pointing down.
fn chevron(f: &mut Frame, r: Rect, turn: f32, color: Color) {
  let (c, (sin, cos)) =
    (Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0), (turn * FRAC_PI_2).sin_cos());
  let at = |x: f32, y: f32| Point::new(c.x + x * cos - y * sin, c.y + x * sin + y * cos);
  f.polyline(&[at(-1.5, -4.0), at(2.5, 0.0), at(-1.5, 4.0)], 1.5, color);
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use keyboard_types::{Modifiers, NamedKey};

  use super::*;
  use crate::app::App;
  use crate::harness::Harness;
  use crate::keys::Key;
  use crate::scene::{Point, Size};
  use crate::ui::layout::Kind;
  use crate::ui::{ControlSize, Id, LinePattern, Tag, Tone};

  /// A section of one row and a footer with the "?" alone, nothing named.
  #[derive(Default)]
  struct Page(bool);

  impl App for Page {
    fn view(&mut self, ui: &mut Ui) {
      let fs = FormStyle::plates(&ui.theme());
      ui.col(fs.page, |ui| {
        ui.form_section(&fs, "Section", |ui| {
          ui.disabled(true)
            .form_row(&fs, FormRow::new("Row"), |ui| ui.id("t").toggle(&mut self.0, ""));
        });
        ui.form_footer(&fs, |_| {});
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

  /// A row labelled by a name and badges after it, one with a tooltip, over one with the name
  /// alone, `.0` the name; each with `.1`'s control.
  struct Badged(&'static str, Ctl);

  /// The control of [Badged]'s rows: a button with a label, a switch, or a group.
  #[derive(Clone, Copy, PartialEq)]
  enum Ctl {
    Use,
    Switch,
    Segments,
  }

  impl App for Badged {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_theme(Theme::PLATES_LIGHT);
      let fs = FormStyle::plates(&ui.theme());
      let (name, ctl) = (self.0, self.1);
      let label = |ui: &mut Ui, s: TextStyle| {
        ui.id("name").text(name, s.line_limit(1));
        ui.control_size(ControlSize::Small).id("new").badge("New", Tone::Accent);
        let ui = ui.help("In testing").control_size(ControlSize::Small);
        ui.id("beta").badge("Beta", Tone::Neutral);
      };
      let control = |ui: &mut Ui, id: &str| match ctl {
        Ctl::Switch => ui.id(id).toggle(&mut false, ""),
        Ctl::Segments => ui.id(id).segmented(&mut 0, &[Tag::new(0, "Light"), Tag::new(1, "Dark")]),
        Ctl::Use => ui.id(id).button("Use"),
      };
      ui.col(fs.page, |ui| {
        ui.form_section(&fs, "Section", |ui| {
          let badged = FormRow::new(label).caption("Under it");
          ui.id("badged").form_row(&fs, badged, |ui| control(ui, "use"));
          let plain = |ui: &mut Ui, s: TextStyle| ui.text(name, s.line_limit(1));
          let plain = FormRow::new(plain).caption("Under it");
          ui.id("plain").form_row(&fs, plain, |ui| control(ui, "other"));
        });
      });
    }
  }

  /// A label of a name and badges: the badges follow the name on its line, in order, clear of
  /// the controls, read after it, the one with a tooltip with its tip; the row keeps the height
  /// of one with the name alone; a long name ends in "…" before them.
  /// Two rows with a description: one with a switch, one with a control as high as the row, as
  /// an app centres a link's ↗ in its whole row.
  struct Tall;

  impl App for Tall {
    fn view(&mut self, ui: &mut Ui) {
      let fs = FormStyle::plates(&ui.theme());
      let row_h = 2.0 * fs.pad_y + ui.line_height(fs.label) + 2.0 + ui.line_height(fs.caption);
      ui.col(fs.page, |ui| {
        ui.form_section(&fs, Section::default(), |ui| {
          let row = FormRow::new("Switch").caption("What it does");
          ui.id("switch").form_row(&fs, row, |ui| ui.toggle(&mut false, ""));
          let row = FormRow::new("Link").caption("Where it goes");
          ui.id("tall").form_row(&fs, row, |ui| {
            let slot = Style::new().h(px(row_h)).align(Align::Center, Align::Center);
            ui.col(slot, |ui| ui.control_size(ControlSize::Large).icon_button(Icon::EXTERNAL))
          });
        });
      });
    }
  }

  #[test]
  fn a_control_as_high_as_the_row_keeps_its_height() {
    let hair = FormStyle::plates(&Theme::PLATES_LIGHT).hairline;
    let mut h = Harness::new(Tall, Size::new(400.0, 300.0), 2.0);
    h.frame();
    let (switch, tall) = (h.rect(Id::of("switch")).unwrap(), h.rect(Id::of("tall")).unwrap());
    assert_eq!(switch.h + hair, tall.h);
  }

  #[test]
  fn badges_follow_the_label_on_its_line() {
    // The plain row has the hairline over it.
    let hair = FormStyle::plates(&Theme::PLATES_LIGHT).hairline;
    let long = "A label long enough to run under the badges and the button at the right";
    for label in ["Row", long] {
      let mut h = Harness::new(Badged(label, Ctl::Use), Size::new(400.0, 300.0), 2.0);
      h.frame();
      let (badged, plain) = (h.rect(Id::of("badged")).unwrap(), h.rect(Id::of("plain")).unwrap());
      assert_eq!(badged.h + hair, plain.h, "{label}: one height");

      // The label's line as laid out: the text before its "…".
      let tree = &h.rt.ui.last;
      let text = Id::of("name");
      let Kind::Text { lines, .. } = tree.get(text).unwrap().kind else { panic!("{label}") };
      let line = tree.lines[lines.0 as usize];
      let shown = tree.str(line.start, line.end);
      assert_eq!(shown.len() < label.len(), label == long, "{label}: shown {shown:?}");

      // The accent pill's tint just after the label, on the line's middle, not on the plain's;
      // the second badge ends before Use.
      let text = h.rect(text).unwrap();
      let new = h.rect(Id::of("new")).unwrap();
      assert_eq!(new.h, 15.0, "{label}: small");
      let (img, y) = (h.render(), text.y - badged.y + text.h / 2.0);
      let at = |r: Rect| img.pixel(text.x + text.w + 12.0, r.y + y);
      let (tint, ground) = (at(badged), at(plain));
      assert!(
        tint[2] > tint[0].saturating_add(10) && ground[0] == ground[2],
        "{label}: {tint:?} {ground:?}"
      );
      let a11y = h.a11y();
      let (r, n, b) =
        (a11y.find(label), a11y.find("\"New\""), a11y.find(r#""Beta" desc "In testing""#));
      assert!(r < n && n < b && r.is_some(), "{label}: read in order\n{a11y}");
      let clear = |x: f32| img.pixel(x, badged.y + y) == img.pixel(x, plain.y + y);
      let at_use = h.rect(Id::of("use")).unwrap().x;
      assert!((1..=8).all(|dx| clear(at_use - dx as f32 / 2.0)), "{label}: clear before Use");
    }
  }

  /// A switch takes the name and the description from a composed label's and caption's plain
  /// texts, which are not read again; the badges, read on their own, come after the switch, beside
  /// it: a group (a segmented control) named so doesn't take them in.
  #[test]
  fn a_composed_label_names_its_control_then_its_tags() {
    let name = "Download in the background";
    for (ctl, read) in [(Ctl::Switch, "switch"), (Ctl::Segments, "radio group")] {
      let mut h = Harness::new(Badged(name, ctl), Size::new(500.0, 300.0), 1.0);
      h.frame();
      let tree = h.a11y();
      let lines: Vec<&str> = tree.lines().map(str::trim).collect();
      let at = |l: &str| lines.iter().position(|x| x.starts_with(l));
      let control = at(&format!("{read} {name:?}"));
      let (new, beta) = (at(r#"static text "New""#), at(r#"static text "Beta" desc "In testing""#));
      assert!(control.is_some() && control < new && new < beta, "name, then tags:\n{tree}");
      let texts = lines.iter().filter(|l| l.contains(name)).count();
      assert_eq!(texts, 2, "the names read once a row:\n{tree}");
      assert!(!lines.contains(&r#"static text "Under it""#), "{tree}");

      // The tags beside the control, under the same parent.
      let t = h.a11y_tree();
      let node = |s: &str| t.nodes.iter().find(|n| t.str(n.props.name) == s).unwrap();
      let (control, new) = (node(name), node("New"));
      assert_eq!(new.parent, control.parent, "{read}: beside it\n{tree}");
    }
  }

  /// A row with its caption composed of texts, one with a tooltip, under one with the same
  /// caption whole.
  struct Composed;

  impl App for Composed {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_theme(Theme::PLATES_LIGHT);
      let fs = FormStyle::plates(&ui.theme());
      let muted = ui.theme().muted;
      // With `widgets` an icon and a button after the texts.
      let caption = |wer: &'static str, widgets: bool| {
        move |ui: &mut Ui, s: TextStyle| {
          ui.row(Style::new().w(grow()).gap(4.0), |ui| {
            let s_dot = s.underline(LinePattern::Dot);
            ui.id(wer).help("Errors per 100 words").text("WER 4.5%", s_dot);
            ui.id((wer, "size")).text(" · 274 MB", s);
            if widgets {
              ui.id((wer, "icon")).icon(Icon::INFO, 12.0, muted);
              _ = ui.control_size(ControlSize::Small).id((wer, "go")).button("Go");
            }
          });
        }
      };
      ui.col(fs.page, |ui| {
        ui.form_section(&fs, "Section", |ui| {
          ui.id("whole").form_row(&fs, FormRow::new("A").caption("WER 4.5% · 274 MB"), |_| {});
          ui.id("parts").form_row(&fs, FormRow::new("A").caption(caption("wer", false)), |_| {});
          ui.form_row(&fs, FormRow::new("B").caption(caption("on", true)), |_| {});
          let off = FormRow::new("B").caption(caption("off", true));
          ui.disabled(true).form_row(&fs, off, |_| {});
          // A text caption and the same widgets as controls, in a disabled row.
          let off = FormRow::new("C").caption("WER 4.5% · 274 MB");
          ui.disabled(true).id("dim").form_row(&fs, off, |ui| {
            ui.id(("dim", "icon")).icon(Icon::INFO, 12.0, muted);
            _ = ui.control_size(ControlSize::Small).id(("dim", "go")).button("Go");
          });
        });
      });
    }
  }

  /// The composed caption lies where the whole one does, the row as high; in a disabled row its
  /// text dims as a text caption, its icon and button once, as among the controls; its term shows
  /// its tooltip and reads it as its description, where it stands.
  #[test]
  fn a_caption_composed_reads_as_one() {
    let mut h = Harness::new(Composed, Size::new(400.0, 500.0), 2.0);
    h.frame();
    let hair = FormStyle::plates(&Theme::PLATES_LIGHT).hairline;
    let (whole, parts) = (h.rect(Id::of("whole")).unwrap(), h.rect(Id::of("parts")).unwrap());
    assert_eq!(whole.h + hair, parts.h, "as high");
    let caption = h.rect(Id::of("whole").key("caption")).unwrap();
    let (a, b) = (h.rect(Id::of("wer")).unwrap(), h.rect(Id::of(("wer", "size"))).unwrap());
    let at = (caption.x, caption.y - whole.y + hair);
    assert_eq!((a.x, a.y - parts.y), at, "where the caption is");
    assert!(a.x + a.w <= b.x && a.y == b.y, "on one line: {a:?} {b:?}");
    let read = r#"static text "WER 4.5%" desc "Errors per 100 words""#;
    assert!(h.a11y().lines().any(|l| l.trim() == read), "{}", h.a11y());

    // The darkest of a text's ink: the disabled row's lighter.
    let ink = |h: &mut Harness<Composed>, id: Id| {
      let r = h.rect(id).unwrap();
      let img = h.render();
      let px = (0..(r.w * 2.0) as i32).flat_map(|x| (0..(r.h * 2.0) as i32).map(move |y| (x, y)));
      px.map(|(x, y)| img.pixel(r.x + x as f32 / 2.0, r.y + y as f32 / 2.0)[0]).min().unwrap()
    };
    let (on, off) = (ink(&mut h, Id::of("wer")), ink(&mut h, Id::of("off")));
    let (whole, dim) =
      (ink(&mut h, Id::of("whole").key("caption")), ink(&mut h, Id::of("dim").key("caption")));
    assert!(
      on == whole && off == dim && off > on + 40,
      "dimmed: {on} {off}, as text {whole} {dim}"
    );
    // An icon and a button in it dim once, as they do among the row's controls.
    for part in ["icon", "go"] {
      let (was, now) = (ink(&mut h, Id::of(("off", part))), ink(&mut h, Id::of(("dim", part))));
      assert_eq!(was, now, "{part}: dimmed once");
      assert!(ink(&mut h, Id::of(("on", part))) < was, "{part}: dimmed");
    }

    h.move_to(a.x + 4.0, a.y + a.h / 2.0);
    h.wait(Duration::from_millis(400));
    assert_eq!(h.text(Id::of("wer").key("tip").nth(0)), Some("Errors per 100 words"));
  }

  /// A disclosure of two rows in a section, under a plain row, disabled by [Ui::disabled] when
  /// `.1`; `.0` the toggles.
  #[derive(Default)]
  struct Fold(usize, bool);

  impl App for Fold {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_theme(Theme::PLATES_LIGHT);
      let fs = FormStyle::plates(&ui.theme());
      ui.col(fs.page, |ui| {
        ui.form_section(&fs, "Section", |ui| {
          ui.form_row(&fs, FormRow::new("Shown"), |_| {});
          let row = FormRow::new("Other models").caption("2 more");
          let r = ui.disabled(self.1).id("fold").form_disclosure(&fs, row, |ui| {
            ui.id("a").form_row(&fs, FormRow::new("A"), |_| {});
            ui.id("b").form_row(&fs, FormRow::new("B"), |_| {});
          });
          self.0 += r.changed as usize;
          let (off, c) = (FormRow::new("Off"), FormRow::new("Off"));
          let r = ui
            .disabled(true)
            .id("off")
            .form_disclosure(&fs, off, |ui| ui.id("c").form_row(&fs, c, |_| {}));
          self.0 += r.changed as usize;
        });
      });
    }
  }

  /// A click opens it, its rows under it and its chevron turning down; Space while focused
  /// closes it; assistive technology reads a button expanded or not.
  #[test]
  fn a_disclosure_opens_by_click_and_closes_by_space() {
    let mut h = Harness::new(Fold::default(), Size::new(400.0, 300.0), 2.0);
    h.frame();
    let fold = Id::of("fold");
    let read =
      |h: &Harness<Fold>| h.a11y().lines().find(|l| l.contains("Other models")).map(str::to_owned);
    assert!(h.rect(Id::of("a")).is_none(), "collapsed at first");
    assert_eq!(read(&h).as_deref().map(str::trim), Some(r#"button "Other models" desc "2 more""#));
    let (r, c) = (h.rect(fold).unwrap(), h.rect(fold.key("chevron")).unwrap());
    let chevron = |h: &mut Harness<Fold>| {
      let img = h.render();
      // Ink at the end of the chevron's upper stroke pointing right, then pointing down.
      let (x, y) = (c.x + c.w / 2.0, c.y + c.h / 2.0);
      (img.pixel(x - 1.5, y - 4.0)[0], img.pixel(x + 4.0, y - 1.5)[0])
    };
    let right = chevron(&mut h);

    h.click_id(fold);
    h.frame();
    h.wait(Duration::from_millis(400));
    assert_eq!(h.app().0, 1);
    let (a, b) = (h.rect(Id::of("a")).unwrap(), h.rect(Id::of("b")).unwrap());
    assert!(r.y + r.h <= a.y && a.y + a.h <= b.y, "its rows under it: {r:?} {a:?} {b:?}");
    assert_eq!(
      read(&h).as_deref().map(str::trim),
      Some(r#"button "Other models" expanded desc "2 more""#)
    );
    let down = chevron(&mut h);
    assert!(right.0 < right.1 && down.1 < down.0, "the chevron turned: {right:?} {down:?}");

    h.set_focus(Some(fold));
    h.key(Key::Char(' '), Modifiers::empty());
    h.frame();
    assert_eq!(h.app().0, 2);
    assert!(h.rect(Id::of("a")).is_none(), "closed by Space");

    // No hover fill; held down, one over its piece of the plate, to the plate's edge and its
    // corners, square in the plate's middle.
    let at = |h: &mut Harness<Fold>| {
      let img = h.render();
      let y = r.y + r.h / 2.0;
      [img.pixel(r.x + r.w - 40.0, y), img.pixel(r.x + 0.5, y), img.pixel(r.x + 0.5, r.y + 0.5)]
    };
    h.leave();
    h.wait(Duration::from_millis(400));
    let rest = at(&mut h);
    h.move_to(r.x + r.w - 40.0, r.y + r.h / 2.0);
    h.wait(Duration::from_millis(400));
    assert_eq!(at(&mut h), rest, "no hover fill");
    h.press(r.x + r.w - 40.0, r.y + r.h / 2.0);
    h.frame();
    let held = at(&mut h);
    assert!(held[0] != rest[0] && held == [held[0]; 3], "pressed, full: {held:?} {rest:?}");
    h.release(r.x + r.w - 40.0, r.y + r.h / 2.0);
    h.frame();
    let n = h.app().0;
    h.click_id(Id::of("off"));
    h.frame();
    assert!(h.rect(Id::of("c")).is_none() && h.app().0 == n, "disabled");
  }

  /// A disclosure in its section, on the window's ground, alone or with a row of the app's own
  /// under it in the plate (`.0`).
  struct Held(bool);

  impl App for Held {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_theme(Theme::PLATES_LIGHT);
      let fs = FormStyle::plates(&ui.theme());
      let after = self.0;
      ui.col(fs.page.grow().bg(fs.bg), |ui| {
        ui.form_section(&fs, "Section", |ui| {
          ui.id("fold").form_disclosure(&fs, FormRow::new("Other models"), |_| {});
          if after {
            ui.row(Style::new().w(grow()).min_h(40.0), |_| {});
          }
        });
      });
    }
  }

  /// Held down, the plate's only row is filled to its edges and rounded at all four corners
  /// as the plate: at the top as the first row, at the bottom as the one the section ends with.
  #[test]
  fn a_held_rows_fill_is_rounded_where_the_plate_is() {
    let mut h = Harness::new(Held(false), Size::new(400.0, 200.0), 2.0);
    h.frame();
    let r = h.rect(Id::of("fold")).unwrap();
    let (x, y) = (r.x + r.w - 40.0, r.y + r.h / 2.0);
    h.press(x, y);
    h.frame();
    let img = h.render();
    let (fill, ground) = (img.pixel(x, y), img.pixel(r.x - 4.0, y));
    assert_ne!(fill, ground);
    assert_eq!((img.pixel(r.x + 0.5, y), img.pixel(r.x + r.w - 0.5, y)), (fill, fill));
    // A point in from each corner of the row's box: the ground, the corner cut off.
    let (l, rr, t, b) = (r.x + 0.5, r.x + r.w - 0.5, r.y + 0.5, r.y + r.h - 0.5);
    for (cx, cy) in [(l, t), (rr, t), (l, b), (rr, b)] {
      assert_eq!(img.pixel(cx, cy), ground, "rounded at ({cx}, {cy})");
    }
  }

  /// Held down with a row of the app's own under it in the plate, a row's fill keeps square
  /// bottom corners: the plate goes on under it.
  #[test]
  fn a_held_row_above_more_of_the_plate_is_square_at_its_bottom() {
    let mut h = Harness::new(Held(true), Size::new(400.0, 200.0), 2.0);
    h.frame();
    let r = h.rect(Id::of("fold")).unwrap();
    let (x, y) = (r.x + r.w - 40.0, r.y + r.h / 2.0);
    h.press(x, y);
    h.frame();
    let img = h.render();
    let (fill, ground) = (img.pixel(x, y), img.pixel(r.x - 4.0, y));
    assert_ne!(fill, ground);
    let (l, rr, t, b) = (r.x + 0.5, r.x + r.w - 0.5, r.y + 0.5, r.y + r.h - 0.5);
    for (cx, cy) in [(l, t), (rr, t)] {
      assert_eq!(img.pixel(cx, cy), ground, "rounded at ({cx}, {cy})");
    }

    for (cx, cy) in [(l, b), (rr, b)] {
      assert_eq!(img.pixel(cx, cy), fill, "square at ({cx}, {cy})");
    }
  }

  /// A click on the chevron toggles its row. Disabled, by the row or by [Ui::disabled] while it
  /// has focus: dimmed, read disabled, out of the Tab order, the focus let go, Space ignored.
  #[test]
  fn a_disclosure_toggles_by_its_chevron_and_disabled_takes_nothing() {
    let mut h = Harness::new(Fold::default(), Size::new(400.0, 300.0), 2.0);
    h.frame();
    let (fold, off) = (Id::of("fold"), Id::of("off"));
    h.click_id(fold.key("chevron"));
    h.frame();
    assert!(h.app().0 == 1 && h.rect(Id::of("a")).is_some(), "the chevron's click");

    let read = |h: &Harness<Fold>, name: &str| {
      let tree = h.a11y();
      tree.lines().map(str::trim).find(|l| l.contains(name)).unwrap_or_default().to_owned()
    };
    assert!(read(&h, "\"Off\"").contains("disabled"), "{}", read(&h, "\"Off\""));
    assert!(!read(&h, "\"Other models\"").contains("disabled"));
    // Its label in the tertiary colour, the other's in the text's.
    let ink = |h: &mut Harness<Fold>, id: Id| {
      let r = h.rect(id.key("label")).unwrap();
      let img = h.render();
      let px = (0..(r.w * 2.0) as i32).flat_map(|x| (0..(r.h * 2.0) as i32).map(move |y| (x, y)));
      px.map(|(x, y)| img.pixel(r.x + x as f32 / 2.0, r.y + y as f32 / 2.0)[0]).min().unwrap()
    };
    let (on_ink, off_ink) = (ink(&mut h, fold), ink(&mut h, off));
    assert!(off_ink > on_ink + 60, "dimmed: {on_ink} {off_ink}");
    h.set_focus(Some(fold));
    for _ in 0..3 {
      h.key(NamedKey::Tab, Modifiers::empty());
      assert_ne!(h.focused(), Some(off), "out of the Tab order");
    }

    h.set_focus(Some(fold));
    h.app_mut().1 = true;
    h.frame();
    assert_eq!(h.focused(), None, "the focus let go");
    assert!(read(&h, "\"Other models\"").contains("disabled"));
    h.key(Key::Char(' '), Modifiers::empty());
    h.frame();
    assert_eq!(h.app().0, 1, "Space ignored");
  }

  /// Toolbar tabs, disabled or not, in the light theme.
  struct Bar(bool);

  impl App for Bar {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_theme(Theme::PLATES_LIGHT);
      let fs = FormStyle::plates(&ui.theme());
      let tabs = [TabItem::new(0, Icon::GEAR, "General"), TabItem::new(1, Icon::INFO, "About")];
      ui.col(Style::new().grow().bg(fs.bg), |ui| {
        ui.disabled(self.0).id("bar").toolbar_tabs(&fs, &mut 0, &tabs);
      });
    }
  }

  /// A section titled by a string, one with a header built of a title and a note and a footer,
  /// and one with an empty title.
  struct Sections;

  impl App for Sections {
    fn view(&mut self, ui: &mut Ui) {
      ui.set_theme(Theme::PLATES_LIGHT);
      let fs = FormStyle::plates(&ui.theme());
      ui.col(fs.page, |ui| {
        ui.id("a").form_section(&fs, "Startup", |ui| ui.form_row(&fs, FormRow::new("A"), |_| {}));
        let head = |ui: &mut Ui, s: TextStyle| {
          ui.text("Storage", s);
          ui.spacer();
          ui.id("note").text("640 MB used", fs.note);
        };
        let section = Section::new(head).footer("Kept until you clear it.");
        ui.id("b").form_section(&fs, section, |ui| ui.form_row(&fs, FormRow::new("B"), |_| {}));
        ui.id("c")
          .form_section(&fs, "", |ui| ui.id("row").form_row(&fs, FormRow::new("C"), |_| {}));
      });
    }
  }

  /// The header over the plate, its note at the right on the title's line; the footer under the
  /// plate in the caption's style; an empty title shows no header.
  #[test]
  fn a_section_has_a_header_and_a_footer() {
    let mut h = Harness::new(Sections, Size::new(400.0, 400.0), 1.0);
    h.frame();
    let (title, note) = (h.rect(Id::of("b").key("header")), h.rect(Id::of("note")).unwrap());
    assert!(title.is_none(), "a built header has no text of its own");
    let b = h.rect(Id::of("b")).unwrap();
    assert!(note.x + note.w > b.x + b.w - 20.0, "the note at the right: {note:?} in {b:?}");
    let foot = h.rect(Id::of("b").key("footer")).unwrap();
    assert!(foot.y + foot.h <= b.y + b.h && foot.y > note.y + 30.0, "under the plate: {foot:?}");
    let Kind::Text { style, .. } = h.rt.ui.last.get(Id::of("b").key("footer")).unwrap().kind else {
      panic!()
    };
    assert_eq!(style, FormStyle::plates(&Theme::PLATES_LIGHT).caption);
    let (c, row) = (h.rect(Id::of("c")).unwrap(), h.rect(Id::of("row")).unwrap());
    assert_eq!(c.y, row.y, "no header");
    assert!(h.rect(Id::of("a").key("header")).is_some());
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
        ui.toolbar_tabs(&fs, &mut 0, &[TabItem::new(0, Icon::GEAR, "General")]);
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
