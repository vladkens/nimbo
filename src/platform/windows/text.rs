// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The text backend on DirectWrite: the system UI font (Segoe UI Variable, else Segoe UI) and a
//! monospaced one (Cascadia Mono, else Consolas), shaped with font fallback by an
//! IDWriteTextLayout whose glyph runs a text renderer of ours collects, metrics from the font
//! face, and glyphs drawn by IDWriteGlyphRunAnalysis: grayscale coverage (with smoothing, given
//! DirectWrite's contrast and gamma correction as it draws text itself), and colour emoji as
//! the COLR layers TranslateColorGlyphRun gives, each a coverage mask in its colour. Fonts are
//! shaped at their size in points (DIPs); only glyph bitmaps are made at the pixel size.
//!
//! The COM interfaces are called through their vtables by slot number (the order of
//! `dwrite.h`, `dwrite_2.h`), with no bindings crate: `windows-sys` has no COM.

use std::ffi::c_void;
use std::ptr::{NonNull, null, null_mut};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::RECT;
use windows_sys::core::{BOOL, GUID, HRESULT};

use crate::text::{Font, Glyph, GlyphBox, Line, Metrics, Raster, Weight};

// MARK: COM

/// Calls vtable slot `$i` of `$com` (a [Com]) with the interface pointer first:
/// `call!(com, 18, (u32) -> HRESULT; n)`.
macro_rules! call {
  ($com:expr, $i:expr, ($($t:ty),*) -> $r:ty; $($a:expr),*) => {{
    let c: &Com = &$com;
    let f: unsafe extern "system" fn(*mut c_void $(, $t)*) -> $r = c.slot($i);
    f(c.ptr() $(, $a)*)
  }};
}

/// An owned COM interface pointer: AddRef on clone, Release on drop.
struct Com(NonNull<c_void>);

impl Com {
  /// Takes the reference an out parameter returned, `None` for null.
  fn new(p: *mut c_void) -> Option<Self> {
    NonNull::new(p).map(Self)
  }

  /// A new reference to an interface DirectWrite lends a callback.
  fn borrowed(p: *mut c_void) -> Option<Self> {
    let com = std::mem::ManuallyDrop::new(Self::new(p)?);
    Some((*com).clone())
  }

  fn ptr(&self) -> *mut c_void {
    self.0.as_ptr()
  }

  /// The function in vtable slot `i`, of type `F` (an `extern "system"` fn pointer).
  unsafe fn slot<F: Copy>(&self, i: usize) -> F {
    // SAFETY: a COM object starts with its vtable pointer; the caller names the slot's type.
    unsafe {
      let vtbl = *self.ptr().cast::<*const usize>();
      std::mem::transmute_copy(&*vtbl.add(i))
    }
  }
}

impl Clone for Com {
  fn clone(&self) -> Self {
    unsafe { call!(self, 1, () -> u32;) };
    Self(self.0)
  }
}

impl Drop for Com {
  fn drop(&mut self) {
    unsafe { call!(self, 2, () -> u32;) };
  }
}

/// An interface from a call that returns it through its last parameter.
fn out(f: impl FnOnce(*mut *mut c_void) -> HRESULT) -> Option<Com> {
  let mut p = null_mut();
  match f(&mut p) {
    0.. => Com::new(p),
    _ => None,
  }
}

fn iid(g: &GUID) -> u128 {
  let tail = u64::from_be_bytes(g.data4) as u128;
  (g.data1 as u128) << 96 | (g.data2 as u128) << 80 | (g.data3 as u128) << 64 | tail
}

fn wide(s: &str) -> Vec<u16> {
  s.encode_utf16().chain([0]).collect()
}

const IID_IUNKNOWN: u128 = 0x00000000_0000_0000_c000_000000000046;
const IID_FACTORY2: u128 = 0x0439fc60_ca44_4994_8dee_3a9af7b732ec;
const IID_PIXEL_SNAPPING: u128 = 0xeaf3a2da_ecf4_4d24_b644_b34f6842024b;
const IID_TEXT_RENDERER: u128 = 0xef8a8135_5cc6_45fe_8825_c5a0724eb819;
const IID_RENDERING_PARAMS1: u128 = 0x94413cf4_a6fc_4248_8b50_6674348fcad3;
const E_NOINTERFACE: HRESULT = 0x80004002_u32 as i32;

// Vtable slots (IUnknown is 0–2).
const FACTORY_SYSTEM_FONTS: usize = 3;
const FACTORY_RENDERING_PARAMS: usize = 10;
const FACTORY_TEXT_FORMAT: usize = 15;
const FACTORY_TEXT_LAYOUT: usize = 18;
const FACTORY2_TRANSLATE_COLOR: usize = 28;
const FACTORY2_GLYPH_ANALYSIS: usize = 30;
const COLLECTION_FAMILY: usize = 4;
const COLLECTION_FIND: usize = 5;
const FAMILY_MATCH: usize = 7;
const FONT_CREATE_FACE: usize = 13;
const FACE_METRICS: usize = 8;
const FACE_TRY_TABLE: usize = 12;
const FACE_RELEASE_TABLE: usize = 13;
const FORMAT_WORD_WRAPPING: usize = 5;
const PARAMS_GAMMA: usize = 3;
const PARAMS1_GRAYSCALE_CONTRAST: usize = 8;
const LAYOUT_DRAW: usize = 58;
const LAYOUT_METRICS: usize = 60;
const ANALYSIS_BOUNDS: usize = 3;
const ANALYSIS_TEXTURE: usize = 4;
const COLOR_RUNS_NEXT: usize = 3;
const COLOR_RUNS_CURRENT: usize = 4;

const STRETCH_NORMAL: u32 = 5;
const STYLE_NORMAL: u32 = 0;
const NO_WRAP: u32 = 1;
const MEASURING_NATURAL: u32 = 0;
const RENDERING_NATURAL_SYMMETRIC: u32 = 5;
const GRID_FIT_DEFAULT: u32 = 0;
const ANTIALIAS_GRAYSCALE: u32 = 1;
const TEXTURE_ALIASED: u32 = 0;
/// A colour layer in the text's colour, not the palette's.
const PALETTE_TEXT: u16 = 0xFFFF;

#[repr(C)]
struct GlyphRun {
  face: *mut c_void,
  em: f32,
  count: u32,
  ids: *const u16,
  advances: *const f32,
  offsets: *const GlyphOffset,
  sideways: BOOL,
  bidi: u32,
}

impl GlyphRun {
  /// One glyph at no advance.
  fn one(face: &Com, em: f32, id: &u16) -> Self {
    static ZERO: (f32, GlyphOffset) = (0.0, GlyphOffset { advance: 0.0, ascender: 0.0 });
    let (ids, advances, offsets) = (id as *const u16, &ZERO.0 as *const f32, &ZERO.1 as *const _);
    Self { face: face.ptr(), em, count: 1, ids, advances, offsets, sideways: 0, bidi: 0 }
  }
}

#[repr(C)]
struct GlyphOffset {
  advance: f32,
  ascender: f32,
}

#[repr(C)]
struct RunDescription {
  locale: *const u16,
  string: *const u16,
  len: u32,
  cluster_map: *const u16,
  position: u32,
}

#[repr(C)]
struct ColorRun {
  run: GlyphRun,
  description: *mut RunDescription,
  x: f32,
  y: f32,
  color: [f32; 4],
  palette: u16,
}

#[repr(C)]
#[derive(Default)]
struct FontMetrics {
  upem: u16,
  ascent: u16,
  descent: u16,
  line_gap: i16,
  cap_height: u16,
  x_height: u16,
  underline_pos: i16,
  underline_thickness: u16,
  strike_pos: i16,
  strike_thickness: u16,
}

#[repr(C)]
#[derive(Default)]
struct TextMetrics {
  left: f32,
  top: f32,
  width: f32,
  width_trailing: f32,
  height: f32,
  layout_width: f32,
  layout_height: f32,
  bidi_depth: u32,
  lines: u32,
}

#[repr(C)]
struct Matrix([f32; 6]);

#[link(name = "dwrite")]
unsafe extern "system" {
  fn DWriteCreateFactory(kind: u32, iid: *const GUID, factory: *mut *mut c_void) -> HRESULT;
}

// MARK: Factory

/// The shared DirectWrite factory, the system font collection and the families found in it.
struct Dwrite {
  factory: Com,
  fonts: Com,
  /// For text below 20 pt, and from 20 pt.
  ui: [&'static str; 2],
  mono: &'static str,
  /// Smoothed coverage of light text and of dark text, by raw coverage: [alpha_tables].
  alpha: [[u8; 256]; 2],
}

// SAFETY: a shared DirectWrite factory and its system font collection are free-threaded.
unsafe impl Send for Dwrite {}
unsafe impl Sync for Dwrite {}

fn dwrite() -> &'static Dwrite {
  static DWRITE: OnceLock<Dwrite> = OnceLock::new();
  DWRITE.get_or_init(|| {
    let iid = GUID::from_u128(IID_FACTORY2);
    // DWRITE_FACTORY_TYPE_SHARED: 0.
    let factory = out(|p| unsafe { DWriteCreateFactory(0, &iid, p) }).expect("DirectWrite");
    let fonts = out(|p| unsafe {
      call!(factory, FACTORY_SYSTEM_FONTS, (*mut *mut c_void, BOOL) -> HRESULT; p, 0)
    })
    .expect("system fonts");
    let has = |name: &str| find_family(&fonts, name).is_some();
    let ui = match has("Segoe UI Variable Text") {
      true => ["Segoe UI Variable Text", "Segoe UI Variable Display"],
      false => ["Segoe UI", "Segoe UI"],
    };

    let mono = if has("Cascadia Mono") { "Cascadia Mono" } else { "Consolas" };
    let alpha = alpha_tables(&factory);
    Dwrite { factory, fonts, ui, mono, alpha }
  })
}

fn find_family(fonts: &Com, name: &str) -> Option<u32> {
  let name = wide(name);
  let (mut i, mut found) = (0u32, 0 as BOOL);
  let hr = unsafe {
    call!(fonts, COLLECTION_FIND, (*const u16, *mut u32, *mut BOOL) -> HRESULT;
      name.as_ptr(), &mut i, &mut found)
  };
  (hr >= 0 && found != 0).then_some(i)
}

/// The font face of `family` closest to `weight`.
fn match_face(dw: &Dwrite, family: &str, weight: u32) -> Option<Com> {
  let i = find_family(&dw.fonts, family)?;
  let fam = out(|p| unsafe {
    call!(dw.fonts, COLLECTION_FAMILY, (u32, *mut *mut c_void) -> HRESULT; i, p)
  })?;
  let font = out(|p| unsafe {
    call!(fam, FAMILY_MATCH, (u32, u32, u32, *mut *mut c_void) -> HRESULT;
      weight, STRETCH_NORMAL, STYLE_NORMAL, p)
  })?;
  out(|p| unsafe { call!(font, FONT_CREATE_FACE, (*mut *mut c_void) -> HRESULT; p) })
}

// MARK: Alpha correction

/// DirectWrite's gamma ratios for a target that blends in sRGB space ("gamma incorrect"), for
/// gamma 1.0 to 2.2 by 0.1. From src/dwrite.cpp of
/// https://github.com/lhecker/dwrite-hlsl (MIT License, Copyright (c) Microsoft Corporation),
/// which took them from DirectWrite.
const GAMMA_RATIOS: [[f32; 4]; 13] = [
  [0.0000, -0.0000, 0.0000, -0.0000],
  [0.0166, -0.0807, 0.2227, -0.0751],
  [0.0350, -0.1760, 0.4325, -0.1370],
  [0.0543, -0.2821, 0.6302, -0.1876],
  [0.0739, -0.3963, 0.8167, -0.2287],
  [0.0933, -0.5161, 0.9926, -0.2616],
  [0.1121, -0.6395, 1.1588, -0.2877],
  [0.1300, -0.7649, 1.3159, -0.3080],
  [0.1469, -0.8911, 1.4644, -0.3234],
  [0.1627, -1.0170, 1.6051, -0.3347],
  [0.1773, -1.1420, 1.7385, -0.3426],
  [0.1908, -1.2652, 1.8650, -0.3476],
  [0.2031, -1.3864, 1.9851, -0.3501],
];

/// What DirectWrite does to grayscale coverage when it draws text, as tables for light text
/// (white, `[0]`) and dark text (black, `[1]`) over our blending in sRGB space: the system's
/// grayscale enhanced contrast (dark text only), then its gamma correction. Without it glyphs
/// come out lighter and thinner than Windows draws them, most visibly light text on dark.
/// The steps are `DWrite_GetGrayScaleCorrectedAlpha` and `DWrite_GetGammaRatios` of
/// lhecker/dwrite-hlsl (src/dwrite.hlsl, src/dwrite.cpp; MIT, Microsoft).
fn alpha_tables(factory: &Com) -> [[u8; 256]; 2] {
  let params =
    out(|p| unsafe { call!(factory, FACTORY_RENDERING_PARAMS, (*mut *mut c_void) -> HRESULT; p) });
  // The system's values (ClearType Tuner), else DirectWrite's defaults.
  let gamma = params.as_ref().map_or(1.8, |p| unsafe { call!(p, PARAMS_GAMMA, () -> f32;) });
  let iid = GUID::from_u128(IID_RENDERING_PARAMS1);
  let params1 = params.and_then(|p| {
    out(|q| unsafe { call!(p, 0, (*const GUID, *mut *mut c_void) -> HRESULT; &iid, q) })
  });
  let contrast =
    params1.map_or(1.0, |p| unsafe { call!(p, PARAMS1_GRAYSCALE_CONTRAST, () -> f32;) }).max(0.0);

  let row = GAMMA_RATIOS[((gamma * 10.0 + 0.5) as usize).clamp(10, 22) - 10];
  // The source divides the table by 4 and multiplies it back with these.
  let (norm13, norm24) = (65536.0 / (255.0 * 255.0), 256.0 / 255.0);
  let g = [row[0] * norm13, row[1] * norm24, row[2] * norm13, row[3] * norm24];

  // The text colour's intensity and its contrast multiplier: none from a lightness of 0.75.
  let mut tables = [[0u8; 256]; 2];
  for (table, (f, k)) in tables.iter_mut().zip([(1.0, 0.0), (0.0, contrast)]) {
    for (i, v) in table.iter_mut().enumerate() {
      let a = i as f32 / 255.0;
      let a = a * (k + 1.0) / (a * k + 1.0);
      let a = a + a * (1.0 - a) * ((g[0] * f + g[1]) * a + (g[2] * f + g[3]));
      *v = (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    }
  }

  tables
}

// MARK: Faces

/// An IDWriteFontFace at a size in points, with the text format that shapes with it when it is
/// a system font (fallback faces from shaping have none).
#[derive(Clone)]
pub struct Face {
  face: Com,
  size: f32,
  color: bool,
  format: Option<Com>,
}

/// The same font face at the same size, with the same text format: requests that resolve to
/// one face keep their own format (its weight picks the fallback fonts), its clones share it.
impl PartialEq for Face {
  fn eq(&self, other: &Self) -> bool {
    let format = |f: &Self| f.format.as_ref().map(|c| c.0);
    self.face.0 == other.face.0 && self.size == other.size && format(self) == format(other)
  }
}

impl Face {
  fn new(face: Com, size: f32, format: Option<Com>) -> Self {
    let color = has_table(&face, *b"COLR");
    Self { face, size, color, format }
  }
}

/// Whether the font has the OpenType table `tag`.
fn has_table(face: &Com, tag: [u8; 4]) -> bool {
  let (mut data, mut size, mut cx, mut found) = (null(), 0u32, null_mut(), 0 as BOOL);
  let hr = unsafe {
    call!(face, FACE_TRY_TABLE,
      (u32, *mut *const c_void, *mut u32, *mut *mut c_void, *mut BOOL) -> HRESULT;
      u32::from_le_bytes(tag), &mut data, &mut size, &mut cx, &mut found)
  };
  let found = hr >= 0 && found != 0;
  if found {
    unsafe { call!(face, FACE_RELEASE_TABLE, (*mut c_void) -> (); cx) };
  }

  found
}

/// The system font (Segoe UI Variable Text, Display from 20 pt; or Cascadia Mono) at its size
/// and weight.
pub fn face(font: Font) -> Face {
  let dw = dwrite();
  let weight = match font.weight {
    Weight::Regular => 400,
    Weight::Medium => 500,
    Weight::Semibold => 600,
    Weight::Bold => 700,
  };

  let family = match font.mono {
    true => dw.mono,
    false => dw.ui[(font.size >= 20.0) as usize],
  };

  let face = match_face(dw, family, weight).expect("no system font");
  let (name, locale) = (wide(family), wide("en-us"));
  let format = out(|p| unsafe {
    call!(dw.factory, FACTORY_TEXT_FORMAT,
      (*const u16, *mut c_void, u32, u32, u32, f32, *const u16, *mut *mut c_void) -> HRESULT;
      name.as_ptr(), dw.fonts.ptr(), weight, STYLE_NORMAL, STRETCH_NORMAL, font.size,
      locale.as_ptr(), p)
  })
  .expect("text format");
  unsafe { call!(format, FORMAT_WORD_WRAPPING, (u32) -> HRESULT; NO_WRAP) };
  Face::new(face, font.size, Some(format))
}

pub fn metrics(face: &Face) -> Metrics {
  let mut m = FontMetrics::default();
  unsafe { call!(face.face, FACE_METRICS, (*mut FontMetrics) -> (); &mut m) };
  let k = face.size / m.upem.max(1) as f32;
  Metrics {
    ascent: m.ascent as f32 * k,
    descent: m.descent as f32 * k,
    leading: m.line_gap.max(0) as f32 * k,
    cap_height: m.cap_height as f32 * k,
    x_height: m.x_height as f32 * k,
  }
}

/// Its glyphs are colour layers (Segoe UI Emoji's COLR table).
pub fn is_color(face: &Face) -> bool {
  face.color
}

// MARK: Shaping

/// A glyph run the text layout drew: its face and how many glyphs it added.
struct Run {
  face: Com,
  size: f32,
  glyphs: usize,
}

/// Our IDWriteTextRenderer: collects the glyph runs of one `Draw`, in order.
#[repr(C)]
struct Collector {
  vtbl: &'static RendererVtbl,
  runs: Vec<Run>,
  ids: Vec<u16>,
  xs: Vec<f32>,
  /// Per glyph, its offset up from the baseline.
  ys: Vec<f32>,
  /// Per glyph, the UTF-16 offset where its cluster starts.
  clusters: Vec<u32>,
}

type Ctx = *const c_void;
type This = *mut Collector;

/// IDWriteTextRenderer: IUnknown, IDWritePixelSnapping, then its own four.
#[repr(C)]
struct RendererVtbl {
  query: unsafe extern "system" fn(This, *const GUID, *mut *mut c_void) -> HRESULT,
  add_ref: unsafe extern "system" fn(This) -> u32,
  release: unsafe extern "system" fn(This) -> u32,
  snapping_disabled: unsafe extern "system" fn(This, Ctx, *mut BOOL) -> HRESULT,
  transform: unsafe extern "system" fn(This, Ctx, *mut Matrix) -> HRESULT,
  pixels_per_dip: unsafe extern "system" fn(This, Ctx, *mut f32) -> HRESULT,
  glyph_run: unsafe extern "system" fn(
    This,
    Ctx,
    f32,
    f32,
    u32,
    *const GlyphRun,
    *const RunDescription,
    *mut c_void,
  ) -> HRESULT,
  underline: unsafe extern "system" fn(This, Ctx, f32, f32, *const c_void, *mut c_void) -> HRESULT,
  strikethrough:
    unsafe extern "system" fn(This, Ctx, f32, f32, *const c_void, *mut c_void) -> HRESULT,
  inline_object:
    unsafe extern "system" fn(This, Ctx, f32, f32, *mut c_void, BOOL, BOOL, *mut c_void) -> HRESULT,
}

/// The collector lives on the stack for one synchronous `Draw`: references are not counted.
/// Unsnapped positions in DIPs, no transform.
static RENDERER: RendererVtbl = RendererVtbl {
  query: collector_query,
  add_ref: collector_ref,
  release: collector_ref,
  snapping_disabled,
  transform,
  pixels_per_dip,
  glyph_run: collector_glyph_run,
  underline: no_line,
  strikethrough: no_line,
  inline_object: no_object,
};

unsafe extern "system" fn collector_query(
  this: This,
  riid: *const GUID,
  out: *mut *mut c_void,
) -> HRESULT {
  let ok = matches!(iid(unsafe { &*riid }), IID_IUNKNOWN | IID_PIXEL_SNAPPING | IID_TEXT_RENDERER);
  unsafe { *out = if ok { this.cast() } else { null_mut() } };
  if ok { 0 } else { E_NOINTERFACE }
}

unsafe extern "system" fn collector_ref(_: This) -> u32 {
  1
}

unsafe extern "system" fn snapping_disabled(_: This, _: Ctx, out: *mut BOOL) -> HRESULT {
  unsafe { *out = 1 };
  0
}

unsafe extern "system" fn transform(_: This, _: Ctx, out: *mut Matrix) -> HRESULT {
  unsafe { *out = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]) };
  0
}

unsafe extern "system" fn pixels_per_dip(_: This, _: Ctx, out: *mut f32) -> HRESULT {
  unsafe { *out = 1.0 };
  0
}

unsafe extern "system" fn no_line(
  _: This,
  _: Ctx,
  _: f32,
  _: f32,
  _: *const c_void,
  _: *mut c_void,
) -> HRESULT {
  0
}

unsafe extern "system" fn no_object(
  _: This,
  _: Ctx,
  _: f32,
  _: f32,
  _: *mut c_void,
  _: BOOL,
  _: BOOL,
  _: *mut c_void,
) -> HRESULT {
  0
}

/// Slices of a run's arrays; a null pointer (optional advances or offsets) is empty.
unsafe fn parts<'a, T>(p: *const T, n: usize) -> &'a [T] {
  match p.is_null() {
    true => &[],
    false => unsafe { std::slice::from_raw_parts(p, n) },
  }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "system" fn collector_glyph_run(
  this: This,
  _: Ctx,
  x: f32,
  _: f32,
  _: u32,
  run: *const GlyphRun,
  desc: *const RunDescription,
  _: *mut c_void,
) -> HRESULT {
  // SAFETY: DirectWrite passes our collector and a run and description valid for the call.
  let (c, run, desc) = unsafe { (&mut *this, &*run, &*desc) };
  let Some(face) = Com::borrowed(run.face) else { return 0 };
  let n = run.count as usize;
  let (ids, advances, offsets, map) = unsafe {
    (
      parts(run.ids, n),
      parts(run.advances, n),
      parts(run.offsets, n),
      parts(desc.cluster_map, desc.len as usize),
    )
  };
  // The cluster map gives each character its cluster's first glyph: a cluster is the run of
  // characters with one value, and owns the glyphs up to the next cluster's first.
  let at = c.clusters.len();
  c.clusters.resize(at + n, desc.position);
  let mut j = 0;
  while j < map.len() {
    let k = (j + 1..map.len()).find(|&k| map[k] != map[j]).unwrap_or(map.len());
    let end = map.get(k).map_or(n, |&g| (g as usize).min(n));
    for g in map[j] as usize..end {
      c.clusters[at + g] = desc.position + j as u32;
    }

    j = k;
  }

  // Right-to-left runs advance leftwards from their origin.
  let rtl = run.bidi & 1 == 1;
  let mut pen = 0.0;
  for (i, &id) in ids.iter().enumerate() {
    let adv = advances.get(i).copied().unwrap_or(0.0);
    let off = offsets.get(i).map_or(0.0, |o| o.advance);
    let gx = match rtl {
      true => x - pen - adv - off,
      false => x + pen + off,
    };

    pen += adv;
    c.ids.push(id);
    c.xs.push(gx);
    c.ys.push(offsets.get(i).map_or(0.0, |o| o.ascender));
  }

  c.runs.push(Run { face, size: run.em, glyphs: n });
  0
}

/// Shapes `s` with an IDWriteTextLayout (kerning, ligatures, font fallback); `index` gives the
/// text system's index of each run's font.
pub fn shape(face: &Face, s: &str, mut index: impl FnMut(Face) -> u16) -> Line {
  let dw = dwrite();
  let empty = Line { glyphs: Vec::new(), clusters: Vec::new(), width: 0.0 };
  let Some(format) = &face.format else { return empty };
  let text: Vec<u16> = s.encode_utf16().collect();
  let layout = out(|p| unsafe {
    call!(dw.factory, FACTORY_TEXT_LAYOUT,
      (*const u16, u32, *mut c_void, f32, f32, *mut *mut c_void) -> HRESULT;
      text.as_ptr(), text.len() as u32, format.ptr(), 1e6, 1e6, p)
  });
  let Some(layout) = layout else { return empty };
  let mut c = Collector {
    vtbl: &RENDERER,
    runs: Vec::new(),
    ids: Vec::new(),
    xs: Vec::new(),
    ys: Vec::new(),
    clusters: Vec::new(),
  };
  let mut m = TextMetrics::default();
  unsafe {
    call!(layout, LAYOUT_DRAW, (*mut c_void, This, f32, f32) -> HRESULT;
      null_mut(), &mut c, 0.0, 0.0);
    call!(layout, LAYOUT_METRICS, (*mut TextMetrics) -> HRESULT; &mut m);
  }

  // UTF-16 offsets to bytes, unless they are the same.
  let bytes: Vec<u32> = match s.is_ascii() {
    true => Vec::new(),
    false => s
      .char_indices()
      .flat_map(|(i, c)| std::iter::repeat_n(i as u32, c.len_utf16()))
      .chain([s.len() as u32])
      .collect(),
  };

  let at = |u: u32| match bytes.is_empty() {
    true => u,
    false => bytes.get(u as usize).copied().unwrap_or(s.len() as u32),
  };
  let mut glyphs = Vec::with_capacity(c.ids.len());
  let mut start = 0;
  for run in c.runs {
    let id = match run.face.0 == face.face.0 && run.size == face.size {
      true => index(face.clone()),
      false => index(Face::new(run.face, run.size, None)),
    };

    let end = start + run.glyphs;
    glyphs.extend((start..end).map(|i| Glyph { face: id, id: c.ids[i], x: c.xs[i], y: c.ys[i] }));
    start = end;
  }

  let clusters = c.clusters.iter().map(|&u| at(u)).collect();
  Line { glyphs, clusters, width: m.width_trailing }
}

// MARK: Glyph raster

/// Grayscale analysis of glyph `id` at `em` pixels, its pen `x` pixels right of the origin.
fn analysis(face: &Com, em: f32, id: u16, x: f32) -> Option<Com> {
  let run = GlyphRun::one(face, em, &id);
  out(|p| unsafe {
    call!(dwrite().factory, FACTORY2_GLYPH_ANALYSIS,
      (*const GlyphRun, *const Matrix, u32, u32, u32, u32, f32, f32, *mut *mut c_void) -> HRESULT;
      &run, null(), RENDERING_NATURAL_SYMMETRIC, MEASURING_NATURAL, GRID_FIT_DEFAULT,
      ANTIALIAS_GRAYSCALE, x, 0.0, p)
  })
}

/// The ink bounds of an analysis in pixels, y down from the baseline; `None` when empty.
fn bounds(a: &Com) -> Option<RECT> {
  let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
  let hr =
    unsafe { call!(a, ANALYSIS_BOUNDS, (u32, *mut RECT) -> HRESULT; TEXTURE_ALIASED, &mut r) };
  (hr >= 0 && r.right > r.left && r.bottom > r.top).then_some(r)
}

/// Coverage of `a` in `r`, one byte per pixel.
fn coverage(a: &Com, r: &RECT) -> Option<Vec<u8>> {
  let (w, h) = ((r.right - r.left) as usize, (r.bottom - r.top) as usize);
  let mut data = vec![0u8; w * h];
  let hr = unsafe {
    call!(a, ANALYSIS_TEXTURE, (u32, *const RECT, *mut u8, u32) -> HRESULT;
      TEXTURE_ALIASED, r, data.as_mut_ptr(), data.len() as u32)
  };
  (hr >= 0).then_some(data)
}

/// The colour layers of glyph `id`, bottom first, each a glyph of the face with its sRGB colour
/// (`None`: the text's); `None` when it has none.
fn color_layers(face: &Face, id: u16, em: f32) -> Option<Vec<(u16, Option<[f32; 4]>)>> {
  let run = GlyphRun::one(&face.face, em, &id);
  let runs = out(|p| unsafe {
    call!(dwrite().factory, FACTORY2_TRANSLATE_COLOR,
      (f32, f32, *const GlyphRun, *const RunDescription, u32, *const Matrix, u32,
        *mut *mut c_void) -> HRESULT;
      0.0, 0.0, &run, null(), MEASURING_NATURAL, null(), 0, p)
  })?;
  let mut layers = Vec::new();
  loop {
    let mut more = 0 as BOOL;
    let hr = unsafe { call!(runs, COLOR_RUNS_NEXT, (*mut BOOL) -> HRESULT; &mut more) };
    let mut cur: *const ColorRun = null();
    if hr < 0
      || more == 0
      || unsafe { call!(runs, COLOR_RUNS_CURRENT, (*mut *const ColorRun) -> HRESULT; &mut cur) } < 0
      || cur.is_null()
    {
      break;
    }
    // SAFETY: the current run stays valid until the next MoveNext.
    let cr = unsafe { &*cur };
    let color = (cr.palette != PALETTE_TEXT).then_some(cr.color);
    let ids = unsafe { parts(cr.run.ids, cr.run.count as usize) };
    layers.extend(ids.iter().map(|&id| (id, color)));
  }

  Some(layers)
}

/// The glyphs drawn for `id`: its colour layers in a colour face, else itself.
fn layers(face: &Face, id: u16, em: f32) -> Vec<(u16, Option<[f32; 4]>)> {
  let layers = face.color.then(|| color_layers(face, id, em)).flatten();
  layers.unwrap_or_else(|| vec![(id, None)])
}

/// The pixel box of glyph `id` at `scale`, drawn `subpixel` quarters of a pixel to the right:
/// its ink bounds (of every colour layer for a colour glyph), with one pixel of padding on each
/// side. `None` for a glyph with no ink (a space).
pub fn glyph_box(face: &Face, id: u16, scale: f32, subpixel: u8) -> Option<GlyphBox> {
  let (em, x) = (face.size * scale, subpixel as f32 / 4.0);
  let r = layers(face, id, em)
    .into_iter()
    .filter_map(|(id, _)| bounds(&analysis(&face.face, em, id, x)?))
    .reduce(|a, b| RECT {
      left: a.left.min(b.left),
      top: a.top.min(b.top),
      right: a.right.max(b.right),
      bottom: a.bottom.max(b.bottom),
    })?;
  let (w, h) = ((r.right - r.left + 2) as usize, (r.bottom - r.top + 2) as usize);
  Some(GlyphBox { left: r.left - 1, bottom: -r.bottom - 1, w, h })
}

/// Draws glyph `id` into the pixels of `b` (from [glyph_box] with the same `subpixel`), rows
/// top-down: coverage, one byte per pixel (with smoothing, corrected as DirectWrite draws it for
/// the text's polarity: [alpha_tables]), or premultiplied sRGB RGBA for a colour face (`color`):
/// its layers one over another, those in the text's colour black.
pub fn rasterize(
  face: &Face,
  id: u16,
  subpixel: u8,
  b: &GlyphBox,
  raster: Raster,
  color: bool,
) -> Option<Vec<u8>> {
  let (em, x) = (face.size * raster.scale, subpixel as f32 / 4.0);
  let top = -(b.bottom + b.h as i32);
  let r = RECT { left: b.left, top, right: b.left + b.w as i32, bottom: top + b.h as i32 };
  if !color {
    let mut cov = coverage(&analysis(&face.face, em, id, x)?, &r)?;
    if raster.smooth {
      let table = &dwrite().alpha[raster.dark as usize];
      cov.iter_mut().for_each(|k| *k = table[*k as usize]);
    }

    return Some(cov);
  }

  let mut rgba = vec![0u8; b.w * b.h * 4];
  for (id, c) in layers(face, id, em) {
    let Some(cov) = analysis(&face.face, em, id, x).and_then(|a| coverage(&a, &r)) else {
      continue;
    };
    let [cr, cg, cb, ca] = c.unwrap_or([0.0, 0.0, 0.0, 1.0]);
    for (px, &k) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(&cov) {
      let a = k as f32 / 255.0 * ca;
      if a <= 0.0 {
        continue;
      }

      for (d, s) in px.iter_mut().zip([cr * a, cg * a, cb * a, a]) {
        *d = (s * 255.0 + *d as f32 * (1.0 - a) + 0.5) as u8;
      }
    }
  }

  Some(rgba)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn requests_keep_their_format() {
    let regular = face(Font::ui(13.0));
    let medium = face(Font { weight: Weight::Medium, ..Font::ui(13.0) });
    assert!(regular == regular.clone(), "a clone shares the format");
    assert!(regular != medium, "another weight keeps its own format, even on the same face");
  }
}
