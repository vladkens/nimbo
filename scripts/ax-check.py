# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# /// script
# requires-python = ">=3.11"
# ///
"""Checks what assistive technology sees of the examples in the macOS test VM.

Builds the scenarios' examples (release) and the AX dumper (scripts/ax-dump.swift, Swift: the AX
client API is C and Objective-C, and the guest has no toolchain), copies them to the VM, starts
each scenario's example with its arguments (a window of a bigger demo: `dictation settings`) in
the desktop session, then runs its ops through the dumper: a dump, actions (press, step, set a
value, focus), the notifications seen, a dump again; takes a screenshot (to compare the frames
with the controls), quits the example and prints everything. A name that is no scenario runs
that example with no arguments and dumps it.

The VM is ~/Code/vm's macOS profile, prepared once with `./vm prepare macos --test-host`
(`./vm exec macos --gui` runs trusted for Accessibility). Nothing opens on this machine.

uv run scripts/ax-check.py [scenario...]     default: every scenario
"""

import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VM = Path.home() / "Code/vm/vm"
OUT = ROOT / "target/ax"
GUEST = "nimbo"

# Each scenario: the example, its arguments, and the ops after the pid, run in order by ax-dump
# (see its header); at "shot" (else at the end) the script takes the screenshot.
SCENARIOS: dict[str, tuple[str, list[str], list[str]]] = {
  "settings": ("dictation", ["settings"], [
    "dump", "count", "focused",
    "hit", "AXCheckBox:Open at login", "hit", "AXRadioButton:Beta",
    "press", "AXCheckBox:Open at login", "sleep", "1", "count",
    "press", "AXButton:Reset", "sleep", "0.5",
    "focus", "AXPopUpButton:Language", "sleep", "0.5", "count", "focused",
    "dump",
    "press", "AXPopUpButton:Language", "sleep", "0.5", "count", "dump",
    "press", "AXMenuItem:English", "sleep", "0.5", "count",
    "dump",
    "watch", "AXCheckBox:Open at login",
    "press", "AXRadioButton:Appearance", "sleep", "0.5", "count",
    "inc", "AXSlider:Text size", "sleep", "0.5", "count",
    "dec", "AXSlider:Text size", "dec", "AXSlider:Text size", "sleep", "0.5", "count",
    "dump",
    "press", "AXRadioButton:Advanced", "sleep", "0.5", "count",
    "press", "AXButton:Clear…", "sleep", "0.5", "count", "focused", "dump",
    "press", "AXButton:Reset All…", "press", "AXButton:Clear", "sleep", "0.5", "count",
    "dump",
    "shot",
    "press", "AXCloseButton", "sleep", "0.5", "count",
  ]),
  "gallery": ("gallery", [], [
    "dump", "count",
    "press", "AXCheckBox:Off", "sleep", "0.5", "count",
    "press", "AXCheckBox:Live", "sleep", "1", "count",
    "hit", "AXSlider", "hit", "AXTextField:Search widgets",
    "inc", "AXSlider", "sleep", "0.5", "count",
    "focus", "AXTextField:Search widgets", "sleep", "0.5", "count", "focused",
    "set", "AXTextField:Search widgets", "Sliders", "sleep", "0.5", "count",
    "dump",
  ]),
  # The browser's row cut by the list's bottom edge scrolled into view (its whole frame at its
  # centre); an open menu over the rows: a hit on its items finds them, not the rows under them.
  "files": ("files", [], [
    "scroll", "AXRow:Item 000019 678 KB", "sleep", "0.5", "count",
    "hit", "AXRow:Item 000019 678 KB",
    "press", "AXButton:Actions", "sleep", "0.5",
    "hit", "AXMenuItem:Go to First", "hit", "AXMenuItem:Open", "shot",
  ]),
}


def run(*cmd: str, check: bool = True) -> str:
  rs = subprocess.run(cmd, capture_output=True, text=True)
  if check and rs.returncode != 0:
    raise SystemExit(f"ax-check: {' '.join(cmd)} failed:\n{rs.stderr or rs.stdout}")

  return rs.stdout


def gui(*cmd: str) -> list[str]:
  """`cmd` run in the guest's desktop session."""
  return [str(VM), "exec", "macos", "--gui", "--", *cmd]


def started(ssh: list[str], example: str) -> str:
  """The example's pid in the guest once it runs, waiting up to 10 s."""
  for _ in range(40):
    pid = run(*ssh, "pgrep", "-nx", example, check=False).strip()
    if pid:
      return pid

    time.sleep(0.25)

  raise SystemExit(f"ax-check: {example} did not start")


def drive(pid: str, ops: list[str]) -> None:
  """Runs `ops` through ax-dump on the app `pid` and prints what it says."""
  if not ops:
    return

  rs = subprocess.run(gui(f"{GUEST}/ax-dump", pid, *ops), capture_output=True, text=True)
  print(rs.stdout + rs.stderr, flush=True)


def shoot(config: Path, name: str) -> None:
  """A screenshot of the guest's screen, to target/ax/<name>.png."""
  shot, out = f"{GUEST}/{name}.png", OUT / f"{name}.png"
  run(*gui("screencapture", "-x", shot))
  run("scp", "-F", str(config), f"vm-macos:{shot}", str(out))
  print(f"screenshot: {out}")


def main() -> None:
  names = sys.argv[1:] or list(SCENARIOS)
  scenarios = {n: SCENARIOS.get(n, (n, [], ["dump"])) for n in names}
  examples = sorted({example for example, _, _ in scenarios.values()})
  OUT.mkdir(parents=True, exist_ok=True)
  args = [a for e in examples for a in ("--example", e)]
  subprocess.run(["cargo", "build", "--release", *args], cwd=ROOT, check=True)

  dumper = OUT / "ax-dump"
  src = ROOT / "scripts/ax-dump.swift"
  if not dumper.exists() or dumper.stat().st_mtime < src.stat().st_mtime:
    run("xcrun", "swiftc", "-target", "arm64-apple-macos14", "-O", str(src), "-o", str(dumper))

  config = Path(tempfile.mkdtemp()) / "ssh_config"
  config.write_text(run(str(VM), "ssh-config", "macos"))
  ssh = ["ssh", "-F", str(config), "vm-macos"]
  run(*ssh, "mkdir", "-p", GUEST)
  bins = [str(ROOT / "target/release/examples" / e) for e in examples] + [str(dumper)]
  rsync = ["rsync", "-a", "-e", f"ssh -F {config}", "--rsync-path=/usr/bin/rsync"]
  run(*rsync, *bins, f"vm-macos:{GUEST}/")

  for name, (example, argv, ops) in scenarios.items():
    print(f"== {name}", flush=True)
    run(*ssh, "pkill", "-x", example, check=False)
    app = subprocess.Popen(gui(f"{GUEST}/{example}", *argv))
    try:
      pid = started(ssh, example)
      time.sleep(1.5)
      cut = ops.index("shot") if "shot" in ops else len(ops)
      drive(pid, ops[:cut])
      shoot(config, name)
      drive(pid, ops[cut + 1 :])
    finally:
      run(*ssh, "pkill", "-x", example, check=False)
      app.wait(timeout=10)


if __name__ == "__main__":
  main()
