# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
"""Makes an app's release files from its Cargo.toml: the macOS app and DMG, the Windows setup,
the AppImage and .deb, and the signed update feed (src/update.rs).

The config is the package's `[package.metadata.nimbo]` (`id`, `name`, `icon`, a 1024 px PNG,
`comment`, the tables `macos`, `linux` and `windows`), its version, binary and `repository`, read
by `cargo metadata --no-deps`: a plain `python3` (3.9+) with the standard library runs this, beside
cargo and the tools below. The binary is built already, by default
`<target dir>/<triple>/dist/<bin>`, the target this machine's arch on the command's OS. The files,
in `--out` (`<target dir>/distrib`), are `<Name>-v<version>-<platform>.<ext>`, or
`<Name>-<label>-…` with `--label` (`dev`, or a release's `vX.Y.Z`). `app` writes
`<target dir>/<Name>.app`, which `dmg` packs, or the one `--app` gives, never signed again. `dist`
runs this OS's steps locally: `app` and `dmg`, `setup`, or `appimage` and `deb`. On a Mac codesign
signs, with a timestamp only in CI (`$CI`), so a Developer ID signs offline. One table per OS:

  [package.metadata.nimbo]
  id = "me.vladkens.wavo"
  name = "Wavo"
  icon = "assets/icon.png"
  comment = "Local dictation"
  [package.metadata.nimbo.macos]
  accessory = true                 # a tray app: no Dock icon, never ended by macOS on its own
  entitlements = "assets/wavo.entitlements"
  plist.NSMicrophoneUsageDescription = "…"
  plist.LSEnvironment.MallocSpaceEfficient = "1"
  [package.metadata.nimbo.windows]
  app-id = "314a56a8-8a8b-4c18-954d-b930203f086d"
  copyright = "© 2026 vladkens"     # the exe's LegalCopyright, by default "© <publisher>"
  [package.metadata.nimbo.linux]
  categories = ["Utility"]
  deb-package = "wavo"
  deb-depends = ["libasound2 | libasound2t64"]

In CI three build jobs (macOS, Windows, Linux) only `cargo build` both arches and copy each binary
to `target/distrib/<triple>/<bin>[.exe]`; one Ubuntu job (`apt-get install nsis hfsprogs`)
downloads them there and runs `release` with the secrets in its env: anything else there fails it
(the build jobs ran dependencies' build scripts). It writes beside them, per triple, the DMG, the
setup, or the AppImage and .deb, then with `--tag` update.txt, its urls
`<repository>/releases/download/<tag>/<file>`. An app's Makefile finds nimbo's checkout, the rev
its Cargo.lock pins, by `cargo metadata`:

  NIMBO = $(shell cargo metadata --format-version 1 --locked | python3 -c 'import json, os, sys; \
    print(next(os.path.dirname(p["manifest_path"]) for p in json.load(sys.stdin)["packages"] \
    if p["name"] == "nimbo"))')
  release:
  	python3 $(NIMBO)/scripts/pack.py release target/distrib --label $(LABEL) \
  	  $(if $(TAG),--tag $(TAG))

Off macOS `app` and `dmg` use no Apple tool: rcodesign signs the app with APPLE_CERTIFICATE (a
base64 .p12) and APPLE_CERTIFICATE_PASSWORD for notarization, else ad hoc; the DMG is an HFS+
image by `mkfs.hfsplus` (hfsprogs) and libdmg-hfsplus's `hfsplus` and `dmg` (from $NIMBO_TOOLS,
else fetched), then rcodesign signs it and, with APPLE_API_KEY (the .p8's text), APPLE_API_KEY_ID
and APPLE_API_ISSUER, notarizes and staples it. `setup` is NSIS's, by makensis (Homebrew's on a
Mac). A `v*` label is a release: a missing secret fails it.

`feed` writes update.txt: `version <v>`, a line per file, `<platform> <size> <sha512> <url>`, and
last `sig <hex>`, the Ed25519 signature of every byte before it. A file's platform is in its name
(`linux-aarch64`, `windows-x64`) or given, `<platform>=<file>`; its url is `--url` (`{version}` the
version) and its name. The key, `--key <file>` or `$UPDATE_KEY`, is its seed in hex or a PEM.
`feed-key` makes the one key, in memory only: the repo's secret UPDATE_KEY (gh), the private key on
the clipboard to keep (Bitwarden), the public key printed in hex, the app's `Feed::key`. Both need
OpenSSL 3 (macOS's own openssl is LibreSSL).

python3 scripts/pack.py release <dir> --label <dev|vX.Y.Z> [--tag <tag>] [--manifest <Cargo.toml>]
python3 scripts/pack.py dist|app|setup|appimage|deb [--target <triple>] [--bin <path>] [--label <l>] [--out <dir>] [--manifest <Cargo.toml>]
python3 scripts/pack.py dmg [--app <Name>.app] [--target <triple>] [--label <l>] [--out <dir>] [--manifest <Cargo.toml>]
python3 scripts/pack.py feed [--key <file>] --url <base> [--out <dir>] [<platform>=]<file>...
python3 scripts/pack.py feed [--key <file>] --public-key
python3 scripts/pack.py feed-key [--repo <owner/name>]
"""

from __future__ import annotations

import argparse
import array
import base64
import functools
import hashlib
import json
import os
import platform as host
import plistlib
import re
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import uuid
import zlib
from dataclasses import dataclass
from itertools import accumulate, islice
from operator import add, itemgetter, mul, sub
from pathlib import Path

# The triple's OS of each command, for the default target.
OS = {
  "app": "apple-darwin",
  "dmg": "apple-darwin",
  "setup": "pc-windows-msvc",
  "appimage": "unknown-linux-gnu",
  "deb": "unknown-linux-gnu",
}
# The commands `dist` runs on each OS, in order.
DIST = {"darwin": ("app", "dmg"), "win32": ("setup",), "linux": ("appimage", "deb")}
# The app and DMG by codesign and hdiutil, else by rcodesign and libdmg-hfsplus.
MAC = sys.platform == "darwin"


@dataclass
class Config:
  id: str
  name: str
  icon: Path
  comment: str
  version: str
  bin: str
  macos: dict
  linux: dict
  windows: dict
  publisher: str
  authors: list
  root: Path
  target: Path
  label: str | None = None
  repository: str = ""


def stem(cfg: Config) -> str:
  """A file's name before its platform: `<Name>-<label>`, else `<Name>-v<version>`.

  >>> cfg = Config("me.a", "A", Path("i.png"), "", "1.2.0", "a", {}, {}, {}, "", [], Path("."),
  ...   Path("target"))
  >>> stem(cfg), stem(Config(**{**cfg.__dict__, "label": "dev"}))
  ('A-v1.2.0', 'A-dev')
  """
  return f"{cfg.name}-{cfg.label or 'v' + cfg.version}"


def run(*args, **kw) -> subprocess.CompletedProcess:
  return subprocess.run([str(a) for a in args], check=True, **kw)


def load(manifest: Path) -> Config:
  if not shutil.which("cargo"):
    sys.exit("pack: cargo not found: `cargo metadata` reads the config")

  args = ["--format-version", "1", "--no-deps", "--manifest-path", manifest]
  return parse(json.loads(run("cargo", "metadata", *args, capture_output=True).stdout), manifest)


def parse(meta: dict, manifest: Path) -> Config:
  """The config of the package at `manifest` in `cargo metadata`'s output.

  >>> pkg = {"name": "a", "version": "1.2.0", "manifest_path": "/x/Cargo.toml", "default_run": None}
  >>> pkg["targets"] = [{"name": "a", "kind": ["bin"]}]
  >>> pkg["metadata"] = {"nimbo": {"id": "me.a", "name": "A", "icon": "i.png"}}
  >>> c = parse({"packages": [pkg]}, Path("/x/Cargo.toml"))
  >>> c.bin, c.version, c.icon.name, c.macos
  ('a', '1.2.0', 'i.png', {})
  """
  manifest = manifest.resolve()
  pkg = next((p for p in meta["packages"] if Path(p["manifest_path"]).resolve() == manifest), None)
  if pkg is None:
    sys.exit(f"pack: {manifest} is no package")

  nimbo = (pkg.get("metadata") or {}).get("nimbo")
  if nimbo is None:
    sys.exit(f"pack: {manifest} has no [package.metadata.nimbo]")

  missing = [k for k in ("id", "name", "icon") if not isinstance(nimbo.get(k), str)]
  if missing:
    sys.exit(f"pack: [package.metadata.nimbo] needs {', '.join(missing)}")

  # The one binary, else the one cargo runs, else the package's own.
  bins = [t["name"] for t in pkg["targets"] if "bin" in t["kind"]]
  bin = bins[0] if len(bins) == 1 else pkg.get("default_run") or pkg["name"]
  authors = pkg.get("authors") or []
  return Config(
    id=nimbo["id"],
    name=nimbo["name"],
    icon=manifest.parent / nimbo["icon"],
    comment=nimbo.get("comment", ""),
    version=pkg["version"],
    bin=bin,
    macos=nimbo.get("macos", {}),
    linux=nimbo.get("linux", {}),
    windows=nimbo.get("windows", {}),
    # The config's, else the first author without the mail.
    publisher=nimbo.get("publisher") or next(iter(authors), "").split(" <")[0],
    authors=authors,
    root=manifest.parent,
    target=Path(meta.get("target_directory") or manifest.parent / "target"),
    repository=(pkg.get("repository") or "").rstrip("/").removesuffix(".git"),
  )


def arch(target: str) -> str:
  """The arch of a target triple, as Rust names it.

  >>> arch("aarch64-apple-darwin"), arch("x86_64-pc-windows-msvc"), arch("arm64")
  ('aarch64', 'x86_64', 'aarch64')
  """
  name = target.split("-")[0].lower()
  name = {"arm64": "aarch64", "amd64": "x86_64"}.get(name, name)
  if name not in ("x86_64", "aarch64"):
    sys.exit(f"pack: {target}: the arch is x86_64 or aarch64")

  return name


def here() -> str:
  """This machine's arch, as Rust names it."""
  return arch(host.machine())


def align(n: int, to: int) -> int:
  return -(-n // to) * to


# MARK: Images
# A PNG codec, the icon's sizes, the .icns and the .ico, in place of sips and iconutil on every OS.
# `resized` and `cropped` are ported from wavo-app's build.rs (MIT, vladkens).

PNG = b"\x89PNG\r\n\x1a\n"
# A byte's size as a signed one, for the PNG filter's guess.
SIGNED = bytes(min(b, 256 - b) for b in range(256))
# The sizes of the .ico and the exe's icon: Explorer's views and the taskbar at 100–200%.
ICO = (16, 24, 32, 48, 64, 256)
# The .icns entries iconutil writes: 16, 32, 128, 256, 512 and each at 2x, all PNG but ic04 and
# ic05, ARGB: macOS 27's iconutil reads a PNG in icp4 or icp5 (16, 32) as noise (2026-10-29).
ICNS = (
  (b"ic04", 16), (b"ic05", 32), (b"ic07", 128), (b"ic08", 256), (b"ic09", 512), (b"ic10", 1024),
  (b"ic11", 32), (b"ic12", 64), (b"ic13", 256), (b"ic14", 512),
)  # fmt: skip


def png_decode(data: bytes) -> tuple[int, int, bytes]:
  """A PNG's `(width, height, RGBA)`, 8 bits a channel, straight alpha: grey or RGB, with or
  without alpha, at 8 or 16 bits, or an 8-bit palette, each with its tRNS; never interlaced.

  >>> px = bytes(range(32))
  >>> png_decode(png_encode(4, 2, px)) == (4, 2, px)
  True
  >>> rows = bytes([4, 7, 2, 1])  # Paeth then Up, one grey pixel each
  >>> head = struct.pack(">IIBBBBB", 1, 2, 8, 0, 0, 0, 0)
  >>> img = PNG + chunk(b"IHDR", head) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")
  >>> png_decode(img)
  (1, 2, b'\\x07\\x07\\x07\\xff\\x08\\x08\\x08\\xff')
  >>> def keyed(depth, color, px, key):  # one row, unfiltered, with a tRNS colour key
  ...   head = struct.pack(">IIBBBBB", 2, 1, depth, color, 0, 0, 0)
  ...   parts = (b"IHDR", head), (b"tRNS", key), (b"IDAT", zlib.compress(b"\\0" + px))
  ...   return png_decode(PNG + b"".join(chunk(*c) for c in parts) + chunk(b"IEND", b""))
  >>> keyed(8, 2, bytes([1, 2, 3, 4, 5, 6]), bytes([0, 4, 0, 5, 0, 6]))  # RGB: the second clear
  (2, 1, b'\\x01\\x02\\x03\\xff\\x04\\x05\\x06\\x00')
  >>> keyed(16, 0, bytes([1, 2, 1, 3]), bytes([1, 3]))  # 16-bit grey: both bytes compared
  (2, 1, b'\\x01\\x01\\x01\\xff\\x01\\x01\\x01\\x00')
  """
  if data[:8] != PNG:
    raise ValueError("not a PNG")

  pos, chunks, idat = 8, {}, []
  while pos + 8 <= len(data):
    n, kind = struct.unpack_from(">I4s", data, pos)
    body = data[pos + 8 : pos + 8 + n]
    idat += [body] if kind == b"IDAT" else []
    chunks.setdefault(kind, body)
    pos += 12 + n
    if kind == b"IEND":
      break

  w, h, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", chunks[b"IHDR"])
  channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}.get(color)
  if not channels or depth not in (8, 16) or (color == 3 and depth != 8) or interlace:
    raise ValueError(
      f"a PNG of color type {color} at {depth} bits, interlaced {interlace}: pack reads grey, RGB"
      " or RGBA at 8 or 16 bits or an 8-bit palette, not interlaced"
    )

  bpp = channels * depth // 8
  stride = w * bpp + 1
  raw = zlib.decompress(b"".join(idat))
  if len(raw) < stride * h:
    raise ValueError("its pixels are cut short")

  prev, rows = bytes(stride - 1), []
  for y in range(h):
    prev = unfilter(raw[y * stride], raw[y * stride + 1 : (y + 1) * stride], prev, bpp)
    rows.append(prev)

  # 16 bits: the high byte of each.
  full = b"".join(rows)
  px = full[:: depth // 8]
  if color == 3:
    plte, trns = chunks[b"PLTE"], chunks.get(b"tRNS", b"")
    table = [bytes(plte[c::3]).ljust(256, b"\0") for c in range(3)]
    channels = [px.translate(t) for t in table] + [px.translate(trns.ljust(256, b"\xff"))]
  else:
    channels = [px[c :: len(px) // (w * h)] for c in range(len(px) // (w * h))]
    if color in (0, 4):
      channels = channels[:1] * 3 + channels[1:]

    if len(channels) == 3:
      # tRNS: the one grey or RGB that is clear, as 16-bit samples; at 8 bits, of their low bytes.
      key = chunks.get(b"tRNS", b"")[: 2 if color == 0 else 6]
      if depth == 8:
        key = b"" if any(key[::2]) else key[1::2]

      if len(key) == bpp:
        pixels = range(0, len(full), bpp)
        channels.append(bytes(0 if full[i : i + bpp] == key else 255 for i in pixels))
      else:
        channels.append(b"\xff" * (w * h))

  out = bytearray(w * h * 4)
  for c, plane in enumerate(channels):
    out[c::4] = plane

  return w, h, bytes(out)


def unfilter(kind: int, line: bytes, prev: bytes, bpp: int) -> bytes:
  """One row of a PNG as it was before its filter, `prev` the row above, as it was."""
  if kind == 0:
    return line

  if kind == 2:
    return bytewise(line, prev, True)

  if kind == 1:
    # The running sum of every bpp-th byte, by doubling steps (Hillis and Steele's scan).
    step = bpp
    while step < len(line):
      line, step = bytewise(line, bytes(step) + line[:-step], True), step * 2

    return line

  if kind not in (3, 4):
    raise ValueError(f"no PNG filter {kind}")

  # Average and Paeth, byte by byte: each takes the byte left of it as it is decoded, none (a
  # zero) left of the first pixel.
  out, up = bytearray(bpp) + line, bytes(bpp) + prev
  for i in range(bpp, len(out)):
    a, b, c = out[i - bpp], up[i], up[i - bpp]
    pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - c - c)
    paeth = a if pa <= pb and pa <= pc else b if pb <= pc else c
    out[i] = (out[i] + ((a + b) >> 1 if kind == 3 else paeth)) & 255

  return bytes(out[bpp:])


def bytewise(x: bytes, y: bytes, plus: bool) -> bytes:
  """Each byte of `x` plus, else minus, `y`'s, mod 256, all at once in one integer: the high bits
  apart, so no carry crosses a byte (Hacker's Delight, 2–18).

  >>> bytewise(b"\\x01\\xff\\x80", b"\\x02\\x01\\x80", True)
  b'\\x03\\x00\\x00'
  >>> bytewise(b"\\x01\\x00", b"\\x02\\x01", False)
  b'\\xff\\xff'
  """
  n = len(x)
  a, b = int.from_bytes(x, "little"), int.from_bytes(y, "little")
  high = int.from_bytes(b"\x80" * n, "little")
  low = high ^ ((1 << 8 * n) - 1)
  if plus:
    out = ((a & low) + (b & low)) ^ ((a ^ b) & high)
  else:
    out = ((a | high) - (b & low)) ^ ((a ^ ~b) & high)

  return out.to_bytes(n, "little")


def chunk(kind: bytes, body: bytes) -> bytes:
  return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))


def png_encode(w: int, h: int, rgba: bytes) -> bytes:
  """An 8-bit RGBA PNG, each row filtered by None, Sub or Up, the one that sums smallest.

  >>> png = png_encode(1, 1, b"\\x01\\x02\\x03\\x04")
  >>> png[:8] == PNG, png[12:16], len(png)
  (True, b'IHDR', 70)
  """
  stride, prev, rows = w * 4, bytes(w * 4), []
  for y in range(h):
    line = rgba[y * stride : (y + 1) * stride]
    left, up = bytewise(line, bytes(4) + line[:-4], False), bytewise(line, prev, False)
    # The smallest sum of the bytes as signed ones, the usual guess at what deflates best.
    cost = [sum(f.translate(SIGNED)) for f in (line, left, up)]
    best = cost.index(min(cost))
    rows += [bytes([best]), (line, left, up)[best]]
    prev = line

  head = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
  idat = zlib.compress(b"".join(rows))
  return PNG + chunk(b"IHDR", head) + chunk(b"IDAT", idat) + chunk(b"IEND", b"")


def resized(w: int, h: int, rgba: bytes, *sizes: int) -> list[bytes]:
  """The image at each size, `n` × `n`, each pixel the average of the source pixels it covers,
  weighted by their alpha: averaging straight colours would bleed the transparent black. Only ever
  smaller.

  >>> px = bytes([255, 0, 0, 255, 0, 0, 0, 0, 0, 255, 0, 255, 0, 0, 255, 255])
  >>> resized(2, 2, px, 1, 2) == [b"UUU\\xbf", px]
  True
  """
  if max(sizes) > min(w, h):
    raise ValueError(f"the icon is {w}×{h}, smaller than {max(sizes)} px")

  # Each channel's running sums across and down (alpha's, and the colours times it), from which
  # each box's sum is four of them: in arrays, as the image's are millions.
  alpha = rgba[3::4]
  planes = [map(mul, rgba[c::4], alpha) for c in range(3)] + [iter(alpha)]
  sums = []
  for plane in planes:
    rows = [array.array("q", bytes(8 * (w + 1)))]
    for _ in range(h):
      line = accumulate(islice(plane, w), initial=0)
      rows.append(array.array("q", map(add, rows[-1], line)))

    sums.append(rows)

  out = []
  for n in sizes:
    if n == w == h:
      out.append(bytes(rgba))
      continue

    xs = [-(-j * w // n) for j in range(n + 1)]
    ys = [-(-k * h // n) for k in range(n + 1)]
    pick = itemgetter(*xs)
    boxes = []
    for rows in sums:
      box = []
      for k in range(n):
        down = list(map(sub, pick(rows[ys[k + 1]]), pick(rows[ys[k]])))
        box += map(sub, down[1:], down[:-1])

      boxes.append(box)

    r, g, b, a = boxes
    counts = [(ys[k + 1] - ys[k]) * (xs[j + 1] - xs[j]) for k in range(n) for j in range(n)]
    px = bytearray(n * n * 4)
    # Rounded half up, as Rust's round() does for these.
    for c, box in enumerate((r, g, b)):
      px[c::4] = bytes([(2 * v + s) // (2 * s) if s else 0 for v, s in zip(box, a)])

    px[3::4] = bytes([(2 * s + k) // (2 * k) for s, k in zip(a, counts)])
    out.append(bytes(px))

  return out


def cropped(w: int, h: int, rgba: bytes) -> tuple[int, bytes]:
  """`rgba` cut to the square around its visible pixels, centred, a side and its pixels: macOS
  icons keep a transparent margin around their tile (about a tenth on each side), Windows' taskbar
  and Linux docks show the image as it is.

  >>> px = bytearray(4 * 4 * 4)
  >>> px[(1 * 4 + 1) * 4 : (1 * 4 + 3) * 4] = b"\\xff" * 8
  >>> side, tile = cropped(4, 4, bytes(px))
  >>> side, tile == bytes(8) + b"\\xff" * 8
  (2, True)
  """
  alpha = rgba[3::4]
  xs = [x for x in range(w) if max(alpha[x::w]) > 8]
  ys = [y for y in range(h) if max(alpha[y * w : (y + 1) * w]) > 8]
  if not xs:
    raise ValueError("the icon is transparent")

  (x0, x1), (y0, y1) = (xs[0], xs[-1]), (ys[0], ys[-1])
  side = max(x1 - x0, y1 - y0) + 1
  # The square's corner, the tile centred in it; outside the image is transparent.
  left, top = (x0 + x1 + 1) // 2 - side // 2, (y0 + y1 + 1) // 2 - side // 2
  tile = bytearray(side * side * 4)
  a, b = max(left, 0), min(left + side, w)
  for y in range(max(top, 0), min(top + side, h)):
    at = ((y - top) * side + a - left) * 4
    tile[at : at + (b - a) * 4] = rgba[(y * w + a) * 4 : (y * w + b) * 4]

  return side, bytes(tile)


@functools.cache
def picture(path: Path) -> tuple[int, int, bytes]:
  try:
    return png_decode(path.read_bytes())
  except (OSError, ValueError, KeyError, struct.error, zlib.error) as e:
    sys.exit(f"pack: {path}: {e}")


def icons(path: Path, sizes, crop: bool) -> dict[int, bytes]:
  """The icon's RGBA at each size, `crop`ped to its tile or with its margin."""
  w, h, px = picture(path)
  if crop:
    w, px = cropped(w, h, px)
    h = w

  sizes = sorted(sizes)
  return dict(zip(sizes, resized(w, h, px, *sizes)))


def icns(images: dict[int, bytes]) -> bytes:
  """The .icns of the icon's RGBA at ICNS's sizes, as iconutil writes it: ic04 and ic05 'ARGB',
  each channel in literal runs of the icns RLE (a byte, the run's length less one), the rest PNG.

  >>> data = icns({n: bytes(n * n * 4) for _, n in ICNS})
  >>> data[:4], struct.unpack(">I", data[4:8])[0] == len(data), data[8:12], data[16:20]
  (b'icns', True, b'ic04', b'ARGB')
  >>> struct.unpack(">I", data[12:16])[0] == 8 + 4 + 4 * (256 + 2)
  True
  """
  body = b""
  for kind, n in ICNS:
    px = images[n]
    if kind in (b"ic04", b"ic05"):
      planes = [px[c::4] for c in (3, 0, 1, 2)]
      runs = [p[i : i + 128] for p in planes for i in range(0, len(p), 128)]
      data = b"ARGB" + b"".join(bytes([len(run) - 1]) + run for run in runs)
    else:
      data = png_encode(n, n, px)

    body += kind + struct.pack(">I", 8 + len(data)) + data

  return b"icns" + struct.pack(">I", 8 + len(body)) + body


def icon_dir(pngs: dict[int, bytes], ends: list[bytes]) -> bytes:
  """An icon directory of the PNGs by size, each entry ending in `ends`'s bytes: in an .ico the
  PNG's offset, in an exe's RT_GROUP_ICON its RT_ICON's id."""
  head = struct.pack("<3H", 0, 1, len(pngs))
  for (n, png), end in zip(pngs.items(), ends):
    head += struct.pack("<4B2HI", n % 256, n % 256, 0, 0, 1, 32, len(png)) + end

  return head


def ico(pngs: dict[int, bytes]) -> bytes:
  """The .ico of the PNGs by size (Windows Vista and later).

  >>> data = ico({n: png_encode(n, n, bytes(n * n * 4)) for n in (16, 256)})
  >>> struct.unpack("<3H", data[:6]), data[6], data[22], data[38:42] == PNG[:4]
  ((0, 1, 2), 16, 0, True)
  """
  at = accumulate([len(png) for png in pngs.values()], initial=6 + 16 * len(pngs))
  return icon_dir(pngs, [struct.pack("<I", n) for n in at]) + b"".join(pngs.values())


# MARK: macOS

KEYS = {"accessory", "plist", "entitlements"}


def info(cfg: Config, minos: str) -> dict:
  """The Info.plist: the bundle from the config, then its `plist` keys (usage strings among them)
  over it. An `accessory` app (a tray's) has no Dock icon and is never ended by macOS on its own.

  >>> mac = {"accessory": True, "plist": {"NSMicrophoneUsageDescription": "m", "X": 1}}
  >>> cfg = Config("me.a", "A", Path("i.png"), "", "1.2.0-beta.1", "a", mac, {}, {}, "", [],
  ...   Path("."), Path("target"))
  >>> p = info(cfg, "14.0")
  >>> p["CFBundleExecutable"], p["CFBundleVersion"], p["LSUIElement"], p["X"]
  ('a', '1.2.0', True, 1)
  >>> p["NSSupportsSuddenTermination"], p["NSMicrophoneUsageDescription"]
  (False, 'm')
  >>> p["LSMinimumSystemVersion"]
  '14.0'
  """
  bad = set(cfg.macos) - KEYS
  if bad:
    bad, known = ", ".join(sorted(bad)), ", ".join(sorted(KEYS))
    sys.exit(f"pack: unknown [package.metadata.nimbo.macos] keys {bad}: one of {known}")

  # Both versions are numbers and dots only: 1.2.0 of 1.2.0-beta.1.
  version = re.match(r"\d+(\.\d+)*", cfg.version)[0]
  plist = {
    "CFBundleIdentifier": cfg.id,
    "CFBundleName": cfg.name,
    "CFBundleDisplayName": cfg.name,
    "CFBundleExecutable": cfg.bin,
    "CFBundleIconFile": cfg.name,
    "CFBundlePackageType": "APPL",
    "CFBundleShortVersionString": version,
    "CFBundleVersion": version,
    "CFBundleInfoDictionaryVersion": "6.0",
    "CFBundleDevelopmentRegion": "en",
    "LSMinimumSystemVersion": minos,
    "NSHighResolutionCapable": True,
  }
  if cfg.macos.get("accessory"):
    plist |= {
      "LSUIElement": True,
      "NSSupportsAutomaticTermination": False,
      "NSSupportsSuddenTermination": False,
    }

  return {**plist, **cfg.macos.get("plist", {})}


# Mach-O's CPU types (xnu's machine.h), named as lipo names them.
CPUS = {0x0100000C: "arm64", 0x01000007: "x86_64"}


def macho(data: bytes) -> list[tuple[str, str]]:
  """Each slice of a Mach-O binary, thin or fat, as `(arch, the macOS it needs)`, in place of lipo
  and vtool: LC_BUILD_VERSION's minos for macOS, else LC_VERSION_MIN_MACOSX (xnu's loader.h).

  >>> cmd = struct.pack("<6I", 0x32, 24, 1, 0x0E0000, 0, 0)
  >>> thin = struct.pack("<8I", 0xFEEDFACF, 0x0100000C, 0, 2, 1, len(cmd), 0, 0) + cmd
  >>> old = struct.pack("<4I", 0x24, 16, 0x0A0C06, 0)
  >>> intel = struct.pack("<8I", 0xFEEDFACF, 0x01000007, 3, 2, 1, len(old), 0, 0) + old
  >>> macho(thin)
  [('arm64', '14.0')]
  >>> fat = struct.pack(">2I", 0xCAFEBABE, 2)
  >>> fat += struct.pack(">5I", 0x01000007, 3, 48, len(intel), 0)
  >>> fat += struct.pack(">5I", 0x0100000C, 0, 48 + len(intel), len(thin), 0)
  >>> macho(fat + intel + thin)
  [('x86_64', '10.12.6'), ('arm64', '14.0')]
  """
  view = memoryview(data)
  magic = bytes(view[:4])
  if magic in (b"\xca\xfe\xba\xbe", b"\xca\xfe\xba\xbf"):
    # fat_arch, or fat_arch_64 (FAT_MAGIC_64): the offset and size after the CPU type and subtype.
    (fmt, step), out = (">8xQQ", 32) if magic[3] == 0xBF else (">8xII", 20), []
    for i in range(struct.unpack_from(">I", view, 4)[0]):
      at, size = struct.unpack_from(fmt, view, 8 + step * i)
      out += macho(view[at : at + size])

    return out

  if magic != b"\xcf\xfa\xed\xfe":
    raise ValueError("not a 64-bit Mach-O binary")

  cpu, _, _, count = struct.unpack_from("<4I", view, 4)
  at, version = 32, None
  for _ in range(count):
    cmd, size, first, second = struct.unpack_from("<4I", view, at)
    # LC_BUILD_VERSION for PLATFORM_MACOS, else LC_VERSION_MIN_MACOSX.
    if cmd == 0x32 and first == 1:
      version = second
    elif cmd == 0x24 and version is None:
      version = first

    at += size

  if version is None:
    raise ValueError("it says no macOS version")

  parts = [version >> 16, version >> 8 & 255] + ([version & 255] if version & 255 else [])
  return [(CPUS.get(cpu, hex(cpu)), ".".join(map(str, parts)))]


def slices(binary: Path) -> list[tuple[str, str]]:
  try:
    return macho(binary.read_bytes())
  except (OSError, ValueError, struct.error) as e:
    sys.exit(f"pack: {binary}: {e}")


def identities(valid: bool) -> list[str]:
  args = ["find-identity", *(["-v"] if valid else []), "-p", "codesigning"]
  out = run("security", *args, capture_output=True, text=True).stdout
  return [ln for ln in out.splitlines() if '"' in ln]


def make_identity(name: str) -> None:
  """A self-signed code signing certificate in the login keychain, codesign allowed to use it."""
  with tempfile.TemporaryDirectory() as tmp:
    key, cert, p12 = (Path(tmp) / n for n in ("key.pem", "cert.pem", "identity.p12"))
    # macOS's own openssl (LibreSSL): `security import` reads its PKCS#12, not OpenSSL 3's.
    openssl = "/usr/bin/openssl"
    new = ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "3650", "-subj", f"/CN={name}"]
    new += ["-addext", "keyUsage=critical,digitalSignature"]
    new += ["-addext", "extendedKeyUsage=critical,codeSigning"]
    run(openssl, *new, "-keyout", key, "-out", cert, capture_output=True)
    export = ["pkcs12", "-export", "-inkey", key, "-in", cert, "-out", p12]
    run(openssl, *export, "-passout", "pass:nimbo", capture_output=True)
    keychain = Path.home() / "Library/Keychains/login.keychain-db"
    trust = ["-P", "nimbo", "-T", "/usr/bin/codesign"]
    run("security", "import", p12, "-k", keychain, *trust, capture_output=True)

  if not any(f'"{name}"' in ln for ln in identities(False)):
    sys.exit(f'pack: made "{name}", but `security find-identity` does not list it')

  print(f'pack: made the code signing identity "{name}" in the login keychain', file=sys.stderr)


def given() -> tuple[str, str] | None:
  """The release's identity, as codesign takes it and as it is said: $NIMBO_SIGN_IDENTITY (`-` ad
  hoc), else a Developer ID in the keychain by its hash (two, a renewal, are ambiguous by name)."""
  name = os.environ.get("NIMBO_SIGN_IDENTITY")
  if name:
    return name, "ad hoc" if name == "-" else name

  dev = [ln for ln in identities(True) if '"Developer ID Application:' in ln]
  return (dev[0].split()[1], dev[0].split('"')[1]) if dev else None


def identity(cfg: Config) -> tuple[str, str]:
  """What signs the app, as `given` says it: the given one, else ad hoc in CI, else "<Name> Local
  Signing", made once in the login keychain: its designated requirement names the bundle id and
  the certificate, not the code hash, so the permissions macOS grants survive rebuilds."""
  found = given()
  if found:
    return found

  if os.environ.get("CI"):
    return "-", "ad hoc (CI)"

  name = f"{cfg.name} Local Signing"
  if not any(f'"{name}"' in ln for ln in identities(False)):
    make_identity(name)

  return name, name


def stamp(sha: str) -> str:
  """A secure timestamp, which notarization needs, in CI only (macos-sign signs with one again):
  locally a Developer ID signs offline. Never for ad hoc."""
  return "--timestamp" if os.environ.get("CI") and sha != "-" else "--timestamp=none"


def app(cfg: Config, binary: Path) -> Path:
  """`<target dir>/<Name>.app`: the binary, the icns and the Info.plist, signed by
  `identity` with the hardened runtime and `macos.entitlements`. One codesign fails on (or a
  keychain dialog nobody answers stops) is no bundle, never one signed ad hoc, whose grants would
  last that build only. Off macOS rcodesign signs it, with APPLE_CERTIFICATE for notarization."""
  sha, name = identity(cfg) if MAC else ("", "Developer ID" if secrets(cfg, CERT) else "ad hoc")
  bundle = cfg.target / f"{cfg.name}.app"
  shutil.rmtree(bundle, ignore_errors=True)
  (bundle / "Contents/MacOS").mkdir(parents=True)
  (bundle / "Contents/Resources").mkdir(parents=True)
  executable(binary, bundle / "Contents/MacOS" / cfg.bin)
  images = icons(cfg.icon, {n for _, n in ICNS}, False)
  (bundle / f"Contents/Resources/{cfg.name}.icns").write_bytes(icns(images))
  (bundle / "Contents/PkgInfo").write_text("APPL????")
  # The macOS every slice runs on: the highest any needs.
  minos = max((v for _, v in slices(binary)), key=lambda v: [int(n) for n in v.split(".")])
  (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps(info(cfg, minos)))
  entitlements = cfg.macos.get("entitlements")
  if MAC:
    run("xattr", "-cr", bundle)
    sign = ["--force", "--sign", sha, "--options", "runtime", stamp(sha)]
    sign += ["--entitlements", cfg.root / entitlements] if entitlements else []
    try:
      run("codesign", *sign, bundle, timeout=60)
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as e:
      shutil.rmtree(bundle, ignore_errors=True)
      sys.exit(f'pack: codesign with "{name}" failed ({e}): no bundle')

    run("codesign", "--verify", "--strict", bundle)
  else:
    signed = name != "ad hoc"
    args = ["--code-signature-flags", "runtime", *(["--for-notarization"] if signed else [])]
    args += ["--entitlements-xml-file", cfg.root / entitlements] if entitlements else []
    rsign(cfg, bundle, signed, *args)

  print(f"{bundle}: {cfg.name} {cfg.version}, signed: {name}")
  return bundle


def dmg(cfg: Config, bundle: Path, target: str | None, out_dir: Path) -> Path:
  """`<stem>-macos-<arch>.dmg` (UDZO) of `bundle`, as it is signed, and an /Applications link,
  the arch `target`'s, else the bundle's binary's; the DMG alone signed as `given` says
  unless that is ad hoc. Off macOS `hfs_dmg` makes it."""
  plist = bundle / "Contents/Info.plist"
  if not plist.is_file():
    sys.exit(f"pack: {bundle} is no app bundle")

  exe = bundle / "Contents/MacOS" / plistlib.loads(plist.read_bytes())["CFBundleExecutable"]
  archs = [target] if target else [a for a, _ in slices(exe)]
  if len(archs) != 1:
    sys.exit(f"pack: {exe} is built for {' and '.join(archs)}: give --target")

  out = out_dir / f"{stem(cfg)}-macos-{arch(archs[0])}.dmg"
  out_dir.mkdir(parents=True, exist_ok=True)
  if not MAC:
    return hfs_dmg(cfg, bundle, out)

  with tempfile.TemporaryDirectory() as tmp:
    stage = Path(tmp) / cfg.name
    stage.mkdir()
    # ditto keeps the signature, the links and the modes as they are.
    run("ditto", bundle, stage / bundle.name)
    (stage / "Applications").symlink_to("/Applications")
    # hdiutil, which macOS 27 deprecates for `diskutil image`: the runners up to macOS 15 lack that.
    image = ["-volname", cfg.name, "-srcfolder", stage, "-format", "UDZO", "-ov"]
    run("hdiutil", "create", *image, out, capture_output=True)

  found = given()
  signed = found[1] if found and found[0] != "-" else "no"
  if found and found[0] != "-":
    run("codesign", "--force", "--sign", found[0], stamp(found[0]), out, timeout=60)

  print(f"{out}: {cfg.name} {cfg.version}, signed: {signed}")
  return out


# MARK: macOS on Linux
# The app signed by rcodesign and the DMG made the way Firefox's mozpack/dmg.py makes it, both with
# no Apple tool, as the Ubuntu publish job does. libdmg-hfsplus is GPL-3: run, never linked.

CERT = ("APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD")
NOTARY = ("APPLE_API_KEY", "APPLE_API_KEY_ID", "APPLE_API_ISSUER")
# apple-codesign 0.29.0 (2024-11-29), by the triple it runs on, pinned in SHA256.
RCODESIGN = (
  "https://github.com/indygreg/apple-platform-rs/releases/download/apple-codesign/0.29.0/"
  "apple-codesign-0.29.0-{triple}.tar.gz"
)
# PLACEHOLDER until vladkens/actions' tools.yml has made its tools-<date> release: the URL of its
# tarball of libdmg-hfsplus's `dmg` and `hfsplus` (mozilla/libdmg-hfsplus e18579b39195, x86_64
# Linux), its sha256 in SHA256 by the file's name. Until then $NIMBO_TOOLS gives them.
LIBDMG = None


def secrets(cfg: Config, names: tuple[str, ...]) -> bool:
  """Whether every one of `names` is set; a release (a `v*` label) without one fails.

  >>> cfg = Config("me.a", "A", Path("i.png"), "", "1.2.0", "a", {}, {}, {}, "", [], Path("."),
  ...   Path("target"), "dev")
  >>> secrets(cfg, ("NIMBO_NO_SUCH_SECRET",))
  False
  >>> secrets(Config(**{**cfg.__dict__, "label": "v1.2.0"}), ("NIMBO_NO_SUCH_SECRET",))
  Traceback (most recent call last):
  SystemExit: pack: v1.2.0 is a release, signed and notarized: set NIMBO_NO_SUCH_SECRET
  """
  missing = [n for n in names if not os.environ.get(n)]
  if missing and (cfg.label or "").startswith("v"):
    sys.exit(f"pack: {cfg.label} is a release, signed and notarized: set {', '.join(missing)}")

  return not missing


def tool(cfg: Config, name: str) -> Path:
  """rcodesign, fetched for this host into <target dir>/tools, or libdmg-hfsplus's `dmg` and
  `hfsplus`, from $NIMBO_TOOLS when it is set, else fetched too; the tarballs pinned in SHA256."""
  where = os.environ.get("NIMBO_TOOLS")
  if name != "rcodesign" and where:
    path = Path(where) / name
    if not path.is_file():
      sys.exit(f"pack: no {name} in $NIMBO_TOOLS ({where})")

    return path

  if name == "rcodesign":
    triple = here() + ("-apple-darwin" if sys.platform == "darwin" else "-unknown-linux-musl")
    url = RCODESIGN.format(triple=triple)
  elif LIBDMG:
    url = LIBDMG
  else:
    sys.exit("pack: dmg and hfsplus: run vladkens/actions tools.yml and pin its hashes (LIBDMG)")

  tarball = cfg.target / "tools" / url.rsplit("/", 1)[1]
  if tarball.name not in SHA256:
    sys.exit(f"pack: no {name} is pinned for {here()} {sys.platform}")

  with tarfile.open(fetch(url, tarball)) as tar:
    member = next((m for m in tar if m.isfile() and Path(m.name).name == name), None)
    if member is None:
      sys.exit(f"pack: {tarball.name} has no {name}")

    path = tarball.parent / name
    path.write_bytes(tar.extractfile(member).read())

  path.chmod(0o755)
  return path


def private(path: Path, data: bytes) -> Path:
  """`data` in a new file only its owner reads: a key, a certificate."""
  with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "wb") as f:
    f.write(data)

  return path


def certificate(tmp: Path) -> Path:
  """APPLE_CERTIFICATE's key and certificate as a PEM in `tmp`: rcodesign rejects a .p12 in
  OpenSSL 3's defaults (rcodesign #315), and OpenSSL 3 reads an older one's RC2 only `-legacy`."""
  p12 = private(tmp / "id.p12", base64.b64decode(os.environ["APPLE_CERTIFICATE"]))
  args = [openssl3(), "pkcs12", "-in", p12, "-nodes", "-passin", "env:APPLE_CERTIFICATE_PASSWORD"]
  for legacy in ([], ["-legacy"]):
    done = subprocess.run([str(a) for a in args + legacy], capture_output=True)
    if done.returncode == 0:
      return private(tmp / "id.pem", done.stdout)

  sys.exit(f"pack: APPLE_CERTIFICATE: openssl reads no .p12: {done.stderr.decode().strip()}")


def rsign(cfg: Config, path: Path, signed: bool, *args) -> None:
  """rcodesign signs `path` in place: with APPLE_CERTIFICATE's Developer ID, else ad hoc."""
  with tempfile.TemporaryDirectory() as tmp:
    key = ["--pem-file", certificate(Path(tmp))] if signed else []
    try:
      run(tool(cfg, "rcodesign"), "sign", *key, *args, path, stdout=subprocess.DEVNULL)
    except subprocess.CalledProcessError as e:
      sys.exit(f"pack: rcodesign could not sign {path.name} (exit {e.returncode}), as it says")


def hfs_dmg(cfg: Config, bundle: Path, out: Path) -> Path:
  """The DMG as mozpack/dmg.py makes it: an HFS+ image a tenth and 4 MiB over the app's size
  by `mkfs.hfsplus`, the app and the /Applications link put in by `hfsplus`, compressed by `dmg`
  (LZMA, which macOS reads since 10.15). rcodesign signs it with APPLE_CERTIFICATE, then with the
  App Store Connect API key notarizes and staples it."""
  signed = secrets(cfg, CERT)
  notarized = secrets(cfg, NOTARY) and signed
  mkfs = shutil.which("mkfs.hfsplus")
  if not mkfs:
    sys.exit("pack: no mkfs.hfsplus: `apt-get install hfsprogs`")

  hfsplus, quiet = tool(cfg, "hfsplus"), {"stdout": subprocess.DEVNULL}
  with tempfile.TemporaryDirectory() as tmp:
    image = Path(tmp) / "image.hfs"
    kib = int(run("du", "-sk", bundle, capture_output=True, text=True).stdout.split()[0])
    with image.open("wb") as f:
      f.truncate((kib * 11 // 10 + 4096) * 1024)

    run(mkfs, "-v", cfg.name, image, **quiet)
    run(hfsplus, image, "mkdir", f"/{bundle.name}", **quiet)
    run(hfsplus, image, "addall", bundle, f"/{bundle.name}", **quiet)
    run(hfsplus, image, "symlink", "/Applications", "/Applications", **quiet)
    out.unlink(missing_ok=True)
    run(tool(cfg, "dmg"), "--compression", "lzma", "build", image, out, **quiet)

  if signed:
    rsign(cfg, out, True)

  if notarized:
    notarize(cfg, out)

  state = "notarized" if notarized else "Developer ID" if signed else "no"
  print(f"{out}: {cfg.name} {cfg.version}, signed: {state}")
  return out


def notarize(cfg: Config, dmg: Path) -> None:
  """rcodesign submits the DMG to Apple's notary service, waits up to 20 minutes and staples the
  ticket, the API key encoded into a JSON only its owner reads. It prints Apple's log itself."""
  env, rcodesign = os.environ, tool(cfg, "rcodesign")
  with tempfile.TemporaryDirectory() as tmp:
    p8 = private(Path(tmp) / "key.p8", env["APPLE_API_KEY"].encode())
    encode = [rcodesign, "encode-app-store-connect-api-key", env["APPLE_API_ISSUER"]]
    key = run(*encode, env["APPLE_API_KEY_ID"], p8, capture_output=True).stdout
    path = private(Path(tmp) / "key.json", key)
    submit = ["--api-key-file", path, "--staple", "--max-wait-seconds", "1200", dmg]
    if subprocess.run([str(a) for a in [rcodesign, "notary-submit", *submit]]).returncode:
      sys.exit(f"pack: notarizing {dmg.name} failed: rcodesign's log is above")


# MARK: Windows

NAMES = {"x86_64": "x64", "aarch64": "arm64"}
RUN = r"Software\Microsoft\Windows\CurrentVersion\Run"
APPROVED = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
UNINSTALL = r"Software\Microsoft\Windows\CurrentVersion\Uninstall"


def setup(cfg: Config, binary: Path, arch: str, out_dir: Path) -> Path:
  r"""`<stem>-windows-<x64|arm64>-setup.exe`, NSIS's, of the exe with its resources
  (`exe_resources`), built by makensis (apt's nsis, Homebrew's makensis): a per-user install, no
  administrator, into `%LOCALAPPDATA%\Programs\<Name>` with a Start menu entry and the uninstaller
  in Settings › Apps, the app's icon on all of them. The updater (src/update.rs) runs it as `/S`
  once the app has quit; an interactive install offers to start it."""
  makensis = shutil.which("makensis")
  if not makensis:
    sys.exit("pack: no makensis: `apt-get install nsis`, on a Mac `brew install makensis`")

  out_dir.mkdir(parents=True, exist_ok=True)
  out = out_dir / f"{stem(cfg)}-windows-{NAMES[arch]}-setup.exe"
  license = cfg.root / "LICENSE"
  with tempfile.TemporaryDirectory() as tmp:
    # The files by fixed names beside the script, which makensis reads from there: no path in it.
    tmp = Path(tmp)
    pngs = {n: png_encode(n, n, px) for n, px in icons(cfg.icon, ICO, True).items()}
    # A signature later: here, the exe's, before makensis packs it.
    exe_resources(binary, tmp / "app.exe", cfg, pngs)
    (tmp / "app.ico").write_bytes(ico(pngs))
    if license.is_file():
      shutil.copy(license, tmp / "LICENSE.txt")

    (tmp / "setup.nsi").write_text(nsi(cfg, arch, license.is_file()), encoding="utf-8")
    run(makensis, "-V2", "-WX", "-INPUTCHARSET", "UTF8", tmp / "setup.nsi")
    # A signature later: here, the setup's, once makensis has written it.
    shutil.move(str(tmp / "setup.exe"), out)

  return out


def nsis(s: str) -> str:
  """`s` in an NSIS string: `$` doubled (a define's `${` too), a quote `$\\"`, no line breaks.

  >>> print(nsis('a "$b" ${c}\\n'))
  a $\\"$$b$\\" $${c}
  """
  return " ".join(s.replace("$", "$$").replace('"', '$\\"').split("\n")).strip()


def nsi(cfg: Config, arch: str, license: bool) -> str:
  r"""The setup's .nsi: it packs `app.exe`, `app.ico` and, with `license`, `LICENSE.txt` from its
  folder and writes `setup.exe` there. Its `.onInit` exits 2 on a Windows of another arch; the stub
  is x86, so that runs on both. The uninstaller keeps the app's data.

  >>> from types import SimpleNamespace as N
  >>> cfg = N(id="me.vladkens.counter", name="Counter", version="1.2.0-3-gabc", comment="Counts",
  ...   windows={}, publisher="", bin="counter")
  >>> text = nsi(cfg, "aarch64", True)
  >>> keys = ("Install", "VIProduct", "!define MUI_F", "${", "File", "Delete", "CreateS", "Message")
  >>> print("\n".join(l.strip() for l in text.splitlines() if l.strip().startswith(keys)))
  InstallDir "$LOCALAPPDATA\Programs\Counter"
  InstallDirRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\{e65f79c2-5031-5421-a3d1-3c434f820e5d}" "InstallLocation"
  !define MUI_FINISHPAGE_RUN "$INSTDIR\Counter.exe"
  VIProductVersion 1.2.0.0
  ${IfNot} ${IsNativeARM64}
  MessageBox MB_ICONSTOP "This setup of Counter is for arm64 Windows." /SD IDOK
  ${EndIf}
  File "/oname=Counter.exe" "app.exe"
  File "/oname=LICENSE.txt" "LICENSE.txt"
  CreateShortcut "$SMPROGRAMS\Counter.lnk" "$INSTDIR\Counter.exe" "" "$INSTDIR\Counter.exe" 0 SW_SHOWNORMAL "" "Counts"
  Delete "$INSTDIR\Counter.exe"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\Uninstall.exe"
  Delete "$SMPROGRAMS\Counter.lnk"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "me.vladkens.counter"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "me.vladkens.counter"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\{e65f79c2-5031-5421-a3d1-3c434f820e5d}"
  >>> print("\n".join(l.split("}\" ")[1] for l in text.splitlines() if "UninstallString" in l))
  "UninstallString" "$\"$INSTDIR\Uninstall.exe$\""
  "QuietUninstallString" "$\"$INSTDIR\Uninstall.exe$\" /S"
  >>> x64 = nsi(cfg, "x86_64", False)
  >>> "  ${IfNot} ${IsNativeAMD64}\n  ${AndIfNot} ${IsNativeARM64}" in x64, "LICENSE" in x64
  (True, False)
  """
  name, exe = nsis(cfg.name), f"$INSTDIR\\{nsis(cfg.name)}.exe"
  key, value = f"{UNINSTALL}\\{app_id(cfg)}", f'"{nsis(cfg.id)}"'
  version = ".".join(map(str, quad(cfg.version)))
  files = [("app.exe", f"{name}.exe")] + ([("LICENSE.txt", "LICENSE.txt")] if license else [])
  check = ["${IfNot} ${IsNativeARM64}"]
  if arch == "x86_64":
    check = ["${IfNot} ${IsNativeAMD64}", "${AndIfNot} ${IsNativeARM64}"]

  every = {**version_strings(cfg), "FileDescription": f"{cfg.name} Setup"}
  keys = "ProductName ProductVersion FileVersion FileDescription CompanyName LegalCopyright"
  strings = {k: every[k] for k in keys.split()}
  entries = {
    "DisplayName": name,
    "DisplayVersion": nsis(cfg.version),
    "Publisher": nsis(publisher(cfg)),
    "DisplayIcon": exe,
    "InstallLocation": "$INSTDIR",
    "UninstallString": r"$\"$INSTDIR\Uninstall.exe$\"",
    "QuietUninstallString": r"$\"$INSTDIR\Uninstall.exe$\" /S",
  }
  lines = [
    f"; {name}'s setup, written by nimbo's scripts/pack.py, built by makensis -INPUTCHARSET UTF8.",
    "; A signature later: the exe signed before makensis packs it, the setup once it is written,",
    "; the uninstaller by `!uninstfinalize 'sign \"%1\"'`.",
    "Unicode true",
    "ManifestDPIAware true",
    "RequestExecutionLevel user",
    "SetCompressor /SOLID lzma",
    f'Name "{name}" "{name.replace("&", "&&")}"',
    'OutFile "setup.exe"',
    f'InstallDir "$LOCALAPPDATA\\Programs\\{name}"',
    "; An update lands in the folder the first install chose.",
    f'InstallDirRegKey HKCU "{key}" "InstallLocation"',
    "",
    "!include MUI2.nsh",
    "!include x64.nsh",
    "",
    '!define MUI_ICON "app.ico"',
    '!define MUI_UNICON "app.ico"',
    "; The finish page's \"Run\": a silent install (the updater's) shows no page, starts nothing.",
    f'!define MUI_FINISHPAGE_RUN "{exe}"',
    "!insertmacro MUI_PAGE_INSTFILES",
    "!insertmacro MUI_PAGE_FINISH",
    "!insertmacro MUI_UNPAGE_CONFIRM",
    "!insertmacro MUI_UNPAGE_INSTFILES",
    '!insertmacro MUI_LANGUAGE "English"',
    "",
    f"VIProductVersion {version}",
    f"VIFileVersion {version}",
    *(f'VIAddVersionKey /LANG=${{LANG_ENGLISH}} "{k}" "{nsis(v)}"' for k, v in strings.items()),
    "",
    "Function .onInit",
    "  SetShellVarContext current",
    *(f"  {line}" for line in check),
    f'    MessageBox MB_ICONSTOP "This setup of {name} is for {NAMES[arch]} Windows." /SD IDOK',
    "    SetErrorLevel 2",
    "    Abort",
    "  ${EndIf}",
    "FunctionEnd",
    "",
    "Function un.onInit",
    "  SetShellVarContext current",
    "FunctionEnd",
    "",
    "Section",
    '  SetOutPath "$INSTDIR"',
    *(f'  File "/oname={nsis(to)}" "{src}"' for src, to in files),
    '  WriteUninstaller "$INSTDIR\\Uninstall.exe"',
    f'  CreateShortcut "$SMPROGRAMS\\{name}.lnk" "{exe}" "" "{exe}" 0 SW_SHOWNORMAL ""'
    f' "{nsis(cfg.comment)}"',
    *(f'  WriteRegStr HKCU "{key}" "{k}" "{v}"' for k, v in entries.items()),
    f'  WriteRegDWORD HKCU "{key}" "NoModify" 1',
    f'  WriteRegDWORD HKCU "{key}" "NoRepair" 1',
    "SectionEnd",
    "",
    "; The files and the shortcut, and nimbo's launch at login, which would start a deleted exe.",
    'Section "Uninstall"',
    *(f'  Delete "$INSTDIR\\{nsis(to)}"' for _, to in files),
    '  Delete "$INSTDIR\\Uninstall.exe"',
    '  RMDir "$INSTDIR"',
    f'  Delete "$SMPROGRAMS\\{name}.lnk"',
    f'  DeleteRegValue HKCU "{RUN}" {value}',
    f'  DeleteRegValue HKCU "{APPROVED}" {value}',
    f'  DeleteRegKey HKCU "{key}"',
    "SectionEnd",
  ]
  return "\n".join(lines) + "\n"


def app_id(cfg: Config) -> str:
  """The setup's AppId, the same in every version, so a newer setup replaces the install:
  `windows.app-id`, else a GUID made from the app's id. Never changed.

  >>> from types import SimpleNamespace as N
  >>> app_id(N(id="me.vladkens.wavo", windows={}))
  '{a754b18f-b91c-5def-b91e-a32c2017c75f}'
  >>> app_id(N(id="x", windows={"app-id": "314a56a8-8a8b-4c18-954d-b930203f086d"}))
  '{314a56a8-8a8b-4c18-954d-b930203f086d}'
  """
  given = cfg.windows.get("app-id")
  guid = uuid.UUID(given) if given else uuid.uuid5(uuid.NAMESPACE_DNS, cfg.id)
  return "{" + str(guid) + "}"


def publisher(cfg: Config) -> str:
  """Who Settings › Apps names: the config's publisher, else the id's owner (`me.vladkens.x`)."""
  if cfg.publisher:
    return cfg.publisher

  parts = cfg.id.split(".")
  return parts[1] if len(parts) > 2 else parts[0]


# MARK: The exe's resources
# The icon, the version and the manifest written into a PE32+ exe (x64, arm64) by pack itself, so
# the job that makes the setup needs no Windows tool. The PE/COFF layout is Microsoft's "PE Format"
# (learn.microsoft.com/windows/win32/debug/pe-format), the version's VS_VERSIONINFO and the icon's
# GRPICONDIR are Win32's (verrsrc.h, the "Icons" resource format), as rc.exe writes them.

RT_ICON, RT_GROUP_ICON, RT_VERSION, RT_MANIFEST = 3, 14, 16, 24
# en-US, rc.exe's language.
LANG = 0x409
SECTION = struct.Struct("<8sIIIIIIHHI")
# IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ.
RSRC = 0x40000040
# The supportedOS of Windows 10 and 11, which share it.
MANIFEST = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"""


def exe_resources(exe: Path, out: Path, cfg: Config, pngs: dict[int, bytes]) -> Path:
  """`exe` as `out` with the app's resources: its icon (`pngs`, the tile at ICO's sizes), the
  version and the names Explorer and Task Manager show, and the manifest (asInvoker, Windows 10
  and 11, PerMonitorV2, Common Controls 6). Those types replace the linker's; the rest stay."""
  ids = [struct.pack("<H", i) for i in range(1, len(pngs) + 1)]
  ours = {
    RT_ICON: {i: {LANG: png} for i, png in enumerate(pngs.values(), 1)},
    RT_GROUP_ICON: {1: {LANG: icon_dir(pngs, ids)}},
    RT_VERSION: {1: {LANG: version_info(cfg)}},
    RT_MANIFEST: {1: {LANG: MANIFEST.encode()}},
  }
  try:
    out.write_bytes(pe_resources(exe.read_bytes(), ours))
  except (OSError, ValueError, struct.error) as e:
    sys.exit(f"pack: {exe}: {e}")

  return out


def node(key: str, value: bytes | str = b"", children=()) -> bytes:
  """One VS_VERSIONINFO block: a text value, a binary one or none, then its children, each at a
  4-byte boundary; the text ones' wType is 1 and their wValueLength in characters."""
  text = isinstance(value, str) or not value
  value = (value + "\0").encode("utf-16-le") if isinstance(value, str) else value
  body = pad4(bytes(6) + (key + "\0").encode("utf-16-le")) + value
  for child in children:
    body = pad4(body) + child

  return struct.pack("<3H", len(body), len(value) // 2 if text else len(value), text) + body[6:]


def pad4(data: bytes) -> bytes:
  return data + bytes(-len(data) % 4)


def version_strings(cfg: Config) -> dict[str, str]:
  """The exe's version strings: the app's name its FileDescription, which Task Manager shows,
  `windows.copyright` its LegalCopyright, else "© <publisher>"."""
  return {
    "CompanyName": publisher(cfg),
    "FileDescription": cfg.name,
    "FileVersion": cfg.version,
    "InternalName": cfg.bin,
    "LegalCopyright": cfg.windows.get("copyright") or f"© {publisher(cfg)}",
    "OriginalFilename": f"{cfg.name}.exe",
    "ProductName": cfg.name,
    "ProductVersion": cfg.version,
  }


def version_info(cfg: Config) -> bytes:
  r"""The RT_VERSION: the version as numbers (1.2.0.0 of 1.2.0-beta.1) and `version_strings`.

  >>> from types import SimpleNamespace as N
  >>> cfg = N(id="me.a.b", name="B", version="1.2.3-beta.1", bin="b", windows={}, publisher="")
  >>> data = version_info(cfg)
  >>> struct.unpack_from("<4I", data, 40 + 8)
  (65538, 196608, 65538, 196608)
  >>> "1.2.3-beta.1\0" in data.decode("utf-16-le"), "© a\0" in data.decode("utf-16-le")
  (True, True)
  """
  a, b, c, d = quad(cfg.version)
  ms, ls = a << 16 | b, c << 16 | d
  # VS_FIXEDFILEINFO: VOS_NT_WINDOWS32, VFT_APP.
  fixed = struct.pack("<13I", 0xFEEF04BD, 0x10000, ms, ls, ms, ls, 0x3F, 0, 0x40004, 1, 0, 0, 0)
  # The strings in en-US, Unicode (1200), as the translation says.
  table = node(f"{LANG:04X}04B0", children=[node(k, v) for k, v in version_strings(cfg).items()])
  var = node("Translation", struct.pack("<2H", LANG, 1200))
  children = [node("StringFileInfo", children=[table]), node("VarFileInfo", children=[var])]
  return node("VS_VERSION_INFO", fixed, children)


def quad(version: str) -> tuple[int, ...]:
  """The version as Windows' four numbers.

  >>> quad("1.2.3-beta.1"), quad("2")
  ((1, 2, 3, 0), (2, 0, 0, 0))
  """
  nums = [int(n) for n in re.match(r"\d+(\.\d+){0,3}", version)[0].split(".")]
  if max(nums) > 65535:
    sys.exit(f"pack: version {version}: Windows takes numbers up to 65535")

  return tuple((nums + [0, 0, 0])[:4])


def pe_headers(data: bytes) -> tuple[int, int, list[list]]:
  """A PE32+ exe's optional header's offset, its section table's and its sections, each the list
  of IMAGE_SECTION_HEADER's fields."""
  if data[:2] != b"MZ":
    raise ValueError("not an exe")

  pe = struct.unpack_from("<I", data, 0x3C)[0]
  if data[pe : pe + 4] != b"PE\0\0":
    raise ValueError("no PE header")

  machine, count, _, _, _, size = struct.unpack_from("<HHIIIH", data, pe + 4)
  opt = pe + 24
  if struct.unpack_from("<H", data, opt)[0] != 0x20B or machine not in (0x8664, 0xAA64):
    raise ValueError(f"machine {machine:#x}: pack writes the resources of x64 and arm64 exes")

  table = opt + size
  return opt, table, [list(SECTION.unpack_from(data, table + 40 * i)) for i in range(count)]


def directory(data: bytes, opt: int, i: int) -> tuple[int, int]:
  return struct.unpack_from("<II", data, opt + 112 + 8 * i)


def holds(section: list, rva: int) -> bool:
  return section[2] <= rva < section[2] + max(section[1], section[3])


def offset(sections: list[list], rva: int) -> int:
  """Where the RVA is in the file."""
  for s in sections:
    if holds(s, rva):
      return s[4] + rva - s[2]

  raise ValueError(f"RVA {rva:#x} is in no section")


def pe_tree(data: bytes, sections: list[list], rva: int) -> dict:
  """The resource tree at `rva`: type, then name, then language, each an id or a string, to the
  data."""
  base = offset(sections, rva)

  def walk(at: int, depth: int) -> dict:
    if depth > 3:
      raise ValueError("a resource tree deeper than type, name and language")

    named, ids = struct.unpack_from("<HH", data, base + at + 12)
    tree = {}
    for j in range(named + ids):
      key, to = struct.unpack_from("<II", data, base + at + 16 + 8 * j)
      if key >> 31:
        name = base + (key & 0x7FFFFFFF)
        key = data[name + 2 : name + 2 + 2 * struct.unpack_from("<H", data, name)[0]]
        key = key.decode("utf-16-le")

      if to >> 31:
        tree[key] = walk(to & 0x7FFFFFFF, depth + 1)
      else:
        at_rva, size = struct.unpack_from("<II", data, base + to)
        tree[key] = bytes(data[offset(sections, at_rva) :][:size])

    return tree

  return walk(0, 1)


def rsrc(tree: dict, rva: int) -> bytes:
  """The .rsrc section at `rva` of a resource tree: the directories breadth first, the names, the
  data entries, then the data, each at 8 bytes; in each directory the names first, then the ids,
  both in order.

  >>> data = rsrc({RT_MANIFEST: {1: {LANG: b"<m/>"}}, "X": {"Y": {0: b"z"}}}, 0x5000)
  >>> pe_tree(data, [[b".rsrc", len(data), 0x5000, len(data), 0]], 0x5000)
  {'X': {'Y': {0: b'z'}}, 24: {1: {1033: b'<m/>'}}}
  """
  dirs, rows, blobs = [tree], [], []
  for branch in dirs:
    row = []
    for key in sorted(branch, key=lambda k: (0, k.upper(), k) if isinstance(k, str) else (1, k)):
      kids = dirs if isinstance(branch[key], dict) else blobs
      row.append((key, kids is dirs, len(kids)))
      kids.append(branch[key])

    rows.append(row)

  starts = list(accumulate((16 + 8 * len(row) for row in rows), initial=0))
  at = names_at = starts.pop()
  strings, names = b"", {}
  for key in (k for row in rows for k, _, _ in row if isinstance(k, str)):
    if key not in names:
      names[key] = at + len(strings)
      strings += struct.pack("<H", len(key.encode("utf-16-le")) // 2) + key.encode("utf-16-le")

  leaves = align(at + len(strings), 8)
  at, places = leaves + 16 * len(blobs), []
  for blob in blobs:
    places.append(align(at, 8))
    at = places[-1] + len(blob)

  out = bytearray(at)
  out[names_at : names_at + len(strings)] = strings
  for row, start in zip(rows, starts):
    named = sum(isinstance(k, str) for k, _, _ in row)
    struct.pack_into("<IIHHHH", out, start, 0, 0, 0, 0, named, len(row) - named)
    for j, (key, isdir, i) in enumerate(row):
      name = names[key] | 1 << 31 if isinstance(key, str) else key
      to = starts[i] | 1 << 31 if isdir else leaves + 16 * i
      struct.pack_into("<II", out, start + 16 + 8 * j, name, to)

  for i, (blob, place) in enumerate(zip(blobs, places)):
    struct.pack_into("<IIII", out, leaves + 16 * i, rva + place, len(blob), 0, 0)
    out[place : place + len(blob)] = blob

  return bytes(out)


def checksum(data: bytes, at: int) -> int:
  """The PE checksum, the one at `at` taken as 0: the 16-bit words summed, the carries folded in,
  plus the length."""
  raw = bytes(data[:at]) + bytes(4) + bytes(data[at + 4 :]) + bytes(len(data) % 2)
  total = sum(struct.unpack(f"<{len(raw) // 2}H", raw))
  while total >> 16:
    total = (total & 0xFFFF) + (total >> 16)

  return total + len(data)


def pe_resources(data: bytes, ours: dict) -> bytes:
  """`data`, an exe, with a .rsrc of its resources, ours replacing its types. The old .rsrc and
  the .reloc after it come out and go back in, ours then .reloc, at the end: nothing points into
  either but their data directories. No other section moves; the section table grows into the
  headers' free space, else into the DOS stub; anything else is an error, never a broken exe.

  >>> pe = bytearray(0x400)
  >>> pe[:2], pe[0x3C] = b"MZ", 0x40
  >>> struct.pack_into("<4sHH12xHH", pe, 0x40, b"PE\\0\\0", 0x8664, 1, 240, 0x22)
  >>> struct.pack_into("<H30xII16xII44xI", pe, 0x58, 0x20B, 0x1000, 0x200, 0x2000, 0x200, 16)
  >>> SECTION.pack_into(pe, 0x148, b".text", 16, 0x1000, 0x200, 0x200, 0, 0, 0, 0, 0x60000020)
  >>> one = pe_resources(bytes(pe), {RT_MANIFEST: {1: {LANG: b"<m/>"}}, 6: {1: {LANG: b"s"}}})
  >>> two = pe_resources(one, {RT_MANIFEST: {1: {LANG: b"<n/>"}}, RT_VERSION: {1: {0: b"v"}}})
  >>> opt, _, sections = pe_headers(two)
  >>> [s[0].rstrip(b"\\0").decode() for s in sections], directory(two, opt, 2)[0] == sections[1][2]
  (['.text', '.rsrc'], True)
  >>> pe_tree(two, sections, directory(two, opt, 2)[0])
  {6: {1: {1033: b's'}}, 16: {1: {0: b'v'}}, 24: {1: {1033: b'<n/>'}}}
  >>> struct.unpack_from("<I", two, opt + 64)[0] == checksum(two, opt + 64)
  True
  """
  opt, table, sections = pe_headers(data)
  pe, count = opt - 24, len(sections)
  section_align, file_align = struct.unpack_from("<II", data, opt + 32)
  dirs = struct.unpack_from("<I", data, opt + 108)[0]
  if dirs < 6:
    raise ValueError("fewer than 6 data directories")

  if directory(data, opt, 4)[1]:
    raise ValueError("it is signed: its resources go in before the signature")

  if struct.unpack_from("<I", data, pe + 12)[0]:
    raise ValueError("it has a COFF symbol table")

  end = max((s[4] + s[3] for s in sections if s[3]), default=0)
  if end != len(data):
    raise ValueError(f"{len(data) - end} bytes after its last section (or before its end)")

  # The old .rsrc and the sections after it, which may be only .reloc.
  rva, size = directory(data, opt, 2)
  first = next((i for i, s in enumerate(sections) if size and holds(s, rva)), count)
  moved, kept = sections[first:], sections[:first]
  names = [s[0].rstrip(b"\0") for s in moved]
  if names and (names[0] != b".rsrc" or any(n != b".reloc" for n in names[1:])):
    raise ValueError(f"the resources are in {names[0]}, before {names[1:]}: pack moves no others")

  for d in (d for d in range(min(dirs, 16)) if d not in (2, 4, 5)):
    at, n = directory(data, opt, d)
    if n and any(holds(s, at) for s in moved):
      raise ValueError(f"data directory {d} points into {names}, which pack moves")

  start = max((s[4] + s[3] for s in kept if s[3]), default=0)
  if any(s[3] and s[4] < start for s in moved):
    raise ValueError("the .rsrc or .reloc's data lies before another section's")

  tree = pe_tree(data, sections, rva) if size else {}
  tree = {**{k: v for k, v in tree.items() if k not in ours}, **ours}
  out = bytearray(data[: align(start, file_align)])
  va = align(max(s[2] + (s[1] or s[3]) for s in kept), section_align)
  blob = rsrc(tree, va)
  placed = [[b".rsrc", len(blob), va, align(len(blob), file_align), len(out), 0, 0, 0, 0, RSRC]]
  out += blob.ljust(placed[0][3], b"\0")
  struct.pack_into("<II", out, opt + 112 + 16, va, len(blob))
  for s in moved[1:]:
    va = align(placed[-1][2] + placed[-1][1], section_align)
    at, n = directory(data, opt, 5)
    if n and holds(s, at):
      struct.pack_into("<I", out, opt + 112 + 40, at - s[2] + va)

    placed.append([s[0], s[1], va, s[3], len(out), *s[5:]])
    out += data[s[4] : s[4] + s[3]]

  sections = kept + placed
  room = min([struct.unpack_from("<I", data, opt + 60)[0]] + [s[4] for s in sections if s[3]])
  need = table + 40 * len(sections)
  if need > room:
    # The DOS stub, which only DOS runs, gives the headers room: they move to just after the DOS
    # header, unless bound imports (in the headers, by offset) would move with them.
    shift = pe - 64
    if need - shift > room or directory(data, opt, 11)[1]:
      raise ValueError("no room in its headers for one more section header, nor in its DOS stub")

    size = table + 40 * count - pe
    out[64 : 64 + size], out[64 + size : pe + size] = out[pe : pe + size], bytes(shift)
    struct.pack_into("<I", out, 0x3C, 64)
    pe, opt, table, need = pe - shift, opt - shift, table - shift, need - shift

  if any(out[table + 40 * count : need]):
    raise ValueError("its headers hold data after the section table")

  for i, s in enumerate(sections):
    SECTION.pack_into(out, table + 40 * i, *s)

  struct.pack_into("<H", out, pe + 6, len(sections))
  last = sections[-1]
  struct.pack_into("<I", out, opt + 56, align(last[2] + last[1], section_align))
  # SizeOfInitializedData: the new .rsrc's for the old's.
  initialized = struct.unpack_from("<I", out, opt + 8)[0] + placed[0][3]
  initialized -= moved[0][3] if moved else 0
  struct.pack_into("<I", out, opt + 8, initialized & 0xFFFFFFFF)
  struct.pack_into("<I", out, opt + 64, checksum(out, opt + 64))
  return bytes(out)


# MARK: Linux
# Ported from wavo-app's scripts/dist.py. The .desktop entry is named by the app id, nimbo's X11
# WM_CLASS and Wayland app id, so desktops find it, and its icon, from the window.

APPIMAGETOOL = (
  "https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-{arch}.AppImage"
)
RUNTIME = "https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-{arch}"
SHA256 = {
  "appimagetool-x86_64": "ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0",
  "appimagetool-aarch64": "f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158",
  "runtime-x86_64": "2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d",
  "runtime-aarch64": "00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444",
}
# RCODESIGN's by the triple it runs on, as its release's SHA256SUMS gives them.
RCODESIGN_SHA256 = {
  "x86_64-unknown-linux-musl": "dbe85cedd8ee4217b64e9a0e4c2aef92ab8bcaaa41f20bde99781ff02e600002",
  "aarch64-unknown-linux-musl": "4af92c87ddf52f5f2d1258a3b4e56c7dcb8f1b2468df744976c5f139e031961f",
  "aarch64-apple-darwin": "d1a532150adaf90048260d76359261aa716abafc45c53c5dc18845029184334a",
}
SHA256 |= {RCODESIGN.format(triple=t).rsplit("/")[-1]: v for t, v in RCODESIGN_SHA256.items()}
DEB_ARCH = {"x86_64": "amd64", "aarch64": "arm64"}
# The libraries nimbo loads at run time, by their Debian names (t64 since Ubuntu 24.04).
NIMBO = [
  "libgtk-3-0 | libgtk-3-0t64",
  "libgtk-layer-shell0",
  "libfontconfig1",
  "libfreetype6",
  "libharfbuzz0b",
  "libx11-6",
  "libxtst6",
]


def desktop(cfg: Config) -> str:
  """The .desktop entry: the app started by its binary's name, on the PATH."""
  lines = ["[Desktop Entry]", "Type=Application", f"Name={cfg.name}"]
  if cfg.comment:
    lines.append(f"Comment={cfg.comment}")

  lines += [f"Exec={cfg.bin}", f"Icon={cfg.id}", f"StartupWMClass={cfg.id}", "StartupNotify=true"]
  # appimagetool refuses an entry with none.
  categories = cfg.linux.get("categories") or ["Utility"]
  lines.append("Categories=" + "".join(f"{c};" for c in categories))

  lines.append("Terminal=false")
  return "\n".join(lines) + "\n"


def deb_version(version: str) -> str:
  """The Cargo version as Debian orders it: a pre-release sorts before its release.

  >>> deb_version("1.2.0-beta.1"), deb_version("1.2.0")
  ('1.2.0~beta.1', '1.2.0')
  """
  return version.replace("-", "~", 1)


def fetch(url: str, path: Path) -> Path:
  """`url` into `path`, once, checked against SHA256 by the file's name."""
  if not path.exists():
    path.parent.mkdir(parents=True, exist_ok=True)
    part = path.with_suffix(".part")
    with urllib.request.urlopen(url, timeout=60) as r, open(part, "wb") as f:
      shutil.copyfileobj(r, f)

    part.rename(path)

  digest = hashlib.sha256(path.read_bytes()).hexdigest()
  if digest != SHA256[path.name]:
    path.unlink()
    sys.exit(f"pack: {path.name}: SHA-256 {digest}, expected {SHA256[path.name]}")

  path.chmod(0o755)
  return path


def appimage(cfg: Config, binary: Path, arch: str, out_dir: Path) -> Path:
  """`<stem>-linux-<arch>.AppImage`: an AppDir of the binary, AppRun, the .desktop
  entry and the icon, packed by appimagetool with the static type2 runtime (no libfuse2), both
  pinned and fetched into <target dir>/appimage. nimbo links only glibc and loads the rest."""
  # The tool runs here; the runtime is the AppImage's own, of its arch.
  tools = cfg.target / "appimage"
  tool = fetch(APPIMAGETOOL.format(arch=here()), tools / f"appimagetool-{here()}")
  runtime = fetch(RUNTIME.format(arch=arch), tools / f"runtime-{arch}")
  out_dir.mkdir(parents=True, exist_ok=True)
  out = out_dir / f"{stem(cfg)}-linux-{arch}.AppImage"
  with tempfile.TemporaryDirectory() as tmp:
    appdir = Path(tmp) / f"{cfg.name}.AppDir"
    (appdir / "usr/bin").mkdir(parents=True)
    executable(binary, appdir / "usr/bin" / cfg.bin)
    (appdir / "AppRun").symlink_to(f"usr/bin/{cfg.bin}")
    (appdir / f"{cfg.id}.desktop").write_text(desktop(cfg))
    shutil.copy2(cfg.icon, appdir / f"{cfg.id}.png")
    (appdir / ".DirIcon").symlink_to(f"{cfg.id}.png")
    # Runs without FUSE (CI has none): the tool unpacks itself.
    env = {**os.environ, "APPIMAGE_EXTRACT_AND_RUN": "1", "ARCH": arch}
    run(tool, "--no-appstream", "--runtime-file", runtime, appdir, out, env=env)

  return out


def put(path: Path, data: bytes) -> None:
  path.parent.mkdir(parents=True, exist_ok=True)
  path.write_bytes(data)


def executable(binary: Path, path: Path) -> None:
  """`binary` copied to `path` at 0755, whatever its own mode: GitHub's artifacts lose it.

  >>> src = Path(tempfile.mkdtemp()) / "a"
  >>> _ = src.write_bytes(b"x")
  >>> src.chmod(0o644)
  >>> executable(src, src.with_name("b"))
  >>> oct(src.with_name("b").stat().st_mode & 0o777)
  '0o755'
  """
  shutil.copy2(binary, path)
  path.chmod(0o755)


def deb(cfg: Config, binary: Path, arch: str, out_dir: Path) -> Path:
  """`<stem>-linux-<arch>.deb` (apt installs it by any name) by dpkg-deb: /usr/bin/<bin>, the
  .desktop entry, the icon in hicolor's 512x512 (hicolor has no 1024 folder; GTK scales it) and
  pixmaps, the LICENSE as the copyright. `linux.deb-package` names it (else the binary),
  `linux.deb-depends` and `deb-recommends` add to its relations."""
  author = next(iter(cfg.authors), "")
  if not re.search(r"<.+@.+>", author):
    sys.exit("pack: deb needs `authors` in Cargo.toml (its Maintainer)")

  pkg = cfg.linux.get("deb-package") or cfg.bin.lower().replace("_", "-")
  version = deb_version(cfg.version)
  out_dir.mkdir(parents=True, exist_ok=True)
  out = out_dir / f"{stem(cfg)}-linux-{arch}.deb"
  with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp) / pkg
    share = root / "usr/share"
    put(root / "usr/bin" / cfg.bin, binary.read_bytes())
    put(share / f"applications/{cfg.id}.desktop", desktop(cfg).encode())
    for dir in ("icons/hicolor/512x512/apps", "pixmaps"):
      put(share / dir / f"{cfg.id}.png", cfg.icon.read_bytes())

    license = cfg.root / "LICENSE"
    if license.is_file():
      put(share / f"doc/{pkg}/copyright", license.read_bytes())

    # dpkg-deb takes the modes as they are: 0755 for the directories and the binary.
    size = 0
    for dir, _, files in os.walk(root):
      os.chmod(dir, 0o755)
      for name in files:
        path = Path(dir) / name
        path.chmod(0o755 if path == root / "usr/bin" / cfg.bin else 0o644)
        size += path.stat().st_size

    control = {
      "Package": pkg,
      "Version": version,
      "Architecture": DEB_ARCH[arch],
      "Maintainer": author,
      "Installed-Size": str(size // 1024 + 1),
      "Depends": ", ".join(cfg.linux.get("deb-depends", [])),
      # What nimbo loads at run time (GTK and the text stack), then the app's own.
      "Recommends": ", ".join(dict.fromkeys(NIMBO + cfg.linux.get("deb-recommends", []))),
      "Section": "utils",
      "Priority": "optional",
      "Description": cfg.comment or cfg.name,
    }
    (root / "DEBIAN").mkdir(mode=0o755)
    (root / "DEBIAN/control").write_text("".join(f"{k}: {v}\n" for k, v in control.items() if v))
    # xz, not Ubuntu's default zstd, which older dpkg can't read.
    run("dpkg-deb", "-Zxz", "--root-owner-group", "--build", root, out)

  return out


# MARK: Feed

PLATFORMS = {f"{o}-{a}" for o in ("macos", "windows", "linux") for a in ("x86_64", "aarch64")}
# An Ed25519 private key's PKCS#8 DER (RFC 8410) before its 32-byte seed.
SEED_DER = bytes.fromhex("302e020100300506032b657004220420")
# Homebrew's OpenSSL 3, on Apple silicon and on Intel, when the PATH's is LibreSSL.
OPENSSL = (
  "openssl",
  "/opt/homebrew/opt/openssl@3/bin/openssl",
  "/usr/local/opt/openssl@3/bin/openssl",
)


def platform(name: str) -> str | None:
  """A file's feed platform by its name, None when the name doesn't say.

  >>> platform("Wavo-v1.2.0-linux-aarch64.AppImage"), platform("Wavo-v1.2.0-windows-x64-setup.exe")
  ('linux-aarch64', 'windows-x86_64')
  >>> platform("Wavo-v1.2.0-macos-aarch64.dmg"), platform("Wavo-v1.2.0-macos.zip")
  ('macos-aarch64', None)
  >>> platform("Wavo-dev-macos-aarch64.dmg"), platform("Wavo-dev-windows-arm64-setup.exe")
  ('macos-aarch64', 'windows-aarch64')
  >>> platform("Wavo-dev-linux-x86_64.AppImage"), platform("Wavo-dev-linux-aarch64.deb")
  ('linux-x86_64', 'linux-aarch64')
  >>> platform("Wavo-linux-x86_64-macos-aarch64.dmg")
  'macos-aarch64'
  """
  # The last: a label may look like one.
  found = re.findall(r"-(macos|windows|linux)-(x86_64|aarch64|x64|arm64)(?=[-.]|$)", name)
  return f"{found[-1][0]}-{arch(found[-1][1].replace('x64', 'x86_64'))}" if found else None


def body(version: str, packages: list[tuple[str, str, bytes]]) -> bytes:
  """The feed before its signature, of (platform, url, file's bytes).

  >>> body("1.2", [("linux-x86_64", "https://a/b", b"abc")]).decode().split()[:4]
  ['version', '1.2', 'linux-x86_64', '3']
  """
  lines = [f"version {version}"]
  for p, url, data in packages:
    lines.append(f"{p} {len(data)} {hashlib.sha512(data).hexdigest()} {url}")

  return "".join(f"{line}\n" for line in lines).encode()


@functools.cache
def openssl3() -> str:
  """The first OpenSSL 3 or newer of OPENSSL; exits naming how to install it."""
  for tool in filter(None, map(shutil.which, OPENSSL)):
    out = subprocess.run([tool, "version"], capture_output=True, text=True).stdout
    m = re.match(r"OpenSSL (\d+)\.", out)
    if m and int(m[1]) >= 3:
      return tool

  sys.exit(
    "pack: Ed25519 needs OpenSSL 3, and macOS's openssl is LibreSSL: `brew install openssl@3`"
    f" puts it at {OPENSSL[1]} ({OPENSSL[2]} on Intel)"
  )


def openssl(*args, input: bytes | None = None) -> bytes:
  return run(openssl3(), *args, input=input, capture_output=True).stdout


def pem(key: str) -> bytes:
  """The private key as openssl reads it: a PEM as it is, a hex seed wrapped in PKCS#8.

  >>> pem("00" * 32).splitlines()[0]
  b'-----BEGIN PRIVATE KEY-----'
  """
  key = key.strip()
  if not re.fullmatch(r"[0-9a-fA-F]{64}", key):
    return key.encode() + b"\n"

  b64 = base64.b64encode(SEED_DER + bytes.fromhex(key)).decode()
  return f"-----BEGIN PRIVATE KEY-----\n{b64}\n-----END PRIVATE KEY-----\n".encode()


def public_key(key: bytes) -> str:
  # The DER of an Ed25519 public key ends in its 32 bytes.
  return openssl("pkey", "-pubout", "-outform", "DER", input=key)[-32:].hex()


def feed(key: bytes, version: str, url: str, files: list[str], out: Path) -> Path:
  base = url.replace("{version}", version).rstrip("/")
  packages = []
  for arg in files:
    p, _, path = arg.partition("=")
    if p not in PLATFORMS:
      p, path = platform(Path(arg).name), arg

    if p is None:
      sys.exit(f"pack: {arg}: its name doesn't say its platform, give it: <platform>=<file>")

    file = Path(path)
    packages.append((p, f"{base}/{file.name}", file.read_bytes()))

  if len({p for p, _, _ in packages}) < len(packages):
    sys.exit("pack: a platform is given twice")

  if any(c.isspace() for c in version + "".join(u for _, u, _ in packages)):
    sys.exit("pack: the version and the urls hold no whitespace")

  out.mkdir(parents=True, exist_ok=True)
  # Signed beside it, then in place: a failed signature leaves the feed that was there.
  path, part = out / "update.txt", out / "update.txt.part"
  part.write_bytes(body(version, packages))
  # Ed25519 signs only a file: the feed is one, the key comes on stdin.
  sig = openssl("pkeyutl", "-sign", "-inkey", "/dev/stdin", "-rawin", "-in", part, input=key)
  with part.open("ab") as f:
    f.write(f"sig {sig.hex()}\n".encode())

  part.replace(path)

  print(f"{path}: {version} for {', '.join(p for p, _, _ in packages)}, signed")
  return path


def feed_key(repo: str | None) -> None:
  # Never a second key: every app shipped trusts the first one's public half only.
  where = ["--repo", repo] if repo else []
  names = run("gh", "secret", "list", *where, capture_output=True, text=True).stdout.split()
  if "UPDATE_KEY" in names:
    sys.exit("pack: UPDATE_KEY is set already; the apps shipped trust it: delete it by hand first")

  if not shutil.which("pbcopy"):
    sys.exit("pack: feed-key runs on a Mac: pbcopy hands the private key over")

  key = openssl("genpkey", "-algorithm", "ed25519")
  seed = openssl("pkey", "-outform", "DER", input=key)[-32:].hex()
  # The clipboard first: a key only in the secret could never be read back.
  run("pbcopy", input=seed.encode())
  run("gh", "secret", "set", "UPDATE_KEY", *where, input=seed.encode())
  print(
    "pack: the private key is UPDATE_KEY and on the clipboard: keep it in Bitwarden",
    file=sys.stderr,
  )
  print(public_key(pem(seed)))


# MARK: Release
# The release files of the build jobs' bare binaries, made on one job, which holds the secrets.

OSES = ("apple-darwin", "pc-windows-msvc", "unknown-linux-gnu")
TRIPLES = [f"{a}-{o}" for o in OSES for a in NAMES]
LABEL = r"dev|v\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?"


def binaries(cfg: Config, dir: Path) -> list[tuple[str, Path]]:
  """The binaries in `dir`, each `<triple>/<bin>[.exe]`, in TRIPLES' order. Anything else there
  exits, as does a binary of another OS or arch: the build jobs ran dependencies' build scripts,
  and nothing but what they were to leave gets packed and signed."""
  if not dir.is_dir():
    sys.exit(f"pack: no folder {dir}")

  found, bad = [], []
  for entry in sorted(dir.iterdir()):
    exe = cfg.bin + (".exe" if "-windows-" in entry.name else "")
    kids = sorted(entry.iterdir()) if entry.is_dir() and not entry.is_symlink() else []
    names = [k.name for k in kids]
    if entry.name not in TRIPLES or names != [exe] or kids[0].is_symlink() or not kids[0].is_file():
      bad.append(entry.name + (f" ({', '.join(names)})" if names else ""))
      continue

    kind = binary_kind(kids[0])
    if kind != (entry.name.split("-", 1)[1], arch(entry.name)):
      built = f"built for {kind[1]}-{kind[0]}" if kind else "no binary pack reads"
      bad.append(f"{entry.name}/{exe}, {built}")
      continue

    found.append((entry.name, kids[0]))

  if bad or not found:
    sys.exit(
      f"pack: {dir} holds only <x86_64|aarch64>-<{'|'.join(OSES)}>/{cfg.bin}[.exe]: refused "
      + ("; ".join(bad) if bad else "an empty folder")
    )

  return sorted(found, key=lambda f: TRIPLES.index(f[0]))


def binary_kind(path: Path) -> tuple[str, str] | None:
  """The OS and arch a binary is for by its format, as a triple names them: Mach-O (one slice), PE
  or ELF.

  >>> elf = Path(tempfile.mkdtemp()) / "a"
  >>> _ = elf.write_bytes(b"\\x7fELF" + bytes(14) + struct.pack("<H", 0xB7))
  >>> binary_kind(elf)
  ('unknown-linux-gnu', 'aarch64')
  """
  data = path.read_bytes()
  try:
    if data[:4] == b"\x7fELF":
      machine = {0x3E: "x86_64", 0xB7: "aarch64"}.get(struct.unpack_from("<H", data, 18)[0])
      return ("unknown-linux-gnu", machine) if machine else None

    if data[:2] == b"MZ":
      opt = pe_headers(data)[0]
      machine = struct.unpack_from("<H", data, opt - 20)[0]
      return "pc-windows-msvc", "x86_64" if machine == 0x8664 else "aarch64"

    (cpu, _), *more = macho(data)
    cpu = {"arm64": "aarch64", "x86_64": "x86_64"}.get(cpu)
    return ("apple-darwin", cpu) if cpu and not more else None
  except (ValueError, struct.error):
    return None


def release(cfg: Config, dir: Path, tag: str | None) -> None:
  """Per triple in `dir` its files at `dir`'s top level: the app's DMG, the setup, or the AppImage
  and .deb; then, with `tag`, update.txt, signed by UPDATE_KEY, its urls the GitHub release's."""
  if cfg.label.startswith("v") and cfg.label != f"v{cfg.version}":
    sys.exit(f"pack: the label {cfg.label} is not Cargo.toml's version, {cfg.version}")

  key = os.environ.get("UPDATE_KEY")
  if tag and not key:
    sys.exit("pack: --tag writes update.txt, signed: set UPDATE_KEY")

  if tag and not cfg.repository.startswith("https://github.com/"):
    sys.exit("pack: --tag: the feed's urls are the GitHub release's: `repository` in Cargo.toml")

  files = []
  for triple, binary in binaries(cfg, dir):
    a = arch(triple)
    if "darwin" in triple:
      files.append(dmg(cfg, app(cfg, binary), triple, dir))
      continue

    if "windows" in triple:
      made = [setup(cfg, binary, a, dir)]
    else:
      made = [appimage(cfg, binary, a, dir), deb(cfg, binary, a, dir)]

    print(*made, sep="\n")
    files += made

  if tag:
    # The updater takes the AppImage on Linux; a .deb is apt's.
    url = f"{cfg.repository}/releases/download/{tag}"
    feed(pem(key), cfg.version, url, [str(f) for f in files if f.suffix != ".deb"], dir)


# MARK: Command line


def main():
  ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
  ap.add_argument("command", choices=("dist", *OS, "release", "feed", "feed-key"))
  ap.add_argument("files", nargs="*", help="dmg: the .app; release: the folder; feed: the files")
  ap.add_argument("--manifest", type=Path, default=Path("Cargo.toml"), help="the app's Cargo.toml")
  ap.add_argument("--target", help="the target triple, by default this machine's arch")
  ap.add_argument("--bin", type=Path, help="the binary, else <target dir>/<triple>/dist/<bin>")
  ap.add_argument("--app", type=Path, help="dmg: the .app, by default <target dir>/<Name>.app")
  ap.add_argument("--label", help="the files' <Name>-<label>-…, by default <Name>-v<version>-…")
  ap.add_argument("--out", type=Path, help="where the files go, by default <target dir>/distrib")
  ap.add_argument("--key", type=Path, help="feed: the private key's file, else $UPDATE_KEY")
  ap.add_argument("--url", help="feed: where the files will be served, {version} the version")
  ap.add_argument("--tag", help="release: the GitHub release's tag, for update.txt's urls")
  ap.add_argument("--public-key", action="store_true", help="feed: print the key's public half")
  ap.add_argument("--repo", help="feed-key: the secret's GitHub repo, by default the current one")
  args = ap.parse_intermixed_args()
  if args.command == "feed-key":
    feed_key(args.repo)
    return

  if args.command == "feed":
    key = args.key.read_text() if args.key else os.environ.get("UPDATE_KEY")
    if not key:
      ap.error("feed needs the key: --key <file> or $UPDATE_KEY")

    if args.public_key:
      print(public_key(pem(key)))
      return

    if not args.url or not args.files:
      ap.error("feed needs --url and the files")

  if args.label and not re.fullmatch(r"[A-Za-z0-9.-]+", args.label):
    ap.error("--label is letters, digits, dots and hyphens")

  if args.command == "release":
    if len(args.files) != 1 or args.target or args.bin or args.app or args.out:
      ap.error("release takes the folder of the binaries, and no --target, --bin, --app or --out")

    if not re.fullmatch(LABEL, args.label or ""):
      ap.error("release needs --label dev or v<version>")

    if args.tag and not re.fullmatch(r"[A-Za-z0-9._-]+", args.tag):
      ap.error("--tag is letters, digits, dots, hyphens and underscores")

  elif args.tag:
    ap.error("--tag is release's")

  cfg = load(args.manifest)
  cfg.label = args.label
  out = args.out or cfg.target / "distrib"
  if args.command == "release":
    release(cfg, Path(args.files[0]), args.tag)
    return

  if args.command == "feed":
    feed(pem(key), cfg.version, args.url, args.files, out)
    return

  if args.command == "dmg":
    bundles = [*args.files, *([args.app] if args.app else [])]
    if len(bundles) > 1:
      ap.error("dmg takes one .app")

    dmg(cfg, Path(bundles[0]) if bundles else cfg.target / f"{cfg.name}.app", args.target, out)
    return

  if args.files or args.app:
    ap.error(f"{args.command} takes no .app or files")

  # The .app goes beside the files, never among them.
  if args.command == "app" and args.out:
    ap.error("app writes <target dir>/<Name>.app: --out is the files'")

  steps = DIST.get(sys.platform) if args.command == "dist" else (args.command,)
  if not steps:
    sys.exit(f"pack: dist runs on macOS, Windows and Linux, not {sys.platform}")

  target = args.target or f"{here()}-{OS[steps[0]]}"
  exe = cfg.bin + (".exe" if "windows" in target else "")
  binary = args.bin or cfg.target / target / "dist" / exe
  if not binary.is_file():
    sys.exit(f"pack: no binary at {binary}: build it, or give --bin")

  for step in steps:
    if step == "app":
      bundle = app(cfg, binary)
    elif step == "dmg":
      dmg(cfg, bundle, args.target, out)
    else:
      make = {"setup": setup, "appimage": appimage, "deb": deb}[step]
      print(make(cfg, binary, arch(target), out))


if __name__ == "__main__":
  main()
