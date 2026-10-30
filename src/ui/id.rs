// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Element ids: an element's index among its siblings, its key or its name, hashed with its
//! parent's id, so it stays the same across frames while the app builds the same element at the
//! same place ([super::Ui::key], [super::Ui::id]).

use std::hash::{Hash, Hasher};

use crate::text::FxHasher;

/// An element's id, stable across frames while the app builds the same element at the same place
/// (or with the same key or name).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Id(pub u64);

impl Id {
  /// The implicit root: the window, a column.
  pub const ROOT: Self = Self(0);

  /// A frame-wide id from a name: what [super::Ui::id] gives an element.
  pub fn of(name: impl Hash) -> Self {
    hash((2u8, name))
  }

  /// A child's id from a key unique among its siblings: what [super::Ui::key] gives an element.
  pub fn key(self, key: impl Hash) -> Self {
    hash((self.0, 1u8, key))
  }

  /// The id of the `i`-th child with no key or name.
  pub fn nth(self, i: usize) -> Self {
    hash((self.0, 0u8, i))
  }
}

fn hash(v: impl Hash) -> Id {
  // A seed: FxHash of zeros from a zero state is zero, the root's id.
  let mut h = IdHasher(FxHasher::default());
  h.write_u64(0x9e37_79b9_7f4a_7c15);
  v.hash(&mut h);
  Id(h.finish())
}

/// FxHash with every integer fed in as 64 bits, so a key doesn't depend on its integer's type:
/// `id.key(3)`, `id.key(3usize)` and `id.key(3u8)` are one id, and so are `-1i32` and `-1i64`.
struct IdHasher(FxHasher);

impl Hasher for IdHasher {
  fn finish(&self) -> u64 {
    self.0.finish()
  }

  fn write(&mut self, bytes: &[u8]) {
    self.0.write(bytes);
  }

  fn write_u8(&mut self, v: u8) {
    self.0.write_u64(v as u64);
  }

  fn write_u16(&mut self, v: u16) {
    self.0.write_u64(v as u64);
  }

  fn write_u32(&mut self, v: u32) {
    self.0.write_u64(v as u64);
  }

  fn write_u64(&mut self, v: u64) {
    self.0.write_u64(v);
  }

  fn write_usize(&mut self, v: usize) {
    self.0.write_u64(v as u64);
  }

  fn write_i8(&mut self, v: i8) {
    self.0.write_u64(v as i64 as u64);
  }

  fn write_i16(&mut self, v: i16) {
    self.0.write_u64(v as i64 as u64);
  }

  fn write_i32(&mut self, v: i32) {
    self.0.write_u64(v as i64 as u64);
  }

  fn write_i64(&mut self, v: i64) {
    self.0.write_u64(v as u64);
  }

  fn write_isize(&mut self, v: isize) {
    self.0.write_u64(v as i64 as u64);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn integer_keys_hash_alike_whatever_their_type() {
    let list = Id::of("list");
    assert_eq!(list.key(3), list.key(3usize));
    assert_eq!(list.key(3u8), list.key(3u64));
    assert_eq!(list.key(-1i32), list.key(-1i64));
    assert_eq!(list.key(("row", 5u32)), list.key(("row", 5u64)));
    assert_ne!(list.key(3), list.key(4));
    assert_ne!(list.key("a"), list.key("b"));
    assert_ne!(list.key(3), list.nth(3));
    assert_ne!(Id::of("a"), Id::ROOT);
  }
}
