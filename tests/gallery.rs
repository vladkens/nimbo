// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! Harness tests for `examples/gallery.rs`: every block has a page that builds, named by the
//! block; the sidebar turns the pages; a block's disabled variant reads disabled and its sizes
//! differ; a block alone is as high as its variants; in each appearance the widgets work and draw
//! in its theme (a check box checks in its accent, a switch slides, the slider drags, a picker
//! chooses by letters, a segment picks, a tooltip shows, a recorder records and its ⊗ clears,
//! the alert asks in its theme, Escape closes it, its action counts), the search field draws in
//! its theme and clears from its ⊗; a stepped slider lands on its steps from the pointer and the
//! keys and marks them with dots and labels; a row's footer spans its row; assistive technology
//! reads each widget by its role, the ⊗ marks beside their controls; and `docs/api.md` is the
//! gallery's, and so are its pictures on the `assets` branch when its worktree is there.

#[path = "../examples/gallery.rs"]
// The example sets the Windows subsystem, an attribute of its own crate, not of this module.
#[allow(dead_code, unused_attributes)]
mod gallery;

mod common;

use std::time::Duration;

use common::{diff, mid, px, reads};
use gallery::*;
use nimbo::ui::{Action, Role};
use nimbo::{Appearance, Event, Harness, Id, Modifiers, NamedKey, Rect, Size, Theme, alpha, mix};

type H = Harness<Gallery>;

fn gallery(page: usize, only: bool) -> H {
  let mut h = Harness::new(Gallery::on(page, only), Size::new(WIDTH + 200.0, 600.0), 1.0);
  h.frame();
  h
}

fn page(name: &str) -> usize {
  BLOCKS.iter().position(|b| b.name == name).unwrap_or_else(|| panic!("no {name}"))
}

/// The first node assistive technology reads as `role` named `name`, in the enabled line: its
/// id and rect.
fn node(h: &H, role: Role, name: &str) -> (Id, Rect) {
  let t = h.a11y_tree();
  let n = t.nodes.iter().find(|n| n.props.role == role && t.str(n.props.name) == name);
  let n = n.unwrap_or_else(|| panic!("no {role:?} {name:?} in\n{}", h.a11y()));
  (n.id, n.rect)
}

fn find(h: &H, role: Role, name: &str) -> Id {
  node(h, role, name).0
}

/// The gallery on the page of block `name` in `look`.
fn on(name: &str, look: Appearance) -> H {
  let mut h = gallery(page(name), false);
  h.event(Event::Appearance(look));
  h.frame();
  h
}

const LOOKS: [(Appearance, Theme); 2] =
  [(Appearance::LIGHT, Theme::PLATES_LIGHT), (Appearance::DARK, Theme::PLATES_DARK)];

#[test]
fn every_block_has_a_page_named_by_it() {
  for (i, b) in BLOCKS.iter().enumerate() {
    let h = gallery(i, false);
    let title = format!("static text {:?}", b.name);
    assert!(h.a11y().lines().any(|l| l.trim() == title), "{}: {}", b.name, h.a11y());
    assert!(GROUPS.contains(&b.group), "{}", b.name);
  }
}

#[test]
fn the_sidebar_turns_the_pages() {
  let mut h = gallery(0, false);
  let button = page("Button");
  h.click_id(Id::of(SIDEBAR).key(("row", button as u64)));
  h.frame();
  assert_eq!(h.app().page, button);
  let read = h.a11y();
  assert!(
    read.contains("static text \"Button\"") && read.contains("button \"Prominent\""),
    "{read}"
  );
}

/// The switch reads on in its enabled line, disabled in its disabled one; a large one is wider;
/// a small button is lower than the regular one, a large one higher.
#[test]
fn variants_are_disabled_and_sized() {
  let h = gallery(page("Switch"), false);
  let tree = h.a11y();
  let switches: Vec<&str> =
    tree.lines().map(str::trim).filter(|l| l.starts_with("switch")).collect();
  assert_eq!(switches.len(), 4, "enabled, disabled, small, large: {tree}");
  assert!(!switches[0].contains("disabled") && switches[1].contains("disabled"), "{switches:?}");
  let t = h.a11y_tree();
  let widths: Vec<f32> =
    t.nodes.iter().filter(|n| n.props.role == Role::Switch).map(|n| n.rect.w).collect();
  assert!(widths[3] > widths[0], "large wider: {widths:?}");
  let h = gallery(page("Button"), false);
  let t = h.a11y_tree();
  let hs: Vec<f32> = (t.nodes.iter())
    .filter(|n| n.props.role == Role::Button && t.str(n.props.name) == "Bordered")
    .map(|n| n.rect.h)
    .collect();
  assert!(hs.len() == 4 && hs[2] < hs[0] && hs[3] > hs[0], "small, large: {hs:?}");
}

/// Alone, a block's window is as wide as a page and as high as its variants, whatever it was.
#[test]
fn a_block_alone_is_as_high_as_its_variants() {
  for name in ["Button", "List sections", "Text"] {
    let mut h = gallery(page(name), true);
    h.frame();
    let (size, block) = (h.size(), h.rect(Id::of(BLOCK)).unwrap());
    assert_eq!((size.w, size.h), (WIDTH, block.h.ceil()), "{name}");
  }
}

/// The photo block shows the sunset the OS decoded, stretched to its box (blue sky at the top,
/// the dark ground at the bottom), filling a square and fitting one, kept across frames.
#[test]
fn the_photo_block_shows_the_decoded_photo() {
  let mut h = gallery(page("Photo"), true);
  assert_eq!(h.app().photo.as_ref().map(|p| (p.width(), p.height())), Some((240, 160)));
  for _ in 0..2 {
    // The photo, 120 × 80, inside the block's 20 points of padding.
    let b = h.rect(Id::of(BLOCK)).unwrap();
    let (x, y, img) = (b.x + 20.0, b.y + 20.0, h.render());
    let (sky, ground) = (img.pixel(x + 60.0, y + 2.0), img.pixel(x + 60.0, y + 78.0));
    assert!(diff(sky, [62, 72, 160, 255]) < 24, "sky {sky:?}");
    assert!(diff(ground, [22, 28, 44, 255]) < 24, "ground {ground:?}");
    // Filling a square: sky to its top; fitting it: letterboxed, no sky at its top.
    let (fill, fit) = (img.pixel(x + 170.0, y + 2.0), img.pixel(x + 260.0, y + 2.0));
    assert!(diff(fill, sky) < 24 && diff(fit, sky) > 48, "fill {fill:?}, fit {fit:?}");
    h.redraw();
    h.frame();
  }
}

/// In each appearance: a check box checked in its theme's accent, a switch that slides, the
/// slider dragged, a picker choosing by letters, a segment picked, a tooltip after a rest, a
/// recorder recording a chord and its ⊗ clearing it, the alert in its theme.
#[test]
fn every_widget_in_both_themes() {
  for (look, t) in LOOKS {
    let mut h = on("Check box", look);
    let (check, r) = node(&h, Role::CheckBox, "Wrap");
    h.click_id(check);
    h.leave();
    h.wait(Duration::from_millis(300));
    assert!(h.app().wrap);
    assert_eq!(h.render().pixel(r.x + 8.0, r.y + r.h / 2.0 - 6.0), px(t.accent), "{look:?}");

    let mut h = on("Switch", look);
    let (switch, r) = node(&h, Role::Switch, "Live");
    let at = |h: &mut H| h.render().pixel(r.x + 6.0, r.y + r.h / 2.0);
    assert_eq!(at(&mut h), px(t.accent), "on, in the accent");
    h.click_id(switch);
    h.leave();
    h.wait(Duration::from_millis(200));
    assert!(!h.app().live && at(&mut h) != px(t.accent), "{look:?}");

    let mut h = on("Slider", look);
    let s = node(&h, Role::Slider, "Zoom").1;
    h.press(s.x + 2.0, s.y + s.h / 2.0);
    h.frame();
    h.move_to(s.x + s.w / 2.0, s.y + s.h / 2.0);
    h.frame();
    h.release(s.x + s.w / 2.0, s.y + s.h / 2.0);
    assert!((h.app().zoom - 100.0).abs() < 1.0, "{}", h.app().zoom);

    let mut h = on("Picker", look);
    let size = find(&h, Role::PopUpButton, "Size");
    h.click_id(size);
    h.frame();
    h.frame();
    assert!(h.rect(size.key("menu")).is_some(), "open");
    h.key('l', Modifiers::empty());
    h.key(NamedKey::Enter, Modifiers::empty());
    h.frame();
    assert_eq!(h.app().size, gallery::Size::Large);

    let mut h = on("Segmented picker", look);
    h.click_id(find(&h, Role::RadioButton, "Columns 3"));
    assert_eq!(h.app().view, 2);

    let mut h = on("Tooltip", look);
    let (save, r) = node(&h, Role::Button, "Save");
    let (x, y) = mid(r);
    h.move_to(x, y);
    h.wait(Duration::from_millis(400));
    assert!(h.rect(save.key("tip")).is_some(), "the tip after a rest");

    let mut h = on("Shortcut recorder", look);
    // The recorder: a button with no name, its shortcut its value.
    let t2 = h.a11y_tree();
    let rec =
      t2.nodes.iter().find(|n| n.props.role == Role::Button && t2.str(n.props.name).is_empty());
    h.click_id(rec.expect("a recorder").id);
    h.key(NamedKey::F5, Modifiers::ALT);
    assert_eq!(h.app().hotkey.map(|k| k.to_string()).as_deref(), Some("option+f5"));
    h.frame();
    h.click_id(find(&h, Role::Button, "Clear"));
    assert_eq!(h.app().hotkey, None, "the ⊗ clears it");

    let mut h = on("Alert", look);
    h.click_id(find(&h, Role::Button, "Delete…"));
    h.wait(Duration::from_millis(200));
    let dialog = Id::of(DIALOG);
    let card = h.rect(dialog.key("card")).expect("the dialog");
    let fill = h.render().pixel(card.x + card.w / 2.0, card.y + 8.0);
    assert_eq!(fill, px(t.dialog), "{look:?}");
    h.key(NamedKey::Escape, Modifiers::empty());
    h.frame();
    assert!(!h.app().ask && h.rect(dialog.key("card")).is_none());
    h.click_id(find(&h, Role::Button, "Delete…"));
    h.frame();
    h.click_id(dialog.key("action"));
    assert_eq!((h.app().deleted, h.app().ask), (1, false));
  }
}

/// The search field in each theme's look over its ground; cleared from its ⊗, typed again, the
/// text kept from one appearance to the other.
#[test]
fn the_search_field_in_both_themes() {
  let mut h = on("Search field", Appearance::LIGHT);
  for (look, t) in LOOKS {
    h.event(Event::Appearance(look));
    h.frame();
    let (field, r) = node(&h, Role::TextField, "Search");
    let fill = t.search().frame.bg;
    let want = px(mix(t.bg, alpha(fill, 1.0), fill[3]));
    let got = h.render().pixel(r.x + r.w / 2.0, r.y + 4.0);
    assert!(diff(got, want) <= 1, "{look:?}: {got:?}, not {want:?}");
    if look == Appearance::LIGHT {
      h.click_id(field.key("clear"));
      h.frame();
      assert_eq!(h.app().query, "");
      assert!(h.rect(field.key("clear")).is_none());
      h.type_text("icons");
      h.frame();
    }
  }

  assert_eq!(h.app().query, "icons");
  let field = find(&h, Role::TextField, "Search");
  assert!(h.rect(field.key("clear")).is_some(), "its ⊗ back in the dark");
}

/// Each kind of widget is what it is to assistive technology: its role, name, value and states.
#[test]
fn every_widget_has_its_role() {
  let pages: [(&str, &[&str]); 11] = [
    ("Button", &["button \"Bordered\"", "button \"Prominent\" disabled"]),
    (
      "Help marks",
      &[
        "button \"Info\" desc \"An ⓘ explains what one line can't\"",
        "button \"Warning\" desc \"A warning tells what is wrong\"",
      ],
    ),
    ("Icon button", &["button \"Reset\""]),
    ("Check box", &["check box \"Wrap\" off", "check box \"Wrap\" off disabled"]),
    ("Switch", &["switch \"Live\" on"]),
    ("Slider", &["slider \"Zoom\" 60 in 0..200"]),
    ("Slider steps", &["slider \"Level\" 30 in 10..90", "static text \"50%\""]),
    (
      "Picker",
      &[
        "pop up button \"Size\" value \"Medium\"",
        "pop up button \"Size\" value \"Medium\" disabled",
      ],
    ),
    (
      "Segmented picker",
      &["radio group", "radio button \"List 128\" on selected", "radio button \"Grid 12\" off"],
    ),
    ("Progress", &["progress indicator 0.42 in 0..1"]),
    ("Link", &["link \"Learn more…\""]),
  ];
  for (name, lines) in pages {
    let h = gallery(page(name), false);
    reads(&h.a11y(), lines);
  }

  let h = gallery(page("Badge"), false);
  reads(&h.a11y(), &["static text \"New\""]);
  let h = gallery(page("Search field"), false);
  reads(&h.a11y(), &["text field \"Search\" value \"Switches\"", "button \"Clear\""]);
}

/// A segmented control goes to the next segment on Right and Down, back on Left and Up, and no
/// further than its ends: the arrows read as a list's, not a slider's.
#[test]
fn segments_step_down_and_up() {
  let mut h = gallery(page("Segmented picker"), false);
  let t = h.a11y_tree();
  let group = t.nodes.iter().find(|n| n.props.role == Role::RadioGroup).expect("a radio group");
  h.set_focus(Some(group.id));
  let key = |h: &mut H, k: NamedKey| {
    h.key(k, Modifiers::empty());
    h.frame();
    h.app().view
  };
  assert_eq!(key(&mut h, NamedKey::ArrowDown), 1, "down: the next");
  assert_eq!(key(&mut h, NamedKey::ArrowRight), 2);
  assert_eq!(key(&mut h, NamedKey::ArrowDown), 2, "not past the last");
  assert_eq!(key(&mut h, NamedKey::ArrowUp), 1, "up: the one before");
  assert_eq!(key(&mut h, NamedKey::ArrowLeft), 0);
}

/// The stepped slider's page ("Slider steps") at 2x in `look`: the enabled slider's id and rect.
fn steps(look: Appearance) -> (H, Id, Rect) {
  let size = Size::new(WIDTH + 200.0, 600.0);
  let mut h = Harness::new(Gallery::on(page("Slider steps"), false), size, 2.0);
  h.event(Event::Appearance(look));
  h.frame();
  let (id, r) = node(&h, Role::Slider, "Level");
  (h, id, r)
}

/// Where `v` of the stepped slider's 10..=90 sits along it, its track half a knob in from either
/// end.
fn level_x(r: Rect, v: f32) -> f32 {
  let inset = Theme::PLATES_LIGHT.sizes.knob[0] / 2.0;
  r.x + inset + (r.w - 2.0 * inset) * (v - 10.0) / 80.0
}

/// A press, a drag and a click land on the nearest step, the knob drawn there.
#[test]
fn a_stepped_slider_snaps_drags_and_clicks() {
  let (mut h, _, r) = steps(Appearance::LIGHT);
  let y = r.y + r.h / 2.0;
  h.press(level_x(r, 33.0), y);
  h.frame();
  assert_eq!(h.app().level, 30.0);
  h.move_to(level_x(r, 47.0), y);
  h.frame();
  assert_eq!(h.app().level, 50.0);
  let knob = px(Theme::PLATES_LIGHT.knob);
  assert_eq!(h.render().pixel(level_x(r, 50.0), y), knob, "the knob jumped to the step");
  h.release(level_x(r, 47.0), y);
  h.frame();
  assert_eq!(h.app().level, 50.0);
  h.click(level_x(r, 74.0), y);
  assert_eq!(h.app().level, 70.0);
}

/// The arrows and assistive technology's increment and decrement move a step, to the range's
/// ends and no further; the value reads as it is.
#[test]
fn a_stepped_slider_moves_a_step() {
  let (mut h, id, _) = steps(Appearance::LIGHT);
  h.set_focus(Some(id));
  let key = |h: &mut H, k: NamedKey| {
    h.key(k, Modifiers::empty());
    h.frame();
  };
  key(&mut h, NamedKey::ArrowRight);
  assert_eq!(h.app().level, 40.0);
  key(&mut h, NamedKey::ArrowLeft);
  key(&mut h, NamedKey::ArrowLeft);
  assert_eq!(h.app().level, 20.0);
  assert!(h.act(id, Action::Decrement));
  assert!(h.act(id, Action::Decrement));
  assert_eq!(h.app().level, 10.0, "not below the start");
  assert!(h.act(id, Action::Increment));
  h.frame();
  reads(&h.a11y(), &["slider \"Level\" 20 in 10..90 focused"]);
  key(&mut h, NamedKey::ArrowUp);
  assert_eq!(h.app().level, 30.0, "up raises it, as AppKit's slider");
  key(&mut h, NamedKey::ArrowDown);
  key(&mut h, NamedKey::ArrowDown);
  assert_eq!(h.app().level, 10.0, "down lowers it");
  (0..10).for_each(|_| key(&mut h, NamedKey::ArrowRight));
  assert_eq!(h.app().level, 90.0, "not past the end");
}

/// In each appearance a dot in the tertiary colour under the track at every step but the one
/// under the knob, nothing between them; the labels under their steps, within the slider.
#[test]
fn a_stepped_slider_marks_its_steps() {
  for (look, t) in LOOKS {
    let (mut h, _, r) = steps(look);
    // A quarter point above and left of a dot's middle: inside it, off its rim.
    let y = r.y + r.h / 2.0 + t.sizes.knob[1] / 2.0 - 1.25;
    let dot = px(t.tertiary);
    let img = h.render();
    for v in [10.0, 20.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0] {
      let got = img.pixel(level_x(r, v) - 0.25, y);
      assert!(diff(got, dot) <= 8, "{look:?}: a dot at {v}: {got:?}, not {dot:?}");
    }

    for (x, what) in
      [(level_x(r, 55.0), "between dots"), (level_x(r, 30.0) - 0.25, "under the knob")]
    {
      let got = img.pixel(x, y);
      assert!(diff(got, dot) > 40, "{look:?}: no dot {what}: {got:?}");
    }

    let tree = h.a11y_tree();
    let label = |name: &str| {
      let n = tree
        .nodes
        .iter()
        .find(|n| n.props.role == Role::StaticText && tree.str(n.props.name) == name);
      n.unwrap_or_else(|| panic!("no {name:?} in\n{}", h.a11y())).rect
    };
    let (first, mid50, last) = (label("10%"), label("50%"), label("90%"));
    assert!((mid(mid50).0 - level_x(r, 50.0)).abs() < 0.5, "{mid50:?} under 50 of {r:?}");
    assert!(first.x >= r.x && last.x + last.w <= r.x + r.w + 0.01, "{first:?} {last:?} in {r:?}");
    assert!(first.y >= r.y + r.h && mid50.y == first.y, "below the track: {first:?} {r:?}");
  }
}

/// A row's footer is in the row under its line, across its width: a slider there runs from the
/// plate's padding at the left to the switch's right edge, over the plate's fill with no
/// hairline above it, and takes its disabled look from the switch.
#[test]
fn a_row_footer_spans_its_row() {
  for (look, t) in LOOKS {
    let mut h = on("Row footer", look);
    let (_, slider) = node(&h, Role::Slider, "Level");
    let (switch, toggle) = node(&h, Role::Switch, "Turn off below 30%");
    let ends = (slider.x + slider.w - toggle.x - toggle.w).abs();
    assert!(ends < 0.5 && slider.y >= toggle.y + toggle.h, "{look:?}: {slider:?}, {toggle:?}");
    let (img, plate) = (h.render(), px(t.plate));
    let on_plate = |x: f32, y: f32| diff(img.pixel(x, y), plate) <= 2;
    let y = slider.y + slider.h - 2.0;
    let pad = (1..40).find(|&d| !on_plate(slider.x - d as f32 + 0.5, y));
    assert_eq!(pad, Some(11), "{look:?}: 10 points of the plate left of {slider:?}");
    let x = slider.x + slider.w / 2.0 + 30.0;
    for y in (toggle.y + toggle.h) as i32..slider.y as i32 + 6 {
      assert!(on_plate(x, y as f32 + 0.5), "{look:?}: no plate at {x}, {y}");
    }

    let enabled =
      |h: &H| h.a11y().lines().filter(|l| l.trim() == "slider \"Level\" 30 in 10..90").count();
    assert_eq!(enabled(&h), 1, "{look:?}: on with the switch");
    h.click_id(switch);
    h.frame();
    assert_eq!(enabled(&h), 0, "{look:?}: off with the switch\n{}", h.a11y());
  }
}

/// The ⊗ of a recorder and of a search field are beside them, not inside the leaves, where a
/// screen reader wouldn't reach them.
#[test]
fn clear_marks_are_beside_their_controls() {
  for (name, first) in
    [("Shortcut recorder", "button value "), ("Search field", "text field \"Search\" value")]
  {
    let h = gallery(page(name), false);
    let dump = h.a11y();
    // The recorder's value is the platform's label of its shortcut (⌃⌥Space, Ctrl+Alt+Space).
    let lines: Vec<&str> = dump.lines().collect();
    let at =
      lines.iter().position(|l| l.trim_start().starts_with(first) && !l.contains("disabled"));
    let at = at.unwrap_or_else(|| panic!("no {first} in\n{dump}"));
    let next = lines[at + 1..].iter().find(|l| l.trim_start().starts_with("button \"Clear\""));
    let depth = |l: &str| l.len() - l.trim_start().len();
    assert_eq!(next.map(|l| depth(l)), Some(depth(lines[at])), "{name}: beside it\n{dump}");
  }
}

#[test]
fn the_alert_asks_and_counts_its_action() {
  let mut h = gallery(page("Alert"), false);
  let ask = find(&h, Role::Button, "Delete…");
  assert!(h.act(ask, Action::Press));
  h.frame();
  h.frame();
  assert!(h.app().ask);
  assert!(h.act(Id::of(DIALOG).key("action"), Action::Press));
  h.frame();
  assert_eq!((h.app().ask, h.app().deleted), (false, 1));
}

/// The `assets` branch's pictures (`api/`), when its worktree is there: `.worktrees/assets` of the
/// repo's main checkout.
fn pictures() -> Option<std::path::PathBuf> {
  let common = ["rev-parse", "--path-format=absolute", "--git-common-dir"];
  let mut git = std::process::Command::new("git");
  git.args(common).current_dir(env!("CARGO_MANIFEST_DIR"));
  let out = git.output().ok().filter(|o| o.status.success())?;
  let dir = std::path::PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
  Some(dir.parent()?.join(".worktrees/assets/api")).filter(|d| d.is_dir())
}

/// Compresses the picture `png` in place as `make api-shots` does (`scripts/api-shots.py`).
fn compress(png: &std::path::Path) {
  let quant = ["--quality=80-95", "--strip", "--skip-if-larger", "--force", "--ext", ".png"];
  for (tool, args) in [("pngquant", &quant[..]), ("oxipng", &["-o", "max", "--strip", "all"][..])] {
    let run = std::process::Command::new(tool).args(args).arg(png).status();
    let run = run.unwrap_or_else(|e| panic!("{tool}: {e} (brew install pngquant oxipng)"));
    // pngquant's 98-99: the picture kept as it was, smaller so.
    assert!(run.success() || run.code() == Some(98) || run.code() == Some(99), "{tool}: {run}");
  }
}

/// `docs/api.md` is what the gallery's table makes. With the `assets` branch's worktree there, on
/// macOS, where `make api-shots` draws them, each of its pictures is what the block draws now: the
/// same size and nearly every pixel within a tolerance (another macOS draws its glyphs a touch
/// differently, and the pictures are compressed with loss), or byte for byte, compressed the same
/// way, with `NIMBO_API_SHOTS_STRICT` set, as `make api-shots` checks.
#[test]
fn the_api_doc_is_the_gallerys() {
  let root = env!("CARGO_MANIFEST_DIR");
  let doc = std::fs::read_to_string(format!("{root}/docs/api.md")).unwrap();
  assert!(doc == api_md(), "docs/api.md is stale: make api-shots");
  let Some(dir) = pictures().filter(|_| cfg!(target_os = "macos")) else { return };
  let strict = std::env::var_os("NIMBO_API_SHOTS_STRICT").is_some();
  let fresh = std::env::temp_dir().join(format!("nimbo-api-{}", std::process::id()));
  std::fs::create_dir_all(&fresh).unwrap();
  for (i, b) in BLOCKS.iter().enumerate() {
    let path = dir.join(format!("{}.png", b.slug()));
    let png = path.display();
    let kept = std::fs::read(&path).unwrap_or_else(|_| panic!("no {png}: make api-shots"));
    // As `gallery <slug> --shot` draws it.
    let mut h = Harness::new(Gallery::on(i, true), Size::new(WIDTH, 300.0), 2.0);
    h.event(Event::Appearance(Appearance::LIGHT));
    h.frame();
    let shot = fresh.join(format!("{}.png", b.slug()));
    h.shot(&shot).unwrap();
    if strict {
      compress(&shot);
      assert!(std::fs::read(&shot).unwrap() == kept, "{png} is stale: make api-shots");
      continue;
    }

    let now = std::fs::read(&shot).unwrap();

    let rgba = |file: Vec<u8>| nimbo::Png::new(Box::leak(file.into_boxed_slice())).rgba();
    let ((w, h1, a), (w2, h2, b2)) = (rgba(kept), rgba(now));
    assert_eq!((w, h1), (w2, h2), "{png}: another size, stale: make api-shots");
    let off = a.iter().zip(b2).filter(|(x, y)| x.abs_diff(**y) > 48).count();
    assert!(off * 100 <= a.len(), "{png}: {off} channels off, stale: make api-shots");
  }

  _ = std::fs::remove_dir_all(&fresh);
}
