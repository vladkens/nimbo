// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! What the layout computes from a text in a font (its least width, its lines wrapped at a
//! width), kept across frames, so a steady frame looks it up instead of measuring it again. Each
//! is computed once per (font, text, key) by the caller's function, so it is the same as before;
//! bounded as the shaped-line cache is: past its cap, a layout drops what it didn't use.

use std::collections::HashMap;
use std::collections::hash_map::Entry as Slot;
use std::hash::{BuildHasherDefault, Hash, Hasher};

use super::{Font, FxHasher};

/// Entries kept before a layout starts dropping those it didn't use.
const CAP: usize = 4096;

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
  cap: usize,
}

impl<V> Default for Memo<V> {
  fn default() -> Self {
    Self { map: HashMap::default(), now: 0, cap: CAP }
  }
}

impl<V> Memo<V> {
  /// The value for `text` in `font` under `key` (what else it depends on), from `f` the first
  /// time.
  pub fn get(&mut self, font: Font, text: &str, key: u64, f: impl FnOnce() -> V) -> &V {
    let mut h = FxHasher::default();
    (font.size.to_bits(), font.weight as u8, font.mono, key, text).hash(&mut h);
    let same = |e: &Entry<V>| e.key == key && e.font == font && *e.text == *text;
    let e = match self.map.entry(h.finish()) {
      Slot::Occupied(o) if same(o.get()) => o.into_mut(),
      slot => {
        let e = Entry { font, text: text.into(), key, value: f(), used: 0 };
        slot.insert_entry(e).into_mut()
      }
    };

    e.used = self.now;
    &e.value
  }

  /// Ends a layout: past the cap, drops what it didn't use.
  pub fn end(&mut self) {
    if self.map.len() > self.cap {
      let now = self.now;
      self.map.retain(|_, e| e.used == now);
    }

    self.now += 1;
  }
}

#[cfg(test)]
mod tests {
  use std::cell::Cell;

  use super::*;
  use crate::text::Weight;

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
}
