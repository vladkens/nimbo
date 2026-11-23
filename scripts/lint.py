# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# /// script
# requires-python = ">=3.11"
# ///
"""lint: nimbo's checks that clippy and rustfmt don't have: the code style on the lines a branch
changes, and the layers of the crate on all of it.

  uv run scripts/lint.py [--all] [--base REF]                         check
  uv run scripts/lint.py fix [--all] [--base REF]                     add the missing blank lines
  uv run scripts/lint.py modules                                      what each top module uses
  uv run scripts/lint.py layers                                       each file's layer
  uv run scripts/lint.py size [--files | --map | --check] [--vs REF]  lines per module
  uv run scripts/lint.py hook                                         Stop hook of Claude Code, Codex

Only the changed lines count for the style rules, unless `--all`: the ones that differ from the
merge base with the default branch (origin/main or main), uncommitted and untracked files
included. On the default branch that leaves the uncommitted changes; CI on the default branch
checks its last commit.

  blank-line    a statement after an if / for / while / loop / match block starts after a blank
                line (error; `fix` adds it)
  dense-block   more than DENSE statements in a row in a function, without a blank line (error)
  long-comment  more than COMMENT comment lines in a row, `//!`, empty lines and doc examples aside
                (warning: locally each one is shown once, in CI always)
  dead-code     an allow or expect of dead_code or unused_*, unless a platform or feature has the
                code (error)
  allow         an allow attribute, or an expect without a reason (error)
  unused-pub    a new pub item that nothing else in the repository names, tests and doc examples
                included (error)
  unwrap        an unwrap outside tests, examples and build scripts (warning, as long-comment)
  cycle         a new use between the top modules that closes a cycle; with --all every cycle
                (warning, as long-comment)
  new-use       the first use of a top module by another (note)

The layer rules look at the whole crate on every run (errors):

  layer         a file uses an item of a higher layer (LAYERS: the base vocabulary at the bottom,
                then text, render, OS services, the ui core, headless behaviour, the styled
                design system, the shell, the harness, the crate root; PLACES puts each file in
                one); a `pub use` counts only when its file uses the name too; the platform
                facades (FACADES) are the platform's front and may be used from any layer; or
                calls a method of `Ui` / `UiState` that only a higher layer defines, by its name
  private       a file of the styled layer uses what a lower layer keeps to the crate, which a
                design system outside the crate couldn't: a field (`ui.st`, `self.next_id`), a
                type, a method or a function (a path to it or a name its `use` brings in) that
                is not `pub` there, by its name, unless a lower layer has a `pub` one of the
                name, the styled layer one of its own or std's types a method
  os            OS crates or `cfg(target_os)`-style gates outside src/platform/
  unplaced      a file of the crate no PLACES entry covers, or an entry no file matches

What breaks a layer rule today is listed in BASELINE, the reason in `#` comments: a finding's
key and how many there are (`layer src/ui/mod.rs -> ui/theme.rs::Theme 3`, `os src/keys.rs
target_os 5`). Findings up to the count pass; a line that allows more than is found, or more
than the base branch's baseline did, fails, so the list only shrinks.

size counts the non-blank lines of code, docs, comments and tests (every line of a test item)
per module: a folder under src/, a file right in src/, the tests, examples and benches each as
one. `--files` counts per file, `--map` lists each file's code lines and the first sentence of its
`//!` doc, `--vs REF` adds how the code changed since REF. `--check` fails when the branch adds
more than GROWTH lines of library code (under src/) since its base or REF, net of what it
removes.

The hook reads the agent's Stop event on stdin and works in repositories whose Makefile runs this
script. It runs the rules and, on an error or a warning it hasn't shown, asks the agent to go on;
with stop_hook_active it lets the agent stop. It fails open.

Ported from vladkens/lint rust.py (MIT), the checks every Rust repository of its author shares.
"""

import argparse
import bisect
import collections
import functools
import hashlib
import itertools
import json
import os
import posixpath
import re
import subprocess
import sys
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

DENSE = 12
COMMENT = 3
HOOK_LIMIT = 10
GROWTH = 600
# In a project's Makefile: the hook works there.
OPT_IN = "scripts/lint.py"

CI = bool(os.environ.get("CI"))
GITHUB = os.environ.get("GITHUB_ACTIONS") == "true"

OPEN = re.compile(r"^(?:let\b.*=\s*)?(?:if|for|while|loop|match)\b.*\{$")
CLOSE = re.compile(r"^\}[;,]?$")
NOT_STATEMENT = ("}", ")", "]", ".", "else", "//", "#[")
FN = re.compile(r"\bfn\s+(\w+)")
MOD = re.compile(r"\bmod\s+(\w+)$")
TEST = re.compile(r"#\[(?:cfg\((?:all\()?test\b|cfg\(any\(test\)\)|(?:\w+::)*test\b)")
TEST_DIRS = {"tests", "benches", "examples"}
ATTR = re.compile(r"#!?\[")
LINT_ATTR = re.compile(r"\s*(allow|expect|cfg_attr)\s*\(")
BRACKETS = {"[": 1, "(": 1, "{": 1, "]": -1, ")": -1, "}": -1}
REASON = re.compile(r"\breason\s*=")
UNWRAP = re.compile(r"\.\s*unwrap\s*\(\s*\)|\b(?:Option|Result)\s*::\s*unwrap\b")
DEAD = re.compile(r"#!?\[.*\b(?:allow|expect)\s*\(.*\b(?:dead_code|unused\w*)\b")
PUB = re.compile(
    r"^\s*pub\s+(?:(?:const|async|unsafe|extern\s*(?:\"\")?)\s+)*"
    r"(?:fn|struct|enum|trait|type|const|static|union)\s+(\w+)"
)
# Code that some platforms or features use: CI on the others reports it if it's dead there too.
PLATFORM = re.compile(r"cfg_attr\s*\(.*\b(?:target_\w+|unix|windows|feature)\b")
DOC = re.compile(r"^\s*//[/!]\s?(.*)")
# Items other languages call, which no Rust code names.
EXPORTED = re.compile(r"no_mangle|export_name|wasm_bindgen|\bextern\b|pyfunction|napi|uniffi")
IDENT = re.compile(r"\b[A-Za-z_]\w*\b")
# A name used on its own, not as a path's later segment (`Flow::Row` doesn't use `Row`).
USED = re.compile(r"(?<!::)\b[A-Za-z_]\w*\b")
USE = re.compile(r"\buse\s+([^;]+);")
PATH = re.compile(r"(?<![\w$])\$?(crate|super|self)((?:\s*::\s*\w+)+)")
MOD_DECL = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;")
PATH_ATTR = re.compile(r'#\[(?:cfg_attr\(.*?,\s*)?path\s*=\s*"([^"]+)"\)?\]')
ROOT = re.compile(r"(?:^|/)src/(?:lib|main|bin/[^/]+|bin/[^/]+/main)\.rs$")
LINKS = re.compile(r"\b(?:crate|super|self)\s*::|\bmod\s|\buse\s")

TOKEN = re.compile(r"""//|/\*|(?<!\w)[bc]?r#*"|"|'""")
BLOCK = re.compile(r"/\*|\*/")
STRING_END = re.compile(r'(?:[^"\\]|\\.)*"', re.DOTALL)
CHAR = re.compile(r"'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]{1,6}\}|.)|[^\\'\n])'")
HUNK = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")


@dataclass
class Hit:
    path: str
    line: int
    rule: str
    msg: str
    kind: str = "error"  # a warning is shown once locally; a note doesn't count
    key: str = ""

    @property
    def warn(self) -> bool:
        return self.kind == "warning"

    def __str__(self):
        return f"{self.path}:{self.line}: {self.kind}[{self.rule}]: {self.msg}"


@dataclass
class Frame:
    fn: tuple[str, int] | None
    test: bool
    macro: bool
    mod: str | None
    line: int


@dataclass
class Scan:
    raws: list[str]
    codes: list[str]  # without comments, with string and char literals emptied
    noted: set[int]  # lines with a comment
    body: list[bool]  # the line starts inside a function body
    test: list[bool]
    mods: list[tuple[str, ...]]  # the inline modules the line is in


# MARK: Rules


def strip(text: str) -> tuple[list[str], set[int]]:
    """Each line's code without comments and literal contents, and the lines with a comment."""
    out, noted, pos, line = [], set(), 0, 0
    while m := TOKEN.search(text, pos):
        out.append(text[pos : m.start()])
        line += text.count("\n", pos, m.start())
        tok, end = m[0], m.end()
        if tok == "//":
            end = text.find("\n", end)
            end = len(text) if end < 0 else end
            noted.add(line)
        elif tok == "/*":
            depth = 1
            for b in BLOCK.finditer(text, end):
                depth += 1 if b[0] == "/*" else -1
                if not depth:
                    end = b.end()
                    break
            else:
                end = len(text)

            lines = text.count("\n", m.start(), end)
            noted.update(range(line, line + lines + 1))
            out.append("\n" * lines)
            line += lines
        elif tok == "'":
            c = CHAR.match(text, m.start())
            end = c.end() if c else end
            out.append("''" if c else "'")
        else:
            if "r" in tok:
                close = '"' + "#" * tok.count("#")
                end = text.find(close, end)
                end = len(text) if end < 0 else end + len(close)
            else:
                s = STRING_END.match(text, end)
                end = s.end() if s else len(text)

            lines = text.count("\n", m.start(), end)
            out.append('""' + "\n" * lines)
            line += lines

        pos = end

    out.append(text[pos:])
    return "".join(out).split("\n"), noted


def blocks(codes: list[str]) -> tuple[list[bool], list[bool], list[tuple[str, ...]]]:
    """Per line: whether it starts in a function body outside macros, whether it's test code and
    the inline modules it's in."""
    body, test, mods, stack = [], [], [], []
    fn, tst = None, False
    for n, code in enumerate(codes):
        s = code.strip()
        body.append(any(f.fn for f in stack) and not any(f.macro for f in stack))
        mods.append(tuple(f.mod for f in stack if f.mod))
        was = tst or any(f.test for f in stack)
        if m := FN.search(s):
            fn = (m[1], n)

        tst = tst or bool(TEST.search(s))
        for i, c in enumerate(code):
            if c == "{":
                head = code[:i].rstrip()
                mod = None if fn else MOD.search(head)
                stack.append(Frame(fn, tst, head.endswith("!"), mod and mod[1], n))
                fn, tst = None, False
            elif c == "}" and stack:
                stack.pop()

        if s.endswith(";"):
            fn, tst = None, False

        test.append(was or tst or any(f.test for f in stack))

    return body, test, mods


def scan(text: str) -> Scan:
    codes, noted = strip(text)
    return Scan(text.split("\n"), codes, noted, *blocks(codes))


def blank_lines(sc: Scan) -> list[int]:
    """Indexes of the lines that need a blank line above them."""
    opened, out = [], []
    for i, (raw, code) in enumerate(zip(sc.raws, sc.codes)):
        text = code.strip()
        indent = len(raw) - len(raw.lstrip())
        if OPEN.match(text):
            opened.append(indent)
            continue

        if not (opened and indent == opened[-1] and CLOSE.match(text)):
            continue

        opened.pop()
        if i + 1 >= len(sc.raws):
            continue

        after = sc.raws[i + 1]
        if (
            after.strip()
            and len(after) - len(after.lstrip()) == indent
            and not after.strip().startswith(NOT_STATEMENT)
        ):
            out.append(i + 1)

    return out


def dense_blocks(sc: Scan) -> list[tuple[int, int, int]]:
    """Runs of more than DENSE statements without a blank line: first and last index, count."""
    out, run, start, end = [], 0, 0, 0
    for i, (raw, code) in enumerate(zip(sc.raws + [""], sc.codes + [""])):
        s = code.strip()
        if not raw.strip() or not (i < len(sc.body) and sc.body[i]):
            if run > DENSE:
                out.append((start, end, run))

            run = 0
            continue

        if s.endswith((";", "{")) and not s.startswith("}"):
            start = i if not run else start
            run, end = run + 1, i

    return out


def long_comments(sc: Scan) -> list[tuple[int, int, int]]:
    """Runs of more than COMMENT lines of comment text: first and last index, and the count. Empty
    comment lines and a doc comment's code examples don't count."""
    out, start, n, fenced = [], None, 0, False
    for i, (raw, code) in enumerate(zip(sc.raws + [""], sc.codes + [""])):
        text = raw.strip()
        if i in sc.noted and text and not code.strip() and not text.startswith(("//!", "// MARK")):
            start = i if start is None else start
            body = text.lstrip("/").strip()
            fenced ^= body.startswith("```")
            n += bool(body) and not fenced and not body.startswith("```")
            continue

        if start is not None and n > COMMENT:
            out.append((start, i - 1, n))

        start, n, fenced = None, 0, False

    return out


def closing(text: str, i: int) -> int:
    """The end of the bracket group that opens at i, or of the text."""
    depth = 0
    for j in range(i, len(text)):
        depth += BRACKETS.get(text[j], 0)
        if not depth:
            return j + 1

    return len(text)


def items(text: str) -> list[str]:
    """A list's items: split at its commas, not those in brackets."""
    out, depth, start = [], 0, 0
    for j, c in enumerate(text):
        depth += BRACKETS.get(c, 0)
        if c == "," and not depth:
            out.append(text[start:j])
            start = j + 1

    return out + [text[start:]]


def lint_args(attr: str) -> list[tuple[str, str]]:
    """The allows and expects of an attribute's content, those of a cfg_attr included: which one,
    and its arguments."""
    m = LINT_ATTR.match(attr)
    if not m:
        return []

    args = attr[m.end() : closing(attr, m.end() - 1) - 1]
    if m[1] != "cfg_attr":
        return [(m[1], args)]

    return [x for item in items(args)[1:] for x in lint_args(item)]


def lint_attrs(sc: Scan) -> list[tuple[int, int, str, str]]:
    """Each allow and expect of an attribute, plain or in a cfg_attr: the attribute's first and last
    line index, allow or expect, and its arguments."""
    text, out = "\n".join(sc.codes), []
    for m in ATTR.finditer(text):
        end = closing(text, m.end() - 1)
        a = text.count("\n", 0, m.start())
        b = a + text.count("\n", m.start(), end)
        out += [(a, b, kind, args) for kind, args in lint_args(text[m.end() : end - 1])]

    return out


def testing(path: str) -> bool:
    """Whether the whole file is tests, examples, benchmarks or a build script."""
    parts = Path(path).parts
    return bool(TEST_DIRS & set(parts[:-1])) or parts[-1] in ("build.rs", "tests.rs")


def touches(lines: set[int] | None, a: int, b: int) -> bool:
    """Whether the changed line numbers (None: all) meet the line indexes a to b."""
    return lines is None or not lines.isdisjoint(range(a + 1, b + 2))


def hits(path: str, sc: Scan, lines: set[int] | None) -> list[Hit]:
    """What the text rules find in a file, limited to the changed lines (None: all of them)."""

    def near(a: int, b: int) -> bool:
        return touches(lines, a, b)

    out = [
        Hit(path, i + 1, "blank-line", "a statement after a block starts after a blank line")
        for i in blank_lines(sc)
        if near(i - 1, i)
    ]
    out += [
        Hit(path, a + 1, "dense-block", f"{n} statements without a blank line; split the steps")
        for a, b, n in dense_blocks(sc)
        if near(a, b)
    ]
    for a, b, n in long_comments(sc):
        if not near(a, b):
            continue

        text = "\n".join(x.strip() for x in sc.raws[a : b + 1])
        key = hashlib.sha1(f"{path}\n{text}".encode()).hexdigest()[:16]
        msg = f"{n} comment lines; say only what the code can't, or keep it if it's needed"
        out.append(Hit(path, a + 1, "long-comment", msg, "warning", key))

    dead = {
        i
        for i, code in enumerate(sc.codes)
        if not testing(path) and not sc.test[i] and DEAD.search(code) and not PLATFORM.search(code)
    }
    msg = (
        "the compiler is told not to report unused code: delete it or use it; a field kept for its "
        "Drop or an FFI layout takes a name that starts with `_`"
    )
    out += [Hit(path, i + 1, "dead-code", msg) for i in sorted(dead) if near(i, i)]
    for a, b, kind, args in lint_attrs(sc):
        if a in dead or not near(a, b):
            continue

        if kind == "allow":
            msg = 'an allow stays when the lint is gone: `expect(lint, reason = "why")` says so'
        elif not REASON.search(args):
            msg = 'an expect says why: `expect(lint, reason = "why")`'
        else:
            continue

        out.append(Hit(path, a + 1, "allow", msg))

    if testing(path):
        return out

    msg = 'an unwrap panics without saying why: `?` if the function can fail, else `expect("why")`'
    for i, code in enumerate(sc.codes):
        if sc.test[i] or not near(i, i) or not UNWRAP.search(code):
            continue

        key = hashlib.sha1(f"{path}\n{sc.raws[i].strip()}".encode()).hexdigest()[:16]
        out.append(Hit(path, i + 1, "unwrap", msg, "warning", key))

    return out


# MARK: Unused pub items, modules


def package(path: str) -> str | None:
    """The directory of the package a file under src/ is in."""
    parts = path.split("/")[:-1]
    if "src" not in parts:
        return None

    return "/".join(parts[: len(parts) - 1 - parts[::-1].index("src")])


def unused_pubs(scans: "Scans", scope: dict | None, bins: bool) -> list[Hit]:
    """New pub items whose name appears nowhere else; in binaries only with bins, as the compiler
    reports them there."""
    libs = {package(x) for x in scans.names if x.endswith("src/lib.rs")}
    found = []
    for path in scans.names if scope is None else [x for x in scans.names if x in scope]:
        if testing(path) or not (bins or package(path) in libs):
            continue

        sc, lines = scans[path], None if scope is None else scope[path]
        for i, code in enumerate(sc.codes):
            if sc.test[i] or (lines is not None and i + 1 not in lines):
                continue

            m = PUB.match(code)
            name = m and m[1]
            if not name or name == "main":
                continue

            if not any(EXPORTED.search(x) for x in [code, *sc.raws[max(0, i - 3) : i]]):
                found.append((path, i, name))

    if not found:
        return []

    seen = collections.Counter(
        w for x in scans.names for code in uses(scans[x]) for w in IDENT.findall(code)
    )
    msg = "is used nowhere, not even in a test: use it, test it or delete it"
    return [
        Hit(path, i + 1, "unused-pub", f"`{name}` {msg}")
        for path, i, name in found
        if seen[name] < 2
    ]


def uses(sc: Scan) -> list[str]:
    """The code of a file, the examples in its doc comments, which run as tests, and its attributes,
    which name functions in strings: `#[serde(default = "port")]`."""
    out, fenced = list(sc.codes), False
    for raw in sc.raws:
        doc = DOC.match(raw)
        if doc and doc[1].startswith("```"):
            fenced = not fenced
        elif doc and fenced:
            out.append(doc[1])
        elif not doc:
            fenced = False
            if raw.lstrip().startswith("#["):
                out.append(raw)

    return out


def modules(scans: "Scans") -> dict[str, tuple[str, tuple[str, ...]]]:
    """Each file of a crate: its crate root and module path, from the roots' `mod x;` lines."""
    out = {}
    for root in sorted(x for x in scans.names if ROOT.search(x)):
        todo = [(root, ())]
        while todo:
            path, mod = todo.pop()
            if path in out:
                continue

            out[path] = (root, mod)
            sc, parent, stem = scans[path], posixpath.dirname(path), posixpath.basename(path)[:-3]
            here = parent if path == root or stem == "mod" else posixpath.join(parent, stem)
            for i, code in enumerate(sc.codes):
                if not (m := MOD_DECL.match(code)):
                    continue

                # A path per OS (`cfg_attr(unix, path = …)`) makes each file the module.
                attr = [a[1] for x in sc.raws[max(0, i - 3) : i] if (a := PATH_ATTR.search(x))]
                d = posixpath.join(here, *sc.mods[i])
                found = [posixpath.normpath(posixpath.join(parent, x)) for x in attr]
                found = [x for x in found if x in scans.names]
                if not attr:
                    cands = [posixpath.join(d, m[1] + ".rs"), posixpath.join(d, m[1], "mod.rs")]
                    cands = [posixpath.normpath(x) for x in cands]
                    found = [x for x in cands if x in scans.names][:1]

                todo += [(x, mod + sc.mods[i] + (m[1],)) for x in found]

    return out


def expand(tree: str) -> list[list[str]]:
    """The paths a use tree names: `a::{b, c::d as e}` gives a::b and a::c::d."""
    return [segs for segs, _ in named(tree)]


def named(tree: str) -> list[tuple[list[str], str]]:
    """The paths a use tree names, each with the name it brings in: its alias or its last part."""
    out = []

    def go(prefix: list[str], s: str):
        if s.endswith("}") and "{" in s:
            i, depth, part = s.index("{"), 0, ""
            head = [x for x in s[:i].split("::") if x]
            for c in s[i + 1 : -1] + ",":
                depth += (c == "{") - (c == "}")
                if c == "," and not depth:
                    go(prefix + head, part)
                    part = ""
                else:
                    part += c

        elif s:
            path, _, alias = s.partition(" as ")
            segs = prefix + [x for x in path.split("::") if x]
            out.append((segs, alias.strip() or (segs[-1] if segs else "")))

    go([], re.sub(r"\s*(::|[{},])\s*", r"\1", " ".join(tree.split())))
    return out


def resolve(cur: tuple[str, ...], segs: list[str]) -> tuple[str, ...] | None:
    """The absolute path that a crate::, self:: or super:: path names in module cur."""
    if not segs or segs[0] not in ("crate", "self", "super"):
        return None

    path, i = ([] if segs[0] == "crate" else list(cur)), int(segs[0] != "super")
    while i < len(segs) and segs[i] == "super":
        if not path:
            return None

        path.pop()
        i += 1

    return tuple(path + segs[i:])


def links(scans: "Scans") -> dict[tuple[str, str, str], list[tuple[str, int]]]:
    """Uses between the top modules of each crate, outside tests and crate roots: (root, from, to)
    and the lines."""
    tree = modules(scans)
    tops = collections.defaultdict(set)
    for root, mod in tree.values():
        if mod:
            tops[root].add(mod[0])

    out = collections.defaultdict(list)
    for path, (root, mod) in sorted(tree.items()):
        if not mod or testing(path):
            continue

        sc = scans[path]
        text = "\n".join(sc.codes)
        starts = list(itertools.accumulate((len(x) + 1 for x in sc.codes[:-1]), initial=0))
        uses = [(m.start(), m.end(), expand(m[1])) for m in USE.finditer(text)]
        refs = [(a, x) for a, _, xs in uses for x in xs]
        refs += [
            (m.start(), [m[1], *re.split(r"\s*::\s*", m[2].strip())[1:]])
            for m in PATH.finditer(text)
            if not any(a <= m.start() < b for a, b, _ in uses)
        ]
        for pos, segs in refs:
            i = bisect.bisect_right(starts, pos) - 1
            to = None if sc.test[i] else resolve(mod + sc.mods[i], segs)
            if to and to[0] in tops[root] and to[0] != mod[0]:
                at = out[(root, mod[0], to[0])]
                if (path, i + 1) not in at:
                    at.append((path, i + 1))

    return out


def route(graph: dict[str, set[str]], a: str, b: str) -> list[str] | None:
    """The shortest way from module a to b."""
    prev, todo = {a: a}, [a]
    for x in todo:
        if x == b:
            out = [b]
            while out[-1] != a:
                out.append(prev[out[-1]])

            return out[::-1]

        for y in sorted(graph.get(x, ())):
            if y not in prev:
                prev[y] = x
                todo.append(y)

    return None


def graphs(found: dict) -> dict[str, dict[str, set[str]]]:
    out: dict[str, dict[str, set[str]]] = collections.defaultdict(
        lambda: collections.defaultdict(set)
    )
    for root, a, b in found:
        out[root][a].add(b)

    return out


def reachable(graph: dict[str, set[str]], a: str) -> set[str]:
    todo = [a]
    for x in todo:
        todo += [y for y in sorted(graph.get(x, ())) if y not in todo]

    return set(todo)


def cycles(graph: dict[str, set[str]]) -> list[list[str]]:
    """The groups of modules that reach each other."""
    nodes = sorted(set(graph) | {y for ys in graph.values() for y in ys})
    reach = {x: reachable(graph, x) for x in nodes}
    out, done = [], set()
    for x in nodes:
        group = [y for y in nodes if y in reach[x] and x in reach[y]]
        if x not in done and len(group) > 1:
            out.append(group)

        done.update(group)

    return out


def tangles(top: Path, since: str | None, scope: dict | None, scans: "Scans") -> list[Hit]:
    """Cycles between modules: with --all every one, else those the change closes, as warnings,
    and the first uses of a module by another, as notes."""
    if scope is not None and not any(
        LINKS.search(raw)
        for x in scans.names
        if x in scope
        for n, raw in enumerate(scans[x].raws, 1)
        if scope[x] is None or n in scope[x]
    ):
        return []

    now, out = links(scans), []
    graph = graphs(now)
    if scope is None:
        for root in sorted(graph):
            for group in cycles(graph[root]):
                path, line = min(
                    r
                    for (rt, a, b), rs in now.items()
                    if rt == root and {a, b} <= {*group}
                    for r in rs
                )
                key = hashlib.sha1(f"cycle\n{root}\n{group}".encode()).hexdigest()[:16]
                msg = f"modules {', '.join(group)} use each other in a cycle"
                out.append(Hit(path, line, "cycle", msg, "warning", key))

        return out

    old = {}
    if since:
        same = {x for x in scans.names if x not in scope}
        before = Scans(top, snapshot(top, since, same))
        before.update({x: scans[x] for x in before.names if x in same})
        old = links(before)

    done = set()
    for (root, a, b), refs in sorted(now.items()):
        if (root, a, b) in old or (root, a) in done:
            continue

        path, line = next(
            ((p, n) for p, n in refs if p in scope and (scope[p] is None or n in scope[p])),
            refs[0],
        )
        back = route(graph[root], b, a)
        if not back:
            out.append(Hit(path, line, "new-use", f"`{a}` uses `{b}` for the first time", "note"))
            continue

        # One warning for each tangle: the other new uses in it close the same cycles.
        done.update((root, x) for x in reachable(graph[root], a) if a in reachable(graph[root], x))
        key = hashlib.sha1(f"cycle\n{root}\n{a}\n{b}".encode()).hexdigest()[:16]
        msg = (
            f"`{a}` now uses `{b}`, which uses it back: {' -> '.join([a, *back])}; move what both "
            "need into a module of their own, or keep it if it's wanted"
        )
        out.append(Hit(path, line, "cycle", msg, "warning", key))

    return out


# MARK: Layers

# Each file's layer, by the longest matching path under src/; a lower layer never uses a higher
# one. A platform file takes the layer of what it backs; the window host is the shell's. Every
# key names a file or folder that exists, and every file has a layer.
LAYERS = ("base", "text", "render", "service", "ui", "headless", "styled", "shell", "harness")
LAYERS += ("root",)
SERVICES = (
    *("activity", "autostart", "desktop", "file_dialog", "focus", "hardware", "hotkey"),
    *("input_source", "instance", "open", "paste", "paths", "permissions", "runloop", "system"),
    "uninstall",
)
PLACES = {
    **{x: "base" for x in ("clipboard", "geom.rs", "info.rs", "keys.rs", "locale", "menu.rs")},
    "clipboard/system.rs": "service",
    "text": "text",
    **{x: "render" for x in ("bitmap.rs", "cpu", "png.rs", "scene.rs", "svg.rs")},
    **{f"{x}.rs": "service" for x in SERVICES},
    "event.rs": "ui",
    "window.rs": "ui",
    "ui": "ui",
    "ui/behavior": "headless",
    **{f"ui/{x}": "styled" for x in ("containers", "dialog.rs", "form.rs", "theme.rs", "widgets")},
    **{x: "shell" for x in ("app.rs", "shell.rs", "tray", "platform")},
    "harness.rs": "harness",
    "lib.rs": "root",
    # The platform's files below the shell, by what they back.
    **{
        f"platform/{x}": "base"
        for x in ("linux/dbus.rs", "linux/display.rs", "linux/dl.rs", "linux/keys.rs")
    },
    **{f"platform/{x}": "base" for x in ("macos/keyboard.rs", "windows/keyboard.rs")},
    **{f"platform/{x}/text.rs": "text" for x in ("linux", "macos", "windows")},
    **{f"platform/{x}/image.rs": "render" for x in ("linux", "macos", "windows")},
    **{
        f"platform/{os}/app/{x}": "service"
        for os, names in (
            (
                "linux",
                "activity autostart desktop focus grab hardware hotkey input_source instance",
            ),
            ("linux", "open paste post remote uninstall write"),
            ("macos", "activity autostart desktop focus hardware hotkey hotkey/carbon"),
            ("macos", "input_source open paste permissions post uninstall"),
            ("windows", "activity autostart desktop focus hardware hotkey input_source instance"),
            ("windows", "open paste post uninstall"),
        )
        for x in (f"{n}.rs" for n in names.split())
    },
}
# The platform's facades: every layer reaches the OS through them.
FACADES = {"src/platform/mod.rs"} | {
    f"src/platform/{os}/{x}"
    for os in ("linux", "macos", "windows")
    for x in ("mod.rs", "app/mod.rs")
}
BASELINE = "scripts/lint-baseline.txt"
PUB_USE = re.compile(r"^\s*pub(?:\([^)]*\))?\s+use\b")
MACRO = re.compile(r"#\[macro_export\]\s*macro_rules!\s*(\w+)")
VIS = r"(?:(pub(?:\([^)]*\))?)\s+)?"
TYPE_DEF = re.compile(rf"^[ \t]*{VIS}(?:struct|enum|union|trait|type)\s+(\w+)", re.MULTILINE)
FN_DEF = re.compile(
    rf"^[ \t]*{VIS}(?:(?:const|async|unsafe|extern\s*\"\")\s+)*fn\s+(\w+)[^(]*"
    r"\(\s*(?:&\s*(?:'\w+\s+)?)?(?:mut\s+)?(self\b)?",
    re.MULTILINE,
)
STRUCT = re.compile(rf"^[ \t]*{VIS}struct\s+(\w+)[^;{{(]*\{{", re.MULTILINE)
FIELD = re.compile(r"^\s*(?:#\[[^\]]*\]\s*)*(pub(?:\([^)]*\))?\s+)?(\w+)\s*:")
# An impl or trait block, the trait it implements and its type.
IMPL = re.compile(
    r"^[ \t]*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?(impl|trait)\b\s*(?:<(?:[^<>]|<[^<>]*>)*>)?"
    r"\s*([^{\n]*?\bfor\s+)?(?:\w+::)*(\w+)[^{\n]*\{",
    re.MULTILINE,
)
METHOD = re.compile(r"\.\s*(\w+)\s*(?:::<[^>]*>)?\s*\(")
# A function called by a path, or by its bare name: `::` when the path names it.
CALL = re.compile(r"(?:(::)\s*|(?<![\w.:])(?<!fn ))(\w+)\s*(?:::<[^>]*>)?\s*\(")
FIELD_USE = re.compile(r"\b(?:self|ui)\s*\.\s*(\w+)\b(?!\s*(?:\(|::))")
TYPE_USE = re.compile(r"(?<![\w.])[A-Z]\w*")
# Methods of std types that a function of the crate may share a name with.
STD_NAMES = {"take", "get", "insert", "remove", "push", "pop", "len", "contains", "clear", "iter"}
STD_NAMES |= {"map", "set", "replace", "new", "hash", "finish", "read", "write", "fmt", "cmp", "eq"}
STD_NAMES |= {"open", "retain"}
OS_CRATE = re.compile(r"\b(objc2\w*|block2|dispatch2|windows_sys)\s*::")
OS_GATE = re.compile(
    r"\bcfg(?:_attr)?\s*!?\s*\((?:[^\];{}()]|\()*?\b(target_os|target_family|target_vendor|windows|unix)\b"
)


@functools.cache
def layer(path: str) -> str | None:
    """The layer of a file under src/; None for tests, examples, benches and a file PLACES misses."""
    if not path.startswith("src/"):
        return None

    rest = path[4:]
    best = max((k for k in PLACES if rest == k or rest.startswith(k + "/")), key=len, default=None)
    return PLACES[best] if best else None


def unplaced(scans: "Scans") -> list[Hit]:
    """A file of the crate with no layer, and a PLACES key that names nothing."""
    names = [x for x in scans.names if x.startswith("src/")]
    out = [
        Hit(x, 1, "layer", f"{x[4:]} has no layer: add it to PLACES in scripts/lint.py")
        for x in names
        if layer(x) is None
    ]
    stale = [k for k in PLACES if not any(x[4:] == k or x[4:].startswith(k + "/") for x in names)]
    out += [Hit("scripts/lint.py", 1, "layer", f"PLACES names `{k}`, which is gone") for k in stale]
    return out


def absolute(cur: tuple[str, ...], segs: list[str], files: dict) -> tuple[str, ...] | None:
    """The crate path a path names in module cur: a crate::, self:: or super:: path, or one that
    starts with a child module; None for other crates."""
    found = resolve(cur, segs)
    if found is None and segs and cur + (segs[0],) in files:
        found = cur + tuple(segs)

    return found


def exports(scans: "Scans", tree: dict) -> dict[tuple[str, ...], tuple[str, ...]]:
    """What a `pub use` names: the path through the re-export to the path it re-exports; and a
    module's private alias (`use macos as imp;`), which its re-exports go through."""
    files = {mod: path for path, (_, mod) in tree.items()}
    found = []
    for path, (_, mod) in tree.items():
        sc = scans[path]
        text = "\n".join(sc.codes)
        starts = list(itertools.accumulate((len(x) + 1 for x in sc.codes[:-1]), initial=0))
        for m in USE.finditer(text):
            line = bisect.bisect_right(starts, m.start()) - 1
            if not sc.test[line]:
                cur = mod + sc.mods[line]
                found += [
                    (cur, segs, name, PUB_USE.match(sc.codes[line])) for segs, name in named(m[1])
                ]

    out = {}
    for cur, segs, name, public in found:
        to = absolute(cur, segs, files)
        if not public and to in files:
            out[cur + (name,)] = to

    known = files | out
    for cur, segs, name, public in found:
        to = absolute(cur, segs, known)
        if public:
            # Another crate's item: no file of ours.
            out[cur + (name,)] = to if to is not None else ("<extern>",)

    # An exported macro is the crate root's, defined where its `macro_rules!` is.
    for path, (_, mod) in tree.items():
        for name in MACRO.findall("\n".join(scans[path].codes)):
            out[(name,)] = mod + (name,)

    return out


def target(segs: tuple[str, ...], files: dict, reexp: dict) -> str | None:
    """The file that defines the item a path names, following re-exports."""
    for _ in range(8):
        # A path that goes on past a module names the module, not a function of the same name.
        k = next(
            (
                i
                for i in range(len(segs), 0, -1)
                if segs[:i] in reexp and not (segs[:i] in files and i < len(segs))
            ),
            None,
        )
        if k is None:
            break

        segs = reexp[segs[:k]] + segs[k:]

    if segs[:1] == ("<extern>",):
        return None

    return next((files[segs[:i]] for i in range(len(segs), -1, -1) if segs[:i] in files), None)


def edges(scans: "Scans") -> list[tuple[str, int, str, str]]:
    """The uses of one file's items by another, outside tests, `#[cfg(doc)]` and re-exports: the
    file, line, the path as written and the file it names."""
    tree = {x: v for x, v in modules(scans).items() if v[0] == "src/lib.rs"}
    files = {mod: path for path, (_, mod) in tree.items()}
    reexp = exports(scans, tree)
    out = []
    for path, (_, mod) in sorted(tree.items()):
        sc = scans[path]
        text = "\n".join(sc.codes)
        starts = list(itertools.accumulate((len(x) + 1 for x in sc.codes[:-1]), initial=0))
        uses = [(m.start(), m.end(), named(m[1])) for m in USE.finditer(text)]
        refs = [(a, segs, name) for a, _, xs in uses for segs, name in xs]
        refs += [
            (m.start(), segs, segs[-1])
            for m in PATH.finditer(text)
            if not any(a <= m.start() < b for a, b, _ in uses)
            for segs in [[m[1], *re.split(r"\s*::\s*", m[2].strip())[1:]]]
        ]
        bare = text
        for a, b, _ in uses:
            bare = bare[:a] + re.sub(r"[^\n]", " ", bare[a:b]) + bare[b:]

        # Tests don't count: a name used only there doesn't make its re-export a use.
        used = set(USED.findall("\n".join(x for x, t in zip(bare.split("\n"), sc.test) if not t)))
        for pos, segs, name in refs:
            i = bisect.bisect_right(starts, pos) - 1
            if sc.test[i] or "#[cfg(doc)]" in sc.raws[max(0, i - 1)]:
                continue

            # A re-export alone isn't a use; the file using the name it brings in is.
            if PUB_USE.match(sc.codes[i]) and name not in used:
                continue

            to = absolute(mod + sc.mods[i], segs, files)
            file = to and target(to, files, reexp)
            if file and file != path:
                out.append((path, i + 1, "::".join(x for x in to if x != "*"), file))

    return out


@dataclass
class Def:
    kind: str  # fn, method (a fn that takes self), field or type
    name: str
    public: bool  # `pub`, or a trait's
    owner: str | None  # the type a fn is implemented on, or a field's struct


def defs(sc: Scan) -> list[Def]:
    """The items a file defines outside tests: its types, its structs' fields and its functions,
    those of an impl block with its type."""
    if hasattr(sc, "defs"):
        return sc.defs

    # What a function body defines is its own.
    found = {p: [m for i, m in matches(sc, p) if not sc.body[i]] for p in (IMPL, TYPE_DEF, STRUCT)}
    text, fns = sc.text, [m for i, m in matches(sc, FN_DEF) if not sc.body[i]]
    blocks = [(m.start(), closing(text, m.end() - 1), m) for m in found[IMPL]]
    out = [Def("type", m[2], m[1] == "pub", None) for m in found[TYPE_DEF]]
    for m in found[STRUCT]:
        body = text[m.end() : closing(text, m.end() - 1) - 1]
        fields = (FIELD.match(x) for x in items(body))
        out += [Def("field", f[2], (f[1] or "").strip() == "pub", m[2]) for f in fields if f]

    for m in fns:
        imp = next((b for a, z, b in reversed(blocks) if a < m.start() < z), None)
        # A trait's functions are as public as the trait.
        public = m[1] == "pub" or bool(imp and (imp[1] == "trait" or imp[2]))
        out.append(Def("method" if m[3] else "fn", m[2], public, imp and imp[3]))

    sc.defs = out
    return out


def hidden(scans: "Scans") -> dict[str, set[str]]:
    """Per kind, the names of what the lower layers keep to the crate: not public in them, nor
    defined in the styled layer, nor a method std's types share."""
    low = LAYERS[: LAYERS.index("headless") + 1]
    private, public = collections.defaultdict(set), collections.defaultdict(set)
    # The platform's own files are behind its facades, their `pub` the platform's only.
    for path in (x for x in scans.names if not x.startswith("src/platform/") or x in FACADES):
        at = layer(path)
        for d in defs(scans[path]) if at in low or at == "styled" else []:
            # A styled file's `self` and `ui` are a Ui or its own type: another's field is no
            # name it could reach.
            if at == "styled" or d.public and d.kind != "field":
                public[d.kind].add(d.name)
            else:
                private[d.kind].add(d.name)

    return {k: private[k] - public[k] - STD_NAMES for k in ("fn", "method", "field", "type")}


def upward(scans: "Scans") -> list[Hit]:
    """Calls of a method of Ui or UiState that only a higher layer defines, by its name."""
    ui, lowest = {}, collections.defaultdict(lambda: len(LAYERS))
    for path in scans.names:
        if (at := layer(path)) is None:
            continue

        for d in (d for d in defs(scans[path]) if d.kind == "method"):
            lowest[d.name] = min(lowest[d.name], LAYERS.index(at))
            if d.owner in ("Ui", "UiState"):
                ui.setdefault(d.name, path)

    out = []
    for path in scans.names:
        at = layer(path)
        for i, m in matches(scans[path], METHOD) if at else []:
            file = ui.get(m[1])
            if file and lowest[m[1]] > LAYERS.index(at) and m[1] not in STD_NAMES:
                msg = (
                    f"{path[4:]} ({at}) -> .{m[1]}() of {file[4:]} ({layer(file)}): a lower layer "
                    "may not call a method a higher one adds; move it down, or hand it in"
                )
                key = f"layer {path} -> {file[4:]}::{m[1]}"
                out.append(Hit(path, i + 1, "layer", msg, key=key))

    return out


def layering(scans: "Scans", since: str | None = None) -> list[Hit]:
    """The layer rules on the whole crate; the baseline compared with its own at `since`."""
    out = unplaced(scans)
    for path, line, item, file in edges(scans):
        a, b = layer(path), layer(file)
        if file in FACADES or not (a and b) or LAYERS.index(b) <= LAYERS.index(a):
            continue

        name = item.split("::")[-1]
        msg = (
            f"{path[4:]} ({a}) -> {item} ({b}): a lower layer may not use a higher one; move the "
            "item down, or hand it in from above (a hook or a parameter)"
        )
        key = f"layer {path} -> {file[4:]}::{name}"
        out.append(Hit(path, line, "layer", msg, key=key))

    out += upward(scans)
    kept = hidden(scans)
    for path in scans.names:
        sc, at = scans[path], layer(path)
        if at is not None:
            out += styled(path, sc, kept) if at == "styled" else []
            out += os_code(path, sc) if not path.startswith("src/platform/") else []

    return baselined(out, scans.top, since)


def matches(sc: Scan, pattern: re.Pattern) -> list[tuple[int, re.Match]]:
    """Each match of a pattern over the file's code, which may span lines, outside tests: the line
    it starts on and the match."""
    if not hasattr(sc, "text"):
        sc.text = "\n".join(sc.codes)
        sc.starts = list(itertools.accumulate((len(x) + 1 for x in sc.codes[:-1]), initial=0))

    found = ((bisect.bisect_right(sc.starts, m.start()) - 1, m) for m in pattern.finditer(sc.text))
    return [(i, m) for i, m in found if not sc.test[i]]


def styled(path: str, sc: Scan, kept: dict[str, set[str]]) -> list[Hit]:
    """What the styled layer takes from the lower ones that a design system outside the crate
    couldn't: a field, type, method or function they keep to the crate ([hidden])."""
    out = []
    found = [(i, "field", m[1]) for i, m in matches(sc, FIELD_USE)]
    found += [(i, "type", m[0]) for i, m in matches(sc, TYPE_USE)]
    found += [(i, "method", m[1]) for i, m in matches(sc, METHOD)]
    # A function the file brings in by a `use`, or names by a path.
    brought = {name for _, m in matches(sc, USE) for _, name in named(m[1])}
    found += [(i, "fn", m[2]) for i, m in matches(sc, CALL) if m[1] or m[2] in brought]
    msg = "`{}` is the crate's own: a design system outside it couldn't use it"
    return [
        Hit(path, i + 1, "private", msg.format(name), key=f"private {path} {name}")
        for i, kind, name in sorted(found)
        if name in kept[kind]
    ]


def os_code(path: str, sc: Scan) -> list[Hit]:
    """OS crates and OS gates outside src/platform/."""
    out = []
    for i, m in sorted(matches(sc, OS_CRATE) + matches(sc, OS_GATE), key=lambda x: x[0]):
        msg = f"`{m[1]}` outside src/platform/: OS code lives in platform/<os>/"
        out.append(Hit(path, i + 1, "os", msg, key=f"os {path} {m[1]}"))

    return out


def allowed(text: str) -> dict[str, tuple[int, int | None]]:
    """A baseline's lines: each key with its line number and how many findings it allows (None
    for a line without a count)."""
    out = {}
    for n, raw in enumerate(text.splitlines(), 1):
        key = raw.split("#")[0].strip()
        head, _, num = key.rpartition(" ")
        if key:
            out[head if num.isdigit() else key] = (n, int(num) if num.isdigit() else None)

    return out


def baselined(found: list[Hit], top: Path, since: str | None) -> list[Hit]:
    """The findings the baseline doesn't allow, and its lines that allow more than is found or
    than the base allowed. A line is a finding's key and how many there are:
    `layer src/ui/mod.rs -> ui/theme.rs::Theme 3`, `os src/keys.rs target_os 5`."""
    file = top / BASELINE
    listed = allowed(read(file)) if file.is_file() else {}
    before = since and git(top, "show", f"{since}:{BASELINE}")
    before = allowed(before) if before else None
    counts = collections.Counter(h.key for h in found)
    out = []
    for h in found:
        n = listed.get(h.key, (0, 0))[1]
        if n is not None and counts[h.key] > n:
            h.msg += f" ({counts[h.key]} found, the baseline allows {n})" if n else ""
            out.append(h)

    for key, (line, n) in listed.items():
        was = before.get(key, (0, 0))[1] if before is not None else None
        if n is None:
            msg = f"`{key}` has no count: end the line with how many are found ({counts[key]})"
        elif was is not None and n > was:
            msg = (
                f"`{key}` allows {n}, more than the base's {was}: fix the finding, it only shrinks"
            )
        elif counts[key] < n:
            msg = f"`{key}` is found {counts[key]} times, not {n}: lower or delete the line"
        else:
            continue

        out.append(Hit(BASELINE, line, "baseline", msg))

    return out


def places(top: Path) -> int:
    """Prints each file of the crate with its layer."""
    for path in Scans(top).names:
        if (at := layer(path)) is not None:
            print(f"{at:<9} {path}")

    return 0


# MARK: Changed lines


def git(top: Path, *args: str) -> str | None:
    rep = subprocess.run(
        ["git", *args], cwd=top, capture_output=True, encoding="utf-8", errors="replace"
    )
    return rep.stdout.strip() if rep.returncode == 0 else None


def base(top: Path, ref: str | None) -> str | None:
    """The commit the changed lines are counted from; None when there is nothing to compare to."""
    if ref:
        found = git(top, "rev-parse", "--verify", "--quiet", ref + "^{commit}")
        if not found:
            sys.exit(f"lint: unknown base {ref}")

        return found

    if CI and git(top, "rev-parse", "--is-shallow-repository") == "true":
        git(top, "fetch", "--no-tags", "--quiet", "--unshallow", "origin")

    names = ["origin/HEAD", "origin/main", "origin/master", "main", "master"]
    if os.environ.get("GITHUB_BASE_REF"):
        names.insert(0, "origin/" + os.environ["GITHUB_BASE_REF"])

    head = git(top, "rev-parse", "--verify", "--quiet", "HEAD")
    found = next((x for x in (git(top, "merge-base", "HEAD", n) for n in names) if x), None)
    if not found:
        return head

    if CI and found == head:
        return git(top, "rev-parse", "--verify", "--quiet", "HEAD~1") or found

    return found


def changed(top: Path, since: str) -> dict[str, set[int] | None]:
    """Changed .rs files and their changed line numbers; None for an untracked file."""
    diff = git(
        top,
        *("-c", "core.quotePath=false", "diff", "-U0", "--no-color", "--no-ext-diff"),
        *("--src-prefix=a/", "--dst-prefix=b/", since, "--", "*.rs"),
    )
    out: dict[str, set[int]] = {}
    path = None
    for line in (diff or "").splitlines():
        if line.startswith("+++ "):
            path = line[6:] if line.startswith("+++ b/") else None
        elif path and (m := HUNK.match(line)):
            a, n = int(m[1]), int(m[2] or 1)
            # A deletion touches the lines around it: it can remove a needed blank line.
            out.setdefault(path, set()).update(range(a, a + n) if n else (a, a + 1))

    new = git(top, "ls-files", "--others", "--exclude-standard", "--", "*.rs")
    return {**out, **dict.fromkeys((new or "").splitlines())}


def targets(top: Path, scope: dict | None) -> list[tuple[str, set[int] | None]]:
    if scope is None:
        names = git(top, "ls-files", "-co", "--exclude-standard", "--", "*.rs") or ""
        scope = dict.fromkeys(names.splitlines())

    return [(x, lines) for x, lines in sorted(scope.items()) if (top / x).is_file()]


def read(path: Path) -> str:
    return path.read_bytes().decode("utf-8", "replace")


class Scans(dict):
    """The scans of a repository's .rs files, made when first needed, from the given texts or the
    working tree."""

    def __init__(self, top: Path, texts: Mapping[str, str | None] | None = None):
        super().__init__()
        self.top, self.texts = top, texts
        self.names = sorted(texts) if texts is not None else [x for x, _ in targets(top, None)]

    def __missing__(self, path: str) -> Scan:
        text = self.texts.get(path) if self.texts is not None else None
        self[path] = scan(read(self.top / path) if text is None else text)
        return self[path]


def snapshot(top: Path, rev: str, same: set[str]) -> dict[str, str | None]:
    """The .rs files of a commit; None for those in same, as the working tree has them."""
    names = (git(top, "ls-tree", "-r", "-z", "--name-only", rev) or "").split("\0")
    out = {x: None for x in names if x.endswith(".rs")}
    need = [x for x in out if x not in same]
    batch = "".join(f"{rev}:{x}\n" for x in need).encode()
    data = subprocess.run(["git", "cat-file", "--batch"], cwd=top, input=batch, capture_output=True)
    pos, data = 0, data.stdout
    for x in need:
        end = data.find(b"\n", pos)
        head, pos = data[pos:end].split(), end + 1
        if len(head) == 3 and head[1] == b"blob":
            out[x] = data[pos : pos + int(head[2])].decode("utf-8", "replace")
            pos += int(head[2]) + 1

    return out


def findings(top: Path, scope: dict | None, since: str | None, bins: bool = False) -> list[Hit]:
    scans, out = Scans(top), []
    for x in scans.names:
        if scope is None or x in scope:
            out += hits(x, scans[x], None if scope is None else scope[x])

    out += unused_pubs(scans, scope, bins) + tangles(top, since, scope, scans)
    return out + layering(scans, since)


def unseen(top: Path, warns: list[Hit]) -> list[Hit]:
    """The warnings not shown in this repository before, remembered as shown from now on."""
    path = top / (git(top, "rev-parse", "--git-common-dir") or ".git") / "nimbo-lint-seen"
    seen = set(read(path).split()) if path.is_file() else set()
    new = [x for x in warns if x.key not in seen]
    if new:
        path.write_text("".join(f"{x}\n" for x in sorted(seen | {x.key for x in new})))

    return new


# MARK: Size

KINDS = ("code", "docs", "comm", "tests")


def kinds(sc: Scan) -> dict[str, int]:
    """The non-blank lines of code, docs, comments and tests."""
    n = dict.fromkeys(KINDS, 0)
    for i, (raw, code) in enumerate(zip(sc.raws, sc.codes)):
        if not raw.strip():
            continue

        if sc.test[i]:
            n["tests"] += 1
        elif code.strip() or i not in sc.noted:
            n["code"] += 1
        else:
            n["docs" if DOC.match(raw) else "comm"] += 1

    return n


def place(path: str) -> tuple[list[str], list[str]] | None:
    """A file under a package's src/: the package's folder and the path in src/, without `.rs`;
    None for tests, examples, benches and build scripts."""
    parts = path.removesuffix(".rs").split("/")
    if "src" not in parts[:-1]:
        return None

    k = len(parts) - 2 - parts[-2::-1].index("src")
    return None if TEST_DIRS & set(parts[:k]) else (parts[:k], parts[k + 1 :])


def module(path: str, dirs: set[str]) -> str:
    """The row a file counts in: its folder under src/, or the file when it's right in src/ or has
    a folder of its own; the tests, examples and benches one each, after the package's folder."""
    found = place(path)
    if found is None:
        parts = path.split("/")
        k = next((i for i, x in enumerate(parts[:-1]) if x in TEST_DIRS), len(parts) - 1)
        return "/".join(parts[: k + 1]).removesuffix(".rs")

    pkg, rest = found
    if len(rest) > 1 and path.removesuffix(".rs") not in dirs:
        rest = rest[:-1]

    return "/".join(pkg + rest)


def summary(sc: Scan) -> str:
    """The first sentence of a file's `//!` doc."""
    doc = []
    for raw in sc.raws:
        s = raw.strip()
        if s.startswith("//!"):
            doc.append(s[3:].strip())
        elif doc or (s and not s.startswith(("//", "#!["))):
            break

    first = re.split(r"(?<=\.)\s", " ".join(doc).strip(), maxsplit=1)[0].rstrip(".")
    return first if len(first) <= 100 else first[:99].rstrip() + "…"


def table(scans: "Scans", files: bool) -> dict[str, dict[str, int]]:
    dirs = {posixpath.dirname(x) for x in scans.names}
    rows = collections.defaultdict(lambda: dict.fromkeys(KINDS, 0))
    for x in scans.names:
        row = rows[x if files else module(x, dirs)]
        for k, v in kinds(scans[x]).items():
            row[k] += v

    return rows


def library(scans: "Scans") -> int:
    """The code lines under the packages' src/."""
    return sum(kinds(scans[x])["code"] for x in scans.names if place(x) is not None)


# MARK: Commands


def annotate(kind: str, path: str, line: int, title: str, msg: str):
    def esc(x: str, prop: bool = False) -> str:
        x = x.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
        return x.replace(":", "%3A").replace(",", "%2C") if prop else x

    print(f"::{kind} file={esc(path, True)},line={line},title={esc(title, True)}::{esc(msg)}")


def check(top: Path, scope: dict | None, since: str | None) -> int:
    found = findings(top, scope, since)
    errors = [x for x in found if x.kind == "error"]
    warns = [x for x in found if x.warn]
    notes = [x for x in found if x.kind == "note"]
    shown = warns if CI or scope is None else unseen(top, warns)
    for h in errors + shown + notes:
        print(h)
        if GITHUB:
            annotate("notice" if h.kind == "note" else h.kind, h.path, h.line, h.rule, h.msg)

    failed = len(errors)
    where = f"the lines changed since {since[:7]}" if since else "all lines"
    said = [count(failed, "error")] if failed or not shown else []
    said += [count(len(shown), "warning")] if shown else []
    said += [count(len(notes), "note")] if notes else []
    hidden = len(warns) - len(shown)
    if hidden:
        it = "it" if hidden == 1 else "them"
        where += f" ({count(hidden, 'warning')} shown before, `--all` lists {it})"

    print(f"lint: {', '.join(said)} in {where}")
    return 1 if failed else 0


def outline(top: Path) -> int:
    """Prints what each top module of each crate uses, in how many places, and the cycles."""
    found = links(Scans(top))
    for root, graph in sorted(graphs(found).items()):
        print(root)
        for a in sorted(graph):
            print(
                f"  {a} -> " + ", ".join(f"{b} {len(found[root, a, b])}" for b in sorted(graph[a]))
            )

        for group in cycles(graph):
            print(f"  cycle: {', '.join(group)}")

    return 0


def size(top: Path, opts) -> int:
    """Prints the lines per module or file, or checks how much library code the branch adds."""
    scans = Scans(top)
    if opts.check:
        since = base(top, opts.vs or opts.base)
        if not since:
            return 0

        added = library(scans) - library(Scans(top, snapshot(top, since, set())))
        print(f"lint: {added:+} lines of library code since {since[:7]}, at most {GROWTH}")
        if added > GROWTH:
            print(f"lint: {added - GROWTH} over: split the work into several PRs")
            return 1

        return 0

    if opts.map:
        w = max(map(len, scans.names), default=0)
        for x in sorted(scans.names, key=lambda x: (x.count("/"), x)):
            print(f"{x:<{w}} {kinds(scans[x])['code']:>6}  {summary(scans[x])}")

        return 0

    rows = table(scans, opts.files)
    since = base(top, opts.vs) if opts.vs else None
    was = table(Scans(top, snapshot(top, since, set())), opts.files) if since else None
    empty = dict.fromkeys(KINDS, 0)
    names = sorted(set(rows) | set(was or {}), key=lambda x: (-rows.get(x, empty)["code"], x))
    total = {k: sum(r[k] for r in rows.values()) for k in KINDS}
    w = max(map(len, [*names, "module"]))
    head = f"{'module':<{w}} {'code':>7} {'%':>4} {'docs':>6} {'comm':>6} {'tests':>6}"
    head += f" {'vs ' + opts.vs:>10}" if was is not None else ""
    print(head)
    print("-" * len(head))
    for x in [*names, None]:
        if x is None:
            print("-" * len(head))

        r = total if x is None else rows.get(x, empty)
        pct = 100 * r["code"] / max(total["code"], 1)
        line = f"{x or 'total':<{w}} {r['code']:>7} {pct:>4.0f} {r['docs']:>6} {r['comm']:>6}"
        line += f" {r['tests']:>6}"
        if was is not None:
            then = sum(y["code"] for y in was.values()) if x is None else was.get(x, empty)["code"]
            line += f" {r['code'] - then:>+10}"

        print(line)

    return 0


def count(n: int, word: str) -> str:
    return f"{n or 'no'} {word}{'' if n == 1 else 's'}"


def fix(top: Path, scope: dict | None) -> int:
    added = 0
    for x, lines in targets(top, scope):
        text = read(top / x)
        sc = scan(text)
        need = [h.line - 1 for h in hits(x, sc, lines) if h.rule == "blank-line"]
        if not need:
            continue

        eol = "\r" if "\r\n" in text else ""
        for i in reversed(need):
            sc.raws.insert(i, eol)

        (top / x).write_bytes("\n".join(sc.raws).encode())
        added += len(need)

    if added:
        print(f"lint: added {count(added, 'blank line')}")

    return 0


def hook() -> int:
    try:
        pld = json.load(sys.stdin)
        top = git(Path(pld.get("cwd") or "."), "rev-parse", "--show-toplevel")
        make = Path(top or ".", "Makefile")
        if pld.get("stop_hook_active") or not top or not make.is_file():
            return 0

        top = Path(top)
        since = base(top, None) if OPT_IN in read(make) else None
        if not since:
            return 0

        found = findings(top, changed(top, since), since, bins=True)
        errors = [x for x in found if x.kind == "error"]
        new = unseen(top, [x for x in found if x.warn])
        if not errors and not new:
            return 0

        shown = errors + new
        why = ["lint found this in the Rust lines the branch changes:"]
        why += [str(x) for x in shown[:HOOK_LIMIT]]
        if len(shown) > HOOK_LIMIT:
            why.append(f"…and {len(shown) - HOOK_LIMIT} more; `make check` lists them all.")

        if errors:
            why.append("Fix the errors before you finish; `make prepare` adds the blank lines.")

        if new:
            why.append(
                "A warning comes up once: shorten a long comment to what the reader needs, "
                "replace an unwrap, untangle a cycle, or leave it if it's needed."
            )

        print(json.dumps({"decision": "block", "reason": "\n".join(why)}))
    except Exception:
        pass

    return 0


def main() -> int:
    cli = argparse.ArgumentParser(
        prog="lint", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    cmds = ("check", "fix", "modules", "layers", "size", "hook")
    cli.add_argument("cmd", nargs="?", choices=cmds, default="check")
    cli.add_argument("--all", action="store_true", help="every line counts, not only changed ones")
    cli.add_argument("--base", help="count the changes from this commit")
    cli.add_argument("--files", action="store_true", help="size: one row per file")
    cli.add_argument("--map", action="store_true", help="size: each file and what it does")
    cli.add_argument("--vs", metavar="REF", help="size: how the code changed since REF")
    cli.add_argument("--check", action="store_true", help=f"size: at most {GROWTH} new lines")
    opts = cli.parse_args()
    if opts.cmd == "hook":
        return hook()

    top = git(Path.cwd(), "rev-parse", "--show-toplevel")
    if not top:
        sys.exit("lint: not in a git repository")

    top = Path(top)
    if opts.cmd == "modules":
        return outline(top)

    if opts.cmd == "layers":
        return places(top)

    if opts.cmd == "size":
        return size(top, opts)

    since = None if opts.all else base(top, opts.base)
    scope = changed(top, since) if since else None
    return fix(top, scope) if opts.cmd == "fix" else check(top, scope, since)


if __name__ == "__main__":
    sys.exit(main())
