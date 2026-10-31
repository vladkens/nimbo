# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# /// script
# requires-python = ">=3.11"
# ///
"""Checks the global hotkeys of examples/hotkeys.rs in the macOS test VM, with keys posted.

Builds the example (release), the event poster (scripts/hotkey-post.swift) and the AX dumper
(scripts/ax-dump.swift), copies them to the VM, opens an empty document in TextEdit and starts the
example in the desktop session. Each scenario brings TextEdit to the front, empties its text,
posts key events at the HID level (below the example's event tap) and compares the edges the
example printed to stderr (`hotkeys: Down 0`) and the text TextEdit got with what is expected:
a hotkey's keys never reach TextEdit, other typing does, unchanged. Its modes: `carbon`, Carbon's
hotkeys alone (the tap not started: chords only, the keys pass); `tap`, with the event tap (Start
pressed through AX; `./vm exec macos --gui` runs trusted for Accessibility); `noax`, the example
in an app bundle opened by Launch Services, its own responsible process with no grants, Start
pressed: the tap fails, Carbon's hotkeys work. At the end every modifier and key used is
released.

The VM is ~/Code/vm's macOS profile, prepared once with `./vm prepare macos --test-host`. Nothing
opens on this machine.

uv run scripts/hotkey-check.py [--bin <hotkeys binary>] [carbon|tap|noax...]   default: all
"""

import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VM = Path.home() / "Code/vm/vm"
OUT = ROOT / "target/hotkey"
GUEST = "nimbo"
DOC = f"{GUEST}/typed.txt"
BUNDLE = "HotkeysCheck.app"

# The demo's defaults: hotkeys fn (0), ⌥Space (1), F2 (2); keys Z (0), X (1), 1 (2).
# (name, poster ops, {mode: (edges, text)}); a text of None is printed, not compared (the
# characters an ⌥ key types).
SCENARIOS = [
  ("typing", "z+ z- x+ x- 1+ 1-", {"carbon": ([], "zx1"), "tap": ([], "zx1")}),
  # A posted autorepeat types nothing in TextEdit, nor does Carbon repeat on it: the repeats'
  # checks below are the tap's edges.
  ("typing, Z repeated", "z+ z* z* z-", {"carbon": ([], "z"), "tap": ([], "z")}),
  ("fn alone", "fn+ @0.4 fn-", {"carbon": ([], ""), "tap": (["Down 0", "Up 0"], "")}),
  ("fn-Z", "fn+ z+ z- fn-", {"carbon": ([], "z"), "tap": (["Down 0", "Key 0", "Up 0"], "")}),
  (
    "fn-Z-X: every key told",
    "fn+ z+ z- x+ x- fn-",
    {"carbon": ([], "zx"), "tap": (["Down 0", "Key 0", "Key 1", "Up 0"], "")},
  ),
  (
    "Z held past fn's release",
    "fn+ z+ fn- z* z* z-",
    {"carbon": ([], "z"), "tap": (["Down 0", "Key 0", "Up 0"], "")},
  ),
  (
    "fn-J: the user's chord",
    "fn+ j+ j- fn-",
    {"carbon": ([], "j"), "tap": (["Down 0", "Aborted 0"], "j")},
  ),
  (
    "⌥Space",
    "alt+ space+ space- alt-",
    {"carbon": (["Down 1", "Up 1"], ""), "tap": (["Down 1", "Up 1"], "")},
  ),
  (
    "⌥Space + X",
    "alt+ space+ state alt state space x+ x- space- alt-",
    {"carbon": (["Down 1", "Up 1"], None), "tap": (["Down 1", "Key 1", "Up 1"], "")},
  ),
  (
    "⌥Space + Z, posted from the HID state",
    "source hid alt+ space+ state alt state space z+ z- space- alt-",
    {"carbon": (["Down 1", "Up 1"], None), "tap": (["Down 1", "Key 0", "Up 1"], "")},
  ),
  (
    # Z's press right before Space's release, with no gap: the tap may judge Z late.
    "⌥Space + Z, Space up at once",
    "alt+ space+ gap 0 z+ space- gap 0.025 z- alt-",
    {"carbon": (["Down 1", "Up 1"], None), "tap": (["Down 1", "Key 0", "Up 1"], "")},
  ),
  (
    "F2 + 1",
    "f2+ state f2 1+ 1- f2-",
    {"carbon": (["Down 2", "Up 2"], "1"), "tap": (["Down 2", "Key 2", "Up 2"], "")},
  ),
  (
    "F2 + 1, posted from the HID state",
    "source hid f2+ state f2 1+ 1- f2-",
    {"carbon": (["Down 2", "Up 2"], "1"), "tap": (["Down 2", "Key 2", "Up 2"], "")},
  ),
  (
    "F2 repeats",
    "f2+ f2* f2* f2-",
    {
      "carbon": (["Down 2", "Up 2"], ""),
      "tap": (["Down 2", "Down 2 repeat", "Down 2 repeat", "Up 2"], ""),
    },
  ),
  (
    # F2's press unseen by the tap: Carbon's alone (when the tap was disabled); 1 is typing.
    "F2's press missed by the tap, then 1",
    "!f2+ @0.3 1+ 1- @0.1 f2-",
    {"carbon": (["Down 2", "Up 2"], "1"), "tap": (["Down 2", "Up 2"], "1")},
  ),
  (
    # Secure Input on while F2 is held keeps its release from the tap (and Carbon, which never
    # heard the tap's press): the hotkey thread looks, finds F2 up and tells its release; Z after
    # it is typing.
    "F2 released under Secure Input, then Z",
    "f2+ @0.1 secure on @0.1 f2- @0.7 secure off @0.1 z+ z- @0.2",
    {"carbon": (["Down 2", "Up 2"], "z"), "tap": (["Down 2", "Up 2"], "z")},
  ),
  ("typing after", "z+ z- x+ x-", {"carbon": ([], "zx"), "tap": ([], "zx")}),
]


def run(*cmd: str, check: bool = True) -> str:
  rs = subprocess.run(cmd, check=False, capture_output=True, text=True)
  if check and rs.returncode != 0:
    raise SystemExit(f"hotkey-check: {' '.join(cmd)} failed:\n{rs.stderr or rs.stdout}")

  return rs.stdout


def gui(*cmd: str) -> list[str]:
  """`cmd` run in the guest's desktop session."""
  return [str(VM), "exec", "macos", "--gui", "--", *cmd]


def pid_of(ssh: list[str], name: str) -> str:
  """The pid of `name` in the guest once it runs, waiting up to 10 s."""
  for _ in range(40):
    pid = run(*ssh, "pgrep", "-nx", name, check=False).strip()
    if pid:
      return pid

    time.sleep(0.25)

  raise SystemExit(f"hotkey-check: {name} did not start")


def swift(name: str) -> Path:
  """scripts/<name>.swift built into target/hotkey, unless it is up to date."""
  out, src = OUT / name, ROOT / f"scripts/{name}.swift"
  if not out.exists() or out.stat().st_mtime < src.stat().st_mtime:
    run("xcrun", "swiftc", "-target", "arm64-apple-macos14", "-O", str(src), "-o", str(out))

  return out


class Edges:
  """The example's stderr, read on a thread: its edges, and every line for the log."""

  def __init__(self, stream):
    self.lines: list[str] = []
    self.lock = threading.Lock()
    threading.Thread(target=self.read, args=(stream,), daemon=True).start()

  def read(self, stream) -> None:
    for line in stream:
      with self.lock:
        self.lines.append(line.rstrip("\n"))

  def mark(self) -> int:
    with self.lock:
      return len(self.lines)

  def since(self, at: int) -> list[str]:
    with self.lock:
      return self.lines[at:]


def post(*ops: str) -> str:
  rs = subprocess.run(
    gui(f"{GUEST}/hotkey-post", *ops), check=False, capture_output=True, text=True
  )
  return rs.stdout + rs.stderr


def bundle(binary: Path) -> Path:
  """The example in an app bundle of its own (ad-hoc signed), for `noax`."""
  app = OUT / BUNDLE
  (app / "Contents/MacOS").mkdir(parents=True, exist_ok=True)
  (app / "Contents/MacOS" / binary.name).write_bytes(binary.read_bytes())
  (app / "Contents/MacOS" / binary.name).chmod(0o755)
  keys = {
    "CFBundleIdentifier": "com.example.nimbo-hotkeys-check",
    "CFBundleExecutable": binary.name,
    "CFBundlePackageType": "APPL",
  }
  entries = "".join(f"<key>{k}</key><string>{v}</string>" for k, v in keys.items())
  plist = f'<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict>{entries}</dict>'
  (app / "Contents/Info.plist").write_text(plist + "</plist>")
  run("codesign", "-s", "-", "--force", str(app))
  return app


def check(mode: str, ssh: list[str], binary: str, doc: str) -> int:
  """Runs the scenarios with the example started in `mode`; the number that failed."""
  print(f"== {mode}", flush=True)
  run(*ssh, "pkill", "-x", binary, check=False)
  pipe = {"stdout": subprocess.PIPE, "stderr": subprocess.STDOUT, "text": True}
  if mode == "noax":
    err = f"{run(*ssh, 'pwd').strip()}/{GUEST}/hotkeys.err"
    run(*ssh, "sh", "-c", f": > {err}")
    app = subprocess.Popen([*ssh, "tail", "-f", err], **pipe)
    run(*gui("open", "-n", "--stderr", err, f"{GUEST}/{BUNDLE}"))
  else:
    app = subprocess.Popen(gui(f"{GUEST}/{binary}"), **pipe)

  edges = Edges(app.stdout)
  failed = 0
  try:
    pid = pid_of(ssh, binary)
    time.sleep(1.5)
    ax = [f"{GUEST}/ax-dump", pid]
    # The window on the screen, scrolled to its end: Start and the status in the AX tree.
    print(post("frame", pid, "40", "40", "560", "860", "scroll", pid, "-30").strip())
    if mode != "carbon":
      print(run(*gui(*ax, "press", "AXButton:Start", "sleep", "1")).strip())

    status = run(*gui(*ax, "dump")).splitlines()
    at = next((i for i, line in enumerate(status) if 'value="Status"' in line), len(status))
    for line in status[at:]:
      if "AXStaticText" in line and "chars=" not in line:
        print("status:", line.strip().split(" @")[0])

    for name, ops, want in SCENARIOS:
      edges_want, text_want = want["tap" if mode == "tap" else "carbon"]
      at = edges.mark()
      said = post("front", doc, "clear", doc, "@0.2", *ops.split(), "@0.5", "text", doc)
      time.sleep(0.3)
      got = [line.removeprefix("hotkeys: ") for line in edges.since(at)]
      got_edges = [line for line in got if not line.startswith("raw:")]
      text = next((s for s in said.splitlines() if s.startswith("text: ")), "text: none")
      text = text.removeprefix("text: ").strip('"')
      ok = got_edges == edges_want and (text_want is None or text == text_want)
      failed += not ok
      print(f"{'ok  ' if ok else 'FAIL'} {name}: edges {got_edges}, text {text!r}", flush=True)
      if not ok:
        print(f"     want edges {edges_want}, text {text_want!r}")

      for line in said.splitlines():
        if not line.startswith(("text: ", "front ", "clear: ")):
          print("     poster:", line)

      for line in got:
        if line.startswith("raw:"):
          print("    ", line)
  finally:
    print(post("release").strip())
    run(*ssh, "pkill", "-x", binary, check=False)
    if mode == "noax":
      app.terminate()

    app.wait(timeout=10)

  return failed


def main() -> None:
  args = sys.argv[1:]
  binary = None
  if args[:1] == ["--bin"]:
    binary, args = Path(args[1]), args[2:]

  modes = args or ["carbon", "tap", "noax"]
  OUT.mkdir(parents=True, exist_ok=True)
  if binary is None:
    subprocess.run(["cargo", "build", "--release", "--example", "hotkeys"], cwd=ROOT, check=True)
    binary = ROOT / "target/release/examples/hotkeys"

  tools = [swift("hotkey-post"), swift("ax-dump")]
  if "noax" in modes:
    tools.append(bundle(binary))

  config = Path(tempfile.mkdtemp()) / "ssh_config"
  config.write_text(run(str(VM), "ssh-config", "macos"))
  ssh = ["ssh", "-F", str(config), "vm-macos"]
  run(*ssh, "mkdir", "-p", GUEST)
  rsync = ["rsync", "-a", "-e", f"ssh -F {config}", "--rsync-path=/usr/bin/rsync"]
  run(*rsync, str(binary), *map(str, tools), f"vm-macos:{GUEST}/")
  run(*ssh, "pkill", "-x", "TextEdit", check=False)
  run(*ssh, "sh", "-c", f": > {DOC}")
  run(*gui("open", "-a", "TextEdit", DOC))
  doc = pid_of(ssh, "TextEdit")
  time.sleep(2)
  failed = 0
  try:
    for mode in modes:
      failed += check(mode, ssh, binary.name, doc)
  finally:
    run(*ssh, "pkill", "-x", "TextEdit", check=False)

  print(f"hotkey-check: {'all passed' if not failed else f'{failed} failed'}")
  sys.exit(1 if failed else 0)


if __name__ == "__main__":
  main()
