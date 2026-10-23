// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The paint pass: walks the laid-out tree into the frame's scene. Each box draws its shadow,
//! background and border, then its content (text lines, an image, a canvas callback), then its
//! children, clipped when it clips. Overlays follow, each in a new layer, by z. Elements outside
//! the window or their clip are skipped; text by where its ink reaches, not its box.

use super::layout::{ELLIPSIS, Kind, Tree};
use super::textlayout::{Edit, TextLayout};
use super::{Canvas, Clip, FocusRing, Style};
use crate::scene::{Color, Frame, Quad, Rect};
use crate::text::Ink;

/// Paints `t`; `ring` rings node `.0`, the focused one.
pub(super) fn paint(
  t: &Tree,
  canvases: &mut [Option<Canvas>],
  ring: Option<(usize, FocusRing)>,
  f: &mut Frame,
) {
  if t.nodes.len() <= 1 {
    return;
  }

  let window = Rect::new(0.0, 0.0, f.size.w, f.size.h);
  node(t, 0, 1.0, window, ring, canvases, f);
  for &i in &t.floats {
    f.layer();
    // Overlays keep their ancestors' opacity, not their clips.
    let mut alpha = 1.0;
    let mut p = t.nodes[i as usize].parent as usize;
    while p != 0 {
      alpha *= t.nodes[p].style.opacity;
      p = t.nodes[p].parent as usize;
    }

    match t.float_clip(i) {
      Some(c) => {
        f.push_clip(c);
        node(t, i as usize, alpha, c.intersect(window), ring, canvases, f);
        f.pop_clip();
      }
      None => node(t, i as usize, alpha, window, ring, canvases, f),
    }
  }
}

/// The content of scroll container `p` fading into `color` under its top and bottom edges, each
/// as strong as the content is scrolled away from it, full at `len` points.
fn edge_fade(t: &Tree, p: usize, color: Color, len: f32, alpha: f32, f: &mut Frame) {
  let n = &t.nodes[p];
  let (r, off) = (n.rect, n.scroll.y);
  let max = (n.content[1] - r.h).max(0.0);
  let len = len.min(r.h / 2.0);
  if len <= 0.0 {
    return;
  }

  for (k, d) in [off, max - off].into_iter().enumerate() {
    let a = (d / len).clamp(0.0, 1.0) * alpha;
    if a <= 0.0 {
      continue;
    }

    let (solid, clear) = (fade(color, a), fade(color, 0.0));
    let q = match k {
      0 => Quad::new(Rect::new(r.x, r.y, r.w, len), solid).gradient(clear, true),
      _ => Quad::new(Rect::new(r.x, r.y + r.h - len, r.w, len), clear).gradient(solid, true),
    };

    f.quad(q);
  }
}

fn fade(c: Color, alpha: f32) -> Color {
  [c[0], c[1], c[2], c[3] * alpha]
}

/// The box with what it draws outside itself: its shadow, and the focus ring when it has one.
fn outer(s: &Style, r: Rect, ring: Option<FocusRing>) -> Rect {
  let (mut x0, mut y0, mut x1, mut y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
  let sh = s.shadow;
  if sh.color[3] > 0.0 {
    let b = sh.blur * 2.0;
    (x0, y0) = (x0.min(r.x + sh.offset.x - b), y0.min(r.y + sh.offset.y - b));
    (x1, y1) = (x1.max(r.x + r.w + sh.offset.x + b), y1.max(r.y + r.h + sh.offset.y + b));
  }

  if let Some(ring) = ring {
    let o = ring.gap + ring.width;
    (x0, y0, x1, y1) =
      (x0.min(r.x - o), y0.min(r.y - o), x1.max(r.x + r.w + o), y1.max(r.y + r.h + o));
  }

  Rect::new(x0, y0, x1 - x0, y1 - y0)
}

fn visible(r: Rect, clip: Rect) -> bool {
  r.x < clip.x + clip.w && clip.x < r.x + r.w && r.y < clip.y + clip.h && clip.y < r.y + r.h
}

fn node(
  t: &Tree,
  i: usize,
  alpha: f32,
  clip: Rect,
  ring: Option<(usize, FocusRing)>,
  canvases: &mut [Option<Canvas>],
  f: &mut Frame,
) {
  let n = &t.nodes[i];
  let (s, r) = (&n.style, n.rect);
  let alpha = alpha * s.opacity;
  let leaf = n.end as usize == i + 1;
  let own = ring.filter(|&(k, _)| k == i).map(|(_, ring)| ring);
  let shown = |out: Rect| match n.kind {
    // Its ink can reach past its box: stacked accents, a line height less than the font's.
    Kind::Text { lines, baseline, line_h, .. } if !visible(out, clip) => {
      visible(ink_rect(t, lines, r, baseline, line_h), clip)
    }
    _ => visible(out, clip),
  };
  if alpha <= 0.0 || (leaf || s.clip != Clip::None) && !shown(outer(s, r, own)) {
    return;
  }

  decorate(s, r, alpha, f);
  // Outside the box, over its shadow.
  if let Some(ring) = own {
    let o = ring.gap + ring.width;
    let rect = Rect::new(r.x - o, r.y - o, r.w + 2.0 * o, r.h + 2.0 * o);
    let q = Quad::new(rect, [0.0; 4]).radii(s.radii.map(|v| v + o));
    f.quad(q.border(ring.width, fade(ring.color, alpha)));
  }

  match n.kind {
    Kind::Box => {}
    Kind::Text { range, style, lines, baseline, line_h } => {
      let edit = t.edit(i as u32).map(|k| &t.edits[k]);
      if let Some(e) = edit {
        selection(t, i, e, alpha, f);
      }

      let color = fade(style.color, alpha);
      let (mut first, mut last) = lines;
      if last - first > 1 {
        // Only the lines whose ink reaches into the clip, from their baselines.
        let ink = reach(t, lines);
        let (n, top) = ((last - first) as f32, r.y + baseline);
        first += ((clip.y - ink.down - top) / line_h).floor().clamp(0.0, n) as u32;
        last =
          lines.0 + ((clip.y + clip.h + ink.up - top) / line_h + 1.0).floor().clamp(0.0, n) as u32;
      }

      for k in first..last {
        let l = &t.lines[k as usize];
        let y = r.y + (k - lines.0) as f32 * line_h + baseline;
        let mut w = f.text(r.x, y, t.str(l.start, l.end), style.font, color);
        if style.ellipsis && lines.1 - lines.0 == 1 && l.end < range.1 {
          w += f.text(r.x + w, y, ELLIPSIS, style.font, color);
        }

        if style.strike && w > 0.0 {
          strike(r.x, y, w, style.font.size, color, f);
        }
      }

      if let Some(e) = edit {
        caret(t, i, e, style.color, baseline, alpha, f);
      }
    }
    Kind::Image(id) => f.image(id, r, alpha),
    Kind::Fade { color, len } => edge_fade(t, n.parent as usize, color, len, alpha, f),
    Kind::Canvas(k) => {
      if let Some(paint) = canvases[k as usize].take() {
        paint(f, r);
      }

      // Its children over all it drew: a layer draws its glyphs and lines over its quads.
      if !leaf {
        f.layer();
      }
    }
  }

  if leaf {
    return;
  }

  let clip = match s.clip_rect(r) {
    Some(r) => {
      f.push_clip(r);
      clip.intersect(r)
    }
    None => clip,
  };

  for c in t.children(i as u32) {
    if t.nodes[c as usize].float.is_none() {
      node(t, c as usize, alpha, clip, ring, canvases, f);
    }
  }

  if s.clip != Clip::None {
    f.pop_clip();
  }
}

/// The farthest the ink of lines `lines` of [Tree::lines] reaches past their pens: `right` past
/// their ends; a point more each way for the pixel snapping.
fn reach(t: &Tree, lines: (u32, u32)) -> Ink {
  let mut r = Ink::default();
  for l in &t.lines[lines.0 as usize..lines.1 as usize] {
    let i = l.ink;
    (r.left, r.right) = (r.left.max(i.left), r.right.max(l.width + i.right));
    (r.up, r.down) = (r.up.max(i.up), r.down.max(i.down));
  }

  Ink { left: r.left + 1.0, right: r.right + 1.0, up: r.up + 1.0, down: r.down + 1.0 }
}

/// Where a text box at `r` draws its lines' ink.
fn ink_rect(t: &Tree, lines: (u32, u32), r: Rect, baseline: f32, line_h: f32) -> Rect {
  let ink = reach(t, lines);
  let (top, n) = (r.y + baseline, lines.1.saturating_sub(lines.0).max(1) as f32);
  let bottom = top + (n - 1.0) * line_h;
  Rect::new(r.x - ink.left, top - ink.up, ink.left + ink.right, bottom + ink.down - top + ink.up)
}

/// The selection of an editable or selectable text, under its glyphs.
fn selection(t: &Tree, i: usize, e: &Edit, alpha: f32, f: &mut Frame) {
  let (a, b) = (e.sel.0.min(e.sel.1), e.sel.0.max(e.sel.1));
  if a == b {
    return;
  }

  let Some(lay) = TextLayout::at(t, i as u32) else { return };
  let color = fade(e.sel_color, alpha);
  lay.rects(a as usize..b as usize, |r| f.rect(r, color, 0.0));
}

/// The caret, and the marked text's underline in the text's colour: the composition shows at
/// the caret, apart from the committed text by its underline.
fn caret(t: &Tree, i: usize, e: &Edit, text: Color, baseline: f32, alpha: f32, f: &mut Frame) {
  let marked = e.marked.0 < e.marked.1;
  if !e.caret && !marked {
    return;
  }

  let Some(lay) = TextLayout::at(t, i as u32) else { return };
  if marked {
    let color = fade(text, alpha * 0.8);
    let (a, b) = (e.marked.0 as usize, e.marked.1 as usize);
    lay.rects(a..b, |r| {
      let y = (r.y + baseline + 2.0).min(r.y + r.h - 1.0);
      f.rect(Rect::new(r.x, y, r.w, 1.0), color, 0.0);
    });
  }

  if e.caret {
    let c = lay.caret(e.sel.1 as usize);
    // 1.5 points wide, from half a point before the stop: on a pixel boundary at 2x.
    let x = (c.x - 0.5).max(lay.rect.x - 1.0);
    f.rect(Rect::new(x, c.y, 1.5, c.h), fade(e.caret_color, alpha), 0.0);
  }
}

/// A strikethrough `w` wide from `x` over a line with its baseline at `y`: at half the
/// x-height (about 0.53 em in SF Pro), a point thick up to 14 points, on whole pixels.
fn strike(x: f32, y: f32, w: f32, size: f32, color: Color, f: &mut Frame) {
  let h = (size / 14.0).max(1.0);
  let top = ((y - size * 0.27 - h / 2.0) * f.scale).round() / f.scale;
  f.rect(Rect::new(x, top, w, h), color, 0.0);
}

/// Shadow, background and border.
fn decorate(s: &Style, r: Rect, alpha: f32, f: &mut Frame) {
  let sh = s.shadow;
  if sh.color[3] > 0.0 {
    let rect = Rect::new(r.x + sh.offset.x, r.y + sh.offset.y, r.w, r.h);
    f.quad(Quad::new(rect, fade(sh.color, alpha)).radii(s.radii).blur(sh.blur));
  }

  let border = s.border.iter().any(|&w| w > 0.0) && s.border_color[3] > 0.0;
  if s.bg[3] > 0.0 || border {
    let q = Quad::new(r, fade(s.bg, alpha)).radii(s.radii);
    f.quad(if border { q.borders(s.border, fade(s.border_color, alpha)) } else { q });
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::scene::{Images, Scene, Size};
  use crate::text::{Font, TextSystem};
  use crate::ui::{Float, Id, Sys, TextStyle, Ui, UiState, px};

  #[test]
  fn paints_tree_then_overlays() {
    let (size, scale) = (Size::new(200.0, 100.0), 2.0);
    let (mut scene, mut text, mut images) =
      (Scene::default(), TextSystem::new(scale), Images::default());
    let label = TextStyle::new(Font::ui(13.0), [1.0; 4]);
    let mut drawn = None;
    let mut ui = Ui::new(UiState::default(), size, None, Duration::ZERO);
    let panel = Style::new().w(px(100.0)).h(px(50.0)).bg([0.2, 0.2, 0.2, 1.0]);
    ui.id("panel").col(
      panel.shadow(crate::ui::Shadow::new(2.0, 4.0, [0.0, 0.0, 0.0, 0.5])).clip(),
      |ui| {
        ui.text("hi", label);
        ui.float(Float::parent().attach(1.0, 0.0)).col(panel.opacity(0.5), |ui| {
          ui.text("tip", label);
        });
        ui.canvas(Style::new().w(px(10.0)).h(px(10.0)), |_, r| drawn = Some(r));
      },
    );
    // Outside the window: not painted.
    ui.float(Float::window().offset(500.0, 0.0)).text("away", label);
    let mut sys = Sys { text: &mut text, images: &images, scale };
    ui.layout(&mut sys, scale);
    let mut f = Frame {
      size,
      scale,
      mouse: None,
      now: Duration::ZERO,
      scene: &mut scene,
      text: &mut text,
      images: &mut images,
      animate: false,
    };
    let st = ui.paint(&mut f);

    let canvas = st.last.nodes.iter().find(|n| matches!(n.kind, Kind::Canvas(_))).unwrap();
    assert_eq!(drawn, Some(canvas.rect));
    // Layers: the panel, its clipped children, the overlay (reusing the empty layer after the
    // clip), then the culled one.
    let layers = scene.layers();
    assert_eq!(layers.len(), 4);
    assert_eq!(layers[0].quads.len(), 2);
    assert_eq!(layers[1].clip, st.last.rect(Id::of("panel")));
    assert_eq!(layers[1].glyphs.len(), 2);
    assert_eq!((layers[2].clip, layers[2].quads.len(), layers[2].glyphs.len()), (None, 1, 3));
    assert_eq!(layers[2].quads[0].color[3], 0.5);
    assert_eq!(scene.counts().quads + scene.counts().glyphs, 8);
  }

  /// The glyphs painted of `body(ui)` in a 100 × 50 window.
  fn glyphs(body: impl FnOnce(&mut Ui)) -> usize {
    let (size, scale) = (Size::new(100.0, 50.0), 1.0);
    let (mut scene, mut text, mut images) =
      (Scene::default(), TextSystem::new(scale), Images::default());
    let mut ui = Ui::new(UiState::default(), size, None, Duration::ZERO);
    body(&mut ui);
    let mut sys = Sys { text: &mut text, images: &images, scale };
    ui.layout(&mut sys, scale);
    let mut f = Frame {
      size,
      scale,
      mouse: None,
      now: Duration::ZERO,
      scene: &mut scene,
      text: &mut text,
      images: &mut images,
      animate: false,
    };
    ui.paint(&mut f);
    scene.counts().glyphs
  }

  #[test]
  fn ink_above_its_box_paints() {
    let font = Font::ui(13.0);
    let tall = format!("e{}", "\u{301}".repeat(24));
    let up = TextSystem::new(1.0).layout(&tall, font).ink.up;
    assert!(up > 4.0 * font.size, "the accents stack {up} pt high");
    let style = TextStyle::new(font, [1.0; 4]).nowrap();
    // The box 2 em below the window, its accents up in it.
    let below =
      glyphs(|ui| ui.float(Float::window().offset(0.0, 50.0 + 2.0 * 13.0)).text(&tall, style));
    assert!(below > 0);
    // The paragraph's first line in the window, the second's box below it, its accents up in it.
    let tight = style.line_h(13.0);
    let text = format!("a\n{tall}");
    let lines = glyphs(|ui| ui.float(Float::window().offset(0.0, 40.0)).text(&text, tight));
    assert!(lines > 1, "{lines} glyphs");
  }

  #[test]
  fn a_shadow_in_view_paints_though_its_box_is_not() {
    let (size, scale) = (Size::new(100.0, 50.0), 1.0);
    let (mut scene, mut text, mut images) =
      (Scene::default(), TextSystem::new(scale), Images::default());
    let mut ui = Ui::new(UiState::default(), size, None, Duration::ZERO);
    // 12 above the window, 10 high, its shadow 4 lower with no blur: 2 rows of it show.
    let shadow = crate::ui::Shadow::new(4.0, 0.0, [0.0, 0.0, 0.0, 1.0]);
    let s = Style::new().w(px(20.0)).h(px(10.0)).bg([1.0; 4]).shadow(shadow);
    ui.float(Float::window().offset(0.0, -12.0)).el(s);
    ui.float(Float::window().offset(0.0, -40.0)).el(s);
    let mut sys = Sys { text: &mut text, images: &images, scale };
    ui.layout(&mut sys, scale);
    let mut f = Frame {
      size,
      scale,
      mouse: None,
      now: Duration::ZERO,
      scene: &mut scene,
      text: &mut text,
      images: &mut images,
      animate: false,
    };
    ui.paint(&mut f);
    // The first one's shadow and box; nothing of the one far above.
    assert_eq!(scene.counts().quads, 2);
  }

  /// The focus ring paints over the element's shadow, which reaches past its box as the ring
  /// does.
  #[test]
  fn the_focus_ring_paints_over_the_shadow() {
    let (size, scale) = (Size::new(100.0, 50.0), 1.0);
    let (mut scene, mut text, mut images) =
      (Scene::default(), TextSystem::new(scale), Images::default());
    let mut ui = Ui::new(UiState::default(), size, None, Duration::ZERO);
    let black = [0.0, 0.0, 0.0, 1.0];
    let shadow = crate::ui::Shadow::new(4.0, 0.0, black);
    ui.id("box").el(Style::new().w(px(20.0)).h(px(10.0)).bg([1.0; 4]).shadow(shadow));
    let mut sys = Sys { text: &mut text, images: &images, scale };
    ui.layout(&mut sys, scale);
    let mut f = Frame {
      size,
      scale,
      mouse: None,
      now: Duration::ZERO,
      scene: &mut scene,
      text: &mut text,
      images: &mut images,
      animate: false,
    };
    let i = ui.st.tree.find(Id::of("box")).unwrap() as usize;
    let ring = FocusRing::new([1.0, 0.0, 0.0, 1.0]);
    paint(&ui.st.tree, &mut ui.canvases, Some((i, ring)), &mut f);
    let quads = &scene.layers()[0].quads;
    let shadow = quads.iter().position(|q| q.color == black).unwrap();
    let ring = quads.iter().position(|q| q.border_color == [1.0, 0.0, 0.0, 1.0]).unwrap();
    assert!(ring > shadow, "the ring {ring}, the shadow {shadow}");
  }

  #[test]
  fn struck_text_gets_a_line_per_line() {
    let (size, scale) = (Size::new(200.0, 100.0), 2.0);
    let (mut scene, mut text, mut images) =
      (Scene::default(), TextSystem::new(scale), Images::default());
    let plain = TextStyle::new(Font::ui(13.0), [1.0, 0.0, 0.0, 1.0]);
    let mut ui = Ui::new(UiState::default(), size, None, Duration::ZERO);
    ui.col(Style::new().w(px(60.0)), |ui| {
      ui.id("plain").text("Buy milk", plain);
      // Wraps to two lines at 60 points.
      ui.id("done").text("Buy milk and eggs", plain.strike());
    });
    let mut sys = Sys { text: &mut text, images: &images, scale };
    ui.layout(&mut sys, scale);
    let mut f = Frame {
      size,
      scale,
      mouse: None,
      now: Duration::ZERO,
      scene: &mut scene,
      text: &mut text,
      images: &mut images,
      animate: false,
    };
    let st = ui.paint(&mut f);
    let quads = &scene.layers()[0].quads;
    assert_eq!(quads.len(), 2);
    let done = st.last.get(Id::of("done")).unwrap();
    let Kind::Text { lines, baseline, line_h, .. } = done.kind else { panic!() };
    assert_eq!(lines.1 - lines.0, 2);
    for (k, q) in quads.iter().enumerate() {
      let base = done.rect.y + k as f32 * line_h + baseline;
      // Above the baseline, under the top of the lowercase letters, in the text's colour.
      assert!(q.rect.y < base - 2.0 && q.rect.y > base - 7.0, "{:?} over {base}", q.rect);
      assert_eq!((q.rect.x, q.rect.h, q.color), (done.rect.x, 1.0, plain.color));
      assert!(q.rect.w > 20.0 && q.rect.w <= 60.0);
    }
  }

  #[test]
  fn canvas_children_lay_out_and_paint_over_it() {
    let (size, scale) = (Size::new(200.0, 100.0), 1.0);
    let (mut scene, mut text, mut images) =
      (Scene::default(), TextSystem::new(scale), Images::default());
    let mut ui = Ui::new(UiState::default(), size, None, Duration::ZERO);
    let paint = |f: &mut Frame, r: Rect| {
      f.rect(r, [0.0, 0.0, 1.0, 1.0], 0.0);
      f.text(r.x + 12.0, r.y + 24.0, "chart", Font::ui(13.0), [1.0; 4]);
    };
    ui.id("chart").canvas_row(Style::new().w(px(120.0)).h(px(80.0)).pad(10.0), paint, |ui| {
      ui.id("plot").el(Style::new().w(crate::ui::grow()).h(crate::ui::grow()).bg([1.0; 4]));
      ui.id("axis").el(Style::new().w(px(30.0)).h(crate::ui::grow()));
    });
    let mut sys = Sys { text: &mut text, images: &images, scale };
    ui.layout(&mut sys, scale);
    let mut f = Frame {
      size,
      scale,
      mouse: None,
      now: Duration::ZERO,
      scene: &mut scene,
      text: &mut text,
      images: &mut images,
      animate: false,
    };
    let st = ui.paint(&mut f);
    assert_eq!(st.last.rect(Id::of("plot")), Some(Rect::new(10.0, 10.0, 70.0, 60.0)));
    assert_eq!(st.last.rect(Id::of("axis")), Some(Rect::new(80.0, 10.0, 30.0, 60.0)));
    // The canvas first, over its whole box, its text with it; then the children in a layer over
    // it, so the plot's ground covers the canvas's text too.
    let layers = scene.layers();
    let (quads, glyphs) = (&layers[0].quads, layers[0].glyphs.len());
    assert_eq!(quads.len(), 1);
    assert!(glyphs > 0);
    assert_eq!(
      (quads[0].rect, quads[0].color),
      (Rect::new(0.0, 0.0, 120.0, 80.0), [0.0, 0.0, 1.0, 1.0])
    );
    assert_eq!((layers[1].quads.len(), layers[1].quads[0].color), (1, [1.0; 4]));
    // Children take the pointer, innermost first.
    let mut hits = Vec::new();
    crate::ui::hit(&st.last, crate::scene::Point::new(20.0, 20.0), &mut hits);
    assert_eq!(hits, [Id::of("plot"), Id::of("chart")]);
  }
}
