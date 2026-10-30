// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What the layout computes from a text in a font (its least width, its lines wrapped at a
//! width), kept across frames, so a steady frame looks it up instead of measuring it again. Each
//! is computed once per (font, text, key) by the caller's function, so it is the same as before;
//! bounded: past a count of entries or of bytes (their text and what they hold), a layout drops
//! what it didn't use (a long text edited makes an entry per keystroke).

use std::collections::HashMap;
use std::collections::hash_map::Entry as Slot;
use std::hash::{BuildHasherDefault, Hash, Hasher};

use super::{Font, FxHasher};

/// Entries kept before a layout starts dropping those it didn't use.
const CAP: usize = 4096;
/// Bytes kept (texts and what their values hold) before a layout starts dropping what it didn't
/// use.
const BYTES: usize = 4 << 20;

/// The bytes a kept value holds besides itself.
pub(crate) trait Held {
  fn held(&self) -> usize {
    0
  }
}

impl Held for f32 {}

impl<T> Held for Vec<T> {
  fn held(&self) -> usize {
    self.capacity() * size_of::<T>()
  }
}

struct Entry<V> {
  font: Font,
  text: Box<str>,
  key: u64,
  value: V,
  /// The last layout that used it.
  used: u64,
}

/// Values by (font, text, key), keyed by a hash of them; the entry keeps them to rule out
/// collisions, so a hit allocates nothing.
pub(crate) struct Memo<V> {
  map: HashMap<u64, Entry<V>, BuildHasherDefault<FxHasher>>,
  /// Layouts so far ([Memo::end]).
  now: u64,
  /// The bytes the entries hold, the replaced included until a layout drops what it didn't use.
  bytes: usize,
  cap: usize,
}

impl<V> Default for Memo<V> {
  fn default() -> Self {
    Self { map: HashMap::default(), now: 0, bytes: 0, cap: CAP }
  }
}

impl<V: Held> Memo<V> {
  /// The value for `text` in `font` under `key` (what else it depends on), from `f` the first
  /// time.
  pub fn get(&mut self, font: Font, text: &str, key: u64, f: impl FnOnce() -> V) -> &V {
    let mut h = FxHasher::default();
    (font.size.to_bits(), font.weight as u8, font.mono, key, text).hash(&mut h);
    let same = |e: &Entry<V>| e.key == key && e.font == font && *e.text == *text;
    let e = match self.map.entry(h.finish()) {
      Slot::Occupied(o) if same(o.get()) => o.into_mut(),
      slot => {
        let value = f();
        self.bytes += text.len() + value.held();
        let e = Entry { font, text: text.into(), key, value, used: 0 };
        slot.insert_entry(e).into_mut()
      }
    };

    e.used = self.now;
    &e.value
  }

  /// Ends a layout: past either cap, drops what it didn't use.
  pub fn end(&mut self) {
    if self.map.len() > self.cap || self.bytes > BYTES {
      let now = self.now;
      self.map.retain(|_, e| e.used == now);
      self.bytes = self.map.values().map(|e| e.text.len() + e.value.held()).sum();
    }

    self.now += 1;
  }
}

#[cfg(test)]
mod tests {
  use std::cell::Cell;

  use super::*;
  use crate::text::Weight;

  impl Held for i32 {}

  #[test]
  fn computes_once_per_font_text_and_key() {
    let (mut memo, calls) = (Memo::default(), Cell::new(0));
    let mut get = |font, text, key| {
      *memo.get(font, text, key, || {
        calls.set(calls.get() + 1);
        calls.get()
      })
    };
    let (ui, mono) = (Font::ui(13.0), Font::mono(13.0));
    assert_eq!([get(ui, "a", 0), get(ui, "a", 0)], [1, 1]);
    assert_eq!([get(mono, "a", 0), get(ui, "b", 0), get(ui, "a", 1)], [2, 3, 4]);
    assert_eq!([get(ui.weight(Weight::Bold), "a", 0), get(ui, "a", 0)], [5, 1]);
  }

  #[test]
  fn past_the_cap_a_layout_drops_what_it_did_not_use() {
    let mut memo = Memo { cap: 3, ..Memo::default() };
    let font = Font::ui(13.0);
    for s in ["a", "b", "c"] {
      memo.get(font, s, 0, || 0);
    }

    // At the cap: all stay.
    memo.end();
    memo.get(font, "a", 0, || 0);
    memo.get(font, "d", 0, || 0);
    assert_eq!(memo.map.len(), 4);
    // Over it: only what the last layout used.
    memo.end();
    let mut kept: Vec<_> = memo.map.values().map(|e| &*e.text).collect();
    kept.sort();
    assert_eq!(kept, ["a", "d"]);
    assert_eq!(*memo.get(font, "b", 0, || 7), 7);
  }

  /// A long text edited: an entry per keystroke, bounded by its bytes long before its count;
  /// what the values hold counts too.
  #[test]
  fn past_its_bytes_a_layout_drops_what_it_did_not_use() {
    let (mut memo, font) = (Memo::default(), Font::ui(13.0));
    let mut text = "w".repeat(50_000);
    for _ in 0..1000 {
      text.push('x');
      memo.get(font, &text, 0, || 0);
      memo.end();
      assert!(memo.bytes <= BYTES + text.len(), "{} bytes kept", memo.bytes);
    }

    assert!(memo.map.len() < 100, "{} entries kept", memo.map.len());
    // Small texts holding much: a line a character.
    let mut memo = Memo::default();
    for i in 0..1000 {
      memo.get(font, &i.to_string(), 0, || vec![0u64; 50_000]);
      memo.end();
    }

    assert!(memo.map.len() < 20, "{} entries kept", memo.map.len());
  }
}
