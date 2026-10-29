// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! PNG files written and read, the same on every OS, std only. Written for harness shots: 8-bit
//! RGBA tagged sRGB, each row filtered with the PNG filter that sums smallest (the usual
//! heuristic), compressed by a small deflate (LZ77 over one hash slot per 4 bytes, fixed Huffman
//! codes); checked on macOS against ImageIO's shots (2026-10-18): the same pixels, files ~4%
//! larger, written in ~2x the time. Read for the images an app ships ([Png]): 8-bit RGB, RGBA or a
//! palette with its tRNS, not interlaced, inflated with fixed and dynamic Huffman codes.

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use crate::cpu::Pixels;

/// Writes `px` (BGRA premultiplied) as a PNG at `path`.
pub fn write(path: &Path, px: &Pixels) -> Result<(), String> {
  std::fs::write(path, encode(px)).map_err(|e| format!("can't write {}: {e}", path.display()))
}

/// The PNG file of `px`.
pub fn encode(px: &Pixels) -> Vec<u8> {
  encode_rgba(px.w, px.h, &rgba_rows(px))
}

/// The PNG file of `w` × `h` straight-alpha RGBA pixels, rows top-down.
pub fn encode_rgba(w: usize, h: usize, rows: &[u8]) -> Vec<u8> {
  let mut raw = Vec::with_capacity((w * 4 + 1) * h);
  let mut prev = vec![0u8; w * 4];
  let mut best = Vec::with_capacity(w * 4);
  let mut cand = Vec::with_capacity(w * 4);
  for row in rows.chunks_exact(w * 4) {
    let (mut kind, mut least) = (0, u64::MAX);
    for k in 0..5 {
      cand.clear();
      for i in 0..row.len() {
        cand.push(row[i].wrapping_sub(predict(k, row, &prev, i, 4)));
      }

      let sum = cand.iter().map(|&b| (b as i8).unsigned_abs() as u64).sum();
      if sum < least {
        (least, kind) = (sum, k);
        std::mem::swap(&mut best, &mut cand);
      }
    }

    raw.push(kind);
    raw.extend_from_slice(&best);
    prev.copy_from_slice(row);
  }

  let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
  let mut ihdr = [(w as u32).to_be_bytes(), (h as u32).to_be_bytes()].concat();
  // 8 bits per channel, colour type 6 (RGBA), deflate, adaptive filters, no interlace.
  ihdr.extend([8, 6, 0, 0, 0]);
  chunk(&mut out, b"IHDR", &ihdr);
  // sRGB, perceptual rendering intent.
  chunk(&mut out, b"sRGB", &[0]);
  chunk(&mut out, b"IDAT", &zlib(&raw));
  chunk(&mut out, b"IEND", &[]);
  out
}

/// Straight-alpha RGBA rows from premultiplied BGRA.
fn rgba_rows(px: &Pixels) -> Vec<u8> {
  let mut out = Vec::with_capacity(px.w * px.h * 4);
  for &[b, g, r, a] in px.data.as_chunks::<4>().0 {
    let un = |c: u8| match a {
      0 => 0,
      255 => c,
      a => ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8,
    };
    out.extend([un(r), un(g), un(b), a]);
  }

  out
}

/// PNG filter `kind`'s guess at byte `i` of `row` (0 none, 1 sub, 2 up, 3 average, 4 Paeth), from
/// the bytes left of it, above it in `prev` and up-left, `bpp` bytes a pixel: none for the first.
#[inline(always)]
fn predict(kind: u8, row: &[u8], prev: &[u8], i: usize, bpp: usize) -> u8 {
  let (a, b, c) = if i >= bpp { (row[i - bpp], prev[i], prev[i - bpp]) } else { (0, prev[i], 0) };
  match kind {
    0 => 0,
    1 => a,
    2 => b,
    3 => ((a as u16 + b as u16) / 2) as u8,
    _ => paeth(a, b, c),
  }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
  let p = a as i16 + b as i16 - c as i16;
  let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
  if pa <= pb && pa <= pc {
    a
  } else if pb <= pc {
    b
  } else {
    c
  }
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
  let body = [&kind[..], data].concat();
  out.extend([&(data.len() as u32).to_be_bytes()[..], &body, &crc32(&body).to_be_bytes()].concat());
}

// MARK: Deflate

/// The longest match deflate codes, and the window it may reach back.
const MAX_LEN: usize = 258;
const WINDOW: usize = 32 * 1024;
const HASH_BITS: u32 = 15;

/// A zlib stream (RFC 1950) of one fixed-Huffman deflate block (RFC 1951).
fn zlib(data: &[u8]) -> Vec<u8> {
  let mut w = Bits { out: vec![0x78, 0x01], acc: 0, n: 0 };
  // BFINAL, BTYPE = 01 (fixed codes).
  w.put(0b011, 3);
  let mut head = vec![u32::MAX; 1 << HASH_BITS];
  let hash = |i: usize| {
    let k = u32::from_le_bytes(data[i..i + 4].try_into().unwrap());
    (k.wrapping_mul(0x9e37_79b1) >> (32 - HASH_BITS)) as usize
  };
  let mut i = 0;
  while i < data.len() {
    let (mut len, mut dist) = (0, 0);
    if i + 4 <= data.len() {
      let h = hash(i);
      let cand = head[h] as usize;
      head[h] = i as u32;
      if cand != u32::MAX as usize && i - cand <= WINDOW {
        let most = (data.len() - i).min(MAX_LEN);
        len = (0..most).take_while(|&k| data[cand + k] == data[i + k]).count();
        dist = i - cand;
      }
    }

    if len >= 4 {
      w.length(len);
      w.distance(dist);
      // Index the positions the match covers, so later matches find them.
      for j in i + 1..(i + len).min(data.len().saturating_sub(3)) {
        head[hash(j)] = j as u32;
      }

      i += len;
    } else {
      w.literal(data[i] as u16);
      i += 1;
    }
  }

  w.literal(256);
  let mut out = w.finish();
  out.extend(adler32(data).to_be_bytes());
  out
}

/// Deflate's bit writer: values least significant bit first, Huffman codes most first.
struct Bits {
  out: Vec<u8>,
  acc: u64,
  n: u32,
}

impl Bits {
  fn put(&mut self, v: u32, bits: u32) {
    self.acc |= (v as u64) << self.n;
    self.n += bits;
    while self.n >= 8 {
      self.out.push(self.acc as u8);
      self.acc >>= 8;
      self.n -= 8;
    }
  }

  /// A Huffman code of `bits` bits, reversed into deflate's order.
  fn code(&mut self, code: u32, bits: u32) {
    self.put(code.reverse_bits() >> (32 - bits), bits);
  }

  /// A literal/length symbol in the fixed code (RFC 1951, 3.2.6).
  fn literal(&mut self, sym: u16) {
    let sym = sym as u32;
    match sym {
      0..=143 => self.code(0x30 + sym, 8),
      144..=255 => self.code(0x190 + sym - 144, 9),
      256..=279 => self.code(sym - 256, 7),
      _ => self.code(0xc0 + sym - 280, 8),
    }
  }

  fn length(&mut self, len: usize) {
    let (k, base, extra) = bucket(&LENGTHS, len);
    self.literal(257 + k as u16);
    self.put((len - base) as u32, extra);
  }

  fn distance(&mut self, dist: usize) {
    let (k, base, extra) = bucket(&DISTANCES, dist);
    self.code(k as u32, 5);
    self.put((dist - base) as u32, extra);
  }

  fn finish(mut self) -> Vec<u8> {
    if self.n > 0 {
      self.out.push(self.acc as u8);
    }

    self.out
  }
}

/// Base values of the length codes 257.. and distance codes 0.. with their extra bits.
#[rustfmt::skip]
const LENGTHS: [(usize, u32); 29] = [
  (3, 0), (4, 0), (5, 0), (6, 0), (7, 0), (8, 0), (9, 0), (10, 0),
  (11, 1), (13, 1), (15, 1), (17, 1), (19, 2), (23, 2), (27, 2), (31, 2),
  (35, 3), (43, 3), (51, 3), (59, 3), (67, 4), (83, 4), (99, 4), (115, 4),
  (131, 5), (163, 5), (195, 5), (227, 5), (258, 0),
];
#[rustfmt::skip]
const DISTANCES: [(usize, u32); 30] = [
  (1, 0), (2, 0), (3, 0), (4, 0), (5, 1), (7, 1), (9, 2), (13, 2),
  (17, 3), (25, 3), (33, 4), (49, 4), (65, 5), (97, 5), (129, 6), (193, 6),
  (257, 7), (385, 7), (513, 8), (769, 8), (1025, 9), (1537, 9), (2049, 10), (3073, 10),
  (4097, 11), (6145, 11), (8193, 12), (12289, 12), (16385, 13), (24577, 13),
];

/// The code of `v` in a table of (base, extra bits): its index, base and extra bits.
fn bucket(table: &[(usize, u32)], v: usize) -> (usize, usize, u32) {
  let k = table.iter().rposition(|&(base, _)| base <= v).unwrap();
  (k, table[k].0, table[k].1)
}

// MARK: Reading

/// The size and straight RGBA pixels, rows top-down, of the PNG `file`: 8-bit RGB, RGBA or a
/// palette with its tRNS, not interlaced. Panics on another kind of PNG or a broken one: the app's
/// own file ([Png]), which its first run shows.
pub fn decode(file: &[u8]) -> (usize, usize, Vec<u8>) {
  let mut at = file.strip_prefix(b"\x89PNG\r\n\x1a\n").expect("nimbo: not a PNG file");
  let (mut head, mut palette, mut trns, mut idat) = (&[][..], &[][..], &[][..], Vec::new());
  loop {
    let n = u32::from_be_bytes(at[..4].try_into().unwrap()) as usize;
    let data = &at[8..n + 8];
    match &at[4..8] {
      b"IHDR" => head = data,
      b"PLTE" => palette = data,
      b"tRNS" => trns = data,
      b"IDAT" => idat.extend_from_slice(data),
      b"IEND" => break,
      _ => {}
    }

    at = &at[n + 12..];
  }

  // 8 bits a channel, RGB, palette or RGBA, deflate, the five filters, no interlace.
  let &[w0, w1, w2, w3, h0, h1, h2, h3, 8, color @ (2 | 3 | 6), 0, 0, 0] = head else {
    panic!("nimbo: a PNG other than 8-bit RGB, RGBA or a palette, not interlaced");
  };
  let w = u32::from_be_bytes([w0, w1, w2, w3]) as usize;
  let h = u32::from_be_bytes([h0, h1, h2, h3]) as usize;
  let bpp = [3, 1, 0, 0, 4][color as usize - 2];
  let stride = w * bpp;
  let raw = inflate(&idat);
  assert_eq!(raw.len(), (stride + 1) * h, "nimbo: a broken PNG");
  // A row of zeros above the first, dropped after.
  let mut rows = vec![0u8; stride * (h + 1)];
  for (y, line) in raw.chunks_exact(stride + 1).enumerate() {
    let (above, row) = rows.split_at_mut((y + 1) * stride);
    let (kind, prev, row) = (line[0], &above[y * stride..], &mut row[..stride]);
    assert!(kind <= 4, "nimbo: a broken PNG");
    row.copy_from_slice(&line[1..]);
    // Up alone, the commonest, as one pass the compiler vectorizes.
    match kind {
      0 => {}
      2 => row.iter_mut().zip(prev).for_each(|(r, &b)| *r = r.wrapping_add(b)),
      _ => (0..stride).for_each(|i| row[i] = row[i].wrapping_add(predict(kind, row, prev, i, bpp))),
    }
  }

  rows.drain(..stride);
  let out: Vec<[u8; 4]> = match color {
    6 => return (w, h, rows),
    2 => rows.as_chunks().0.iter().map(|&[r, g, b]| [r, g, b, 255]).collect(),
    _ => {
      // tRNS: an alpha per palette entry.
      let lut: Vec<[u8; 4]> = (palette.as_chunks().0.iter().enumerate())
        .map(|(i, &[r, g, b])| [r, g, b, trns.get(i).copied().unwrap_or(255)])
        .collect();
      rows.iter().map(|&i| lut[i as usize]).collect()
    }
  };

  (w, h, out.into_flattened())
}

/// The data of the zlib stream `z` (RFC 1950) of deflate blocks (RFC 1951).
fn inflate(z: &[u8]) -> Vec<u8> {
  let mut r = Reader { data: &z[2..], at: 0, acc: 0, n: 0 };
  let mut out = Vec::new();
  let mut last = false;
  while !last {
    last = r.take(1) == 1;
    let (lit, dist) = match r.take(2) {
      0 => {
        r.take(r.n % 8);
        let len = r.take(16);
        r.take(16);
        out.extend((0..len).map(|_| r.take(8) as u8));
        continue;
      }
      1 => {
        let mut lens = [8u8; 288 + 32];
        lens[144..256].fill(9);
        lens[256..280].fill(7);
        lens[288..].fill(5);
        (Huffman::new(&lens[..288]), Huffman::new(&lens[288..]))
      }
      2 => dynamic(&mut r),
      _ => panic!("nimbo: a broken PNG"),
    };

    // Literals, and lengths with their distances, up to the end code.
    loop {
      match lit.decode(&mut r) {
        sym @ 0..256 => out.push(sym as u8),
        256 => break,
        sym => {
          let (len, extra) = LENGTHS[sym - 257];
          let len = len + r.take(extra) as usize;
          let (d, extra) = DISTANCES[dist.decode(&mut r)];
          let from = out.len() - (d + r.take(extra) as usize);
          // A copy that overlaps repeats what it wrote: in rounds, each twice the last.
          let mut left = len;
          while left > 0 {
            let n = left.min(out.len() - from);
            out.extend_from_within(from..from + n);
            left -= n;
          }
        }
      }
    }
  }

  out
}

/// A dynamic block's codes (RFC 1951, 3.2.7): the literal/length one and the distance one.
fn dynamic(r: &mut Reader) -> (Huffman, Huffman) {
  const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
  let (lit, dist, n) = (r.take(5) as usize + 257, r.take(5) as usize + 1, r.take(4) as usize + 4);
  let mut cl = [0u8; 19];
  ORDER[..n].iter().for_each(|&i| cl[i] = r.take(3) as u8);
  let cl = Huffman::new(&cl);
  let mut lens = vec![0u8; lit + dist];
  let mut i = 0;
  while i < lens.len() {
    let (len, times) = match cl.decode(r) {
      16 => (lens[i - 1], 3 + r.take(2)),
      17 => (0, 3 + r.take(3)),
      18 => (0, 11 + r.take(7)),
      sym => (sym as u8, 1),
    };

    let to = i + times as usize;
    lens[i..to].fill(len);
    i = to;
  }

  (Huffman::new(&lens[..lit]), Huffman::new(&lens[lit..]))
}

/// A Huffman code as a table over the stream's next `bits` bits: each entry its symbol and
/// length (`sym << 4 | len`), 0 where no code is.
struct Huffman {
  table: Vec<u16>,
  bits: u32,
}

impl Huffman {
  /// The canonical code of the code lengths `lens` (RFC 1951, 3.2.2).
  fn new(lens: &[u8]) -> Self {
    let bits = lens.iter().copied().max().unwrap_or(0) as u32;
    let mut count = [0u32; 16];
    lens.iter().for_each(|&l| count[l as usize] += 1);
    count[0] = 0;
    let mut next = [0u32; 16];
    for l in 1..16 {
      next[l] = (next[l - 1] + count[l - 1]) << 1;
    }

    let mut table = vec![0u16; 1 << bits];
    for (sym, &l) in lens.iter().enumerate().filter(|s| *s.1 > 0) {
      let code = next[l as usize];
      next[l as usize] += 1;
      // The stream holds codes most significant bit first: the table is by the reversed code.
      let rev = (code.reverse_bits() >> (32 - l as u32)) as usize;
      table[rev..].iter_mut().step_by(1 << l).for_each(|e| *e = (sym << 4) as u16 | l as u16);
    }

    Self { table, bits }
  }

  fn decode(&self, r: &mut Reader) -> usize {
    let e = self.table[r.peek(self.bits) as usize];
    assert!(e != 0, "nimbo: a broken PNG");
    r.take(u32::from(e & 15));
    usize::from(e >> 4)
  }
}

/// Deflate's bit reader, least significant bit first: it panics on taking bits past the end.
struct Reader<'a> {
  data: &'a [u8],
  at: usize,
  acc: u64,
  n: u32,
}

impl Reader<'_> {
  /// The next `k` bits (up to 16), not taken.
  fn peek(&mut self, k: u32) -> u32 {
    while self.n < k {
      self.acc |= (self.data.get(self.at).copied().unwrap_or(0) as u64) << self.n;
      (self.at, self.n) = (self.at + 1, self.n + 8);
    }

    (self.acc & ((1 << k) - 1)) as u32
  }

  fn take(&mut self, k: u32) -> u32 {
    let v = self.peek(k);
    (self.acc, self.n) = (self.acc >> k, self.n - k);
    assert!(self.at * 8 - self.n as usize <= self.data.len() * 8, "nimbo: a PNG cut short");
    v
  }
}

// MARK: Images

/// An image the app ships as a PNG file, `Png::new(include_bytes!("logo.png"))`: decoded the
/// first time its pixels are asked for ([Png::rgba]), then kept for the app's life. For the
/// windows' icon ([crate::Config::png_icon]), the launcher's ([crate::desktop::Launcher::icon]),
/// a view's image ([crate::Ui::png_image]); [Png::tile] and [Png::side] shape it first.
#[derive(Clone, Copy, Hash)]
pub struct Png {
  file: &'static [u8],
  side: u32,
  tile: bool,
}

impl Png {
  pub const fn new(file: &'static [u8]) -> Self {
    Self { file, side: 0, tile: false }
  }

  /// The app's icon (a macOS one, 1024 px with its margin) as windows, taskbars and launchers
  /// off macOS show it: its tile ([Png::tile]) at 256 px ([Png::side]), which serves every size
  /// they show.
  pub const fn icon(file: &'static [u8]) -> Self {
    Self::new(file).tile().side(256)
  }

  /// Cut to the square around its visible pixels, centred: a macOS icon's tile without the
  /// transparent margin Apple keeps around it (about a tenth on each side), which would show it
  /// small where the image shows as it is (Windows' taskbar, Linux docks).
  pub const fn tile(self) -> Self {
    Self { tile: true, ..self }
  }

  /// Shrunk so that its longer side is `side` pixels, each the average of the pixels it covers
  /// weighted by their alpha: 128 for an image drawn 64 points wide on a Retina screen. An image
  /// no larger stays as it is.
  pub const fn side(self, side: u32) -> Self {
    Self { side, ..self }
  }

  /// Its width, height and straight RGBA pixels, rows top-down: decoded on the first call (a
  /// 1024 px icon in ~4 ms in a release build, ~7 cut to its tile at 256 px), the same pixels
  /// after. Panics when the file is not a PNG this reads (the module's doc): the app's own file,
  /// which its first run shows.
  pub fn rgba(&self) -> (u32, u32, &'static [u8]) {
    type Decoded = ((usize, usize, u32, bool), (u32, u32, &'static [u8]));
    static DECODED: Mutex<Vec<Decoded>> = Mutex::new(Vec::new());
    let key = (self.file.as_ptr() as usize, self.file.len(), self.side, self.tile);
    let mut all = DECODED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(&(_, image)) = all.iter().find(|d| d.0 == key) {
      return image;
    }

    let (mut w, mut h, mut px) = decode(self.file);
    if self.tile {
      let (side, tile) = cropped(w, h, &px);
      (w, h, px) = (side, side, tile);
    }

    let (long, side) = (w.max(h), self.side as usize);
    if side > 0 && side < long {
      let (nw, nh) = ((w * side / long).max(1), (h * side / long).max(1));
      (w, h, px) = (nw, nh, resized(w, h, &px, nw, nh));
    }

    let image = (w as u32, h as u32, &*px.leak());
    all.push((key, image));
    image
  }

  /// [Png::rgba] of a square image, as a window's icon is given ([crate::Config::icon]): its
  /// side and pixels. Panics when it is not square.
  pub(crate) fn square(&self) -> (u32, &'static [u8]) {
    let (w, h, px) = self.rgba();
    assert_eq!(w, h, "nimbo: a {w} × {h} icon is not square");
    (w, px)
  }
}

// Ported from wavo-app's build.rs (https://github.com/vladkens/wavo-app, MIT License): `resized`
// to a width and a height, both by rows, the same pixels.

/// `rgba` cut to the square around its visible pixels, centred, a side and its pixels: macOS
/// icons keep a transparent margin around their tile (about a tenth on each side), Windows'
/// taskbar and Linux docks show the image as it is.
fn cropped(w: usize, h: usize, rgba: &[u8]) -> (usize, Vec<u8>) {
  // The visible pixels' columns and rows, first and last.
  let (mut x0, mut x1, mut y0, mut y1) = (usize::MAX, 0, usize::MAX, 0);
  for (y, row) in rgba.chunks_exact(w * 4).enumerate() {
    let mut seen = row.as_chunks::<4>().0.iter().enumerate().filter(|p| p.1[3] > 8).map(|p| p.0);
    if let Some(first) = seen.next() {
      (x0, x1, y0, y1) = (x0.min(first), x1.max(seen.next_back().unwrap_or(first)), y0.min(y), y);
    }
  }

  if x0 == usize::MAX {
    let side = w.max(h);
    return (side, resized(w, h, rgba, side, side));
  }

  let side = (x1 - x0).max(y1 - y0) + 1;
  // The square's corner, the tile centred in it; outside the image is transparent.
  let left = (x0 + x1 + 1) as isize / 2 - side as isize / 2;
  let top = (y0 + y1 + 1) as isize / 2 - side as isize / 2;
  let (from, to) = (left.max(0) as usize, (left + side as isize).min(w as isize) as usize);
  let mut tile = vec![0; side * side * 4];
  for y in top.max(0) as usize..(top + side as isize).min(h as isize) as usize {
    let at = ((y as isize - top) as usize * side + (from as isize - left) as usize) * 4;
    tile[at..at + (to - from) * 4].copy_from_slice(&rgba[(y * w + from) * 4..(y * w + to) * 4]);
  }

  (side, tile)
}

/// `nw` × `nh` pixels, each the average of the source pixels it covers, weighted by their alpha:
/// averaging straight colours would bleed the transparent black. Only shrinks.
fn resized(w: usize, h: usize, rgba: &[u8], nw: usize, nh: usize) -> Vec<u8> {
  let mut sums = vec![[0f64; 5]; nw * nh];
  let cols: Vec<usize> = (0..w).map(|x| x * nw / w).collect();
  for (y, row) in rgba.chunks_exact(w * 4).enumerate() {
    let line = &mut sums[y * nh / h * nw..][..nw];
    for (p, &x) in row.as_chunks::<4>().0.iter().zip(&cols) {
      let (sum, a) = (&mut line[x], f64::from(p[3]));
      (0..3).for_each(|c| sum[c] += f64::from(p[c]) * a);
      sum[3] += a;
      sum[4] += 1.0;
    }
  }

  (sums.iter())
    .flat_map(|s| {
      let color = |c: f64| if s[3] > 0.0 { (c / s[3]).round() as u8 } else { 0 };
      [color(s[0]), color(s[1]), color(s[2]), (s[3] / s[4]).round() as u8]
    })
    .collect()
}

// MARK: Checksums

fn crc32(data: &[u8]) -> u32 {
  static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
  let table = TABLE.get_or_init(|| {
    std::array::from_fn(|n| {
      (0..8).fold(n as u32, |c, _| if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 })
    })
  });
  !data.iter().fold(!0u32, |c, &b| table[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8))
}

fn adler32(data: &[u8]) -> u32 {
  let (mut a, mut b) = (1u32, 0u32);
  // 5552 bytes keep both sums below 2^32 between reductions (zlib's NMAX).
  for block in data.chunks(5552) {
    for &x in block {
      a += x as u32;
      b += a;
    }

    (a, b) = (a % 65521, b % 65521);
  }

  (b << 16) | a
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn checksums() {
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
  }

  #[test]
  fn deflate_round_trips() {
    let text = b"nimbo nimbo nimbo: a small custom-drawn UI toolkit, a small one".repeat(50);
    let flat = vec![7u8; 100_000];
    let noise: Vec<u8> =
      (0..70_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
    for data in [&b""[..], b"a", b"abcabcabcabc", &text, &flat, &noise] {
      assert_eq!(inflate(&zlib(data)), data);
    }

    assert!(zlib(&flat).len() < 1000);
  }

  #[test]
  fn png_rows_unfilter() {
    // A gradient, a flat run and a half-transparent pixel, through every filter.
    let (w, h) = (9, 4);
    let mut data = Vec::new();
    for y in 0..h {
      for x in 0..w {
        let v = (x * 28 + y * 5) as u8;
        data.extend(if x == 4 { [64, 32, 16, 128] } else { [v, 255 - v, 30, 255] });
      }
    }

    let px = Pixels { w, h, data };
    let want = rgba_rows(&px);
    assert_eq!(decode(&encode(&px)), (w, h, want.clone()));
    // Premultiplied BGRA to straight RGBA.
    assert_eq!(want[16..20], [32, 64, 128, 128]);
  }

  /// A `w` × 1 PNG of colour type `color`, its IDAT `idat` and the `extra` chunks before it.
  fn png(w: u8, color: u8, idat: &[u8], extra: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut file = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut file, b"IHDR", &[0, 0, 0, w, 0, 0, 0, 1, 8, color, 0, 0, 0]);
    extra.iter().for_each(|&(kind, data)| chunk(&mut file, kind, data));
    chunk(&mut file, b"IDAT", idat);
    chunk(&mut file, b"IEND", &[]);
    file
  }

  #[test]
  fn decodes_rgb_and_palettes() {
    // Two RGB pixels through Sub, in a stored block: its length, then the length's complement.
    let stored = [0x78, 0x01, 0x01, 7, 0, !7, 0xff, 1, 10, 20, 30, 5, 5, 5];
    assert_eq!(decode(&png(2, 2, &stored, &[])), (2, 1, vec![10, 20, 30, 255, 15, 25, 35, 255]));
    // Three palette entries through Paeth, the first two with tRNS alphas.
    let idat = zlib(&[4, 0, 1, 1]);
    let extra: [(&[u8; 4], &[u8]); 2] =
      [(b"PLTE", &[1, 2, 3, 4, 5, 6, 7, 8, 9]), (b"tRNS", &[0, 128])];
    let want = vec![1, 2, 3, 0, 4, 5, 6, 128, 7, 8, 9, 255];
    assert_eq!(decode(&png(3, 3, &idat, &extra)), (3, 1, want));
  }

  #[test]
  #[should_panic(expected = "nimbo: a PNG other than 8-bit RGB, RGBA or a palette")]
  fn panics_on_grey() {
    decode(&png(1, 0, &zlib(&[0, 0]), &[]));
  }

  /// Real files (a palette and tRNS, as png-squash leaves them, in dynamic Huffman codes) read as
  /// Pillow reads them: the CRC-32 of its RGBA.
  #[test]
  fn decodes_the_brand_marks() {
    let mark = decode(include_bytes!("../docs/brand/mark-iris.png"));
    assert_eq!((mark.0, mark.1, crc32(&mark.2)), (512, 512, 0x2156_f163));
    let header = decode(include_bytes!("../docs/brand/header-iris.png"));
    assert_eq!((header.0, header.1, crc32(&header.2)), (1200, 300, 0xe44f_a414));
  }

  #[test]
  fn tiles_and_shrinks() {
    // 6 × 4: transparent but for an opaque 2 × 2 at (2, 1), red but for one blue pixel.
    let mut px = vec![0u8; 6 * 4 * 4];
    for (x, y) in [(2, 1), (3, 1), (2, 2), (3, 2)] {
      px[(y * 6 + x) * 4..][..4].copy_from_slice(&[255, 0, 0, 255]);
    }

    px[(2 * 6 + 3) * 4..][..4].copy_from_slice(&[0, 0, 255, 255]);
    let (side, tile) = cropped(6, 4, &px);
    assert_eq!(
      (side, tile[..4].to_vec(), tile[12..16].to_vec()),
      (2, vec![255, 0, 0, 255], vec![0, 0, 255, 255])
    );
    // Halved: a pixel over the red and the blue averages them; over the red and the clear, red
    // at half the alpha, the clear's black kept out.
    let half = resized(6, 4, &px, 3, 2);
    assert_eq!(half[4..8], [255, 0, 0, 128]);
    assert_eq!(half[16..20], [128, 0, 128, 128]);
    assert_eq!(half[..4], [0; 4]);
  }

  /// [Png] decodes once, then gives the same pixels; [Png::icon] is the 256 px tile.
  #[test]
  fn pngs_decode_once() {
    const MARK: Png = Png::new(include_bytes!("../docs/brand/mark-iris.png"));
    let (w, h, px) = MARK.rgba();
    assert_eq!((w, h, px.as_ptr()), (512, 512, MARK.rgba().2.as_ptr()));
    assert_eq!(MARK.side(128).rgba().0, 128);
    let (w, h, icon) = Png::icon(MARK.file).rgba();
    assert_eq!((w, h, icon.len()), (256, 256, 256 * 256 * 4));
    let header = Png::new(include_bytes!("../docs/brand/header-iris.png")).side(600).rgba();
    assert_eq!((header.0, header.1), (600, 150));
  }
}
