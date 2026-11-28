# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# /// script
# requires-python = ">=3.11"
# ///
"""Checks what assistive technology sees of the examples in a macOS test VM.

Builds the examples (release) and the AX dumper (scripts/ax-dump.swift, Swift: the AX client API
is C and Objective-C, and the guest has no toolchain), copies them to the VM, grants the dumper
Accessibility, starts each example on the guest's desktop, then runs its scenario through the
dumper: a dump, actions (press, step, set a value, focus), the notifications seen, a dump again;
takes a screenshot (to compare the frames with the controls), quits the example and prints
everything.

The VM is trivm's (https://github.com/vladkens/trivm, `trivm` on PATH): the one named in
TRIVM_VM, else `macos-27`, made with `trivm create macos-27`. The grant turns its SIP off once.
`trivm run macos-27 -- uv run scripts/ax-check.py` runs it on a clone deleted after. Nothing opens
on this machine.

uv run scripts/ax-check.py [example...]      default: messenger gallery (each scenario of theirs)
"""

import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VM = os.environ.get("TRIVM_VM", "macos-27")
OUT = ROOT / "target/ax"
GUEST = "/Users/user/nimbo"

# By the example and its arguments: the ops after the pid, run in order by ax-dump (see its
# header); at "shot" (else at the end) the script takes the screenshot.
SCENARIOS = {
  "messenger": [
    "dump", "count", "focused",
    "hit", "AXButton:Profile", "hit", "AXTextArea:Message",
    "press", "AXButton:Profile", "sleep", "0.5", "count",
    "focus", "AXTextArea:Message", "sleep", "0.5", "count", "focused",
    "set", "AXTextArea:Message", "12 * 7 + 1", "sleep", "0.5", "count",
    "press", "AXButton:Send", "sleep", "0.3", "count", "dump",
    "sleep", "1.5", "count", "dump",
    "focus", "AXTextField:Search", "set", "AXTextField:Search", "frame", "sleep", "0.5", "count",
    "dump",
    "shot",
    "press", "AXCloseButton", "sleep", "0.5", "count",
  ],
  # The gallery's blocks alone (`gallery <slug>`), each enabled then disabled.
  "gallery switch": [
    "dump", "count",
    "press", "AXCheckBox:Live", "sleep", "0.5", "count",
    "dump",
  ],
  "gallery slider": [
    "hit", "AXSlider", "inc", "AXSlider", "sleep", "0.5", "count",
    "dump",
  ],
  "gallery search-field": [
    "focus", "AXTextField:Search", "sleep", "0.5", "count", "focused",
    "set", "AXTextField:Search", "Sliders", "sleep", "0.5", "count",
    "dump",
  ],
  "gallery date-picker": [
    "dump",
    "focus", "AXTextField:Start", "sleep", "0.5", "count", "focused",
    "set", "AXTextField:Start", "1/2/2027", "sleep", "0.5", "count",
    "dump",
  ],
}


def run(*cmd: str, check: bool = True) -> str:
  rs = subprocess.run(cmd, capture_output=True, text=True)
  if check and rs.returncode != 0:
    raise SystemExit(f"ax-check: {' '.join(cmd)} failed:\n{rs.stderr or rs.stdout}")

  return rs.stdout


def guest(*cmd: str, check: bool = True) -> str:
  """`cmd` run in the guest; `desktop <program>` starts it on its desktop (see `trivm exec`)."""
  return run("trivm", "exec", VM, "--", *cmd, check=check)


def started(name: str) -> str:
  """The pid of the guest's process `name` once it runs, waiting up to 10 s."""
  for _ in range(40):
    pid = guest("pgrep", "-nx", name, check=False).strip()
    if pid:
      return pid

    time.sleep(0.25)

  raise SystemExit(f"ax-check: {name} did not start")


def drive(pid: str, ops: list[str]) -> None:
  """Runs `ops` through ax-dump on the app `pid` and prints what it says."""
  if not ops:
    return

  # Started by launchd, so that its own grant applies, which drops its output.
  out = f"{GUEST}/ax-dump.txt"
  guest("rm", "-f", out)
  guest("desktop", f"{GUEST}/ax-dump", "--out", out, pid, *ops)
  for _ in range(240):
    if not guest("pgrep", "-x", "ax-dump", check=False):
      break

    time.sleep(0.25)
  else:
    guest("pkill", "-x", "ax-dump", check=False)
    raise SystemExit(f"ax-check: ax-dump still runs after 60 s:\n{guest('cat', out)}")

  print(guest("cat", out), flush=True)


def shoot(name: str) -> None:
  """A screenshot of the guest's screen, to target/ax/<name>.png."""
  out = OUT / f"{name}.png"
  run("trivm", "shot", VM, str(out))
  print(f"screenshot: {out}")


def main() -> None:
  # An example with no scenario of its own gets a dump.
  asked = sys.argv[1:] or list(dict.fromkeys(k.split()[0] for k in SCENARIOS))
  runs = [k for e in asked for k in [k for k in SCENARIOS if k.split()[0] == e] or [e]]
  examples = list(dict.fromkeys(k.split()[0] for k in runs))
  OUT.mkdir(parents=True, exist_ok=True)
  args = [a for e in examples for a in ("--example", e)]
  subprocess.run(["cargo", "build", "--release", *args], cwd=ROOT, check=True)

  dumper = OUT / "ax-dump"
  src = ROOT / "scripts/ax-dump.swift"
  if not dumper.exists() or dumper.stat().st_mtime < src.stat().st_mtime:
    run("xcrun", "swiftc", "-target", "arm64-apple-macos14", "-O", str(src), "-o", str(dumper))

  guest("mkdir", "-p", GUEST)
  for b in [ROOT / "target/release/examples" / e for e in examples] + [dumper]:
    run("trivm", "cp", str(b), f"{VM}:{GUEST}/")

  # A grant is to the binary's signature, so a new build needs its own.
  run("trivm", "os", "grant", VM, f"{GUEST}/ax-dump", "accessibility")

  for key in runs:
    example, *argv = key.split()
    print(f"== {key}", flush=True)
    guest("pkill", "-x", example, check=False)
    try:
      guest("desktop", f"{GUEST}/{example}", *argv)
      pid = started(example)
      time.sleep(1.5)
      ops = SCENARIOS.get(key, ["dump"])
      cut = ops.index("shot") if "shot" in ops else len(ops)
      drive(pid, ops[:cut])
      shoot("-".join(key.split()))
      drive(pid, ops[cut + 1 :])
    finally:
      guest("pkill", "-x", example, check=False)


if __name__ == "__main__":
  main()
