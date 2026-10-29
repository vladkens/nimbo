# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
r"""Makes an app's release files from its Cargo.toml: the macOS app and DMG, the Windows setup,
the AppImage and .deb, and the signed update feed (src/update.rs).

The config is the package's `[package.metadata.nimbo]` (`id`, `name`, `icon`, a 1024 px PNG,
`comment`, the tables `macos`, `linux` and `windows`), its version, binary and `repository`, read
by `cargo metadata --no-deps`: a plain `python3` (3.9+) with the standard library runs this.

  [package.metadata.nimbo]
  id = "me.vladkens.wavo"
  name = "Wavo"
  icon = "assets/icon.png"
  comment = "Local dictation"
  [package.metadata.nimbo.macos]
  accessory = true                 # a tray app: no Dock icon, never ended by macOS on its own
  entitlements = "assets/wavo.entitlements"
  plist.NSMicrophoneUsageDescription = "…"
  [package.metadata.nimbo.windows]
  app-id = "314a56a8-8a8b-4c18-954d-b930203f086d"
  copyright = "© 2026 vladkens"     # the exe's LegalCopyright, by default "© <publisher>"
  [package.metadata.nimbo.linux]
  categories = ["Utility"]
  deb-package = "wavo"
  deb-depends = ["libasound2 | libasound2t64"]

`release <dir>` packs the build jobs' binaries, `<dir>/<triple>/<bin>[.exe]`, into
`<dir>/<Name>-dev-<platform>.<ext>`: the DMG, the setup, or the AppImage and .deb, rust-build's
names, which rust-publish renames to `<Name>-<tag>-…` on a release. `--tag` is a release: then
update.txt (that name kept), signed by UPDATE_KEY, lists the files by their renamed names, at
`<repository>/releases/download/<tag>/<Name>-<tag>-…`, the same bytes.
In CI it runs on Ubuntu (`apt-get install nsis hfsprogs`), from the app's Makefile: this file
alone, at the nimbo commit Cargo.lock pins, so no crate's source reaches the job with the secrets.

  NIMBO_REV = $(shell sed -n 's|^source = "git+https://github.com/vladkens/nimbo?.*#\([0-9a-f]\{40\}\)"$$|\1|p' Cargo.lock)
  release:
  	curl -fsSLO https://raw.githubusercontent.com/vladkens/nimbo/$(NIMBO_REV)/scripts/pack.py
  	python3 pack.py release target/distrib $(if $(TAG),--tag $(TAG))

Off macOS rcodesign signs the app with APPLE_CERTIFICATE (a base64 .p12) and
APPLE_CERTIFICATE_PASSWORD, else ad hoc; the DMG is made by `mkfs.hfsplus` and libdmg-hfsplus
(from $NIMBO_TOOLS, else fetched), then signed and, with APPLE_API_KEY (the .p8's text),
APPLE_API_KEY_ID and APPLE_API_ISSUER, notarized. A release (`--tag`) fails on a missing secret.
A dev build's .deb is `<version>~dev`, which apt orders before the release. `app --bin <binary>`
writes `<target dir>/<Name>.app` alone, on a Mac signed by codesign.

`feed` writes `<target dir>/distrib/update.txt`: `version <v>`, a line per file, `<platform> <size>
<sha512> <url>`, and last `sig <hex>`, the Ed25519 signature of every byte before it. A file's
platform is in its name (`linux-aarch64`, `windows-x64`) or given, `<platform>=<file>`; its url is
`--url` (`{version}` the version) and its name. The key, `--key <file>` or `$UPDATE_KEY`, is its
seed in hex or a PEM. `feed-key` makes the one key, in memory only: the repo's secret UPDATE_KEY
(gh), the private key on the clipboard to keep (Bitwarden), the public key printed in hex, the app's
`Feed::key`. Both need OpenSSL 3 (macOS's own openssl is LibreSSL).

python3 scripts/pack.py release <dir> [--tag vX.Y.Z] [--manifest <Cargo.toml>]
python3 scripts/pack.py app --bin <binary> [--manifest <Cargo.toml>]
python3 scripts/pack.py feed [--key <file>] --url <base> [<platform>=]<file>... | --public-key
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
  tag: str | None = None
  repository: str = ""


def stem(cfg: Config, tag: str = "dev") -> str:
  """A file's name before its platform: rust-build's `<Name>-dev`, `<Name>-<tag>` once renamed.

  >>> from types import SimpleNamespace as N
  >>> stem(N(name="Wavo")), stem(N(name="Wavo"), "v1.2.0")
  ('Wavo-dev', 'Wavo-v1.2.0')
  """
  return f"{cfg.name}-{tag}"


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
  """A PNG's `(width, height, RGBA)`, straight alpha, as src/png.rs reads them: 8-bit RGB, RGBA or
  a palette with its tRNS, never interlaced.

  >>> px = bytes(range(32))
  >>> png_decode(png_encode(4, 2, px)) == (4, 2, px)
  True
  >>> def png(w, h, color, rows, *more):
  ...   head = (b"IHDR", struct.pack(">IIBBBBB", w, h, 8, color, 0, 0, 0))
  ...   parts = (head, *more, (b"IDAT", zlib.compress(rows)), (b"IEND", b""))
  ...   return png_decode(PNG + b"".join(chunk(*c) for c in parts))
  >>> png(1, 2, 2, bytes([4, 7, 8, 9, 2, 1, 1, 1]))  # RGB, Paeth then Up
  (1, 2, b'\\x07\\x08\\t\\xff\\x08\\t\\n\\xff')
  >>> png(2, 1, 3, bytes([1, 0, 1]), (b"PLTE", b"abcdef"), (b"tRNS", b"\\0"))  # a palette, Sub
  (2, 1, b'abc\\x00def\\xff')
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
  bpp = {2: 3, 3: 1, 6: 4}.get(color)
  if not bpp or depth != 8 or interlace:
    raise ValueError(
      f"a PNG of colour type {color} at {depth} bits, interlaced {interlace}: pack reads 8-bit RGB,"
      " RGBA or a palette, not interlaced"
    )

  stride = w * bpp + 1
  raw = zlib.decompress(b"".join(idat))
  if len(raw) < stride * h:
    raise ValueError("its pixels are cut short")

  prev, rows = bytes(stride - 1), []
  for y in range(h):
    prev = unfilter(raw[y * stride], raw[y * stride + 1 : (y + 1) * stride], prev, bpp)
    rows.append(prev)

  px = b"".join(rows)
  if color == 6:
    return w, h, px

  if color == 3:
    plte, trns = chunks[b"PLTE"], chunks.get(b"tRNS", b"")
    planes = [px.translate(bytes(plte[c::3]).ljust(256, b"\0")) for c in range(3)]
    planes.append(px.translate(trns.ljust(256, b"\xff")))
  else:
    planes = [px[c::3] for c in range(3)] + [b"\xff" * (w * h)]

  out = bytearray(w * h * 4)
  for c, plane in enumerate(planes):
    out[c::4] = plane

  return w, h, bytes(out)


def unfilter(kind: int, line: bytes, prev: bytes, bpp: int) -> bytes:
  """One row of a PNG as it was before its filter, `prev` the row above, as it was."""
  if kind == 0:
    return line

  if kind == 2:
    return bytewise(line, prev, True)

  if kind == 1:
    # Each channel's running sum, mod 256.
    out = bytearray(line)
    for c in range(bpp):
      out[c::bpp] = bytes(map((255).__and__, accumulate(line[c::bpp])))

    return bytes(out)

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


def icns(images: dict[int, bytes], file: bytes = b"") -> bytes:
  """The .icns of the icon's RGBA at ICNS's sizes, as iconutil writes it: ic04 and ic05 'ARGB',
  each channel in literal runs of the icns RLE (a byte, the run's length less one), the rest PNG,
  ic10 the icon's own `file` when it is 1024 px.

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
      data = file if n == 1024 and file else png_encode(n, n, px)

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


def info(cfg: Config, minimum: str) -> dict:
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
    "LSMinimumSystemVersion": minimum,
    "NSHighResolutionCapable": True,
  }
  if cfg.macos.get("accessory"):
    plist |= {
      "LSUIElement": True,
      "NSSupportsAutomaticTermination": False,
      "NSSupportsSuddenTermination": False,
    }

  return {**plist, **cfg.macos.get("plist", {})}


def minos(data: bytes) -> str:
  """The macOS a thin Mach-O binary needs: its LC_BUILD_VERSION's minos (xnu's loader.h).

  >>> cmd = struct.pack("<6I", 0x32, 24, 1, 0x0E0000, 0, 0)
  >>> minos(struct.pack("<8I", 0xFEEDFACF, 0x0100000C, 0, 2, 1, len(cmd), 0, 0) + cmd)
  '14.0'
  """
  if data[:4] != b"\xcf\xfa\xed\xfe":
    raise ValueError("not a thin 64-bit Mach-O binary")

  at = 32
  for _ in range(struct.unpack_from("<I", data, 16)[0]):
    cmd, size, platform, version = struct.unpack_from("<4I", data, at)
    # PLATFORM_MACOS.
    if cmd == 0x32 and platform == 1:
      parts = [version >> 16, version >> 8 & 255] + ([version & 255] if version & 255 else [])
      return ".".join(map(str, parts))

    at += size

  raise ValueError("it has no LC_BUILD_VERSION for macOS")


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


def developer() -> tuple[str, str] | None:
  """A Developer ID in the keychain, as codesign takes it (its hash: two, a renewal, are ambiguous
  by name) and as it is said."""
  dev = [ln for ln in identities(True) if '"Developer ID Application:' in ln]
  return (dev[0].split()[1], dev[0].split('"')[1]) if dev else None


def identity(cfg: Config) -> tuple[str, str]:
  """What signs the app on a Mac, as `developer` says it: a Developer ID, else "<Name> Local
  Signing", made once in the login keychain: its designated requirement names the bundle id and
  the certificate, not the code hash, so the permissions macOS grants survive rebuilds."""
  found = developer()
  if found:
    return found

  name = f"{cfg.name} Local Signing"
  if not any(f'"{name}"' in ln for ln in identities(False)):
    make_identity(name)

  return name, name


def app(cfg: Config, binary: Path) -> Path:
  """`<target dir>/<Name>.app`: the binary, the icns and the Info.plist, signed by
  `identity` with the hardened runtime and `macos.entitlements`. One codesign fails on (or a
  keychain dialog nobody answers stops) is no bundle, never one signed ad hoc, whose grants would
  last that build only. Off macOS rcodesign signs it, with APPLE_CERTIFICATE for notarization."""
  try:
    needs = minos(binary.read_bytes())
  except (OSError, ValueError, struct.error) as e:
    sys.exit(f"pack: {binary}: {e}")

  sha, name = identity(cfg) if MAC else ("", "Developer ID" if secrets(cfg, CERT) else "ad hoc")
  bundle = cfg.target / f"{cfg.name}.app"
  shutil.rmtree(bundle, ignore_errors=True)
  (bundle / "Contents/MacOS").mkdir(parents=True)
  (bundle / "Contents/Resources").mkdir(parents=True)
  executable(binary, bundle / "Contents/MacOS" / cfg.bin)
  images = icons(cfg.icon, {n for _, n in ICNS}, False)
  file = cfg.icon.read_bytes() if picture(cfg.icon)[:2] == (1024, 1024) else b""
  (bundle / f"Contents/Resources/{cfg.name}.icns").write_bytes(icns(images, file))
  (bundle / "Contents/PkgInfo").write_text("APPL????")
  (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps(info(cfg, needs)))
  entitlements = cfg.macos.get("entitlements")
  if MAC:
    run("xattr", "-cr", bundle)
    sign = ["--force", "--sign", sha, "--options", "runtime", "--timestamp=none"]
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


def dmg(cfg: Config, bundle: Path, arch: str, out_dir: Path) -> Path:
  """`<stem>-macos-<arch>.dmg` of `bundle`, as it is signed, and an /Applications link: on a Mac
  UDZO by hdiutil, signed by a Developer ID if the keychain has one; off macOS `hfs_dmg`'s."""
  out = out_dir / f"{stem(cfg)}-macos-{arch}.dmg"
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

  found = developer()
  if found:
    run("codesign", "--force", "--sign", found[0], "--timestamp=none", out, timeout=60)

  print(f"{out}: {cfg.name} {cfg.version}, signed: {found[1] if found else 'no'}")
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
# libdmg-hfsplus's `dmg` and `hfsplus` (mozilla/libdmg-hfsplus e18579b39195, static x86_64 Linux),
# as vladkens/actions' tools.yml released them, by the name, pinned in SHA256.
LIBDMG = "https://github.com/vladkens/actions/releases/download/tools-20261029/{name}"


def secrets(cfg: Config, names: tuple[str, ...]) -> bool:
  """Whether every one of `names` is set; a release (a tag) without one fails."""
  missing = [n for n in names if not os.environ.get(n)]
  if missing and cfg.tag:
    sys.exit(f"pack: {cfg.tag} is a release, signed and notarized: set {', '.join(missing)}")

  return not missing


def tool(cfg: Config, name: str) -> Path:
  """rcodesign, fetched for this host into <target dir>/tools, or libdmg-hfsplus's `dmg` and
  `hfsplus`, from $NIMBO_TOOLS when it is set, else fetched there too on x86_64 Linux; all pinned
  in SHA256."""
  where = os.environ.get("NIMBO_TOOLS")
  if name != "rcodesign" and where:
    path = Path(where) / name
    if not path.is_file():
      sys.exit(f"pack: no {name} in $NIMBO_TOOLS ({where})")

    return path

  tools = cfg.target / "tools"
  if name != "rcodesign":
    if (here(), sys.platform) != ("x86_64", "linux"):
      sys.exit(f"pack: {name} is pinned for x86_64 Linux only: set $NIMBO_TOOLS")

    return fetch(LIBDMG.format(name=name), tools / name)

  triple = here() + ("-apple-darwin" if sys.platform == "darwin" else "-unknown-linux-musl")
  url = RCODESIGN.format(triple=triple)
  tarball = tools / url.rsplit("/", 1)[1]
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

    # Settings › Apps' size: the installed files', not the icon's.
    kib = -(-sum(f.stat().st_size for f in tmp.iterdir() if f.suffix != ".ico") // 1024)
    (tmp / "setup.nsi").write_text(nsi(cfg, arch, license.is_file(), out.name, kib), "utf-8")
    run(makensis, "-V2", "-WX", "-INPUTCHARSET", "UTF8", tmp / "setup.nsi")
    # A signature later: here, the setup's, once makensis has written it.
    shutil.move(str(tmp / out.name), out)

  return out


def nsis(s: str) -> str:
  """`s` in an NSIS string: `$` doubled (a define's `${` too), a quote `$\\"`, no line breaks.

  >>> print(nsis('a "$b" ${c}\\n'))
  a $\\"$$b$\\" $${c}
  """
  return " ".join(s.replace("$", "$$").replace('"', '$\\"').split("\n")).strip()


def nsi(cfg: Config, arch: str, license: bool, out: str, kib: int) -> str:
  r"""The setup's .nsi: it packs `app.exe`, `app.ico` and, with `license`, `LICENSE.txt` from its
  folder and writes the setup there as `out`, its final name: never `setup.exe`, which Windows
  loads compatibility shims into (makensis -WX fails on it, warning 9000). Its `.onInit` exits 2
  on a Windows of another arch; the stub is x86, so that runs on both. Settings › Apps shows `kib`,
  the packed files' size. The uninstaller removes the install folder whole, with what the updater
  left in it (src/update.rs stages there), and keeps the app's data."""
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
    "Unicode true",
    "ManifestDPIAware true",
    "RequestExecutionLevel user",
    "SetCompressor /SOLID lzma",
    f'Name "{name}" "{name.replace("&", "&&")}"',
    f'OutFile "{nsis(out)}"',
    f'InstallDir "$LOCALAPPDATA\\Programs\\{name}"',
    "",
    "!include MUI2.nsh",
    "!include x64.nsh",
    "",
    '!define MUI_ICON "app.ico"',
    '!define MUI_UNICON "app.ico"',
    # The finish page's "Run": a silent install (the updater's) shows no page, starts nothing.
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
    *(f"  {line}" for line in check),
    f'    MessageBox MB_ICONSTOP "This setup of {name} is for {NAMES[arch]} Windows." /SD IDOK',
    "    SetErrorLevel 2",
    "    Abort",
    "  ${EndIf}",
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
    f'  WriteRegDWORD HKCU "{key}" "EstimatedSize" {kib}',
    "SectionEnd",
    "",
    # The folder with the updater's staging, which its installer couldn't remove, the shortcut,
    # and nimbo's launch at login, which would start a deleted exe.
    'Section "Uninstall"',
    '  RMDir /r "$INSTDIR"',
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
# the job that makes the setup needs no Windows tool: a new last section the resource directory
# points at, the linker's left unused. The PE/COFF layout is Microsoft's "PE Format"
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
</assembly>
"""


def exe_resources(exe: Path, out: Path, cfg: Config, pngs: dict[int, bytes]) -> Path:
  """`exe` as `out` with the app's resources, in place of the linker's: its icon (`pngs`, the
  tile at ICO's sizes), the version and the names Explorer and Task Manager show, and the manifest
  (asInvoker, Windows 10 and 11, Common Controls 6; nimbo sets PerMonitorV2 itself)."""
  ids = [struct.pack("<H", i) for i in range(1, len(pngs) + 1)]
  tree = {
    RT_ICON: dict(enumerate(pngs.values(), 1)),
    RT_GROUP_ICON: {1: icon_dir(pngs, ids)},
    RT_VERSION: {1: version_info(cfg)},
    RT_MANIFEST: {1: MANIFEST.encode()},
  }
  try:
    out.write_bytes(pe_resources(exe.read_bytes(), tree))
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


def rsrc(tree: dict, rva: int) -> bytes:
  """The .rsrc at `rva` of `{type: {id: data}}`, each in LANG: the root, a directory per type, one
  per id (its language), the data entries, then the data, each at 8 bytes.

  >>> data = rsrc({RT_MANIFEST: {1: b"<m/>"}, RT_ICON: {1: b"a", 2: b"bc"}}, 0x5000)
  >>> def walk(at):
  ...   kids = [struct.unpack_from("<II", data, at + 16 + 8 * j) for j in range(data[at + 14])]
  ...   return {k: walk(to & 0x7FFFFFFF) if to >> 31 else leaf(to) for k, to in kids}
  >>> def leaf(at):
  ...   rva, size = struct.unpack_from("<II", data, at)
  ...   return data[rva - 0x5000 :][:size]
  >>> walk(0)
  {3: {1: {1033: b'a'}, 2: {1033: b'bc'}}, 24: {1: {1033: b'<m/>'}}}
  """
  types = sorted(tree.items())
  leaves = [data for _, ids in types for _, data in sorted(ids.items())]
  langs = 16 + 8 * len(types) + sum(16 + 8 * len(ids) for _, ids in types)
  entries = langs + 24 * len(leaves)
  at = [align(entries + 16 * len(leaves), 8)]
  for data in leaves:
    at.append(align(at[-1] + len(data), 8))

  out = bytearray(at[-1])
  struct.pack_into("<14xH", out, 0, len(types))
  sub, n = 16 + 8 * len(types), 0
  for i, (kind, ids) in enumerate(types):
    struct.pack_into("<II", out, 16 + 8 * i, kind, sub | 1 << 31)
    struct.pack_into("<14xH", out, sub, len(ids))
    for j, name in enumerate(sorted(ids)):
      lang = langs + 24 * n
      struct.pack_into("<II", out, sub + 16 + 8 * j, name, lang | 1 << 31)
      struct.pack_into("<14xHII", out, lang, 1, LANG, entries + 16 * n)
      struct.pack_into("<II", out, entries + 16 * n, rva + at[n], len(leaves[n]))
      out[at[n] : at[n] + len(leaves[n])] = leaves[n]
      n += 1

    sub += 16 + 8 * len(ids)

  return bytes(out)


def pe_resources(data: bytes, tree: dict) -> bytes:
  """`data`, a PE32+ exe, with a new last section, .rsrc, of `tree`'s resources, the resource
  directory pointing at it; the linker's stay in the file, unused. Its CheckSum is 0, which
  Windows checks only in drivers.

  >>> pe = bytearray(0x400)
  >>> pe[:2], pe[0x3C] = b"MZ", 0x40
  >>> struct.pack_into("<4sHH12xHH", pe, 0x40, b"PE\\0\\0", 0x8664, 1, 240, 0x22)
  >>> struct.pack_into("<H30xII16xII44xI", pe, 0x58, 0x20B, 0x1000, 0x200, 0x2000, 0x200, 16)
  >>> SECTION.pack_into(pe, 0x148, b".text", 16, 0x1000, 0x200, 0x200, 0, 0, 0, 0, 0x60000020)
  >>> out = pe_resources(bytes(pe), {RT_MANIFEST: {1: b"<m/>"}})
  >>> name, size, va, _, at = SECTION.unpack_from(out, 0x170)[:5]
  >>> out[0x46], name, struct.unpack_from("<II", out, 0x58 + 128) == (va, size), hex(va), at
  (2, b'.rsrc\\x00\\x00\\x00', True, '0x2000', 1024)
  >>> struct.unpack_from("<I", out, 0x58 + 56)[0] == 0x3000, len(out)
  (True, 1536)
  >>> struct.pack_into("<I", pe, 0x58 + 60, 0x170)
  >>> pe_resources(bytes(pe), {})
  Traceback (most recent call last):
  ValueError: its headers have no room for one more section header
  """
  pe = struct.unpack_from("<I", data, 0x3C)[0]
  if data[:2] != b"MZ" or data[pe : pe + 4] != b"PE\0\0" or data[pe + 24 : pe + 26] != b"\x0b\x02":
    raise ValueError("not a 64-bit exe")

  count, size = struct.unpack_from("<H12xH", data, pe + 6)
  opt = pe + 24
  table = opt + size + 40 * count
  (section, file), (image, headers) = (struct.unpack_from("<II", data, opt + i) for i in (32, 56))
  if table + 40 > headers or any(data[table : table + 40]):
    raise ValueError("its headers have no room for one more section header")

  va = align(image, section)
  blob = rsrc(tree, va)
  out = bytearray(data.ljust(align(len(data), file), b"\0"))
  raw = align(len(blob), file)
  SECTION.pack_into(out, table, b".rsrc", len(blob), va, raw, len(out), 0, 0, 0, 0, RSRC)
  out += blob.ljust(raw, b"\0")
  struct.pack_into("<H", out, pe + 6, count + 1)
  struct.pack_into("<I", out, opt + 56, align(va + len(blob), section))
  struct.pack_into("<I", out, opt + 64, 0)
  struct.pack_into("<II", out, opt + 128, va, len(blob))
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
# LIBDMG's, as its release's SHA256SUMS gives them.
SHA256 |= {
  "dmg": "b238e3739f101db61c533e0b7d64591195514f13cdd8b82eba41e4d1d0096b34",
  "hfsplus": "f85887ab739f7ff0ddb477e81fbde391ad296de7f31ae589ca60c9d731f23270",
}
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


def deb_version(version: str, dev: bool) -> str:
  """The Cargo version as Debian orders it: a pre-release, or a dev build, sorts before its release.

  >>> deb_version("1.2.0-beta.1", False), deb_version("1.2.0", False)
  ('1.2.0~beta.1', '1.2.0')
  >>> deb_version("1.2.0", True), deb_version("1.2.0-beta.1", True)
  ('1.2.0~dev', '1.2.0~beta.1~dev')
  """
  return version.replace("-", "~", 1) + "~dev" * dev


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
  """`binary` copied to `path` at 0755, whatever its own mode: GitHub's artifacts lose it."""
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
  version = deb_version(cfg.version, not cfg.tag)
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
  """A file's feed platform by its name, None when the name doesn't say."""
  # The last: the name or tag may look like one.
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


def feed(key: bytes, version: str, url: str, files: list[str], out: Path, name=None) -> Path:
  """`out`/update.txt of `files`, each at `<url>/<name>`, `name(file's name)` when given."""
  base = url.replace("{version}", version).rstrip("/")
  packages = []
  for arg in files:
    p, _, path = arg.partition("=")
    if p not in PLATFORMS:
      p, path = platform(Path(arg).name), arg

    if p is None:
      sys.exit(f"pack: {arg}: its name doesn't say its platform, give it: <platform>=<file>")

    file = Path(path)
    packages.append((p, f"{base}/{name(file.name) if name else file.name}", file.read_bytes()))

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


def binaries(cfg: Config, dir: Path) -> list[tuple[str, Path]]:
  """The binaries in `dir`, each `<triple>/<bin>[.exe]`, of TRIPLES."""
  found = [(t, dir / t / (cfg.bin + ".exe" * ("windows" in t))) for t in TRIPLES]
  found = [(t, path) for t, path in found if path.is_file()]
  if not found:
    sys.exit(f"pack: {dir} holds no <triple>/{cfg.bin}[.exe], of {', '.join(TRIPLES)}")

  return found


def release(cfg: Config, dir: Path) -> None:
  """Per triple in `dir` its files at `dir`'s top level, `<Name>-dev-…`: the app's DMG, the
  setup, or the AppImage and .deb; then, with `cfg.tag`, update.txt, signed by UPDATE_KEY, of the
  files as the GitHub release names and serves them, `<Name>-<tag>-…`."""
  tag = cfg.tag
  if tag and tag != f"v{cfg.version}":
    sys.exit(f"pack: the tag {tag} is not Cargo.toml's version, v{cfg.version}")

  key = os.environ.get("UPDATE_KEY")
  if tag and not key:
    sys.exit("pack: --tag writes update.txt, signed: set UPDATE_KEY")

  if tag and not cfg.repository.startswith("https://github.com/"):
    sys.exit("pack: --tag: the feed's urls are the GitHub release's: `repository` in Cargo.toml")

  files = []
  for triple, binary in binaries(cfg, dir):
    a = arch(triple)
    if "darwin" in triple:
      files.append(dmg(cfg, app(cfg, binary), a, dir))
      continue

    if "windows" in triple:
      made = [setup(cfg, binary, a, dir)]
    else:
      made = [appimage(cfg, binary, a, dir), deb(cfg, binary, a, dir)]

    print(*made, sep="\n")
    files += made

  if tag:
    # The updater takes the AppImage on Linux; a .deb is apt's. rust-publish renames
    # `<Name>-dev-…` to `<Name>-<tag>-…`, the same bytes.
    url = f"{cfg.repository}/releases/download/{tag}"
    files = [str(f) for f in files if f.suffix != ".deb"]
    renamed = stem(cfg, tag)
    feed(pem(key), cfg.version, url, files, dir, lambda n: renamed + n.removeprefix(stem(cfg)))


# MARK: Command line


def main():
  ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
  ap.add_argument("command", choices=("release", "app", "feed", "feed-key"))
  ap.add_argument("files", nargs="*", help="release: the binaries' folder; feed: the files")
  ap.add_argument("--manifest", type=Path, default=Path("Cargo.toml"), help="the app's Cargo.toml")
  ap.add_argument("--bin", type=Path, help="app: the binary")
  ap.add_argument("--tag", help="release: the GitHub release's tag, v<version>, for update.txt")
  ap.add_argument("--key", type=Path, help="feed: the private key's file, else $UPDATE_KEY")
  ap.add_argument("--url", help="feed: where the files will be served, {version} the version")
  ap.add_argument("--public-key", action="store_true", help="feed: print the key's public half")
  ap.add_argument("--repo", help="feed-key: the secret's GitHub repo, by default the current one")
  args = ap.parse_intermixed_args()
  if args.command == "feed-key":
    feed_key(args.repo)
    return

  key = args.key.read_text() if args.key else os.environ.get("UPDATE_KEY")
  if args.command == "feed":
    if not key:
      ap.error("feed needs the key: --key <file> or $UPDATE_KEY")

    if args.public_key:
      print(public_key(pem(key)))
      return

    if not args.url or not args.files:
      ap.error("feed needs --url and the files")

  if args.command == "release" and len(args.files) != 1:
    ap.error("release takes the binaries' folder")

  if args.command == "app" and not args.bin:
    ap.error("app takes --bin, the binary")

  cfg = load(args.manifest)
  cfg.tag = args.tag
  if args.command == "release":
    release(cfg, Path(args.files[0]))
  elif args.command == "feed":
    feed(pem(key), cfg.version, args.url, args.files, cfg.target / "distrib")
  else:
    app(cfg, args.bin)


if __name__ == "__main__":
  main()
