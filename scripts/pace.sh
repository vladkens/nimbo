#!/bin/sh
# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# Checks frame pacing in a real window: runs the pace example (examples/pace.rs, release build)
# traced, counts the frames presented while its bar moves and the wakeups once it is still,
# prints both and fails when frames come slower than PACE_MIN fps (55; at 60 Hz, frames paced from
# their end come at about 40) or the still window wakes at all.
#
#   scripts/pace.sh [binary]      default target/release/examples/pace
#
# It runs in the session's display: on Linux Wayland if WAYLAND_DISPLAY is set, else X11
# (`xvfb-run -a scripts/pace.sh` headless), or on Windows from Git Bash in the desktop session.
# Wakeups are the context switches of all its threads. macOS paces by its display link: not
# checked.
set -eu
if [ "$(uname -s)" = Darwin ]; then
  echo "pace: macOS paces by its display link, and opens no test windows here" >&2
  exit 2
fi

bin=${1:-target/release/examples/pace}
min=${PACE_MIN:-55}
log=$(mktemp)
app=''
cleanup() {
  [ -n "$app" ] && kill "$app" 2>/dev/null || true
  rm -f "$log"
}
trap cleanup EXIT INT TERM HUP

NIMBO_TRACE=1 "$bin" 2>"$log" &
app=$!

# Context switches so far, summed over the app's threads: /proc on Linux, the raw thread
# performance counters on Windows (`/proc/<pid>/winpid` is its Windows pid).
switches() {
  if [ -d "/proc/$app/task" ]; then
    cat /proc/"$app"/task/*/status | awk '/ctxt_switches/ { n += $2 } END { print n }'
  else
    powershell -NoProfile -Command "(Get-CimInstance Win32_PerfRawData_PerfProc_Thread \
      -Filter 'IDProcess=$(cat "/proc/$app/winpid")' | Measure-Object ContextSwitchesPersec -Sum).Sum" |
      tr -d '\r'
  fi
}

# The bar moves for 3 s from the first frame: still well after 4 s.
sleep 4.5
idle=2
c0=$(switches)
sleep "$idle"
wakeups=$(($(switches) - c0))

kill "$app" 2>/dev/null || true
wait "$app" 2>/dev/null || true
app=''

# Frames from half a second after the first to the end of the run: the start's settled.
fps=$(awk '$2 == "presented" { t = $3; if (!t0) t0 = t; if (t >= t0 + 500 && t <= t0 + 2950) { if (!a) a = t; b = t; n++ } }
  END { if (n > 1) printf "%.1f", (n - 1) * 1000 / (b - a); else print 0 }' "$log")
frames=$(grep -c ' presented ' "$log" || true)
echo "pace: $fps fps while moving ($frames frames), $wakeups wakeups in ${idle} s still"
fail=$(awk -v f="$fps" -v m="$min" -v w="$wakeups" 'BEGIN { print (f < m || w > 0) }')
if [ "$fail" = 1 ]; then
  echo "pace: FAIL (needs at least $min fps and no wakeups)" >&2
  grep -v ' presented ' "$log" >&2 || true
  exit 1
fi
