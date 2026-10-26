// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The accessibility model: what each element is to assistive technology (its role, name,
//! value and states), recorded by id while the view builds and assembled from the last laid-out
//! tree into the nodes a platform's adapter exposes.
//!
//! Every behaviour ([super::behavior]) records its control as it begins, with its role and
//! disabled state; widgets refine the record by the same id ([Ui::a11y]). A node's name is the
//! one [Ui::a11y_label] gave the next record, else the label of the form row around it
//! ([Ui::form_row], its caption the description), else, for roles titled by their text (a
//! button, a tab), the text inside it. A control with text of its own (a button's label) keeps
//! it over the form row's, whose label and caption are then read as they are; one whose text is
//! its value (a shortcut recorder) is not named by it. Leaf roles (a button, a field) hide the
//! text inside them; plain text anywhere else is static text, with no record.
//!
//! ```ignore
//! let b = ui.a11y_label("Delete").button_behavior(Id::of("delete"));
//! ui.a11y(b.id, Role::Button);
//! ```
//!
//! Records live beside the tree ([Tree::a11y]) and are cleared with it, so a frame built in
//! several passes has the last pass's, once. [UiState::a11y_tree] joins them to the last frame's
//! laid-out nodes when asked: each node nested under its nearest recorded ancestor (an overlay
//! under its own parent), its rect in window points clipped to the scroll areas around it, and
//! a node clipped away entirely left out; an open modal dialog leaves out what is behind it. A
//! leaf has no children (a record inside it, a recorder's ⊗, is beside it), what is inside a
//! disabled node is disabled, and a hit ([A11yTree::at]) finds the topmost overlay as a click
//! does.
//!
//! Assistive technology acts on a node by [crate::Event::A11y], which the runtime takes through
//! the input paths ([Action]), so a click, Space and VoiceOver's press run the same code. A node
//! takes the actions its role and states allow ([A11yNode::can]); others are refused.

use std::collections::HashMap;
use std::fmt::{self, Display};
use std::hash::BuildHasherDefault;
use std::iter::successors;

use super::{Focus, Id, Kind, Tree, Ui, UiState};
use crate::app::Event;
use crate::scene::{Point, Rect};
use crate::text::FxHasher;

/// A byte range of a string arena; empty when unset.
pub type Span = (u32, u32);

/// What an element is, as AppKit's roles (and subroles) name it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Role {
  #[default]
  Group,
  Button,
  CheckBox,
  /// A check box with the switch subrole.
  Switch,
  RadioButton,
  /// Radio buttons, one on: a segmented control.
  RadioGroup,
  Slider,
  TextField,
  /// A text field of many lines.
  TextArea,
  SecureTextField,
  StaticText,
  Link,
  PopUpButton,
  Menu,
  MenuItem,
  TabList,
  Tab,
  List,
  Row,
  ProgressIndicator,
  Dialog,
  /// A split's divider, its value the first pane's share.
  Splitter,
}

impl Role {
  /// Hides the text elements inside it: they are its name or its value, not nodes of their own.
  pub const fn leaf(self) -> bool {
    use Role::*;
    !matches!(self, Group | RadioGroup | Menu | TabList | List | Row | Dialog)
  }

  /// Named by the text inside it when nothing else names it.
  pub const fn titled(self) -> bool {
    use Role::*;
    matches!(
      self,
      Button | CheckBox | Switch | RadioButton | StaticText | Link | MenuItem | Tab | Row
    )
  }
}

/// A node's value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Value {
  #[default]
  None,
  /// A field's text, in the arena of the records or the tree that holds it.
  Text(Span),
  /// A slider's or a progress indicator's number, in its range.
  Number {
    now: f64,
    min: f64,
    max: f64,
  },
  /// A check box's, a switch's or a radio button's state: off, on, or mixed (a check box over
  /// some of its items).
  Off,
  On,
  Mixed,
}

/// A state a node has or not, a bit of [Props::states].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
  Disabled,
  Focused,
  Selected,
  Expanded,
  Modal,
}

const STATES: [State; 5] =
  [State::Disabled, State::Focused, State::Selected, State::Expanded, State::Modal];

/// What assistive technology asks of a node, sent as [crate::Event::A11y]; the runtime takes it
/// through the input paths, so it runs the code a click or a key runs.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Action {
  /// A left click at the node's centre, the pointer and the hover left where they are.
  Press,
  /// The Right arrow delivered to it: a slider's step up. A splitter has no keys, nor these.
  Increment,
  /// The Left arrow delivered to it.
  Decrement,
  /// Focus moved to it, its scroll areas showing it.
  Focus,
  /// A text field's new text, as one edit; a secure field takes it and still never shows it.
  SetValue(String),
  /// Its scroll areas showing it.
  ScrollToVisible,
}

impl Action {
  /// Its bit in [A11yNode::actions].
  pub fn bit(&self) -> u8 {
    use Action::*;
    1 << match self {
      Press => 0,
      Increment => 1,
      Decrement => 2,
      Focus => 3,
      SetValue(_) => 4,
      ScrollToVisible => 5,
    }
  }
}

/// The actions ([Action::bit]s) a node of `p` takes: enabled, a press on what a click acts on,
/// the steps on a slider, focus on an element that takes it (`focusable`), a new text on a
/// field; scrolling to it on any.
fn actions(p: &Props, focusable: bool) -> u8 {
  use Role::*;
  let press = matches!(
    p.role,
    Button | CheckBox | Switch | RadioButton | Link | PopUpButton | MenuItem | Tab | Row
  );
  let (step, set) = (p.role == Slider, matches!(p.role, TextField | TextArea | SecureTextField));
  let on = !p.has(State::Disabled);
  let can = [press, step, step, focusable, set];
  can.iter().zip(0..).fold(Action::ScrollToVisible.bit(), |b, (&c, k)| b | u8::from(c && on) << k)
}

/// What an element is to assistive technology; its strings are in the arena of the records or
/// the tree that holds it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Props {
  pub role: Role,
  pub name: Span,
  pub description: Span,
  pub value: Value,
  /// [State] bits.
  pub states: u8,
  /// The descendant with the cursor: a list's cursor row, a menu's highlighted item.
  pub active: Option<Id>,
}

impl Props {
  pub fn has(&self, s: State) -> bool {
    self.states & 1 << s as u8 != 0
  }
}

/// A frame's records, by id, as the view made them ([Ui::a11y]).
#[derive(Default)]
pub struct Records {
  list: Vec<(Id, Props)>,
  /// Each id's place in the list.
  index: HashMap<Id, usize, BuildHasherDefault<FxHasher>>,
  /// Their strings, back to back.
  text: String,
  /// The name for the next record ([Ui::a11y_label]).
  next: Option<Span>,
  /// The records that took a form row's label and caption, and those text elements.
  forms: Vec<(usize, [Id; 2])>,
  /// While the first has the focus, the second shows focused ([Ui::a11y_focus]).
  focus: Option<(Id, Id)>,
}

impl Records {
  pub(super) fn clear(&mut self) {
    self.list.clear();
    self.index.clear();
    self.text.clear();
    self.forms.clear();
    (self.next, self.focus) = (None, None);
  }
}

fn put(text: &mut String, s: &str) -> Span {
  let start = text.len() as u32;
  text.push_str(s);
  (start, text.len() as u32)
}

/// A record being written ([Ui::a11y]).
pub struct Access<'r> {
  recs: &'r mut Records,
  i: usize,
}

impl Access<'_> {
  fn props(&mut self) -> &mut Props {
    &mut self.recs.list[self.i].1
  }

  pub fn name(mut self, s: &str) -> Self {
    self.props().name = put(&mut self.recs.text, s);
    self
  }

  /// A name when nothing named it: an icon's ("Info"), a field's hint.
  pub fn name_or(mut self, s: &str) -> Self {
    let n = self.props().name;
    if n.0 == n.1 { self.name(s) } else { self }
  }

  /// What it does, read after the name and the value: a tooltip, a form row's caption.
  pub fn description(mut self, s: &str) -> Self {
    self.props().description = put(&mut self.recs.text, s);
    self
  }

  /// A field's text as its value.
  pub fn text(mut self, s: &str) -> Self {
    self.props().value = Value::Text(put(&mut self.recs.text, s));
    self
  }

  /// A number or a check state as its value ([Access::text] for text).
  pub fn value(mut self, v: Value) -> Self {
    self.props().value = v;
    self
  }

  pub fn state(mut self, s: State, on: bool) -> Self {
    let (p, bit) = (self.props(), 1 << s as u8);
    p.states = if on { p.states | bit } else { p.states & !bit };
    self
  }

  /// The descendant with the cursor ([Props::active]).
  pub fn active(mut self, id: Option<Id>) -> Self {
    self.props().active = id;
    self
  }
}

/// A form row's label and caption, for the first control recorded in it with no name, which
/// takes them from the slot ([Ui::form_names]).
pub(crate) struct FormNames {
  name: Span,
  description: Span,
  texts: [Id; 2],
}

impl Ui<'_> {
  /// Records `id` for assistive technology as `role`, or sets the role of its record this
  /// frame: a widget refining its behaviour's. A new record takes the name [Ui::a11y_label] set,
  /// else, unnamed, the form row's around it ([Ui::form_row]).
  pub fn a11y(&mut self, id: Id, role: Role) -> Access<'_> {
    let recs = &mut self.st.tree.a11y;
    let found = recs.index.get(&id).copied();
    let i = *recs.index.entry(id).or_insert(recs.list.len());
    recs.list.extend(found.is_none().then_some((id, Props::default())));

    let p = &mut recs.list[i].1;
    (p.role, p.name) = (role, recs.next.take().unwrap_or(p.name));
    let unnamed = found.is_none() && p.name.0 == p.name.1;
    if unnamed && let Some(f) = self.set_slot::<FormNames>(None) {
      let recs = &mut self.st.tree.a11y;
      (recs.list[i].1.name, recs.list[i].1.description) = (f.name, f.description);
      recs.forms.push((i, f.texts));
    }

    Access { recs: &mut self.st.tree.a11y, i }
  }

  /// The record of `id` this frame, to refine whatever its role: a tooltip's description, a
  /// menu's trigger expanded.
  pub fn a11y_of(&mut self, id: Id) -> Option<Access<'_>> {
    let recs = &mut self.st.tree.a11y;
    let i = *recs.index.get(&id)?;
    Some(Access { recs, i })
  }

  /// The text assistive technology sets on `id` ([Action::SetValue]), seen once: a text
  /// field's behaviour takes it as an edit.
  pub(super) fn a11y_value(&self, id: Id) -> Option<&str> {
    self.st.take(id, |e| match e {
      Event::A11y { action: Action::SetValue(s), .. } => Some(s.as_str()),
      _ => None,
    })
  }

  /// Names the next element recorded for assistive technology ([Ui::a11y]): what nothing else
  /// names, an icon button. Taken by the next record, as [Ui::disabled] by the next widget.
  pub fn a11y_label(&mut self, name: &str) -> &mut Self {
    let recs = &mut self.st.tree.a11y;
    recs.next = Some(put(&mut recs.text, name));
    self
  }

  /// Shows `shown` focused to assistive technology while `held`, which it doesn't read, has the
  /// focus: a modal dialog's default button while its backdrop holds the keys.
  pub fn a11y_focus(&mut self, held: Id, shown: Id) {
    self.st.tree.a11y.focus = Some((held, shown));
  }

  /// Records a form row a click selects (`id`) as a row named by its label and caption
  /// (`text`), which are then not read, as [Ui::form_names] gives them.
  pub(crate) fn a11y_row(&mut self, id: Id, text: [&str; 2]) -> Access<'_> {
    let outer = self.form_names(id, text);
    self.a11y(id, Role::Row);
    self.set_slot(outer);
    self.a11y_of(id).unwrap()
  }

  /// Gives a form row's label and caption (`text`) to the first control recorded with no name
  /// from now on, whose text elements (`row.key("label")`, `row.key("caption")`) are then not
  /// read; returns the names it replaces, for [Ui::set_slot] after the row's controls.
  pub(crate) fn form_names(&mut self, row: Id, text: [&str; 2]) -> Option<FormNames> {
    let recs = &mut self.st.tree.a11y;
    let (name, description) = (put(&mut recs.text, text[0]), put(&mut recs.text, text[1]));
    let texts = [row.key("label"), row.key("caption")];
    self.set_slot(Some(FormNames { name, description, texts }))
  }
}

/// A node of the assembled tree ([A11yTree]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct A11yNode {
  pub id: Id,
  pub props: Props,
  /// In window points, clipped to the scroll areas around it.
  pub rect: Rect,
  pub parent: Option<u32>,
  /// One past the last node inside it: the nodes are in pre-order.
  pub end: u32,
  /// The [Action]s it takes, as [Action::bit]s ([A11yNode::can]).
  pub actions: u8,
  /// The overlay it shows in, by z: 0 the window's tree, else one past its place in
  /// [Tree::floats] ([A11yTree::at]).
  pub layer: u16,
}

impl A11yNode {
  /// Whether it takes `a`: one it doesn't is refused.
  pub fn can(&self, a: &Action) -> bool {
    self.actions & a.bit() != 0
  }
}

/// The last frame as assistive technology sees it ([UiState::a11y_tree]). Its [Display] is one
/// line per node, indented by depth: `switch "Open at login" off`.
#[derive(Clone, Debug, Default)]
pub struct A11yTree {
  pub nodes: Vec<A11yNode>,
  /// The nodes' strings, back to back.
  pub text: String,
}

impl A11yTree {
  pub fn str(&self, s: Span) -> &str {
    &self.text[s.0 as usize..s.1 as usize]
  }

  /// The first node of `id`.
  pub fn get(&self, id: Id) -> Option<&A11yNode> {
    self.nodes.iter().find(|n| n.id == id)
  }

  /// The node at `p` in window points, as a click finds it: the deepest in the topmost overlay
  /// there (an open menu's item, not the row under it).
  pub fn at(&self, p: Point) -> Option<u32> {
    let on = self.nodes.iter().zip(0..).filter(|(n, _)| n.rect.contains(p));
    on.max_by_key(|&(n, i)| (n.layer, i)).map(|(_, i)| i)
  }

  /// Adds a node inside the `open` ones but the leaves, which have no children (a recorder's
  /// ⊗ is beside it), the last its parent, disabled with it; `at` is whether its element takes
  /// focus and its overlay ([A11yNode::layer]).
  fn push(&mut self, id: Id, mut props: Props, rect: Rect, at: (bool, u16), open: &[Open]) {
    let (up, end) = (open.iter().filter(|o| !o.2), self.nodes.len() as u32 + 1);
    let parent = up.clone().next_back().map(|o| o.1);
    let off = parent.is_some_and(|p| self.nodes[p as usize].props.has(State::Disabled));
    props.states |= u8::from(off) << State::Disabled as u8;
    let (actions, layer) = (actions(&props, at.0), at.1);
    self.nodes.push(A11yNode { id, props, rect, parent, end, actions, layer });
    for o in up {
      self.nodes[o.1 as usize].end = end;
    }
  }
}

/// A recorded node open around the one walked: its end in the tree, its index, whether it
/// hides text (a leaf).
type Open = (u32, u32, bool);

/// No record at a node, or a text element not read.
const NONE: u32 = u32::MAX;
const HIDDEN: u32 = u32::MAX - 1;

impl UiState {
  /// The last frame's nodes for assistive technology, assembled from its records and layout;
  /// see the module's docs.
  pub fn a11y_tree(&self) -> A11yTree {
    let (t, recs) = (&self.last, &self.last.a11y);
    // The record at each node, or not read.
    let mut at = vec![NONE; t.nodes.len()];
    for (i, k) in recs.list.iter().zip(0..).filter_map(|(r, k)| Some((t.find(r.0)?, k))) {
      at[i as usize] = k;
    }

    // A form row's label and caption name its control and are not read, unless it has text of
    // its own (a button's label, not counting them: a row a click selects holds them): then
    // they are read, and it is named by its text.
    let mut own = vec![false; recs.list.len()];
    for &(k, texts) in &recs.forms {
      let (id, p) = recs.list[k];
      let named = |i: u32| titled(&p) && !inner_text(t, &at, i as usize, &texts).is_empty();
      own[k] = t.find(id).is_some_and(named);
      for i in texts.iter().filter(|_| !own[k]).filter_map(|&id| t.find(id)) {
        if at[i as usize] == NONE {
          at[i as usize] = HIDDEN;
        }
      }
    }

    // The focus, or what shows focused while it is held ([Ui::a11y_focus]).
    let focus = self.focus().map(|f| recs.focus.filter(|h| h.0 == f).map_or(f, |h| h.1));

    // An open modal dialog (the last) leaves out what is behind it.
    let modal = recs.list.iter().rev().filter(|r| r.1.has(State::Modal)).find_map(|r| t.find(r.0));
    let inside =
      |i: usize| modal.is_none_or(|m| (m..t.nodes[m as usize].end).contains(&(i as u32)));

    // Where each node's children show, and the recorded nodes open around the one walked.
    let (mut out, mut clip) = (A11yTree::default(), vec![Rect::default(); t.nodes.len()]);
    let (mut open, mut layer): (Vec<Open>, _) = (Vec::new(), vec![0u16; t.nodes.len()]);
    for (i, n) in t.nodes.iter().enumerate() {
      while open.last().is_some_and(|o| o.0 as usize <= i) {
        open.pop();
      }

      // Hidden: a text element, with nothing inside.
      if at[i] == HIDDEN {
        continue;
      }

      // An overlay that doesn't keep its ancestors' clips has the window's.
      let keep = i > 0 && n.float.is_none_or(|f| f.clip);
      let outer = if keep { clip[n.parent as usize] } else { t.nodes[0].rect };
      clip[i] = n.style.clip_rect(n.rect).map_or(outer, |c| c.intersect(outer));
      let rect = n.rect.intersect(outer);
      let shown = rect.w > 0.0 && rect.h > 0.0 && inside(i);
      // The overlay it shows in, by z ([A11yNode::layer]).
      let z = n.float.and_then(|_| t.floats.iter().zip(1..).find(|f| *f.0 as usize == i));
      layer[i] = z.map_or(if i > 0 { layer[n.parent as usize] } else { 0 }, |f| f.1);
      if let Some(&(id, p)) = recs.list.get(at[i] as usize).filter(|_| shown) {
        let copy =
          |out: &mut A11yTree, s: Span| put(&mut out.text, &recs.text[s.0 as usize..s.1 as usize]);
        let (mut name, mut description) = (copy(&mut out, p.name), copy(&mut out, p.description));
        if own[at[i] as usize] {
          (name, description) = ((0, 0), (0, 0));
        }

        let mut props = Props { name, description, ..p };
        if let Value::Text(s) = p.value {
          props.value = Value::Text(copy(&mut out, s));
        }

        if name.0 == name.1 && titled(&p) {
          props.name = put(&mut out.text, &inner_text(t, &at, i, &[]));
        }

        props.states |= u8::from(focus == Some(id)) << State::Focused as u8;
        out.push(id, props, rect, (n.style.focus != Focus::None, layer[i]), &open);
        open.push((n.end, out.nodes.len() as u32 - 1, p.role.leaf()));
      }

      let s = if let Kind::Text { range, .. } = n.kind { t.str(range.0, range.1) } else { "" };
      if shown && !s.trim().is_empty() && !open.last().is_some_and(|o| o.2) {
        let name = put(&mut out.text, s.trim());
        let props = Props { role: Role::StaticText, name, ..Props::default() };
        out.push(n.id, props, rect, (false, layer[i]), &open);
      }
    }

    out
  }
}

/// Whether a record is named by the text inside it: a titled role whose text is not its value.
fn titled(p: &Props) -> bool {
  p.role.titled() && !matches!(p.value, Value::Text(_))
}

/// The text inside node `i` of `t`, the texts joined by spaces, leaving out the records inside
/// it, the hidden texts and those of ids `not`.
fn inner_text(t: &Tree, at: &[u32], i: usize, not: &[Id]) -> String {
  let (mut out, mut k) = (String::new(), i);
  while k < t.nodes[i].end as usize {
    let n = &t.nodes[k];
    let skip = k != i && (at[k] != NONE || not.contains(&n.id));
    if !skip && let Kind::Text { range, .. } = n.kind {
      let s = t.str(range.0, range.1).trim();
      if !out.is_empty() && !s.is_empty() {
        out.push(' ');
      }

      out.push_str(s);
    }

    k = if skip { n.end as usize } else { k + 1 };
  }

  out
}

/// A role's, a state's or a value's name in lowercase words: `CheckBox` as `check box`.
fn words(name: impl fmt::Debug) -> String {
  let mut s = String::new();
  for c in format!("{name:?}").chars() {
    if c.is_uppercase() && !s.is_empty() {
      s.push(' ');
    }

    s.push(c.to_ascii_lowercase());
  }

  s
}

impl Display for A11yTree {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    for n in &self.nodes {
      let (p, depth) = (&n.props, successors(n.parent, |&k| self.nodes[k as usize].parent).count());
      let quoted =
        |pre, s: Span| if s.0 == s.1 { String::new() } else { format!(" {pre}{:?}", self.str(s)) };
      write!(f, "{:w$}{}{}", "", words(p.role), quoted("", p.name), w = 2 * depth)?;

      match p.value {
        Value::None => {}
        Value::Text(s) => write!(f, " value {:?}", self.str(s))?,
        // In f32, as sliders keep them: 0.3, not 0.30000001192092896.
        Value::Number { now, min, max } => {
          write!(f, " {} in {}..{}", now as f32, min as f32, max as f32)?
        }
        v => write!(f, " {}", words(v))?,
      }

      let states: String =
        STATES.iter().filter(|&&s| p.has(s)).map(|s| format!(" {}", words(s))).collect();
      write!(f, "{states}{}", quoted("desc ", p.description))?;
      if let Some(a) = p.active.and_then(|a| self.get(a)) {
        write!(f, "{}", quoted("active ", a.props.name))?;
      }

      writeln!(f)?;
    }

    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::scene::{Point, Size};
  use crate::text::Font;
  use crate::ui::layout::tests::Fake;
  use crate::ui::{Float, FormRow, FormStyle, Style, TextStyle, px};

  /// 6 points per character, 10 per line.
  const T: TextStyle = TextStyle::new(Font::ui(10.0), [1.0; 4]);

  /// A frame of a 200 x 100 window built by `f`.
  fn frame(st: UiState, f: impl FnOnce(&mut Ui)) -> UiState {
    let mut ui = Ui::new(st, Size::new(200.0, 100.0), None, Duration::ZERO);
    f(&mut ui);
    ui.layout(&mut Fake, 1.0);
    ui.finish()
  }

  fn dump(f: impl FnOnce(&mut Ui)) -> String {
    frame(UiState::default(), f).a11y_tree().to_string()
  }

  /// An element `name` recorded as `role`, holding `text`.
  fn control(ui: &mut Ui, name: &str, role: Role, text: &str) {
    ui.a11y(Id::of(name), role);
    ui.id(name).row(Style::new(), |ui| ui.text(text, T));
  }

  #[test]
  fn records_nest_under_their_nearest_recorded_ancestor() {
    let s = dump(|ui| {
      ui.a11y(Id::of("pane"), Role::Group).name("Pane");
      ui.id("pane").col(Style::new(), |ui| {
        ui.text("Title", T);
        // A container with no record adds no level.
        ui.row(Style::new(), |ui| control(ui, "ok", Role::Button, "OK"));
        ui.a11y(Id::of("menu"), Role::Menu);
        ui.id("menu").float(Float::parent()).col(Style::new(), |ui| {
          control(ui, "copy", Role::MenuItem, "Copy");
        });
      });
      ui.text("Footer", T);
    });
    let want = "group \"Pane\"\n  static text \"Title\"\n  button \"OK\"\n  menu\n    menu item \
                \"Copy\"\nstatic text \"Footer\"\n";
    assert_eq!(s, want);
  }

  /// A titled leaf is named by its texts, which it hides; a field hides its text unnamed; a
  /// group shows its text as static text.
  #[test]
  fn leaves_take_their_name_from_the_text_inside_and_hide_it() {
    let s = dump(|ui| {
      ui.a11y(Id::of("save"), Role::Button);
      ui.id("save").row(Style::new().gap(4.0), |ui| {
        ui.text("Save", T);
        ui.text("all", T);
      });
      control(ui, "field", Role::TextField, "draft");
      control(ui, "group", Role::Group, "Inside");
      ui.text(" ", T);
    });
    assert_eq!(s, "button \"Save all\"\ntext field\ngroup\n  static text \"Inside\"\n");
  }

  #[test]
  fn a_set_name_wins_over_the_text_inside() {
    let s = dump(|ui| {
      ui.a11y_label("Delete");
      control(ui, "del", Role::Button, "⌫");
      control(ui, "next", Role::Button, "Next");
    });
    // Taken by the first record only.
    assert_eq!(s, "button \"Delete\"\nbutton \"Next\"\n");
  }

  #[test]
  fn a_behaviour_records_its_disabled_state_and_focus_shows() {
    let mut st = UiState::default();
    st.set_focus(Some(Id::of("b")));
    let st = frame(st, |ui| {
      for (name, off) in [("a", true), ("b", false)] {
        let b = ui.disabled(off).button_behavior(Id::of(name));
        ui.a11y(b.id, Role::Button);
        ui.with_id(b.id).row(b.style(Style::new()), |ui| ui.text(name, T));
      }
    });
    assert_eq!(st.a11y_tree().to_string(), "button \"a\" disabled\nbutton \"b\" focused\n");
  }

  /// A form row's label and caption name its first unnamed control and aren't read again; a
  /// control named on its own leaves them to the next.
  #[test]
  fn a_form_row_names_its_first_unnamed_control() {
    let s = dump(|ui| {
      let fs = FormStyle::plates(&ui.theme());
      let row = FormRow::new("Open at login").caption("Starts with the system");
      ui.form_row(&fs, row, |ui| {
        ui.a11y_label("Help");
        control(ui, "help", Role::Button, "?");
        ui.a11y(Id::of("login"), Role::Switch).value(Value::Off);
        ui.id("login").el(Style::new().w(px(30.0)).h(px(16.0)));
        control(ui, "more", Role::Button, "More");
      });
      ui.form_row(&fs, FormRow::new("Plain"), |ui| ui.text("Note", T));
    });
    let want = "button \"Help\"\nswitch \"Open at login\" off desc \"Starts with the \
                system\"\nbutton \"More\"\nstatic text \"Plain\"\nstatic text \"Note\"\n";
    assert_eq!(s, want);
  }

  /// A button keeps its own text, and the row's label is read; a text that is the value (a
  /// shortcut recorder's) names nothing.
  #[test]
  fn a_control_with_text_of_its_own_keeps_it_in_a_form_row() {
    let s = dump(|ui| {
      let fs = FormStyle::plates(&ui.theme());
      ui.form_row(&fs, FormRow::new("Account"), |ui| control(ui, "out", Role::Button, "Out"));
      ui.form_row(&fs, FormRow::new("Hotkey"), |ui| {
        ui.a11y(Id::of("key"), Role::Button).text("F5");
        ui.id("key").row(Style::new(), |ui| ui.text("F5", T));
      });
    });
    assert_eq!(s, "static text \"Account\"\nbutton \"Out\"\nbutton \"Hotkey\" value \"F5\"\n");
  }

  /// Rows of 20 in a scroll area of 30 scrolled by 15: the first cut at the top, the third at
  /// the bottom, the last two left out.
  #[test]
  fn nodes_are_clipped_to_their_scroll_areas() {
    let area = Id::of("area");
    let st = frame(UiState::default(), |ui| {
      ui.scroll_to(area, Point::new(0.0, 15.0));
      ui.with_id(area).col(Style::new().w(px(100.0)).h(px(30.0)).scroll_y(), |ui| {
        for i in 0..5 {
          ui.a11y(area.key(i), Role::Row);
          ui.with_id(area.key(i)).el(Style::new().w(px(100.0)).h(px(20.0)));
        }
      });
    });
    let t = st.a11y_tree();
    let rects: Vec<_> = t.nodes.iter().map(|n| (n.id, n.rect)).collect();
    let row = |i: i32, y: f32, h: f32| (area.key(i), Rect::new(0.0, y, 100.0, h));
    assert_eq!(rects, [row(0, 0.0, 5.0), row(1, 5.0, 20.0), row(2, 25.0, 5.0)]);
  }

  /// A pass laid out and run again on its layout, then one only delivering input: each record
  /// once, and a name set in a pass dropped and not taken by the next.
  #[test]
  fn records_survive_multi_pass_frames_once() {
    let view = |ui: &mut Ui| {
      control(ui, "a", Role::Button, "A");
      ui.a11y(Id::of("a"), Role::CheckBox).value(Value::On);
      control(ui, "b", Role::Tab, "B");
    };
    let mut ui = Ui::new(UiState::default(), Size::new(200.0, 100.0), None, Duration::ZERO);
    view(&mut ui);
    ui.a11y_label("Stale");
    ui.layout(&mut Fake, 1.0);
    let st = frame(ui.relayout(), view);
    let mut ui = Ui::new(st, Size::new(200.0, 100.0), None, Duration::ZERO);
    view(&mut ui);
    let st = ui.discard();
    assert_eq!(st.a11y_tree().to_string(), "check box \"A\" on\ntab \"B\"\n");
  }

  #[test]
  fn parents_and_ends_follow_the_nesting() {
    let st = frame(UiState::default(), |ui| {
      ui.a11y(Id::of("list"), Role::List);
      ui.id("list").col(Style::new(), |ui| {
        control(ui, "r0", Role::Row, "One");
        control(ui, "r1", Role::Row, "Two");
      });
    });
    let t = st.a11y_tree();
    let shape: Vec<_> = t.nodes.iter().map(|n| (n.props.role, n.parent, n.end)).collect();
    let (list, row, text) = (Role::List, Role::Row, Role::StaticText);
    let want = [
      (list, None, 5),
      (row, Some(0), 3),
      (text, Some(1), 3),
      (row, Some(0), 5),
      (text, Some(3), 5),
    ];
    assert_eq!(shape, want);
  }

  /// An open menu over the rows under it, built inside the first: a hit there finds its item,
  /// not the row it covers.
  #[test]
  fn a_hit_finds_the_overlay_on_top() {
    let row = Style::new().w(px(100.0)).h(px(20.0));
    let st = frame(UiState::default(), |ui| {
      ui.a11y(Id::of("r0"), Role::Row);
      ui.id("r0").col(row, |ui| {
        ui.a11y(Id::of("menu"), Role::Menu);
        ui.id("menu").float(Float::parent().offset(0.0, 20.0)).col(Style::new(), |ui| {
          control(ui, "go", Role::MenuItem, "Go");
        });
      });
      ui.a11y(Id::of("r1"), Role::Row);
      ui.id("r1").el(row);
    });
    let t = st.a11y_tree();
    let at = |x, y| t.at(Point::new(x, y)).map(|i| t.nodes[i as usize].id);
    assert_eq!(at(2.0, 25.0), Some(Id::of("go")));
    assert_eq!(at(50.0, 25.0), Some(Id::of("r1")));
    assert_eq!(at(50.0, 5.0), Some(Id::of("r0")));
  }

  /// A record inside a leaf (a recorder's ⊗) is beside it; what is inside a disabled node is
  /// disabled, and takes no press.
  #[test]
  fn leaves_have_no_children_and_disabled_holds_inside() {
    let s = dump(|ui| {
      ui.a11y(Id::of("rec"), Role::Button).text("F5");
      ui.id("rec").row(Style::new(), |ui| {
        ui.text("F5", T);
        ui.a11y(Id::of("clear"), Role::Button).name("Clear");
        ui.id("clear").el(Style::new().w(px(10.0)).h(px(10.0)));
      });
      ui.a11y(Id::of("list"), Role::List).state(State::Disabled, true);
      ui.id("list").col(Style::new(), |ui| control(ui, "row", Role::Row, "One"));
    });
    let want = "button value \"F5\"\nbutton \"Clear\"\nlist disabled\n  row \"One\" \
                disabled\n    static text \"One\" disabled\n";
    assert_eq!(s, want);
  }

  /// Records found by id, not by a scan: 20,000 of them, each refined twice, well within the
  /// time a scan of the list per record takes (seconds).
  #[test]
  fn records_scale_with_their_count() {
    let mut ui = Ui::new(UiState::default(), Size::new(200.0, 100.0), None, Duration::ZERO);
    let start = std::time::Instant::now();
    for i in 0..20_000 {
      ui.a11y(Id::of("b").key(i), Role::Button);
      ui.a11y(Id::of("b").key(i), Role::CheckBox).value(Value::On);
      ui.a11y_of(Id::of("b").key(i)).unwrap().name("B");
    }

    let took = start.elapsed();
    assert_eq!(ui.st.tree.a11y.list.len(), 20_000);
    assert!(took < Duration::from_millis(400), "{took:?}");
  }

  /// A disabled list's rows and a disabled segmented control's segments read disabled and take
  /// no press.
  #[test]
  fn a_disabled_containers_items_are_disabled() {
    let st = frame(UiState::default(), |ui| {
      ui.disabled_scope(true, |ui| {
        ui.id("seg").segmented(&mut 0, &[(0, "A"), (1, "B")]);
        ui.list(Id::of("list"), 1, 20.0, |ui, _| ui.text("One", T));
      });
    });
    let t = st.a11y_tree();
    let items = t.nodes.iter().filter(|n| matches!(n.props.role, Role::RadioButton | Role::Row));
    let off: Vec<_> =
      items.map(|n| (n.props.has(State::Disabled), n.can(&Action::Press))).collect();
    assert_eq!(off, [(true, false); 3]);
  }
}
