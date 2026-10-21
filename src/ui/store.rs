// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Per-element state the app should not hold (scroll offsets, hover animations, text cursors),
//! typed and keyed by element id, and property animations in Sublime's style: a `target` and a
//! `speed`, linear or smoothstep.
//!
//! The rule: an entry lives while its id is built or its state used in each frame; it is dropped
//! after the first frame without either, unless an ancestor (or the id itself) is retained with
//! [super::Ui::retain] that frame, as a hidden tab keeps its subtree's state. Virtualized lists
//! and trees keep per-row state (expanded, selected) by row key in their own state or in the app
//! model, never per visible element: rows scrolled out are not built and would lose theirs.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::time::Duration;

use super::Id;
use super::layout::Tree;
use crate::text::FxHasher;

struct Entry {
  val: Box<dyn Any>,
  /// The frame it was last used in.
  frame: u64,
  /// The ids of its element's ancestors when it was last used, root first.
  path: Vec<Id>,
}

#[derive(Default)]
pub struct Store {
  map: HashMap<(Id, TypeId), Entry, BuildHasherDefault<FxHasher>>,
  frame: u64,
  retained: Vec<Id>,
}

impl Store {
  /// The `T` of `id`, created with its default; marks it used this frame under `path`.
  pub fn get<T: Default + 'static>(&mut self, id: Id, path: impl Iterator<Item = Id>) -> &mut T {
    let frame = self.frame;
    let e = self.map.entry((id, TypeId::of::<T>())).or_insert_with(|| Entry {
      val: Box::new(T::default()),
      frame,
      path: Vec::new(),
    });
    e.frame = frame;
    e.path.clear();
    e.path.extend(path);
    e.val.downcast_mut().unwrap()
  }

  /// The `T` of `id`, created if missing, without marking it used (event handling).
  pub fn raw<T: Default + 'static>(&mut self, id: Id) -> &mut T {
    let frame = self.frame;
    let e = self.map.entry((id, TypeId::of::<T>())).or_insert_with(|| Entry {
      val: Box::new(T::default()),
      frame,
      path: Vec::new(),
    });
    e.val.downcast_mut().unwrap()
  }

  pub fn peek<T: 'static>(&self, id: Id) -> Option<&T> {
    self.map.get(&(id, TypeId::of::<T>())).and_then(|e| e.val.downcast_ref())
  }

  pub fn retain(&mut self, id: Id) {
    self.retained.push(id);
  }

  #[cfg(test)]
  fn len(&self) -> usize {
    self.map.len()
  }

  /// Ends a frame whose laid-out tree is `tree`: drops what it neither built, used nor retained.
  pub fn gc(&mut self, tree: &Tree) {
    let (frame, kept) = (self.frame, &self.retained);
    self.map.retain(|(id, _), e| {
      e.frame == frame
        || tree.get(*id).is_some()
        || kept.iter().any(|r| r == id || e.path.contains(r))
    });
    self.retained.clear();
    self.frame += 1;
  }
}

// MARK: Animation

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ease {
  #[default]
  Linear,
  /// `3t² - 2t³`: starts and ends slowly.
  Smooth,
}

/// How an animated value moves to a new target: Sublime's `speed` (larger is faster; here the
/// value moves `speed` units per second, so 0 to 1 at speed 4 takes 250 ms) and its
/// `interpolation`. `const`, so a theme holds them: `const FADE: Transition =
/// Transition::smooth(6.0)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
  pub speed: f32,
  pub ease: Ease,
}

impl Transition {
  pub const fn linear(speed: f32) -> Self {
    Self { speed, ease: Ease::Linear }
  }

  pub const fn smooth(speed: f32) -> Self {
    Self { speed, ease: Ease::Smooth }
  }
}

/// One animated value: from where it was when the target last changed, toward the target.
#[derive(Default)]
pub(super) struct Anim {
  from: f32,
  to: f32,
  start: Duration,
  /// Seconds from `from` to `to`.
  dur: f32,
  ease: Ease,
  init: bool,
}

impl Anim {
  fn at(&self, now: Duration) -> f32 {
    if self.dur <= 0.0 {
      return self.to;
    }

    let t = (now.saturating_sub(self.start).as_secs_f32() / self.dur).max(0.0);
    if t >= 1.0 {
      return self.to;
    }

    let k = match self.ease {
      Ease::Linear => t,
      Ease::Smooth => t * t * (3.0 - 2.0 * t),
    };

    self.from + (self.to - self.from) * k
  }

  /// The value at `now` heading for `target` (a new target starts from the current value; the
  /// first one is taken as is) and whether it is still moving.
  pub fn step(&mut self, target: f32, tr: Transition, now: Duration) -> (f32, bool) {
    if !self.init {
      (self.from, self.to, self.init) = (target, target, true);
    }

    if target != self.to {
      let cur = self.at(now);
      self.from = cur;
      self.to = target;
      self.start = now;
      self.dur = if tr.speed > 0.0 { (target - cur).abs() / tr.speed } else { 0.0 };
      self.ease = tr.ease;
    }

    let v = self.at(now);
    (v, v != self.to)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scene::Size;

  fn ms(v: u64) -> Duration {
    Duration::from_millis(v)
  }

  #[test]
  fn anim_moves_at_speed_and_retargets() {
    let close = |(v, moving): (f32, bool), want: f32| (v - want).abs() < 1e-4 && moving;
    let mut a = Anim::default();
    let tr = Transition::linear(4.0);
    assert_eq!(a.step(0.0, tr, ms(0)), (0.0, false));
    // 0 to 1 at 4 per second: 250 ms.
    assert_eq!(a.step(1.0, tr, ms(100)), (0.0, true));
    assert!(close(a.step(1.0, tr, ms(150)), 0.2));
    assert_eq!(a.step(1.0, tr, ms(350)), (1.0, false));
    // Back halfway, then reversed: starts from where it is.
    a.step(0.0, tr, ms(400));
    assert!(close(a.step(0.0, tr, ms(525)), 0.5));
    assert!(close(a.step(1.0, tr, ms(525)), 0.5));
    assert_eq!(a.step(1.0, tr, ms(650)), (1.0, false));

    let mut s = Anim::default();
    s.step(0.0, Transition::smooth(1.0), ms(0));
    s.step(1.0, Transition::smooth(1.0), ms(0));
    let (q, h) = (s.step(1.0, Transition::smooth(1.0), ms(250)).0, s.at(ms(500)));
    assert!(q < 0.25 && h == 0.5, "{q} {h}");
  }

  #[test]
  fn gc_drops_unseen_unless_retained() {
    let (a, b, tab) = (Id::of("a"), Id::of("b"), Id::of("tab"));
    let mut st = Store::default();
    let mut tree = Tree::default();
    tree.clear(Size::new(10.0, 10.0));
    *st.get::<u32>(a, [Id::ROOT].into_iter()) = 1;
    *st.get::<u32>(b, [Id::ROOT, tab].into_iter()) = 2;
    *st.get::<f32>(b, [Id::ROOT, tab].into_iter()) = 3.0;
    st.gc(&tree);
    assert_eq!(st.len(), 3);

    // A frame that uses only `a` but retains the tab: `b` stays.
    st.get::<u32>(a, std::iter::empty());
    st.retain(tab);
    st.gc(&tree);
    assert_eq!((st.len(), st.peek::<u32>(b)), (3, Some(&2)));

    // Not retained: dropped, both types.
    st.get::<u32>(a, std::iter::empty());
    st.gc(&tree);
    assert_eq!((st.len(), st.peek::<f32>(b)), (1, None));
    // Unused for a frame: dropped too.
    st.gc(&tree);
    assert_eq!(st.len(), 0);
  }
}
