# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# /// script
# requires-python = ">=3.11"
# ///
"""Lines per module: code, docs, comments and inline tests, apart.

A module is a directory under src/ (platform/<os>, platform/<os>/app, ui/widgets and
ui/containers each on their own, and so are the add-on files ui/form.rs and ui/dialog.rs) or a
top-level file; examples/ and tests/ are one row each. Code is every line that is not blank, a
comment, a doc comment or inside a `#[cfg(test)]` item. With a git ref, a column shows how code
moved since it.
`--map` lists the files with the first sentence of each one's module doc (`//!`): what it does.
`--check` sums the code per part against its budget (BUDGETS) and fails when one is over.

uv run scripts/size.py [--files | --map | --check] [--vs <ref>]
"""

import argparse
import re
import subprocess
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DIRS = ("src", "examples", "tests")
EXTS = (".rs", ".metal")
KINDS = ("code", "docs", "comments", "tests", "blank")
# The code lines each part may have: a platform, the UI core (ui/: the tree, layout, input,
# paint, theme), its widgets and containers (ui/widgets, ui/containers: add-ons over it), the
# text system (text/: shaping, the glyph atlas, the editing model), the app platform (its
# portable part: the shell, tray, hotkeys, paths, launch at login; and each OS's
# platform/<os>/app) and the core (the rest of src/). Raising one is a decision recorded in its
# PR.
BUDGETS = {
  "platform/macos": 1650,
  "platform/windows": 2000,
  "platform/linux": 2500,
  "ui": 3700,
  "widgets": 3100,
  "text": 1500,
  "core": 3500,
  "app/shared": 900,
  "app/macos": 1200,
  "app/windows": 1200,
  "app/linux": 1500,
}
# Modules that count against a budget of another name ("app", src/app.rs, is the core's seams).
# The settings form and the dialog are add-ons over the widgets, as the containers are.
PARTS = {
  "ui/widgets": "widgets",
  "ui/containers": "widgets",
  "ui/form": "widgets",
  "ui/dialog": "widgets",
  **{m: "app/shared" for m in ("shell", "tray", "hotkey", "paths", "autostart")},
}


def module(path: str, files: bool) -> str:
  """The module a file's lines count in.

  >>> [module(p, False) for p in ("src/ui/form.rs", "src/ui/theme.rs", "src/ui/widgets/x.rs")]
  ['ui/form', 'ui', 'ui/widgets']
  """
  if files:
    return path

  parts = path.split("/")
  if parts[0] != "src":
    return parts[0]

  if len(parts) == 2:
    return parts[1].removesuffix(".rs")

  if parts[1] == "platform" and len(parts) > 4 and parts[3] == "app":
    return f"app/{parts[2]}"

  if parts[1] in ("platform", "ui") and len(parts) > 2:
    sub = f"{parts[1]}/{parts[2].removesuffix('.rs')}"
    if parts[1] == "platform" and len(parts) > 3 or sub in PARTS:
      return sub

  return parts[1]


def part(module: str) -> str | None:
  """The budget a module's code counts against; examples and tests have none.

  >>> [part(m) for m in ("platform/linux", "ui", "ui/containers", "text", "cpu", "tests")]
  ['platform/linux', 'ui', 'widgets', 'text', 'core', None]
  >>> [part(m) for m in ("app", "tray", "app/linux")]
  ['core', 'app/shared', 'app/linux']
  """
  if module in BUDGETS:
    return module

  if module in PARTS:
    return PARTS[module]

  return None if module in ("examples", "tests") else "core"


def check(rows: dict[str, dict[str, int]]) -> bool:
  """Prints each part's code against its budget; whether all are within."""
  used = defaultdict(int)
  for m, r in rows.items():
    if (p := part(m)) is not None:
      used[p] += r["code"]

  ok = True
  for p, limit in BUDGETS.items():
    over = used[p] > limit
    print(f"{p:<18} {used[p]:>6} / {limit:<6} {'over' if over else 'ok'}")
    ok &= not over

  return ok


def braces(line: str) -> int:
  line = re.sub(r'"(\\.|[^"\\])*"', "", line.split("//")[0])
  line = re.sub(r"'(\\.|[^'\\])'", "", line)
  return line.count("{") - line.count("}")


def count(text: str) -> dict[str, int]:
  n = dict.fromkeys(KINDS, 0)
  test, depth = False, 0

  for raw in text.splitlines():
    line = raw.strip()
    if not test and line.startswith("#[cfg(test)]"):
      test, depth = True, 0

    if test:
      n["tests"] += 1
      depth += braces(line)
      if depth == 0 and ("}" in line or line.endswith(";")):
        test = False
      continue

    if not line:
      n["blank"] += 1
    elif line.startswith(("///", "//!")):
      n["docs"] += 1
    elif line.startswith("//"):
      n["comments"] += 1
    else:
      n["code"] += 1

  return n


def summary(text: str) -> str:
  """The first sentence of a file's leading `//!` doc."""
  doc = []
  for raw in text.splitlines():
    line = raw.strip()
    if line.startswith("//!"):
      doc.append(line[3:].strip())
    elif doc or (line and not line.startswith("//")):
      break

  first = re.split(r"(?<=[.])\s", " ".join(doc).strip(), maxsplit=1)[0]
  return first.rstrip(".") if len(first) <= 110 else first[:109].rstrip() + "…"


def files_now() -> dict[str, str]:
  out = {}
  for d in DIRS:
    for p in sorted((ROOT / d).rglob("*")):
      if p.suffix in EXTS:
        out[str(p.relative_to(ROOT))] = p.read_text()

  return out


def files_at(ref: str) -> dict[str, str]:
  git = ["git", "-C", str(ROOT)]
  names = subprocess.run([*git, "ls-tree", "-r", "--name-only", ref, *DIRS], capture_output=True, text=True, check=True).stdout
  out = {}
  for name in names.split():
    if name.endswith(EXTS):
      out[name] = subprocess.run([*git, "show", f"{ref}:{name}"], capture_output=True, text=True, check=True).stdout

  return out


def table(files: dict[str, str], by_file: bool) -> dict[str, dict[str, int]]:
  rows = defaultdict(lambda: dict.fromkeys(KINDS, 0))
  for path, text in files.items():
    row = rows[module(path, by_file)]
    for k, v in count(text).items():
      row[k] += v

  return rows


def main():
  ap = argparse.ArgumentParser(description="Lines per module.")
  ap.add_argument("--files", action="store_true", help="one row per file")
  ap.add_argument("--map", action="store_true", help="one row per file with what it does")
  ap.add_argument("--vs", metavar="REF", help="show the code change since this git ref")
  ap.add_argument("--check", action="store_true", help="fail when a part is over its budget")
  args = ap.parse_args()

  files = files_now()
  if args.check:
    raise SystemExit(0 if check(table(files, False)) else 1)

  if args.map:
    for path in sorted(files, key=lambda p: (p.count("/"), p)):
      print(f"{path:<36} {count(files[path])['code']:>5}  {summary(files[path])}")

    return

  rows = table(files, args.files)
  base = table(files_at(args.vs), args.files) if args.vs else None
  names = sorted(set(rows) | set(base or {}), key=lambda m: -rows[m]["code"] if m in rows else 0)
  total = {k: sum(r[k] for r in rows.values()) for k in KINDS}

  w = max(len("module"), *(len(m) for m in names))
  head = f"{'module':<{w}} {'code':>7} {'%':>4} {'docs':>6} {'comm':>6} {'tests':>6}"
  print(head + (f" {'vs ' + args.vs:>10}" if base else ""))
  print("-" * (len(head) + (11 if base else 0)))

  for m in [*names, None]:
    r = total if m is None else rows.get(m, dict.fromkeys(KINDS, 0))
    if m is None:
      print("-" * (len(head) + (11 if base else 0)))

    pct = 100 * r["code"] / max(total["code"], 1)
    line = f"{m or 'total':<{w}} {r['code']:>7} {pct:>4.0f} {r['docs']:>6} {r['comments']:>6} {r['tests']:>6}"
    if base is not None:
      was = sum(b["code"] for b in base.values()) if m is None else base.get(m, {"code": 0})["code"]
      line += f" {r['code'] - was:>+10}"

    print(line)


if __name__ == "__main__":
  main()
