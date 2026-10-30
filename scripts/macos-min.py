# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# /// script
# requires-python = ">=3.11"
# ///
"""Checks that the examples run on macOS 11 (MIN), built as an app builds them, with no target set.

It builds the examples with MACOSX_DEPLOYMENT_TARGET removed from the environment and no
`--config`, so a deployment target set again in `.cargo/config.toml` fails it, then:
1. each example's LC_BUILD_VERSION minos (`otool -l`) must be MIN (skipped on a non-arm64 host,
   where rustc's default is 10.12);
2. the C symbols they import (`nm -mu`, libSystem's too; not libc++'s, which are C++) and the
   unmangled undefined symbols of libnimbo's rlib (`nm -u`: also the externs no example links)
3. are referenced in one Objective-C file over the SDK's headers (FRAMEWORKS, HEADERS: those of
   every libSystem function std and nimbo call), compiled with clang at
   `-mmacosx-version-min=MIN -Werror=unguarded-availability`, so the headers' availability says
   which are newer than MIN;
4. every clang error fails, naming the symbol and the binaries importing it, except an undeclared
   identifier that libobjc exports (its .tbd; its ARC entry points have no public header),
   objc2's statics, compiler-rt's (`__*`) or std's (`rust_*`), and UNWIND. A libSystem function
   with no header included fails as undeclared: its header goes into HEADERS.
Selectors are sent at run time and stay unchecked: those are for review. Off macOS it does nothing.

uv run scripts/macos-min.py
"""

import json
import os
import platform
import re
import subprocess
import sys
import tempfile
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MIN = "11.0"
# libc++'s imports are C++ (mangled): no C header declares them.
CXX = "libc++"
FRAMEWORKS = (
  "Cocoa/Cocoa.h",
  "Carbon/Carbon.h",
  "CoreText/CoreText.h",
  "IOSurface/IOSurface.h",
  "QuartzCore/QuartzCore.h",
  "ServiceManagement/ServiceManagement.h",
  "CoreAudio/CoreAudio.h",
  "IOKit/IOKitLib.h",
  "ApplicationServices/ApplicationServices.h",
  "CoreServices/CoreServices.h",
)
HEADERS = (
  "dispatch/dispatch.h",
  "objc/runtime.h",
  "objc/message.h",
  "pthread.h",
  "dlfcn.h",
  "Block.h",
  "crt_externs.h",
  "mach-o/dyld.h",
  "unwind.h",
  "stdlib.h",
  "stdio.h",
  "string.h",
  "strings.h",
  "unistd.h",
  "fcntl.h",
  "dirent.h",
  "signal.h",
  "sched.h",
  "time.h",
  "math.h",
  "errno.h",
  "poll.h",
  "sys/socket.h",
  "sys/stat.h",
  "sys/mman.h",
  "sys/file.h",
  "sys/ioctl.h",
  "sys/uio.h",
  "sys/time.h",
  "sys/event.h",
  "sys/sysctl.h",
  "mach/mach.h",
  "mach/mach_time.h",
  "malloc/malloc.h",
  "os/lock.h",
  "os/log.h",
  "os/os_sync_wait_on_address.h",
  "CommonCrypto/CommonRandom.h",
)
# std's personality calls these: unwind.h marks them unavailable on arm64, libSystem exports them.
UNWIND = ("_Unwind_GetDataRelBase", "_Unwind_GetTextRelBase")
# objc2's statics for the classes it declares, after the Mach-O leading underscore.
OBJC2 = ("__CLASS_", "__IVAR_OFFSET_", "__DROP_FLAG_OFFSET_")
CLASS = re.compile(r"^OBJC_(?:META)?CLASS_\$_(\w+)$")


def nm_import(line: str) -> tuple[str, str] | None:
  """The symbol (no leading underscore) and its library from a line of `nm -mu`.

  >>> nm_import("                 (undefined) external _CAFrameRateRangeMake (from QuartzCore)")
  ('CAFrameRateRangeMake', 'QuartzCore')
  >>> nm_import("                 (undefined) weak external _foo (from libSystem)")
  ('foo', 'libSystem')
  >>> nm_import("                 (undefined) external _bar (dynamically looked up)")
  ('bar', '')
  >>> nm_import("") is None
  True
  """
  m = re.match(r"\s*\(undefined\) (?:weak )?external _(\S+)(?: \(from ([^)]+)\))?", line)
  return (m[1], m[2] or "") if m else None


def minos(text: str) -> str | None:
  """The minos of the LC_BUILD_VERSION load command in `otool -l` output.

  >>> minos("Load command 9\\n      cmd LC_BUILD_VERSION\\n  cmdsize 32\\n    minos 14.0\\n")
  '14.0'
  >>> minos("      cmd LC_VERSION_MIN_MACOSX\\n  version 10.12\\n") is None
  True
  """
  m = re.search(r"cmd LC_BUILD_VERSION\n(?:.*\n)*?\s*minos (\S+)", text)
  return m[1] if m else None


def tbd_symbols(text: str) -> set[str]:
  """The exported symbols (no leading underscore) listed in a .tbd file.

  >>> sorted(tbd_symbols("symbols: [ '$ld$hide$os10.12$_a', __b,\\n _c ]\\nweak-symbols: [ _d ]"))
  ['_b', 'c', 'd']
  """
  out = set()
  for group in re.findall(r"symbols:\s*\[([^\]]*)\]", text):
    names = (s.strip().strip("'\"") for s in group.split(","))
    out |= {s[1:] for s in names if s.startswith("_")}

  return out


def run(*cmd: str, **kw) -> str:
  return subprocess.run(cmd, check=True, capture_output=True, text=True, **kw).stdout


def build() -> tuple[list[Path], Path]:
  """The examples and libnimbo's rlib, built with no deployment target in the environment."""
  env = {k: v for k, v in os.environ.items() if k != "MACOSX_DEPLOYMENT_TARGET"}
  cmd = ("cargo", "build", "--examples", "--message-format=json-render-diagnostics")
  res = subprocess.run(cmd, cwd=ROOT, env=env, stdout=subprocess.PIPE, text=True)
  if res.returncode != 0:
    sys.exit("macos-min: the build failed")

  examples, rlib = [], None
  for line in res.stdout.splitlines():
    msg = json.loads(line)
    if msg.get("reason") != "compiler-artifact":
      continue

    if "example" in msg["target"]["kind"] and msg.get("executable"):
      examples.append(Path(msg["executable"]))

    if msg["target"]["name"] == "nimbo" and "lib" in msg["target"]["kind"]:
      rlib = next(Path(f) for f in msg["filenames"] if f.endswith(".rlib"))

  return sorted(examples), rlib


def imports(examples: list[Path], rlib: Path) -> dict[str, set[str]]:
  """Each C symbol to check, with the names of the binaries importing it."""
  out = defaultdict(set)
  for path in examples:
    for line in run("nm", "-mu", str(path)).splitlines():
      found = nm_import(line)
      if found and found[1] != CXX:
        out[found[0]].add(path.name)

  # Members without symbols (lib.rmeta) only warn on stderr.
  for line in run("nm", "-u", str(rlib)).splitlines():
    sym = line.strip()
    if not sym.startswith("_") or sym.endswith(":") or sym.startswith(("__ZN", "__R")):
      continue

    out[sym[1:]].add("libnimbo.rlib")

  return out


def exempt(sym: str, msg: str, objc: set[str]) -> bool:
  """A clang error that says nothing about availability: an undeclared identifier libobjc,
  objc2, compiler-rt or std has, or UNWIND's unavailable.

  >>> syms = ("objc_retain", "__CLASS_NimboView", "__udivti3", "rust_eh_personality", "memcpy")
  >>> [exempt(s, "use of undeclared identifier", {"objc_retain"}) for s in syms]
  [True, True, True, True, False]
  >>> [exempt("_Unwind_GetTextRelBase", m, set()) for m in ("is unavailable", "is only available")]
  [True, False]
  """
  if sym in UNWIND:
    return "is unavailable" in msg

  known = sym in objc or sym.startswith(OBJC2) or sym.startswith(("__", "rust_"))
  return "undeclared identifier" in msg and known


def compile_check(syms: list[str]) -> list[tuple[str, str]]:
  """Clang's errors on a reference to each symbol at MIN, as (symbol or "", message)."""
  head = [f"#import <{h}>" for h in FRAMEWORKS] + [f"#include <{h}>" for h in HEADERS]
  body = []
  for sym in syms:
    m = CLASS.match(sym)
    body.append(f"  (void)[{m[1]} class];" if m else f"  (void)&{sym};")

  first = len(head) + 2
  src = "\n".join(head + ["void nimbo_refs(void) {"] + body + ["}", ""])
  with tempfile.TemporaryDirectory() as tmp:
    path = Path(tmp) / "refs.m"
    path.write_text(src)
    cmd = ("xcrun", "clang", "-fsyntax-only", "-x", "objective-c", f"-mmacosx-version-min={MIN}")
    flags = ("-Werror=unguarded-availability", "-ferror-limit=0", str(path))
    res = subprocess.run(cmd + flags, capture_output=True, text=True)

  out = []
  for line in res.stderr.splitlines():
    m = re.match(rf"{re.escape(str(path))}:(\d+):\d+: (?:fatal )?error: (.*)", line)
    if m:
      i = int(m[1]) - first
      out.append((syms[i] if 0 <= i < len(syms) else "", m[2]))
    elif ": error: " in line or ": fatal error: " in line:
      out.append(("", line))

  if res.returncode != 0 and not out:
    out.append(("", res.stderr.strip() or f"clang exited with {res.returncode}"))

  return out


def configs() -> list[Path]:
  """The cargo configs that apply here and set a deployment target: ROOT's and its parents'."""
  home = Path(os.environ.get("CARGO_HOME") or Path.home() / ".cargo")
  paths = [d / ".cargo" / n for d in (ROOT, *ROOT.parents) for n in ("config.toml", "config")]
  paths += [home / "config.toml", home / "config"]
  return [p for p in paths if p.is_file() and "MACOSX_DEPLOYMENT_TARGET" in p.read_text()]


def main() -> int:
  if platform.system() != "Darwin":
    print("macos-min: not macOS, skipped")
    return 0

  examples, rlib = build()
  fails = []
  if platform.machine() != "arm64":
    print(f"macos-min: {platform.machine()} host, rustc's default isn't {MIN}: minos not checked")
  else:
    found = defaultdict(list)
    for path in examples:
      found[minos(run("otool", "-l", str(path)))].append(path.name)

    fails += [f"minos {v}, not {MIN}: {', '.join(names)}" for v, names in found.items() if v != MIN]
    if set(found) != {MIN}:
      fails += [f"MACOSX_DEPLOYMENT_TARGET is set in {p}" for p in configs()]

  sdk = Path(run("xcrun", "--show-sdk-path").strip())
  objc = tbd_symbols((sdk / "usr/lib/libobjc.A.tbd").read_text())
  syms = imports(examples, rlib)
  for sym, msg in compile_check(sorted(syms)):
    if sym and exempt(sym, msg, objc):
      continue

    fails.append(f"{sym}: {msg} ({', '.join(sorted(syms[sym]))})" if sym else msg)

  for line in fails:
    print(f"macos-min: {line}", file=sys.stderr)

  if fails:
    print(f"macos-min: FAIL (needs macOS {MIN})", file=sys.stderr)
    return 1

  print(f"macos-min: {len(examples)} examples, {len(syms)} C symbols: macOS {MIN}")
  return 0


if __name__ == "__main__":
  sys.exit(main())
