# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# /// script
# requires-python = ">=3.11"
# ///
"""Checks frame pacing in a real window.

Runs the pace example (examples/pace.rs, release build) traced, counts the frames presented
while its bar moves and the wakeups once it is still, prints both and fails when frames come
slower than PACE_MIN fps (55; at 60 Hz, frames paced from their end come at about 40) or the
still window wakes at all.

It runs in the session's display: on Linux Wayland if WAYLAND_DISPLAY is set, else X11
(`xvfb-run -a uv run scripts/pace.py` headless), or on Windows in the desktop session. Wakeups
are the context switches of all its threads: /proc on Linux, the raw thread performance
counters on Windows. macOS paces by its display link: not checked.

uv run scripts/pace.py [binary]      default target/release/examples/pace
"""

import os
import platform
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

# The bar moves for 3 s from the first frame: still well after 4 s.
SETTLE = 4.5
IDLE = 2


def switches(pid: int) -> int:
  """Context switches so far, summed over the process's threads."""
  tasks = Path(f"/proc/{pid}/task")
  if tasks.is_dir():
    n = 0
    for status in tasks.glob("*/status"):
      for line in status.read_text().splitlines():
        if "ctxt_switches" in line:
          n += int(line.split()[1])

    return n

  query = (
    f"(Get-CimInstance Win32_PerfRawData_PerfProc_Thread -Filter 'IDProcess={pid}'"
    " | Measure-Object ContextSwitchesPersec -Sum).Sum"
  )
  ps = ["powershell", "-NoProfile", "-Command", query]
  out = subprocess.run(ps, capture_output=True, text=True)
  return int(out.stdout.strip() or 0)


# The measured part of the run, in ms after the first frame: the bar moves for 3 s.
WINDOW = (500, 2950)


def fps(log: str) -> str:
  """Frames per second from half a second after the first to the end of the run (the start's
  settled), counted over that whole window: an animation that stops early comes out slow.

  >>> trace = lambda ms: chr(10).join(f"nimbo: presented {k * 1000 / 60:.3f}" for k in range(ms * 60 // 1000))
  >>> fps(trace(3000))
  '60.0'
  >>> fps(trace(750))
  '6.1'
  """
  t0, n = None, 0
  for line in log.splitlines():
    f = line.split()
    if len(f) < 3 or f[1] != "presented":
      continue

    t = float(f[2])
    t0 = t if t0 is None else t0
    if WINDOW[0] <= t - t0 < WINDOW[1]:
      n += 1

  return f"{n * 1000 / (WINDOW[1] - WINDOW[0]):.1f}"


def main() -> int:
  if platform.system() == "Darwin":
    print("pace: macOS paces by its display link, and opens no test windows here", file=sys.stderr)
    return 2

  # A signal ends the run as an error would: the window closed, the log gone.
  for name in ("SIGTERM", "SIGHUP"):
    if hasattr(signal, name):
      signal.signal(getattr(signal, name), lambda *_: sys.exit(1))

  binary = sys.argv[1] if len(sys.argv) > 1 else "target/release/examples/pace"
  least = os.environ.get("PACE_MIN", "55")
  env = {**os.environ, "NIMBO_TRACE": "1"}

  with tempfile.TemporaryFile("w+") as log:
    app = subprocess.Popen([binary], stderr=log, env=env)
    try:
      time.sleep(SETTLE)
      c0 = switches(app.pid)
      time.sleep(IDLE)
      wakeups = switches(app.pid) - c0
    finally:
      app.kill()
      app.wait()

    log.seek(0)
    text = log.read()

  rate = fps(text)
  lines = text.splitlines()
  frames = sum(" presented " in line for line in lines)
  summary = f"pace: {rate} fps while moving ({frames} frames), {wakeups} wakeups in {IDLE} s still"
  print(summary, flush=True)
  if float(rate) < float(least) or wakeups > 0:
    print(f"pace: FAIL (needs at least {least} fps and no wakeups)", file=sys.stderr)
    for line in lines:
      if " presented " not in line:
        print(line, file=sys.stderr)

    return 1

  return 0


if __name__ == "__main__":
  sys.exit(main())
