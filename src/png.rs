// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! A minimal PNG writer for harness shots, the same on every OS: 8-bit RGBA tagged sRGB, each
//! row filtered with the PNG filter that sums smallest (the usual heuristic), compressed by a
//! small deflate (LZ77 over one hash slot per 4 bytes, fixed Huffman codes). Std only. Checked
//! on macOS against ImageIO's shots (2026-10-18): the same pixels, files ~4% larger, written in
//! ~2x the time.

use std::path::Path;

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
    let mut kind = 0;
    let mut least = u64::MAX;
    for k in 0..5 {
      filter(k, row, &prev, &mut cand);
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
  let mut ihdr = Vec::with_capacity(13);
  ihdr.extend((w as u32).to_be_bytes());
  ihdr.extend((h as u32).to_be_bytes());
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

/// Filters `row` with PNG filter `kind` (0 none, 1 sub, 2 up, 3 average, 4 Paeth) into `out`.
fn filter(kind: u8, row: &[u8], prev: &[u8], out: &mut Vec<u8>) {
  out.clear();
  for i in 0..row.len() {
    let a = if i >= 4 { row[i - 4] } else { 0 };
    let (b, c) = (prev[i], if i >= 4 { prev[i - 4] } else { 0 });
    let pred = match kind {
      0 => 0,
      1 => a,
      2 => b,
      3 => ((a as u16 + b as u16) / 2) as u8,
      _ => paeth(a, b, c),
    };

    out.push(row[i].wrapping_sub(pred));
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
  out.extend((data.len() as u32).to_be_bytes());
  let at = out.len();
  out.extend_from_slice(kind);
  out.extend_from_slice(data);
  let crc = crc32(&out[at..]);
  out.extend(crc.to_be_bytes());
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
    let mut len = 0;
    let mut dist = 0;
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
const LENGTHS: [(usize, u32); 29] = [
  (3, 0),
  (4, 0),
  (5, 0),
  (6, 0),
  (7, 0),
  (8, 0),
  (9, 0),
  (10, 0),
  (11, 1),
  (13, 1),
  (15, 1),
  (17, 1),
  (19, 2),
  (23, 2),
  (27, 2),
  (31, 2),
  (35, 3),
  (43, 3),
  (51, 3),
  (59, 3),
  (67, 4),
  (83, 4),
  (99, 4),
  (115, 4),
  (131, 5),
  (163, 5),
  (195, 5),
  (227, 5),
  (258, 0),
];
const DISTANCES: [(usize, u32); 30] = [
  (1, 0),
  (2, 0),
  (3, 0),
  (4, 0),
  (5, 1),
  (7, 1),
  (9, 2),
  (13, 2),
  (17, 3),
  (25, 3),
  (33, 4),
  (49, 4),
  (65, 5),
  (97, 5),
  (129, 6),
  (193, 6),
  (257, 7),
  (385, 7),
  (513, 8),
  (769, 8),
  (1025, 9),
  (1537, 9),
  (2049, 10),
  (3073, 10),
  (4097, 11),
  (6145, 11),
  (8193, 12),
  (12289, 12),
  (16385, 13),
  (24577, 13),
];

/// The code of `v` in a table of (base, extra bits): its index, base and extra bits.
fn bucket(table: &[(usize, u32)], v: usize) -> (usize, usize, u32) {
  let k = table.iter().rposition(|&(base, _)| base <= v).unwrap();
  (k, table[k].0, table[k].1)
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
pub(crate) mod tests {
  use super::*;

  /// Inflates a stream of fixed-Huffman blocks, the only kind [zlib] writes.
  fn inflate(z: &[u8]) -> Vec<u8> {
    let data = &z[2..z.len() - 4];
    let mut pos = 0;
    let mut bit = |n: u32| {
      let mut v = 0;
      for k in 0..n {
        v |= ((data[pos / 8] >> (pos % 8)) as u32 & 1) << k;
        pos += 1;
      }

      v
    };
    let mut out: Vec<u8> = Vec::new();
    assert_eq!(bit(3), 0b011, "one final fixed block");
    // Codes read most significant bit first.
    let code = |bit: &mut dyn FnMut(u32) -> u32, n: u32| (0..n).fold(0, |c, _| (c << 1) | bit(1));
    loop {
      let mut c = code(&mut bit, 7);
      let sym = match c {
        0..=0x17 => c + 256,
        _ => {
          c = c << 1 | bit(1);
          match c {
            0x30..=0xbf => c - 0x30,
            0xc0..=0xc7 => c - 0xc0 + 280,
            _ => (c << 1 | bit(1)) - 0x190 + 144,
          }
        }
      };

      match sym {
        0..=255 => out.push(sym as u8),
        256 => break,
        _ => {
          let (base, extra) = LENGTHS[sym as usize - 257];
          let len = base + bit(extra) as usize;
          let (base, extra) = DISTANCES[code(&mut bit, 5) as usize];
          let dist = base + bit(extra) as usize;
          for _ in 0..len {
            out.push(out[out.len() - dist]);
          }
        }
      }
    }

    assert_eq!(adler32(&out).to_be_bytes(), z[z.len() - 4..]);
    out
  }

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
      let z = zlib(data);
      assert_eq!(inflate(&z), data);
    }

    assert!(zlib(&flat).len() < 1000);
  }

  /// The size and straight RGBA rows of a PNG file [encode_rgba] wrote, its chunks checked:
  /// IHDR, sRGB, IDAT, IEND with valid CRCs.
  pub(crate) fn decode(file: &[u8]) -> (usize, usize, Vec<u8>) {
    assert_eq!(&file[..8], b"\x89PNG\r\n\x1a\n");
    let mut at = 8;
    let mut kinds = Vec::new();
    let (mut ihdr, mut idat) = (Vec::new(), Vec::new());
    while at < file.len() {
      let n = u32::from_be_bytes(file[at..at + 4].try_into().unwrap()) as usize;
      let body = &file[at + 4..at + 8 + n];
      let crc = u32::from_be_bytes(file[at + 8 + n..at + 12 + n].try_into().unwrap());
      assert_eq!(crc32(body), crc);
      kinds.push(String::from_utf8_lossy(&body[..4]).into_owned());
      match &body[..4] {
        b"IHDR" => ihdr.extend_from_slice(&body[4..]),
        b"IDAT" => idat.extend_from_slice(&body[4..]),
        _ => {}
      }

      at += 12 + n;
    }

    assert_eq!(kinds, ["IHDR", "sRGB", "IDAT", "IEND"]);
    let size = |i: usize| u32::from_be_bytes(ihdr[i..i + 4].try_into().unwrap()) as usize;
    let (w, h) = (size(0), size(4));
    assert_eq!(ihdr[8..], [8, 6, 0, 0, 0], "8-bit RGBA");
    let raw = inflate(&idat);
    assert_eq!(raw.len(), (w * 4 + 1) * h);
    let mut out = Vec::with_capacity(w * h * 4);
    let mut prev = vec![0u8; w * 4];
    for line in raw.chunks(w * 4 + 1) {
      let mut row = vec![0u8; w * 4];
      for i in 0..w * 4 {
        let a = if i >= 4 { row[i - 4] } else { 0 };
        let (b, c) = (prev[i], if i >= 4 { prev[i - 4] } else { 0 });
        let pred = match line[0] {
          0 => 0,
          1 => a,
          2 => b,
          3 => ((a as u16 + b as u16) / 2) as u8,
          _ => paeth(a, b, c),
        };

        row[i] = line[1 + i].wrapping_add(pred);
      }

      out.extend_from_slice(&row);
      prev = row;
    }

    (w, h, out)
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
}
