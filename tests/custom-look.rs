// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Tests of `examples/custom-look.rs` through the harness: the git client paints from its own
//! palette in dark and light, its diff tints lines and their changed words; its made-up history
//! has merges and lanes; and the behaviours under the look work as under nimbo's: a click or the
//! arrows pick a commit and show its files and diff, a file shows its part alone, the search
//! filters, sections fold, a branch or a parent leads to its commit, the divider drags, the
//! pointing hand and a hover over what clicks, the tabs and the toolbar's actions.

#[path = "../examples/custom-look.rs"]
#[expect(dead_code, reason = "its main and what this file doesn't use")]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[cfg_attr(windows, expect(unused_attributes, reason = "its windows_subsystem"))]
mod custom_look;

mod common;

use std::time::{Duration, Instant};

use common::{diff, mid, px, reads};
use custom_look::*;
use nimbo::a11y::Role;
use nimbo::harness::Image;
use nimbo::{
  Appearance, Cursor, Event, Font, Harness, Id, Modifiers, NamedKey, Phase, Point, Rect, Size,
};

type H = Harness<Rill>;

fn rill(look: Appearance) -> H {
  let mut h = Harness::new(Rill::default(), Size::new(1240.0, 760.0), 1.0);
  h.event(Event::Appearance(look));
  h.frame();
  h.frame();
  h
}

fn rect(h: &H, id: Id) -> Rect {
  h.rect(id).unwrap_or_else(|| panic!("no {id:?}"))
}

/// The history's row of commit `c`.
fn row(c: usize) -> Id {
  Id::of(COMMITS).key(("row", c as u64))
}

/// Line `i` of the diff of commit `c`, `file` the one shown alone.
fn line(c: usize, file: Option<usize>, i: usize) -> Id {
  Id::of(LINES).key((c, file, i))
}

fn click(h: &mut H, id: Id) {
  let r = rect(h, id);
  h.click(r.x + r.w.min(60.0) / 2.0, r.y + r.h / 2.0);
  h.frame();
}

#[test]
fn the_palette_paints_dark_and_light() {
  for (look, p) in [(Appearance::DARK, Palette::DARK), (Appearance::LIGHT, Palette::LIGHT)] {
    let mut h = rill(look);
    h.move_to(1.0, 700.0);
    h.wait(Duration::from_millis(300));
    let img = h.render();
    assert_eq!(img.pixel(600.0, 4.0), px(p.chrome), "{look:?}: the tabs' strip");
    assert_eq!(img.pixel(300.0, 40.0), px(p.bar), "the toolbar");

    let side = rect(&h, Id::of(SIDEBAR));
    assert_eq!(img.pixel(side.x + 4.0, side.y + side.h - 4.0), px(p.side), "the sidebar");
    let list = rect(&h, Id::of(COMMITS));
    let ground = img.pixel(list.x + list.w / 2.0, list.y + ROW * 20.5 - 1.0);
    assert_eq!(ground, px(p.list), "the history");

    let field = rect(&h, Id::of(SEARCH));
    assert_eq!(img.pixel(field.x + field.w - 6.0, field.y + 12.0), px(p.raised), "the search");
    let tip = rect(&h, row(0));
    let fill = img.pixel(tip.x + tip.w - 2.0, tip.y + 2.0);
    assert!(diff(fill, px(p.list)) > 20, "the selected row: {fill:?}");
  }
}

#[test]
fn the_history_has_merges_and_several_lanes() {
  let app = Rill::default();
  let repo = app.repo();
  assert_eq!(repo.commits.len(), 320);
  let merges = repo.commits.iter().filter(|c| c.parents.len() > 1).count();
  assert!(merges >= 20, "{merges} merges");
  assert!(repo.lanes >= 4, "{} lanes", repo.lanes);
  assert!(repo.commits.iter().enumerate().all(|(i, c)| c.parents.iter().all(|&p| p > i)));

  // The newest commit's dot on its lane, main's colour.
  let h = &mut rill(Appearance::DARK);
  let r = rect(h, row(0));
  let lane = repo.graph[0].lane as f32;
  let dot = h.render().pixel(r.x + 4.0 + 12.0 * (lane + 0.5), r.y + r.h / 2.0);
  assert_eq!(dot, px(Palette::DARK.lanes[repo.graph[0].color as usize]));
}

#[test]
fn a_click_on_a_commit_shows_its_files_and_diff() {
  let mut h = rill(Appearance::DARK);
  let c = 5;
  click(&mut h, row(c));
  assert_eq!(h.app().selected(), Some(c));
  let commit = h.app().repo().commits[c].clone();
  assert_eq!(h.text(Id::of(SUBJECT)), Some(commit.subject.as_str()));
  let d = &h.app().diffs[commit.diff];
  for k in 0..d.files.len() {
    assert!(h.rect(Id::of(FILES).key(("row", k as u64))).is_some(), "file {k}");
  }

  assert!(h.rect(line(c, None, 0)).is_some(), "the first file's header");
  assert!(h.rect(line(0, None, 0)).is_none(), "the last commit's diff is gone");
}

#[test]
fn the_arrows_move_through_the_history() {
  let mut h = rill(Appearance::DARK);
  click(&mut h, row(2));
  for (key, at) in [(NamedKey::ArrowDown, 3), (NamedKey::ArrowDown, 4), (NamedKey::ArrowUp, 3)] {
    h.key(key, Modifiers::empty());
    h.frame();
    assert_eq!(h.app().selected(), Some(at), "{key:?}");
  }

  let subject = h.app().repo().commits[3].subject.clone();
  assert_eq!(h.text(Id::of(SUBJECT)), Some(subject.as_str()));
}

#[test]
fn the_search_filters_the_history_and_escape_clears_it() {
  let mut h = rill(Appearance::DARK);
  let all = h.app().shown.len();
  h.click_id(Id::of(SEARCH));
  h.type_text("merge");
  h.frame();
  let app = h.app();
  assert!(app.shown.len() > 10 && app.shown.len() < all / 2, "{} of {all}", app.shown.len());
  assert!(app.shown.iter().all(|&c| app.repo().commits[c].subject.starts_with("Merge")));
  let first = app.shown[0];
  assert!(h.rect(row(first)).is_some(), "the first found shows first");

  h.key(NamedKey::Escape, Modifiers::empty());
  h.frame();
  assert_eq!((h.app().query.as_str(), h.app().shown.len()), ("", all));

  // By author, any case.
  h.type_text("yuki");
  h.frame();
  let app = h.app();
  assert!(app.shown.iter().all(|&c| AUTHORS[app.repo().commits[c].author].0 == "Yuki Tanaka"));
}

/// A search that hides the commit selected picks the first it finds and shows it; cleared, the
/// commit from before comes back, unless another was picked while searching.
#[test]
fn the_search_keeps_a_commit_selected_and_gives_it_back() {
  let mut h = rill(Appearance::DARK);
  click(&mut h, row(2));
  let subject = |h: &H| h.text(Id::of(SUBJECT)).map(str::to_owned);
  h.click_id(Id::of(SEARCH));
  h.type_text("merge");
  h.frame();
  let first = h.app().shown[0];
  assert_eq!(h.app().selected(), Some(first));
  assert_eq!(subject(&h).as_deref(), Some(h.app().repo().commits[first].subject.as_str()));

  // Nothing found: nothing selected, and the pane says so.
  h.type_text("zzz");
  h.frame();
  assert_eq!((h.app().shown.len(), h.app().selected()), (0, None));
  assert_eq!(subject(&h), None);

  h.key(NamedKey::Escape, Modifiers::empty());
  h.frame();
  assert_eq!(h.app().selected(), Some(2), "the commit from before");
  assert!(h.rect(row(2)).is_some());

  // A commit picked while searching stays picked.
  h.type_text("merge");
  h.frame();
  let third = h.app().shown[2];
  click(&mut h, row(third));
  h.click_id(Id::of(SEARCH));
  h.key(NamedKey::Escape, Modifiers::empty());
  h.frame();
  assert_eq!((h.app().query.as_str(), h.app().selected()), ("", Some(third)));
  assert!(h.rect(row(third)).is_some(), "scrolled to");
}

/// The graph's column keeps its width while filtered: the messages stay where they were.
#[test]
fn a_search_keeps_the_messages_in_place() {
  let mut h = rill(Appearance::DARK);
  let c = h.app().repo().commits.iter().position(|c| c.subject.starts_with("Merge")).unwrap();
  let subject = h.app().repo().commits[c].subject.clone();
  let at = |h: &H| h.find(Role::StaticText, &subject).map(|e| e.rect.x);
  h.app_mut().select(c);
  h.frame();
  h.frame();
  let was = at(&h);
  assert!(was.is_some());

  h.click_id(Id::of(SEARCH));
  h.type_text("merge");
  h.frame();
  assert_eq!(at(&h), was);
}

/// On a selected row, focused or not, and on a hovered one, every text reads: its pixels stand
/// well off the row's fill.
#[test]
fn text_on_a_selected_or_hovered_row_reads() {
  for look in [Appearance::DARK, Appearance::LIGHT] {
    let mut h = rill(look);
    let merge = h.app().repo().commits.iter().position(|c| c.parents.len() > 1).unwrap();
    h.app_mut().select(merge);
    h.frame();
    h.frame();
    let r = rect(&h, row(merge));
    let hover = rect(&h, row(merge + 1));
    h.move_to(hover.x + 300.0, hover.y + 10.0);
    h.wait(Duration::from_millis(300));

    for (what, r) in [("selected", r), ("hovered", hover)] {
      let img = h.render();
      let fill = img.pixel(r.x + r.w - 2.0, r.y + 2.0);
      // The subject's, the author's and the date's columns, from the right.
      for (part, x0, x1) in [
        ("date", r.x + r.w - 84.0, r.x + r.w - 10.0),
        ("author", r.x + r.w - 202.0, r.x + r.w - 84.0),
      ] {
        let most = (x0 as usize..x1 as usize)
          .flat_map(|x| (0..r.h as usize).map(move |y| (x as f32, r.y + y as f32)))
          .map(|(x, y)| diff(img.pixel(x, y), fill))
          .max()
          .unwrap();
        assert!(most > 90, "{look:?} {what} {part}: {most}");
      }
    }
  }
}

#[test]
fn cmd_f_goes_to_the_search() {
  let mut h = rill(Appearance::DARK);
  h.key('f', nimbo::Os::CURRENT.cmd());
  h.frame();
  assert_eq!(h.focus_id(), Some(Id::of(SEARCH)));
}

#[test]
fn a_file_shows_its_diff_alone_until_escape() {
  let mut h = rill(Appearance::DARK);
  let files = h.app().diffs[h.app().repo().commits[0].diff].files.len();
  assert!(files >= 2);
  click(&mut h, Id::of(FILES).key(("row", 1u64)));
  assert_eq!(h.app().file(), Some(1));
  assert!(h.rect(line(0, Some(1), 0)).is_some(), "the file's header first");
  assert!(h.rect(line(0, None, 0)).is_none());

  h.key(NamedKey::Escape, Modifiers::empty());
  h.frame();
  assert_eq!(h.app().file(), None);
  assert!(h.rect(line(0, None, 0)).is_some(), "all files again");

  // Another commit shows all of its own.
  click(&mut h, Id::of(FILES).key(("row", 1u64)));
  click(&mut h, row(1));
  assert_eq!(h.app().file(), None);
}

/// The pixels of a run of text in one colour along `y`, from `x0` to `x1`.
fn count(img: &Image, (x0, x1): (f32, f32), y: f32, color: [u8; 4]) -> usize {
  (x0 as usize..x1 as usize).filter(|&x| img.pixel(x as f32, y) == color).count()
}

#[test]
fn the_diff_tints_its_lines_and_the_words_they_changed() {
  let mut h = rill(Appearance::DARK);
  let p = Palette::DARK;
  let d = h.app().diffs[h.app().repo().commits[0].diff].clone();
  let del = d.lines.iter().position(|l| l.kind == LineKind::Del && l.word.is_some()).unwrap();
  let add = del + 1 + d.lines[del + 1..].iter().position(|l| l.kind == LineKind::Add).unwrap();
  let same = d.lines.iter().position(|l| l.kind == LineKind::Same && !l.text.is_empty()).unwrap();
  let img = h.render();

  for (i, ground, word) in
    [(del, p.removed, p.removed_word), (add, p.added, p.added_word), (same, p.view, p.view)]
  {
    let r = rect(&h, line(0, None, i));
    let l = &d.lines[i];
    // Right of its text: the line's ground, all the way.
    assert_eq!(img.pixel(r.x + r.w - 3.0, r.y + r.h / 2.0), px(ground), "{:?}", l.kind);
    let Some((a, b)) = l.word else { continue };

    // The changed word on its stronger ground: between its glyphs and above them.
    let mono = Font::mono(12.0);
    let x = r.x + 92.0;
    let (wa, wb) = (h.measure(&l.text[..a as usize], mono), h.measure(&l.text[..b as usize], mono));
    let y = r.y + 3.0;
    let n = count(&img, (x + wa + 1.0, x + wb - 1.0), y, px(word));
    assert!(n as f32 >= (wb - wa) * 0.8, "{:?}: {n} of {}", l.kind, wb - wa);
    assert_eq!(count(&img, (x + wa + 1.0, x + wb - 1.0), y, px(ground)), 0);
  }
}

#[test]
fn a_section_folds_and_opens() {
  let mut h = rill(Appearance::DARK);
  let tag = Id::of(REF).key((2usize, 0usize));
  assert!(h.rect(tag).is_some());
  click(&mut h, Id::of(SECTION).key(2usize));
  assert!(!h.app().open[2] && h.rect(tag).is_none(), "folded");
  click(&mut h, Id::of(SECTION).key(2usize));
  assert!(h.app().open[2] && h.rect(tag).is_some(), "open");

  // Stashes start folded.
  assert!(h.rect(Id::of(REF).key((3usize, 0usize))).is_none());
  click(&mut h, Id::of(SECTION).key(3usize));
  assert!(h.rect(Id::of(REF).key((3usize, 0usize))).is_some());
}

#[test]
fn a_branch_or_a_tag_selects_its_commit_and_shows_it() {
  let mut h = rill(Appearance::DARK);
  for (k, i) in [(0usize, 1usize), (2, 5)] {
    let c = h.app().repo().refs[k][i].commit;
    click(&mut h, Id::of(REF).key((k, i)));
    h.frame();
    assert_eq!(h.app().selected(), Some(c));
    let (r, list) = (rect(&h, row(c)), rect(&h, Id::of(COMMITS)));
    assert!(r.y >= list.y - 0.5 && r.y + r.h <= list.y + list.h + 0.5, "{r:?} in {list:?}");
  }
}

#[test]
fn a_parent_link_goes_to_the_parent() {
  let mut h = rill(Appearance::DARK);
  let merge = h.app().repo().commits.iter().position(|c| c.parents.len() > 1).unwrap();
  h.app_mut().select(merge);
  h.frame();
  h.frame();
  let second = h.app().repo().commits[merge].parents[1];
  click(&mut h, Id::of(PARENT).key(1usize));
  h.frame();
  assert_eq!(h.app().selected(), Some(second));
  assert!(h.rect(row(second)).is_some(), "scrolled to");
}

#[test]
fn the_dividers_drag() {
  let mut h = rill(Appearance::DARK);
  let ratio = |a: &Rill, name: &str| if name == SPLIT { a.split } else { a.side };
  for name in [SPLIT, SIDE] {
    let was = ratio(h.app(), name);
    let (x, y) = mid(rect(&h, Id::of(name).key("handle")));
    h.move_to(x, y);
    h.frame();
    assert_eq!(h.cursor(), Cursor::ResizeX, "{name}");

    h.press(x, y);
    h.frame();
    h.move_to(x + 62.0, y);
    h.frame();
    h.release(x + 62.0, y);
    h.frame();
    let now = ratio(h.app(), name);
    assert!((now - was - 62.0 / 1240.0).abs() < 0.03, "{name}: {was} to {now}");
  }
}

#[test]
fn what_clicks_shows_the_hand_and_a_hover() {
  let mut h = rill(Appearance::DARK);
  let ids = [
    ("a tab", Id::of(TABS).key(("item", 1usize))),
    ("the branch", Id::of(BRANCH_BUTTON)),
    ("fetch", Id::of(FETCH_BUTTON)),
    ("a section", Id::of(SECTION).key(0usize)),
    ("a branch", Id::of(REF).key((0usize, 1usize))),
    ("a commit", row(3)),
    ("a file", Id::of(FILES).key(("row", 0u64))),
    ("a parent", Id::of(PARENT).key(0usize)),
  ];
  for (what, id) in ids {
    let r = rect(&h, id);
    h.move_to(1.0, 700.0);
    h.wait(Duration::from_millis(300));
    let (x, y) = (r.x + r.w - 3.0, r.y + 3.0);
    let rest = h.render().pixel(x, y);

    h.move_to(r.x + r.w / 2.0, r.y + r.h / 2.0);
    h.frame();
    assert_eq!(h.cursor(), Cursor::Pointer, "{what}");
    // Fading in: part way at 60 ms, all of it by 300.
    h.wait(Duration::from_millis(60));
    let part = h.render().pixel(x, y);
    h.wait(Duration::from_millis(300));
    let full = h.render().pixel(x, y);
    if what != "a parent" {
      assert!(
        diff(rest, full) > diff(rest, part) && diff(rest, part) > 0,
        "{what}: {rest:?} {part:?} {full:?}"
      );
    }
  }

  let field = mid(rect(&h, Id::of(SEARCH)));
  h.move_to(field.0, field.1);
  h.frame();
  assert_eq!(h.cursor(), Cursor::Text, "the search's I-beam");
}

#[test]
fn a_tooltip_is_a_slip_of_the_look() {
  let mut h = rill(Appearance::DARK);
  let (x, y) = mid(rect(&h, Id::of(FETCH_BUTTON)));
  h.move_to(x, y);
  h.wait(Duration::from_millis(1500));
  let tip = h.rect(Id::of(FETCH_BUTTON).key("tip")).expect("the tip shows");
  assert_eq!(h.render().pixel(tip.x + 3.0, tip.y + tip.h / 2.0), px(Palette::DARK.raised));
}

#[test]
fn push_sends_the_commits_ahead_then_waits() {
  let mut h = rill(Appearance::DARK);
  assert_eq!(h.app().repo().ahead(), 2);
  h.click_id(Id::of(PUSH_BUTTON));
  h.frame();
  assert_eq!(
    (h.app().repo().ahead(), h.app().status.as_str()),
    (0, "Pushed 2 commits to origin/main")
  );
  reads(&h.a11y(), &["button \"Push\" disabled desc \"Push main to origin\""]);
  h.click_id(Id::of(FETCH_BUTTON));
  assert_eq!(h.app().status, "Fetched origin: nothing new");
}

#[test]
fn a_tab_opens_the_other_repository() {
  let mut h = rill(Appearance::DARK);
  h.click_id(Id::of(SEARCH));
  h.type_text("merge");
  click(&mut h, Id::of(TABS).key(("item", 1usize)));
  let app = h.app();
  assert_eq!((app.repo, app.repo().name, app.query.as_str()), (1, "quill-plugins", ""));
  assert_eq!((app.shown.len(), app.selected()), (140, Some(0)));
  let subject = app.repo().commits[0].subject.clone();
  assert_eq!(h.text(Id::of(SUBJECT)), Some(subject.as_str()));
}

#[test]
fn assistive_technology_reads_the_controls() {
  let h = rill(Appearance::DARK);
  reads(
    &h.a11y(),
    &[
      "radio group \"Repositories\"",
      "radio button \"quill\" on selected",
      "text field \"Search\" value \"\"",
      "button \"Fetch\" desc \"Fetch from origin\"",
      "check box \"BRANCHES 5\" on",
      "check box \"STASHES 2\" off",
      "splitter 0.17 in 0..1",
    ],
  );
  assert!(h.find(Role::List, "History").is_some());
  assert_eq!(h.focus_id(), Some(Id::of(COMMITS)), "the history first, for the arrows");
  assert!(h.find(Role::List, "Files").is_some());
}

/// A wheel's turn of `dy` over `id`, and the frame after it: how long the frame took.
fn scroll(h: &mut H, id: Id, dy: f32) -> Duration {
  let (x, y) = mid(rect(h, id));
  let (pos, delta) = (Point::new(x, y), Point::new(0.0, dy));
  let (phase, momentum, mods) = (Phase::None, Phase::None, Modifiers::empty());
  h.event(Event::Scroll { pos, delta, precise: true, phase, momentum, mods });
  let t = Instant::now();
  h.frame();
  h.render();
  t.elapsed()
}

fn median(mut v: Vec<Duration>) -> Duration {
  v.sort();
  v[v.len() / 2]
}

/// The middle of 31 frames scrolling a 2,400-line diff costs about what one of a short diff
/// does: the column builds only the lines in view. Relative, as tests in parallel slow both.
#[test]
fn a_big_diff_scrolls_as_fast_as_a_short_one() {
  let mut h = rill(Appearance::DARK);
  let worst = |h: &mut H, c: usize| {
    h.app_mut().select(c);
    h.frame();
    h.frame();
    median((0..31).map(|_| scroll(h, Id::of(DIFF), -40.0)).collect())
  };
  let (short, big) = (worst(&mut h, 0), worst(&mut h, 3));
  assert_eq!(h.app().diffs[0].lines.len(), 2417);
  assert!(h.scroll(Id::of(DIFF)).unwrap().offset.y > 1000.0, "scrolled");
  assert!(big < short * 3 + Duration::from_millis(20), "{big:?} against {short:?}");

  // The history too, its graph and labels in view.
  let list = median((0..31).map(|_| scroll(&mut h, Id::of(COMMITS), -60.0)).collect());
  assert!(list < short * 3 + Duration::from_millis(20), "{list:?} against {short:?}");
}
