// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The text backend on the system's libraries, loaded at run time ([super::dl]): fontconfig
//! picks the UI font (`sans-serif`) and the monospaced one (`monospace`) and the fallback chain
//! (`FcFontSort`, by charset per character), HarfBuzz shapes (its own OpenType functions, at the
//! font's units per em: unhinted advances, scaled to points), FreeType gives metrics and draws
//! glyphs: grayscale coverage with light hinting (vertical only, so subpixel x positions stay;
//! none when fontconfig turns hinting off), colour glyphs (CBDT strikes scaled to the size, COLR
//! layers) as RGBA.
//!
//! Process-wide state: the libraries, fontconfig's config, one FreeType library, and every font
//! file opened once (`Arc`, shared across text systems and threads), each FT_Face behind its
//! own lock: FreeType faces are not thread-safe.

use std::ffi::{CStr, CString, c_char, c_int, c_long, c_uint, c_void};
use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use super::dl::{Lib, fns};
use crate::text::edit::is_extend;
use crate::text::{Font, Glyph, GlyphBox, Ink, Line, Metrics, Raster};

// MARK: FFI

type P = *mut c_void;

fns!(Fc {
  fn FcInitLoadConfigAndFonts() -> P;
  fn FcPatternCreate() -> P;
  fn FcPatternDestroy(p: P);
  fn FcPatternAddString(p: P, obj: *const c_char, s: *const c_char) -> c_int;
  fn FcPatternAddInteger(p: P, obj: *const c_char, i: c_int) -> c_int;
  fn FcPatternGetString(p: P, obj: *const c_char, n: c_int, s: *mut *const c_char) -> c_int;
  fn FcPatternGetInteger(p: P, obj: *const c_char, n: c_int, i: *mut c_int) -> c_int;
  fn FcPatternGetBool(p: P, obj: *const c_char, n: c_int, b: *mut c_int) -> c_int;
  fn FcPatternGetCharSet(p: P, obj: *const c_char, n: c_int, cs: *mut P) -> c_int;
  fn FcConfigSubstitute(cfg: P, p: P, kind: c_int) -> c_int;
  fn FcDefaultSubstitute(p: P);
  fn FcFontMatch(cfg: P, p: P, res: *mut c_int) -> P;
  fn FcFontSort(cfg: P, p: P, trim: c_int, csp: *mut P, res: *mut c_int) -> *mut FcFontSet;
  fn FcCharSetHasChar(cs: P, c: u32) -> c_int;
});

#[repr(C)]
struct FcFontSet {
  nfont: c_int,
  sfont: c_int,
  fonts: *mut P,
}

const FC_MATCH_PATTERN: c_int = 0;
const FC_RESULT_MATCH: c_int = 0;
/// `FC_WEIGHT_REGULAR`, `_MEDIUM`, `_DEMIBOLD`, `_BOLD`, by [crate::text::Weight].
const FC_WEIGHTS: [c_int; 4] = [80, 100, 180, 200];

fns!(Ft {
  fn FT_Init_FreeType(lib: *mut P) -> c_int;
  fn FT_New_Face(lib: P, path: *const c_char, index: c_long, face: *mut *mut FtFaceRec) -> c_int;
  fn FT_Set_Char_Size(f: *mut FtFaceRec, w: c_long, h: c_long, x: c_uint, y: c_uint) -> c_int;
  fn FT_Select_Size(face: *mut FtFaceRec, strike: c_int) -> c_int;
  fn FT_Set_Transform(face: *mut FtFaceRec, matrix: P, delta: *mut [c_long; 2]);
  fn FT_Load_Glyph(face: *mut FtFaceRec, glyph: c_uint, flags: i32) -> c_int;
  fn FT_Render_Glyph(slot: *mut FtGlyphSlotRec, mode: c_int) -> c_int;
});

// <freetype/freetype.h>: the public head of FT_FaceRec and FT_GlyphSlotRec, up to the fields
// read.
#[repr(C)]
struct FtFaceRec {
  num_faces: c_long,
  face_index: c_long,
  face_flags: c_long,
  style_flags: c_long,
  num_glyphs: c_long,
  family_name: *const c_char,
  style_name: *const c_char,
  num_fixed_sizes: c_int,
  available_sizes: *const FtBitmapSize,
  num_charmaps: c_int,
  charmaps: P,
  generic: [P; 2],
  bbox: [c_long; 4],
  units_per_em: u16,
  ascender: i16,
  descender: i16,
  height: i16,
  max_advance_width: i16,
  max_advance_height: i16,
  underline_position: i16,
  underline_thickness: i16,
  glyph: *mut FtGlyphSlotRec,
}

#[repr(C)]
struct FtBitmapSize {
  height: i16,
  width: i16,
  size: c_long,
  x_ppem: c_long,
  y_ppem: c_long,
}

#[repr(C)]
struct FtGlyphSlotRec {
  library: P,
  face: P,
  next: P,
  glyph_index: c_uint,
  generic: [P; 2],
  metrics: [c_long; 8],
  linear_hori_advance: c_long,
  linear_vert_advance: c_long,
  advance: [c_long; 2],
  format: c_uint,
  bitmap: FtBitmap,
  bitmap_left: c_int,
  bitmap_top: c_int,
}

#[repr(C)]
struct FtBitmap {
  rows: c_uint,
  width: c_uint,
  pitch: c_int,
  buffer: *mut u8,
  num_grays: u16,
  pixel_mode: u8,
  palette_mode: u8,
  palette: P,
}

const FT_FACE_FLAG_SCALABLE: c_long = 1;
const FT_FACE_FLAG_COLOR: c_long = 1 << 14;
const FT_LOAD_NO_HINTING: i32 = 1 << 1;
const FT_LOAD_NO_BITMAP: i32 = 1 << 3;
const FT_LOAD_COLOR: i32 = 1 << 20;
/// `FT_LOAD_TARGET_(FT_RENDER_MODE_LIGHT)`.
const FT_LOAD_TARGET_LIGHT: i32 = 1 << 16;
const FT_RENDER_MODE_NORMAL: c_int = 0;
const FT_PIXEL_MODE_GRAY: u8 = 2;
const FT_PIXEL_MODE_BGRA: u8 = 7;

fns!(Hb {
  fn hb_blob_create_from_file(path: *const c_char) -> P;
  fn hb_face_create(blob: P, index: c_uint) -> P;
  fn hb_face_get_upem(face: P) -> c_uint;
  fn hb_font_create(face: P) -> P;
  fn hb_font_set_scale(font: P, x: c_int, y: c_int);
  fn hb_font_set_var_named_instance(font: P, instance: c_uint);
  fn hb_buffer_create() -> P;
  fn hb_buffer_destroy(buf: P);
  fn hb_buffer_add_utf8(buf: P, text: *const c_char, len: c_int, offset: c_uint, item: c_int);
  fn hb_buffer_guess_segment_properties(buf: P);
  fn hb_shape(font: P, buf: P, features: *const c_void, n: c_uint);
  fn hb_buffer_get_glyph_infos(buf: P, len: *mut c_uint) -> *const HbGlyphInfo;
  fn hb_buffer_get_glyph_positions(buf: P, len: *mut c_uint) -> *const HbGlyphPosition;
  fn hb_ot_metrics_get_position(font: P, tag: u32, position: *mut i32) -> c_int;
});

#[repr(C)]
struct HbGlyphInfo {
  codepoint: u32,
  mask: u32,
  cluster: u32,
  var1: u32,
  var2: u32,
}

#[repr(C)]
struct HbGlyphPosition {
  x_advance: i32,
  y_advance: i32,
  x_offset: i32,
  y_offset: i32,
  var: u32,
}

/// The libraries and their process-wide objects.
struct Sys {
  fc: Fc,
  ft: Ft,
  hb: Hb,
  config: P,
  library: P,
}

// SAFETY: fontconfig's config is used only under [cache]'s lock, the FreeType library only to
// open faces, also under it.
unsafe impl Send for Sys {}
unsafe impl Sync for Sys {}

fn sys() -> &'static Sys {
  static SYS: OnceLock<Sys> = OnceLock::new();
  SYS.get_or_init(|| {
    let open = |n: &CStr| Lib::open(&[n]).unwrap_or_else(|| panic!("nimbo: can't load {n:?}"));
    let fc = Fc::load(&open(c"libfontconfig.so.1")).expect("nimbo: fontconfig symbols");
    let ft = Ft::load(&open(c"libfreetype.so.6")).expect("nimbo: FreeType symbols");
    let hb = Hb::load(&open(c"libharfbuzz.so.0")).expect("nimbo: HarfBuzz symbols");
    let config = unsafe { (fc.FcInitLoadConfigAndFonts)() };
    let mut library = std::ptr::null_mut();
    let err = unsafe { (ft.FT_Init_FreeType)(&mut library) };
    assert!(err == 0 && !config.is_null(), "nimbo: fontconfig or FreeType failed to start");
    Sys { fc, ft, hb, config, library }
  })
}

// MARK: Font files

/// A font file (and face index) opened once: its FreeType face for metrics and glyphs, its
/// HarfBuzz font for shaping.
struct FontFile {
  path: CString,
  index: c_int,
  ft: Mutex<FtFace>,
  /// Immutable once made, so shaping from any thread is safe.
  hb: P,
  upem: f32,
  /// Ascender, descender (positive down) and line height in font units.
  ascent: f32,
  descent: f32,
  height: f32,
  /// Cap and x-height (OS/2), in font units; 0 when the font has none.
  cap: f32,
  x: f32,
  color: bool,
  /// For a font of bitmap strikes only (CBDT emoji): the largest strike and its pixel size.
  strike: Option<(c_int, f32)>,
  /// Whether fontconfig asked for hinting; `load` follows it unless the font is colour.
  hinting: bool,
  load: i32,
}

struct FtFace {
  face: *mut FtFaceRec,
  /// The pixel size set last: it changes only when a glyph of another size is drawn.
  px: f32,
}

// SAFETY: the FT_Face is used only under its mutex; the hb_font is immutable.
unsafe impl Send for FontFile {}
unsafe impl Sync for FontFile {}

impl FontFile {
  /// Opens `path` at `index` (fontconfig's: a named instance in the high 16 bits); `hinting`
  /// as fontconfig asks for the font it matched.
  fn open(path: &CStr, index: c_int, hinting: bool) -> Option<Self> {
    let s = sys();
    let mut face = std::ptr::null_mut();
    let err = unsafe { (s.ft.FT_New_Face)(s.library, path.as_ptr(), index as c_long, &mut face) };
    if err != 0 || face.is_null() {
      return None;
    }

    let f = unsafe { &*face };
    let color = f.face_flags & FT_FACE_FLAG_COLOR != 0;
    let strike = match f.face_flags & FT_FACE_FLAG_SCALABLE != 0 || f.num_fixed_sizes <= 0 {
      true => None,
      false => {
        let n = f.num_fixed_sizes as usize;
        let sizes = unsafe { std::slice::from_raw_parts(f.available_sizes, n) };
        let (i, b) = sizes.iter().enumerate().max_by_key(|(_, b)| b.y_ppem)?;
        Some((i as c_int, b.y_ppem as f32 / 64.0))
      }
    };

    let load = match (color, hinting) {
      (true, _) => FT_LOAD_COLOR | FT_LOAD_NO_HINTING,
      (false, true) => FT_LOAD_NO_BITMAP | FT_LOAD_TARGET_LIGHT,
      (false, false) => FT_LOAD_NO_BITMAP | FT_LOAD_NO_HINTING,
    };

    // HarfBuzz's units per em, from the head table: FreeType leaves its own 0 for a font of
    // bitmap strikes only (CBDT emoji).
    let (hb, upem) = unsafe {
      let blob = (s.hb.hb_blob_create_from_file)(path.as_ptr());
      let face = (s.hb.hb_face_create)(blob, (index & 0xffff) as c_uint);
      let upem = (s.hb.hb_face_get_upem)(face) as c_int;
      let hb = (s.hb.hb_font_create)(face);
      (s.hb.hb_font_set_scale)(hb, upem, upem);
      if index >> 16 > 0 {
        (s.hb.hb_font_set_var_named_instance)(hb, (index >> 16) as c_uint - 1);
      }

      (hb, upem)
    };
    // `hb_ot_metrics_tag_t`: 'cpht' and 'xhgt', in font units at the scale set above.
    let metric = |tag: &[u8; 4]| {
      let mut v = 0;
      let ok = unsafe { (s.hb.hb_ot_metrics_get_position)(hb, u32::from_be_bytes(*tag), &mut v) };
      if ok != 0 { v as f32 } else { 0.0 }
    };
    let (cap, x) = (metric(b"cpht"), metric(b"xhgt"));

    Some(Self {
      path: path.to_owned(),
      index,
      ft: Mutex::new(FtFace { face, px: 0.0 }),
      hb,
      upem: (upem as f32).max(1.0),
      ascent: f.ascender as f32,
      descent: -(f.descender as f32),
      height: f.height as f32,
      cap,
      x,
      color,
      strike,
      hinting,
      load,
    })
  }
}

/// A font and its fallbacks, as fontconfig sorts them for one request.
struct Chain {
  /// The request, its size zeroed: sizes share a chain.
  font: Font,
  primary: Arc<FontFile>,
  primary_charset: P,
  /// Per fallback: its pattern (file, index, charset; the font set is kept alive) and the file
  /// once a character needed it.
  fallbacks: Vec<(P, Option<Arc<FontFile>>)>,
  hinting: bool,
}

struct Cache {
  files: Vec<Arc<FontFile>>,
  chains: Vec<Chain>,
}

// SAFETY: the patterns and charsets are read only under the cache's lock.
unsafe impl Send for Cache {}

fn cache() -> MutexGuard<'static, Cache> {
  static CACHE: Mutex<Cache> = Mutex::new(Cache { files: Vec::new(), chains: Vec::new() });
  CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

fn get_string(p: P, obj: &CStr) -> Option<&'static CStr> {
  let mut s = std::ptr::null();
  let r = unsafe { (sys().fc.FcPatternGetString)(p, obj.as_ptr(), 0, &mut s) };
  (r == FC_RESULT_MATCH).then(|| unsafe { CStr::from_ptr(s) })
}

fn get_int(p: P, obj: &CStr) -> Option<c_int> {
  let mut i = 0;
  let r = unsafe { (sys().fc.FcPatternGetInteger)(p, obj.as_ptr(), 0, &mut i) };
  (r == FC_RESULT_MATCH).then_some(i)
}

fn get_bool(p: P, obj: &CStr) -> Option<bool> {
  let mut b = 0;
  let r = unsafe { (sys().fc.FcPatternGetBool)(p, obj.as_ptr(), 0, &mut b) };
  (r == FC_RESULT_MATCH).then_some(b != 0)
}

fn charset(p: P) -> P {
  let mut cs = std::ptr::null_mut();
  unsafe { (sys().fc.FcPatternGetCharSet)(p, c"charset".as_ptr(), 0, &mut cs) };
  cs
}

fn has_char(cs: P, c: char) -> bool {
  !cs.is_null() && unsafe { (sys().fc.FcCharSetHasChar)(cs, c as u32) } != 0
}

impl Cache {
  /// The file at `path` and `index` with `hinting`, opened on first use; a colour font, which
  /// never hints, is one for both.
  fn file(&mut self, path: &CStr, index: c_int, hinting: bool) -> Option<Arc<FontFile>> {
    let same = |f: &&Arc<FontFile>| {
      *f.path == *path && f.index == index && (f.color || f.hinting == hinting)
    };
    if let Some(f) = self.files.iter().find(same) {
      return Some(f.clone());
    }

    let f = Arc::new(FontFile::open(path, index, hinting)?);
    self.files.push(f.clone());
    Some(f)
  }

  /// The chain for `font` (any size), matched on first use.
  fn chain(&mut self, font: Font) -> usize {
    let key = Font { size: 0.0, ..font };
    if let Some(i) = self.chains.iter().position(|c| c.font == key) {
      return i;
    }

    let s = sys();
    let fc = &s.fc;
    let family = if font.mono { c"monospace" } else { c"sans-serif" };
    let (matched, set) = unsafe {
      let pat = (fc.FcPatternCreate)();
      (fc.FcPatternAddString)(pat, c"family".as_ptr(), family.as_ptr());
      (fc.FcPatternAddInteger)(pat, c"weight".as_ptr(), FC_WEIGHTS[font.weight as usize]);
      (fc.FcConfigSubstitute)(s.config, pat, FC_MATCH_PATTERN);
      (fc.FcDefaultSubstitute)(pat);
      let mut res = 0;
      let matched = (fc.FcFontMatch)(s.config, pat, &mut res);
      let set = (fc.FcFontSort)(s.config, pat, 1, std::ptr::null_mut(), &mut res);
      (fc.FcPatternDestroy)(pat);
      (matched, set)
    };
    assert!(!matched.is_null(), "nimbo: fontconfig matched no font for {family:?}");

    let hinting = get_bool(matched, c"hinting").unwrap_or(true)
      && get_int(matched, c"hintstyle").is_none_or(|h| h > 0);
    let path = get_string(matched, c"file").expect("nimbo: a font without a file");
    let index = get_int(matched, c"index").unwrap_or(0);
    let primary = self.file(path, index, hinting).expect("nimbo: FreeType can't open the UI font");
    let fallbacks = match set.is_null() {
      true => Vec::new(),
      false => {
        let set = unsafe { &*set };
        let fonts = unsafe { std::slice::from_raw_parts(set.fonts, set.nfont as usize) };
        fonts.iter().map(|&p| (p, None)).collect()
      }
    };

    let primary_charset = charset(matched);
    self.chains.push(Chain { font: key, primary, primary_charset, fallbacks, hinting });
    self.chains.len() - 1
  }

  /// Splits `s` into runs of one font ([runs]): the chain's first font, else its fallbacks in
  /// order.
  fn itemize(&mut self, chain: usize, s: &str) -> Vec<(Range<usize>, Arc<FontFile>)> {
    let ch = &self.chains[chain];
    let has = |font: usize, c: char| match font {
      0 => has_char(ch.primary_charset, c),
      k => has_char(charset(ch.fallbacks[k - 1].0), c),
    };
    // A fallback's colour from fontconfig (`FC_COLOR`), for an emoji presentation selector.
    let color = |font: usize| match font {
      0 => ch.primary.color,
      k => get_bool(ch.fallbacks[k - 1].0, c"color").unwrap_or(false),
    };
    let runs = runs(s, ch.fallbacks.len() + 1, has, color);

    let mut out = Vec::with_capacity(runs.len());
    for (range, pick) in runs {
      let file = pick.checked_sub(1).and_then(|k| self.fallback(chain, k));
      out.push((range, file.unwrap_or_else(|| self.chains[chain].primary.clone())));
    }

    out
  }

  fn fallback(&mut self, chain: usize, k: usize) -> Option<Arc<FontFile>> {
    let (p, file) = &self.chains[chain].fallbacks[k];
    if let Some(f) = file {
      return Some(f.clone());
    }

    let (p, hinting) = (*p, self.chains[chain].hinting);
    let path = get_string(p, c"file")?;
    let index = get_int(p, c"index").unwrap_or(0);
    let f = self.file(path, index, hinting)?;
    self.chains[chain].fallbacks[k].1 = Some(f.clone());
    Some(f)
  }
}

/// Runs of one font for `s` from fonts `0..fonts` (`has(font, c)`: whether it has `c`;
/// `color(font)`: whether its glyphs are colour), by cluster (a character with what extends it
/// in the editor's clusters: marks, vowel signs, joiners and selectors; what a joiner links; a
/// pair of regional indicators): the first font with the whole cluster in the presentation a
/// selector asks for (U+FE0F emoji: a colour font; U+FE0E text: another), else the first with
/// the whole cluster, else the first with its first character, else font 0. A joined emoji or
/// a letter with its mark is shaped in one font.
fn runs(
  s: &str,
  fonts: usize,
  has: impl Fn(usize, char) -> bool,
  color: impl Fn(usize) -> bool,
) -> Vec<(Range<usize>, usize)> {
  let mut out: Vec<(Range<usize>, usize)> = Vec::new();
  for cluster in clusters(s) {
    let text = &s[cluster.clone()];
    let first = text.chars().next().unwrap_or(' ');
    let pick = match first.is_control() {
      true => 0,
      false => {
        let all = |f: &usize| text.chars().filter(|&c| !ignorable(c)).all(|c| has(*f, c));
        let any = |f: &usize| has(*f, first);
        let wants = match (text.contains('\u{fe0f}'), text.contains('\u{fe0e}')) {
          (true, _) => Some(true),
          (_, true) => Some(false),
          _ => None,
        };

        let shown = |f: &usize| all(f) && wants.is_none_or(|c| color(*f) == c);
        let found = (0..fonts).find(shown).or_else(|| (0..fonts).find(all));
        found.or_else(|| (0..fonts).find(any)).unwrap_or(0)
      }
    };

    match out.last_mut() {
      Some(r) if r.1 == pick => r.0.end = cluster.end,
      _ => out.push((cluster, pick)),
    }
  }

  out
}

/// The clusters of `s` for picking fonts, as byte ranges.
fn clusters(s: &str) -> Vec<Range<usize>> {
  let regional = |c: char| matches!(c as u32, 0x1f1e6..=0x1f1ff);
  let mut out: Vec<Range<usize>> = Vec::new();
  let (mut prev, mut indicators) = (None, 0);
  for (i, c) in s.char_indices() {
    let end = i + c.len_utf8();
    let linked = prev == Some('\u{200d}') || (regional(c) && indicators % 2 == 1);
    match out.last_mut() {
      Some(r) if is_extend(c) || linked => r.end = end,
      _ => {
        out.push(i..end);
        indicators = 0;
      }
    }

    indicators += regional(c) as usize;
    prev = Some(c);
  }

  out
}

/// Characters a font needn't have for a cluster: joiners, variation selectors and tags draw
/// nothing of their own.
fn ignorable(c: char) -> bool {
  matches!(c as u32, 0x200c..=0x200d | 0xfe00..=0xfe0f | 0xe0020..=0xe007f | 0xe0100..=0xe01ef)
}

// MARK: Faces

/// A font file at a point size; `chain` is the request it came from, for fallback.
#[derive(Clone)]
pub struct Face {
  file: Arc<FontFile>,
  size: f32,
  chain: usize,
}

/// The same file at the same size from another request is another face: its fallbacks may
/// differ (Medium falling back to Regular has a chain of its own).
impl PartialEq for Face {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.file, &other.file) && self.size == other.size && self.chain == other.chain
  }
}

/// The system UI font (fontconfig's `sans-serif`), or its monospaced one, at the size and
/// weight.
pub fn face(font: Font) -> Face {
  let mut c = cache();
  let chain = c.chain(font);
  Face { file: c.chains[chain].primary.clone(), size: font.size, chain }
}

/// From the font's hhea (or typo) metrics, unhinted.
pub fn metrics(face: &Face) -> Metrics {
  let (f, k) = (&face.file, face.size / face.file.upem);
  let (ascent, descent) = (f.ascent * k, f.descent * k);
  let leading = (f.height * k - ascent - descent).max(0.0);
  // A font without OS/2 heights: the usual proportions of the ascent.
  let cap_height = if f.cap > 0.0 { f.cap * k } else { ascent * 0.75 };
  let x_height = if f.x > 0.0 { f.x * k } else { ascent * 0.55 };
  Metrics { ascent, descent, leading, cap_height, x_height }
}

/// Its glyphs are colour (CBDT or COLR emoji).
pub fn is_color(face: &Face) -> bool {
  face.file.color
}

/// Shapes `s` with HarfBuzz, one run per font of the fallback chain; `index` gives the text
/// system's index of each run's face. Clusters are byte offsets of `s`, as HarfBuzz gives them
/// for UTF-8 (each run shaped with the whole line as context). Not yet: bidi and script runs, so
/// a font run mixing scripts (Latin and Arabic) shapes in the script and direction HarfBuzz
/// guesses from its start; Arabic alone shapes right.
pub fn shape(face: &Face, s: &str, mut index: impl FnMut(Face) -> u16) -> Line {
  let runs = cache().itemize(face.chain, s);
  let hb = &sys().hb;
  let (mut glyphs, mut clusters, mut pen) = (Vec::new(), Vec::new(), 0.0);
  for (range, file) in runs {
    let (k, font) = (face.size / file.upem, file.hb);
    let id = index(Face { file, size: face.size, chain: face.chain });
    // SAFETY: the buffer is ours until destroyed; HarfBuzz's arrays hold `n` entries while it
    // lives.
    unsafe {
      let buf = (hb.hb_buffer_create)();
      let (off, len) = (range.start as c_uint, range.len() as c_int);
      (hb.hb_buffer_add_utf8)(buf, s.as_ptr().cast(), s.len() as c_int, off, len);
      (hb.hb_buffer_guess_segment_properties)(buf);
      (hb.hb_shape)(font, buf, std::ptr::null(), 0);
      let mut n = 0;
      let infos = (hb.hb_buffer_get_glyph_infos)(buf, &mut n);
      let infos = std::slice::from_raw_parts(infos, n as usize);
      let pos = (hb.hb_buffer_get_glyph_positions)(buf, &mut n);
      let pos = std::slice::from_raw_parts(pos, n as usize);
      for (g, p) in infos.iter().zip(pos) {
        let (x, y) = (pen + p.x_offset as f32 * k, p.y_offset as f32 * k);
        glyphs.push(Glyph { face: id, id: g.codepoint as u16, x, y });
        clusters.push(g.cluster);
        pen += p.x_advance as f32 * k;
      }

      (hb.hb_buffer_destroy)(buf);
    }
  }

  Line { glyphs, clusters, width: pen, ink: Ink::default(), epoch: 0 }
}

// MARK: Glyph raster

/// A glyph as FreeType drew it: `left` and `top` from the pen at the baseline (y up), `k` the
/// factor its strike is scaled by to the size asked (1 for outlines); BGRA or coverage rows.
struct Bitmap {
  left: i32,
  top: i32,
  w: usize,
  h: usize,
  bgra: bool,
  data: Vec<u8>,
  k: f32,
}

impl Bitmap {
  /// The box at the size asked, one pixel of padding each side.
  fn glyph_box(&self) -> GlyphBox {
    let k = self.k;
    let left = (self.left as f32 * k).floor() as i32 - 1;
    let top = (self.top as f32 * k).ceil() as i32 + 1;
    let right = ((self.left as f32 + self.w as f32) * k).ceil() as i32 + 1;
    let bottom = ((self.top as f32 - self.h as f32) * k).floor() as i32 - 1;
    GlyphBox { left, bottom, w: (right - left) as usize, h: (top - bottom) as usize }
  }

  /// Pixel (x, y) of the bitmap scaled to `w` × `h` (an area average for a strike), as
  /// premultiplied BGRA; coverage as black.
  fn sample(&self, x: usize, y: usize, w: usize, h: usize) -> [u8; 4] {
    let get = |sx: usize, sy: usize| -> [u32; 4] {
      match self.bgra {
        true => {
          let i = (sy * self.w + sx) * 4;
          let p = &self.data[i..i + 4];
          [p[0] as u32, p[1] as u32, p[2] as u32, p[3] as u32]
        }
        false => [0, 0, 0, self.data[sy * self.w + sx] as u32],
      }
    };
    if (w, h) == (self.w, self.h) {
      return get(x, y).map(|v| v as u8);
    }

    let (fx, fy) = (self.w as f32 / w as f32, self.h as f32 / h as f32);
    let span = |i: usize, f: f32, n: usize| {
      let a = ((i as f32 * f) as usize).min(n - 1);
      a..((((i + 1) as f32 * f).ceil() as usize).clamp(a + 1, n))
    };
    let (mut sum, mut n) = ([0u32; 4], 0);
    for sy in span(y, fy, self.h) {
      for sx in span(x, fx, self.w) {
        let p = get(sx, sy);
        (0..4).for_each(|c| sum[c] += p[c]);
        n += 1;
      }
    }

    sum.map(|v| (v / n) as u8)
  }
}

/// Draws glyph `id` at the face's size × `scale`, `subpixel` quarters of a pixel right.
fn render(face: &Face, id: u16, scale: f32, subpixel: u8) -> Option<Bitmap> {
  #[cfg(test)]
  tests::RENDERS.with(|n| n.set(n.get() + 1));
  let s = sys();
  let file = &face.file;
  let mut ft = file.ft.lock().unwrap_or_else(|e| e.into_inner());
  let px = face.size * scale;
  let k = match file.strike {
    Some((strike, ppem)) => {
      if ft.px != ppem {
        unsafe { (s.ft.FT_Select_Size)(ft.face, strike) };
        ft.px = ppem;
      }

      px / ppem
    }
    None => {
      if ft.px != px {
        unsafe { (s.ft.FT_Set_Char_Size)(ft.face, 0, (px * 64.0).round() as c_long, 72, 72) };
        ft.px = px;
      }

      1.0
    }
  };
  // Outlines move by the subpixel offset (26.6 units); a strike is scaled, not shifted.
  let dx = if file.strike.is_some() { 0 } else { subpixel as c_long * 16 };
  let mut delta = [dx, 0];
  unsafe { (s.ft.FT_Set_Transform)(ft.face, std::ptr::null_mut(), &mut delta) };
  if unsafe { (s.ft.FT_Load_Glyph)(ft.face, id as c_uint, file.load) } != 0 {
    return None;
  }

  let slot = unsafe { (*ft.face).glyph };
  if unsafe { (s.ft.FT_Render_Glyph)(slot, FT_RENDER_MODE_NORMAL) } != 0 {
    return None;
  }

  let g = unsafe { &*slot };
  let b = &g.bitmap;
  let (w, h) = (b.width as usize, b.rows as usize);
  if w == 0 || h == 0 || b.buffer.is_null() {
    return None;
  }

  let bgra = match b.pixel_mode {
    FT_PIXEL_MODE_BGRA => true,
    FT_PIXEL_MODE_GRAY => false,
    _ => return None,
  };

  let row = if bgra { w * 4 } else { w };
  let mut data = Vec::with_capacity(row * h);
  for y in 0..h {
    // SAFETY: FreeType's bitmap holds `rows` rows `pitch` bytes apart, each `row` bytes wide.
    let start = unsafe { b.buffer.offset(y as isize * b.pitch as isize) };
    data.extend_from_slice(unsafe { std::slice::from_raw_parts(start, row) });
  }

  Some(Bitmap { left: g.bitmap_left, top: g.bitmap_top, w, h, bgra, data, k })
}

/// A rendered glyph's file, size, id, scale and subpixel offset.
type Rendered = (usize, u32, u16, u32, u8);

thread_local! {
  /// The glyph [glyph_box] rendered last, kept for [rasterize]: the atlas asks for the box,
  /// then for the pixels of the same glyph, which FreeType renders once.
  static LAST: std::cell::RefCell<Option<(Rendered, Bitmap)>> = const { std::cell::RefCell::new(None) };
}

fn rendered(face: &Face, id: u16, scale: f32, subpixel: u8) -> Rendered {
  (Arc::as_ptr(&face.file) as usize, face.size.to_bits(), id, scale.to_bits(), subpixel)
}

/// The pixel box of glyph `id` at `scale`, drawn `subpixel` quarters of a pixel to the right:
/// FreeType's bitmap with one pixel of padding on each side. `None` for no ink (a space).
pub fn glyph_box(face: &Face, id: u16, scale: f32, subpixel: u8) -> Option<GlyphBox> {
  let b = render(face, id, scale, subpixel)?;
  let gb = b.glyph_box();
  LAST.with(|l| *l.borrow_mut() = Some((rendered(face, id, scale, subpixel), b)));
  Some(gb)
}

/// Draws glyph `id` into the pixels of `b` (from [glyph_box] with the same `subpixel`), rows
/// top-down: coverage, or premultiplied RGBA for a colour face (`color`). Font smoothing has no
/// FreeType counterpart: `raster.smooth` is ignored.
pub fn rasterize(
  face: &Face,
  id: u16,
  subpixel: u8,
  b: &GlyphBox,
  raster: Raster,
  color: bool,
) -> Option<Vec<u8>> {
  let key = rendered(face, id, raster.scale, subpixel);
  let last = LAST.with(|l| l.borrow_mut().take()).filter(|(k, _)| *k == key);
  let g = match last {
    Some((_, g)) => g,
    None => render(face, id, raster.scale, subpixel)?,
  };

  let gb = g.glyph_box();
  // Where the glyph's pixels (inside its padding) start in `b`, from the top left.
  let ox = (gb.left + 1 - b.left).max(0) as usize;
  let oy = ((b.bottom + b.h as i32) - (gb.bottom + gb.h as i32) + 1).max(0) as usize;
  let (w, h) = (gb.w - 2, gb.h - 2);
  let n = if color { 4 } else { 1 };
  let mut out = vec![0u8; b.w * b.h * n];
  for y in 0..h.min(b.h.saturating_sub(oy)) {
    for x in 0..w.min(b.w.saturating_sub(ox)) {
      let p = g.sample(x, y, w, h);
      let at = ((oy + y) * b.w + ox + x) * n;
      match color {
        true => out[at..at + 4].copy_from_slice(&[p[2], p[1], p[0], p[3]]),
        false => out[at] = p[3],
      }
    }
  }

  Some(out)
}

#[cfg(test)]
mod tests {
  use std::cell::Cell;

  use super::*;
  use crate::text::Font;

  thread_local! {
    /// FreeType renders on this thread.
    pub static RENDERS: Cell<usize> = const { Cell::new(0) };
  }

  #[test]
  fn an_atlas_miss_renders_once() {
    let f = face(Font::ui(13.0));
    let line = shape(&f, "g", |_| 0);
    let (id, raster) = (line.glyphs[0].id, Raster { scale: 2.0, smooth: false, dark: true });
    let alone = {
      let b = glyph_box(&f, id, 2.0, 1).unwrap();
      LAST.with(|l| l.borrow_mut().take());
      rasterize(&f, id, 1, &b, raster, false).unwrap()
    };
    RENDERS.with(|n| n.set(0));
    let b = glyph_box(&f, id, 2.0, 1).unwrap();
    let px = rasterize(&f, id, 1, &b, raster, false).unwrap();
    assert_eq!(RENDERS.with(Cell::get), 1);
    assert!(px == alone, "the same pixels from the kept render");
  }

  #[test]
  fn files_keep_their_hinting() {
    let primary = face(Font::ui(13.0)).file;
    assert!(!primary.color);
    let other = cache().file(&primary.path, primary.index, !primary.hinting).unwrap();
    assert!(!Arc::ptr_eq(&primary, &other), "another hinting opens the file again");
    assert_eq!(other.hinting, !primary.hinting);
    assert_ne!(other.load, primary.load);

    let again = cache().file(&primary.path, primary.index, primary.hinting).unwrap();
    assert!(Arc::ptr_eq(&primary, &again));
  }

  /// Two fonts: a text one (Latin letters, ❤, क) and another (people, ❤, the acute accent, कि).
  fn has(font: usize, c: char) -> bool {
    match font {
      0 => c.is_ascii_alphabetic() || matches!(c, '❤' | 'क'),
      _ => matches!(c, '👩' | '👨' | '❤' | '\u{301}' | 'e' | '🇺' | '🇦' | 'क' | '\u{93f}'),
    }
  }

  /// The other font is the colour one.
  fn picks(s: &str) -> Vec<(&str, usize)> {
    runs(s, 2, has, |f| f == 1).into_iter().map(|(r, f)| (&s[r], f)).collect()
  }

  #[test]
  fn clusters_keep_one_font() {
    // A joined emoji whose heart the text font has too: one run in the emoji font.
    assert_eq!(
      picks("a👩\u{200d}❤\u{fe0f}\u{200d}👨b"),
      [("a", 0), ("👩\u{200d}❤\u{fe0f}\u{200d}👨", 1), ("b", 0)]
    );
    // A letter with a mark only the other font has: both there.
    assert_eq!(picks("ae\u{301}"), [("a", 0), ("e\u{301}", 1)]);
    // A consonant with a vowel sign only the other font has: both there (कि).
    assert_eq!(picks("a\u{915}\u{93f}"), [("a", 0), ("\u{915}\u{93f}", 1)]);
    // Neither has the whole cluster: the font of its first character.
    assert_eq!(picks("a\u{302}b"), [("a\u{302}b", 0)]);
    // A heart alone stays in the text font; flags pair up.
    assert_eq!(picks("❤🇺🇦"), [("❤", 0), ("🇺🇦", 1)]);
    assert_eq!(clusters("🇺🇦🇺🇦").len(), 2);
  }

  /// With a colour font that has the heart in the fallback chain (Noto Color Emoji), a heart
  /// asked for as emoji is drawn in it, not as the UI font's outline (DejaVu Sans has one).
  #[test]
  fn an_emoji_heart_is_drawn_in_colour() {
    let f = face(Font::ui(13.0));
    let colour_heart = {
      let c = cache();
      let ch = &c.chains[f.chain];
      ch.fallbacks
        .iter()
        .any(|&(p, _)| get_bool(p, c"color") == Some(true) && has_char(charset(p), '\u{2764}'))
    };
    if !colour_heart {
      println!("no colour font with the heart: skipped");
      return;
    }

    let mut faces = Vec::new();
    let line = shape(&f, "\u{2764}\u{fe0f}", |f| {
      faces.push(f);
      faces.len() as u16 - 1
    });
    assert!(is_color(&faces[line.glyphs[0].face as usize]));
  }

  #[test]
  fn presentation_selectors_pick_the_font() {
    // An emoji heart in the colour font; a text heart, or one asked for as text, in the other.
    assert_eq!(picks("a❤\u{fe0f}b"), [("a", 0), ("❤\u{fe0f}", 1), ("b", 0)]);
    assert_eq!(picks("a❤b"), [("a❤b", 0)]);
    assert_eq!(picks("❤\u{fe0e}"), [("❤\u{fe0e}", 0)]);
    // No colour font has it: the text font draws it all the same.
    assert_eq!(picks("a\u{fe0f}"), [("a\u{fe0f}", 0)]);
  }
}
