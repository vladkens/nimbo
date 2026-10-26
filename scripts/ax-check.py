# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# /// script
# requires-python = ">=3.11"
# ///
"""Checks what assistive technology sees of the examples in the macOS test VM.

Builds the examples (release) and the AX dumper (scripts/ax-dump.swift, Swift: the AX client API
is C and Objective-C, and the guest has no toolchain), copies them to the VM, starts each example
in the desktop session, then runs its scenario through the dumper: a dump, actions (press, step,
set a value, focus), the notifications seen, a dump again; takes a screenshot (to compare the
frames with the controls), quits the example and prints everything.

The VM is ~/Code/vm's macOS profile, prepared once with `./vm prepare macos --test-host`
(`./vm exec macos --gui` runs trusted for Accessibility). Nothing opens on this machine.

uv run scripts/ax-check.py [example...]      default: settings gallery
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

# The ops after the pid, run in order by ax-dump (see its header); at "shot" (else at the end)
# the script takes the screenshot.
SCENARIOS = {
  "settings": [
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
  ],
  "gallery": [
    "dump", "count",
    "press", "AXCheckBox:Off", "sleep", "0.5", "count",
    "press", "AXCheckBox:Live", "sleep", "1", "count",
    "hit", "AXSlider", "hit", "AXTextField:Search widgets",
    "inc", "AXSlider", "sleep", "0.5", "count",
    "focus", "AXTextField:Search widgets", "sleep", "0.5", "count", "focused",
    "set", "AXTextField:Search widgets", "Sliders", "sleep", "0.5", "count",
    "dump",
  ],
  # An open menu over the rows: a hit on its items finds them, not the rows under them.
  "lists": [
    "press", "AXButton:Actions", "sleep", "0.5",
    "hit", "AXMenuItem:Go to First", "hit", "AXMenuItem:Open", "shot",
  ],
}


def run(*cmd: str, check: bool = True) -> str:
  out = subprocess.run(cmd, capture_output=True, text=True)
  if check and out.returncode != 0:
    raise SystemExit(f"ax-check: {' '.join(cmd)} failed:\n{out.stderr or out.stdout}")

  return out.stdout


def main() -> None:
  examples = sys.argv[1:] or list(SCENARIOS)
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
  run("rsync", "-a", "-e", f"ssh -F {config}", "--rsync-path=/usr/bin/rsync", *bins, f"vm-macos:{GUEST}/")

  for example in examples:
    print(f"== {example}", flush=True)
    run(*ssh, "pkill", "-x", example, check=False)
    app = subprocess.Popen([str(VM), "exec", "macos", "--gui", "--", f"{GUEST}/{example}"])
    try:
      pid = ""
      for _ in range(40):
        pid = run(*ssh, "pgrep", "-nx", example, check=False).strip()
        if pid:
          break

        time.sleep(0.25)

      if not pid:
        raise SystemExit(f"ax-check: {example} did not start")

      time.sleep(1.5)
      ops = SCENARIOS.get(example, ["dump"])
      ops = ops if "shot" in ops else [*ops, "shot"]
      cut = ops.index("shot")
      for part in (ops[:cut], "shot", ops[cut + 1 :]):
        if part == "shot":
          shot = f"{GUEST}/{example}.png"
          run(str(VM), "exec", "macos", "--gui", "--", "screencapture", "-x", shot)
          run("scp", "-F", str(config), f"vm-macos:{shot}", str(OUT / f"{example}.png"))
          print(f"screenshot: {OUT / f'{example}.png'}")
        elif part:
          dump = [str(VM), "exec", "macos", "--gui", "--", f"{GUEST}/ax-dump", pid, *part]
          out = subprocess.run(dump, capture_output=True, text=True)
          print(out.stdout + out.stderr, flush=True)
    finally:
      run(*ssh, "pkill", "-x", example, check=False)
      app.wait(timeout=10)


if __name__ == "__main__":
  main()
