# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# /// script
# requires-python = ">=3.11"
# ///
"""A laptop lid for a Linux VM: a uinput device with the lid switch, closed on SIGUSR1 and opened
on SIGUSR2, removed on SIGTERM or SIGINT. logind and UPower take a device with SW_LID as a lid
(systemd's 70-power-switch.rules tags it `power-switch` by `ID_INPUT_SWITCH`), so the VM's
checks of nimbo's `Change::Lid` have one. Run it as root, under
`systemd-inhibit --what=handle-lid-switch` so logind doesn't suspend the guest when it closes;
restart UPower after it starts when UPower finds lids only as it starts. Plain ioctls through
ctypes, from <linux/uinput.h> and <linux/input.h>.

sudo systemd-inhibit --what=handle-lid-switch uv run scripts/fake-lid.py
"""

import ctypes
import fcntl
import os
import signal
import struct
import sys
import time

EV_SYN, EV_SW = 0x00, 0x05
SYN_REPORT, SW_LID = 0, 0x00
BUS_VIRTUAL = 0x06


def ioc(direction: int, nr: int, size: int) -> int:
  """_IOC('U', nr, size) of the generic encoding (x86 and arm64 alike)."""
  return (direction << 30) | (size << 16) | (ord("U") << 8) | nr


class Setup(ctypes.Structure):
  """struct uinput_setup: struct input_id, a name, ff_effects_max."""

  _fields_ = [
    ("bustype", ctypes.c_uint16),
    ("vendor", ctypes.c_uint16),
    ("product", ctypes.c_uint16),
    ("version", ctypes.c_uint16),
    ("name", ctypes.c_char * 80),
    ("ff_effects_max", ctypes.c_uint32),
  ]


UI_SET_EVBIT = ioc(1, 100, 4)
UI_SET_SWBIT = ioc(1, 109, 4)
UI_DEV_SETUP = ioc(1, 3, ctypes.sizeof(Setup))
UI_DEV_CREATE = ioc(0, 1, 0)
UI_DEV_DESTROY = ioc(0, 2, 0)


def emit(fd: int, kind: int, code: int, value: int) -> None:
  """struct input_event: a timeval (two longs), type, code, value."""
  os.write(fd, struct.pack("llHHi", 0, 0, kind, code, value))


def lid(fd: int, closed: bool) -> None:
  emit(fd, EV_SW, SW_LID, int(closed))
  emit(fd, EV_SYN, SYN_REPORT, 0)
  print(f"fake-lid: {'closed' if closed else 'open'}", flush=True)


def main() -> int:
  fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
  fcntl.ioctl(fd, UI_SET_EVBIT, EV_SW)
  fcntl.ioctl(fd, UI_SET_SWBIT, SW_LID)
  setup = Setup(bustype=BUS_VIRTUAL, vendor=0x1, product=0x1, version=1, name=b"nimbo fake lid")
  fcntl.ioctl(fd, UI_DEV_SETUP, bytes(setup))
  fcntl.ioctl(fd, UI_DEV_CREATE)
  time.sleep(0.5)
  # Open to start with, as a laptop in use.
  lid(fd, False)
  signal.signal(signal.SIGUSR1, lambda *_: lid(fd, True))
  signal.signal(signal.SIGUSR2, lambda *_: lid(fd, False))
  done = []
  for sig in (signal.SIGTERM, signal.SIGINT):
    signal.signal(sig, lambda *_: done.append(1))

  print(f"fake-lid: ready, pid {os.getpid()}", flush=True)
  while not done:
    signal.pause()

  fcntl.ioctl(fd, UI_DEV_DESTROY)
  os.close(fd)
  return 0


if __name__ == "__main__":
  sys.exit(main())
