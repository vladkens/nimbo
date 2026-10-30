# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# /// script
# requires-python = ">=3.11"
# ///
"""Checks frame pacing in a real window.

Runs the pace example (examples/pace.rs, release build) traced, counts the frames presented
while its bar moves and the wakeups once it is still, prints both and fails when frames come
slower than PACE_MIN fps (55; at 60 Hz, frames paced from their end come at about 40) or the
still window wakes at all. Both are timed from its first frame, not from the launch: a CI
runner's cold first launch takes 0.1-1.2 s and at times more, which left the bar moving in a
still window timed from the launch.

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

# The bar moves for 3 s from the first frame: still well after 4 s of it.
SETTLE = 4.5
IDLE = 2
# How long the first frame may take after the launch, in s.
LAUNCH = 15


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
  if out.returncode != 0 or not out.stdout.strip().isdigit():
    raise SystemExit(f"pace: no context switches for pid {pid}: {out.stderr.strip() or out.stdout.strip()}")

  return int(out.stdout.strip())


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


def first_frame(log: Path, app: subprocess.Popen) -> float | None:
  """When the first frame in `log` was presented (unix time, s), once it is there; `None` when
  none comes within LAUNCH s or the app ends first. Only whole lines count: the app may be
  writing the last one."""
  end = time.monotonic() + LAUNCH
  while time.monotonic() < end and app.poll() is None:
    for line in log.read_text(errors="replace").split("\n")[:-1]:
      f = line.split()
      if len(f) >= 3 and f[1] == "presented":
        return float(f[2]) / 1000

    time.sleep(0.02)

  return None


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

  # The log is read as the app writes it, by a handle of its own (a shared one's offset would
  # move under the app's writes).
  with tempfile.TemporaryDirectory() as tmp:
    log = Path(tmp) / "trace.log"
    with log.open("w") as out:
      launched = time.time()
      app = subprocess.Popen([binary], stderr=out, env=env)

    try:
      first = first_frame(log, app)
      if first is not None:
        time.sleep(max(0.0, first + SETTLE - time.time()))
        c0 = switches(app.pid)
        time.sleep(IDLE)
        wakeups = switches(app.pid) - c0
    finally:
      app.kill()
      app.wait()

    text = log.read_text(errors="replace")

  lines = text.splitlines()
  if first is None:
    print(f"pace: FAIL (no frame within {LAUNCH} s, or the app ended first)", file=sys.stderr)
    print(text, file=sys.stderr)
    return 1

  rate = fps(text)
  fields = [line.split() for line in lines]
  times = [float(f[2]) / 1000 for f in fields if len(f) >= 3 and f[1] == "presented"]
  frames = len(times)
  start = (first - launched) * 1000
  summary = f"pace: {rate} fps while moving ({frames} frames), {wakeups} wakeups in {IDLE} s still"
  print(f"{summary}; the first frame {start:.0f} ms after the launch", flush=True)
  if float(rate) < float(least) or wakeups > 0:
    print(f"pace: FAIL (needs at least {least} fps and no wakeups)", file=sys.stderr)
    late = sum(t >= first + SETTLE for t in times)
    print(f"pace: {late} frames presented in the still window", file=sys.stderr)
    for line in lines:
      if " presented " not in line:
        print(line, file=sys.stderr)

    return 1

  return 0


if __name__ == "__main__":
  sys.exit(main())
