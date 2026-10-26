// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The view's accessibility: the last frame's nodes ([A11yTree]) as `NSAccessibilityElement`s
//! for VoiceOver and the other assistive clients, their actions sent to the runtime as
//! [Event::A11y], and the notifications a new frame calls for.
//!
//! Nothing runs until a client asks the view for its children, its focus or a hit: the first
//! query takes a snapshot of the last frame at once (a client that finds a window empty keeps it
//! so), and from then on the view takes one after each frame ([sync]), outside the runtime's
//! borrow, posting what changed since the last ([diff]). The elements, one per node, kept by id
//! and role so a client's references hold across frames, read only the snapshot: clients ask
//! from nested run loops, while the runtime is busy up the stack. An element whose node is gone
//! answers nothing. Actions go through the view's event queue (deferred while it is busy), and
//! those the node doesn't take answer NO.
//!
//! Ported in part from AccessKit (MIT / Apache-2.0), `accesskit_macos` 0.27: `node.rs` (roles,
//! attributes, allowed selectors), `adapter.rs` (the view's queries), `event.rs` and
//! `context.rs` (the notifications).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
  NSAccessibility, NSAccessibilityElement, NSAccessibilityFocusedUIElementChangedNotification,
  NSAccessibilityLayoutChangedNotification, NSAccessibilityPostNotification,
  NSAccessibilitySelectedChildrenChangedNotification,
  NSAccessibilitySelectedRowsChangedNotification, NSAccessibilitySelectedTextChangedNotification,
  NSAccessibilityTitleChangedNotification, NSAccessibilityUIElementDestroyedNotification,
  NSAccessibilityValueChangedNotification,
};
use objc2_foundation::{
  NSArray, NSInteger, NSNumber, NSObject, NSPoint, NSRange, NSRect, NSString, ns_string,
};

use super::input::{ns_range, to_range};
use super::view::View;
use crate::app::Event;
use crate::scene::Point;
use crate::text::edit::{range_from_utf16, utf16_len};
use crate::ui::{A11yNode, A11yTree, Action, Id, Role, State, UiState, Value};

/// A node as elements know it across frames.
type Key = (Id, Role);

fn key(n: &A11yNode) -> Key {
  (n.id, n.props.role)
}

/// AppKit's role and subrole for `r` (accesskit_macos `node.rs`, `ns_role` and `ns_sub_role`; a
/// dialog in the window as WebKit's ARIA dialog).
fn ax_role(r: Role) -> (&'static str, Option<&'static str>) {
  use Role::*;
  match r {
    Group => ("AXGroup", None),
    Button => ("AXButton", None),
    CheckBox => ("AXCheckBox", None),
    Switch => ("AXCheckBox", Some("AXSwitch")),
    RadioButton => ("AXRadioButton", None),
    Tab => ("AXRadioButton", Some("AXTabButton")),
    RadioGroup => ("AXRadioGroup", None),
    Slider => ("AXSlider", None),
    TextField => ("AXTextField", None),
    TextArea => ("AXTextArea", None),
    SecureTextField => ("AXTextField", Some("AXSecureTextField")),
    StaticText => ("AXStaticText", None),
    Link => ("AXLink", None),
    PopUpButton => ("AXPopUpButton", None),
    Menu => ("AXMenu", None),
    MenuItem => ("AXMenuItem", None),
    TabList => ("AXTabGroup", None),
    List => ("AXList", None),
    Row => ("AXRow", None),
    ProgressIndicator => ("AXProgressIndicator", None),
    Dialog => ("AXGroup", Some("AXApplicationDialog")),
    Splitter => ("AXSplitter", None),
  }
}

/// A container whose selected children a client reads: rows, tabs, a segmented control's.
fn selects(r: Role) -> bool {
  matches!(r, Role::List | Role::TabList | Role::RadioGroup)
}

/// Whether a node answers `sel` (accesskit_macos `node.rs`, `is_selector_allowed`): its actions
/// as [A11yNode::can] allows them, the text ranges of a text field, a range of a number, the
/// states of the roles that have them, no other setter; `None` for what AppKit decides.
fn allowed(n: &A11yNode, sel: &str) -> Option<bool> {
  use Role::*;
  let role = n.props.role;
  Some(match sel {
    "accessibilityPerformPress" => n.can(&Action::Press),
    "accessibilityPerformIncrement" => n.can(&Action::Increment),
    "accessibilityPerformDecrement" => n.can(&Action::Decrement),
    "setAccessibilityFocused:" => n.can(&Action::Focus),
    "setAccessibilityValue:" => n.can(&Action::SetValue(String::new())),
    "accessibilityMinValue" | "accessibilityMaxValue" => {
      matches!(n.props.value, Value::Number { .. })
    }
    "accessibilityNumberOfCharacters"
    | "accessibilitySelectedText"
    | "accessibilitySelectedTextRange"
    | "accessibilityInsertionPointLineNumber"
    | "accessibilityStringForRange:" => matches!(role, TextField | TextArea),
    "accessibilitySelectedChildren" => selects(role),
    "isAccessibilitySelected" => matches!(role, Row | Tab | MenuItem),
    "isAccessibilityModal" => role == Dialog,
    "isAccessibilityExpanded" => role == PopUpButton || n.props.has(State::Expanded),
    // A setter not above is not ours to take.
    s if s.starts_with("set") => false,
    _ => return None,
  })
}

/// The last frame as clients see it.
#[derive(Default)]
pub(super) struct Snapshot {
  tree: A11yTree,
  /// The first node of each key.
  index: HashMap<Key, u32>,
  /// The focused text field: its selection in UTF-16 and the line of its caret.
  field: Option<(Id, Range<usize>, usize)>,
}

impl Snapshot {
  fn new(ui: &UiState) -> Self {
    let field = ui.focus().zip(ui.text_input()).map(|(id, t)| {
      // The caret's line, from its rect against the first line's.
      let line = match (t.rect(0..0), t.caret_rect()) {
        (Some((a, _)), Some(c)) if a.h > 0.0 => ((c.y - a.y) / a.h).round().max(0.0) as usize,
        _ => 0,
      };

      (id, t.selected(), line)
    });
    Self::of(ui.a11y_tree(), field)
  }

  fn of(tree: A11yTree, field: Option<(Id, Range<usize>, usize)>) -> Self {
    let mut index = HashMap::with_capacity(tree.nodes.len());
    for (n, i) in tree.nodes.iter().zip(0..) {
      index.entry(key(n)).or_insert(i);
    }

    Self { tree, index, field }
  }

  fn node(&self, i: u32) -> &A11yNode {
    &self.tree.nodes[i as usize]
  }

  /// The nodes right inside `i`, or the top-level ones.
  fn children(&self, i: Option<u32>) -> Vec<u32> {
    let (mut k, end) = i.map_or((0, self.tree.nodes.len() as u32), |i| (i + 1, self.node(i).end));
    let mut out = Vec::new();
    while k < end {
      out.push(k);
      k = self.node(k).end;
    }

    out
  }

  /// The children of a container that are selected (rows, tabs) or on (radio buttons).
  fn selected(&self, i: u32) -> Vec<Key> {
    let on = |n: &A11yNode| n.props.has(State::Selected) || n.props.value == Value::On;
    let kids = self.children(Some(i)).into_iter().map(|k| self.node(k));
    kids.filter(|n| on(n)).map(key).collect()
  }

  /// The focused node, or the descendant it has the cursor on (a list's row).
  fn focused(&self) -> Option<u32> {
    let f = self.tree.nodes.iter().position(|n| n.props.has(State::Focused))?;
    let active = self.tree.nodes[f].props.active;
    let at = active.and_then(|a| self.tree.nodes.iter().position(|n| n.id == a));
    Some(at.unwrap_or(f) as u32)
  }

  /// A field's text.
  fn text(&self, n: &A11yNode) -> &str {
    if let Value::Text(s) = n.props.value { self.tree.str(s) } else { "" }
  }

  /// The selection and the caret's line of `n`, the focused field.
  fn sel(&self, n: &A11yNode) -> Option<(Range<usize>, usize)> {
    self.field.as_ref().filter(|f| f.0 == n.id).map(|f| (f.1.clone(), f.2))
  }

  /// A node's value to compare: its text, else the value itself.
  fn val(&self, n: &A11yNode) -> (Value, &str) {
    match n.props.value {
      Value::Text(s) => (Value::Text((0, 0)), self.tree.str(s)),
      v => (v, ""),
    }
  }
}

/// What a client is told of a new frame.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Note {
  /// The nodes or their nesting changed (not their rects): on the window.
  Layout,
  Destroyed(Key),
  Title(Key),
  /// A container's selected children: a list's rows, a tab group's or a radio group's.
  Selected(Key),
  Value(Key),
  Focus(Key),
  /// The focused field's selection.
  Text(Key),
}

/// The notifications from `old` to `new` (accesskit_macos `event.rs`): the layout when the
/// nodes or their nesting changed, never for rects alone (an animation); the nodes gone; names,
/// a container's selection and values changed, the focused node's value last; the focus moved;
/// the focused field's selection moved. A static text's name is its value.
fn diff(old: &Snapshot, new: &Snapshot) -> Vec<Note> {
  let shape = |s: &Snapshot| s.tree.nodes.iter().map(|n| (key(n), n.parent)).collect::<Vec<_>>();
  let mut out = Vec::new();
  if shape(old) != shape(new) {
    out.push(Note::Layout);
  }

  let gone = old.tree.nodes.iter().map(key).filter(|k| !new.index.contains_key(k));
  out.extend(gone.map(Note::Destroyed));
  let (focus, mut values, mut last) = (new.focused(), Vec::new(), None);
  for (n, i) in new.tree.nodes.iter().zip(0..) {
    let k = key(n);
    let Some(&j) = old.index.get(&k).filter(|_| new.index[&k] == i) else { continue };
    let o = old.node(j);
    let mut named = old.tree.str(o.props.name) != new.tree.str(n.props.name);
    let mut valued = old.val(o) != new.val(n);
    if n.props.role == Role::StaticText {
      (named, valued) = (false, valued || named);
    }

    if named {
      out.push(Note::Title(k));
    }

    if selects(n.props.role) && old.selected(j) != new.selected(i) {
      out.push(Note::Selected(k));
    }

    match focus == Some(i) {
      true => last = last.or(valued.then_some(Note::Value(k))),
      false => values.extend(valued.then_some(Note::Value(k))),
    }
  }

  out.extend(values);
  let was = old.focused().map(|i| key(old.node(i)));
  let now = focus.map(|i| key(new.node(i)));
  if now.is_some() && now != was {
    out.extend(now.map(Note::Focus));
  }

  // The same field, its selection moved.
  if let (Some((id, sel, _)), Some((was, before, _))) = (&new.field, &old.field)
    && id == was
    && sel != before
    && let Some(n) = new.tree.nodes.iter().find(|n| n.id == *id)
  {
    out.push(Note::Text(key(n)));
  }

  out.extend(last);
  out
}

/// What the view and its elements share: the snapshot, the elements by key, the view itself,
/// weakly.
pub(super) struct Shared {
  view: ObjcWeak<View>,
  snap: RefCell<Snapshot>,
  elements: RefCell<HashMap<Key, Retained<Element>>>,
}

impl Shared {
  /// The element of `k`, the same one each time.
  fn element(self: &Rc<Self>, k: Key) -> Option<Retained<AnyObject>> {
    let mtm = MainThreadMarker::new()?;
    let mut map = self.elements.try_borrow_mut().ok()?;
    let e = map.entry(k).or_insert_with(|| Element::new(mtm, Rc::downgrade(self), k));
    Some(any(e.clone()))
  }

  fn array(self: &Rc<Self>, nodes: impl IntoIterator<Item = Key>) -> Retained<NSArray> {
    let all: Vec<_> = nodes.into_iter().filter_map(|k| self.element(k)).collect();
    NSArray::from_retained_slice(&all)
  }
}

/// An object as `id`.
fn any<T: Message>(r: Retained<T>) -> Retained<AnyObject> {
  // SAFETY: every Objective-C object is an AnyObject.
  unsafe { Retained::cast_unchecked(r) }
}

fn ns(s: &str) -> Option<Retained<NSString>> {
  (!s.is_empty()).then(|| NSString::from_str(s))
}

/// A number as the widgets keep it, in f32: 0.42, not 0.41999998688697815.
fn num(v: f64) -> Retained<AnyObject> {
  any(NSNumber::new_f64((v as f32).to_string().parse().unwrap_or(v)))
}

/// The text of UTF-16 range `r` of `s`, clamped to it.
fn substring(s: &str, r: Range<usize>) -> Retained<NSString> {
  NSString::from_str(&s[range_from_utf16(s, r.start.min(r.end)..r.end)])
}

/// What an element knows: the snapshot and its node's key; dead once told destroyed, so a
/// client's old reference never comes back when the key does (a new element stands for it).
pub(super) struct Node {
  shared: Weak<Shared>,
  key: Key,
  dead: Cell<bool>,
}

define_class!(
  // SAFETY: NSAccessibilityElement has no subclassing requirements; we override its accessors.
  #[unsafe(super(NSAccessibilityElement, NSObject))]
  #[thread_kind = MainThreadOnly]
  #[name = "NimboAccessibilityElement"]
  #[ivars = Node]
  pub(super) struct Element;

  impl Element {
    #[unsafe(method(isAccessibilityElement))]
    fn is_element(&self) -> bool {
      self.get(|_, _| Some(true)).unwrap_or(false)
    }

    #[unsafe(method_id(accessibilityRole))]
    fn role(&self) -> Option<Retained<NSString>> {
      self.get(|_, n| ns(ax_role(n.props.role).0))
    }

    #[unsafe(method_id(accessibilitySubrole))]
    fn subrole(&self) -> Option<Retained<NSString>> {
      self.get(|_, n| ns(ax_role(n.props.role).1?))
    }

    /// The name, but a static text's, which is its value.
    #[unsafe(method_id(accessibilityTitle))]
    fn title(&self) -> Option<Retained<NSString>> {
      self.get(|s, n| ns(s.tree.str(n.props.name)).filter(|_| n.props.role != Role::StaticText))
    }

    #[unsafe(method_id(accessibilityHelp))]
    fn help(&self) -> Option<Retained<NSString>> {
      self.get(|s, n| ns(s.tree.str(n.props.description)))
    }

    /// Text as a string; a number, a check state (0, 1, 2 mixed) or a tab's selection as a
    /// number; a static text's name.
    #[unsafe(method_id(accessibilityValue))]
    fn value(&self) -> Option<Retained<AnyObject>> {
      self.get(|s, n| {
        let int = |v: isize| Some(any(NSNumber::new_isize(v)));
        match (n.props.value, n.props.role) {
          (Value::Text(t), _) => Some(any(NSString::from_str(s.tree.str(t)))),
          (Value::Number { now, .. }, _) => Some(num(now)),
          (Value::Off, _) => int(0),
          (Value::On, _) => int(1),
          (Value::Mixed, _) => int(2),
          (Value::None, Role::Tab) => int(n.props.has(State::Selected).into()),
          (Value::None, Role::StaticText) => Some(any(NSString::from_str(s.tree.str(n.props.name)))),
          (Value::None, _) => None,
        }
      })
    }

    #[unsafe(method_id(accessibilityMinValue))]
    fn min_value(&self) -> Option<Retained<AnyObject>> {
      self.bound(false)
    }

    #[unsafe(method_id(accessibilityMaxValue))]
    fn max_value(&self) -> Option<Retained<AnyObject>> {
      self.bound(true)
    }

    #[unsafe(method(isAccessibilityEnabled))]
    fn is_enabled(&self) -> bool {
      self.get(|_, n| Some(!n.props.has(State::Disabled))).unwrap_or(false)
    }

    #[unsafe(method(isAccessibilityFocused))]
    fn is_focused(&self) -> bool {
      self.with(|_, s, i| Some(s.focused() == Some(i))).unwrap_or(false)
    }

    #[unsafe(method(isAccessibilitySelected))]
    fn is_selected(&self) -> bool {
      self.has(State::Selected)
    }

    /// A pop-up button while its menu is open.
    #[unsafe(method(isAccessibilityExpanded))]
    fn is_expanded(&self) -> bool {
      self.has(State::Expanded)
    }

    #[unsafe(method(isAccessibilityModal))]
    fn is_modal(&self) -> bool {
      self.has(State::Modal)
    }

    #[unsafe(method(accessibilityNotifiesWhenDestroyed))]
    fn notifies_when_destroyed(&self) -> bool {
      true
    }

    /// In screen coordinates, from the view's points as the window is now.
    #[unsafe(method(accessibilityFrame))]
    fn frame(&self) -> NSRect {
      let rect = self.with(|sh, s, i| Some((sh.view.load()?, s.node(i).rect)));
      rect.map_or(NSRect::ZERO, |(view, r)| view.to_screen(r))
    }

    /// The node around it, or the window for a top-level one: the view is not an element.
    #[unsafe(method_id(accessibilityParent))]
    fn parent(&self) -> Option<Retained<AnyObject>> {
      self.up()
    }

    #[unsafe(method_id(accessibilityChildren))]
    fn children(&self) -> Option<Retained<NSArray>> {
      self.with(|sh, s, i| {
        let kids = s.children(Some(i)).into_iter().map(|k| key(s.node(k)));
        Some(sh.array(kids.collect::<Vec<_>>()))
      })
    }

    #[unsafe(method_id(accessibilitySelectedChildren))]
    fn selected_children(&self) -> Option<Retained<NSArray>> {
      self.with(|sh, s, i| selects(s.node(i).props.role).then(|| sh.array(s.selected(i))))
    }

    #[unsafe(method_id(accessibilityWindow))]
    fn window(&self) -> Option<Retained<AnyObject>> {
      self.top()
    }

    #[unsafe(method_id(accessibilityTopLevelUIElement))]
    fn top_level(&self) -> Option<Retained<AnyObject>> {
      self.top()
    }

    #[unsafe(method(accessibilityNumberOfCharacters))]
    fn characters(&self) -> NSInteger {
      self.get(|s, n| Some(utf16_len(s.text(n)) as NSInteger)).unwrap_or(0)
    }

    #[unsafe(method(accessibilitySelectedTextRange))]
    fn selected_range(&self) -> NSRange {
      self.get(|s, n| Some(ns_range(s.sel(n)?.0))).unwrap_or(NSRange::new(0, 0))
    }

    #[unsafe(method_id(accessibilitySelectedText))]
    fn selected_text(&self) -> Option<Retained<NSString>> {
      self.get(|s, n| Some(substring(s.text(n), s.sel(n)?.0)))
    }

    #[unsafe(method_id(accessibilityStringForRange:))]
    fn string_for_range(&self, r: NSRange) -> Option<Retained<NSString>> {
      self.get(|s, n| Some(substring(s.text(n), to_range(r)?)))
    }

    #[unsafe(method(accessibilityInsertionPointLineNumber))]
    fn insertion_line(&self) -> NSInteger {
      self.get(|s, n| Some(s.sel(n)?.1 as NSInteger)).unwrap_or(0)
    }

    /// A menu item's check mark: the pop-up's current item.
    #[unsafe(method_id(accessibilityAttributeValue:))]
    fn attribute(&self, name: &NSString) -> Option<Retained<AnyObject>> {
      if !name.isEqualToString(ns_string!("AXMenuItemMarkChar")) {
        return unsafe { msg_send![super(self), accessibilityAttributeValue: name] };
      }

      let mark = |n: &A11yNode| n.props.role == Role::MenuItem && n.props.has(State::Selected);
      self.get(|_, n| mark(n).then(|| any(NSString::from_str("✓"))))
    }

    #[unsafe(method(isAccessibilitySelectorAllowed:))]
    fn selector_allowed(&self, sel: Sel) -> bool {
      let name = sel.name().to_str().unwrap_or("");
      match self.get(|_, n| Some(allowed(n, name))) {
        Some(Some(yes)) => yes,
        Some(None) => unsafe { msg_send![super(self), isAccessibilitySelectorAllowed: sel] },
        None => false,
      }
    }

    #[unsafe(method(accessibilityPerformPress))]
    fn press(&self) -> bool {
      self.act(Action::Press)
    }

    #[unsafe(method(accessibilityPerformIncrement))]
    fn increment(&self) -> bool {
      self.act(Action::Increment)
    }

    #[unsafe(method(accessibilityPerformDecrement))]
    fn decrement(&self) -> bool {
      self.act(Action::Decrement)
    }

    #[unsafe(method(setAccessibilityFocused:))]
    fn set_focused(&self, on: bool) {
      if on {
        self.act(Action::Focus);
      }
    }

    #[unsafe(method(setAccessibilityValue:))]
    fn set_value(&self, v: Option<&AnyObject>) {
      if let Some(s) = v.and_then(|v| v.downcast_ref::<NSString>()) {
        self.act(Action::SetValue(s.to_string()));
      }
    }
  }
);

impl Element {
  fn new(mtm: MainThreadMarker, shared: Weak<Shared>, key: Key) -> Retained<Self> {
    let this = Self::alloc(mtm).set_ivars(Node { shared, key, dead: Cell::new(false) });
    unsafe { msg_send![super(this), init] }
  }

  /// Runs `f` on the snapshot and the node's index; `None` once the node is gone (or the
  /// snapshot is being replaced up the stack).
  fn with<R>(&self, f: impl FnOnce(&Rc<Shared>, &Snapshot, u32) -> Option<R>) -> Option<R> {
    let sh = self.ivars().shared.upgrade().filter(|_| !self.ivars().dead.get())?;
    let s = sh.snap.try_borrow().ok()?;
    let i = *s.index.get(&self.ivars().key)?;
    f(&sh, &s, i)
  }

  fn get<R>(&self, f: impl FnOnce(&Snapshot, &A11yNode) -> Option<R>) -> Option<R> {
    self.with(|_, s, i| f(s, s.node(i)))
  }

  fn up(&self) -> Option<Retained<AnyObject>> {
    let up = self.with(|sh, s, i| Some((sh.clone(), s.node(i).parent.map(|p| key(s.node(p))))));
    match up? {
      (sh, Some(k)) => sh.element(k),
      (sh, None) => sh.view.load()?.accessibilityParent(),
    }
  }

  fn top(&self) -> Option<Retained<AnyObject>> {
    self.with(|sh, _, _| Some(any(sh.view.load()?.window()?)))
  }

  fn has(&self, st: State) -> bool {
    self.get(|_, n| Some(n.props.has(st))).unwrap_or(false)
  }

  fn bound(&self, max: bool) -> Option<Retained<AnyObject>> {
    self.get(|_, n| match n.props.value {
      Value::Number { min, max: top, .. } => Some(num(if max { top } else { min })),
      _ => None,
    })
  }

  /// Sends `a` to the runtime through the view's queue if the node takes it; whether the
  /// runtime took it, or, queued while it is busy, that it was sent.
  fn act(&self, a: Action) -> bool {
    let Some(view) = self.with(|sh, s, i| s.node(i).can(&a).then(|| sh.view.load())?) else {
      return false;
    };

    let ((id, role), queued) = (self.ivars().key, view.busy());
    view.send(Event::A11y { id, role, action: a }) || queued
  }
}

/// The view's assistive state, made on the first query from the last frame (empty while the
/// runtime is busy up the stack: a frame once it is free fills it and tells the client).
fn shared(view: &View) -> Option<Rc<Shared>> {
  let cell = &view.ivars().a11y;
  if let Some(sh) = cell.try_borrow().ok()?.clone() {
    return Some(sh);
  }

  let snap = view.with_ui(Snapshot::new);
  if snap.is_none() {
    view.wake();
  }

  let snap = snap.unwrap_or_default();
  let (snap, elements) = (RefCell::new(snap), RefCell::default());
  let sh = Rc::new(Shared { view: ObjcWeak::new(view), snap, elements });
  *cell.try_borrow_mut().ok()? = Some(sh.clone());
  Some(sh)
}

/// The view's `accessibilityChildren`: the top-level nodes.
pub(super) fn children(view: &View) -> Option<Retained<NSArray>> {
  let sh = shared(view)?;
  let s = sh.snap.try_borrow().ok()?;
  let top = s.children(None).into_iter().map(|i| key(s.node(i)));
  Some(sh.array(top.collect::<Vec<_>>()))
}

/// The view's `accessibilityFocusedUIElement`.
pub(super) fn focused(view: &View) -> Option<Retained<AnyObject>> {
  let sh = shared(view)?;
  let k = sh.snap.try_borrow().ok().and_then(|s| Some(key(s.node(s.focused()?))))?;
  sh.element(k)
}

/// The view's `accessibilityHitTest:` at `p` on screen; `None` where no node is.
pub(super) fn hit_test(view: &View, p: NSPoint) -> Option<Retained<AnyObject>> {
  let sh = shared(view)?;
  let p = view.window().map_or(p, |w| w.convertPointFromScreen(p));
  let p = view.convertPoint_fromView(p, None);
  let at = Point::new(p.x as f32, p.y as f32);
  let k = sh.snap.try_borrow().ok().and_then(|s| Some(key(s.node(s.tree.at(at)?))))?;
  sh.element(k)
}

/// After a frame, once a client has asked: the new snapshot, and the notifications from the
/// last, posted once nothing is borrowed (a client may ask back at once). The removed nodes are
/// told first, while the old snapshot still answers for them: AppKit drops a notification whose
/// element no longer resolves (seen in the VM).
pub(super) fn sync(view: &View) {
  let Some(sh) = view.ivars().a11y.try_borrow().ok().and_then(|a| a.clone()) else { return };
  let Some(new) = view.with_ui(Snapshot::new) else { return };
  let Some(notes) = sh.snap.try_borrow().ok().map(|snap| diff(&snap, &new)) else { return };
  let gone = |&n: &Note| match n {
    Note::Destroyed(k) => Some((sh.elements.try_borrow_mut().ok()?.remove(&k)?, n)),
    _ => None,
  };
  let gone: Vec<_> = notes.iter().filter_map(gone).collect();
  for (el, note) in &gone {
    post(el, *note);
    el.ivars().dead.set(true);
  }

  let Ok(mut snap) = sh.snap.try_borrow_mut() else { return };
  *snap = new;
  drop(snap);
  let posts: Vec<_> = (notes.into_iter())
    .filter_map(|note| match note {
      Note::Layout => Some((any(view.window()?), note)),
      Note::Destroyed(_) => None,
      Note::Title(k) | Note::Selected(k) | Note::Value(k) | Note::Focus(k) | Note::Text(k) => {
        Some((sh.element(k)?, note))
      }
    })
    .collect();
  for (el, note) in posts {
    post(&el, note);
  }
}

/// The window closes: every element is gone, and the state with them.
pub(super) fn close(view: &View) {
  let Some(sh) = view.ivars().a11y.try_borrow_mut().ok().and_then(|mut a| a.take()) else { return };
  let all = sh.elements.try_borrow_mut().map(|mut m| std::mem::take(&mut *m));
  for (k, el) in all.unwrap_or_default() {
    post(&el, Note::Destroyed(k));
    el.ivars().dead.set(true);
  }
}

fn post(el: &AnyObject, note: Note) {
  // SAFETY: AppKit's notification names, posted on the main thread.
  unsafe {
    let name = match note {
      Note::Layout => NSAccessibilityLayoutChangedNotification,
      Note::Destroyed(_) => NSAccessibilityUIElementDestroyedNotification,
      Note::Title(_) => NSAccessibilityTitleChangedNotification,
      Note::Selected((_, Role::List)) => NSAccessibilitySelectedRowsChangedNotification,
      Note::Selected(_) => NSAccessibilitySelectedChildrenChangedNotification,
      Note::Value(_) => NSAccessibilityValueChangedNotification,
      Note::Focus(_) => NSAccessibilityFocusedUIElementChangedNotification,
      Note::Text(_) => NSAccessibilitySelectedTextChangedNotification,
    };

    NSAccessibilityPostNotification(el, name);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scene::Rect;
  use crate::ui::Props;

  const LIST: Key = (Id(1), Role::List);
  const ROW1: Key = (Id(2), Role::Row);
  const ROW2: Key = (Id(3), Role::Row);
  const SWITCH: Key = (Id(4), Role::Switch);
  const FIELD: Key = (Id(5), Role::TextField);
  const COUNT: Key = (Id(6), Role::StaticText);

  /// Adds node `k` named `name` inside `parent`, 10 points below the last.
  fn push(t: &mut A11yTree, parent: Option<u32>, k: Key, name: &str) -> u32 {
    let (i, start) = (t.nodes.len() as u32, t.text.len() as u32);
    t.text.push_str(name);
    let props = Props { role: k.1, name: (start, t.text.len() as u32), ..Props::default() };
    let rect = Rect::new(0.0, 10.0 * i as f32, 100.0, 10.0);
    t.nodes.push(A11yNode { id: k.0, props, rect, parent, end: i + 1, actions: 0, layer: 0 });
    let mut up = parent;
    while let Some(p) = up {
      t.nodes[p as usize].end = i + 1;
      up = t.nodes[p as usize].parent;
    }

    i
  }

  fn text(t: &mut A11yTree, i: u32, s: &str) {
    let start = t.text.len() as u32;
    t.text.push_str(s);
    t.nodes[i as usize].props.value = Value::Text((start, t.text.len() as u32));
  }

  fn set(t: &mut A11yTree, i: u32, s: State) {
    t.nodes[i as usize].props.states |= 1 << s as u8;
  }

  /// A list of two rows, the first selected; a switch, off; a focused field; a counter's text.
  fn window(f: impl FnOnce(&mut A11yTree)) -> A11yTree {
    let mut t = A11yTree::default();
    let list = push(&mut t, None, LIST, "");
    let row = push(&mut t, Some(list), ROW1, "One");
    set(&mut t, row, State::Selected);
    push(&mut t, Some(list), ROW2, "Two");
    let switch = push(&mut t, None, SWITCH, "Open at login");
    t.nodes[switch as usize].props.value = Value::Off;
    let field = push(&mut t, None, FIELD, "Name");
    text(&mut t, field, "hello");
    set(&mut t, field, State::Focused);
    push(&mut t, None, COUNT, "Count 1");
    f(&mut t);
    t
  }

  fn snap(t: A11yTree, sel: Range<usize>) -> Snapshot {
    Snapshot::of(t, Some((FIELD.0, sel, 0)))
  }

  fn at(t: &A11yTree, k: Key) -> usize {
    t.nodes.iter().position(|n| key(n) == k).unwrap()
  }

  #[test]
  fn roles_map_to_appkits() {
    let want = [
      (Role::Switch, ("AXCheckBox", Some("AXSwitch"))),
      (Role::SecureTextField, ("AXTextField", Some("AXSecureTextField"))),
      (Role::RadioGroup, ("AXRadioGroup", None)),
      (Role::Splitter, ("AXSplitter", None)),
      (Role::Tab, ("AXRadioButton", Some("AXTabButton"))),
      (Role::TabList, ("AXTabGroup", None)),
      (Role::PopUpButton, ("AXPopUpButton", None)),
      (Role::Dialog, ("AXGroup", Some("AXApplicationDialog"))),
      (Role::TextArea, ("AXTextArea", None)),
    ];
    for (role, ax) in want {
      assert_eq!(ax_role(role), ax, "{role:?}");
    }
  }

  /// The elements and the view register: in a debug build objc2 checks each method's types
  /// against the superclass's.
  #[test]
  fn the_classes_register() {
    use objc2::ClassType;

    assert_eq!(Element::class().name().to_str(), Ok("NimboAccessibilityElement"));
    assert_eq!(View::class().name().to_str(), Ok("NimboView"));
  }

  #[test]
  fn the_snapshot_nests_selects_and_finds() {
    let s = snap(window(|_| {}), 0..0);
    assert_eq!(s.children(None), [0, 3, 4, 5]);
    assert_eq!(s.children(Some(0)), [1, 2]);
    assert_eq!(s.selected(0), [ROW1]);
    assert_eq!(s.focused(), Some(4));
    assert_eq!(s.tree.at(Point::new(5.0, 25.0)), Some(2));
    assert_eq!(s.tree.at(Point::new(5.0, 95.0)), None);

    // The list focused with its cursor on a row: the row has the focus.
    let s = snap(
      window(|t| {
        t.nodes[4].props.states = 0;
        set(t, 0, State::Focused);
        t.nodes[0].props.active = Some(ROW2.0);
      }),
      0..0,
    );
    assert_eq!(s.focused(), Some(2));
  }

  /// Moved rects alone (an animation, a scroll within a node) tell nothing.
  #[test]
  fn a_frame_that_only_moves_tells_nothing() {
    let old = snap(window(|_| {}), 5..5);
    let new = snap(window(|t| t.nodes.iter_mut().for_each(|n| n.rect.y += 3.0)), 5..5);
    assert_eq!(diff(&old, &new), []);
  }

  #[test]
  fn changes_are_told_with_the_focused_value_last() {
    let old = snap(window(|_| {}), 5..5);
    let new = snap(
      window(|t| {
        let (r1, r2, sw, f, c) =
          (at(t, ROW1), at(t, ROW2), at(t, SWITCH), at(t, FIELD), at(t, COUNT));
        t.nodes[r1].props.states = 0;
        set(t, r2 as u32, State::Selected);
        t.nodes[r1].props.name = t.nodes[r2].props.name;
        t.nodes[sw].props.value = Value::On;
        text(t, f as u32, "hello!");
        let start = t.text.len() as u32;
        t.text.push_str("Count 2");
        t.nodes[c].props.name = (start, start + 7);
      }),
      6..6,
    );
    let want = [
      Note::Selected(LIST),
      Note::Title(ROW1),
      Note::Value(SWITCH),
      Note::Value(COUNT),
      Note::Text(FIELD),
      Note::Value(FIELD),
    ];
    assert_eq!(diff(&old, &new), want);
  }

  #[test]
  fn removed_nodes_are_destroyed_and_the_focus_moves() {
    let old = snap(window(|_| {}), 0..0);
    let mut t = A11yTree::default();
    let list = push(&mut t, None, LIST, "");
    push(&mut t, Some(list), ROW1, "One");
    let sw = push(&mut t, None, SWITCH, "Open at login");
    t.nodes[sw as usize].props.value = Value::Off;
    set(&mut t, sw, State::Focused);
    let new = Snapshot::of(t, None);
    let want = [
      Note::Layout,
      Note::Destroyed(ROW2),
      Note::Destroyed(FIELD),
      Note::Destroyed(COUNT),
      Note::Selected(LIST),
      Note::Focus(SWITCH),
    ];
    assert_eq!(diff(&old, &new), want);
  }

  #[test]
  fn selectors_follow_the_nodes_actions_and_role() {
    let t = window(|t| {
      t.nodes[3].actions = Action::Press.bit();
      t.nodes[4].actions = Action::SetValue(String::new()).bit() | Action::Focus.bit();
    });
    let (switch, field, row) = (&t.nodes[3], &t.nodes[4], &t.nodes[1]);
    assert_eq!(allowed(switch, "accessibilityPerformPress"), Some(true));
    assert_eq!(allowed(field, "accessibilityPerformPress"), Some(false));
    assert_eq!(allowed(switch, "accessibilityPerformIncrement"), Some(false));
    assert_eq!(allowed(field, "setAccessibilityValue:"), Some(true));
    assert_eq!(allowed(field, "setAccessibilityFocused:"), Some(true));
    assert_eq!(allowed(field, "accessibilitySelectedTextRange"), Some(true));
    assert_eq!(allowed(switch, "accessibilityStringForRange:"), Some(false));
    assert_eq!(allowed(switch, "accessibilityMinValue"), Some(false));
    assert_eq!(allowed(row, "isAccessibilitySelected"), Some(true));
    assert_eq!(allowed(&t.nodes[0], "accessibilitySelectedChildren"), Some(true));
    assert_eq!(allowed(row, "accessibilityRole"), None);
    assert_eq!(allowed(row, "setAccessibilitySelected:"), Some(false));
    assert_eq!(allowed(switch, "isAccessibilityExpanded"), Some(false));
    let mut t = t;
    set(&mut t, 3, State::Expanded);
    assert_eq!(allowed(&t.nodes[3], "isAccessibilityExpanded"), Some(true));
  }

  #[test]
  fn numbers_read_as_the_widgets_keep_them() {
    let n = |v: f32| num(v.into()).downcast::<NSNumber>().unwrap().doubleValue();
    assert_eq!(n(0.42), 0.42);
    assert_eq!(n(13.5), 13.5);
  }
}
